//! Web engine settings. Ports upstream `src/lib/websettings.hh` and
//! `src/lib/websettings.cc`.

/// `settings::Web`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Web {
    pub background: bool,
    pub load_images: bool,
    pub enable_javascript: bool,
    pub enable_intelligent_shrinking: bool,
    pub minimum_font_size: i32,
    pub default_encoding: String,
    pub user_style_sheet: String,
    pub enable_plugins: bool,
}

impl Default for Web {
    fn default() -> Self {
        Web {
            background: true,
            load_images: true,
            enable_javascript: true,
            enable_intelligent_shrinking: true,
            minimum_font_size: -1,
            default_encoding: String::new(),
            user_style_sheet: String::new(),
            enable_plugins: false,
        }
    }
}
