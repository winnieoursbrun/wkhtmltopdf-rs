#!/usr/bin/env python3
"""Generate the HTML cases of the rendering-fidelity corpus (tests/fidelity/cases).

The generated files are committed; this script is only the convenient way to
(re)write them. Edit a case here, run `python3 -I tests/fidelity/build.py`, then
regenerate its reference with `make-refs.sh <case>`.

Every case is small and focused on one area of rendering, except the `doc-*`
documents which mimic what pdfkit users render (invoice, report, letter, ...).
Assets live in ../assets (generated images, an SVG, a Bootstrap-3-like style
sheet); the DejaVu faces of crates/wk-fonts/fonts serve as web fonts. No
network access is ever needed.
"""

import os

HERE = os.path.dirname(os.path.abspath(__file__))
OUT = os.path.join(HERE, "cases")

LOREM = (
    "The quick brown fox jumps over the lazy dog while the committee reviews the quarterly "
    "figures. Several invoices remain unpaid, and the accounting team has asked every department "
    "to confirm its open purchase orders before the end of the month. Meanwhile the warehouse "
    "reports that shipments to the northern region were delayed by roughly two days."
)
SHORT = "Sphinx of black quartz, judge my vow. Pack my box with five dozen liquor jugs."

CASES = {}


def page(name, body, style="", title=None, head=""):
    CASES[name] = (
        "<!DOCTYPE html>\n<html><head><meta charset=\"utf-8\"><title>%s</title>\n%s"
        "<style>\n%s\n</style>\n</head>\n<body>\n%s\n</body></html>\n"
        % (title or name, head, style.strip(), body.strip())
    )


def paras(n, text=LOREM):
    return "\n".join("<p>%s</p>" % text for _ in range(n))


# ---------------------------------------------------------------- typography

fams = [
    "serif", "sans-serif", "monospace", "cursive", "fantasy", "Arial", "Helvetica",
    "Times New Roman", "Times", "Courier New", "Courier", "Georgia", "Verdana",
    "DejaVu Sans", "DejaVu Serif", "DejaVu Sans Mono", "NoSuchFont", "NoSuchFont, serif",
]
page("typo-families", "\n".join(
    '<p style="font-family: %s"><b>%s</b>: %s</p>' % (f if "," in f or " " not in f else '"%s"' % f, f, SHORT)
    for f in fams) + '\n<p>No family set (UA default): %s</p>' % SHORT,
    "body { font-size: 14px; } p { margin: 4px 0; }")

rows = []
for fam in ["sans-serif", "serif", "monospace"]:
    for w in ["100", "300", "normal", "500", "600", "bold", "800", "900"]:
        rows.append('<div style="font-family:%s;font-weight:%s">%s %s: Hamburgefonstiv 0123</div>' % (fam, w, fam, w))
    rows.append('<div style="font-family:%s;font-style:italic">%s italic: Hamburgefonstiv 0123</div>' % (fam, fam))
    rows.append('<div style="font-family:%s;font-style:oblique;font-weight:bold">%s bold oblique: Hamburgefonstiv</div>' % (fam, fam))
rows.append('<p>Nested: normal <b>bold <i>bold italic <span style="font-weight:lighter">lighter</span></i></b> '
            '<strong>strong <em>em</em></strong> <span style="font-weight:bolder">bolder</span></p>')
page("typo-weights", "\n".join(rows), "body { font-size: 15px; } div { margin: 1px 0; }")

sizes = ["xx-small", "x-small", "small", "medium", "large", "x-large", "xx-large",
         "9px", "11px", "13px", "15px", "8pt", "10pt", "11pt", "12pt", "14pt", "0.8em", "1.3em", "150%", "1.5rem"]
page("typo-sizes", "\n".join('<div style="font-size:%s">%s: %s</div>' % (s, s, SHORT) for s in sizes)
     + '\n<p style="font-size:12px">Parent 12px <span style="font-size:smaller">smaller <span style="font-size:smaller">smaller</span></span> '
     '<span style="font-size:larger">larger <span style="font-size:larger">larger</span></span></p>',
     "body { font-family: sans-serif; } div { white-space: nowrap; overflow: hidden; }")

lh = ["normal", "1", "1.2", "1.5", "2", "14px", "22px", "150%", "1.3em", "0.8"]
page("typo-line-height", "\n".join(
    '<div class="b" style="line-height:%s"><b>line-height %s</b> %s</div>' % (v, v, LOREM[:240]) for v in lh)
    + '\n<div class="b" style="font-size:20px;line-height:1.2">Mixed <span style="font-size:32px">BIG</span> and '
    '<span style="font-size:10px">small</span> inline sizes on one line, then wrapping onto a second line to show the line box.</div>',
    "body { font-family: serif; font-size: 13px; } .b { background: #eef3fb; margin: 0 0 6px; width: 520px; }")

ls = ["-1px", "-0.5px", "0", "0.5px", "1px", "2px", "0.2em", "5px"]
ws = ["-2px", "0", "4px", "10px", "1em"]
page("typo-spacing",
     "\n".join('<p style="letter-spacing:%s">letter-spacing %s: %s</p>' % (v, v, SHORT) for v in ls)
     + "\n" + "\n".join('<p style="word-spacing:%s">word-spacing %s: %s</p>' % (v, v, SHORT) for v in ws)
     + '\n<p style="letter-spacing:3px;word-spacing:8px;text-align:justify;width:400px">Both spacings, justified: ' + LOREM[:200] + '</p>',
     "body { font-family: sans-serif; font-size: 13px; } p { margin: 3px 0; }")

page("typo-align", "\n".join(
    '<h4>text-align: %s</h4><p style="text-align:%s">%s</p>' % (a, a, LOREM) for a in ["left", "right", "center", "justify"])
    + '<h4>text-indent 40px, justify</h4><p style="text-indent:40px;text-align:justify">%s %s</p>' % (LOREM, SHORT)
    + '<h4>text-indent -20px (hanging), padding-left 20px</h4><p style="text-indent:-20px;padding-left:20px">%s</p>' % LOREM
    + '<h4>align attribute</h4><p align="right">Right via the align attribute.</p><div align="center">Centred via the align attribute.</div><center>center element</center>',
    "body { font-family: serif; font-size: 13px; width: 560px; } h4 { margin: 8px 0 2px; font-family: sans-serif; }")

page("typo-decoration", """
<p><span style="text-decoration:underline">underline</span> <span style="text-decoration:overline">overline</span>
<span style="text-decoration:line-through">line-through</span> <span style="text-decoration:underline overline line-through">all three</span>
<u>u element</u> <s>s element</s> <strike>strike</strike> <del>del</del> <ins>ins</ins></p>
<p style="color:#c00;text-decoration:underline">Red underlined <span style="color:#00c">blue inside keeps red line</span> done.</p>
<p style="font-size:28px;text-decoration:underline">Large underline gjpqy</p>
<p><a href="#x">A default link</a>, <a href="#x" style="text-decoration:none">no underline</a>, <a href="#x" style="color:#080">green link</a>.</p>
<p style="text-transform:uppercase">uppercase: the quick brown fox straße</p>
<p style="text-transform:lowercase">LOWERCASE: THE QUICK BROWN FOX</p>
<p style="text-transform:capitalize">capitalize: the quick brown fox o'neil e-mail</p>
<p style="font-variant:small-caps">Small Caps: The Quick Brown Fox 2026</p>
<p style="font-size:20px;text-decoration:underline;letter-spacing:4px">Spaced underline</p>
""", "body { font-family: sans-serif; font-size: 15px; }")

page("typo-whitespace", """
<div class="b" style="white-space:normal">normal:    several     spaces
and a newline	and a tab.</div>
<div class="b" style="white-space:nowrap">nowrap: %(l)s</div>
<div class="b" style="white-space:pre">pre:    several     spaces
  indented line
	tab-indented line</div>
<div class="b" style="white-space:pre-wrap">pre-wrap:    keeps   spaces and wraps %(l)s</div>
<div class="b" style="white-space:pre-line">pre-line:    collapses   spaces
but keeps
newlines</div>
<pre>&lt;pre&gt; element
    def f(x):
        return x * 2   # comment</pre>
<div class="b">Long word: Supercalifragilisticexpialidocious_and_some_more_characters_without_any_break_at_all_xxxxxxxxxxxxxxxx</div>
<div class="b" style="word-wrap:break-word">break-word: Supercalifragilisticexpialidocious_and_some_more_characters_without_any_break_at_all_xxxxxxxxxxxxxxxx</div>
<div class="b">Line<br>breaks<br><br>double break, then&nbsp;&nbsp;&nbsp;three nbsp, soft&shy;hyphen&shy;ated word&shy;inside</div>
""" % {"l": LOREM[:180]}, "body { font-family: sans-serif; font-size: 13px; } .b { width: 360px; border: 1px solid #aaa; margin: 4px 0; } pre { background: #f4f4f4; }")

va = ["baseline", "sub", "super", "top", "text-top", "middle", "bottom", "text-bottom", "4px", "-4px", "50%"]
page("typo-vertical-align",
     '<p class="l">' + " ".join(
         'x<span style="vertical-align:%s;font-size:11px;background:#fde">%s</span>' % (v, v) for v in va) + "</p>\n"
     + '<p class="l">' + " ".join(
         '<img src="../assets/bullet.png" style="vertical-align:%s;width:14px;height:14px">%s' % (v, v) for v in va[:8]) + "</p>\n"
     + '<p class="l">H<sub>2</sub>O, E = mc<sup>2</sup>, x<sub>i</sub><sup>n</sup>, 1<sup>st</sup>, <small>small</small> <big>big</big>, '
       'footnote<sup><a href="#n">[1]</a></sup></p>\n'
     + '<p class="l" style="line-height:1.5">With line-height 1.5: a<sup>2</sup> + b<sup>2</sup> = c<sup>2</sup>, log<sub>10</sub>.</p>\n'
     + '<p class="l"><span style="font-size:30px">Big</span><span style="font-size:10px;vertical-align:top">top</span>'
       '<span style="font-size:10px;vertical-align:bottom">bottom</span><span style="font-size:10px;vertical-align:middle">middle</span></p>',
     "body { font-family: serif; font-size: 16px; } .l { background: #eef; line-height: normal; }")

