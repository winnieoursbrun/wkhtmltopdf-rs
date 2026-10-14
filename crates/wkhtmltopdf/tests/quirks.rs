//! One test per upstream command line quirk (spec §1, §10), named after the
//! quirk. They drive the library entry points directly, with buffered
//! streams in the C locale.

use wk_cli::Streams;
use wk_settings::{LogLevel, PdfObject, Unit, UnitReal};
use wkhtmltopdf_app::{GlobalTarget, PdfCommandLineParser, run};

fn argv(args: &[&str]) -> Vec<Vec<u8>> {
    std::iter::once("wkhtmltopdf").chain(args.iter().copied()).map(|a| a.as_bytes().to_vec()).collect()
}

struct Parsed {
    result: Result<(), i32>,
    g: GlobalTarget,
    objects: Vec<PdfObject>,
    s: Streams,
}

fn parse(args: &[&str]) -> Parsed {
    let parser = PdfCommandLineParser::new();
    let mut g = GlobalTarget::default();
    let mut objects = Vec::new();
    let mut s = Streams::new(false);
    let result = parser.parse_arguments(&argv(args), false, &mut g, &mut objects, &mut s).map_err(|e| e.0);
    Parsed { result, g, objects, s }
}

fn run_with(args: &[&str], stdin: &str) -> (i32, Streams) {
    let mut s = Streams::new(false);
    let mut input = stdin.as_bytes();
    let code = run(&argv(args), &mut input, &mut s);
    (code, s)
}

fn err(p: &Parsed) -> String {
    String::from_utf8_lossy(&p.s.err).into_owned()
}

fn out(p: &Parsed) -> String {
    String::from_utf8_lossy(&p.s.out).into_owned()
}

fn pages(p: &Parsed) -> Vec<&str> {
    p.objects.iter().map(|o| o.page.as_str()).collect()
}

#[test]
fn usage_newlines_go_to_stdout() {
    let p = parse(&[]);
    assert_eq!(p.result, Err(1));
    assert_eq!(p.s.out, vec![b'\n'; 15]);
    assert!(err(&p).starts_with("You need to specify at least one input file"));
}

#[test]
fn last_arg_is_always_output() {
    let p = parse(&["a.html", "--print-media-type"]);
    assert_eq!(p.result, Ok(()));
    assert_eq!(pages(&p), ["a.html"]);
    assert!(p.objects[0].load.print_media_type);
    assert_eq!(p.g.settings.out, "--print-media-type");
}

#[test]
fn two_inputs_second_becomes_output() {
    let p = parse(&["a.html", "b.html"]);
    assert_eq!(p.result, Ok(()));
    assert_eq!(pages(&p), ["a.html"]);
    assert_eq!(p.g.settings.out, "b.html");
}

#[test]
fn option_argument_may_be_the_output() {
    let p = parse(&["a.html", "--title", "out.pdf"]);
    assert_eq!(p.result, Err(1));
    assert_eq!(p.g.settings.document_title, "");
    // --title is global: rejected after an object.
    assert!(err(&p).starts_with("--title specified in incorrect location"));
    let p = parse(&["a.html", "--zoom", "out.pdf"]);
    assert_eq!(p.result, Err(1));
    assert!(err(&p).starts_with("Invalid argument(s) parsed to --zoom"));
    let p = parse(&["a.html", "--window-status", "out.pdf"]);
    assert_eq!(p.result, Ok(()));
    assert_eq!(p.objects[0].load.window_status, "out.pdf");
    assert_eq!(p.g.settings.out, "out.pdf");
}

#[test]
fn dashdash_is_unknown_long_argument() {
    let p = parse(&["--", "a", "b"]);
    assert_eq!(p.result, Err(1));
    assert!(err(&p).starts_with("Unknown long argument --\n\n"));
}

#[test]
fn dash_zero_enters_default_mode() {
    // --0 sets default mode; the option loops then stop, so "--foo" is taken
    // as an input instead of an unknown switch.
    let p = parse(&["--0", "--foo", "-q", "out.pdf"]);
    assert_eq!(p.result, Ok(()));
    assert_eq!(pages(&p), ["--foo", "-q"]);
    assert_eq!(p.g.settings.log_level, LogLevel::Info);
}

