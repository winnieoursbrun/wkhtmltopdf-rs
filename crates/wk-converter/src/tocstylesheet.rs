//! Default XSLT style sheet of the table of contents. Ports upstream
//! `src/lib/tocstylesheet.cc` (`dumpDefaultTOCStyleSheet`).
//!
//! Used by `--dump-default-toc-xsl` and by the TOC objects (`toc.rs`).

use wk_settings::TableOfContent;
use wk_settings::qt::number_g;

/// The default TOC style sheet for the given settings.
pub fn dump_default_toc_style_sheet(s: &TableOfContent) -> String {
    let mut o = String::new();
    let mut l = |line: &str| {
        o.push_str(line);
        o.push('\n');
    };
    l(r#"<?xml version="1.0" encoding="UTF-8"?>"#);
    l(r#"<xsl:stylesheet version="2.0""#);
    l(r#"                xmlns:xsl="http://www.w3.org/1999/XSL/Transform""#);
    l(r#"                xmlns:outline="http://wkhtmltopdf.org/outline""#);
    l(r#"                xmlns="http://www.w3.org/1999/xhtml">"#);
    l(r#"  <xsl:output doctype-public="-//W3C//DTD XHTML 1.0 Strict//EN""#);
    l(r#"              doctype-system="http://www.w3.org/TR/xhtml1/DTD/xhtml1-transitional.dtd""#);
    l(r#"              indent="yes" />"#);
    l(r#"  <xsl:template match="outline:outline">"#);
    l("    <html>");
    l("      <head>");
    l(&format!("        <title>{}</title>", s.caption_text));
    l(r#"        <meta http-equiv="Content-Type" content="text/html; charset=utf-8" />"#);
    l("        <style>");
    l("          h1 {");
    l("            text-align: center;");
    l("            font-size: 20px;");
    l("            font-family: arial;");
    l("          }");
    if s.use_dotted_lines {
        l("          div {border-bottom: 1px dashed rgb(200,200,200);}");
    }
    l("          span {float: right;}");
    l("          li {list-style: none;}");
    l("          ul {");
    l("            font-size: 20px;");
    l("            font-family: arial;");
    l("          }");
    // float * double, printed by QTextStream with 6 significant digits.
    l(&format!("          ul ul {{font-size: {}%; }}", number_g(f64::from(s.font_scale) * 100.0)));
    l("          ul {padding-left: 0em;}");
    l(&format!("          ul ul {{padding-left: {};}}", s.indentation));
    l("          a {text-decoration:none; color: black;}");
    l("        </style>");
    l("      </head>");
    l("      <body>");
    l(&format!("        <h1>{}</h1>", s.caption_text));
    l(r#"        <ul><xsl:apply-templates select="outline:item/outline:item"/></ul>"#);
    l("      </body>");
    l("    </html>");
    l("  </xsl:template>");
    l(r#"  <xsl:template match="outline:item">"#);
    l("    <li>");
    l(r#"      <xsl:if test="@title!=''">"#);
    l("        <div>");
    l("          <a>");
    if s.forward_links {
        l(r#"            <xsl:if test="@link">"#);
        l(r#"              <xsl:attribute name="href"><xsl:value-of select="@link"/></xsl:attribute>"#);
        l("            </xsl:if>");
    }
    l(r#"            <xsl:if test="@backLink">"#);
    l(r#"              <xsl:attribute name="name"><xsl:value-of select="@backLink"/></xsl:attribute>"#);
    l("            </xsl:if>");
    l(r#"            <xsl:value-of select="@title" /> "#);
    l("          </a>");
    l(r#"          <span> <xsl:value-of select="@page" /> </span>"#);
    l("        </div>");
    l("      </xsl:if>");
    l("      <ul>");
    l("        <xsl:comment>added to prevent self-closing tags in QtXmlPatterns</xsl:comment>");
    l(r#"        <xsl:apply-templates select="outline:item"/>"#);
    l("      </ul>");
    l("    </li>");
    l("  </xsl:template>");
    l("</xsl:stylesheet>");
    o
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn options_change_the_style_sheet() {
        let d = dump_default_toc_style_sheet(&TableOfContent::default());
        assert!(d.contains("ul ul {font-size: 80%; }"));
        assert!(d.contains("dashed"));
        assert!(d.contains("@link"));
        let t = TableOfContent {
            use_dotted_lines: false,
            forward_links: false,
            font_scale: 0.5,
            caption_text: "Index".into(),
            indentation: "2em".into(),
            ..TableOfContent::default()
        };
        let d = dump_default_toc_style_sheet(&t);
        assert!(!d.contains("dashed"));
        assert!(!d.contains("\"@link\""));
        assert!(d.contains("font-size: 50%"));
        assert!(d.contains("<h1>Index</h1>"));
        assert!(d.contains("padding-left: 2em;"));
    }
}