page("typo-elements", """
<h1>Heading 1</h1><h2>Heading 2</h2><h3>Heading 3</h3><h4>Heading 4</h4><h5>Heading 5</h5><h6>Heading 6</h6>
<p>Paragraph with <strong>strong</strong>, <em>em</em>, <b>b</b>, <i>i</i>, <code>code</code>, <kbd>kbd</kbd>, <samp>samp</samp>,
<var>var</var>, <cite>cite</cite>, <q>quote</q>, <abbr title="abbreviation">abbr</abbr>, <dfn>dfn</dfn>, <tt>tt</tt>, <mark>mark</mark>.</p>
<blockquote>Blockquote: %s</blockquote>
<hr>
<address>Address element<br>12 Example Street<br>75000 Paris</address>
<dl><dt>Term</dt><dd>Definition of the term.</dd><dt>Another term</dt><dd>Another definition, longer, wrapping over the line if needed to show the indent.</dd></dl>
<hr style="width:50%%"><hr noshade size="4"><hr style="border:0;border-top:2px dashed #888">
""" % LOREM, "")

page("typo-colors", """
<p style="color:red">named red</p><p style="color:#0a0">hex #0a0</p><p style="color:#1e3c8cff">hex8 (ignored in 2012?)</p>
<p style="color:rgb(200,100,0)">rgb()</p><p style="color:rgba(0,0,255,0.5)">rgba 0.5</p><p style="color:hsl(280,60%,40%)">hsl()</p>
<p style="color:hsla(120,100%,25%,0.6)">hsla 0.6</p>
<p style="text-shadow:2px 2px 0 #f80">text-shadow hard</p><p style="text-shadow:1px 1px 3px rgba(0,0,0,.6)">text-shadow blurred</p>
<p style="background:#222;color:#fff;padding:4px">white on dark</p>
<p style="color:transparent;background:#ddd">transparent text</p>
<p style="opacity:0.4;background:#09c;color:#fff;padding:4px">opacity 0.4 box</p>
""", "body { font-family: sans-serif; font-size: 18px; font-weight: bold; } p { margin: 4px 0; }")

# ---------------------------------------------------------------- fonts

FONTDIR = "../../../crates/wk-fonts/fonts"
page("fonts-webfont", """
<p class="a">@font-face "BrandSerif" (DejaVuSerif.ttf): %(s)s</p>
<p class="a" style="font-weight:bold">@font-face "BrandSerif" bold (DejaVuSerif-Bold.ttf): %(s)s</p>
<p class="b">@font-face "CodeFace" (DejaVuSansMono.ttf): %(s)s</p>
<p class="c">@font-face with a missing file, fallback monospace: %(s)s</p>
<p class="d">@font-face family "DejaVu Sans" overridden by DejaVuSerif.ttf: %(s)s</p>
<p class="a" style="font-style:italic">BrandSerif italic (synthesised): %(s)s</p>
""" % {"s": SHORT}, """
@font-face { font-family: "BrandSerif"; src: url("%(d)s/DejaVuSerif.ttf") format("truetype"); font-weight: normal; }
@font-face { font-family: "BrandSerif"; src: url("%(d)s/DejaVuSerif-Bold.ttf") format("truetype"); font-weight: bold; }
@font-face { font-family: "CodeFace"; src: url("%(d)s/DejaVuSansMono.ttf"); }
@font-face { font-family: "Missing"; src: url("%(d)s/NoSuchFont.ttf"); }
@font-face { font-family: "DejaVu Sans"; src: url("%(d)s/DejaVuSerif.ttf"); }
body { font-size: 16px; }
.a { font-family: "BrandSerif", sans-serif; }
.b { font-family: "CodeFace", serif; }
.c { font-family: "Missing", monospace; }
.d { font-family: "DejaVu Sans"; }
""" % {"d": FONTDIR})

page("fonts-unicode", """
<p>Latin-1: àâäéèêëîïôöùûüÿç ÀÉÎÕÜ ñÑ ß æœ ÆŒ ø Å ¿¡ «» ‹› „“ ” ‘’ — – … € £ ¥ © ® ™ § ¶ ° ± × ÷ ½ ¼ ¾</p>
<p>Latin Extended: ąćęłńóśźż ĄĆĘŁŃÓŚŹŻ čďěňřšťůž ğışİŞ ŐőŰű ḍṛṣṭ</p>
<p>Greek: Αα Ββ Γγ Δδ Εε Ζζ Ηη Θθ — Καλημέρα κόσμε</p>
<p>Cyrillic: Съешь же ещё этих мягких французских булок, да выпей чаю</p>
<p>Armenian: Բարեւ աշխարհ; Georgian: გამარჯობა მსოფლიო</p>
<p>Arrows and maths: ← ↑ → ↓ ↔ ⇒ ⇔ ∀ ∃ ∅ ∈ ∉ ∑ ∏ √ ∞ ≈ ≠ ≤ ≥ ∫ ∂ ∇</p>
<p>Box drawing: ┌─┬─┐ │ ├─┼─┤ └─┴─┘ ░▒▓█ ■□▪▫ ●○◆◇ ★☆ ✓✔✗✘ ☐☑☒</p>
<p>Ligature candidates: ffi ffl fi fl Th st office affluent</p>
<p>Combining marks: e&#769; a&#776; n&#771; o&#770; &mdash; precomposed é ä ñ ô</p>
<p>Not in DejaVu (CJK, emoji): 日本語 中文 한국어 😀 🚀</p>
<p class="m">Mono: àéîõü Ωπ Жж ← → ✓ ┼</p>
<p class="s">Serif: àéîõü Ωπ Жж ← → ✓</p>
""", "body { font-family: sans-serif; font-size: 15px; } .m { font-family: monospace; } .s { font-family: serif; }")

page("rtl-bidi", """
<p>Mixed bidi in an LTR paragraph: English then עברית בתוך שורה then English again, and العربية مع أرقام 123.</p>
<p dir="rtl">פסקה מימין לשמאל עם מספרים 2026 ומילה באנגלית English באמצע.</p>
<p dir="rtl" style="text-align:left">RTL paragraph aligned left: שלום עולם</p>
<p dir="rtl">مرحبا بالعالم، هذه فقرة باللغة العربية لاختبار تشكيل الحروف والاتجاه من اليمين إلى اليسار.</p>
<div dir="rtl" class="box"><ul><li>פריט ראשון</li><li>פריט שני</li></ul><ol><li>אחד</li><li>שניים</li></ol></div>
<table dir="rtl" border="1" cellpadding="4"><tr><th>שם</th><th>כמות</th><th>מחיר</th></tr><tr><td>תפוחים</td><td>3</td><td>12.50</td></tr></table>
<p><span dir="rtl">embedded rtl span: אבג</span> after; <bdo dir="rtl">bdo override abc</bdo>; <span style="unicode-bidi:bidi-override;direction:rtl">css override 123</span></p>
""", "body { font-family: sans-serif; font-size: 15px; } .box { border: 1px solid #999; width: 300px; }")

# ---------------------------------------------------------------- block layout

page("block-margins", """
<div class="p"><div class="c" style="margin:20px 0">A: margin 20px (collapses with B's 30px)</div><div class="c" style="margin:30px 0 10px">B: margin 30px 0 10px</div></div>
<div class="p"><div class="c" style="margin-top:25px">Child margin-top 25px escapes the parent (no border/padding)</div></div>
<div class="p" style="padding-top:1px"><div class="c" style="margin-top:25px">Parent has padding-top 1px: margin stays inside</div></div>
<div class="p"><div style="margin:15px 0"></div><div class="c">After an empty div with margins 15px (collapse through)</div></div>
<div class="p"><div class="c" style="margin-bottom:20px">margin-bottom 20px</div><div class="c" style="margin-top:-10px">margin-top -10px</div></div>
<div class="p"><div class="c" style="width:200px;margin:0 auto">auto margins centred, 200px</div><div class="c" style="width:200px;margin-left:auto">margin-left auto</div></div>
<div class="p" style="overflow:hidden"><div class="c" style="margin-top:20px">overflow hidden parent: no collapse</div></div>
<div class="c" style="margin:10px 40px 10px 10%">percent margin-left 10%, right 40px</div>
<p style="margin:0;background:#ffd">p margin 0</p><p style="background:#ffd">p default 1em margins</p><h2 style="background:#ffd">h2 default margins</h2><p style="background:#ffd">p</p>
""", "body { font-family: sans-serif; font-size: 13px; } .p { background: #dde8f8; margin-bottom: 12px; } .c { background: #f8d8b0; }")

page("block-box-sizing", """
<div class="c" style="width:300px">width 300px</div>
<div class="c" style="width:300px;padding:10px 30px;border-width:5px">width 300 + padding 30 + border 5 (content-box)</div>
<div class="c" style="width:300px;padding:10px 30px;border-width:5px;box-sizing:border-box">same, box-sizing border-box</div>
<div class="c" style="width:300px;padding:10px 30px;border-width:5px;-webkit-box-sizing:border-box">same, -webkit-box-sizing</div>
<div class="c" style="width:50%">width 50%</div>
<div class="c" style="width:80%;max-width:400px">width 80%, max-width 400px</div>
<div class="c" style="width:100px;min-width:250px">width 100, min-width 250</div>
<div class="c" style="height:60px">height 60px</div>
<div class="c" style="height:30px;min-height:50px">height 30, min-height 50</div>
<div class="c" style="padding:5% 2%">padding 5% 2% (of containing width)</div>
<div class="c" style="width:200px;height:40px;padding:6px">fixed height with long content that overflows the box because it does not fit within forty pixels</div>
<div style="margin-top:40px" class="c">after overflow</div>
""", "body { font-family: sans-serif; font-size: 13px; } .c { background: #e4f0d0; border: 2px solid #5a7; margin: 6px 0; }")

styles = ["solid", "dashed", "dotted", "double", "groove", "ridge", "inset", "outset", "none", "hidden"]
cells = []
for st in styles:
    for w in [1, 3, 6, 10]:
        cells.append('<div class="b" style="border:%dpx %s #3060a0">%s %dpx</div>' % (w, st, st, w))
