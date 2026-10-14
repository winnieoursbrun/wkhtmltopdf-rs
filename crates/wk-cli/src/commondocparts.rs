//! Documentation sections shared by the tools. Ports upstream
//! `src/shared/commondocparts.cc`, and `CommandLineParserBase::license` from
//! `src/shared/commandlineparserbase.cc`.
//!
//! `AUTHORS` and `LICENSE` are the Qt resources of upstream
//! `wkhtmltopdf.qrc`. `QTextStream::readAll` drops the UTF-8 BOM of
//! `AUTHORS`, so the embedded copy has it removed.

use crate::commandlineparserbase::app_version;
use crate::outputter::{Fd, Outputter, Streams};
use crate::textoutputter::TextOutputter;

/// Content of the `:/AUTHORS` resource (BOM stripped).
pub const AUTHORS: &str = include_str!("../res/AUTHORS");
/// Content of the `:/LICENSE` resource.
pub const LICENSE: &str = include_str!("../res/LICENSE");

/// `outputName`.
pub fn output_name(o: &mut dyn Outputter, app_name: &str) {
    o.begin_section("Name");
    o.paragraph(&format!("{} {}", app_name, app_version()));
    o.end_section();
}

/// `outputLicense`.
pub fn output_license(o: &mut dyn Outputter) {
    o.begin_section("License");
    o.paragraph("Copyright (c) 2010-2020 wkhtmltopdf authors");
    o.verbatim(LICENSE);
    o.end_section();
}

/// `outputAuthors`.
pub fn output_authors(o: &mut dyn Outputter) {
    o.begin_section("Authors");
    o.verbatim(AUTHORS);
    o.end_section();
}

/// `outputStaticProblems`.
pub fn output_static_problems(o: &mut dyn Outputter) {
    o.begin_section("Static version");
    o.begin_paragraph();
    o.text("On the wkhtmltopdf website you can download a static version of wkhtmltopdf ");
    o.link("https://wkhtmltopdf.org/downloads.html");
    o.text(". This static binary will work on most systems and comes with a build in patched QT.");
    o.end_paragraph();

    o.begin_paragraph();
    o.text(concat!(
        "Unfortunately the static binary is not particularly static, on Linux it depends ",
        "on both glibc and openssl, furthermore you will need to have an xserver installed ",
        "but not necessary running. You will need to have different fonts install including ",
        "xfonts-scalable (Type1), and msttcorefonts."
    ));
    o.end_paragraph();
    o.end_section();
}

/// `outputProxyDoc`.
pub fn output_proxy_doc(o: &mut dyn Outputter) {
    o.begin_section("Specifying A Proxy");
    o.paragraph(concat!(
        "By default proxy information will be read from the environment",
        " variables: proxy, all_proxy and http_proxy, proxy options can",
        " also by specified with the -p switch"
    ));
    o.verbatim(concat!(
        "<type> := \"http://\" | \"socks5://\"\n",
        "<serif> := <username> (\":\" <password>)? \"@\"\n",
        "<proxy> := \"None\" | <type>? <string>? <host> (\":\" <port>)?\n"
    ));
    o.paragraph("Here are some examples (In case you are unfamiliar with the BNF):");
    o.verbatim("http://user:password@myproxyserver:8080\nsocks5://myproxyserver\nNone\n");
    o.end_section();
}

/// `CommandLineParserBase::license` (`--license`).
pub fn license(s: &mut Streams, fd: Fd, app_name: &str) {
    let mut o = TextOutputter::new(s, fd, false);
    output_name(&mut o, app_name);
    output_authors(&mut o);
    output_license(&mut o);
}
