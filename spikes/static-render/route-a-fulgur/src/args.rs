//! Minimal CLI: page geometry in points, wkhtmltopdf-style defaults.
use std::io::{Read, Write};
use std::path::PathBuf;

pub struct Args {
    pub input: String,
    pub output: String,
    /// Page size in pt.
    pub page_w: f32,
    pub page_h: f32,
    /// Margins in pt: top, right, bottom, left.
    pub margin: [f32; 4],
}

const MM: f32 = 72.0 / 25.4;

impl Args {
    pub fn parse() -> Self {
        let v: Vec<String> = std::env::args().skip(1).collect();
        // wkhtmltopdf defaults: A4, 10mm on every side.
        let mut a = Args { input: String::new(), output: String::new(), page_w: 210.0 * MM, page_h: 297.0 * MM, margin: [10.0 * MM; 4] };
        let mut pos = vec![];
        let mut i = 0;
        while i < v.len() {
            match v[i].as_str() {
                "--page-size" => {
                    i += 1;
                    match v[i].as_str() {
                        "Letter" => { a.page_w = 612.0; a.page_h = 792.0; }
                        _ => { a.page_w = 210.0 * MM; a.page_h = 297.0 * MM; }
                    }
                }
                "--margin-in" => { i += 1; a.margin = [v[i].parse::<f32>().unwrap() * 72.0; 4]; }
                "--margin-mm" => {
                    i += 1;
                    let m: Vec<f32> = v[i].split(',').map(|s| s.parse::<f32>().unwrap() * MM).collect();
                    a.margin = [m[0], m[1], m[2], m[3]];
                }
                s => pos.push(s.to_string()),
            }
            i += 1;
        }
        a.input = pos.first().cloned().unwrap_or("-".into());
        a.output = pos.get(1).cloned().unwrap_or("-".into());
        a
    }
    pub fn read_input(&self) -> String {
        // Lossy UTF-8, like `--encoding UTF-8` on a non-UTF-8 input.
        let mut b = vec![];
        if self.input == "-" { std::io::stdin().read_to_end(&mut b).unwrap(); } else { b = std::fs::read(&self.input).unwrap(); }
        String::from_utf8_lossy(&b).into_owned()
    }
    pub fn base_dir(&self) -> PathBuf {
        if self.input == "-" { std::env::current_dir().unwrap() }
        else { std::fs::canonicalize(&self.input).unwrap().parent().unwrap().to_path_buf() }
    }
    pub fn write_output(&self, pdf: &[u8]) {
        if self.output == "-" { std::io::stdout().write_all(pdf).unwrap() } else { std::fs::write(&self.output, pdf).unwrap() }
    }
}
