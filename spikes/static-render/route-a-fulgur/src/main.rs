//! Route (a): HTML -> PDF entirely through fulgur 0.40.
mod args;
mod route_fulgur;

fn main() {
    let a = args::Args::parse();
    let html = a.read_input();
    let pdf = route_fulgur::render(&a, &html);
    a.write_output(&pdf);
}
