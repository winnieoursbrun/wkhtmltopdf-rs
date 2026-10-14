# wkhtmltopdf: upstream behaviour specification (PDF tool only)

Reference spec for a Rust drop-in replacement of `wkhtmltopdf`. It covers the PDF binary only (not
`wkhtmltoimage`, not the `libwkhtmltox` C API) and focuses on what the `pdfkit` Ruby gem relies on.

Sources used:

| Source | What it is | Used for |
|---|---|---|
| `upstream/src/pdf`, `upstream/src/lib`, `upstream/src/shared` | C++ source, `VERSION` = `0.12.7-dev` (master after 0.12.6) | Logic, defaults, format strings |
| `upstream/docs/` | Website and doxygen docs | Cross-checks only |
| Official `0.12.6 (with patched qt)` linux-amd64 binary (from the `wkhtmltopdf-binary-edge-0.12.6.0` gem), run in an `ubuntu:20.04` amd64 container | The release almost every user runs | Byte-exact reference output, see `docs/upstream-fixtures/0.12.6/` |
| `pdfkit` 0.8.7.3 (same as GitHub `master`) | The client we must satisfy | Section 7 |

**Target version.** The source tree is `0.12.7-dev`. The deployed release is `0.12.6`. The two
differ in a handful of user-visible strings and in logging (listed in section 9). The recommendation
is to **reproduce 0.12.6 byte-for-byte**, since that is what clients have been tested against, and
to treat the 0.12.7-dev differences as optional.

**Patched vs unpatched Qt.** Most features exist only when wkhtmltopdf is compiled against the
patched Qt (`__EXTENSIVE_WKHTMLTOPDF_QT_HACK__` defined). All official release binaries are patched,
and `--version` prints `(with patched qt)`. **The target is the patched behaviour.** Unpatched
behaviour is documented only where it changes user-visible output (section 1.9).

Notation:
- `QHACK`: the option is flagged "only available with patched Qt".
- `EXT`: the option only appears in `--extended-help`, not in `--help`.
- Scope:
  - `G` (global) is only accepted before the first object.
  - `P` (page) is accepted in the global area, where it becomes the default for every object, or
    after a page, cover or toc object.
  - `T` (toc) is accepted only after a `toc` object.

---

## 1. Command line

### 1.1 Grammar

```
wkhtmltopdf [GLOBAL OPTION]... [OBJECT]... <output file>

OBJECT := (page)? <input url/file name> [PAGE OPTION]...
        | cover <input url/file name> [PAGE OPTION]...
        | toc [TOC OPTION]...          (TOC OPTION = page options + TOC options)
```

The exact algorithm comes from `PdfCommandLineParser::parseArguments`, quoted here because every
quirk matters:

```cpp
void PdfCommandLineParser::parseArguments(int argc, const char ** argv, bool fromStdin) {
	bool defaultMode = false;
	int arg=1;
	PdfObject def;
	//Parse global options
	for (;arg < argc;++arg) {
		if (argv[arg][0] != '-' || argv[arg][1] == '\0' || defaultMode) break;
		parseArg(global | page, argc, argv, defaultMode, arg, (char *)&def);
	}
	if (readArgsFromStdin && !fromStdin) return;
	//Parse page options
	while (arg < argc-1) {
		pageSettings.push_back(def);
		PdfObject & ps = pageSettings.back();
		int sections = page;
		if (!strcmp(argv[arg],"cover")) {
			++arg;
			if (arg >= argc-1) {
				fprintf(stderr, "You need to specify a input file to cover\n\n");
				usage(stderr, false);
				exit(1);
			}
			ps.page = QString::fromLocal8Bit(argv[arg++]);
			// parse page options and then override the header/footer settings
			for (;arg < argc;++arg) {
				if (argv[arg][0] != '-' || argv[arg][1] == '\0' || defaultMode) break;
				parseArg(sections, argc, argv, defaultMode, arg, (char*)&ps);
			}
			ps.header.left = ps.header.right = ps.header.center = "";
			ps.footer.left = ps.footer.right = ps.footer.center = "";
			ps.header.line = ps.footer.line = false;
			ps.header.htmlUrl = ps.footer.htmlUrl = "";
			ps.includeInOutline = false;
			continue;
		} else if (!strcmp(argv[arg],"toc")) {
			++arg;
			sections = page | toc;
			ps.isTableOfContent = true;
		} else {
			if (!strcmp(argv[arg],"page")) {
				++arg;
				if (arg >= argc-1) {
					fprintf(stderr, "You need to specify a input file to page\n\n");
					usage(stderr, false);
					exit(1);
				}
			}
			QByteArray a(argv[arg]);
			ps.page = QString::fromLocal8Bit(a);
			++arg;
		}
		for (;arg < argc;++arg) {
			if (argv[arg][0] != '-' || argv[arg][1] == '\0' || defaultMode) break;
			parseArg(sections, argc, argv, defaultMode, arg, (char*)&ps);
		}
	}
	if (pageSettings.size() == 0 || argc < 2) {
		fprintf(stderr, "You need to specify at least one input file, and exactly one output file\nUse - for stdin or stdout\n\n");
		usage(stderr, false);
		exit(1);
	}
	globalSettings.out = QString::fromLocal8Bit(argv[argc-1]);
}
```

What follows from it:

1. **Global area.** Every leading argument that starts with `-` and is not exactly `-` is an option
   parsed with scope `global | page`. Page-scoped options given here are written into a template
   object `def`, which every later object copies when it is created. Global-scoped options are
   written straight into the global settings.
2. **The last argument is always the output file.** It is never parsed as an option or an object,
   even if it starts with `-`. For example, `wkhtmltopdf a.html --print-media-type` treats
   `--print-media-type` as a page option of `a.html` (consuming it), and then still uses it as the
   output file name. `-` as the output means stdout.
3. **Object keywords.** `cover`, `toc` and `page` are recognised by exact, case-sensitive string
   comparison. Any other token is an input. A file literally named `toc` must be written as
   `page toc`.
4. **Options after an object** that start with `-` (and are not exactly `-`) belong to that object.
   The scope there is `page` for page and cover objects, and `page|toc` for toc objects.
   - A global-only option after an object fails with `--X specified in incorrect location`.
   - A toc-only option in the global area or after a page object fails the same way.
5. **`toc` has no file argument** and no "missing input" check. `wkhtmltopdf toc out.pdf` is valid.
6. **`cover`.** After its options are parsed, the cover's header/footer texts and HTML, header and
   footer lines, and `includeInOutline` are forcibly cleared. A cover therefore never has
   headers or footers and never appears in the outline or TOC, but **it still counts for page
   numbering** (`pagesCount` stays true).
7. **Stdin input.** An input of `-` reads stdin. `-` alone ends the global area because
   `argv[1] == '\0'`.
8. **Two non-option arguments.** `wkhtmltopdf a.html b.html` gives one object (`a.html`) and output
   `b.html`, which silently overwrites `b.html`. Exit 0.
9. **No object.** With no object (`pageSettings.size()==0`), or with no argument at all, the program
   prints the "You need to specify..." message, then `usage(stderr,false)`, and exits 1.
10. **The `--` default mode is broken.** The intent was that `--` switches to "default mode". The
    code instead tests `argv[arg][2] == '0'` (the character `'0'`, not NUL). Consequences:
    - Any long option beginning with `0` (for example `--0`) sets `defaultMode = true` and is
      otherwise ignored.
    - A plain `--` is looked up as the long option `""` and fails with `Unknown long argument --`.
    - In default mode, the option loops stop at the next argument.
11. **Read-args-from-stdin.** When `--read-args-from-stdin` was seen in the global area of the real
    command line, parsing stops right after the global area. See section 1.7.

### 1.2 Option parsing (`CommandLineParserBase::parseArg`)

Long options (`--name`):

- Lookup is an exact, case-sensitive match on the text after `--`. There is no `--name=value` form
  and no abbreviation.
- Errors, checked in this order. Each prints to stderr, then `usage(stderr,false)` (section 2), then
  exits 1:

| Condition | Message (exact; `%s` is the full argv token, including the `--`) |
|---|---|
| Unknown option | `Unknown long argument %s\n\n` |
| Wrong scope | `%s specified in incorrect location\n\n` |
| `argc-arg < nargs+1` | `Not enough arguments parsed to %s\n\n` |
| Handler returned false | `Invalid argument(s) parsed to %s\n\n` |

- Option arguments are the following argv entries, taken verbatim. They may start with `-` and may
  even be the output file name. The arity check only makes sure enough argv entries exist; it does
  not keep the last one in reserve for the output.

Short options (`-x`, groupable as `-xyz`):

- Each character is looked up in turn. The same four checks apply, with messages
  `Unknown switch -%c`, `-%c specified in incorrect location`, `Not enough arguments parsed to -%c`
  and `Invalid argument(s) parsed to -%c`.
- Each switch that takes arguments consumes the next argv entries, and the index moves on.
  `-Tq 5mm` sets `-T 5mm` and `-q`. `-qV` prints the version.
- `-` alone is never an option (see 1.1).

Under unpatched Qt only, a `QHACK` option still gets parsed and stored, but this is printed
**without a trailing newline**:
`The switch --%s is not supported when using unpatched qt and will be ignored.`
(The short form is `The switch -%c is not ...`.)

**Action options run during parsing**, as soon as they are reached, and call `exit(0)`:
`--help`, `-h`, `--extended-help`, `-H`, `--version`, `-V`, `--license`, `--manpage`, `--htmldoc`,
`--readme` and `--dump-default-toc-xsl`. Consequences:

- Arguments that come later are never validated. `wkhtmltopdf --version --bogus` prints the version
  and exits 0.
- An earlier invalid argument wins. `--bogus --version` gives the usage error and exits 1.
- Options already parsed **change the "(default …)" values shown in help**, because help prints the
  current value of each destination field (see 2.3). For example `--dpi 300 --extended-help` shows
  `(default 300)`, and `-q --help` shows `--log-level ... (default none)` and `--quiet ... (default)`.
- `--dump-default-toc-xsl` always dumps with the default TOC settings. Toc options cannot precede
  it, because it is global and toc options are only valid after `toc`.

### 1.3 Value parsers

| Type | Accepts | Invalid → |
|---|---|---|
| int (`IntSetter`) | `QString::toInt(&ok)`: base 10, optional sign, surrounding whitespace allowed | `Invalid argument(s)` |
| float (`FloatSetter`) | `QString::toFloat(&ok)`: sign, decimal, exponent | same |
| string (`QStrSetter`) | anything, `QString::fromLocal8Bit` | never |
| bool switches (`ConstSetter`) | no argument; assign a constant | never |
| log level | case-insensitive `none\|error\|warn\|info\|debug` in source; **0.12.6 rejects `debug`** | same |
| orientation | case-insensitive `Landscape\|Portrait` (`strcasecmp`) | same |
| page size | case-insensitive match against the table below | same |
| load error handling | case-insensitive `abort\|skip\|ignore` | same |
| unitreal | see below | same |
| proxy | see below | same |

**unitreal** (`strToUnitReal`):

- The value is `[0-9]*` `[.]?` `[0-9]*`, followed by a unit suffix compared case-insensitively.
- There is no sign and no exponent, so `-1cm` and `1e2mm` are invalid.
- The number is converted with `QString::toDouble`, so `""` and `"."` are invalid, while `.5` and
  `5.` are fine.

| Suffix | Internal unit, scale |
|---|---|
| empty, `mm`, `millimeter` | Millimeter ×1 |
| `cm`, `centimeter` | Millimeter ×10 |
| `m`, `meter` | Millimeter ×1000 |
| `didot` | Didot |
| `inch`, `in` | Inch |
| `pica`, `pc` | Pica |
| `cicero` | Cicero |
| `pixel`, `px` | DevicePixel |
| `point`, `pt` | Point |

A value with no unit (for example `0.75`) means millimetres. The display form used in help is
`QString("%1%2").arg(value).arg(unit)` with unit names `didot`, `in`, `pica`, `px`, `pt` and `mm`
(Cicero gives no default text). A value of `-1` means "unset" and shows no default.

