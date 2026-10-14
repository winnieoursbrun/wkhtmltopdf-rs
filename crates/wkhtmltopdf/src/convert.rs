//! Conversion entry point, standing in for `PdfConverter` +
//! `ProgressFeedback` + `handleError` in upstream `src/pdf/wkhtmltopdf.cc`.
//! The pipeline itself lives in `wk-converter`.

use std::io::Read;

use wk_cli::Streams;
use wk_settings::{PdfGlobal, PdfObject};

/// Convert `objects` into `global.out` and return the process exit code.
/// `app_name` is the executable's file name (it appears in the
/// `User-Agent`, like `QCoreApplication::applicationName`).
pub fn convert(
    global: &PdfGlobal,
    objects: &[PdfObject],
    stdin: &mut dyn Read,
    app_name: &str,
    s: &mut Streams,
) -> i32 {
    wk_converter::convert(
        global,
        objects,
        wk_converter::ConvertIo {
            stdin,
            stdout: &mut s.out,
            stderr: &mut s.err,
            app_name: app_name.to_string(),
        },
    )
}

/// One conversion of a `--read-args-from-stdin` batch: success only, no
/// `Exit with code` line (upstream returns `EXIT_FAILURE` directly).
pub fn convert_batch_line(
    global: &PdfGlobal,
    objects: &[PdfObject],
    stdin: &mut dyn Read,
    app_name: &str,
    s: &mut Streams,
) -> bool {
    wk_converter::convert_raw(
        global,
        objects,
        wk_converter::ConvertIo {
            stdin,
            stdout: &mut s.out,
            stderr: &mut s.err,
            app_name: app_name.to_string(),
        },
    )
    .0
}