page("borders-styles", "\n".join(cells) + """
<div class="b" style="border-style:solid dashed dotted double;border-width:4px 6px 8px 10px;border-color:#c00 #0a0 #00c #888;width:200px">mixed sides</div>
<div class="b" style="border:4px solid;border-color:#c00 transparent">transparent sides</div>
<div class="b" style="border:3px solid rgba(0,0,0,.4)">rgba border</div>
<div class="b" style="border:5px groove #ccc">groove light</div>
<div class="b" style="border-top:8px solid #c00;border-left:8px solid #00c">top+left mitre</div>
<span style="border:2px dashed #a0a;padding:2px 6px">inline span with border wrapping across lines? inline boxes keep their borders on each fragment of the line</span>
""", "body { font-family: sans-serif; font-size: 11px; } .b { display: inline-block; width: 120px; height: 34px; margin: 4px; vertical-align: top; }")

page("borders-radius", """
<div class="b" style="border-radius:10px">radius 10</div>
<div class="b" style="border-radius:30px">radius 30</div>
<div class="b" style="border-radius:50%">50%</div>
<div class="b" style="border-radius:20px/10px">20px/10px</div>
<div class="b" style="border-radius:0 20px 0 20px">per corner</div>
<div class="b" style="-webkit-border-radius:15px">-webkit-</div>
<div class="b" style="-webkit-border-top-left-radius:25px;-webkit-border-bottom-right-radius:25px">-webkit- corner</div>
<div class="b" style="border:6px dashed #c33;border-radius:15px">dashed r15</div>
<div class="b" style="border:6px dotted #c33;border-radius:15px">dotted r15</div>
<div class="b" style="border:6px double #333;border-radius:15px">double r15</div>
<div class="b" style="border-width:2px 10px;border-radius:20px">uneven widths</div>
<div class="b" style="border:0;background:#486;color:#fff;border-radius:12px">no border</div>
<div class="b" style="border:4px solid #a50;background:url(../assets/tile.png);border-radius:20px">bg image clipped</div>
<img src="../assets/photo.png" style="border-radius:16px;border:3px solid #333;vertical-align:top">
<span style="background:#fd0;border-radius:8px;padding:2px 8px">inline pill</span>
""", "body { font-family: sans-serif; font-size: 12px; } .b { display: inline-block; width: 120px; height: 60px; margin: 6px; border: 3px solid #36c; background: #def; vertical-align: top; }")

page("box-shadow-outline", """
<div class="b" style="box-shadow:4px 4px 0 #888">hard 4px</div>
<div class="b" style="box-shadow:0 0 8px #333">blur 8</div>
<div class="b" style="box-shadow:5px 5px 10px 2px rgba(0,0,0,.5)">blur+spread</div>
<div class="b" style="box-shadow:inset 3px 3px 6px #555">inset</div>
<div class="b" style="box-shadow:0 0 0 4px #c33, 0 0 0 8px #fc3">two spreads</div>
<div class="b" style="-webkit-box-shadow:3px 3px 5px #036;border-radius:12px">-webkit- radius</div>
<div class="b" style="outline:2px solid #c00">outline solid</div>
<div class="b" style="outline:3px dashed #080;outline-offset:4px">dashed offset 4</div>
<div class="b" style="outline:3px dotted #00c;outline-offset:-8px">dotted offset -8</div>
<div class="b" style="outline:4px double #333;border-radius:10px">double + radius</div>
<p>Inline outline: <a href="#x" style="outline:1px solid #c0c">a link with an outline that spans</a>.</p>
""", "body { font-family: sans-serif; font-size: 12px; } .b { display: inline-block; width: 130px; height: 50px; margin: 14px; border: 1px solid #999; background: #fff; vertical-align: top; }")

# ---------------------------------------------------------------- backgrounds

bg = [
    ("repeat", "url(../assets/tile.png)"),
    ("repeat-x", "url(../assets/tile.png) repeat-x"),
    ("repeat-y", "url(../assets/tile.png) repeat-y"),
    ("no-repeat center", "url(../assets/photo.png) no-repeat center"),
    ("no-repeat right bottom", "url(../assets/bullet.png) no-repeat right bottom #ffe"),
    ("position 20px 10px", "url(../assets/photo.png) no-repeat 20px 10px"),
    ("position 75% 25%", "url(../assets/photo.png) no-repeat 75% 25%"),
    ("size 40px 20px", "url(../assets/photo.png) repeat; background-size: 40px 20px"),
    ("size cover", "url(../assets/photo.png) no-repeat center; background-size: cover"),
    ("size contain", "url(../assets/photo.png) no-repeat center #333; background-size: contain"),
    ("-webkit-background-size 50%", "url(../assets/photo.png) no-repeat; -webkit-background-size: 50% auto"),
    ("jpg", "url(../assets/photo.jpg)"),
    ("gif", "url(../assets/shapes.gif) no-repeat center #eee"),
    ("svg", "url(../assets/chart.svg) no-repeat center"),
    ("alpha png on red", "url(../assets/alpha.png) #f44"),
    ("origin content-box", "url(../assets/bullet.png) no-repeat #eef; background-origin: content-box; padding: 15px"),
    ("clip padding-box", "#9cf; background-clip: padding-box; border: 8px dashed rgba(0,0,0,.4)"),
    ("rgba color", "rgba(0,100,200,.3)"),
]
page("backgrounds", "\n".join('<div class="b" style="background:%s">%s</div>' % (v, k) for k, v in bg),
     "body { font-family: sans-serif; font-size: 11px; } .b { display: inline-block; width: 150px; height: 90px; margin: 5px; border: 1px solid #888; vertical-align: top; }")

gr = [
    "-webkit-linear-gradient(top, #fff, #36c)",
    "-webkit-linear-gradient(left, #c33, #fc3)",
    "-webkit-linear-gradient(45deg, #036, #6cf)",
    "-webkit-linear-gradient(top, #fff 0%, #ddd 50%, #999 51%, #555 100%)",
    "-webkit-linear-gradient(left, rgba(255,0,0,0), rgba(255,0,0,1))",
    "-webkit-gradient(linear, left top, left bottom, from(#8c8), to(#262))",
    "-webkit-gradient(linear, 0 0, 100% 0, from(#fff), color-stop(0.5, #f80), to(#fff))",
    "-webkit-gradient(radial, 50% 50%, 0, 50% 50%, 60, from(#ff0), to(#c00))",
    "-webkit-radial-gradient(center, circle, #fff, #36c)",
    "-webkit-radial-gradient(30% 30%, ellipse cover, #fff, #090)",
    "-webkit-repeating-linear-gradient(left, #eee, #eee 10px, #ccc 10px, #ccc 20px)",
    "linear-gradient(to right, #c33, #33c) /* unprefixed: ignored by 2012 WebKit */",
]
page("backgrounds-gradients", "\n".join(
    '<div class="b" style="background:#eee;background:%s"><span>%s</span></div>' % (g.split(" /*")[0], g) for g in gr)
    + '\n<a class="btn">Gradient button</a>',
    "body { font-family: sans-serif; font-size: 10px; } .b { display: inline-block; width: 200px; height: 80px; margin: 5px; border: 1px solid #888; vertical-align: top; } "
    ".b span { background: rgba(255,255,255,.8); } .btn { display: inline-block; padding: 6px 14px; color: #fff; border-radius: 4px; "
    "background: #36c; background: -webkit-gradient(linear, left top, left bottom, from(#58e), to(#25a)); border: 1px solid #148; font-size: 14px; }")

page("backgrounds-body", paras(3) + '<div style="background:#fff;padding:10px">white box on a coloured page</div>',
     "html { background: #f3ead8; } body { background: #e0eef8 url(../assets/tile.png) repeat-x; font-family: serif; font-size: 14px; margin: 30px; }")

# ---------------------------------------------------------------- lists

types = ["disc", "circle", "square", "decimal", "decimal-leading-zero", "lower-roman", "upper-roman", "lower-greek",
         "lower-alpha", "upper-alpha", "lower-latin", "upper-latin", "armenian", "georgian", "hebrew", "cjk-ideographic", "none"]
page("lists-types", "\n".join(
    '<ol style="list-style-type:%s" start="%s"><li>%s</li><li>second</li><li>third</li></ol>' % (t, 8 if "roman" in t or t == "decimal" else 1, t)
    for t in types) + """
<ol start="5"><li>start=5</li><li value="20">value=20</li><li>next</li></ol>
<ol type="A"><li>type=A</li><li>B</li></ol><ol type="i"><li>type=i</li><li>ii</li></ol>
<ul type="square"><li>type=square</li></ul>
""", "body { font-family: sans-serif; font-size: 12px; } ol, ul { display: inline-block; width: 190px; vertical-align: top; margin: 4px; padding-left: 40px; }")

page("lists-nested", """
<ul><li>Level 1 disc
  <ul><li>Level 2 circle
    <ul><li>Level 3 square<ul><li>Level 4</li></ul></li></ul></li><li>Level 2 again</li></ul></li>
<li>%(l)s</li></ul>
<ol><li>First<ol><li>Nested 1.1</li><li>Nested 1.2<ol type="a"><li>alpha</li></ol></li></ol></li><li>Second</li></ol>
<ul style="list-style-position:inside;border:1px solid #aaa"><li>inside marker %(l)s</li><li>inside short</li></ul>
<ul style="list-style-image:url(../assets/bullet.png)"><li>list-style-image</li><li>second image bullet</li></ul>
<ol style="font-size:22px"><li>Big marker</li></ol><ol style="padding-left:0"><li>padding-left 0 (marker in margin)</li></ol>
<ul style="list-style:none;padding:0"><li>no style</li><li>no style</li></ul>
<ol style="direction:rtl"><li>rtl list</li><li>second</li></ol>
<ul><li><p>Paragraph inside li</p></li><li style="float:left;width:150px">floated li</li></ul>
<div style="clear:both"></div>
<ol><li style="list-style-type:upper-roman">per-item type</li><li style="color:#c00">red item (red marker)</li></ol>
""" % {"l": LOREM}, "body { font-family: serif; font-size: 14px; }")

# ---------------------------------------------------------------- tables

