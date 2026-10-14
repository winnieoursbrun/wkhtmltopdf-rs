package bench;

import com.openhtmltopdf.outputdevice.helper.BaseRendererBuilder.FontStyle;
import com.openhtmltopdf.outputdevice.helper.BaseRendererBuilder.PageSizeUnits;
import com.openhtmltopdf.pdfboxout.PdfRendererBuilder;
import com.openhtmltopdf.util.XRLog;
import java.awt.Color;
import java.awt.Font;
import java.awt.Graphics2D;
import java.awt.RenderingHints;
import java.awt.image.BufferedImage;
import java.io.ByteArrayOutputStream;
import java.io.File;
import java.lang.management.ManagementFactory;
import java.nio.file.Files;
import java.util.ArrayList;
import java.util.Arrays;
import java.util.List;
import javax.imageio.ImageIO;
import org.jsoup.Jsoup;
import org.jsoup.helper.W3CDom;

/**
 * OpenHTMLtoPDF benchmark driver (bench/java/).
 *
 * <pre>
 * java -jar convert.jar [--warmup W] [--loop N] [--json] in.html out.pdf
 * java -jar convert.jar --montage out.png label1 a.png label2 b.png ...
 * </pre>
 *
 * Without --loop it converts once, like a CLI. With --loop N it converts max(1, W) times
 * untimed, then N timed times in the same JVM and prints one JSON object with the per-document
 * wall time (mean, median, p95, min, max, standard deviation), the first in-JVM conversion and
 * the process CPU time per timed document (all JVM threads: JIT and GC included).
 *
 * Each conversion does what an application would: parse the HTML with jsoup, convert it to a
 * W3C DOM, build a renderer with the DejaVu fonts registered, write the PDF to memory. Page
 * setup matches the pdfkit invocation of bench/run.sh: US Letter, 0.75in margins.
 */
public final class Convert {
  private static final String PAGE_CSS = "@page { size: 8.5in 11in; margin: 0.75in; }";
  private static final String FONT_DIR = System.getProperty("bench.fontdir", "/usr/share/fonts/truetype/dejavu");

  /** File name (without .ttf), family, weight, style. Missing files are skipped. */
  private static final Object[][] FONTS = {
    {"DejaVuSerif", "DejaVu Serif", 400, FontStyle.NORMAL},
    {"DejaVuSerif-Bold", "DejaVu Serif", 700, FontStyle.NORMAL},
    {"DejaVuSerif-Italic", "DejaVu Serif", 400, FontStyle.ITALIC},
    {"DejaVuSerif-BoldItalic", "DejaVu Serif", 700, FontStyle.ITALIC},
    {"DejaVuSans", "DejaVu Sans", 400, FontStyle.NORMAL},
    {"DejaVuSans-Bold", "DejaVu Sans", 700, FontStyle.NORMAL},
    {"DejaVuSans-Oblique", "DejaVu Sans", 400, FontStyle.ITALIC},
    {"DejaVuSans-BoldOblique", "DejaVu Sans", 700, FontStyle.ITALIC},
    {"DejaVuSansMono", "DejaVu Sans Mono", 400, FontStyle.NORMAL},
    {"DejaVuSansMono-Bold", "DejaVu Sans Mono", 700, FontStyle.NORMAL},
    {"DejaVuSansMono-Oblique", "DejaVu Sans Mono", 400, FontStyle.ITALIC},
    {"DejaVuSansMono-BoldOblique", "DejaVu Sans Mono", 700, FontStyle.ITALIC},
  };

  /** CSS generic families mapped onto DejaVu, as fontconfig does for the Rust port's peers. */
  private static final String[][] GENERIC = {
    {"serif", "DejaVuSerif"}, {"sans-serif", "DejaVuSans"}, {"monospace", "DejaVuSansMono"},
  };

