//! Documentation sections of wkhtmltopdf. Ports upstream
//! `src/pdf/pdfdocparts.cc`, with the texts of the 0.12.6 release where they
//! differ from the 0.12.7-dev source (spec §9: qt-project.org URLs,
//! "upper left corner", the Outlines and TOC wording).

use wk_cli::Outputter;
use wk_cli::commandlineparserbase::FULL_VERSION;

/// `outputManName`.
pub fn output_man_name(o: &mut dyn Outputter) {
    o.begin_section("Name");
    o.paragraph("wkhtmltopdf - html to pdf converter");
    o.end_section();
}

/// `outputSynopsis` (also prints the "Document objects" section).
pub fn output_synopsis(o: &mut dyn Outputter) {
    o.begin_section("Synopsis");
    o.verbatim("wkhtmltopdf [GLOBAL OPTION]... [OBJECT]... <output file>\n");
    o.end_section();

    o.begin_section("Document objects");
    o.begin_paragraph();
    o.text(concat!(
        "wkhtmltopdf is able to put several objects into the output file, an object is either ",
        "a single webpage, a cover webpage or a table of contents.  The objects are put into ",
        "the output document in the order they are specified on the command line, options can ",
        "be specified on a per object basis or in the global options area. Options from the "
    ));
    o.section_link("Global Options");
    o.text(" section can only be placed in the global options area.");
    o.end_paragraph();

    o.paragraph("A page objects puts the content of a single webpage into the output document.");
    o.verbatim("(page)? <input url/file name> [PAGE OPTION]...");
    o.begin_paragraph();
    o.text(concat!(
        "Options for the page object can be placed in the global options and the page ",
        "options areas. The applicable options can be found in the "
    ));
    o.section_link("Page Options");
    o.text(" and ");
    o.section_link("Headers And Footer Options");
    o.text(" sections.");
    o.end_paragraph();

    o.paragraph(concat!(
        "A cover objects puts the content of a single webpage into the output document, ",
        "the page does not appear in the table of contents, and does not have headers and footers."
    ));
    o.verbatim("cover <input url/file name> [PAGE OPTION]...");
    o.paragraph("All options that can be specified for a page object can also be specified for a cover.");

    o.paragraph("A table of contents object inserts a table of contents into the output document.");
    o.verbatim("toc [TOC OPTION]...");
    o.begin_paragraph();
    o.text(concat!(
        "All options that can be specified for a page object can also be specified for a toc, ",
        "further more the options from the "
    ));
    o.section_link("TOC Options");
    o.text(concat!(
        " section can also be applied. The table of contents is generated via XSLT which means ",
        "that it can be styled to look however you want it to look. To get an idea of how to ",
        "do this you can dump the default xslt document by supplying the --dump-default-toc-xsl, ",
        "and the outline it works on by supplying --dump-outline, see the "
    ));
    o.section_link("Outline Options");
    o.text(" section.");
    o.end_paragraph();

    o.end_section();
}

/// `outputDescripton` (patched-Qt variant).
pub fn output_description(o: &mut dyn Outputter) {
    o.begin_section("Description");
    o.begin_paragraph();
    o.text("Converts one or more HTML pages into a PDF document, ");
    o.text("using wkhtmltopdf patched qt.");
    o.end_paragraph();
    o.end_section();
}

/// `outputNotPatched`.
pub fn output_not_patched(o: &mut dyn Outputter, sure: bool) {
    o.begin_section("Reduced Functionality");
    if sure {
        o.paragraph(concat!(
            "This version of wkhtmltopdf has been compiled against a version of ",
            "QT without the wkhtmltopdf patches. Therefore some features are missing, ",
            "if you need these features please use the static version."
        ));
    } else {
        o.paragraph(concat!(
            "Some versions of wkhtmltopdf are compiled against a version of QT ",
            "without the wkhtmltopdf patches. These versions are missing some features, ",
            "you can find out if your version of wkhtmltopdf is one of these by running wkhtmltopdf --version ",
            "if your version is against an unpatched QT, you can use the static version to get all functionality."
        ));
    }

    o.paragraph("Currently the list of features only supported with patch QT includes:");
    o.begin_list(false);
    o.list_item("Printing more than one HTML document into a PDF file.");
    o.list_item("Running without an X11 server.");
    o.list_item("Adding a document outline to the PDF file.");
    o.list_item("Adding headers and footers to the PDF file.");
    o.list_item("Generating a table of contents.");
    o.list_item("Adding links in the generated PDF file.");
    o.list_item("Printing using the screen media-type.");
    o.list_item("Disabling the smart shrink feature of WebKit.");
    o.end_list();
    o.end_section();
}