page("tables-borders", """
<h4>separate, border-spacing 4px</h4>
<table class="t" style="border-collapse:separate;border-spacing:4px"><tr><td>a</td><td>b</td><td>c</td></tr><tr><td>d</td><td>e</td><td>f</td></tr></table>
<h4>collapse, conflicting widths (cell 3px red wins over table 1px)</h4>
<table class="t" style="border-collapse:collapse"><tr><td>a</td><td style="border:3px solid #c00">b</td><td>c</td></tr><tr><td style="border:2px dashed #00c">d</td><td>e</td><td style="border-style:double;border-width:4px">f</td></tr></table>
<h4>border=1 cellpadding=6 cellspacing=0 attributes</h4>
<table border="1" cellpadding="6" cellspacing="0"><tr><th>H1</th><th>H2</th></tr><tr><td>one</td><td>two</td></tr></table>
<h4>border=3 cellspacing=5 (outset/inset)</h4>
<table border="3" cellspacing="5"><tr><td>x</td><td>y</td></tr></table>
<h4>rules=rows frame=hsides</h4>
<table border="1" rules="rows" frame="hsides" cellpadding="4"><tr><td>r1c1</td><td>r1c2</td></tr><tr><td>r2c1</td><td>r2c2</td></tr></table>
<h4>row and column backgrounds, collapse</h4>
<table class="t" style="border-collapse:collapse"><colgroup><col style="background:#fee"><col style="background:#efe"><col></colgroup>
<tr style="background:#eef"><td>row bg</td><td>row over col</td><td>c</td></tr><tr><td>col</td><td>col</td><td style="background:#ff8">cell</td></tr></table>
<h4>empty-cells: hide (separate)</h4>
<table class="t" style="empty-cells:hide;border-collapse:separate"><tr><td>a</td><td></td><td>c</td></tr></table>
<h4>thick collapse borders with radius on table (ignored)</h4>
<table class="t" style="border-collapse:collapse;border:6px solid #333;border-radius:10px"><tr><td style="border:6px solid #9c3">a</td><td>b</td></tr></table>
""", "body { font-family: sans-serif; font-size: 12px; } h4 { margin: 10px 0 3px; } .t { border: 1px solid #333; } .t td { border: 1px solid #888; padding: 4px 10px; }")

page("tables-spans", """
<table><tr><th rowspan="2">Region</th><th colspan="3">2025</th><th colspan="2">2026</th></tr>
<tr><th>Q1</th><th>Q2</th><th>Q3</th><th>Q1</th><th>Q2</th></tr>
<tr><td rowspan="3">North</td><td>12</td><td>14</td><td rowspan="2">tall</td><td>18</td><td>21</td></tr>
<tr><td colspan="2">merged two</td><td>19</td><td>22</td></tr>
<tr><td>13</td><td colspan="4">four columns merged with a longer text that wraps inside the cell</td></tr>
<tr><td>South</td><td colspan="5" style="text-align:center">all five</td></tr>
<tr><td colspan="6" style="background:#eee">full width footer row</td></tr></table>
<br>
<table><tr><td rowspan="4" style="width:80px">rowspan 4 with a lot of text that is taller than the rows it spans so rows grow</td><td>a</td></tr><tr><td>b</td></tr><tr><td>c</td></tr><tr><td>d</td></tr></table>
""", "body { font-family: sans-serif; font-size: 12px; } table { border-collapse: collapse; } td, th { border: 1px solid #666; padding: 4px 8px; } th { background: #dde; }")

page("tables-layout", """
<h4>auto layout, width 100%%</h4>
<table style="width:100%%"><tr><td>short</td><td>%(s)s</td><td>nowrap-ish label</td></tr></table>
<h4>auto layout, no width</h4>
<table><tr><td>short</td><td>%(s)s</td></tr></table>
<h4>fixed layout, width 500px, first col 100px</h4>
<table style="table-layout:fixed;width:500px"><tr><td style="width:100px">%(s)s</td><td>b</td><td>c</td></tr></table>
<h4>col widths 20%% / 30%% / auto</h4>
<table style="width:100%%"><col style="width:20%%"><col style="width:30%%"><col><tr><td>20%%</td><td>30%%</td><td>auto</td></tr></table>
<h4>white-space: nowrap cell and a very long word</h4>
<table style="width:400px"><tr><td style="white-space:nowrap">no wrap here at all please</td><td>Pneumonoultramicroscopicsilicovolcanoconiosis_and_more_text</td></tr></table>
<h4>percent and pixel mix, width attributes</h4>
<table width="80%%"><tr><td width="50">50</td><td width="40%%">40%%</td><td>rest</td></tr></table>
<h4>text-align and cell padding</h4>
<table style="width:60%%" cellpadding="10"><tr><td align="right">right</td><td style="text-align:center">center</td><td>left</td></tr></table>
<h4>table centred with margin auto</h4>
<table style="margin:0 auto"><tr><td>centred table</td></tr></table>
<table align="center"><tr><td>align=center</td></tr></table>
""" % {"s": LOREM[:150]}, "body { font-family: sans-serif; font-size: 12px; } h4 { margin: 8px 0 2px; } table { border-collapse: collapse; } td { border: 1px solid #777; }")

page("tables-cells", """
<table class="t"><caption>Caption on top</caption>
<tr><th>top</th><th>middle</th><th>bottom</th><th>baseline</th><th>tall</th></tr>
<tr><td style="vertical-align:top">top</td><td style="vertical-align:middle">middle</td><td style="vertical-align:bottom">bottom</td>
<td style="vertical-align:baseline"><span style="font-size:20px">Base</span>line</td><td style="height:80px">80px tall</td></tr>
<tr><td valign="top">valign=top</td><td valign="middle">valign=middle</td><td valign="bottom">valign=bottom</td><td>default</td><td style="height:40px">40</td></tr></table>
<table class="t" style="caption-side:bottom"><caption>Caption at the bottom</caption><tr><th style="text-align:left">th left</th><th>th default (centred)</th><th style="text-align:right">th right</th></tr>
<tr><td>a</td><td>b</td><td>c</td></tr></table>
<table class="t"><thead><tr><th>thead</th><th>x</th></tr></thead><tfoot><tr><td>tfoot</td><td>y</td></tr></tfoot><tbody><tr><td>tbody 1</td><td>1</td></tr><tr><td>tbody 2</td><td>2</td></tr></tbody></table>
<table class="t"><tr><td style="padding:0"><img src="../assets/photo.png" style="display:block"></td><td><img src="../assets/photo.png"></td><td style="width:60px;overflow:hidden">image in narrow cell: <img src="../assets/photo.png"></td></tr></table>
""", "body { font-family: sans-serif; font-size: 12px; } .t { border-collapse: collapse; margin: 8px 0; } .t td, .t th { border: 1px solid #666; padding: 3px 8px; } caption { font-style: italic; }")

rows = "\n".join(
    "<tr><td>%d</td><td>Item %03d %s</td><td>%d</td><td>%d.%02d</td></tr>" % (i, i, "with a longer description that wraps" if i % 7 == 0 else "", i % 9 + 1, i * 3, i % 100)
    for i in range(1, 121))
page("tables-long", """
<h2>Long table: thead and tfoot repeat on every page</h2>
<table><thead><tr><th>#</th><th>Description</th><th>Qty</th><th>Amount</th></tr></thead>
<tfoot><tr><td colspan="3">Carried forward</td><td>—</td></tr></tfoot>
<tbody>%s</tbody></table><p>After the table.</p>
""" % rows, "body { font-family: sans-serif; font-size: 11px; } table { width: 100%; border-collapse: collapse; } "
           "th { background: #1e3c8c; color: #fff; text-align: left; } td, th { border-bottom: 1px solid #ccc; padding: 3px 6px; } "
           "tbody tr:nth-child(even) td { background: #f2f5fb; } tfoot td { font-weight: bold; border-top: 2px solid #333; } "
           "thead { display: table-header-group; } tfoot { display: table-footer-group; } tr { page-break-inside: avoid; }")

page("tables-nested", """
<table class="o"><tr><td>outer a<table class="i"><tr><td>inner 1</td><td>inner 2</td></tr><tr><td colspan="2">inner span</td></tr></table></td>
<td>outer b<table class="i" style="width:100%"><tr><td>100%</td></tr></table></td></tr></table>
<div style="float:right;width:220px;margin-left:10px"><table class="o" style="width:100%"><tr><td>table in a right float</td></tr><tr><td>row 2</td></tr></table></div>
<p>%s</p>
<div style="display:table;width:100%;border:1px solid #999"><div style="display:table-row"><div style="display:table-cell;padding:6px;background:#eef;width:30%">CSS table cell 30%</div><div style="display:table-cell;padding:6px">CSS table cell auto, vertical-align middle by default? no: baseline</div></div></div>
""".replace("%s", LOREM, 1), "body { font-family: sans-serif; font-size: 12px; } .o { border: 2px solid #333; border-collapse: collapse; } .o > tbody > tr > td { border: 1px solid #333; padding: 6px; vertical-align: top; } .i { border: 1px solid #c33; margin-top: 4px; } .i td { border: 1px solid #e99; padding: 2px 4px; }")

# ---------------------------------------------------------------- floats, positioning, display

page("floats", """
<div class="f" style="float:left;width:120px;height:80px">float left 120x80</div>
<p>%(l)s %(l)s</p>
<div class="f" style="float:right;width:150px">float right, auto height, some text inside it</div>
<p>%(l)s</p>
<p style="clear:both;background:#efe">clear both</p>
<div class="f" style="float:left;width:100px">L1</div><div class="f" style="float:left;width:100px">L2</div><div class="f" style="float:right;width:100px">R1</div>
<p>Text between three floats. %(s)s</p>
<div style="clear:left"></div>
<div style="overflow:hidden;border:1px solid #999;padding:4px"><div class="f" style="float:left;width:80px;height:50px">BFC</div>overflow hidden contains the float and does not wrap around it beyond its border.</div>
<div style="border:1px solid #999;margin-top:6px"><div class="f" style="float:left;width:80px;height:60px">no BFC</div>Parent does not contain the float.</div>
<p>Image floats: <img src="../assets/photo.png" style="float:right;margin:0 0 6px 10px">%(l)s</p>
<div style="clear:both"></div>
<div class="f" style="float:left;width:45%%">Two columns: left 45%%. %(s)s</div><div class="f" style="float:right;width:45%%">right 45%%. %(s)s %(s)s</div>
<div style="clear:both;height:1px"></div>
""" % {"l": LOREM, "s": SHORT}, "body { font-family: serif; font-size: 13px; } .f { background: #fde3c0; border: 1px solid #c90; margin: 2px; }")

