//! `wkhtmltopdf` executable. Ports the process entry of upstream
//! `src/pdf/wkhtmltopdf.cc`.

use std::process::ExitCode;

use wk_cli::Streams;

// Heap profiling (docs/perf.md): `cargo build --release --features dhat-heap`
// writes dhat-heap.json in the working directory at exit.
#[cfg(feature = "dhat-heap")]
#[global_allocator]
static ALLOC: dhat::Alloc = dhat::Alloc;

#[cfg(all(feature = "dlmalloc", not(feature = "dhat-heap")))]
#[global_allocator]
static ALLOC: dlmalloc::GlobalDlmalloc = dlmalloc::GlobalDlmalloc;

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
    // Style resolution and Boa recurse deeply on nested documents. Windows gives the main
    // thread 1 MiB of stack (Linux 8 MiB), so the conversion runs on a thread with a stack as
    // large as WebKit-era pages need. Untouched stack pages cost no memory.
    let code = std::thread::Builder::new()
        .name("main".into())
        .stack_size(64 << 20)
        .spawn(|| {
            let argv: Vec<Vec<u8>> = std::env::args_os().map(os_bytes).collect();
            let mut streams = Streams::from_env();
            let stdin = std::io::stdin();
            let mut lock = stdin.lock();
            let code = wkhtmltopdf_app::run(&argv, &mut lock, &mut streams);
            streams.flush_to_process();
            code
        })
        .expect("spawn the conversion thread")
        .join()
        .unwrap_or_else(|e| std::panic::resume_unwind(e));
    ExitCode::from(code as u8)
}
