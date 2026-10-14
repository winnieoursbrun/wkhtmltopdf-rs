//! Replays every case of `docs/upstream-fixtures/0.12.6/cli/capture.sh`
//! against the built binary and requires stdout, stderr and the exit code to
//! be byte-identical with the output of the real wkhtmltopdf 0.12.6.
//!
//! The fixtures were captured in the C locale (spec §8), where Qt's
//! `toLocal8Bit` drops non-ASCII characters (`Pablo Ruiz Garca` in
//! `--license`), so the binary runs with `LC_ALL=C`. No case reads stdin and
//! none contains a date or other nondeterministic content, so no
//! normalisation is applied.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

fn fixtures_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/upstream-fixtures/0.12.6/cli")
}

/// `(name, args)` for each `cap <name> <args…>` line of capture.sh. The
/// script uses no quoting, so splitting on whitespace is exact.
fn cases() -> Vec<(String, Vec<String>)> {
    let script = fs::read_to_string(fixtures_dir().join("capture.sh")).expect("capture.sh");
    script
        .lines()
        .filter_map(|l| l.strip_prefix("cap "))
        .map(|l| {
            let mut words = l.split_whitespace().map(str::to_string);
            let name = words.next().expect("case name");
            (name, words.collect())
        })
        .collect()
}

fn show(b: &[u8]) -> String {
    let s = String::from_utf8_lossy(b);
    let head: String = s.chars().take(400).collect();
    format!("{head:?} ({} bytes)", b.len())
}

/// First differing byte offset, for readable failures.
fn first_diff(a: &[u8], b: &[u8]) -> usize {
    a.iter().zip(b).position(|(x, y)| x != y).unwrap_or(a.len().min(b.len()))
}

#[test]
fn cli_fixtures_are_byte_identical() {
    let cases = cases();
    assert_eq!(cases.len(), 23, "capture.sh case count changed");
    let dir = fixtures_dir();
    let workdir = std::env::temp_dir().join(format!("wk-cli-fixtures-{}", std::process::id()));
    fs::create_dir_all(&workdir).unwrap();

    let mut failures = Vec::new();
    for (name, args) in &cases {
        let out = Command::new(env!("CARGO_BIN_EXE_wkhtmltopdf"))
            .args(args)
            .env("LC_ALL", "C")
            .current_dir(&workdir)
            .stdin(Stdio::null())
            .output()
            .expect("run wkhtmltopdf");
        let want_out = fs::read(dir.join(format!("{name}.stdout"))).unwrap();
        let want_err = fs::read(dir.join(format!("{name}.stderr"))).unwrap();
        let want_rc: i32 =
            fs::read_to_string(dir.join(format!("{name}.rc"))).unwrap().trim().parse().unwrap();
        let rc = out.status.code().expect("exit code");
        if out.stdout != want_out {
            let at = first_diff(&out.stdout, &want_out);
            failures.push(format!(
                "{name}: stdout differs at byte {at}\n  got:  {}\n  want: {}",
                show(&out.stdout[at.min(out.stdout.len())..]),
                show(&want_out[at.min(want_out.len())..])
            ));
        }
        if out.stderr != want_err {
            let at = first_diff(&out.stderr, &want_err);
            failures.push(format!(
                "{name}: stderr differs at byte {at}\n  got:  {}\n  want: {}",
                show(&out.stderr[at.min(out.stderr.len())..]),
                show(&want_err[at.min(want_err.len())..])
            ));
        }
        if rc != want_rc {
            failures.push(format!("{name}: exit code {rc}, want {want_rc}"));
        }
    }
    let _ = fs::remove_dir_all(&workdir);
    assert!(failures.is_empty(), "{} mismatch(es):\n{}", failures.len(), failures.join("\n"));
}