#[test]
fn dash_alone_is_stdin_and_ends_global_area() {
    let p = parse(&["-q", "-", "-"]);
    assert_eq!(p.result, Ok(()));
    assert_eq!(pages(&p), ["-"]);
    assert_eq!(p.g.settings.out, "-");
    assert_eq!(p.g.settings.log_level, LogLevel::None);
}

#[test]
fn help_defaults_reflect_previous_global_options() {
    let p = parse(&["-q", "--help"]);
    assert_eq!(p.result, Err(0));
    let o = out(&p);
    assert!(o.contains("info (default none)"), "{o}");
    assert!(o.contains(" none (default)\n"), "{o}");
}

#[test]
fn help_defaults_ignore_previous_page_options() {
    // Page options given globally go to the object template, not to the
    // parser's own template that the help reads.
    let p = parse(&["--zoom", "2", "--javascript-delay", "5", "-H"]);
    assert_eq!(p.result, Err(0));
    let o = out(&p);
    assert!(o.contains("Use this zoom factor (default 1)"));
    assert!(o.contains("finish (default 200)"));
}

#[test]
fn page_offset_shows_default_and_writes_global() {
    let p = parse(&["--page-offset", "7", "-H"]);
    assert!(out(&p).contains("Set the starting page number (default 7)"));
}

#[test]
fn version_action_ignores_later_arguments() {
    let p = parse(&["--version", "--bogus"]);
    assert_eq!(p.result, Err(0));
    assert_eq!(out(&p), "wkhtmltopdf 0.12.6 (with patched qt)\n");
    let p = parse(&["--bogus", "--version"]);
    assert_eq!(p.result, Err(1));
    assert!(err(&p).starts_with("Unknown long argument --bogus"));
}

#[test]
fn short_switches_group_and_consume_arguments() {
    let p = parse(&["-Tq", "5mm", "a", "b"]);
    assert_eq!(p.result, Ok(()));
    assert_eq!(p.g.settings.margin.top, UnitReal::new(5.0, Unit::Millimeter));
    assert_eq!(p.g.settings.log_level, LogLevel::None);
    let p = parse(&["-qV"]);
    assert_eq!(p.result, Err(0));
    assert_eq!(out(&p), "wkhtmltopdf 0.12.6 (with patched qt)\n");
    let p = parse(&["-qz", "a", "b"]);
    assert!(err(&p).starts_with("Unknown switch -z\n\n"));
    let p = parse(&["-T"]);
    assert!(err(&p).starts_with("Not enough arguments parsed to -T\n\n"));
    let p = parse(&["-T", "-1cm", "a", "b"]);
    assert!(err(&p).starts_with("Invalid argument(s) parsed to -T\n\n"));
    let p = parse(&["a.html", "-T", "1cm", "b"]);
    assert!(err(&p).starts_with("-T specified in incorrect location\n\n"));
}

#[test]
fn no_custom_header_propagation_enables_propagation() {
    let p = parse(&["--no-custom-header-propagation", "a", "b"]);
    assert!(p.objects[0].load.repeat_custom_headers);
}

#[test]
fn page_scoped_options_write_global_settings() {
    let p = parse(&[
        "a.html",
        "--page-offset",
        "3",
        "--viewport-size",
        "800x600",
        "--keep-relative-links",
        "--default-header",
        "out.pdf",
    ]);
    assert_eq!(p.result, Ok(()));
    assert_eq!(p.g.settings.page_offset, 3);
    assert_eq!(p.g.settings.viewport_size, "800x600");
    assert!(!p.g.settings.resolve_relative_links);
    assert_eq!(p.g.settings.margin.top, UnitReal::new(20.0, Unit::Millimeter));
    let o = &p.objects[0];
    assert_eq!(
        (o.header.left.as_str(), o.header.right.as_str(), o.header.line),
        ("[webpage]", "[page]/[topage]", true)
    );
}