**Page sizes**, case-insensitive (`QMap` order, used for display): `A0`–`A9`, `B0`, `B1`, `B10`,
`B2`–`B9`, `C5E`, `Comm10E`, `DLE`, `Executive`, `Folio`, `Ledger`, `Legal`, `Letter`, `Tabloid`.
The display name is the canonical case from this list.

**Proxy** (`strToProxy`):

- `none` (exact, lowercase) means no proxy, and is OK.
- Otherwise the type is HTTP, or SOCKS5 if the value starts with `socks5://`. A leading `http://`
  is stripped.
- `user[:password]@` is split on the **last** `@`, and the password on the first `:`.
- The host and port are split on the first `:`. The port defaults to 1080 and must parse as an int
  in 0..65535.
- An empty host is invalid.

The help text says `"None"`, but only lowercase `none` is special.

### 1.4 Complete option table

Columns:
- **Sect**: the help section.
- **Scope**: G, P or T.
- **Ext**: `EXT` = only in extended help.
- **QH**: `QHACK`.
- **Field**: the settings field written.
- **Default**: the initial value. "(default …)" is the text help prints.

`PdfGlobal` fields are written directly. For P/T options with a `PdfObject` field, the value goes
into the current object, or into `def` when given in the global area.

> **Gotcha.** `--page-offset`, `--resolve-relative-links`, `--keep-relative-links`,
> `--viewport-size` and `--default-header` are *page-scoped* but write **global** fields. Writing
> them after any object changes the global setting. `--default-header` writes both the object
> header and the global `margin.top`.

#### Global Options (section "Global Options", scope G)

| Long | Short | Args | Ext | QH | Field | Default / help text | Description (0.12.6 text) |
|---|---|---|---|---|---|---|---|
| `--help` | `-h` | – | | | action | – | Display help |
| `--version` | `-V` | – | | | action | – | Output version information and exit |
| `--license` | | – | | | action | – | Output license information and exit |
| `--extended-help` | `-H` | – | | | action | – | Display more extensive help, detailing less common command switches |
| `--manpage` | | – | EXT | | action | – | Output program man page |
| `--htmldoc` | | – | EXT | | action | – | Output program html help |
| `--readme` | | – | EXT | | action | – | Output program readme |
| `--quiet` | `-q` | – | | | `logLevel=None` | info → no "(default)" | Be less verbose, maintained for backwards compatibility; Same as using --log-level none |
| `--log-level` | | `<level>` | | | `logLevel` | `info` "(default info)" | Set log level to: none, error, warn or info (source: "none, error, warn, info or debug") |
| `--no-collate` | | – | | | `collate=false` | | Do not collate when printing multiple copies |
| `--collate` | | – | | | `collate=true` | true "(default)" | Collate when printing multiple copies |
| `--copies` | | `<number>` | | | `copies` | 1 | Number of copies to print into the pdf file |
| `--orientation` | `-O` | `<orientation>` | | | `orientation` | Portrait | Set orientation to Landscape or Portrait |
| `--page-size` | `-s` | `<Size>` | | | `size.pageSize` | A4 | Set paper size to: A4, Letter, etc. |
| `--grayscale` | `-g` | – | | | `colorMode=GrayScale` | Color | PDF will be generated in grayscale |
| `--lowquality` | `-l` | – | | | `resolution=ScreenResolution` | HighResolution | Generates lower quality pdf/ps. Useful to shrink the result document space |
| `--title` | | `<text>` | | | `documentTitle` | "" | The title of the generated pdf file (The title of the first document is used if not specified) |
| `--read-args-from-stdin` | | – | | | parser `readArgsFromStdin=true` | false | Read command line arguments from stdin |
| `--margin-bottom` | `-B` | `<unitreal>` | EXT | | `margin.bottom` | -1 (unset → 10mm) | Set the page bottom margin |
| `--margin-left` | `-L` | `<unitreal>` | EXT | | `margin.left` | 10mm | Set the page left margin |
| `--margin-right` | `-R` | `<unitreal>` | EXT | | `margin.right` | 10mm | Set the page right margin |
| `--margin-top` | `-T` | `<unitreal>` | EXT | | `margin.top` | -1 (unset → 10mm) | Set the page top margin |
| `--dpi` | `-d` | `<dpi>` | EXT | | `dpi` | 96 | Change the dpi explicitly (this has no effect on X11 based systems) |
| `--page-height` | | `<unitreal>` | EXT | | `size.height` | -1 | Page height |
| `--page-width` | | `<unitreal>` | EXT | | `size.width` | -1 | Page width |
| `--cookie-jar` | | `<path>` | EXT | | `load.cookieJar` | "" | Read and write cookies from and to the supplied cookie jar file |
| `--image-quality` | | `<integer>` | EXT | QH | `imageQuality` | 94 | When jpeg compressing images use this quality |
| `--image-dpi` | | `<integer>` | EXT | QH | `imageDPI` | 600 | When embedding images scale them down to this dpi |
| `--no-pdf-compression` | | – | EXT | QH | `useCompression=false` | true | Do not use lossless compression on pdf objects |
| `--use-xserver` | | – | EXT | QH | `useGraphics=true` | false | Use the X server (some plugins and other stuff might not work without X11). **Unix only.** |

#### Outline Options (section "Outline Options", scope G, all EXT and QH)

| Long | Args | Field | Default | Description |
|---|---|---|---|---|
| `--outline` | – | `outline=true` | true "(default)" | Put an outline into the pdf |
| `--no-outline` | – | `outline=false` | | Do not put an outline into the pdf |
| `--outline-depth` | `<level>` | `outlineDepth` | 4 | Set the depth of the outline |
| `--dump-outline` | `<file>` | `dumpOutline` | "" | Dump the outline to a file |
| `--dump-default-toc-xsl` | – | action | – | Dump the default TOC xsl style sheet to stdout |

#### Page Options (section "Page Options", scope P)

All are EXT except `--print-media-type` and `--no-print-media-type`. The table is in source order;
help sorts it (see 2.3).

| Long | Short | Args | QH | Field (on object unless noted) | Default | Description |
|---|---|---|---|---|---|---|
| `--default-header` | | – | QH | `header.left="[webpage]"`, `header.right="[page]/[topage]"`, `header.line=true`, **global** `margin.top=2cm` | | Add a default header… (text differs; see section 9) |
| `--viewport-size` | | `<>` (empty arg name!) | QH | **global** `viewportSize` | "" | Set viewport size if you have custom scrollbars or css attribute overflow to emulate window size |
| `--enable-plugins` | | – | | `web.enablePlugins=true` | false | Enable installed plugins (plugins will likely not work) |
| `--disable-plugins` | | – | | `web.enablePlugins=false` | "(default)" | Disable installed plugins |
| `--minimum-font-size` | | `<int>` | | `web.minimumFontSize` | -1 (no default shown) | Minimum font size |
| `--user-style-sheet` | | `<path>` | | `web.userStyleSheet` | "" | Specify a user style sheet, to load with every page |
| `--no-images` | | – | | `web.loadImages=false` | | Do not load or print images |
| `--images` | | – | | `web.loadImages=true` | "(default)" | Do load or print images |
| `--disable-javascript` | `-n` | – | | `web.enableJavascript=false` | | Do not allow web pages to run javascript |
| `--enable-javascript` | | – | | `web.enableJavascript=true` | "(default)" | Do allow web pages to run javascript |
| `--encoding` | | `<encoding>` | | `web.defaultEncoding` | "" | Set the default text encoding, for input |
| `--no-background` | | – | | `web.background=false` | | Do not print background |
| `--background` | | – | | `web.background=true` | "(default)" | Do print background |
| `--include-in-outline` | | – | QH | `includeInOutline=true` | "(default)" | Include the page in the table of contents and outlines |
| `--exclude-from-outline` | | – | QH | `includeInOutline=false` | | Do not include the page in the table of contents and outlines |
| `--disable-smart-shrinking` | | – | QH | `web.enableIntelligentShrinking=false` | | Disable the intelligent shrinking strategy used by WebKit that makes the pixel/dpi ratio non-constant |
| `--enable-smart-shrinking` | | – | QH | `web.enableIntelligentShrinking=true` | "(default)" | Enable the intelligent shrinking strategy used by WebKit that makes the pixel/dpi ratio non-constant |
| `--print-media-type` | | – | QH (non-EXT) | `load.printMediaType=true` | | Use print media-type instead of screen |
| `--no-print-media-type` | | – | QH (non-EXT) | `load.printMediaType=false` | "(default)" | Do not use print media-type instead of screen |
| `--enable-forms` | | – | QH | `produceForms=true` | | Turn HTML form fields into pdf form fields |
| `--disable-forms` | | – | QH | `produceForms=false` | "(default)" | Do not turn HTML form fields into pdf form fields |
| `--disable-internal-links` | | – | QH | `useLocalLinks=false` | | Do not make local links |
| `--enable-internal-links` | | – | QH | `useLocalLinks=true` | "(default)" | Make local links |
| `--disable-external-links` | | – | QH | `useExternalLinks=false` | | Do not make links to remote web pages |
| `--enable-external-links` | | – | QH | `useExternalLinks=true` | "(default)" | Make links to remote web pages |
| `--resolve-relative-links` | | – | QH | **global** `resolveRelativeLinks=true` | "(default)" | Resolve relative external links into absolute links |
| `--keep-relative-links` | | – | QH | **global** `resolveRelativeLinks=false` | | Keep relative external links as relative external links |
| `--enable-toc-back-links` | | – | QH | `toc.backLinks=true` | | Link from section header to toc |
| `--disable-toc-back-links` | | – | QH | `toc.backLinks=false` | "(default)" | Do not link from section header to toc |
| `--proxy` | `-p` | `<proxy>` | | `load.proxy` | none | Use a proxy |
| `--proxy-hostname-lookup` | | – | | `load.proxyHostNameLookup=true` | false | Use the proxy for resolving hostnames |
| `--bypass-proxy-for` | | `<value>` | | append `load.bypassProxyForHosts` | [] | Bypass proxy for host (repeatable) |
| `--username` | | `<username>` | | `load.username` | "" | HTTP Authentication username |
| `--password` | | `<password>` | | `load.password` | "" | HTTP Authentication password |
| `--ssl-key-path` | | `<path>` | | `load.clientSslKeyPath` | "" | Path to ssl client cert private key in OpenSSL PEM format |
| `--ssl-key-password` | | `<password>` | | `load.clientSslKeyPassword` | "" | Password to ssl client cert private key |
| `--ssl-crt-path` | | `<path>` | | `load.clientSslCrtPath` | "" | Path to the ssl client cert public key in OpenSSL PEM format, optionally followed by intermediate ca and trusted certs |
| `--load-error-handling` | | `<handler>` | | `load.loadErrorHandling` | abort | Specify how to handle pages that fail to load: abort, ignore or skip |
| `--load-media-error-handling` | | `<handler>` | | `load.mediaLoadErrorHandling` | ignore | Specify how to handle media files that fail to load: abort, ignore or skip |
| `--custom-header` | | `<name> <value>` | | append `load.customHeaders` | [] | Set an additional HTTP header (repeatable) |
| `--custom-header-propagation` | | – | | `load.repeatCustomHeaders=true` | false | Add HTTP headers specified by --custom-header for each resource request. |
| `--no-custom-header-propagation` | | – | | **BUG: also sets `repeatCustomHeaders=true`** | | Do not add HTTP headers specified by --custom-header for each resource request. |
| `--javascript-delay` | | `<msec>` | | `load.jsdelay` | 200 | Wait some milliseconds for javascript finish |
| `--window-status` | | `<windowStatus>` | | `load.windowStatus` | "" | Wait until window.status is equal to this string before rendering page |
| `--zoom` | | `<float>` | | `load.zoomFactor` | 1 | Use this zoom factor |
| `--cookie` | | `<name> <value>` | | append `load.cookies` | [] | Set an additional cookie (repeatable), value should be url encoded. |
| `--post` | | `<name> <value>` | | append `load.post` (file=false) | [] | Add an additional post field (repeatable) |
| `--post-file` | | `<name> <path>` | | append `load.post` (file=true) | [] | Post an additional file (repeatable) |
| `--disable-local-file-access` | | – | | `load.blockLocalFileAccess=true` | **true** "(default)" | Do not allowed conversion of a local file to read in other local files, unless explicitly allowed with --allow |
| `--enable-local-file-access` | | – | | `load.blockLocalFileAccess=false` | | Allowed conversion of a local file to read in other local files. |
| `--allow` | | `<path>` | | append `load.allowed` | [] | Allow the file or files from the specified folder to be loaded (repeatable) |
| `--cache-dir` | | `<path>` | | `load.cacheDir` | "" | Web cache directory |
| `--debug-javascript` | | – | | `load.debugJavascript=true` | | Show javascript debugging output (source: "console output") |
| `--no-debug-javascript` | | – | | `load.debugJavascript=false` | "(default)" | Do not show javascript debugging output |
| `--stop-slow-scripts` | | – | | `load.stopSlowScripts=true` | "(default)" | Stop slow running javascripts |
| `--no-stop-slow-scripts` | | – | | `load.stopSlowScripts=false` | | Do not Stop slow running javascripts |
| `--run-script` | | `<js>` | | append `load.runScript` | [] | Run this additional javascript after the page is done loading (repeatable) |
| `--checkbox-svg` | | `<path>` | | `load.checkboxSvg` | "" | Use this SVG file when rendering unchecked checkboxes |
| `--checkbox-checked-svg` | | `<path>` | | `load.checkboxCheckedSvg` | "" | Use this SVG file when rendering checked checkboxes |
| `--radiobutton-svg` | | `<path>` | | `load.radiobuttonSvg` | "" | Use this SVG file when rendering unchecked radiobuttons |
| `--radiobutton-checked-svg` | | `<path>` | | `load.radiobuttonCheckedSvg` | "" | Use this SVG file when rendering checked radiobuttons |
| `--page-offset` | | `<offset>` | (not QH) | **global** `pageOffset` | **0** "(default 0)" | Set the starting page number |

