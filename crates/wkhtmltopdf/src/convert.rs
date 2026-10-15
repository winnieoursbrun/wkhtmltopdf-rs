//! Conversion entry point, standing in for `PdfConverter` +
//! `ProgressFeedback` + `handleError` in upstream `src/pdf/wkhtmltopdf.cc`.
//! The pipeline itself lives in `wk-converter`.

use std::io::Read;

use wk_cli::Streams;
use wk_settings::{PdfGlobal, PdfObject};

/// Worker threads of a conversion: at most [`MAX_THREADS`] (and no more than the CPUs
/// available), or `RAYON_NUM_THREADS`. A large document is styled and has its text layouts
/// built on them, and the PDF writer compresses each page's content stream on the rayon
/// pool while the next page is built. The pool also does the input-independent start-up
/// work ([`wk_converter::warm_up`]) while the input is read and parsed. The output does not
/// depend on the number of threads.
fn configure_worker_pool() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        let threads = worker_threads(
            std::env::var("RAYON_NUM_THREADS").ok().and_then(|v| v.parse().ok()),
            std::thread::available_parallelism().map_or(1, |n| n.get()),
        );
        // Fails only if the pool already exists, which is then used as it is.
        let _ = rayon::ThreadPoolBuilder::new()
            .num_threads(threads)
            .thread_name(|i| format!("wk-worker-{i}"))
            .build_global();
        wk_converter::set_parallelism(rayon::current_num_threads());
        rayon::spawn(wk_converter::warm_up);
    });
}

/// More threads than this bring nothing measurable on the bench documents.
const MAX_THREADS: usize = 4;

/// The worker count for `RAYON_NUM_THREADS` (`0` or unset: the default) and `cpus`.
fn worker_threads(env: Option<usize>, cpus: usize) -> usize {
    match env {
        Some(n) if n > 0 => n,
        _ => cpus.clamp(1, MAX_THREADS),
    }
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

#[cfg(test)]
mod tests {
    use super::worker_threads;

    #[test]
    fn worker_thread_count() {
        assert_eq!(worker_threads(None, 1), 1);
        assert_eq!(worker_threads(None, 2), 2);
        assert_eq!(worker_threads(None, 16), 4);
        assert_eq!(worker_threads(Some(0), 16), 4);
        assert_eq!(worker_threads(Some(8), 2), 8);
    }
}
