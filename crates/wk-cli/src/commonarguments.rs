//! Switches shared by wkhtmltopdf and wkhtmltoimage. Ports upstream
//! `src/shared/commonarguments.cc` (`addDocArgs`, `addGlobalLoadArgs`,
//! `addWebArgs`, `addPageLoadArgs`), with the 0.12.6 descriptions (spec §9).

use wk_settings::{LoadGlobal, LoadPage, Web};

use crate::arghandler::{
    Action, Caller, ConstSetter, FloatSetter, IntSetter, Lens, LoadErrorHandlingSetting, MapSetter, Outcome,
    PostFieldCreator, PostFileCreator, ProxySetter, QStrSetter, StringListSetter, StringPairCreator,
};
use crate::commandlineparserbase::CommandLineParserBase;
use crate::field;

fn help<G, P>(_: &[Vec<u8>], _: &mut G, _: &mut P) -> Outcome {
    Outcome::Action(Action::Help { extended: false })
}
fn extended_help<G, P>(_: &[Vec<u8>], _: &mut G, _: &mut P) -> Outcome {
    Outcome::Action(Action::Help { extended: true })
}
fn version<G, P>(_: &[Vec<u8>], _: &mut G, _: &mut P) -> Outcome {
    Outcome::Action(Action::Version)
}
fn license<G, P>(_: &[Vec<u8>], _: &mut G, _: &mut P) -> Outcome {
    Outcome::Action(Action::License)
}
fn manpage<G, P>(_: &[Vec<u8>], _: &mut G, _: &mut P) -> Outcome {
    Outcome::Action(Action::ManPage)
}
fn htmldoc<G, P>(_: &[Vec<u8>], _: &mut G, _: &mut P) -> Outcome {
    Outcome::Action(Action::Readme { html: true })
}
fn readme<G, P>(_: &[Vec<u8>], _: &mut G, _: &mut P) -> Outcome {
    Outcome::Action(Action::Readme { html: false })
}

impl<G: 'static, P: 'static> CommandLineParserBase<G, P> {
    /// `addDocArgs`.
    pub fn add_doc_args(&mut self) {
        self.extended(false);
        self.qthack(false);
        self.addarg("help", Some(b'h'), "Display help", Caller::new(help));
        self.addarg("version", Some(b'V'), "Output version information and exit", Caller::new(version));
        self.addarg("license", None, "Output license information and exit", Caller::new(license));
        self.addarg(
            "extended-help",
            Some(b'H'),
            "Display more extensive help, detailing less common command switches",
            Caller::new(extended_help),
        );

        self.extended(true);
        self.qthack(false);
        self.addarg("manpage", None, "Output program man page", Caller::new(manpage));
        self.addarg("htmldoc", None, "Output program html help", Caller::new(htmldoc));
        self.addarg("readme", None, "Output program readme", Caller::new(readme));
    }

    /// `addGlobalLoadArgs`.
    pub fn add_global_load_args(&mut self, s: Lens<G, P, LoadGlobal>) {
        self.extended(true);
        self.qthack(false);
        self.addarg(
            "cookie-jar",
            None,
            "Read and write cookies from and to the supplied cookie jar file",
            QStrSetter::new(field!(s, cookie_jar), "path"),
        );
    }

    /// `addWebArgs`.
    pub fn add_web_args(&mut self, s: Lens<G, P, Web>) {
        self.extended(true);
        self.qthack(false);

        self.addarg(
            "enable-plugins",
            None,
            "Enable installed plugins (plugins will likely not work)",
            ConstSetter::new(field!(s, enable_plugins), true),
        );
        self.addarg(
            "disable-plugins",
            None,
            "Disable installed plugins",
            ConstSetter::new(field!(s, enable_plugins), false),
        );

        self.addarg(
            "minimum-font-size",
            None,
            "Minimum font size",
            IntSetter::new(field!(s, minimum_font_size), "int"),
        );
        self.addarg(
            "user-style-sheet",
            None,
            "Specify a user style sheet, to load with every page",
            QStrSetter::new(field!(s, user_style_sheet), "path"),
        );
        self.addarg(
            "no-images",
            None,
            "Do not load or print images",
            ConstSetter::new(field!(s, load_images), false),
        );
        self.addarg(
            "images",
            None,
            "Do load or print images",
            ConstSetter::new(field!(s, load_images), true),
        );

        self.addarg(
            "disable-javascript",
            Some(b'n'),
            "Do not allow web pages to run javascript",
            ConstSetter::new(field!(s, enable_javascript), false),
        );
        self.addarg(
            "enable-javascript",
            None,
            "Do allow web pages to run javascript",
            ConstSetter::new(field!(s, enable_javascript), true),
        );

        self.extended(true);
        self.qthack(true);
        self.qthack(false);
        self.addarg(
            "encoding",
            None,
            "Set the default text encoding, for input",
            QStrSetter::new(field!(s, default_encoding), "encoding"),
        );
    }

