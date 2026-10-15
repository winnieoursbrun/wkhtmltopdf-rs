//! Conversion entry point, standing in for `PdfConverter` +
//! `ProgressFeedback` + `handleError` in upstream `src/pdf/wkhtmltopdf.cc`.
//! The pipeline itself lives in `wk-converter`.

use std::io::Read;

use wk_cli::Streams;
use wk_settings::{PdfGlobal, PdfObject};

/// The PDF writer compresses each page's content stream on the rayon pool while the next
/// page is built. One worker keeps up with the page builder; a pool of one thread per CPU
/// (rayon's default) only costs thread creation on short documents. `RAYON_NUM_THREADS`
/// still overrides it. The pool also does the input-independent start-up work
/// ([`wk_converter::warm_up`]) while the input is read and parsed.
fn configure_worker_pool() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        if std::env::var_os("RAYON_NUM_THREADS").is_none() {
            // Fails only if the pool already exists, which is then used as it is.
            let _ = rayon::ThreadPoolBuilder::new().num_threads(2).build_global();
        }
        rayon::spawn(wk_converter::warm_up);
    });
}

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
    configure_worker_pool();
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
    configure_worker_pool();
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
