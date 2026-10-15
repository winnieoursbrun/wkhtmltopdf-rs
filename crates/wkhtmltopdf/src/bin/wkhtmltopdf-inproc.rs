//! In-process throughput harness for benchmarks (bench/java/). Not shipped: built only with
//! `--features bench-inproc`.
//!
//! Calls `wkhtmltopdf_app::run` repeatedly in one process, the way a long-lived service
//! embedding the library would, so it can be compared with a warmed-up JVM converting
//! documents in a loop.
//!
//! ```text
//! wkhtmltopdf-inproc [--warmup W] [--loop N] [--stdin FILE] [--out FILE] -- <wkhtmltopdf args>
//! ```
//!
//! The first iteration is always untimed (it counts as one of the `--warmup` runs). Each iteration gets a fresh copy of `--stdin` (if any) as standard input and fresh output
//! buffers; the PDF written to stdout (`-` as output) by the last iteration is saved to `--out`.
//! Prints one JSON object on stdout: wall milliseconds per timed iteration (mean, median,
//! p95, min, max, standard deviation), the first (cold, in-process) iteration, and CPU
//! milliseconds per timed iteration read from `/proc/self/stat` (Linux only, -1 elsewhere).

use std::io::Cursor;
use std::time::Instant;

use wk_cli::Streams;

// Same allocator as the CLI's static musl build (src/main.rs), so warm numbers compare the
// shipped configuration.
#[cfg(all(
    target_env = "musl",
    target_pointer_width = "64",
    not(any(feature = "dhat-heap", feature = "dlmalloc", feature = "system-alloc"))
))]
#[global_allocator]
static ALLOC: wkhtmltopdf_app::alloc::WkAlloc = wkhtmltopdf_app::alloc::WkAlloc;

fn usage() -> ! {
    eprintln!(
        "usage: wkhtmltopdf-inproc [--warmup W] [--loop N] [--stdin FILE] [--out FILE] -- <wkhtmltopdf args>"
    );
    std::process::exit(2);
}

/// User + system CPU time of this process in milliseconds, from /proc/self/stat.
fn cpu_ms() -> f64 {
    let Ok(stat) = std::fs::read_to_string("/proc/self/stat") else { return -1.0 };
    // Fields after the parenthesised command name; utime and stime are fields 14 and 15.
    let Some(rest) = stat.rsplit_once(')').map(|(_, r)| r) else { return -1.0 };
    let f: Vec<&str> = rest.split_whitespace().collect();
    let ticks = |i: usize| f.get(i).and_then(|v| v.parse::<f64>().ok()).unwrap_or(0.0);
    // rest starts at field 3 (state); utime = field 14 -> index 11, stime -> index 12.
    // USER_HZ is 100 on every Linux ABI the bench targets.
    (ticks(11) + ticks(12)) * 10.0
}

fn main() {
    let mut warmup = 3usize;
    let mut iterations = 20usize;
    let mut stdin_file: Option<String> = None;
    let mut out_file: Option<String> = None;
    let mut args = std::env::args().skip(1);
    let mut wk_args: Vec<Vec<u8>> = vec![b"wkhtmltopdf".to_vec()];
    while let Some(a) = args.next() {
        match a.as_str() {
            "--warmup" => warmup = args.next().and_then(|v| v.parse().ok()).unwrap_or_else(|| usage()),
            "--loop" => iterations = args.next().and_then(|v| v.parse().ok()).unwrap_or_else(|| usage()),
            "--stdin" => stdin_file = Some(args.next().unwrap_or_else(|| usage())),
            "--out" => out_file = Some(args.next().unwrap_or_else(|| usage())),
            "--" => {
                wk_args.extend(args.by_ref().map(String::into_bytes));
                break;
            }
            _ => usage(),
        }
    }
    if iterations == 0 {
        usage();
    }
    let input =
        stdin_file.map(|p| std::fs::read(&p).unwrap_or_else(|e| panic!("read {p}: {e}"))).unwrap_or_default();

    // Same 64 MiB conversion stack as the CLI (src/main.rs).
    let report = std::thread::Builder::new()
        .stack_size(64 << 20)
        .spawn(move || {
            let convert_once = || -> (i32, Vec<u8>, Vec<u8>) {
                let mut streams = Streams::new(true);
                let mut stdin = Cursor::new(input.as_slice());
                let code = wkhtmltopdf_app::run(&wk_args, &mut stdin, &mut streams);
                (code, streams.out, streams.err)
            };
            let check = |code: i32, err: &[u8]| {
                if code != 0 {
                    eprintln!("conversion failed with exit {code}: {}", String::from_utf8_lossy(err));
                    std::process::exit(code.clamp(1, 255));
                }
            };

            let t = Instant::now();
            let (code, _, err) = convert_once();
            check(code, &err);
            let first_ms = t.elapsed().as_secs_f64() * 1e3;
            for _ in 1..warmup {
                let (code, _, err) = convert_once();
                check(code, &err);
            }

            let mut times = Vec::with_capacity(iterations);
            let mut last = Vec::new();
            let cpu0 = cpu_ms();
            for _ in 0..iterations {
                let t = Instant::now();
                let (code, out, err) = convert_once();
                times.push(t.elapsed().as_secs_f64() * 1e3);
                check(code, &err);
                last = out;
            }
            let cpu1 = cpu_ms();
            if let Some(p) = out_file {
                std::fs::write(&p, &last).unwrap_or_else(|e| panic!("write {p}: {e}"));
            }

            let n = times.len() as f64;
            let mean = times.iter().sum::<f64>() / n;
            let sd = (times.iter().map(|t| (t - mean).powi(2)).sum::<f64>() / n).sqrt();
            let mut sorted = times.clone();
            sorted.sort_by(f64::total_cmp);
            let pct = |p: f64| sorted[((p * (n - 1.0)).round() as usize).min(sorted.len() - 1)];
            let cpu = if cpu0 < 0.0 { -1.0 } else { (cpu1 - cpu0) / n };
            format!(
                "{{\"impl\":\"rust-inproc\",\"warmup\":{warmup},\"loop\":{iterations},\"first_ms\":{first_ms:.3},\
                 \"mean_ms\":{mean:.3},\"median_ms\":{:.3},\"p95_ms\":{:.3},\"min_ms\":{:.3},\"max_ms\":{:.3},\
                 \"stddev_ms\":{sd:.3},\"cpu_ms_per_doc\":{cpu:.3},\"bytes\":{}}}",
                pct(0.5),
                pct(0.95),
                sorted[0],
                sorted[sorted.len() - 1],
                last.len()
            )
        })
        .expect("spawn the conversion thread")
        .join()
        .unwrap_or_else(|e| std::panic::resume_unwind(e));
    println!("{report}");
}
