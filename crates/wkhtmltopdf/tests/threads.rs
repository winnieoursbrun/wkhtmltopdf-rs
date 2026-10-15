//! The output does not depend on the number of worker threads: a long document (text, a
//! table, internal and external links, an outline) converted with one thread, where every
//! page is drawn in the document, and with four, where pages are drafted on the pool
//! (`wk_print::pdf::write_pdf`), gives the same bytes.

use std::process::{Command, Stdio};

/// A document of 30 sections, each starting on a new page.
fn document() -> String {
    let mut html = String::from(
        "<!DOCTYPE html><html><head><style>\
         h1 { page-break-before: always; } td, th { border: 1px solid #888; padding: 2px; }\
         </style></head><body><p id=top>Contents: ",
    );
    for i in 0..30 {
        html.push_str(&format!("<a href=\"#s{i}\">section {i}</a> "));
    }
    html.push_str("</p>");
    for i in 0..30 {
        html.push_str(&format!("<h1 id=s{i}>Section {i}</h1><h2>Part {i}.1</h2>"));
        for j in 0..4 {
            html.push_str(&format!(
                "<p>Paragraph {j} of section {i}: lorem ipsum dolor sit amet, consectetur \
                 adipiscing elit, sed do eiusmod tempor incididunt ut labore et dolore magna \
                 aliqua. <a href=\"https://example.com/{i}/{j}\">external link</a>, \
                 <a href=\"#top\">back to the top</a>.</p>"
            ));
        }
        html.push_str("<h2>Figures</h2><table><tr><th>Item</th><th>Quantity</th><th>Price</th></tr>");
        for r in 0..8 {
            html.push_str(&format!(
                "<tr><td>Item {i}-{r}</td><td>{}</td><td>{}.{:02} €</td></tr>",
                r + 1,
                i * r,
                r * 7
            ));
        }
        html.push_str("</table>");
    }
    html.push_str("</body></html>");
    html
}

/// The PDF and the "pages drawn from drafts: N of M" counts of a conversion with `threads`.
fn convert(html: &std::path::Path, threads: &str) -> (Vec<u8>, usize, usize) {
    let child = Command::new(env!("CARGO_BIN_EXE_wkhtmltopdf"))
        .args(["--quiet", "--outline"])
        .arg(html)
        .arg("-")
        .env("RAYON_NUM_THREADS", threads)
        .env("SOURCE_DATE_EPOCH", "1700000000")
        .env("WKHTMLTOPDF_SYSTEM_FONTS", "0")
        .env("WKHTMLTOPDF_DEBUG_TIMING", "1")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("run wkhtmltopdf");
    let o = child.wait_with_output().unwrap();
    let err = String::from_utf8_lossy(&o.stderr);
    assert!(o.status.success() && o.stdout.starts_with(b"%PDF"), "no PDF: {err}");
    let counts = err
        .lines()
        .find_map(|l| l.split_once("pages drawn from drafts: ").map(|(_, c)| c.to_string()))
        .unwrap_or_else(|| panic!("no draft count in: {err}"));
    let (drafted, pages) = counts.split_once(" of ").unwrap();
    (o.stdout, drafted.trim().parse().unwrap(), pages.trim().parse().unwrap())
}

#[test]
fn drafted_pages_are_identical_to_pages_drawn_in_the_document() {
    // From a file: HTML read from stdin goes through a temporary file of random name, which
    // the internal links' destinations would carry.
    let html = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join("threads.html");
    std::fs::write(&html, document()).unwrap();
    let (one, drafted_one, pages) = convert(&html, "1");
    assert!(pages >= 20, "{pages} pages");
    assert_eq!(drafted_one, 0, "one thread drafts nothing");
    let (four, drafted_four, pages_four) = convert(&html, "4");
    assert_eq!(pages_four, pages);
    assert!(drafted_four > 0, "four threads must draft pages on the pool");
    assert!(one == four, "the PDFs differ ({} and {} bytes)", one.len(), four.len());
}