page("positioning", """
<div class="r">relative container
  <div class="a" style="top:10px;right:10px">abs top/right</div>
  <div class="a" style="bottom:5px;left:30px;background:#cfc">abs bottom/left</div>
  <div class="a" style="top:40px;left:120px;width:150px;z-index:2;background:#fcc">z-index 2</div>
  <div class="a" style="top:55px;left:200px;width:150px;z-index:1;background:#ccf">z-index 1</div>
</div>
<p>Relative offsets: <span style="position:relative;top:6px;left:10px;background:#ff0">top 6 left 10</span> then text continues.</p>
<div style="position:relative;left:40px;width:300px;background:#eef">relative block left 40px</div>
<div class="a" style="top:0;right:0;width:120px;background:#ffd">abs to the page (top right)</div>
<div style="position:relative;height:60px;border:1px dashed #999;margin-top:10px"><div style="position:absolute;left:50%%;top:50%%;width:100px;margin-left:-50px;height:20px;margin-top:-10px;background:#9cf">centred</div></div>
<div style="position:relative;margin-top:10px"><img src="../assets/photo.png"><span style="position:absolute;left:10px;bottom:10px;color:#fff;font-weight:bold">caption over image</span></div>
<p>%s</p>
""" % LOREM, "body { font-family: sans-serif; font-size: 12px; } .r { position: relative; height: 120px; border: 2px solid #36c; margin: 10px 0; } .a { position: absolute; background: #eee; border: 1px solid #666; padding: 3px; }")

page("position-fixed", '<div class="fx">position: fixed banner</div><div class="fx2">fixed bottom</div>' + "\n".join(
    "<h3>Section %d</h3>%s" % (i, paras(2)) for i in range(1, 7)),
    "body { font-family: serif; font-size: 13px; } .fx { position: fixed; top: 0; right: 0; background: #c33; color: #fff; padding: 4px 8px; } "
    ".fx2 { position: fixed; bottom: 0; left: 0; background: #36c; color: #fff; padding: 4px 8px; }")

page("inline-block", """
<div class="row"><div class="ib" style="height:40px">40</div><div class="ib" style="height:70px">70</div><div class="ib">auto with text wrapping inside</div><div class="ib" style="vertical-align:top;height:30px">top</div><div class="ib" style="vertical-align:middle;height:30px">middle</div><div class="ib" style="vertical-align:bottom;height:30px">bottom</div> text baseline</div>
<div class="row"><div class="ib">whitespace</div> <div class="ib">between</div>   <div class="ib">boxes</div><div class="ib">none</div><div class="ib">here</div></div>
<div class="row" style="font-size:0"><div class="ib" style="font-size:12px">font-size 0</div> <div class="ib" style="font-size:12px">parent</div></div>
<div class="row"><span class="ib" style="width:30%">30%</span><span class="ib" style="width:30%">30%</span><span class="ib" style="width:30%">30%</span></div>
<div class="row"><button>button</button> <input type="text" value="input"> <img src="../assets/bullet.png"> <span class="ib" style="padding:10px">padded</span> <span class="ib" style="overflow:hidden;height:20px">overflow hidden ib baseline</span></div>
<p>Badges: <span class="badge">new</span> <span class="badge" style="background:#c33">3</span> <span class="badge" style="background:#393">ok</span> inline with text.</p>
""", "body { font-family: sans-serif; font-size: 12px; } .row { border: 1px solid #999; margin: 8px 0; background: #fafafa; } .ib { display: inline-block; width: 90px; background: #cde; border: 1px solid #579; margin: 2px; } "
     ".badge { display: inline-block; padding: 2px 6px; font-size: 10px; font-weight: bold; color: #fff; background: #36c; border-radius: 9px; }")

page("overflow", """
<div class="b" style="overflow:visible">visible: %(l)s</div>
<div class="b" style="overflow:hidden">hidden: %(l)s</div>
<div class="b" style="overflow:scroll">scroll: %(l)s</div>
<div class="b" style="overflow:auto">auto: %(l)s</div>
<div class="b" style="overflow-x:hidden;overflow-y:visible">x hidden</div>
<div class="b" style="height:auto;white-space:nowrap;overflow:hidden;text-overflow:ellipsis">ellipsis: %(l)s</div>
<div class="b" style="position:relative;overflow:hidden;height:40px"><img src="../assets/photo.png" style="position:absolute;top:-20px;left:-30px"></div>
<div class="b" style="height:40px;border-radius:20px;overflow:hidden;background:#fd8">hidden with radius: %(l)s</div>
<div style="clear:both"></div>
""" % {"l": LOREM}, "body { font-family: sans-serif; font-size: 11px; } .b { float: left; width: 200px; height: 70px; margin: 8px 20px 50px 8px; border: 2px solid #579; background: #eef; }")

page("display-misc", """
<p>display:none → [<span style="display:none">hidden</span>] visibility:hidden → [<span style="visibility:hidden">invisible</span>]</p>
<p><span style="display:block;background:#eef">span as block</span><div style="display:inline;background:#fee">div inline</div> <div style="display:inline;background:#efe">div inline 2</div></p>
<div style="display:list-item;margin-left:30px">div display:list-item</div>
<div style="display:-webkit-box;-webkit-box-orient:horizontal;width:400px;border:1px solid #999"><div style="-webkit-box-flex:1;background:#fdd">box-flex 1</div><div style="-webkit-box-flex:2;background:#dfd">box-flex 2</div><div style="width:80px;background:#ddf">80px</div></div>
<div style="display:flex;border:1px solid #999"><div style="background:#fee">flex (ignored</div><div style="background:#efe">in 2012)</div></div>
<div style="display:table;border-spacing:6px;background:#eee"><div style="display:table-cell;background:#fff;padding:4px">table-cell</div><div style="display:table-cell;background:#fff;padding:4px;vertical-align:middle">middle</div></div>
<div style="display:inline-table;border:1px solid #333"><div style="display:table-row"><div style="display:table-cell">inline-table</div></div></div> after inline-table
<div style="-webkit-transform:rotate(-5deg);width:200px;margin:20px;background:#9cf;padding:6px">-webkit-transform rotate(-5deg)</div>
<div style="transform:rotate(20deg);width:200px;background:#fc9;padding:6px">transform (unprefixed, ignored)</div>
<div style="-webkit-transform:scale(1.5) translate(40px,0);width:120px;background:#c9f;margin:20px 0">scale+translate</div>
<p style="column-count:2">column-count 2 (ignored): %s</p>
""" % LOREM, "body { font-family: sans-serif; font-size: 12px; }")

page("css-generated", """
<h2 class="c">Counters</h2><h3 class="c">First</h3><h3 class="c">Second</h3><h2 class="c">Next chapter</h2><h3 class="c">Again</h3>
<p class="q"><q>Quoted <q>nested</q> text</q></p>
<p class="ba">Before and after</p>
<p class="fl">First letter and first line styling on a paragraph that is long enough to wrap: %s</p>
<ul class="kids"><li>first-child</li><li>second</li><li>third</li><li>last-child</li></ul>
<p><a class="ext" href="http://example.com/">link with attr(href) after</a></p>
""" % LOREM, """
body { font-family: serif; font-size: 13px; counter-reset: ch; }
h2.c { counter-increment: ch; counter-reset: sec; } h2.c:before { content: "Chapter " counter(ch) ": "; }
h3.c { counter-increment: sec; } h3.c:before { content: counter(ch) "." counter(sec, lower-roman) " "; color: #c33; }
.ba:before { content: "\\2192  "; color: #36c; } .ba:after { content: " [end]"; font-style: italic; }
.fl:first-letter { font-size: 32px; float: left; color: #a33; line-height: 1; margin-right: 3px; } .fl:first-line { font-weight: bold; }
.kids li:first-child { color: #c00; } .kids li:last-child { color: #00c; } .kids li:nth-child(2n) { background: #eee; }
.ext:after { content: " (" attr(href) ")"; font-size: 10px; }
""")

# ---------------------------------------------------------------- images

page("images-formats", """
<p>PNG <img src="../assets/photo.png"> JPEG <img src="../assets/photo.jpg"></p>
<p>GIF <img src="../assets/shapes.gif"> SVG <img src="../assets/chart.svg"></p>
<p>Alpha PNG on colours: <span style="background:#36c;padding:8px;display:inline-block"><img src="../assets/alpha.png"></span>
<span style="background:url(../assets/tile.png);padding:8px;display:inline-block"><img src="../assets/alpha.png"></span></p>
<p>data: URI PNG <img src="data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAgAAAAICAIAAABLbSncAAAAF0lEQVR4nGP4z8DwHwMhUZEYuM8wYAAAQxsI+O+6MWkAAAAASUVORK5CYII=" width="40" height="40"></p>
<p>Inline SVG: <svg width="120" height="60" xmlns="http://www.w3.org/2000/svg"><rect x="2" y="2" width="116" height="56" rx="8" fill="#fd8" stroke="#a60" stroke-width="2"/><circle cx="30" cy="30" r="18" fill="#36c"/><text x="55" y="36" font-family="sans-serif" font-size="14">SVG</text></svg></p>
""", "body { font-family: sans-serif; font-size: 12px; }")

page("images-sizing", """
<p>natural <img src="../assets/photo.png"> width=80 <img src="../assets/photo.png" width="80"> height=40 <img src="../assets/photo.png" height="40"></p>
<p>both 200x50 (distorted) <img src="../assets/photo.png" width="200" height="50"> css 60px <img src="../assets/photo.png" style="width:60px"></p>
<div style="width:300px;border:1px solid #999">width 100% of 300px <img src="../assets/photo.jpg" style="width:100%"></div>
<div style="width:100px;border:1px solid #999">max-width 100% <img src="../assets/photo.jpg" style="max-width:100%"></div>
<p>Inline baseline gap: <span style="display:inline-block;border:1px solid #c00"><img src="../assets/photo.png" width="80"></span>
display:block: <span style="display:inline-block;border:1px solid #c00"><img src="../assets/photo.png" width="80" style="display:block"></span></p>
<p>Border + padding: <img src="../assets/photo.png" width="80" style="border:4px solid #333;padding:4px;background:#fd8"></p>
<p>Broken: <img src="../assets/missing.png" alt="missing image alt text"> sized broken: <img src="../assets/missing.png" alt="alt" width="100" height="40"> no alt: <img src="../assets/missing.png"></p>
<p>Scaled up SVG <img src="../assets/chart.svg" width="300"> GIF x2 <img src="../assets/shapes.gif" width="240"></p>
""", "body { font-family: sans-serif; font-size: 12px; }")