#### Headers And Footer Options (section "Headers And Footer Options", scope P, all EXT and QH)

| Long | Args | Field | Default | Description |
|---|---|---|---|---|
| `--footer-center` | `<text>` | `footer.center` | "" | Centered footer text |
| `--footer-font-name` | `<name>` | `footer.fontName` | Arial | Set footer font name |
| `--footer-font-size` | `<size>` | `footer.fontSize` (int) | 12 | Set footer font size |
| `--footer-left` | `<text>` | `footer.left` | "" | Left aligned footer text |
| `--footer-line` / `--no-footer-line` | – | `footer.line` | false | Display / Do not display line above the footer |
| `--footer-right` | `<text>` | `footer.right` | "" | Right aligned footer text |
| `--footer-spacing` | `<real>` | `footer.spacing` (float, mm) | 0 | Spacing between footer and content in mm |
| `--footer-html` | `<url>` | `footer.htmlUrl` | "" | Adds a html footer |
| `--header-center` | `<text>` | `header.center` | "" | Centered header text |
| `--header-font-name` | `<name>` | `header.fontName` | Arial | Set header font name |
| `--header-font-size` | `<size>` | `header.fontSize` | 12 | Set header font size |
| `--header-left` | `<text>` | `header.left` | "" | Left aligned header text |
| `--header-line` / `--no-header-line` | – | `header.line` | false | Display / Do not display line below the header |
| `--header-right` | `<text>` | `header.right` | "" | Right aligned header text |
| `--header-spacing` | `<real>` | `header.spacing` | 0 | Spacing between header and content in mm |
| `--header-html` | `<url>` | `header.htmlUrl` | "" | Adds a html header |
| `--replace` | `<name> <value>` | append `replacements` | [] | Replace [name] with value in header and footer (repeatable) |

#### TOC Options (section "TOC Options", scope T only, all EXT and QH)

| Long | Args | Field | Default | Description |
|---|---|---|---|---|
| `--xsl-style-sheet` | `<file>` | `tocXsl` | "" | Use the supplied xsl style sheet for printing the table of contents |
| `--toc-header-text` | `<text>` | `toc.captionText` | Table of Contents | The header text of the toc |
| `--disable-toc-links` | – | `toc.forwardLinks=false` | true | Do not link from toc to sections |
| `--disable-dotted-lines` | – | `toc.useDottedLines=false` | true | Do not use dotted lines in the toc |
| `--toc-text-size-shrink` | `<real>` | `toc.fontScale` | 0.8 | For each level of headings in the toc the font is scaled by this factor |
| `--toc-level-indentation` | `<width>` | `toc.indentation` (free string, CSS length) | 1em | For each level of headings in the toc indent by this length |

There is no `--enable-toc-links` and no `--enable-dotted-lines`. The total is **122 long options**,
the same set in 0.12.6 and in source.

Settings that have no CLI option: `PdfObject.pagesCount` (always true), `isTableOfContent` (set by
`toc`), `page` (the input), `PdfGlobal.out` (last argv) and `load.proxy` defaults.

### 1.5 "--no-x / --x" pairs

| Off | On | Default |
|---|---|---|
| `--no-collate` | `--collate` | on |
| `--no-outline` | `--outline` | on |
| `--disable-plugins` | `--enable-plugins` | off |
| `--no-images` | `--images` | on |
| `--disable-javascript` | `--enable-javascript` | on |
| `--no-background` | `--background` | on |
| `--exclude-from-outline` | `--include-in-outline` | include |
| `--disable-smart-shrinking` | `--enable-smart-shrinking` | on |
| `--no-print-media-type` | `--print-media-type` | screen |
| `--disable-forms` | `--enable-forms` | off |
| `--disable-internal-links` | `--enable-internal-links` | on |
| `--disable-external-links` | `--enable-external-links` | on |
| `--keep-relative-links` | `--resolve-relative-links` | resolve |
| `--disable-toc-back-links` | `--enable-toc-back-links` | off |
| `--no-custom-header-propagation` (buggy) | `--custom-header-propagation` | off |
| `--disable-local-file-access` | `--enable-local-file-access` | **blocked** |
| `--no-debug-javascript` | `--debug-javascript` | off |
| `--no-stop-slow-scripts` | `--stop-slow-scripts` | on |
| `--no-header-line` | `--header-line` | off |
| `--no-footer-line` | `--footer-line` | off |
| `--no-pdf-compression` | (none) | compression on |

The last occurrence wins, since each option is a plain assignment. List options append.

### 1.6 Options requiring patched Qt (`QHACK`)

From the `*` marks in `--readme` (0.12.6), matching the source:

`image-dpi image-quality no-pdf-compression use-xserver dump-default-toc-xsl dump-outline outline no-outline outline-depth default-header disable/enable-external-links disable/enable-forms disable/enable-internal-links keep-relative-links resolve-relative-links exclude-from-outline include-in-outline print-media-type no-print-media-type disable/enable-smart-shrinking disable/enable-toc-back-links viewport-size`
together with all Headers And Footer options and all TOC options.

`--default-header` and `--viewport-size` are QH only by accident: they are registered while the
qthack flag is still set from the Outline section. `--page-offset` is *not* QH, because
`addPageLoadArgs` resets the flag. That is ironic, since page numbers only exist with headers.

### 1.7 `--read-args-from-stdin`

From `main()`:

```cpp
if (parser.readArgsFromStdin) {
	char *buff;
	while ((buff = fgets_large(stdin)) != NULL) {
		int nargc=argc;
		char **nargv;
		parseString(buff,nargc,&nargv);
		for (int i=0; i < argc; ++i) nargv[i] = argv[i];
		PdfGlobal globalSettings;
		QList<PdfObject> objectSettings;
		PdfCommandLineParser parser(globalSettings, objectSettings);
		parser.parseArguments(nargc, (const char**)nargv, true);
		PdfConverter converter(globalSettings);
		ProgressFeedback feedback(globalSettings.logLevel, converter);
		foreach (const PdfObject & object, objectSettings)
			converter.addResource(object);
		if (!converter.convert())
			exit(EXIT_FAILURE);
		free(buff);
		free(nargv);
	}
	exit(EXIT_SUCCESS);
}
```

- Each stdin line is one conversion. Its argv is **the original full argv (including
  `--read-args-from-stdin` itself and anything after it), followed by the tokens of the line**.
  The whole thing is re-parsed from scratch. On the real command line, objects or output after
  the global options are ignored for the initial parse, but re-parsed per line.
