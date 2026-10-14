//! The wkhtmltopdf command line parser: object grammar (`page`, `cover`,
//! `toc`), scoping of switches, usage/man/readme generation. Ports upstream
//! `src/pdf/pdfcommandlineparser.hh` and `src/pdf/pdfcommandlineparser.cc`.
//!
//! Upstream prints and calls `exit()` from inside the parser. Here the parser
//! writes into [`Streams`] and returns [`Exit`]; the caller flushes the
//! streams and exits with that code, which keeps every path unit-testable.

use wk_cli::commandlineparserbase::version_text;
use wk_cli::commondocparts::{
    license, output_authors, output_license, output_name, output_proxy_doc, output_static_problems,
};
use wk_cli::htmloutputter::HtmlOutputter;
use wk_cli::manoutputter::ManOutputter;
use wk_cli::textoutputter::TextOutputter;
use wk_cli::{Action, CommandLineParserBase, Fd, Outputter, ParseError, Streams};
use wk_settings::{PdfGlobal, PdfObject, TableOfContent};

use crate::pdfarguments::{DUMP_DEFAULT_TOC_XSL, register};
use crate::pdfdocparts::*;
use crate::tocstylesheet::dump_default_toc_style_sheet;

/// Scope bit: valid in the global options area.
pub const GLOBAL: i32 = 1;
/// Scope bit: valid for page, cover and toc objects (and as global default).
pub const PAGE: i32 = 2;
/// Scope bit: valid after a `toc` object only.
pub const TOC: i32 = 4;

/// Target of the global-scoped switches: the global settings plus the
/// parser's own `readArgsFromStdin` flag.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct GlobalTarget {
    pub settings: PdfGlobal,
    pub read_args_from_stdin: bool,
}

/// The process must exit with this code (after flushing the streams).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Exit(pub i32);

/// `PdfCommandLineParser`.
pub struct PdfCommandLineParser {
    pub base: CommandLineParserBase<GlobalTarget, PdfObject>,
    /// The pristine object template (`od`): help shows its values for every
    /// page-scoped switch, whatever was parsed before.
    pub od: PdfObject,
}

impl Default for PdfCommandLineParser {
    fn default() -> Self {
        Self::new()
    }
}

/// `appName()`.
pub const APP_NAME: &str = "wkhtmltopdf";

impl PdfCommandLineParser {
    /// Build the parser and register every switch.
    pub fn new() -> Self {
        let mut base = CommandLineParserBase::default();
        register(&mut base);
        PdfCommandLineParser { base, od: PdfObject::default() }
    }

    /// `usage(fd, extended)`: `--help`, `-H`, and the usage printed after an
    /// argument error.
    pub fn usage(&self, s: &mut Streams, fd: Fd, extended: bool, g: &GlobalTarget) {
        let mut o = TextOutputter::new(s, fd, false);
        output_name(&mut o, APP_NAME);
        output_synopsis(&mut o);
        output_description(&mut o);
        self.base.output_switches(&mut o, extended, false, g, &self.od);
        if extended {
            output_page_sizes(&mut o);
            output_args_from_stdin(&mut o);
            output_proxy_doc(&mut o);
            output_header_footer_doc(&mut o);
            output_outline_doc(&mut o);
            output_table_of_content_doc(&mut o);
        }
        output_contact(&mut o);
    }

    /// `manpage(fd)`: `--manpage`.
    pub fn manpage(&self, s: &mut Streams, fd: Fd, g: &GlobalTarget) {
        let mut o = ManOutputter::new(s, fd);
        output_man_name(&mut o);
        output_synopsis(&mut o);
        output_description(&mut o);
        self.base.output_switches(&mut o, true, false, g, &self.od);
        output_proxy_doc(&mut o);
        output_header_footer_doc(&mut o);
        output_outline_doc(&mut o);
        output_table_of_content_doc(&mut o);
        output_page_sizes(&mut o);
        output_args_from_stdin(&mut o);
        output_page_break_doc(&mut o);
        output_contact(&mut o);
        output_authors(&mut o);
        o.finish();
    }

    /// `readme(fd, html)`: `--readme` and `--htmldoc`.
    pub fn readme(&self, s: &mut Streams, fd: Fd, html: bool, g: &GlobalTarget) {
        if html {
            let mut o = HtmlOutputter::new(s, fd);
            self.readme_parts(&mut o, g);
            o.finish();
        } else {
            let mut o = TextOutputter::new(s, fd, true);
            self.readme_parts(&mut o, g);
            o.finish();
        }
    }

    fn readme_parts(&self, o: &mut dyn Outputter, g: &GlobalTarget) {
        output_doc_start(o);
        output_contact(o);
        output_not_patched(o, false);
        output_license(o);
        output_authors(o);
        output_synopsis(o);
        self.base.output_switches(o, true, true, g, &self.od);
        output_proxy_doc(o);
        output_header_footer_doc(o);
        output_outline_doc(o);
        output_table_of_content_doc(o);
        output_page_break_doc(o);
        output_page_sizes(o);
        output_args_from_stdin(o);
        output_static_problems(o);
        output_installation(o);
        output_examples(o);
    }

