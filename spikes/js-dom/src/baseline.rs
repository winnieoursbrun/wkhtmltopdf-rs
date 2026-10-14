//! Baseline binary: the same blitz pipeline as `jsdom-spike` minus Boa.
//! Only used to measure the binary size delta of adding the JS engine.
fn main() {
    let path = std::env::args().nth(1).expect("usage: baseline page.html");
    let html = std::fs::read_to_string(&path).unwrap();
    let config = blitz_dom::DocumentConfig {
        html_parser_provider: Some(std::sync::Arc::new(blitz_html::HtmlProvider)),
        ..Default::default()
    };
    let mut doc = blitz_html::HtmlDocument::from_html(&html, config).into_inner();
    doc.resolve(0.0);
    let body = doc.find_body_node().map(|n| n.id).unwrap();
    doc.mutate().set_inner_html(body, "<p>x</p>");
    doc.resolve(0.0);
    println!("{}", doc.root_element().outer_html());
}