- **Tokenizer** (`parseString`), a shell-like state machine:
  - Separators are space, `\t`, `\r` and `\n`.
  - `'…'` and `"…"` quote. Inside either kind of quote, `\` escapes the next character (unlike sh,
    single quotes still honour `\`).
  - Outside quotes, `\x` gives `x`.
  - Adjacent pieces concatenate (`a"b c"d` gives `ab cd`).
  - There is no variable or glob expansion. Empty quotes `""` produce no token.
- An **empty line** (or a whitespace-only line) re-parses only the original argv. That usually
  triggers the "You need to specify at least one input file…" usage error and **exits 1 for the
  whole process**.
- Lines are read with `fgets` in 1024-byte chunks with no length limit. A final line without `\n`
  is still processed.
- On the first failed conversion: `exit(1)` with **no "Exit with code" message**, and HTTP or
  network error codes are ignored. Otherwise exit 0 after EOF.
- In this mode the forms/checkbox-SVG style signals are **not connected**, so the `--checkbox-svg`
  family and form rendering style have no effect. Each line creates a new converter, but the
  `QApplication` is shared.

---

## 2. Documentation outputs (`--help`, `-H`, `--version`, …)

The byte-exact 0.12.6 outputs are in `docs/upstream-fixtures/0.12.6/cli/*.stdout`. The tests should
compare against them. This section gives the generation logic so the output can be reproduced
rather than embedded. (Embedding the 0.12.6 texts is a legitimate and simpler strategy, but the
"(default …)" values are dynamic, see 1.2.)

### 2.1 `--version` / `-V`

```cpp
fprintf(fd, "%s %s\n", appName().toLocal8Bit().constData(), appVersion());
// appVersion() = STRINGIZE(FULL_VERSION) " (with patched qt)"   (patched)
//              = STRINGIZE(FULL_VERSION)                         (unpatched)
```

Output on stdout, exit 0: `wkhtmltopdf 0.12.6 (with patched qt)\n`.
`FULL_VERSION` is `$WKHTMLTOX_VERSION` or the `VERSION` file (for example `0.12.7-dev`). The PDF
`/Creator` uses `"wkhtmltopdf " FULL_VERSION`, **without** the "(with patched qt)" suffix.

### 2.2 Which parts each action prints

| Action | Outputter | Parts in order |
|---|---|---|
| `--help` / `-h` (`usage(stdout,false)`) | text (doc=false) | Name, Synopsis (+ Document objects), Description, Switches (non-extended), [unpatched: Reduced Functionality(sure)], Contact |
| `-H` / `--extended-help` (`usage(stdout,true)`) | text (doc=false) | Name, Synopsis, Description, Switches (all), [unpatched: Reduced Functionality], Page sizes, Reading arguments from stdin, Specifying A Proxy, Footers And Headers, Outlines, Table Of Contents, Contact |
| usage on error (`usage(stderr,false)`) | text to **stderr**, but see 2.4 | same as `--help` |
| `--license` | text (doc=false) | Name, Authors, License |
| `--manpage` | man | `.TH` header, Name ("wkhtmltopdf - html to pdf converter"), Synopsis, Description, Switches (all), Specifying A Proxy, [patched: Footers And Headers, Outlines, Table Of Contents \| unpatched: Reduced Functionality], Page sizes, Reading arguments from stdin, Page Breaking, Contact, Authors |
| `--readme` | text (doc=true) | `wkhtmltopdf <FULL_VERSION> Manual`, Contact, Reduced Functionality(sure=false), License, Authors, Synopsis, Switches (all, doc), Specifying A Proxy, Footers And Headers, Outlines, Table Of Contents, Page Breaking, Page sizes, Reading arguments from stdin, Static version, Installation, Examples |
| `--htmldoc` | html | same parts as `--readme` |

All the actions exit 0 after writing to stdout. The text of every section is in
`src/pdf/pdfdocparts.cc` and `src/shared/commondocparts.cc` (0.12.6 text: see fixtures).
`License` prints `Copyright (c) 2010-2020 wkhtmltopdf authors` and then the embedded `LICENSE` file
verbatim. `Authors` prints the embedded `AUTHORS` file verbatim. Both are Qt resources from
`wkhtmltopdf.qrc`. `AUTHORS` starts with a UTF-8 BOM followed by `\n`, which renders as a first line
of `"  "`.

### 2.3 Switch listing (`outputSwitches`)

- **Section order** is registration order: `Global Options`, `Outline Options`, `Page Options`,
  `Headers And Footer Options`, `TOC Options`. A section left empty after filtering is skipped
  entirely.
- **Filtering:** drop `extended` options unless extended. Under unpatched Qt and `doc==false`, also
  drop QH options.
- **Sort** (`ahsort`, `qSort`): strip the regex `^(no|enable|disable|include-in|exclude-from)-` from
  both names and compare with `QString <` (UTF-16 code units). On a tie, compare the full names
  after replacing `^no-` with `zzzz`. So `--collate` sorts before `--no-collate`,
  `--disable-x` before `--enable-x`, and `--exclude-from-outline` before `--include-in-outline`.
- **Description with default** (`getDesc`):
  - `ConstSetter`: append `" (default)"` when the constant equals the *current* value of the field.
  - `SomeSetter`: append `" (default " + TToStr(current) + ")"` when it can be printed:
    - int and float: only when the value is not `-1`. Float uses `QString::number`, so `0.8`, `1`
      and `0`.
    - string: only when non-empty.
    - unitreal: not when `-1`.
    - page size, orientation and load-error-handling: always.
    - log level: always.
  - `Caller`, `MapSetter` and `StringListSetter`: no default.
  - The man outputter uses the raw `desc` (no defaults). The text and html outputters use
    `getDesc()`.

### 2.4 Text outputter (exact algorithm, including bugs)

`--help`, `-H`, `--license` and the error usage use `TextOutputter(doc=false)`. `--readme` uses
`TextOutputter(doc=true)`. Strings are converted with `toLocal8Bit()` (locale-dependent, so
non-ASCII characters are lost in the C locale: `Pablo Ruiz Garca`) when `doc==false`, and with
UTF-8 when `doc==true`.

```cpp
lw = 80;
beginSection(name):  doc ? (x = 80 - len(name) - 4; if (x < 6) x = 60;
                            '=' * (x/2) + "> " + name + " <" + '=' * ((x+1)/2) + "\n")
                         : name + ":\n"
beginParagraph():    first=true; doc ? w=0 : (w=2, print "  ")
text(t):  first=true;                         // NB: reset on every text() call
          for s in t.split(" "):               // empty strings preserved
              if (w + len(s) + (first?0:1) > 80) { print "\n"; doc? w=0 : (w=2, print "  "); first=true; }
              if (first) first=false; else { print " "; ++w; }
              w += len(s); print s
sectionLink(t) = text(t);  bold(t) = text("*"+t+"*");  italic(t) = text("_"+t+"_");  link(t) = text("<"+t+">")
endParagraph(): print "\n\n"
paragraph(t) = beginParagraph(); text(t); endParagraph()
verbatim(t):  doc ? print t + "\n"
                  : for line in t.split("\n"): print "  " + line + "\n"   // trailing "\n" ⇒ extra "  \n" line
beginList(ordered): order = ordered?1:-1;   endList(): print "\n"
listItem(s): (order<0 ? " * " : printf("%3d ", order++)) + s + "\n"
endSection(): nothing;  beginSwitch(): nothing
cswitch(h):
   w=0; if (!doc) { print "  "; w=2; }
   print (h->shortSwitch ? "-%c, " : "    ");  print "--" + longName;  w += 4 + 2 + len(longName);
   if (doc && h->qthack) { print " *"; w += 2; }
   for arg in argn: { print " <" + arg + ">"; w += 3 + len(arg); }
   while (w < 37) { print " "; ++w; }
   for s in getDesc().split(" "):
       if (w + 1 + len(s) > 80) { printf("\n") /* <-- STDOUT, not fd! */; w=0; while (w<37) { print " "; ++w; } }
       print " " + s; w += len(s) + 1;
   print "\n"
endSwitch(): if (doc) print "\nItems marked * are only available using patched QT.\n";
             printf("\n")   /* <-- STDOUT, not fd! */
```

**Critical quirk for byte-exactness.** The description wrap newline in `cswitch` and the blank line
in `endSwitch` are written with `printf`, which always goes to **stdout**, even when the usage
goes to stderr.

When a parse error prints usage to stderr:
- stderr gets the switch descriptions *without* their wrap newlines. Each switch is one long line
  with the continuation indentation glued on, and there is no blank line between switch sections.
- stdout receives one `\n` per wrap and per section, **15 newline bytes** for the default `--help`
  set in 0.12.6. See `cli/noargs.stdout` (15 bytes) and `cli/noargs.stderr`.
- If the caller merges stdout into stderr, the newlines appear at the end, after stdio buffering
  flushes.

This matters to pdfkit: a parse error yields a non-empty stdout of newlines (the result is still
"blank" after `strip`, and the exit status is 1).

Other visible artefacts:
- `"Page Options and "` with a trailing space before the wrap: `text(" and ")` splits into
  `["", "and", ""]`.
- A blank `"  "` line after verbatim blocks.
- A section-start line has no blank line before it, except where the previous paragraph's `\n\n`
  provided one.

### 2.5 Man outputter

- Header: `.TH WKHTMLTOPDF 1 "2009 February 23"\n\n`.
- `beginSection`: `.SH <name>\n`. `endSection`: `\n`. `beginParagraph`: nothing.
  `endParagraph`: `\n\n`.
- `text()` replaces every `-` with `\-`. `bold()` and `italic()` both give `\fB…\fP` without
  escaping. `link()` gives `<url>`.
- `verbatim`: escape `-`, drop trailing empty lines, print `"  "+line+"\n"` for each line, then
  `"\n"`.
- `beginSwitch`: `.PD 0\n`. `endSwitch`: `.PD\n\n`.
- `cswitch`:
  ```
  .TP
  \fB
  (short ? "\-%c, " : "    ")
  "\-\-<longName>\fR"            (longName itself NOT escaped)
  (" \fI<arg>\fR" for each arg)
  "\n" + desc.replace("-", "\-") + "\n"
  ```

### 2.6 HTML outputter

- The header is a fixed block. **The DOCTYPE is malformed** by C string concatenation:
  `<!DOCTYPE html PUBLIC "-//W3C//DTD XHTML 1.0 Transitional//ENhttp://www.w3.org/TR/xhtml1/DTD/xhtml1-transitional.dtd">`.
- The title is `wkhtmltopdf - Manual`, followed by the CSS block. See `cli/htmldoc.stdout`.
- The output ends with `</body></html>\n`.
- `beginSection`: `<a name="%s"><h1>%s</h1></a>\n`. Paragraphs are `<p>…</p>\n`. `verbatim` is
  `<pre>%s</pre>` (no newline). Lists use `<ul>`/`<ol>`, with `<li>%s</li>\n`.
- `sectionLink` gives `<a href="#%s">%s</a>`.
- Switches are `<table>\n`, one row per switch:
  `<tr><td class="short">-x,</td><td class="long">--name[<span …red>*</span>]</td><td class="arg">&lt;arg&gt; …</td><td class="desc">desc-with-default</td></tr>\n`.
  The table ends with `</table>\n<p>Items marked <span style="font-weight: normal; font-size: 80%; color:red;">*</span> are only available using patched QT.</p>`.
- Everything is HTML-escaped (Qt `escape`) and UTF-8.

### 2.7 `--dump-default-toc-xsl`

This prints `dumpDefaultTOCStyleSheet` with default `TableOfContent` settings to stdout (text mode).
Exact output: `cli/tocxsl.stdout`. The generator is in `src/lib/tocstylesheet.cc`. The parametrised
parts are:
- `<title>`/`<h1>` use `captionText`.
- The `div {border-bottom: 1px dashed rgb(200,200,200);}` line appears only if `useDottedLines`.
- `ul ul {font-size: <fontScale*100>%; }`, where the number is formatted by `QTextStream`: `80`
  for 0.8.
- `ul ul {padding-left: <indentation>;}`.
- The `<xsl:if test="@link">…href…</xsl:if>` block appears only if `forwardLinks`.

Note `<xsl:value-of select="@title" /> ` has a trailing space.

---

## 3. Conversion pipeline (patched Qt)

### 3.1 Phases and progress (`pdfconverter.cc`)

Phase descriptions in patched mode, with indices 0..6:

```
0 "Loading pages"   1 "Counting pages"   2 "Loading TOC"   3 "Resolving links"
4 "Loading headers and footers"   5 "Printing pages"   6 "Done"
```

In unpatched mode: `0 "Loading page"  1 "Printing pages"  2 "Done"`.

1. **beginConvert.**
   - For each object, validate `--header-html`/`--footer-html`. If `looksLikeHtmlAndNotAUrl`
     (the value before `?` contains `<`, or the value starts with `data:`, case-insensitive),
     emit the error `--header-html should be a URL and not a string containing HTML code.` (or
     `--footer-html …`) and fail.
   - If a header/footer HTML is set and the corresponding global margin is unset (-1), schedule a
     *measuring* load of that HTML. Otherwise `reserveHeight = margin + spacing`.
   - Non-TOC objects are added to the page loader. Then `phaseChanged`, `loadProgress(0)`.
   - Without measuring, unset top/bottom margins become **10** (in the margin's unit) and
     `reserveHeight = margin`. Then load all pages in parallel.
2. **Measuring** (only if needed): render the header/footer once, measure the `<body>` height in
   mm, and set `reserveHeight = height_mm + spacing`. Then load the pages.
3. **pagesLoaded.**
   - Output target: `-` becomes `/dev/stdout` if it exists, else a temp file copied to stdout at the
     end. An empty `out` (C API only) gives an in-memory buffer.
   - Margin unit check: **all four margin units must be identical** (cm/mm/m all count as
     Millimeter), else error `Currently all margin units must be the same!` and fail. Note that
     the left/right defaults are `10mm`, so `--margin-top 1in` alone fails.
   - Page margins = (left, max over objects of headerReserve, right, max footerReserve), in the
     **left margin's unit**. Measured heights are mm values interpreted in that unit (bug when
     the unit is not mm). Header/footer spacing (mm) is added as a raw number too.
   - Paper: `QSizeF(width,height)` in the height's unit if both `--page-width` and `--page-height`
     are set, else `--page-size`. Orientation, colour mode,
     `creator = "wkhtmltopdf " FULL_VERSION`, compression, image quality and image DPI are set.
   - If the printer is invalid or `painter.begin` fails: error `Unable to write to destination`
     and fail. Qt also prints `QPainter::begin(): Returned false` on stderr.
   - Title: `--title`, else the `<title>` of the first object that is not skipped and not a TOC.
   - Phase 1: for each object, a `QWebPrinter` counts pages. The viewport size is applied here.
     The heading outline is built. Progress string `Object i of n`, percent `i*100/n`.
     TOC objects count 1 page provisionally. `actualPages = pageCount * copies`.
4. **loadTocs** (phase 2, only announced when there is a TOC):
   - For each TOC object: write the XSL (custom `--xsl-style-sheet` or default) and the outline
     XML (2.7 and 4.3) to temp files `wktemp-<uuid>.xsl/.xml/.html` in `QDir::tempPath()`.
   - Run XSLT 2.0 (`QXmlQuery`), load the resulting HTML, and recount its pages.
   - **Repeat until neither the page counts nor the outline change** (`tocChanged`).
   - Errors: `Could not read the TOC XSL`, `Could not read the TOC XML`,
     `Could not open the TOC for writing`. These continue rather than return; that is a bug.
5. **Resolving links** (phase 3): map each loaded page URL (fragment removed, percent-decoded) to
   its object. Collect internal and external links for each object. Progress `Object i of n`.
6. **loadHeaders** (phase 4, always announced): for every page of every non-skipped object with
   header/footer HTML, load a separate copy of that HTML with the query parameters (3.4). If there
   are none, print directly.
7. **printDocument** (phase 5):
   - Progress `Preparing` (0%), then for each page `Page <actualPage> of <actualPages>`.
   - Copies loop: with collate, the whole document is repeated `copies` times. Without collate,
     each page is emitted `copies` times consecutively.
   - Page numbers restart at 1 for each collated copy.
   - Then the PDF outline is written, `--dump-outline` is written, and the painter ends.
   - For stdout through a temp file, the file is copied and removed. On error:
     `Count not write to stdout` (sic).
   - Phase 6 `Done`, `finished(true)`.
8. **Unpatched:**
   - More than one object gives the error
     `This version of wkhtmltopdf is built against an unpatched version of QT, and does not support more than one input document.`
   - Otherwise the page is loaded and printed with `QWebFrame::print` (copies/collate via QPrinter).
     There are no headers, TOC, outline or links. Margins are passed raw (unset top/bottom stay -1).

Any `fail()` stops the conversion, `finished(false)`. Temp files are removed.

### 3.2 Loading (`multipageloader.cc`)

**Input resolution** (`MultiPageLoader::addResource` and `guessUrlFromString`):

- Input `-`: stdin is read fully into the temp file `QDir::tempPath()/wktemp-<uuid>.html` and
  loaded as `file://`. Relative URLs in stdin HTML therefore resolve against the temp directory,
  which is **blocked by default** (see local file access below).