/// `outputPageBreakDoc`.
pub fn output_page_break_doc(o: &mut dyn Outputter) {
    o.begin_section("Page Breaking");
    o.paragraph(concat!(
        "The current page breaking algorithm of WebKit leaves much to be desired. ",
        "Basically WebKit will render everything into one long page, and then cut it up ",
        "into pages. This means that if you have two columns of text where one is ",
        "vertically shifted by half a line. Then WebKit will cut a line into to pieces ",
        "display the top half on one page. And the bottom half on another page. ",
        "It will also break image in two and so on.  If you are using the patched version of ",
        "QT you can use the CSS page-break-inside property to remedy this somewhat. ",
        "There is no easy solution to this problem, until this is solved try organizing ",
        "your HTML documents such that it contains many lines on which pages can be cut ",
        "cleanly."
    ));
    o.end_section();
}

/// `outputHeaderFooterDoc`.
pub fn output_header_footer_doc(o: &mut dyn Outputter) {
    o.begin_section("Footers And Headers");
    o.paragraph(concat!(
        "Headers and footers can be added to the document by the --header-* and --footer* ",
        "arguments respectively.  In header and footer text string supplied to e.g. --header-left, ",
        "the following variables will be substituted."
    ));
    o.verbatim(concat!(
        " * [page]       Replaced by the number of the pages currently being printed\n",
        " * [frompage]   Replaced by the number of the first page to be printed\n",
        " * [topage]     Replaced by the number of the last page to be printed\n",
        " * [webpage]    Replaced by the URL of the page being printed\n",
        " * [section]    Replaced by the name of the current section\n",
        " * [subsection] Replaced by the name of the current subsection\n",
        " * [date]       Replaced by the current date in system local format\n",
        " * [isodate]    Replaced by the current date in ISO 8601 extended format\n",
        " * [time]       Replaced by the current time in system local format\n",
        " * [title]      Replaced by the title of the of the current page object\n",
        " * [doctitle]   Replaced by the title of the output document\n",
        " * [sitepage]   Replaced by the number of the page in the current site being converted\n",
        " * [sitepages]  Replaced by the number of pages in the current site being converted\n",
        "\n"
    ));
    o.paragraph(concat!(
        "As an example specifying --header-right \"Page [page] of [topage]\", ",
        "will result in the text \"Page x of y\" where x is the number of the ",
        "current page and y is the number of the last page, to appear in the upper ",
        "left corner in the document."
    ));
    o.paragraph(concat!(
        "Headers and footers can also be supplied with HTML documents. As an example one ",
        "could specify --header-html header.html, and use the following content in header.html:"
    ));
    o.verbatim(concat!(
        "<!DOCTYPE html>\n",
        "<html><head><script>\n",
        "function subst() {\n",
        "    var vars = {};\n",
        "    var query_strings_from_url = document.location.search.substring(1).split('&');\n",
        "    for (var query_string in query_strings_from_url) {\n",
        "        if (query_strings_from_url.hasOwnProperty(query_string)) {\n",
        "            var temp_var = query_strings_from_url[query_string].split('=', 2);\n",
        "            vars[temp_var[0]] = decodeURI(temp_var[1]);\n",
        "        }\n",
        "    }\n",
        "    var css_selector_classes = ['page', 'frompage', 'topage', 'webpage', 'section', 'subsection', 'date', 'isodate', 'time', 'title', 'doctitle', 'sitepage', 'sitepages'];\n",
        "    for (var css_class in css_selector_classes) {\n",
        "        if (css_selector_classes.hasOwnProperty(css_class)) {\n",
        "            var element = document.getElementsByClassName(css_selector_classes[css_class]);\n",
        "            for (var j = 0; j < element.length; ++j) {\n",
        "                element[j].textContent = vars[css_selector_classes[css_class]];\n",
        "            }\n",
        "        }\n",
        "    }\n",
        "}\n",
        "</script></head><body style=\"border:0; margin: 0;\" onload=\"subst()\">\n",
        "<table style=\"border-bottom: 1px solid black; width: 100%\">\n",
        "  <tr>\n",
        "    <td class=\"section\"></td>\n",
        "    <td style=\"text-align:right\">\n",
        "      Page <span class=\"page\"></span> of <span class=\"topage\"></span>\n",
        "    </td>\n",
        "  </tr>\n",
        "</table>\n",
        "</body></html>\n",
        "\n"
    ));
    o.paragraph(concat!(
        "As can be seen from the example, the arguments are sent to the header/footer ",
        "html documents in get fashion."
    ));
    o.end_section();
}

