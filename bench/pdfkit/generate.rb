# frozen_string_literal: true

# pdfkit integration run: drives one wkhtmltopdf binary through the pdfkit gem the way Rails
# apps use it, and records the outcome of every case. Never aborts on a failing case.
#
#   bundle exec ruby generate.rb <original|rust> <path-to-wkhtmltopdf> <out-dir>
#
# Writes <out-dir>/<case>.pdf, <out-dir>/<case>.png (page 1 at 50 dpi) and
# <out-dir>/results.json. The corpus is read from $CORPUS (default: ../corpus).

require "json"
require "open3"
require "shellwords"
require "fileutils"
require "pdfkit"
require "webrick"

label, binary, out_dir = ARGV
abort "usage: generate.rb <original|rust> <wkhtmltopdf> <out-dir>" unless label && binary && out_dir
CORPUS = File.expand_path(ENV.fetch("CORPUS", File.join(__dir__, "..", "corpus")))
FileUtils.mkdir_p(out_dir)

PDFKit.configure { |c| c.wkhtmltopdf = binary }

# --- Local HTTP server (URL sources, header echo, missing image) ----------------------------
server = WEBrick::HTTPServer.new(
  BindAddress: "127.0.0.1", Port: 0, DocumentRoot: CORPUS,
  Logger: WEBrick::Log.new(File::NULL), AccessLog: []
)
# Echoes the request headers and cookies it received, so the PDF proves they were sent.
server.mount_proc("/echo") do |req, res|
  res.content_type = "text/html; charset=utf-8"
  res.body = <<~HTML
    <html><head><title>Echo</title></head><body style="font-family: 'DejaVu Sans'">
    <h1>Request echo</h1>
    <p>X-Test header: <b>#{WEBrick::HTMLUtils.escape(req["X-Test"].to_s)}</b></p>
    <p>Cookie header: <b>#{WEBrick::HTMLUtils.escape(req["Cookie"].to_s)}</b></p>
    </body></html>
  HTML
end
Thread.new { server.start }
PORT = server.listeners.first.addr[1]
BASE = "http://127.0.0.1:#{PORT}"

def corpus(name) = File.join(CORPUS, name)
def read(name) = File.read(corpus(name), encoding: "UTF-8")