  public static void main(String[] args) throws Exception {
    if (args.length > 0 && args[0].equals("--montage")) {
      montage(Arrays.copyOfRange(args, 1, args.length));
      return;
    }
    int warmup = 0;
    int loop = 0;
    List<String> pos = new ArrayList<>();
    for (int i = 0; i < args.length; i++) {
      switch (args[i]) {
        case "--warmup" -> warmup = Integer.parseInt(args[++i]);
        case "--loop" -> loop = Integer.parseInt(args[++i]);
        default -> pos.add(args[i]);
      }
    }
    if (pos.size() != 2) {
      System.err.println("usage: convert [--warmup W] [--loop N] in.html out.pdf | --montage out.png label a.png ...");
      System.exit(2);
    }
    XRLog.setLoggingEnabled(false);
    File in = new File(pos.get(0));
    File out = new File(pos.get(1));

    if (loop <= 0) {
      Files.write(out.toPath(), convert(in));
      return;
    }

    long t0 = System.nanoTime();
    byte[] pdf = convert(in);
    double firstMs = (System.nanoTime() - t0) / 1e6;
    for (int i = 1; i < warmup; i++) {
      convert(in);
    }
    double[] ms = new double[loop];
    long cpu0 = processCpuNanos();
    for (int i = 0; i < loop; i++) {
      long t = System.nanoTime();
      pdf = convert(in);
      ms[i] = (System.nanoTime() - t) / 1e6;
    }
    long cpu1 = processCpuNanos();
    Files.write(out.toPath(), pdf);

    double mean = Arrays.stream(ms).average().orElse(0);
    double sd = Math.sqrt(Arrays.stream(ms).map(v -> (v - mean) * (v - mean)).sum() / loop);
    double[] s = ms.clone();
    Arrays.sort(s);
    double cpu = cpu0 < 0 ? -1 : (cpu1 - cpu0) / 1e6 / loop;
    System.out.printf(
        java.util.Locale.ROOT,
        "{\"impl\":\"java\",\"warmup\":%d,\"loop\":%d,\"first_ms\":%.3f,\"mean_ms\":%.3f,\"median_ms\":%.3f,"
            + "\"p95_ms\":%.3f,\"min_ms\":%.3f,\"max_ms\":%.3f,\"stddev_ms\":%.3f,\"cpu_ms_per_doc\":%.3f,"
            + "\"bytes\":%d,\"max_heap_mib\":%d,\"java\":\"%s\"}%n",
        warmup, loop, firstMs, mean, pct(s, 0.5), pct(s, 0.95), s[0], s[s.length - 1], sd, cpu,
        pdf.length, Runtime.getRuntime().maxMemory() >> 20, System.getProperty("java.version"));
  }

  static byte[] convert(File in) throws Exception {
    org.jsoup.nodes.Document jdoc = Jsoup.parse(in, "UTF-8", in.toURI().toString());
    jdoc.head().prependElement("style").text(PAGE_CSS);
    org.w3c.dom.Document doc = new W3CDom().fromJsoup(jdoc);

    ByteArrayOutputStream os = new ByteArrayOutputStream(1 << 16);
    PdfRendererBuilder b = new PdfRendererBuilder();
    b.useFastMode();
    b.useDefaultPageSize(8.5f, 11f, PageSizeUnits.INCHES);
    for (Object[] f : FONTS) {
      File ttf = new File(FONT_DIR, f[0] + ".ttf");
      if (ttf.isFile()) {
        b.useFont(ttf, (String) f[1], (Integer) f[2], (FontStyle) f[3], true);
      }
    }
    for (String[] g : GENERIC) {
      File ttf = new File(FONT_DIR, g[1] + ".ttf");
      File bold = new File(FONT_DIR, g[1] + "-Bold.ttf");
      if (ttf.isFile()) b.useFont(ttf, g[0], 400, FontStyle.NORMAL, true);
      if (bold.isFile()) b.useFont(bold, g[0], 700, FontStyle.NORMAL, true);
    }
    b.withW3cDocument(doc, in.toURI().toString());
    b.toStream(os);
    b.run();
    return os.toByteArray();
  }

  static double pct(double[] sorted, double p) {
    return sorted[(int) Math.min(sorted.length - 1, Math.round(p * (sorted.length - 1)))];
  }

  static long processCpuNanos() {
    if (ManagementFactory.getOperatingSystemMXBean() instanceof com.sun.management.OperatingSystemMXBean os) {
      return os.getProcessCpuTime();
    }
    return -1;
  }

  /** Side-by-side PNG: each image under its label, white background, 12 px gutters. */
  static void montage(String[] a) throws Exception {
    File out = new File(a[0]);
    List<String> labels = new ArrayList<>();
    List<BufferedImage> imgs = new ArrayList<>();
    for (int i = 1; i + 1 < a.length; i += 2) {
      labels.add(a[i]);
      imgs.add(ImageIO.read(new File(a[i + 1])));
    }
    int gutter = 12, labelH = 22;
    int w = gutter, h = 0;
    for (BufferedImage im : imgs) {
      w += im.getWidth() + gutter;
      h = Math.max(h, im.getHeight());
    }
    BufferedImage canvas = new BufferedImage(w, h + labelH + 2 * gutter, BufferedImage.TYPE_INT_RGB);
    Graphics2D g = canvas.createGraphics();
    g.setRenderingHint(RenderingHints.KEY_TEXT_ANTIALIASING, RenderingHints.VALUE_TEXT_ANTIALIAS_ON);
    g.setColor(Color.WHITE);
    g.fillRect(0, 0, canvas.getWidth(), canvas.getHeight());
    g.setFont(new Font(Font.SANS_SERIF, Font.BOLD, 14));
    int x = gutter;
    for (int i = 0; i < imgs.size(); i++) {
      BufferedImage im = imgs.get(i);
      g.setColor(Color.BLACK);
      g.drawString(labels.get(i), x, gutter + 15);
      g.drawImage(im, x, gutter + labelH, null);
      g.setColor(Color.GRAY);
      g.drawRect(x - 1, gutter + labelH - 1, im.getWidth() + 1, im.getHeight() + 1);
      x += im.getWidth() + gutter;
    }
    g.dispose();
    ImageIO.write(canvas, "png", out);
  }
}