/// `outputTableOfContentDoc`.
pub fn output_table_of_content_doc(o: &mut dyn Outputter) {
    o.begin_section("Table Of Contents");
    o.paragraph(concat!(
        "A table of contents can be added to the document by adding a toc object ",
        "to the command line. For example:"
    ));
    o.verbatim("wkhtmltopdf toc https://qt-project.org/doc/qt-4.8/qstring.html qstring.pdf\n");
    o.paragraph(concat!(
        "The table of contents is generated based on the H tags in the input ",
        "documents. First a XML document is generated, then it is converted to ",
        "HTML using XSLT."
    ));
    o.paragraph(concat!(
        "The generated XML document can be viewed by dumping it to a file using ",
        "the --dump-outline switch. For example:"
    ));
    o.verbatim(
        "wkhtmltopdf --dump-outline toc.xml https://qt-project.org/doc/qt-4.8/qstring.html qstring.pdf\n",
    );
    o.paragraph("The XSLT document can be specified using the --xsl-style-sheet switch. For example:");
    o.verbatim("wkhtmltopdf toc --xsl-style-sheet my.xsl https://qt-project.org/doc/qt-4.8/qstring.html qstring.pdf\n");
    o.paragraph(concat!(
        "The --dump-default-toc-xsl switch can be used to dump the default ",
        "XSLT style sheet to stdout. This is a good start for writing your ",
        "own style sheet"
    ));
    o.verbatim("wkhtmltopdf --dump-default-toc-xsl");
    o.paragraph(concat!(
        "The XML document is in the namespace ",
        "\"http://wkhtmltopdf.org/outline\" ",
        "it has a root node called \"outline\" which contains a number of ",
        "\"item\" nodes. An item can contain any number of item. These are the ",
        "outline subsections to the section the item represents. A item node ",
        "has the following attributes:"
    ));
    o.begin_list(false);
    o.list_item("\"title\" the name of the section.");
    o.list_item("\"page\" the page number the section occurs on.");
    o.list_item("\"link\" a URL that links to the section.");
    o.list_item("\"backLink\" the name of the anchor the section will link back to.");
    o.end_list();

    o.paragraph(concat!(
        "The remaining TOC options only affect the default style sheet ",
        "so they will not work when specifying a custom style sheet."
    ));
    o.end_section();
}

/// `outputOutlineDoc`.
pub fn output_outline_doc(o: &mut dyn Outputter) {
    o.begin_section("Outlines");
    o.begin_paragraph();
    o.text(concat!(
        "Wkhtmltopdf with patched qt has support for PDF outlines also known as ",
        "book marks, this can be enabled by specifying the --outline switch. ",
        "The outlines are generated based on the <h?> tags, for a in-depth ",
        "description of how this is done see the "
    ));
    o.section_link("Table Of Contents");
    o.text(" section. ");
    o.end_paragraph();
    o.paragraph(concat!(
        "The outline tree can sometimes be very deep, if the <h?> tags where ",
        "spread to generous in the HTML document.  The --outline-depth switch can ",
        "be used to bound this."
    ));
    o.end_section();
}