PRINT_MEDIA_HTML = <<~HTML
  <html><head><meta charset="utf-8"><style>
    body { font-family: 'DejaVu Sans'; }
    @media screen { .only-print { display: none; } .only-screen { color: #b00; } }
    @media print  { .only-screen { display: none; } .only-print { color: #070; font-size: 20pt; } }
  </style></head><body>
  <h1>Media type</h1>
  <p class="only-screen">SCREEN stylesheet applied</p>
  <p class="only-print">PRINT stylesheet applied</p>
  </body></html>
HTML

META_HTML = <<~HTML
  <html><head><meta charset="utf-8">
  <meta name="pdfkit-page_size" content="Legal">
  <meta name="pdfkit-orientation" content="Landscape">
  <title>Meta options</title></head>
  <body style="font-family: 'DejaVu Sans'"><h1>Legal, landscape (from meta tags)</h1><p>#{"Lorem ipsum dolor sit amet. " * 40}</p></body></html>
HTML

BROKEN_IMG_HTML = <<~HTML
  <html><head><meta charset="utf-8"></head><body style="font-family: 'DejaVu Sans'">
  <h1>Broken image</h1>
  <p>Before the image.</p>
  <img src="#{BASE}/missing.png" alt="missing" width="200" height="100">
  <p>After the image.</p>
  </body></html>
HTML

OK = "ok"
EXIT_ERROR = "PDFKit::ImproperWkhtmltopdfExitStatus"

# name => [description, expected outcome, block returning a PDFKit instance or [:file, kit]]
CASES = {
  "string_to_pdf" => ["PDFKit.new(html).to_pdf (defaults)", OK,
                      -> { PDFKit.new(read("medium.html")) }],
  "options_a4" => ["page_size A4, mm margins, landscape", OK,
                   -> { PDFKit.new(read("medium.html"), page_size: "A4", orientation: "Landscape",
                                   margin_top: "10mm", margin_bottom: "10mm",
                                   margin_left: "12mm", margin_right: "12mm") }],
  "to_file" => ["kit.to_file(path)", OK, -> { [:file, PDFKit.new(read("small.html"))] }],
  "file_source" => ["PDFKit.new(File.new(path))", OK, -> { PDFKit.new(File.new(corpus("medium.html"))) }],
  "url_source" => ["URL source from local HTTP server", OK, -> { PDFKit.new("#{BASE}/medium.html") }],
  "stylesheets" => ["kit.stylesheets << extra.css", OK,
                    -> { PDFKit.new(read("medium.html")).tap { |k| k.stylesheets << corpus("extra.css") } }],
  "meta_tags" => ["<meta name=pdfkit-page_size content=Legal> + orientation", OK, -> { PDFKit.new(META_HTML) }],
  "header_footer" => ["header_html / footer_html (local files)", OK,
                      -> { PDFKit.new(read("hf-body.html"), enable_local_file_access: true,
                                      header_html: corpus("header.html"), footer_html: corpus("footer.html"),
                                      header_spacing: 3, footer_spacing: 3) }],
  # pdfkit emits options in hash order: outline must come before toc, otherwise --outline lands
  # in the TOC object's options and wkhtmltopdf rejects it ("specified in incorrect location").
  "toc_outline" => ["outline: true, toc: true", OK,
                    -> { PDFKit.new(read("hf-body.html"), outline: true, toc: true) }],
  "toc_then_outline" => ["toc: true, outline: true (wrong order, upstream rejects it)", EXIT_ERROR,
                         -> { PDFKit.new(read("hf-body.html"), toc: true, outline: true) }],
  "cover" => ["cover: cover.html + toc", OK,
              -> { PDFKit.new(read("part1.html"), enable_local_file_access: true,
                              cover: corpus("cover.html"), toc: true) }],
  "headers_cookies" => ["custom_header + cookie against /echo", OK,
                        -> { PDFKit.new("#{BASE}/echo", custom_header: [%w[X-Test 1]],
                                        cookie: [%w[session abc123]]) }],
  "print_media_type" => ["print_media_type: true", OK, -> { PDFKit.new(PRINT_MEDIA_HTML, print_media_type: true) }],
  "javascript_delay" => ["jQuery page, javascript_delay: 500", OK,
                         -> { PDFKit.new(read("js.html"), javascript_delay: 500) }],
  "broken_image_ignore" => ["404 image, load_error_handling + load_media_error_handling ignore", OK,
                            -> { PDFKit.new(BROKEN_IMG_HTML, load_error_handling: "ignore",
                                            load_media_error_handling: "ignore") }],
  "unreachable_url" => ["unreachable URL must raise", EXIT_ERROR, -> { PDFKit.new("http://127.0.0.1:1/") }],
}.freeze

def pdf_info(path)
  out, = Open3.capture2e("pdfinfo", path)
  { pages: out[/^Pages:\s+(\d+)/, 1]&.to_i, page_size: out[/^Page size:\s+(.+)$/, 1]&.strip }
end

results = CASES.map do |name, (description, expected, build)|
  pdf = File.join(out_dir, "#{name}.pdf")
  FileUtils.rm_f([pdf, File.join(out_dir, "#{name}.png")])
  rec = { case: name, description: description, expected: expected, binary: label }
  t0 = Process.clock_gettime(Process::CLOCK_MONOTONIC)
  begin
    built = build.call
    mode, kit = built.is_a?(Array) ? built : [:string, built]
    rec[:command] = Shellwords.join(kit.command)
    if mode == :file
      kit.to_file(pdf)
    else
      File.binwrite(pdf, kit.to_pdf)
    end
    rec[:outcome] = OK
  rescue StandardError => e
    rec[:outcome] = e.class.name
    rec[:message] = e.message.lines.first.to_s.strip[0, 240]
    FileUtils.rm_f(pdf) if File.exist?(pdf) && File.size(pdf).zero?
  end
  rec[:duration_s] = (Process.clock_gettime(Process::CLOCK_MONOTONIC) - t0).round(4)
  rec[:as_expected] = rec[:outcome] == expected
  if File.exist?(pdf)
    rec[:pdf] = File.basename(pdf)
    rec[:bytes] = File.size(pdf)
    rec.merge!(pdf_info(pdf))
    system("pdftoppm", "-r", "50", "-f", "1", "-l", "1", "-singlefile", "-png", pdf,
           File.join(out_dir, name), exception: false)
  end
  status = rec[:outcome] == OK ? "ok #{rec[:pages]}p" : rec[:outcome]
  puts format("   %-8s %-20s %-40s %.2fs%s", label, name, status, rec[:duration_s],
              rec[:as_expected] ? "" : "  (expected #{expected})")
  rec
end

server.shutdown
File.write(File.join(out_dir, "results.json"), JSON.pretty_generate(results))