# ---------------------------------------------------------------- forms

page("forms-inputs", """
<form>
<p><label>Text <input type="text" name="t" value="Hello"></label> <label>Size 5 <input type="text" size="5" value="12345678"></label></p>
<p><label>Password <input type="password" value="secret"></label> <label>Email <input type="email" value="a@b.example"></label></p>
<p><label>Number <input type="number" value="42"></label> <label>Search <input type="search" value="query"></label></p>
<p><label>Placeholder <input type="text" placeholder="placeholder text"></label> <label>Disabled <input type="text" value="disabled" disabled></label></p>
<p><label>Readonly <input type="text" value="read only" readonly></label> <label>Styled <input type="text" value="styled" style="border:2px solid #36c;padding:4px;font-size:16px;width:200px"></label></p>
<p><label>Hidden [<input type="hidden" value="x">] File <input type="file"></label></p>
<p>Textarea<br><textarea rows="3" cols="30">Line one
Line two of the textarea
Line three</textarea> <textarea style="width:200px;height:50px;font-family:serif">Styled textarea</textarea></p>
</form>
""", "body { font-family: sans-serif; font-size: 13px; }")

page("forms-controls", """
<form>
<p><input type="checkbox"> unchecked <input type="checkbox" checked> checked <input type="checkbox" disabled> disabled <input type="checkbox" checked disabled> checked disabled</p>
<p><input type="radio" name="r"> radio <input type="radio" name="r" checked> radio checked <input type="radio" disabled> disabled</p>
<p>Select <select><option>First option</option><option selected>Second option selected</option></select>
Multiple <select multiple size="3"><option>One</option><option selected>Two</option><option>Three</option><option>Four</option></select>
Size 4 <select size="4"><option>A</option><option>B</option></select></p>
<p><button>Button element</button> <input type="submit" value="Submit"> <input type="reset" value="Reset"> <input type="button" value="Input button">
<button style="background:#36c;color:#fff;border:0;padding:6px 12px;border-radius:4px">Styled</button> <button disabled>Disabled</button></p>
<fieldset><legend>Fieldset legend</legend><label><input type="checkbox" checked> Remember me</label><br><label>Name <input type="text" value="Ada"></label></fieldset>
<p><input type="range" value="30"> range, <input type="color" value="#ff0000"> color, <progress value="40" max="100"></progress> progress, <meter value="0.6">meter</meter></p>
</form>
""", "body { font-family: sans-serif; font-size: 13px; }")

# ---------------------------------------------------------------- pagination

page("breaks", """
<h2>Page 1</h2><p>%(s)s</p>
<div style="page-break-after:always;background:#eef">page-break-after: always</div>
<h2>Page 2</h2><p>%(s)s</p>
<h2 style="page-break-before:always">page-break-before: always (page 3)</h2><p>%(s)s</p>
<div style="break-before:page;background:#fee">break-before: page (CSS3 name: ignored in 2012?)</div>
<p>%(l)s</p>
<table style="width:100%%"><tr><td>row 1</td></tr><tr style="page-break-before:always"><td>row with page-break-before (ignored on rows?)</td></tr></table>
<span style="page-break-before:always">inline span with a break (ignored)</span>
<h2 style="page-break-before:left">page-break-before: left</h2><p>%(s)s</p>
<h2 style="page-break-before:right">page-break-before: right</h2><p>%(s)s</p>
""" % {"s": SHORT, "l": LOREM}, "body { font-family: serif; font-size: 14px; }")

page("breaks-avoid", "<h2>page-break-inside: avoid and long content</h2>" + paras(7) + """
<div class="k">This block avoids breaking inside. %(l)s %(l)s %(l)s</div>
%(p)s
<div class="k" style="height:900px;background:#fee">A block taller than a page with avoid: must break anyway.</div>
<h3 style="page-break-after:avoid">Heading with page-break-after: avoid</h3>
<p>%(l)s</p>
%(p2)s
<img src="../assets/photo.jpg" style="width:500px;height:300px">
<p>Paragraph split across pages: %(l)s %(l)s %(l)s %(l)s %(l)s</p>
""" % {"l": LOREM, "p": paras(4), "p2": paras(5)},
     "body { font-family: serif; font-size: 14px; } .k { page-break-inside: avoid; background: #eef; border: 1px solid #99c; padding: 6px; }")

page("media-print", """
<p class="screen">Only on screen (hidden in print)</p>
<p class="print">Only in print media</p>
<p class="both">Colour depends on media: red for screen, green for print.</p>
<p>%s</p>
""" % LOREM, """
body { font-family: sans-serif; font-size: 14px; }
.print { display: none; }
.both { color: #c00; }
@media print { .screen { display: none; } .print { display: block; } .both { color: #080; } body { font-family: serif; } }
@media screen and (max-width: 600px) { .both:after { content: " (narrow screen)"; } }
@media screen and (min-width: 601px) { .both:after { content: " (wide screen)"; } }
""")

# ---------------------------------------------------------------- headers, footers, TOC

sections = "\n".join(
    "<h1>Chapter %d</h1>%s<h2>Section %d.1</h2>%s<h2>Section %d.2</h2>%s" % (i, paras(2), i, paras(3), i, paras(2))
    for i in range(1, 4))
page("hf-body", sections, "body { font-family: serif; font-size: 13px; } h1 { page-break-before: always; } h1:first-child { page-break-before: auto; }",
     title="Header Footer Body")

CASES["hf-header.html"] = """<!DOCTYPE html>
<html><head><meta charset="utf-8">
<script>
function subst() {
  var vars = {};
  var q = document.location.search.substring(1).split('&');
  for (var i in q) { var kv = q[i].split('=', 2); vars[kv[0]] = decodeURIComponent(kv[1]); }
  var names = ['page', 'topage', 'section', 'subsection', 'doctitle'];
  for (var n in names) {
    var els = document.getElementsByClassName(names[n]);
    for (var j = 0; j < els.length; ++j) els[j].textContent = vars[names[n]];
  }
}
</script>
<style>
body { margin: 0; font-family: sans-serif; font-size: 11px; color: #333; }
table { width: 100%; border-bottom: 2px solid #1e3c8c; border-collapse: collapse; }
td { padding: 2px 0; }
.r { text-align: right; }
</style></head>
<body onload="subst()"><table><tr><td><img src="../assets/logo.png" height="24"></td><td><span class="doctitle"></span> — <span class="section"></span></td><td class="r">Page <span class="page"></span> / <span class="topage"></span></td></tr></table></body></html>
"""
CASES["hf-footer.html"] = """<!DOCTYPE html>
<html><head><meta charset="utf-8">
<script>
function subst() {
  var vars = {};
  var q = document.location.search.substring(1).split('&');
  for (var i in q) { var kv = q[i].split('=', 2); vars[kv[0]] = decodeURIComponent(kv[1]); }
  document.getElementById('p').textContent = vars.page;
  document.getElementById('t').textContent = vars.topage;
}
</script>
<style>body { margin: 0; font-family: serif; font-size: 10px; text-align: center; color: #666; border-top: 1px solid #aaa; padding-top: 3px; }</style>
</head><body onload="subst()">ACME Corp — confidential — <span id="p"></span> of <span id="t"></span></body></html>
"""

# ---------------------------------------------------------------- page geometry

page("pagesize", """
<div class="frame"><h1>Page geometry</h1><p>%s</p><div class="ruler"></div><p class="right">right-aligned text touches the right margin</p>
<div class="wide">a 100%% wide block with a border</div></div>
""" % LOREM, "body { font-family: sans-serif; font-size: 13px; margin: 0; } .frame { border: 2px solid #c33; padding: 10px; } "
     ".ruler { height: 10px; background: -webkit-linear-gradient(left, #36c, #c63); } .right { text-align: right; } .wide { border: 1px solid #333; width: 100%; box-sizing: border-box; }")

page("wide-content", """
<p>%s</p>
<div style="width:1100px;background:#eef;border:2px solid #36c">A block 1100px wide, wider than the page: smart shrinking scales the page to fit it.</div>
<table style="width:1000px" border="1"><tr><td>wide table cell 1</td><td>cell 2</td><td>cell 3</td><td>cell 4</td></tr></table>
<p>%s</p>
""" % (LOREM, LOREM), "body { font-family: sans-serif; font-size: 13px; }")

page("zoom", """
<h2>Zoom probe</h2><p>%s</p><img src="../assets/photo.png"> <span style="border:1px solid #333;padding:4px">1px border</span>
<div style="width:300px;height:50px;background:#9cf;margin-top:10px">300x50 box</div>
<table border="1" cellpadding="4" style="margin-top:10px"><tr><td>12px text</td><td style="font-size:20px">20px</td></tr></table>
""" % LOREM, "body { font-family: serif; font-size: 12px; }")

# ---------------------------------------------------------------- realistic documents

BS = "../assets/bs3-lite.css"