- `guessUrlFromString(s)`, in this order:
  1. Trim. If it matches `^[a-zA-Z\.]+\:[0-9]*$` (host:port), prefix `http://`.
  2. If it matches `^[a-zA-Z]+\://.*` and parses as a valid URL (tolerant mode; ASCII-only strings
     go through `QUrl::fromEncoded`), use it.
  3. If a local file with that name exists, use `file://` plus the absolute path.
  4. If there is no scheme and it contains `.`: if the prefix before the first dot is `ftp`, use
     `ftp://s`, else `http://s`. **So a missing local file `nonexist.html` becomes
     `http://nonexist.html`**, and the error says `Host nonexist.html not found`.
  5. Otherwise use tolerant `QUrl(s)`, and if the scheme is empty, `http://s`.
- The same function is used for `--header-html`, `--footer-html` and `--user-style-sheet`.

**Request construction** (`ResourceObject::load`):

- **Custom headers** (`--custom-header`) are set on the main request with `setRawHeader(latin1)`.
  They are added to every sub-request only when `--custom-header-propagation` is set (or the buggy
  `--no-…` form).
- **Cookies** (`--cookie name value`): an extra cookie list is added to **every** request URL
  (`cookiesForUrl` appends them regardless of domain). The value is used raw, so the caller must
  URL-encode it.
- **Cookie jar** (`--cookie-jar path`):
  - Loaded at loader construction with `QNetworkCookie::parseCookies(file)`.
  - Saved at the end of **every** loader (pages, TOC, headers) as `cookie.toRawForm() + ";\n"` per
    cookie.
  - Each loader (main, measuring, header/footer, two TOC loaders) has its own jar instance.
- **POST** (`--post`, `--post-file`):
  - Without files: an `application/x-www-form-urlencoded` body built with QUrlQuery or QUrl
    encoding.
  - With any file: a multipart body using **`\n` line endings (not CRLF)**, part headers
    `content-disposition: form-data; name="…"` plus `; filename="<basename>"` for files, the
    boundary being a UUID without dashes or braces, and the header
    `Content-Type: multipart/form-data, boundary=<b>` (**comma, not semicolon**).
  - The method becomes POST only if the body is non-empty.
  - An unreadable file gives the error `Unable to open file <path>`, and the loader fails.
- **HTTP auth** (`--username`, `--password`): answered on the `authenticationRequired` challenge,
  up to 2 attempts. If there is no username, the error is `Authentication Required`. After 2 tries
  the error is `Invalid username or password`.
- **Proxy** (`--proxy`):
  - Set only if the host is non-empty.
  - HTTP proxies get the Caching and Tunneling capabilities, plus HostNameLookup with
    `--proxy-hostname-lookup`.
  - `--bypass-proxy-for host` uses no proxy for exact, case-insensitive host matches.
  - Note: the docs say proxy env vars (`proxy`, `all_proxy`, `http_proxy`) are read, but the code
    that did it is commented out. Whatever Qt does by default applies (believed to be no proxy).
- **SSL**:
  - All SSL errors are ignored, with the warning `SSL error ignored` per reply.
  - A client certificate is used only if all three of `--ssl-key-path`, `--ssl-key-password` and
    `--ssl-crt-path` are set. The key is RSA PEM. The first cert of the crt file is the local
    cert; the rest are added to the CAs.
- **Cache**: `--cache-dir` gives a `QNetworkDiskCache`.

**Local file access** (`createRequest`):

- A request is "local" when its scheme length is ≤ 1 (Windows drive letters) or the scheme is
  `file`.
- If `blockLocalFileAccess` is set (**the default since 0.12.6**), a local request is allowed only
  if its canonical path, or one of its ancestors, is in the allowed set: the canonical paths of
  `--allow` entries plus the page's own file.
- Otherwise the warning `Blocked access to file <canonicalPath>` is emitted. The canonical path is
  empty for missing files, which gives `"Blocked access to file "` with a trailing space.
- The request is then **rewritten to `about:blank`**. That request fails with network error 301
  (`ProtocolUnknownError`, message `Protocol "about" is unknown`). The URL has no extension, so it
  is treated as a non-media failure: an `Error:` line is printed and the **network error code is
  recorded, so the process exits 1** (verified with 0.12.6, `conversion/warn.*`).
- Effect for pdfkit: any `<img src="relative.png">`, `<link href="x.css">` or `file://` reference
  in stdin HTML makes wkhtmltopdf exit 1, even though a PDF is produced.

**Page load completion** (`loadFinished`):

```cpp
hasError ||= (!ok && loadErrorHandling == abort);
if (!ok) abort  → error  "Failed loading page <url> (sometimes it will work just to ignore this error with --load-error-handling ignore)"
         skip   → warning "Failed loading page <url> (skipped)"   and the object is dropped from the output
         ignore → warning "Failed loading page <url> (ignored)"
if (isMainLoader) evaluate every --run-script string in the main frame (in order)
if (!ok || window.print() was called || jsdelay == 0) done now
else if (isMainLoader && windowStatus != "") poll window.status every 50 ms
          (debug message every 20 polls: Waiting for window.status; Found: "<x>", but expecting: "<y>".)
          when equal: debug `Window status "<y>" found.` then wait jsdelay ms → done
else wait jsdelay ms → done
```

- `<url>` here is the page URL `toString()` as given, for example `http://nonexist.html` with no
  trailing slash.
- `--run-script` and `--window-status` apply only to the main pages loader, not to the TOC,
  header/footer or measuring loaders. `--javascript-delay` (default 200 ms) applies to every
  loader.
- A `window.print()` call in the page ends the wait immediately.
- On done: stop loading and stop scheduled refreshes. Later requests from that page are rewritten
  to `about:blank`, with the warning
  `Received createRequest signal on a disposed ResourceObject's NetworkAccessManager. This might be an indication of an iframe taking too long to load.`.
- When all resources are done, the cookie jar is saved and `loadFinished(!hasError)` is emitted.
  Abort failures stop the conversion. Skip and ignore continue.

**Per-reply error accounting** (`amfinished`): this decides the exit code.

```cpp
networkStatus = reply->error();  httpStatus = HttpStatusCodeAttribute;
if ((networkStatus != 0 && networkStatus != 5 /*OperationCanceled*/) || (httpStatus > 399 && httpErrorCode == 0)) {
    extension = QFileInfo(url.toString()).completeSuffix().toLower().remove(QRegExp("\\?.*$"));
    mediaFile = extension in {css, js, svg, png, jpg, jpeg, gif};
    if (!mediaFile) {
        error("Failed to load %1, with network status code %2 and http status code %3 - %4"
              .arg(url).arg(networkStatus).arg(httpStatus).arg(reply->errorString()));
        httpErrorCode = networkStatus > 0 ? networkStatus + 1000 : httpStatus;
        return;
    }
    if (mediaLoadErrorHandling == abort) {
        httpErrorCode = networkStatus > 0 ? networkStatus + 1000 : httpStatus;
        error("Failed to load " + url + ", with code: " + code +
              " (sometimes it will work just to ignore this error with --load-media-error-handling ignore)");
    } else
        warning("Failed to load %1 (%2)".arg(url).arg("skip"|"ignore"));
}
```

Consequences:

- **Any non-media sub-resource failure records an error code, whatever `--load-error-handling`
  says.** This covers fonts (`.woff`), favicons (`.ico`), iframes, XHR, extensionless URLs and
  `about:blank` from blocked files. The PDF is still produced, but the exit code is non-zero.
- `completeSuffix` uses everything after the **first** dot of the last path segment:
  - `jquery.min.js` gives `min.js`, which is **not media**.
  - The whole URL string is passed to `QFileInfo`, so a dot in a directory name does not matter,
    but a query string is stripped only after the suffix is taken.
- A media file with `--load-media-error-handling abort` sets the code but does not fail the load
  by itself.
- `--load-media-error-handling skip` behaves exactly like `ignore`; only the word in the warning
  differs.
- An HTTP status is recorded only for the first failure per resource; network errors overwrite it.
- `MultiPageLoader::httpErrorCode()` returns the **maximum** over resources, so network errors
  (≥1000) dominate.
- The converter keeps the first non-zero code across phases: main pages, then measuring, TOC and
  header/footer loaders.

**JavaScript hooks** (`MyQWebPage`):

| Event | 0.12.7-dev source | 0.12.6 binary (verified) |
|---|---|---|
| `alert(msg)` | info `Javascript alert: <msg>` | **Warning:** `Javascript alert: <msg>` |
| `confirm(msg)` | info `Javascript confirm: <msg> (answered yes)`, returns true | warning (same text) |
| `prompt(msg, def)` | info `Javascript prompt: <msg> (answered <def>)`, returns true with **an empty result** (bug: `result` not assigned) | warning |
| `console.log` etc., only with `--debug-javascript` | info `<sourceURL>:<line> <message>` | **Warning:** `<sourceURL>:<line> <message>` |
| Slow script, with `--stop-slow-scripts` (default) | warning `A slow script was stopped`, interrupt | same |
| Slow script, with `--no-stop-slow-scripts` | debug `A slow script has been detected, but was not stopped` | (no debug level) |

Media type: `--print-media-type` makes WebKit use `print` CSS media, else `screen` (patched
`overrideMediaType`).

**Debug-level messages** (source only; 0.12.6 has no debug level):
`Creating request: <url>`, `Finished request: <url>`, `QWebPage load started.`,
`QWebPage load finished.`, `Loading done; Stopping QWebPage and any possible page refreshes.`

**Other warnings:**
`A finished ResourceObject received a loading progress signal. This might be an indication of an iframe taking too long to load.`
and the same with `loading finished signal`.

### 3.3 Layout

- **DPI and zoom.** WebKit renders at 96 dpi. For each resource:
  `zoomFactor = (dpi / 96.0) * --zoom` and `devicePixelRatio = dpi/96` (for CSS media queries).
  The default `dpi=96` gives zoom 1. The printer resolution is `--dpi`.