#[test]
fn global_page_options_are_object_defaults() {
    let p = parse(&["--zoom", "2", "a.html", "b.html", "--zoom", "3", "out.pdf"]);
    assert_eq!(pages(&p), ["a.html", "b.html"]);
    assert_eq!(p.objects[0].load.zoom_factor, 2.0);
    assert_eq!(p.objects[1].load.zoom_factor, 3.0);
}

#[test]
fn cover_clears_headers_footers_and_outline() {
    let p = parse(&[
        "--header-left",
        "H",
        "cover",
        "c.html",
        "--footer-line",
        "--footer-html",
        "f.html",
        "--zoom",
        "2",
        "a.html",
        "out.pdf",
    ]);
    assert_eq!(p.result, Ok(()));
    assert_eq!(pages(&p), ["c.html", "a.html"]);
    let c = &p.objects[0];
    assert_eq!(c.header.left, "");
    assert!(!c.footer.line);
    assert_eq!(c.footer.html_url, "");
    assert!(!c.include_in_outline);
    assert!(c.pages_count);
    assert_eq!(c.load.zoom_factor, 2.0);
    assert_eq!(p.objects[1].header.left, "H");
}

#[test]
fn cover_and_page_need_an_input() {
    let p = parse(&["cover", "b.pdf"]);
    assert!(err(&p).starts_with("You need to specify a input file to cover\n\n"));
    let p = parse(&["page", "b.pdf"]);
    assert!(err(&p).starts_with("You need to specify a input file to page\n\n"));
}

#[test]
fn toc_has_no_input_and_page_keyword_escapes_names() {
    let p = parse(&["toc", "out.pdf"]);
    assert_eq!(p.result, Ok(()));
    assert!(p.objects[0].is_table_of_content);
    let p = parse(&["page", "toc", "out.pdf"]);
    assert_eq!(pages(&p), ["toc"]);
    assert!(!p.objects[0].is_table_of_content);
    // Keywords are case-sensitive.
    let p = parse(&["TOC", "out.pdf"]);
    assert_eq!(pages(&p), ["TOC"]);
}

#[test]
fn toc_options_only_after_toc() {
    let p = parse(&["--toc-header-text", "X", "a", "b"]);
    assert!(err(&p).starts_with("--toc-header-text specified in incorrect location"));
    let p = parse(&["a.html", "--disable-dotted-lines", "b"]);
    assert!(err(&p).starts_with("--disable-dotted-lines specified in incorrect location"));
    let p = parse(&["toc", "--toc-header-text", "X", "--zoom", "2", "a.html", "out.pdf"]);
    assert_eq!(p.result, Ok(()));
    assert_eq!(p.objects[0].toc.caption_text, "X");
    assert_eq!(p.objects[0].load.zoom_factor, 2.0);
    assert_eq!(p.objects[1].toc.caption_text, "Table of Contents");
}

#[test]
fn global_option_after_object_is_incorrect_location() {
    let p = parse(&["a.html", "--margin-top", "1cm", "b.pdf"]);
    assert_eq!(p.result, Err(1));
    assert!(err(&p).starts_with("--margin-top specified in incorrect location\n\n"));
}

#[test]
fn log_level_debug_rejected_in_0_12_6() {
    let p = parse(&["--log-level", "debug", "a", "b"]);
    assert!(err(&p).starts_with("Invalid argument(s) parsed to --log-level"));
    let p = parse(&["--log-level", "WARN", "a", "b"]);
    assert_eq!(p.g.settings.log_level, LogLevel::Warn);
}

#[test]
fn proxy_none_is_lowercase_and_help_says_none() {
    let p = parse(&["-p", "None", "a", "b"]);
    assert_eq!(p.result, Ok(()));
    assert_eq!(p.objects[0].load.proxy.host, "None");
    let p = parse(&["-p", ":1", "a", "b"]);
    assert!(err(&p).starts_with("Invalid argument(s) parsed to -p"));
}