page("doc-invoice", """
<div class="head"><img src="../assets/logo.png" class="logo"><div class="co"><b>ACME Industries SAS</b><br>12 rue de l'Exemple<br>75011 Paris, France<br>SIRET 123 456 789 00012<br>TVA FR12 123456789</div></div>
<h1>INVOICE <span>#INV-2026-0417</span></h1>
<table class="meta"><tr><td><b>Bill to</b><br>Globex Corporation<br>Attn: Accounts Payable<br>4 Industrial Way<br>Springfield, IL 62701, USA</td>
<td><b>Ship to</b><br>Globex Warehouse 3<br>18 Dock Road<br>Springfield, IL 62702</td>
<td class="r"><table class="kv"><tr><th>Invoice date</th><td>2026-10-01</td></tr><tr><th>Due date</th><td>2026-10-31</td></tr><tr><th>PO number</th><td>PO-88412</td></tr><tr><th>Terms</th><td>Net 30</td></tr></table></td></tr></table>
<table class="lines"><thead><tr><th>SKU</th><th>Description</th><th class="n">Qty</th><th class="n">Unit price</th><th class="n">Amount</th></tr></thead><tbody>
%s
</tbody></table>
<table class="totals"><tr><th>Subtotal</th><td>12 847.50 €</td></tr><tr><th>Discount (5%%)</th><td>−642.38 €</td></tr><tr><th>VAT 20%%</th><td>2 441.02 €</td></tr><tr class="grand"><th>Total due</th><td>14 646.14 €</td></tr></table>
<div class="notes"><h3>Payment</h3><p>Bank transfer to IBAN FR76 3000 6000 0112 3456 7890 189, BIC AGRIFRPP. Please quote the invoice number. Late payments bear interest at three times the legal rate plus a fixed recovery fee of 40 €.</p></div>
<div class="stamp">PAID</div>
""" % "\n".join(
    '<tr><td>SKU-%04d</td><td>%s</td><td class="n">%d</td><td class="n">%d.%02d €</td><td class="n">%d.%02d €</td></tr>' % (
        1000 + i * 37, ["Industrial widget, galvanised", "Mounting bracket kit (pack of 12)", "Service hours — on-site installation and calibration of the main line",
                        "Replacement filter cartridge", "Shipping and handling"][i % 5], i % 4 + 1, 20 + i * 13, i * 7 % 100, (i % 4 + 1) * (20 + i * 13), i * 3 % 100)
    for i in range(14)), """
body { font-family: "Helvetica Neue", Helvetica, Arial, sans-serif; font-size: 12px; color: #333; }
.head { overflow: hidden; border-bottom: 3px solid #1e3c8c; padding-bottom: 10px; }
.logo { float: left; } .co { float: right; text-align: right; font-size: 11px; line-height: 1.4; }
h1 { color: #1e3c8c; font-size: 26px; letter-spacing: 1px; margin: 16px 0; } h1 span { font-size: 16px; color: #777; letter-spacing: 0; }
.meta { width: 100%; margin-bottom: 18px; } .meta td { vertical-align: top; width: 33%; line-height: 1.5; } .r { text-align: right; }
.kv { margin-left: auto; border-collapse: collapse; } .kv th { text-align: left; padding: 2px 10px 2px 0; color: #777; font-weight: normal; } .kv td { text-align: right; }
.lines { width: 100%; border-collapse: collapse; } .lines th { background: #1e3c8c; color: #fff; padding: 6px; text-align: left; }
.lines td { padding: 5px 6px; border-bottom: 1px solid #e3e3e3; } .lines tbody tr:nth-child(odd) td { background: #f7f9fc; } .n { text-align: right; white-space: nowrap; }
.totals { margin: 12px 0 0 auto; border-collapse: collapse; min-width: 260px; } .totals th { text-align: left; padding: 4px 10px; font-weight: normal; } .totals td { text-align: right; padding: 4px 6px; }
.grand th, .grand td { border-top: 2px solid #1e3c8c; font-weight: bold; font-size: 15px; color: #1e3c8c; }
.notes { margin-top: 24px; padding: 10px 14px; background: #f3f5f9; border-left: 4px solid #1e3c8c; } .notes h3 { margin: 0 0 4px; font-size: 13px; }
.stamp { position: absolute; top: 220px; right: 40px; border: 4px solid #2e7d32; color: #2e7d32; font-size: 30px; font-weight: bold; padding: 4px 14px; -webkit-transform: rotate(-12deg); border-radius: 8px; }
""", title="Invoice INV-2026-0417")

report_sections = []
for i, t in enumerate(["Executive summary", "Market overview", "Financial results", "Operations", "Outlook"], 1):
    report_sections.append(
        "<h1>%d. %s</h1>%s<h2>%d.1 Key figures</h2>"
        "<table class=\"data\"><tr><th>Indicator</th><th>2024</th><th>2025</th><th>Change</th></tr>"
        "<tr><td>Revenue (M€)</td><td>%d.4</td><td>%d.9</td><td class=\"up\">+%d.2%%</td></tr>"
        "<tr><td>EBITDA margin</td><td>1%d.1%%</td><td>1%d.8%%</td><td class=\"up\">+0.7 pt</td></tr>"
        "<tr><td>Headcount</td><td>%d</td><td>%d</td><td class=\"down\">−%d</td></tr></table>"
        "%s<div class=\"figure\"><img src=\"../assets/chart.svg\" width=\"360\"><p class=\"cap\">Figure %d — quarterly revenue</p></div>"
        "<h2>%d.2 Commentary</h2>%s<div class=\"callout\"><b>Note.</b> %s</div>"
        % (i, t, paras(2), i, 40 + i, 44 + i, i, i, i, 300 + i * 11, 296 + i * 11, 4, paras(1), i, i, paras(3), SHORT))
page("doc-report", "\n".join(report_sections), """
body { font-family: Georgia, "Times New Roman", serif; font-size: 13px; line-height: 1.5; color: #222; }
h1 { font-family: Helvetica, Arial, sans-serif; color: #0b4f6c; border-bottom: 2px solid #0b4f6c; padding-bottom: 4px; page-break-before: always; }
h1:first-child { page-break-before: avoid; }
h2 { font-family: Helvetica, Arial, sans-serif; font-size: 16px; color: #333; margin-top: 18px; page-break-after: avoid; }
p { text-align: justify; margin: 0 0 8px; }
.data { border-collapse: collapse; width: 70%; margin: 8px 0 12px; font-family: Helvetica, Arial, sans-serif; font-size: 12px; }
.data th { background: #0b4f6c; color: #fff; padding: 4px 8px; text-align: left; } .data td { border-bottom: 1px solid #ccc; padding: 4px 8px; }
.up { color: #2e7d32; } .down { color: #c62828; }
.figure { text-align: center; margin: 12px 0; page-break-inside: avoid; } .cap { text-align: center; font-style: italic; font-size: 11px; color: #666; }
.callout { background: #fff8e1; border: 1px solid #ffcc80; border-radius: 4px; padding: 8px 12px; margin: 10px 0; }
""", title="Annual Report 2025")

page("doc-letter", """
<div class="lh"><img src="../assets/logo.png" height="40"><div class="addr">ACME Industries SAS · 12 rue de l'Exemple · 75011 Paris<br>+33 1 23 45 67 89 · contact@acme.example</div></div>
<div class="to">Ms Jane Doe<br>Head of Procurement<br>Globex Corporation<br>4 Industrial Way<br>Springfield, IL 62701<br>USA</div>
<p class="date">Paris, 1 October 2026</p>
<p class="subj"><b>Subject:</b> Renewal of the maintenance agreement no. MA-2023-118</p>
<p>Dear Ms Doe,</p>
%s
<p>We remain at your disposal for any further information and look forward to continuing our collaboration.</p>
<p>Yours sincerely,</p>
<div class="sig"><div class="line"></div>John Smith<br><i>Key Account Manager</i></div>
<div class="enc">Enclosures: price schedule (2 pages), general terms and conditions.</div>
""" % paras(4), """
body { font-family: "Times New Roman", Times, serif; font-size: 14px; line-height: 1.45; color: #111; }
.lh { border-bottom: 1px solid #1e3c8c; padding-bottom: 8px; margin-bottom: 30px; overflow: hidden; } .lh img { float: left; }
.addr { float: right; text-align: right; font-family: Arial, sans-serif; font-size: 10px; color: #555; }
.to { margin: 0 0 30px 55%; line-height: 1.35; } .date { text-align: right; } .subj { margin: 24px 0; }
p { text-align: justify; } .sig { margin-top: 40px; margin-left: 55%; } .sig .line { width: 180px; height: 40px; border-bottom: 1px solid #333; margin-bottom: 4px; }
.enc { margin-top: 40px; font-size: 11px; color: #555; border-top: 1px dotted #999; padding-top: 6px; }
""", title="Letter")

stmt_rows = []
bal = 15234.18
for i in range(70):
    amt = ((i * 7919) % 3000 - 1400) / 7.0
    bal += amt
    stmt_rows.append('<tr><td>2026-%02d-%02d</td><td>%s</td><td class="n">%s</td><td class="n">%s</td><td class="n">%.2f</td></tr>' % (
        7 + i // 25, i % 25 + 1, ["CARD PAYMENT SUPERMARKET 0423", "SEPA TRANSFER SALARY ACME", "DIRECT DEBIT ELECTRICITY",
                                  "ATM WITHDRAWAL PARIS 11", "CHEQUE 0001234", "TRANSFER TO SAVINGS ACCOUNT 778"][i % 6],
        "%.2f" % -amt if amt < 0 else "", "%.2f" % amt if amt >= 0 else "", bal))
page("doc-statement", """
<table class="top"><tr><td><img src="../assets/logo.png" height="36"><br><b>Bank of Example</b><br>Branch 0042 — Paris Bastille</td>
<td class="r"><h1>Account statement</h1>Period 1 July — 30 September 2026<br>Account FR76 3000 6000 0112 3456 7890 189</td></tr></table>
<div class="holder"><b>Mr John Smith</b><br>8 avenue du Test<br>75012 Paris</div>
<table class="sum"><tr><td>Opening balance<br><b>15 234.18 €</b></td><td>Total debits<br><b>8 412.55 €</b></td><td>Total credits<br><b>9 101.20 €</b></td><td>Closing balance<br><b>15 922.83 €</b></td></tr></table>
<table class="tx"><thead><tr><th>Date</th><th>Label</th><th class="n">Debit</th><th class="n">Credit</th><th class="n">Balance</th></tr></thead><tbody>
%s
</tbody></table>
<p class="legal">This statement is provided for information. Any discrepancy must be reported within 30 days. Bank of Example, SA with a capital of 1 000 000 €, RCS Paris 000 000 000.</p>
""" % "\n".join(stmt_rows), """
body { font-family: Verdana, "DejaVu Sans", sans-serif; font-size: 10px; color: #222; }
.top { width: 100%; } .top td { vertical-align: top; } .r { text-align: right; } h1 { margin: 0 0 4px; font-size: 18px; color: #00695c; }
.holder { margin: 16px 0 16px 60%; padding: 8px; border: 1px solid #b2dfdb; }
.sum { width: 100%; border-collapse: separate; border-spacing: 6px; margin-bottom: 10px; } .sum td { background: #e0f2f1; padding: 8px; text-align: center; border-radius: 4px; }
.tx { width: 100%; border-collapse: collapse; } .tx thead { display: table-header-group; } .tx th { border-bottom: 2px solid #00695c; text-align: left; padding: 4px; color: #00695c; }
.tx td { padding: 3px 4px; border-bottom: 1px dotted #bbb; } .tx tr { page-break-inside: avoid; } .n { text-align: right; }
.legal { margin-top: 16px; font-size: 8px; color: #777; }
""", title="Statement")