/// `outputContact`.
pub fn output_contact(o: &mut dyn Outputter) {
    o.begin_section("Contact");
    o.begin_paragraph();
    o.text("If you experience bugs or want to request new features please visit ");
    o.link("https://wkhtmltopdf.org/support.html");
    o.end_paragraph();
    o.end_section();
}

/// `outputDocStart`.
pub fn output_doc_start(o: &mut dyn Outputter) {
    o.begin_section(&format!("wkhtmltopdf {FULL_VERSION} Manual"));
    o.paragraph(concat!(
        "This file documents wkhtmltopdf, a program capable of converting html ",
        "documents into PDF documents."
    ));
    o.end_section();
}

/// `outputArgsFromStdin`.
pub fn output_args_from_stdin(o: &mut dyn Outputter) {
    o.begin_section("Reading arguments from stdin");
    o.paragraph(concat!(
        "If you need to convert a lot of pages in a batch, and you feel that wkhtmltopdf ",
        "is a bit too slow to start up, then you should try --read-args-from-stdin,"
    ));
    o.paragraph(concat!(
        "When --read-args-from-stdin each line of input sent to wkhtmltopdf on stdin ",
        "will act as a separate invocation of wkhtmltopdf, with the arguments specified ",
        "on the given line combined with the arguments given to wkhtmltopdf"
    ));
    o.paragraph("For example one could do the following:");
    o.verbatim(concat!(
        "echo \"https://qt-project.org/doc/qt-4.8/qapplication.html qapplication.pdf\" >> cmds\n",
        "echo \"cover google.com https://en.wikipedia.org/wiki/Qt_(software) qt.pdf\" >> cmds\n",
        "wkhtmltopdf --read-args-from-stdin --book < cmds\n"
    ));
    o.end_section();
}

/// `outputInstallation`.
pub fn output_installation(o: &mut dyn Outputter) {
    o.begin_section("Installation");
    o.paragraph(concat!(
        "There are several ways to install wkhtmltopdf.  You can download a ",
        "already compiled binary, or you can compile wkhtmltopdf yourself. ",
        "On windows the easiest way to install wkhtmltopdf is to download ",
        "the latest installer. On Linux you can download the latest static ",
        "binary, however you still need to install some other pieces of ",
        "software, to learn more about this read the static version section ",
        "of the manual."
    ));
    o.end_section();
}

/// `outputPageSizes`.
pub fn output_page_sizes(o: &mut dyn Outputter) {
    o.begin_section("Page sizes");
    o.begin_paragraph();
    o.text(concat!(
        "The default page size of the rendered document is A4, but by using the --page-size ",
        "option this can be changed to almost anything else, such as: A3, Letter and Legal.  ",
        "For a full list of supported pages sizes please see "
    ));
    o.link("https://qt-project.org/doc/qt-4.8/qprinter.html#PaperSize-enum");
    o.text(".");
    o.end_paragraph();
    o.paragraph(concat!(
        "For a more fine grained control over the page size the ",
        "--page-height and --page-width options may be used"
    ));
    o.end_section();
}

/// `outputExamples`.
pub fn output_examples(o: &mut dyn Outputter) {
    o.begin_section("Examples");
    o.paragraph("This section presents a number of examples of how to invoke wkhtmltopdf.");
    o.paragraph("To convert a remote HTML file to PDF:");
    o.verbatim("wkhtmltopdf https://www.google.com google.pdf\n");
    o.paragraph("To convert a local HTML file to PDF:");
    o.verbatim("wkhtmltopdf my.html my.pdf\n");
    o.paragraph("Produce the eler2.pdf sample file:");
    o.verbatim("wkhtmltopdf -H  https://geekz.co.uk/lovesraymond/archive/eler-highlights-2008 eler2.pdf\n");
    o.paragraph("Printing a book with a table of contents:");
    o.verbatim("wkhtmltopdf -H cover cover.html toc chapter1.html chapter2.html chapter3.html book.pdf\n");
    o.end_section();
}
