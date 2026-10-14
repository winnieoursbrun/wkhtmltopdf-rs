//! Program flow of the `wkhtmltopdf` binary. Ports `main()` from upstream
//! `src/pdf/wkhtmltopdf.cc`: parse the command line, then either convert once
//! or, with `--read-args-from-stdin`, once per stdin line.

use std::io::BufRead;

use wk_cli::Streams;

use crate::convert::{convert, convert_batch_line};
use crate::pdfcommandlineparser::{Exit, GlobalTarget, PdfCommandLineParser};
use crate::stdinargs::{fgets_large, parse_string};

/// Run the program on `argv` (including `argv[0]`). Output goes to `s`;
/// returns the exit code.
pub fn run(argv: &[Vec<u8>], stdin: &mut dyn BufRead, s: &mut Streams) -> i32 {
    let app_name = argv
        .first()
        .map(|a| String::from_utf8_lossy(a).rsplit('/').next().unwrap_or("").to_string())
        .filter(|a| !a.is_empty())
        .unwrap_or_else(|| "wkhtmltopdf".to_string());
    let parser = PdfCommandLineParser::new();
    let mut g = GlobalTarget::default();
    let mut objects = Vec::new();
    if let Err(Exit(code)) = parser.parse_arguments(argv, false, &mut g, &mut objects, s) {
        return code;
    }

    if g.read_args_from_stdin {
        // Each line is a separate invocation whose argv is the original argv
        // (including --read-args-from-stdin) followed by the line's tokens.
        while let Some(line) = fgets_large(stdin) {
            let mut nargv = argv.to_vec();
            nargv.extend(parse_string(&line));
            let parser = PdfCommandLineParser::new();
            let mut g = GlobalTarget::default();
            let mut objects = Vec::new();
            if let Err(Exit(code)) = parser.parse_arguments(&nargv, true, &mut g, &mut objects, s) {
                return code;
            }
            // The first failed conversion ends the batch with exit 1 and no
            // "Exit with code" message.
            if !convert_batch_line(&g.settings, &objects, stdin, &app_name, s) {
                return 1;
            }
        }
        return 0;
    }

    convert(&g.settings, &objects, stdin, &app_name, s)
}