- **Printer mode.** `--lowquality` builds `QPrinter(ScreenResolution)` instead of
  `QPrinter(HighResolution)`. `setResolution(dpi)` is called afterwards, so the effective
  difference comes from Qt's PDF engine (lower image fidelity). Compare with fixtures before
  investing in it.
- **Smart shrinking** (default on) uses WebKit's printing shrink factors, which scale content wider
  than the page down to fit. `--disable-smart-shrinking` forces
  `setPrintingMinimumShrinkFactor(1.0)` and `setPrintingMaximumShrinkFactor(1.0)`. The upstream
  WebKit defaults are believed to be 1.25 and 2.0; check the patched Qt (submodule `qt`, branch
  `wk_4.8.7`, not checked out here).
- **Viewport.** `--viewport-size` takes `WxH`: split on `x`, `toInt` the first and last parts.
  It is applied only if the result is valid and non-empty. It is applied **after loading**, at page
  counting time, with both scrollbars forced off. The headless screen in patched Qt 0.12.4+ is
  1366x768, overridable through environment variables in the patched Qt.
- **Web settings** (`updateWebSettings`, applied to page, TOC and header/footer pages):
  - Default text encoding (`--encoding`) if non-empty.
  - `JavaEnabled` and `PluginsEnabled` = `--enable-plugins`. `JavascriptEnabled` as set.
    `JavascriptCanOpenWindows` and `JavascriptCanAccessClipboard` = false.
  - `MinimumFontSize = --minimum-font-size` (-1 by default).
  - `PrintElementBackgrounds = --background`. `AutoLoadImages = --images`.
  - User stylesheet URL via `guessUrlFromString`.
- Page, header and footer palettes get a **transparent Base brush**, so there is no white page
  background unless the CSS sets one.
- **Page size.** Named sizes come from `QPrinter::PageSize`, for example A4 = 595×842 pt and
  Letter = 612×792 pt. Custom `--page-width`/`--page-height` need both (else ignored) and use the
  **height's** unit. Verified: 100mm×50mm gives 284×142 pt. Landscape swaps the dimensions
  (Letter landscape = 792×612). The measuring printer adds 100 to the height (internal only).
- **Margins.** Defaults are top=unset→10, right=10mm, bottom=unset→10, left=10mm. See 3.1(3) for
  the unit rules and header reservations.
- **Page breaking** is WebKit's: render one long page and cut it. CSS `page-break-*` is honoured by
  the patched Qt.

### 3.4 Headers and footers

**Variables** (`fillParms` and `Outline::fillHeaderFooterParms`), for page number `p` (1-based
across the whole document within one copy, counting only objects with `pagesCount`, which is always
true from the CLI):

| Key | Value |
|---|---|
| (each `--replace name value`) | value. Inserted first, so built-ins override same-named replacements |
| `frompage` | `pageOffset + 1` |
| `topage` | `pageOffset + outline.pageCount`. Each document's real page count, but objects with `includeInOutline=false` (covers, `--exclude-from-outline`) are counted as **1 page** |
| `page` | `p + pageOffset` |
| `webpage` | the object's `page` setting **as typed** (for example `a.html`, `-`, a URL); empty for TOC |
| `section` | text of the most recent level-1 outline item (top-level heading of any document) on or before this page; "" if none |
| `subsection` | the same for level 2 |
| `subsubsection` | the same for level 3 (not documented but present) |
| `doctitle` | the PDF title (3.1, step 3) |
| `title` | this object's `<title>` |
| `time` | `QTime::currentTime().toString(Qt::SystemLocaleShortDate)` (C locale: `13:59:59`) |
| `date` | `QDate::currentDate().toString(Qt::SystemLocaleShortDate)` (C locale: `14 Oct 2026`) |
| `isodate` | `yyyy-MM-dd` |
| `sitepage` | page index within this object, 1-based |
| `sitepages` | page count of this object |

Default `pageOffset` is 0, so the first page is 1. Section caching (`hfCache`): for each level,
walk the outline items in document order. An item on page `j->page + prefixSum[doc]` is recorded
only if no item was recorded for that page yet (the first heading on a page wins). Values carry
forward to later pages.

**Text headers and footers** (`--header-left`, `--header-center`, `--header-right`, and the footer
equivalents):

- Substitution (`hfreplace`): for each key in QHash order, replace `"[" + key + "]"`
  case-insensitively (`[PAGE]` works). Unknown `[foo]` stays literal unless `--replace foo …`.
  The iteration order is unspecified, so a value containing `[x]` may or may not be substituted
  again.
- They are drawn only if at least one of the 6 texts is non-empty or a line is enabled.
- Drawing in printer coordinates (content-area origin, `w` × `h` = printable area,
  `spacing_px = spacing_mm * h / heightMM`):
  - Header line at `y = -spacing`. Header font `QFont(fontName, fontSize)`.
    `dy = boundingRect("M").height`.
  - Header texts drawn in `QRect(0, -dy-spacing, w, h)`, aligned Top+Left, Top+HCenter or
    Top+Right.
  - Footer line at `y = h + spacing`. Footer texts drawn in `QRect(0, 0, w, h+dy+spacing)`,
    aligned Bottom+Left, Bottom+HCenter or Bottom+Right.
- Text headers live inside the top margin and take **no extra space**. If the margin is too small
  they are clipped or run off the page.
- Covers have them cleared. TOC objects get them like pages.
- `--default-header` = `--header-left "[webpage]" --header-right "[page]/[topage]" --header-line`
  plus global `--margin-top 2cm`.

**HTML headers and footers** (`--header-html url`, `--footer-html url`):

- One separate load **per output page** of the URL (via `guessUrlFromString`) with every variable
  above appended as a query item (`QUrl::addQueryItem`, percent-encoded, space → `%20`). The order
  is QHash order, i.e. unspecified.

  Verified example:
  ```
  hdr2.html?foo=bar&page=11&section=Chapter%20One&sitepage=1&title=Doc%20A&subsection=Sub%201.1&frompage=11&subsubsection=&isodate=2026-10-14&topage=12&doctitle=Doc%20A&sitepages=1&webpage=a.html&time=13:59:59&date=14%20Oct%202026
  ```
- `--replace` pairs and `subsubsection` are included. Values are encoded by Qt4's
  `addQueryItem`: `&`, `=`, `+`, `#` and `%` are encoded; `:` and `/` are not.
- Loaded with the object's load settings, so local file access rules apply. The page's own file is
  allowed, and `file://…?query` is allowed via the canonical path of `toLocalFile`.
- Rendering:
  - The header is rendered with zero vertical margins. Its body bottom is placed at
    `-spacing_px`: translate `-spacing`, then `-bodyHeight`. Only page 1 of the header document is
    printed.
  - The footer top is at `h + spacing_px`.
  - Links inside headers and footers are honoured.
- Space reservation:
  - If `--margin-top` (or `--margin-bottom` for footers) was given: reserve `margin + spacing`.
  - Otherwise reserve the **measured body height (mm) + spacing**. The measuring load has no query
    string.
  - The margin used is the maximum over all objects.
- A tall header can leave no room for content. With 0.12.6, a header taller than the page produced
  a PDF with **zero pages** and exit 0 (observed).
- Header/footer HTML that "looks like HTML" (contains `<`, or is a `data:` URI) is rejected with an
  error (3.1, step 1).
- Text and HTML headers can coexist; the text is drawn first.

### 3.5 Outline (bookmarks), TOC, `--dump-outline`

- Headings are collected per object from `h1..h9` (elements `h7`–`h9` are included literally).
  - They are ordered by (page, y, x) of their rendered location, using a `QMap`, so identical keys
    collapse.
  - Headings with empty trimmed text (newlines become spaces) or no location (page -1) are skipped.
- Tree: a level stack. A heading pops the stack while `top >= level`, then becomes a child, so an
  `h3` under an `h1` is a direct child.
- Each document root has `value` = the object's `<title>`, or the TOC caption for TOC objects, and
  page 0.
  - Objects excluded from the outline get an empty root counted as 1 page.
  - Skipped objects are not added at all.
- **Anchors:** every item, including roots, gets `anchor = "__WKANCHOR_" + base36(counter++)` and
  `tocAnchor = "__WKANCHOR_" + base36(counter++)`.
  - The counter is global across documents, assigned depth-first. TOC rebuilds reuse the anchors
    when the structure is unchanged.
  - Bug in the reuse branch: `tocAnchor = other->anchor`, so when anchors are reused,
    `backLink == link`. That is visible in the TOC root of `conversion/outline.xml`.
  - `forwardLinks` (TOC → heading) registers the heading as the named destination `anchor`.
    `backLinks` (`--enable-toc-back-links`) links the heading to `tocAnchor`.
- **PDF outline** (`--outline`, default on):
  - For each document root, its children are emitted recursively as `beginSectionOutline(title,
    anchor)` while `level+1 <= outlineDepth`. With the default depth 4, there are 4 levels below
    the document roots; the roots themselves are not bookmarks.
  - The catalog gets `/PageMode /UseOutlines` (observed).
  - `--no-outline` writes no outline. The TOC and `--dump-outline` still work.
- **`--dump-outline <file>`** (written at the end of printing, UTF-8):

  ```
  <?xml version="1.0" encoding="UTF-8"?>
  <outline xmlns="http://wkhtmltopdf.org/outline">
    <item title="T" page="P" link="__WKANCHOR_x" backLink="__WKANCHOR_y">      (2 spaces per level, roots at level 1)
      <item .../>                                                                (leaf: "/>")
    </item>
  </outline>
  ```

  - `page = item.page + prefixSum[doc] + pageOffset`. `item.page` is 1-based for headings and 0
    for roots, so the root of the first document shows `page="0"` with offset 0.
  - Attribute escaping: `& < > " '` become `&amp; &lt; &gt; &quot; &apos;`.
  - The TOC document itself is a root, and its own `<h1>` is a child (so the default TOC lists
    "Table of Contents" as its first entry). See `conversion/outline.xml`.
- **TOC page:**
  - The same XML (with the current numbering) is transformed by the XSL into HTML and loaded as a
    page.
  - The default XSL renders `outline:item/outline:item` (children of the document roots). For each
    item: a `<li>` containing `<div><a href="@link" name="@backLink">title </a><span> page </span></div>`
    when the title is non-empty, followed by a nested `<ul>`.
  - TOC options other than `--xsl-style-sheet` only affect the default XSL.
  - A custom XSL must be **XSLT 2.0** (QtXmlPatterns). A Rust port needs an XSLT 2.0-capable
    engine, or must at least support the default sheet natively.

### 3.6 Links, forms, misc. PDF features

- **External links:**
  - Every `<a href>` (or `ns0:href`) whose resolved URL (without fragment) does not match a loaded
    page becomes a URI link annotation over the element's box.
  - The target is the resolved absolute URL, or the raw `href` with `--keep-relative-links`.
  - Disabled with `--disable-external-links`.
- **Internal links:**
  - The target must match a loaded page URL (any object).
  - With no fragment the link points at that page's `<body>`. Otherwise it points at the first
    match of `a[name="f"]`, then `*[id="f"]`, then `*[name="f"]`.
  - The destination is named with the full resolved URL (for example `file:///…/a.html#c2`; the
    PDF shows `/Dests` entries like `/file#3a#2f#2f…`).
  - Elements whose name or href starts with `__WKANCHOR_` are TOC/outline anchors.
  - Disabled with `--disable-internal-links`.
  - If a page has both disabled, link scanning is skipped.
- **Forms** (`--enable-forms`):
  - `input` and `textarea` elements are recoloured white (so the rendered text is hidden).
  - `textarea`, `input[type=text]` and `input[type=password]` become text fields (value, name,
    multiline for textarea, password flag, readOnly, `maxlength`).
  - `input[type=checkbox]` becomes a checkbox field (checked, name, readOnly).
  - Radio buttons and selects are not converted.
  - Checkbox and radio appearance: built-in SVGs, or the `--checkbox-svg` family. An invalid file
    gives the Qt warning `Failed to load "<path>"` and falls back to the built-in SVG (source:
    `utilities.cc`, `loadSvg`).
