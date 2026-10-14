//! Route (b): html5ever -> blitz-dom (stylo cascade, taffy/parley layout) -> own paginator -> krilla.
//! Usage: spike <in.html|-> <out.pdf|-> [--page-size A4|Letter] [--margin-in N | --margin-mm T,R,B,L]
mod args;
mod route_blitz;
mod paginate;
mod pdf_painter;

fn main() {
    let a = args::Args::parse();
    let html = a.read_input();
    let pdf = route_blitz::render(&a, &html);
    a.write_output(&pdf);
}
