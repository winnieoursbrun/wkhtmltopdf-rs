//! `wkhtmltopdf` executable. Ports the process entry of upstream
//! `src/pdf/wkhtmltopdf.cc`.

use std::process::ExitCode;

use wk_cli::Streams;

// Heap profiling (docs/perf.md): `cargo build --release --features dhat-heap`
// writes dhat-heap.json in the working directory at exit.
#[cfg(feature = "dhat-heap")]
#[global_allocator]
static ALLOC: dhat::Alloc = dhat::Alloc;

#[cfg(unix)]
fn os_bytes(a: std::ffi::OsString) -> Vec<u8> {
    use std::os::unix::ffi::OsStringExt;
    a.into_vec()
}

#[cfg(not(unix))]
fn os_bytes(a: std::ffi::OsString) -> Vec<u8> {
    a.to_string_lossy().into_owned().into_bytes()
}

fn main() -> ExitCode {
    #[cfg(feature = "dhat-heap")]
    let _profiler = dhat::Profiler::new_heap();
    let argv: Vec<Vec<u8>> = std::env::args_os().map(os_bytes).collect();
    let mut streams = Streams::from_env();
    let stdin = std::io::stdin();
    let mut lock = stdin.lock();
    let code = wkhtmltopdf_app::run(&argv, &mut lock, &mut streams);
    streams.flush_to_process();
    ExitCode::from(code as u8)
}