- **Copies and collate:** see 3.1, step 7. Page progress counts `pages × copies`.
- **Grayscale:** `QPrinter::GrayScale` colour mode.
- **Compression:** `PPK_UseCompression`. **Image quality:** JPEG quality (`PPK_ImageQuality`,
  default 94). **Image DPI:** downscaling cap (`PPK_ImageDPI`, default 600).
- **Metadata** (observed 0.12.6):
  - PDF 1.4.
  - `/Title` = doc title, `/Creator` = `wkhtmltopdf 0.12.6`, `/Producer` = `Qt 4.8.7`,
    `/CreationDate (D:YYYYMMDDhhmmssZ)` in UTC.
  - Strings are UTF-16BE with BOM.
  - Fonts are embedded subsets (`QABAAA+DejaVuSans…`).
- **Temp files:** `QDir::tempPath()` (`$TMPDIR` or `/tmp`), named `wktemp-<uuid-36>.<ext>`. They are
  removed at the end.
- **Locale:** `setlocale(LC_ALL, "")` at start. This affects the `[date]` and `[time]` formats and
  the `toLocal8Bit` conversions of all CLI strings and messages.

---

## 4. Stderr output, log levels, exit codes

### 4.1 `ProgressFeedback` (exact)

`lw` is the width of the last progress line drawn (0 after any `\n`).

```cpp
debug(m):    if (logLevel < Debug) return;  fprintf(stderr, "Debug: %s", m);   finishLine(7 + m.size());
info(m):     if (logLevel < Info)  return;  fprintf(stderr, "Info: %s", m);    finishLine(6 + m.size());
warning(m):  if (logLevel < Warn)  return;  fprintf(stderr, "Warning: %s", m); finishLine(9 + m.size());
error(m):    if (logLevel < Error) return;  fprintf(stderr, "Error: %s", m);   finishLine(7 + m.size());
finishLine(start): pad with ' ' from start to lw; "\n"; lw = 0; fflush(stderr)

phaseChanged(): if (logLevel < Info) return;
    print desc; l = desc.size();
    if (currentPhase < phaseCount-1) l += fprintf(stderr, " (%d/%d)", currentPhase+1, phaseCount-1);
    pad with ' ' up to lw; "\n"; lw = 0; fflush

progressChanged(p): if (logLevel < Info) return;
    "[" ; w=60; n = p*60/100 (integer);
    for i in 0..59: i<n ? "=" : i==n ? ">" : " "
    "]" ; " " + progressString ;  l = 1 + 60 + 2 + progressString.size();
    pad with ' ' from l up to previous lw;  lw = l;  "\r";  fflush
```

- Widths use `QString::size()` (UTF-16 units), not bytes.
- At 100% the bar is 60 `=` with no `>`. For p ≥ 100 the `>` would fall outside.
- The phase count printed is `phaseCount-1` = **6** (patched) or **2** (unpatched). `Done` has no
  `(n/m)`.
- Progress strings:

| When | String | Percent |
|---|---|---|
| Loading phases | `<n>%` | integer average of all resources in that loader (`progressSum / resources`) |
| Counting and resolving | `Object <i> of <n>` | `i*100/n` |
| Printing | `Preparing`, then `Page <i> of <n>` | 0, then `i*100/n` |

- The unpatched print phase ends with `progressChanged(-1)` and an empty string.

Exact stderr of a simple successful run (0.12.6, `conversion/basic.stderr`; `\r` shown as ⏎,
trailing spaces significant):

```
Loading pages (1/6)
[>                                                           ] 0%⏎[======>                                                     ] 10%⏎[==============================>                             ] 50%⏎[============================================================] 100%⏎Counting pages (2/6)                                               
[============================================================] Object 1 of 1⏎Resolving links (4/6)                                                       
[============================================================] Object 1 of 1⏎Loading headers and footers (5/6)                                           
Printing pages (6/6)
[>                                                           ] Preparing⏎[============================================================] Page 1 of 1⏎Done                                                                      
```

- `Loading TOC (3/6)` only appears when a TOC object exists.
- `Loading headers and footers (5/6)` always appears.
- An `Error:`/`Warning:` line printed after a progress bar overwrites it (after `\r`) and is padded
  to the bar width.
- The intermediate percentages (0/10/50/100) come from WebKit's load progress and are not
  deterministic.

### 4.2 Log levels

| Setting | Phases and progress | `Error:` | `Warning:` | `Info:` | `Debug:` |
|---|---|---|---|---|---|
| `--log-level none` / `-q` / `--quiet` | – | – | – | – | – |
| `error` | – | ✓ | – | – | – |
| `warn` | – | ✓ | ✓ | – | – |
| `info` (default) | ✓ | ✓ | ✓ | ✓ | – |
| `debug` (source only) | ✓ | ✓ | ✓ | ✓ | ✓ |

**Not affected by the log level** (always printed):
- Every parse error and usage text (section 1.2).
- The final `Exit with code …` line (4.3).
- Raw Qt warnings such as `QPainter::begin(): Returned false`.
- The unpatched-Qt "switch not supported" notice.

### 4.3 Exit codes (`handleError` in `utilities.cc`)

```cpp
int handleError(bool success, int errorCode) {
	cm = {400:"Bad Request", 401:"Unauthorized", 402:"Payment Required", 403:"Forbidden",
	      404:"Page not found", 405:"Method Not Allowed", 500:"Internal Server Error",
	      501:"Not Implemented", 503:"Service Unavailable", 505:"HTTP Version Not Supported"};
	ce = {404: 2, 401: 3};
	if (errorCode) {
		int c = ce.value(errorCode, EXIT_FAILURE /*1*/);
		if (errorCode < 1000)
			fprintf(stderr, "Exit with code %d due to http error: %d %s\n", c, errorCode, cm.value(errorCode, ""));
		else
			fprintf(stderr, "Exit with code %d due to network error: %s\n", c,
			        QMetaEnum(QNetworkReply::NetworkError).valueToKey(errorCode - 1000));
		return c;
	} else if (!success)
		fprintf(stderr, "Exit with code %d, due to unknown error.\n", EXIT_FAILURE);
	return success ? EXIT_SUCCESS : EXIT_FAILURE;
}
```

| Situation | Exit | Stderr last line | PDF written? |
|---|---|---|---|
| Success, no recorded error | 0 | – | yes |
| `--help`, `--version`, `--license`, `--manpage`, `--readme`, `--htmldoc`, `--dump-default-toc-xsl` | 0 | – | – |
| Parse or usage error | 1 | (usage text) | no |
| Any recorded HTTP 404 (main page, non-media sub-resource, or media with abort) | **2** | `Exit with code 2 due to http error: 404 Page not found` | yes if the main load did not abort |
| Recorded HTTP 401 | **3** | `Exit with code 3 due to http error: 401 Unauthorized` | idem |
| Other HTTP ≥ 400 | 1 | `Exit with code 1 due to http error: <code> <text>`. **Trailing space** when the code is not in the table (e.g. `410 `) | idem |
| Network error (code+1000), e.g. host not found, refused, blocked file via about:blank | 1 | `Exit with code 1 due to network error: HostNotFoundError` / `ConnectionRefusedError` / `ProtocolUnknownError` / … (Qt `NetworkError` enum key) | idem |
| Conversion failed without a recorded code (unwritable output, mixed margin units, bad header-html, …) | 1 | `Exit with code 1, due to unknown error.` | no |
| `--read-args-from-stdin` line failure | 1 | (none) | – |

**Key point:** a recorded error code beats success. A complete PDF plus exit code 1, 2 or 3 is a
normal outcome, for example with `--load-error-handling ignore` and a missing main page, a broken
non-media asset, or a blocked local file. The QHash lookups mean only 404→2 and 401→3 are special.

Messages verified on 0.12.6 (files in `docs/upstream-fixtures/0.12.6/conversion/`):

```
Error: Failed to load http://nonexist.html/, with network status code 3 and http status code 0 - Host nonexist.html not found
Error: Failed loading page http://nonexist.html (sometimes it will work just to ignore this error with --load-error-handling ignore)
Exit with code 1 due to network error: HostNotFoundError

Warning: Failed loading page http://nonexist.html (ignored)          (with --load-error-handling ignore; conversion continues, exit still 1)
Warning: Failed to load file:///wk/conv/missing.css (ignore)           (media, local access enabled)
Warning: Blocked access to file                                        (trailing space; then:)
Error: Failed to load about:blank, with network status code 301 and http status code 0 - Protocol "about" is unknown
Error: Unable to write to destination
Exit with code 1, due to unknown error.
```

With `-q`, only the `Exit with code …` lines remain. For example, mixed margin units under `-q`
print only `Exit with code 1, due to unknown error.`

---

## 5. Settings defaults summary (`PdfGlobal`, `PdfObject`, `LoadPage`, `Web`, `HeaderFooter`, `TableOfContent`)

```
PdfGlobal: size{pageSize=A4,height=-1mm,width=-1mm} logLevel=Info useGraphics=false resolveRelativeLinks=true
           orientation=Portrait colorMode=Color resolution=HighResolution dpi=96 pageOffset=0 copies=1 collate=true
           outline=true outlineDepth=4 dumpOutline="" out="" documentTitle="" useCompression=true
           margin{top=-1mm,right=10mm,bottom=-1mm,left=10mm} viewportSize="" imageDPI=600 imageQuality=94
           load{cookieJar=""}
PdfObject: toc{useDottedLines=true,captionText="Table of Contents",forwardLinks=true,backLinks=false,indentation="1em",fontScale=0.8}
           page="" header/footer{fontSize=12,fontName="Arial",left/right/center="",line=false,htmlUrl="",spacing=0}
           useExternalLinks=true useLocalLinks=true replacements=[] produceForms=false includeInOutline=true
           pagesCount=true isTableOfContent=false tocXsl=""
LoadPage:  username/password/ssl*="" jsdelay=200 windowStatus="" zoomFactor=1.0 customHeaders=[] repeatCustomHeaders=false
           cookies=[] post=[] blockLocalFileAccess=true allowed=[] stopSlowScripts=true debugJavascript=false
           loadErrorHandling=abort mediaLoadErrorHandling=ignore proxy{type=NoProxy,port=-1,host=""} runScript=[]
           checkbox*/radiobutton*Svg="" cacheDir="" bypassProxyForHosts=[] proxyHostNameLookup=false printMediaType=false
Web:       background=true loadImages=true enableJavascript=true enableIntelligentShrinking=true minimumFontSize=-1
           defaultEncoding="" userStyleSheet="" enablePlugins=false
```

---

## 6. Environment and process behaviour

- **Output to `-`:** written through `/dev/stdout` when it exists, so the PDF engine writes
  sequentially to the pipe. The Rust implementation must not seek on stdout.
- The output file is created only when printing starts. Load failures with abort leave no file.
  Successful-but-errored runs leave a complete file.
- **Stdin** is read fully, then the conversion starts. When both input and output are `-`, all of
  stdin must be consumed before any output (pdfkit closes its write end before reading).
- **Qt5 builds** set `QT_QPA_PLATFORM=offscreen` if unset. Not relevant for patched Qt4 builds,
  which run headless without X11 unless `--use-xserver`.
- **Fonts** come from the system fontconfig. Output fidelity depends on the installed fonts (the
  fixtures used DejaVu). This is out of scope for byte-identity of PDFs; PDF bytes are not expected
  to be identical across engines.

---

## 7. pdfkit (Ruby gem, 0.8.7.3 = GitHub master) integration contract

Source: `lib/pdfkit/{pdfkit,configuration,source,wkhtmltopdf,os}.rb`, read from the installed gem
and cross-checked against GitHub master.

### 7.1 Executable discovery

- `PDFKit.configuration.wkhtmltopdf` defaults to the output of `bundle exec which wkhtmltopdf` (if
  a Gemfile exists), else `` `which wkhtmltopdf` ``.