    /// Run a documentation switch, as its upstream handler does before
    /// `exit(0)`.
    pub fn run_action(&self, a: Action, s: &mut Streams, g: &GlobalTarget) {
        match a {
            Action::Help { extended } => self.usage(s, Fd::Stdout, extended, g),
            Action::Version => s.out.extend_from_slice(version_text(APP_NAME).as_bytes()),
            Action::License => license(s, Fd::Stdout, APP_NAME),
            Action::ManPage => self.manpage(s, Fd::Stdout, g),
            Action::Readme { html } => self.readme(s, Fd::Stdout, html, g),
            Action::Custom(name) => {
                debug_assert_eq!(name, DUMP_DEFAULT_TOC_XSL);
                // Always the default settings: toc options cannot precede it.
                let xsl = dump_default_toc_style_sheet(&TableOfContent::default());
                s.out.extend_from_slice(&s.local8bit(&xsl));
            }
        }
    }

    /// Print `msg` and the usage to stderr: the common error path.
    fn usage_error(&self, msg: &[u8], s: &mut Streams, g: &GlobalTarget) -> Exit {
        s.err.extend_from_slice(msg);
        self.usage(s, Fd::Stderr, false, g);
        Exit(1)
    }

    fn handle(&self, e: ParseError, s: &mut Streams, g: &GlobalTarget) -> Exit {
        match e {
            ParseError::Usage(msg) => self.usage_error(&msg, s, g),
            ParseError::Action(a) => {
                self.run_action(a, s, g);
                Exit(0)
            }
        }
    }

    /// `parseArguments(argc, argv, fromStdin)`. `argv[0]` is the program
    /// name. On success the objects are appended to `objects` and the output
    /// is stored in `g.settings.out` (unless the parse stopped early because
    /// of `--read-args-from-stdin`).
    pub fn parse_arguments(
        &self,
        argv: &[Vec<u8>],
        from_stdin: bool,
        g: &mut GlobalTarget,
        objects: &mut Vec<PdfObject>,
        s: &mut Streams,
    ) -> Result<(), Exit> {
        let argc = argv.len();
        let at = |i: usize, j: usize| argv[i].get(j).copied().unwrap_or(0);
        // An argument ends an option list when it does not start with '-',
        // is exactly "-", or default mode is on.
        let is_option = |i: usize, default_mode: bool| at(i, 0) == b'-' && at(i, 1) != 0 && !default_mode;

        let mut default_mode = false;
        let mut arg = 1;
        let mut def = PdfObject::default();

        // Global options.
        while arg < argc {
            if !is_option(arg, default_mode) {
                break;
            }
            if let Err(e) = self.base.parse_arg(GLOBAL | PAGE, argv, &mut default_mode, &mut arg, g, &mut def)
            {
                return Err(self.handle(e, s, g));
            }
            arg += 1;
        }

        if g.read_args_from_stdin && !from_stdin {
            return Ok(());
        }

        // Objects and their options. The last argument is always the output.
        let last = argc.saturating_sub(1);
        while arg < last {
            objects.push(def.clone());
            let mut sections = PAGE;
            let ps_idx = objects.len() - 1;
            if argv[arg] == b"cover" {
                arg += 1;
                if arg >= last {
                    return Err(self.usage_error(b"You need to specify a input file to cover\n\n", s, g));
                }
                objects[ps_idx].page = wk_cli::arghandler::from_local8bit(&argv[arg]);
                arg += 1;
                while arg < argc {
                    if !is_option(arg, default_mode) {
                        break;
                    }
                    let ps = &mut objects[ps_idx];
                    if let Err(e) = self.base.parse_arg(sections, argv, &mut default_mode, &mut arg, g, ps) {
                        return Err(self.handle(e, s, g));
                    }
                    arg += 1;
                }
                // A cover never has headers/footers and is never in the
                // outline, whatever its options say.
                let ps = &mut objects[ps_idx];
                ps.header.left.clear();
                ps.header.right.clear();
                ps.header.center.clear();
                ps.footer.left.clear();
                ps.footer.right.clear();
                ps.footer.center.clear();
                ps.header.line = false;
                ps.footer.line = false;
                ps.header.html_url.clear();
                ps.footer.html_url.clear();
                ps.include_in_outline = false;
                continue;
            } else if argv[arg] == b"toc" {
                arg += 1;
                sections = PAGE | TOC;
                objects[ps_idx].is_table_of_content = true;
            } else {
                if argv[arg] == b"page" {
                    arg += 1;
                    if arg >= last {
                        return Err(self.usage_error(b"You need to specify a input file to page\n\n", s, g));
                    }
                }
                objects[ps_idx].page = wk_cli::arghandler::from_local8bit(&argv[arg]);
                arg += 1;
            }
            while arg < argc {
                if !is_option(arg, default_mode) {
                    break;
                }
                let ps = &mut objects[ps_idx];
                if let Err(e) = self.base.parse_arg(sections, argv, &mut default_mode, &mut arg, g, ps) {
                    return Err(self.handle(e, s, g));
                }
                arg += 1;
            }
        }

        if objects.is_empty() || argc < 2 {
            return Err(self.usage_error(
                b"You need to specify at least one input file, and exactly one output file\nUse - for stdin or stdout\n\n",
                s,
                g,
            ));
        }
        g.settings.out = wk_cli::arghandler::from_local8bit(&argv[argc - 1]);
        Ok(())
    }
}