    /// `addPageLoadArgs`.
    pub fn add_page_load_args(&mut self, s: Lens<G, P, LoadPage>) {
        self.extended(true);
        self.qthack(false);
        self.addarg("proxy", Some(b'p'), "Use a proxy", ProxySetter::new(field!(s, proxy), "proxy"));
        self.addarg(
            "proxy-hostname-lookup",
            None,
            "Use the proxy for resolving hostnames",
            ConstSetter::new(field!(s, proxy_host_name_lookup), true),
        );
        self.addarg(
            "bypass-proxy-for",
            None,
            "Bypass proxy for host (repeatable)",
            StringListSetter::new(field!(s, bypass_proxy_for_hosts), "value"),
        );
        self.addarg(
            "username",
            None,
            "HTTP Authentication username",
            QStrSetter::new(field!(s, username), "username"),
        );
        self.addarg(
            "password",
            None,
            "HTTP Authentication password",
            QStrSetter::new(field!(s, password), "password"),
        );
        self.addarg(
            "ssl-key-path",
            None,
            "Path to ssl client cert private key in OpenSSL PEM format",
            QStrSetter::new(field!(s, client_ssl_key_path), "path"),
        );
        self.addarg(
            "ssl-key-password",
            None,
            "Password to ssl client cert private key",
            QStrSetter::new(field!(s, client_ssl_key_password), "password"),
        );
        self.addarg(
            "ssl-crt-path",
            None,
            "Path to the ssl client cert public key in OpenSSL PEM format, optionally followed by intermediate ca and trusted certs",
            QStrSetter::new(field!(s, client_ssl_crt_path), "path"),
        );
        self.addarg(
            "load-error-handling",
            None,
            "Specify how to handle pages that fail to load: abort, ignore or skip",
            LoadErrorHandlingSetting::new(field!(s, load_error_handling), "handler"),
        );
        self.addarg(
            "load-media-error-handling",
            None,
            "Specify how to handle media files that fail to load: abort, ignore or skip",
            LoadErrorHandlingSetting::new(field!(s, media_load_error_handling), "handler"),
        );
        self.addarg(
            "custom-header",
            None,
            "Set an additional HTTP header (repeatable)",
            MapSetter::<G, P, StringPairCreator>::new(field!(s, custom_headers), "name", "value"),
        );
        self.addarg(
            "custom-header-propagation",
            None,
            "Add HTTP headers specified by --custom-header for each resource request.",
            ConstSetter::new(field!(s, repeat_custom_headers), true),
        );
        // Upstream bug, reproduced on purpose: the "no-" form also sets true.
        self.addarg(
            "no-custom-header-propagation",
            None,
            "Do not add HTTP headers specified by --custom-header for each resource request.",
            ConstSetter::new(field!(s, repeat_custom_headers), true),
        );

        self.addarg(
            "javascript-delay",
            None,
            "Wait some milliseconds for javascript finish",
            IntSetter::new(field!(s, jsdelay), "msec"),
        );
        self.addarg(
            "window-status",
            None,
            "Wait until window.status is equal to this string before rendering page",
            QStrSetter::new(field!(s, window_status), "windowStatus"),
        );

        self.addarg(
            "zoom",
            None,
            "Use this zoom factor",
            FloatSetter::with_def(field!(s, zoom_factor), "float", true),
        );
        self.addarg(
            "cookie",
            None,
            "Set an additional cookie (repeatable), value should be url encoded.",
            MapSetter::<G, P, StringPairCreator>::new(field!(s, cookies), "name", "value"),
        );
        self.addarg(
            "post",
            None,
            "Add an additional post field (repeatable)",
            MapSetter::<G, P, PostFieldCreator>::new(field!(s, post), "name", "value"),
        );
        self.addarg(
            "post-file",
            None,
            "Post an additional file (repeatable)",
            MapSetter::<G, P, PostFileCreator>::new(field!(s, post), "name", "path"),
        );

        self.addarg(
            "disable-local-file-access",
            None,
            "Do not allowed conversion of a local file to read in other local files, unless explicitly allowed with --allow",
            ConstSetter::new(field!(s, block_local_file_access), true),
        );
        self.addarg(
            "enable-local-file-access",
            None,
            "Allowed conversion of a local file to read in other local files.",
            ConstSetter::new(field!(s, block_local_file_access), false),
        );
        self.addarg(
            "allow",
            None,
            "Allow the file or files from the specified folder to be loaded (repeatable)",
            StringListSetter::new(field!(s, allowed), "path"),
        );

        self.addarg("cache-dir", None, "Web cache directory", QStrSetter::new(field!(s, cache_dir), "path"));

        self.addarg(
            "debug-javascript",
            None,
            "Show javascript debugging output",
            ConstSetter::new(field!(s, debug_javascript), true),
        );
        self.addarg(
            "no-debug-javascript",
            None,
            "Do not show javascript debugging output",
            ConstSetter::new(field!(s, debug_javascript), false),
        );
        self.addarg(
            "stop-slow-scripts",
            None,
            "Stop slow running javascripts",
            ConstSetter::new(field!(s, stop_slow_scripts), true),
        );
        self.addarg(
            "no-stop-slow-scripts",
            None,
            "Do not Stop slow running javascripts",
            ConstSetter::new(field!(s, stop_slow_scripts), false),
        );
        self.addarg(
            "run-script",
            None,
            "Run this additional javascript after the page is done loading (repeatable)",
            StringListSetter::new(field!(s, run_script), "js"),
        );

        self.addarg(
            "checkbox-svg",
            None,
            "Use this SVG file when rendering unchecked checkboxes",
            QStrSetter::with_def(field!(s, checkbox_svg), "path", true),
        );
        self.addarg(
            "checkbox-checked-svg",
            None,
            "Use this SVG file when rendering checked checkboxes",
            QStrSetter::with_def(field!(s, checkbox_checked_svg), "path", true),
        );
        self.addarg(
            "radiobutton-svg",
            None,
            "Use this SVG file when rendering unchecked radiobuttons",
            QStrSetter::with_def(field!(s, radiobutton_svg), "path", true),
        );
        self.addarg(
            "radiobutton-checked-svg",
            None,
            "Use this SVG file when rendering checked radiobuttons",
            QStrSetter::with_def(field!(s, radiobutton_checked_svg), "path", true),
        );
    }
}