page("doc-bootstrap", """
<div class="container">
<div class="page-header"><h1>Order summary <small>#58213</small></h1></div>
<div class="row"><div class="col-xs-8"><div class="alert alert-info"><strong>Heads up!</strong> Delivery is scheduled for next Tuesday between 9:00 and 12:00.</div></div>
<div class="col-xs-4"><div class="well well-sm text-center"><span class="label label-success">Paid</span> <span class="label label-warning">Pending shipment</span></div></div></div>
<div class="row">
<div class="col-xs-6"><div class="panel panel-default"><div class="panel-heading"><h3 class="panel-title">Customer</h3></div><div class="panel-body">Jane Doe<br>jane@example.com<br>+1 555 0100</div></div></div>
<div class="col-xs-6"><div class="panel panel-primary"><div class="panel-heading"><h3 class="panel-title">Shipping address</h3></div><div class="panel-body">4 Industrial Way<br>Springfield, IL 62701<br>United States</div></div></div>
</div>
<table class="table table-striped table-bordered table-condensed"><thead><tr><th>#</th><th>Product</th><th class="text-right">Qty</th><th class="text-right">Price</th></tr></thead><tbody>
%s
</tbody><tfoot><tr><th colspan="3" class="text-right">Total</th><th class="text-right">$1,284.00</th></tr></tfoot></table>
<div class="row"><div class="col-xs-3"><div class="thumbnail"><img src="../assets/photo.jpg"><div class="caption">Photo</div></div></div>
<div class="col-xs-3"><div class="thumbnail"><img src="../assets/photo.png"><div class="caption">Render</div></div></div>
<div class="col-xs-6"><p class="lead">Thank you for your order.</p><p>%s</p>
<p><a class="btn btn-primary">Track order</a> <a class="btn btn-default">Contact us</a> <a class="btn btn-danger btn-sm">Cancel</a></p></div></div>
<ul class="list-group"><li class="list-group-item">Free returns within 30 days <span class="badge">new</span></li><li class="list-group-item">Two-year warranty</li></ul>
<div class="progress"><div class="progress-bar" style="width: 60%%">60%%</div></div>
</div>
""" % ("\n".join('<tr><td>%d</td><td>Product %s</td><td class="text-right">%d</td><td class="text-right">$%d.00</td></tr>' % (i, chr(64 + i), i, i * 37) for i in range(1, 9)), SHORT),
    "", title="Bootstrap order", head='<link rel="stylesheet" href="%s">\n' % BS)

page("doc-certificate", """
<div class="frame"><div class="inner">
<div class="small">ACME ACADEMY</div>
<h1>Certificate of Completion</h1>
<p class="pre">This is to certify that</p>
<div class="name">Ada Lovelace</div>
<p class="pre">has successfully completed the course</p>
<div class="course">Advanced Analytical Engines — 40 hours</div>
<table class="sign"><tr><td><div class="l"></div>Director of Studies</td><td><div class="seal">★</div></td><td><div class="l"></div>Date: 1 October 2026</td></tr></table>
</div></div>
""", """
body { margin: 0; font-family: Georgia, serif; text-align: center; color: #3b2f1e; }
.frame { border: 12px double #b08d57; padding: 8px; height: 560px; } .inner { border: 2px solid #b08d57; height: 100%; padding-top: 30px; box-sizing: border-box;
  background: -webkit-radial-gradient(center, ellipse cover, #fffdf5, #f3e9d2); }
.small { letter-spacing: 6px; font-size: 13px; color: #8a6d3b; } h1 { font-size: 44px; font-weight: normal; margin: 14px 0 20px; font-variant: small-caps; }
.pre { font-style: italic; font-size: 16px; margin: 6px; } .name { font-size: 38px; font-family: "DejaVu Sans", sans-serif; border-bottom: 1px solid #b08d57; display: inline-block; padding: 0 40px 4px; margin: 6px 0 12px; }
.course { font-size: 20px; font-weight: bold; } .sign { width: 80%; margin: 50px auto 0; } .sign td { width: 33%; font-size: 12px; vertical-align: bottom; }
.l { border-bottom: 1px solid #3b2f1e; margin: 0 20px 4px; height: 30px; } .seal { font-size: 50px; color: #b08d57; width: 80px; height: 80px; line-height: 80px; margin: auto; border-radius: 40px; border: 3px solid #b08d57; }
""", title="Certificate")

page("doc-resume", """
<div class="side"><img src="../assets/photo.jpg" class="ph"><h3>Contact</h3><p>jane.doe@example.com<br>+33 6 12 34 56 78<br>Paris, France</p>
<h3>Skills</h3><ul><li>Rust, C++, Python</li><li>Distributed systems</li><li>PDF internals</li><li>Team leadership</li></ul>
<h3>Languages</h3><p>French (native)<br>English (fluent)<br>German (B1)</p>
<div class="bar"><span style="width:90%%"></span></div><div class="bar"><span style="width:60%%"></span></div></div>
<div class="main"><h1>Jane Doe</h1><div class="role">Senior Software Engineer</div>
<h2>Experience</h2>
<div class="job"><div class="when">2021 — present</div><b>Lead engineer</b>, ACME Industries<p>%(l)s</p></div>
<div class="job"><div class="when">2017 — 2021</div><b>Software engineer</b>, Globex<p>%(l)s</p></div>
<div class="job"><div class="when">2014 — 2017</div><b>Developer</b>, Initech<p>%(s)s</p></div>
<h2>Education</h2><div class="job"><div class="when">2009 — 2014</div><b>MSc Computer Science</b>, Example University</div>
</div>
""" % {"l": LOREM, "s": SHORT}, """
body { font-family: "DejaVu Sans", Arial, sans-serif; font-size: 11px; color: #333; margin: 0; }
.side { float: left; width: 28%; background: #263238; color: #eceff1; padding: 14px; min-height: 860px; box-sizing: border-box; }
.side h3 { color: #80cbc4; text-transform: uppercase; font-size: 11px; letter-spacing: 2px; border-bottom: 1px solid #546e7a; padding-bottom: 3px; }
.side ul { padding-left: 14px; } .ph { width: 100%; border-radius: 6px; }
.bar { height: 6px; background: #546e7a; margin: 6px 0; } .bar span { display: block; height: 6px; background: #80cbc4; }
.main { margin-left: 31%; } h1 { font-size: 30px; margin: 0; color: #263238; } .role { font-size: 15px; color: #00897b; margin-bottom: 10px; }
h2 { font-size: 14px; text-transform: uppercase; color: #00897b; border-bottom: 2px solid #00897b; margin-top: 18px; }
.job { margin-bottom: 10px; } .when { float: right; color: #888; font-style: italic; } .job p { margin: 3px 0; text-align: justify; }
""", title="Resume")

page("doc-packing-slip", """
<table class="hd"><tr><td><img src="../assets/logo.png" height="30"></td><td class="r"><h1>PACKING SLIP</h1>Order 58213 · 2026-10-01</td></tr></table>
<table class="addr"><tr><td><h4>From</h4>ACME Fulfilment<br>Zone Industrielle Nord<br>69000 Lyon</td><td><h4>To</h4>Jane Doe<br>4 Industrial Way<br>Springfield, IL 62701</td><td class="bc"><div class="barcode">%s</div>1Z 999 AA1 01 2345 6784</td></tr></table>
<table class="items"><tr><th>✓</th><th>Item</th><th>SKU</th><th>Ordered</th><th>Shipped</th></tr>
%s
</table>
<div class="note">Gift message: <i>Happy birthday! Enjoy the new widgets.</i></div>
<div class="cut">✂ - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - -</div>
<h3>Returns form</h3><table class="ret"><tr><td>Reason</td><td class="box"></td></tr><tr><td>Item(s)</td><td class="box"></td></tr><tr><td>Signature</td><td class="box"></td></tr></table>
""" % ("".join('<span style="width:%dpx"></span>' % (1 + (i * 7) % 4) for i in range(40)),
       "\n".join('<tr><td><span class="cb"></span></td><td>%s</td><td>SKU-%04d</td><td>%d</td><td>%d</td></tr>' % (
           ["Industrial widget", "Bracket kit", "Filter cartridge", "Manual (printed)", "Spare screws"][i], 1000 + i * 37, i + 1, i + 1) for i in range(5))), """
body { font-family: "Courier New", Courier, monospace; font-size: 12px; }
.hd, .addr { width: 100%; } .r { text-align: right; } h1 { margin: 0; font-family: Arial, sans-serif; letter-spacing: 3px; } h4 { margin: 0 0 4px; text-transform: uppercase; }
.addr td { vertical-align: top; padding: 10px 0; border-top: 2px solid #000; border-bottom: 2px solid #000; }
.bc { text-align: center; font-size: 10px; } .barcode { height: 50px; } .barcode span { display: inline-block; height: 50px; background: #000; margin-right: 2px; }
.items { width: 100%; border-collapse: collapse; margin-top: 14px; } .items th { border-bottom: 2px solid #000; text-align: left; } .items td { border-bottom: 1px dashed #666; padding: 6px 2px; }
.cb { display: inline-block; width: 12px; height: 12px; border: 1px solid #000; }
.note { margin: 14px 0; padding: 8px; border: 1px dashed #000; } .cut { margin: 24px 0 10px; color: #666; white-space: nowrap; overflow: hidden; }
.ret { width: 100%; } .ret td { padding: 4px; } .ret .box { border-bottom: 1px solid #000; width: 80%; height: 22px; }
""", title="Packing slip")


def main():
    os.makedirs(OUT, exist_ok=True)
    for name, html in CASES.items():
        fname = name if name.endswith(".html") else name + ".html"
        with open(os.path.join(OUT, fname), "w", encoding="utf-8") as f:
            f.write(html)
    print("%d files written to %s" % (len(CASES), OUT))


if __name__ == "__main__":
    main()