#[test]
fn repeatable_options_append() {
    let p = parse(&[
        "--custom-header",
        "A",
        "1",
        "--custom-header",
        "B",
        "2",
        "--post",
        "k",
        "v",
        "--post-file",
        "f",
        "/tmp/x",
        "--allow",
        "/a",
        "--allow",
        "/b",
        "--replace",
        "x",
        "y",
        "a",
        "b",
    ]);
    let o = &p.objects[0];
    assert_eq!(o.load.custom_headers, vec![("A".into(), "1".into()), ("B".into(), "2".into())]);
    assert_eq!(o.load.post.len(), 2);
    assert!(!o.load.post[0].file && o.load.post[1].file);
    assert_eq!(o.load.allowed, ["/a", "/b"]);
    assert_eq!(o.replacements, vec![("x".into(), "y".into())]);
}

#[test]
fn every_one_of_the_122_long_options_is_registered() {
    let parser = PdfCommandLineParser::new();
    assert_eq!(parser.base.handlers.len(), 122);
}

#[test]
fn successful_parse_converts_a_local_file() {
    let dir = std::env::temp_dir().join(format!("wk-quirk-convert-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let input = dir.join("a.html");
    let output = dir.join("out.pdf");
    std::fs::write(&input, "<html><head><title>T</title></head><body><p>Hello</p></body></html>").unwrap();
    let (code, s) = run_with(&["-q", input.to_str().unwrap(), output.to_str().unwrap()], "");
    assert_eq!(code, 0, "{}", String::from_utf8_lossy(&s.err));
    assert!(s.err.is_empty() && s.out.is_empty());
    assert!(std::fs::read(&output).unwrap().starts_with(b"%PDF-1.4"));
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn read_args_from_stdin_ignores_objects_on_real_command_line() {
    let (code, s) = run_with(&["--read-args-from-stdin"], "");
    assert_eq!(code, 0);
    assert!(s.err.is_empty() && s.out.is_empty());
}

#[test]
fn read_args_from_stdin_blank_line_aborts_batch() {
    let (code, s) = run_with(&["--read-args-from-stdin"], "\n");
    assert_eq!(code, 1);
    assert!(String::from_utf8_lossy(&s.err).starts_with("You need to specify at least one input file"));
    // 16 newlines, not 15: the re-parsed argv set --read-args-from-stdin, so
    // its help line gains " (default)" and wraps once more.
    assert_eq!(s.out, vec![b'\n'; 16]);
}

#[test]
fn read_args_from_stdin_appends_line_to_original_argv() {
    // The original argv (with -q) is re-parsed with the line's tokens; the
    // first conversion fails (`a b.html` is not a valid URL), which stops the
    // batch with exit 1 and no `Exit with code` line.
    let (code, s) = run_with(&["-q", "--read-args-from-stdin"], "'a b.html' out.pdf\nnext.html x.pdf\n");
    assert_eq!(code, 1);
    assert!(s.err.is_empty(), "{}", String::from_utf8_lossy(&s.err));
    // A documentation switch on a line exits 0 for the whole batch.
    let (code, s) = run_with(&["--read-args-from-stdin"], "-V\n");
    assert_eq!(code, 0);
    assert_eq!(s.out, b"wkhtmltopdf 0.12.6 (with patched qt)\n");
}

#[test]
fn dump_default_toc_xsl_always_uses_defaults() {
    let p = parse(&["--dump-default-toc-xsl"]);
    assert_eq!(p.result, Err(0));
    assert!(out(&p).contains("<title>Table of Contents</title>"));
}

#[test]
fn utf8_locale_keeps_non_ascii_in_license() {
    let parser = PdfCommandLineParser::new();
    let mut g = GlobalTarget::default();
    let mut s = Streams::new(true);
    let r = parser.parse_arguments(&argv(&["--license"]), false, &mut g, &mut Vec::new(), &mut s);
    assert_eq!(r.map_err(|e| e.0), Err(0));
    assert!(String::from_utf8(s.out).unwrap().contains("Pablo Ruiz García"));
}