- Setting an explicit path that does not exist warns and falls back.
- `PDFKit.new` raises `PDFKit::NoExecutableError` unless `File.exist?(path)`.
  - Message: `"No wkhtmltopdf executable found at #{path}\n>> Please install wkhtmltopdf - https://github.com/pdfkit/PDFKit/wiki/Installing-WKHTMLTOPDF"`.
  - The Rust binary must therefore be a real file on `PATH` named `wkhtmltopdf`, or be configured
    explicitly.
- With `use_xvfb = true`, the argv is prefixed with `xvfb-run`.
- **No version detection.** pdfkit never runs `--version`, `-V` or `--help`.
  - Other ecosystem gems do. For example `wicked_pdf` runs `wkhtmltopdf -V` and parses
    `/wkhtmltopdf\s*(\d*\.\d*\.\d*\w*)/` (from memory, not verified here).
  - Keep the `wkhtmltopdf X.Y.Z (with patched qt)` format exactly.

### 7.2 Command line shape

```ruby
def command(path = nil)
  args = [*executable]                       # "wkhtmltopdf" or ["xvfb-run", "wkhtmltopdf"]
  args.concat(@renderer.options_for_command) # options hash flattened, in insertion order
  args << @source.to_input_for_command       # "-" for HTML string, file path for File/Tempfile, URL for /\Ahttp/
  args << (path ? path.to_s : '-')           # output: "-" (stdout) unless to_file(path)
  args
end
```

Default options, in this order (`Configuration#initialize`):

```ruby
{ disable_smart_shrinking: false, quiet: true, page_size: 'Letter',
  margin_top: '0.75in', margin_right: '0.75in', margin_bottom: '0.75in', margin_left: '0.75in',
  encoding: 'UTF-8' }
```

The resulting typical invocation for `PDFKit.new(html).to_pdf` is (argv array, **no shell**):

```
wkhtmltopdf --quiet --page-size Letter --margin-top 0.75in --margin-right 0.75in --margin-bottom 0.75in --margin-left 0.75in --encoding UTF-8 - -
```

Normalisation (`WkHTMLtoPDF#normalize_options`):

- Keys are `to_s.downcase.gsub(/[^a-z0-9]/,'-')` prefixed with `--`. The exceptions are `toc` and
  `cover`, which are emitted **bare as object keywords** in the middle of the option list.
- Values:
  - `false` or `nil`: the option is **dropped**. `disable_smart_shrinking: false` disappears.
  - `true` or the string `'true'`: flag with no value.
  - A Hash: flattened `k v k v`.
  - An Array: flattened `to_s`.
  - Anything else: `to_s`.
- Repeatable options are `--allow --bypass-proxy-for --cookie --custom-header --post --post-file
  --run-script --replace`.
  - A Hash value gives one `--opt k v` per pair. An Array of pairs gives one `--opt k v` per pair.
    An Array of scalars gives one `--opt x` per element.
- `options.delete(:quiet)` if `config.verbose = true`. Then progress goes to the inherited stderr.
- For non-URL sources, `<meta name="pdfkit-<option>" content="…">` tags in the HTML add options.
  `name="pdfkit-cookie my_cookie"`-style names (with a space) become Hash values.
- Options are placed **before the input**, so they are all in the global area. Hence:
  - Page options act as defaults for every object (that is the intended behaviour).
  - If the hash contains `toc: true`, every option that comes after `toc` in hash order is parsed
    in toc scope. A global-only option there (e.g. `page_size` added after `toc`) gives
    `--page-size specified in incorrect location`, exit 1, 15 newlines on stdout. This is verified.
  - `toc` followed by `-` `-` gives objects [toc, stdin page] and output stdout.
  - `cover: 'cover.html'` gives `cover cover.html`. Its options are parsed until the next
    non-option token, then the input `-` is the next object.
- Since all four margins are inches, the "all margin units must be the same" rule holds. A user
  overriding only some margins with another unit, e.g. `margin_top: '10mm'`, hits
  `Exit with code 1, due to unknown error.` and pdfkit raises.
- URL sources are escaped with `URI::DEFAULT_PARSER.escape` if needed.

### 7.3 I/O protocol

```ruby
result = IO.popen(invoke, "wb+") do |pdf|
  pdf.puts(@source.to_s) if @source.html?   # writes the HTML + "\n" (unless it already ends with "\n")
  pdf.close_write
  pdf.gets(nil) if path.nil?                # read ALL of stdout until EOF, binary
end
raise ImproperWkhtmltopdfExitStatus, invoke if empty_result?(path, result) || !successful?($?)
```

- stdin carries the HTML (binary mode), then EOF. stdout must contain **only** PDF bytes.
- stderr is **not captured**; it goes to the Ruby process's stderr. With the default `--quiet` it
  should be empty on success.
- Before the `puts`, pdfkit may run HTML preprocessing:
  - `root_url`/`protocol` rewriting of `href|src="/…"` and `//…`.
  - `stylesheets`: inline `<style>` blocks inserted before `</head>`.
- `successful?(status)`: true if exit 0, **or** if exit 2 **and** the options contain
  `--ignore-load-errors` or `--load-error-handling` with value `skip` or `ignore`.
  - **Exit 1 or 3 always raises**, even if a valid PDF was produced.
- `empty_result?`:
  - For stdout: `result.to_s.strip.empty?`. A parse error produces only newlines on stdout, which
    counts as empty, so it raises.
  - For `to_file(path)`: `File.size(path) == 0`, raising `Errno::ENOENT` if the file was never
    created.
- Error raised: `PDFKit::ImproperWkhtmltopdfExitStatus` with message
  `"Command failed (exitstatus=#{$?.exitstatus}): #{invoke}"` (`invoke` is the argv array,
  inspect-formatted).
- `PDFKit::Middleware` (Rack) uses the same path, plus `javascript_delay` from the
  `PDFKit-javascript-delay` response header.

### 7.4 pdfkit-relevant compatibility checklist

1. With `--quiet`, a successful run writes **nothing** to stderr and only the PDF to stdout.
2. Stdin HTML is written to a temp file and loaded as `file://`. Relative references therefore
   resolve against the temp directory and are **blocked by default**, which records a network
   error and **exits 1** with a valid PDF. pdfkit then raises.
   - Many apps work around this by passing `enable_local_file_access: true`, or by using
     absolute `http(s)` URLs, or by inlining assets.
   - **Both behaviours must be reproduced exactly**; they are deliberately depended on.
3. Exit 2 on 404 is tolerated only with `load_error_handling: 'ignore'` or `'skip'`.
4. Sub-resource failures of non-media types (fonts, `.min.js`, favicon, iframes) give a non-zero
   exit. Media types (`css js svg png jpg jpeg gif`, by `completeSuffix`) are only warnings by
   default.
5. The four-margin same-unit rule; the scope rules for `toc` and `cover` inside the options hash.
6. `--encoding UTF-8` is always passed. `--disable-smart-shrinking` is never passed by default, so
   smart shrinking is on.
7. Common pdfkit options in the wild:
   `page_size, orientation, margin_*, encoding, print_media_type, javascript_delay, window_status,
   no_stop_slow_scripts, disable_javascript, header_html/footer_html, header_* / footer_* text,
   header_spacing/footer_spacing, footer_font_size, dpi, zoom, lowquality, grayscale, title, toc,
   cover, outline, enable_local_file_access, allow, load_error_handling, load_media_error_handling,
   custom_header, cookie, username/password, viewport_size, disable_smart_shrinking, page_width/page_height,
   image_dpi, image_quality, no_background, run_script, debug_javascript, log_level, quiet`.

---

## 8. Reference fixtures

`docs/upstream-fixtures/0.12.6/` was captured from the official `wkhtmltopdf 0.12.6 (with patched
qt)` linux-amd64 binary in `ubuntu:20.04` (C locale, DejaVu fonts, `TZ=UTC`), on 2026-10-14.

- `cli/<case>.stdout`, `.stderr` and `.rc` for `--help`, `-h`, `-H`, `--version`, `-V`, `-qV`,
  `--readme`, `--manpage`, `--htmldoc`, `--license`, `--dump-default-toc-xsl`, `--dpi 300 -H`, and
  the error cases:
  - `noargs`, `onearg` (missing output)
  - `unknown_long`, `unknown_short`, `bad_value`, `notenough`
  - `wrongloc` (global option after an object), `tocopt_in_global`
  - `cover_missing`, `page_missing`, `dashdash`
  - `capture.sh` reproduces them.
- `conversion/<case>.stderr` and `.rc`:
  - `basic`, `quiet`
  - `warn` (blocked local files)
  - `info_b` (local access enabled; missing media; console and alert)
  - `missing_page`, `missing_page_ignore`, `missing_page_skip` (option after an object scopes only
    to it)
  - `toc` (with `--dump-outline` → `outline.xml`)
  - `badhost`, `unwritable`
  - `twoout` (second input silently becomes the output)
  - `marginmix` (cm + mm is OK)
  - `rs` (`--read-args-from-stdin`, two lines)
  - Inputs `a.html`, `b.html` and the script `run.sh`.

Progress percentages and dates in these files are nondeterministic. Compare the structure, or
normalise them.

---

## 9. Differences between 0.12.6 (released) and the 0.12.7-dev source

| Item | 0.12.6 | Source (0.12.7-dev) |
|---|---|---|
| `--log-level` values | none, error, warn, info. `debug` → `Invalid argument(s) parsed to --log-level` | adds `debug` |
| `--log-level` description | `Set log level to: none, error, warn or info` | `Set log level to: none, error, warn, info or debug` |
| JS console (`--debug-javascript`), alert, confirm, prompt | printed as `Warning: …` | `Info: …` |
| `--debug-javascript` / `--no-debug-javascript` description | `Show javascript debugging output` / `Do not show javascript debugging output` | `… console output` |
| `--default-header` description | `… this is short for: --header-left='[webpage]' --header-right='[page]/[toPage]' --top 2cm --header-line` | `… --header-left '[webpage]' --header-right '[page]/[toPage]' --margin-top 2cm --header-line` |
| Doc URLs | `https://qt-project.org/doc/qt-4.8/...` | `https://doc.qt.io/archives/qt-4.8/...` |
| Footers And Headers text | `… to appear in the upper left corner …` | `upper right corner` |
| Outlines text | `for a in-depth description …`, `where spread to generous` | `for an in-depth …`, `are spread too generously` |
| TOC doc | `…"http://wkhtmltopdf.org/outline" it has a root…`, `any number of item.` | adds a comma; `any number of items.` |
| Version string | `0.12.6` | `0.12.7-dev` |

The option set (122 options), defaults, parsing, exit codes and message formats are otherwise the
same.

---

## 10. Surprises worth a test each

1. Usage on error prints wrap and section newlines to **stdout** (15 bytes for the default help),
   and the rest to stderr.
2. The last argv entry is always the output, even if it looks like an option. Two inputs with no
   output silently overwrite the second.
3. `--` is broken (`Unknown long argument --`), while `--0…` silently enters "default mode".
4. Help "(default …)" values reflect options parsed earlier on the same command line.
5. A complete PDF is produced and the exit code is still 1, 2 or 3 when any non-media resource
   failed, the main page failed with `ignore`, or a local file was blocked (`about:blank` →
   `ProtocolUnknownError`).
6. A missing local input file name becomes `http://<name>`.
7. `jquery.min.js` is not a "media file" (`completeSuffix`), so its 404 is an error, while
   `jquery.js` is just a warning.
8. Mixed margin units fail. The default left/right margins are mm, so `--margin-top 1in` alone
   fails. Header spacing and measured header heights are mm values added in the left margin's
   unit.
9. `--no-custom-header-propagation` enables propagation.
10. `--page-offset`, `--viewport-size`, `--default-header` and `--*-relative-links` are page-scoped
    options that write global settings.
11. A cover counts as exactly one page in `[topage]`, whatever its real length. TOC entries list
    the TOC's own title.
12. Multipart POST uses `\n` and `multipart/form-data, boundary=`.
13. `--read-args-from-stdin`: a blank line aborts the whole batch with a usage error and exit 1.
14. HTML header query parameters come in hash order and include `subsubsection` and `--replace`
    keys.
