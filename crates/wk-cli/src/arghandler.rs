//! Argument handlers: what a switch does when it is parsed, and how its
//! "(default …)" suffix is computed for the help output.
//!
//! Ports upstream `src/shared/arghandler.inl`, `src/shared/arghandler.cc` and
//! the value setters of `src/shared/commonarguments.cc`.
//!
//! Upstream handlers hold a reference into a template `PdfObject` (`od`) and
//! remap it onto the object being parsed (`mapAddress`). Here a handler holds a
//! [`Lens`] that reaches its field either in the global state `G` or in the
//! page state `P`; the parser passes the object being parsed when a switch is
//! applied, and the pristine template when the help text is rendered.

use std::rc::Rc;

use wk_settings::loadsettings::{self, LoadErrorHandling, PostItem, Proxy, load_error_handling_to_str};
use wk_settings::logging::{self, LogLevel};
use wk_settings::qt;

type GetFn<G, P, T> = Rc<dyn for<'a> Fn(&'a G, &'a P) -> &'a T>;
type GetMutFn<G, P, T> = Rc<dyn for<'a> Fn(&'a mut G, &'a mut P) -> &'a mut T>;

fn mk_get<G, P, T: ?Sized, F>(f: F) -> GetFn<G, P, T>
where
    F: for<'a> Fn(&'a G, &'a P) -> &'a T + 'static,
{
    Rc::new(f)
}

fn mk_get_mut<G, P, T: ?Sized, F>(f: F) -> GetMutFn<G, P, T>
where
    F: for<'a> Fn(&'a mut G, &'a mut P) -> &'a mut T + 'static,
{
    Rc::new(f)
}

/// Access path to one settings field, either in the global state `G` or in the
/// page state `P` (the `DstArgHandler::dst` reference of upstream).
pub struct Lens<G, P, T> {
    get: GetFn<G, P, T>,
    get_mut: GetMutFn<G, P, T>,
}

impl<G, P, T> Clone for Lens<G, P, T> {
    fn clone(&self) -> Self {
        Lens { get: self.get.clone(), get_mut: self.get_mut.clone() }
    }
}

impl<G: 'static, P: 'static, T: 'static> Lens<G, P, T> {
    /// A field of the global state.
    pub fn global(get: fn(&G) -> &T, get_mut: fn(&mut G) -> &mut T) -> Self {
        Lens { get: mk_get(move |g, _| get(g)), get_mut: mk_get_mut(move |g, _| get_mut(g)) }
    }

    /// A field of the page (object) state.
    pub fn page(get: fn(&P) -> &T, get_mut: fn(&mut P) -> &mut T) -> Self {
        Lens { get: mk_get(move |_, p| get(p)), get_mut: mk_get_mut(move |_, p| get_mut(p)) }
    }

    /// A sub-field of this field.
    pub fn field<U: 'static>(&self, get: fn(&T) -> &U, get_mut: fn(&mut T) -> &mut U) -> Lens<G, P, U> {
        let g = self.get.clone();
        let gm = self.get_mut.clone();
        Lens { get: mk_get(move |a, b| get(g(a, b))), get_mut: mk_get_mut(move |a, b| get_mut(gm(a, b))) }
    }

    pub fn get<'a>(&self, g: &'a G, p: &'a P) -> &'a T {
        (self.get)(g, p)
    }

    pub fn get_mut<'a>(&self, g: &'a mut G, p: &'a mut P) -> &'a mut T {
        (self.get_mut)(g, p)
    }
}

/// Builds a [`Lens`] on a sub-field: `field!(web_lens, enable_plugins)`.
#[macro_export]
macro_rules! field {
    ($lens:expr, $($f:ident).+) => {
        $lens.field(|x| &x.$($f).+, |x| &mut x.$($f).+)
    };
}

/// Documentation actions. Upstream runs them inside the handler and calls
/// `exit(0)`; here the handler reports them and the parser's owner runs them
/// with the state as parsed so far.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    /// `--help` (`extended == false`) or `--extended-help`.
    Help {
        extended: bool,
    },
    Version,
    License,
    ManPage,
    /// `--readme` (`html == false`) or `--htmldoc`.
    Readme {
        html: bool,
    },
    /// Tool-specific action (for example `dump-default-toc-xsl`).
    Custom(&'static str),
}

/// Result of applying a switch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    Ok,
    /// The handler returned false: `Invalid argument(s) parsed to …`.
    Invalid,
    Action(Action),
}

impl From<bool> for Outcome {
    fn from(ok: bool) -> Self {
        if ok { Outcome::Ok } else { Outcome::Invalid }
    }
}

/// The polymorphic part of `ArgHandler` (`operator()` and `getDesc`).
pub trait Handle<G, P> {
    /// Apply the switch. `args` holds exactly `argn().len()` raw argv entries.
    fn call(&self, args: &[Vec<u8>], g: &mut G, p: &mut P) -> Outcome;
    /// Names of the switch arguments (`ArgHandler::argn`).
    fn argn(&self) -> Vec<String> {
        Vec::new()
    }
    /// The `" (default …)"` suffix of `getDesc`, computed from the current
    /// value of the destination field.
    fn default_suffix(&self, _g: &G, _p: &P) -> Option<String> {
        None
    }
}

/// `ArgHandler`: a registered switch.
pub struct ArgHandler<G, P> {
    pub long_name: String,
    pub desc: String,
    pub short_switch: Option<u8>,
    pub argn: Vec<String>,
    pub display: bool,
    pub extended: bool,
    pub qthack: bool,
    /// Bit mask of the scopes the switch is valid in.
    pub section: i32,
    pub handler: Box<dyn Handle<G, P>>,
}

impl<G, P> ArgHandler<G, P> {
    /// `getDesc()`: the description plus the default value, if any.
    pub fn get_desc(&self, g: &G, od: &P) -> String {
        match self.handler.default_suffix(g, od) {
            Some(s) => format!("{}{}", self.desc, s),
            None => self.desc.clone(),
        }
    }
}

/// Decode an argv entry like `QString::fromLocal8Bit`.
pub fn from_local8bit(b: &[u8]) -> String {
    String::from_utf8_lossy(b).into_owned()
}

/// `ConstSetter<T>`: assigns a constant; the help says "(default)" when the
/// constant equals the current value.
pub struct ConstSetter<G, P, T> {
    dst: Lens<G, P, T>,
    src: T,
}

impl<G, P, T> ConstSetter<G, P, T> {
    pub fn new(dst: Lens<G, P, T>, src: T) -> Self {
        ConstSetter { dst, src }
    }
}

impl<G: 'static, P: 'static, T: Clone + PartialEq + 'static> Handle<G, P> for ConstSetter<G, P, T> {
    fn call(&self, _: &[Vec<u8>], g: &mut G, p: &mut P) -> Outcome {
        *self.dst.get_mut(g, p) = self.src.clone();
        Outcome::Ok
    }
    fn default_suffix(&self, g: &G, p: &P) -> Option<String> {
        (*self.dst.get(g, p) == self.src).then(|| " (default)".to_string())
    }
}

/// The template-method part of `SomeSetter` (`strToT` / `TToStr`).
pub trait ValueType {
    type T;
    /// Parse a raw argv entry. The value is assigned even when invalid,
    /// like upstream.
    fn str_to_t(val: &[u8]) -> (Self::T, bool);
    /// Text shown as "(default …)"; `None` when no default is printed.
    fn t_to_str(_t: &Self::T) -> Option<String> {
        None
    }
}

/// `SomeSetter<TT>`: a switch with one argument parsed by `TT`.
pub struct SomeSetter<G, P, TT: ValueType> {
    dst: Lens<G, P, TT::T>,
    argn: String,
    has_def: bool,
}

impl<G, P, TT: ValueType> SomeSetter<G, P, TT> {
    pub fn new(dst: Lens<G, P, TT::T>, argn: &str) -> Self {
        SomeSetter { dst, argn: argn.to_string(), has_def: true }
    }
    /// The `def` constructor argument (`false` hides the default).
    pub fn with_def(dst: Lens<G, P, TT::T>, argn: &str, has_def: bool) -> Self {
        SomeSetter { dst, argn: argn.to_string(), has_def }
    }
}

impl<G: 'static, P: 'static, TT: ValueType> Handle<G, P> for SomeSetter<G, P, TT>
where
    TT::T: 'static,
{
    fn call(&self, args: &[Vec<u8>], g: &mut G, p: &mut P) -> Outcome {
        let (v, ok) = TT::str_to_t(&args[0]);
        *self.dst.get_mut(g, p) = v;
        ok.into()
    }
    fn argn(&self) -> Vec<String> {
        vec![self.argn.clone()]
    }
    fn default_suffix(&self, g: &G, p: &P) -> Option<String> {
        if !self.has_def {
            return None;
        }
        TT::t_to_str(self.dst.get(g, p)).map(|x| format!(" (default {x})"))
    }
}

fn lossy(b: &[u8]) -> std::borrow::Cow<'_, str> {
    String::from_utf8_lossy(b)
}

/// `IntTM`: `QString::toInt`; default shown unless `-1`.
pub struct IntTM;
impl ValueType for IntTM {
    type T = i32;
    fn str_to_t(val: &[u8]) -> (i32, bool) {
        match qt::to_int(&lossy(val)) {
            Some(v) => (v, true),
            None => (0, false),
        }
    }
    fn t_to_str(t: &i32) -> Option<String> {
        (*t != -1).then(|| t.to_string())
    }
}

/// `FloatTM`: `QString::toFloat`; default shown unless `-1`.
pub struct FloatTM;
impl ValueType for FloatTM {
    type T = f32;
    fn str_to_t(val: &[u8]) -> (f32, bool) {
        match qt::to_float(&lossy(val)) {
            Some(v) => (v, true),
            None => (0.0, false),
        }
    }
    fn t_to_str(t: &f32) -> Option<String> {
        (*t != -1.0).then(|| qt::number_g(f64::from(*t)))
    }
}

/// `LogLevelTM`.
pub struct LogLevelTM;
impl ValueType for LogLevelTM {
    type T = LogLevel;
    fn str_to_t(val: &[u8]) -> (LogLevel, bool) {
        logging::str_to_log_level(&lossy(val))
    }
    fn t_to_str(t: &LogLevel) -> Option<String> {
        logging::log_level_to_str(*t).map(str::to_string)
    }
}

/// `QStrTM`: any string; default shown when non-empty.
pub struct QStrTM;
impl ValueType for QStrTM {
    type T = String;
    fn str_to_t(val: &[u8]) -> (String, bool) {
        (from_local8bit(val), true)
    }
    fn t_to_str(t: &String) -> Option<String> {
        (!t.is_empty()).then(|| t.clone())
    }
}

/// `LoadErrorHandlingTM` (from `commonarguments.cc`).
pub struct LoadErrorHandlingTM;
impl ValueType for LoadErrorHandlingTM {
    type T = LoadErrorHandling;
    fn str_to_t(val: &[u8]) -> (LoadErrorHandling, bool) {
        loadsettings::str_to_load_error_handling(&lossy(val))
    }
    fn t_to_str(t: &LoadErrorHandling) -> Option<String> {
        Some(load_error_handling_to_str(*t).to_string())
    }
}

/// `ProxyTM` (from `commonarguments.cc`); never shows a default.
pub struct ProxyTM;
impl ValueType for ProxyTM {
    type T = Proxy;
    fn str_to_t(val: &[u8]) -> (Proxy, bool) {
        loadsettings::str_to_proxy(&lossy(val))
    }
}

pub type IntSetter<G, P> = SomeSetter<G, P, IntTM>;
pub type FloatSetter<G, P> = SomeSetter<G, P, FloatTM>;
pub type LogLevelSetter<G, P> = SomeSetter<G, P, LogLevelTM>;
pub type QStrSetter<G, P> = SomeSetter<G, P, QStrTM>;
pub type LoadErrorHandlingSetting<G, P> = SomeSetter<G, P, LoadErrorHandlingTM>;
pub type ProxySetter<G, P> = SomeSetter<G, P, ProxyTM>;

/// Builds the list element of a `MapSetter` (`StringPairCreator`,
/// `PostItemCreator<file>`).
pub trait PairCreator {
    type T;
    fn create(key: String, value: String) -> Self::T;
}

/// `StringPairCreator`.
pub struct StringPairCreator;
impl PairCreator for StringPairCreator {
    type T = (String, String);
    fn create(key: String, value: String) -> Self::T {
        (key, value)
    }
}

/// `PostItemCreator<false>`.
pub struct PostFieldCreator;
impl PairCreator for PostFieldCreator {
    type T = PostItem;
    fn create(name: String, value: String) -> PostItem {
        PostItem { name, value, file: false }
    }
}

/// `PostItemCreator<true>`.
pub struct PostFileCreator;
impl PairCreator for PostFileCreator {
    type T = PostItem;
    fn create(name: String, value: String) -> PostItem {
        PostItem { name, value, file: true }
    }
}

/// `MapSetter<T>`: a repeatable two-argument switch appending to a list.
pub struct MapSetter<G, P, C: PairCreator> {
    dst: Lens<G, P, Vec<C::T>>,
    key_name: String,
    value_name: String,
}

impl<G, P, C: PairCreator> MapSetter<G, P, C> {
    pub fn new(dst: Lens<G, P, Vec<C::T>>, key_name: &str, value_name: &str) -> Self {
        MapSetter { dst, key_name: key_name.into(), value_name: value_name.into() }
    }
}

impl<G: 'static, P: 'static, C: PairCreator> Handle<G, P> for MapSetter<G, P, C>
where
    C::T: 'static,
{
    fn call(&self, args: &[Vec<u8>], g: &mut G, p: &mut P) -> Outcome {
        let item = C::create(from_local8bit(&args[0]), from_local8bit(&args[1]));
        self.dst.get_mut(g, p).push(item);
        Outcome::Ok
    }
    fn argn(&self) -> Vec<String> {
        vec![self.key_name.clone(), self.value_name.clone()]
    }
}

/// `StringListSetter`: a repeatable one-argument switch appending to a list.
pub struct StringListSetter<G, P> {
    dst: Lens<G, P, Vec<String>>,
    value_name: String,
}

impl<G, P> StringListSetter<G, P> {
    pub fn new(dst: Lens<G, P, Vec<String>>, value_name: &str) -> Self {
        StringListSetter { dst, value_name: value_name.into() }
    }
}

impl<G: 'static, P: 'static> Handle<G, P> for StringListSetter<G, P> {
    fn call(&self, args: &[Vec<u8>], g: &mut G, p: &mut P) -> Outcome {
        self.dst.get_mut(g, p).push(from_local8bit(&args[0]));
        Outcome::Ok
    }
    fn argn(&self) -> Vec<String> {
        vec![self.value_name.clone()]
    }
}

/// Signature of a `Caller` function.
pub type CallerFn<G, P> = fn(&[Vec<u8>], &mut G, &mut P) -> Outcome;

/// `Caller<T>`: calls a function.
pub struct Caller<G, P> {
    f: CallerFn<G, P>,
    argn: Vec<String>,
}

impl<G, P> Caller<G, P> {
    pub fn new(f: CallerFn<G, P>) -> Self {
        Caller { f, argn: Vec::new() }
    }
    /// A function switch taking one argument.
    pub fn with_arg(f: CallerFn<G, P>, a1: &str) -> Self {
        Caller { f, argn: vec![a1.to_string()] }
    }
}

impl<G, P> Handle<G, P> for Caller<G, P> {
    fn call(&self, args: &[Vec<u8>], g: &mut G, p: &mut P) -> Outcome {
        (self.f)(args, g, p)
    }
    fn argn(&self) -> Vec<String> {
        self.argn.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Default)]
    struct G {
        n: i32,
        f: f32,
        s: String,
        flag: bool,
    }
    #[derive(Default)]
    struct P {
        list: Vec<(String, String)>,
    }

    fn a(s: &str) -> Vec<Vec<u8>> {
        s.split(' ').map(|x| x.as_bytes().to_vec()).collect()
    }

    #[test]
    fn int_setter_parses_and_shows_default() {
        let h: IntSetter<G, P> = SomeSetter::new(Lens::global(|g| &g.n, |g| &mut g.n), "n");
        let (mut g, mut p) = (G::default(), P::default());
        assert_eq!(h.call(&a("42"), &mut g, &mut p), Outcome::Ok);
        assert_eq!(g.n, 42);
        assert_eq!(h.default_suffix(&g, &p).as_deref(), Some(" (default 42)"));
        assert_eq!(h.call(&a("x"), &mut g, &mut p), Outcome::Invalid);
        g.n = -1;
        assert_eq!(h.default_suffix(&g, &p), None);
    }

    #[test]
    fn float_setter_display() {
        let h: FloatSetter<G, P> = SomeSetter::new(Lens::global(|g| &g.f, |g| &mut g.f), "real");
        let (mut g, mut p) = (G::default(), P::default());
        assert_eq!(h.call(&a("0.8"), &mut g, &mut p), Outcome::Ok);
        assert_eq!(h.default_suffix(&g, &p).as_deref(), Some(" (default 0.8)"));
        assert_eq!(h.call(&a("1e40"), &mut g, &mut p), Outcome::Invalid);
    }

    #[test]
    fn string_setter_hides_empty_default() {
        let h: QStrSetter<G, P> = SomeSetter::new(Lens::global(|g| &g.s, |g| &mut g.s), "text");
        let (g, p) = (G::default(), P::default());
        assert_eq!(h.default_suffix(&g, &p), None);
        let h2: QStrSetter<G, P> = SomeSetter::with_def(Lens::global(|g| &g.s, |g| &mut g.s), "text", false);
        let g = G { s: "x".into(), ..G::default() };
        assert_eq!(h2.default_suffix(&g, &p), None);
        assert_eq!(h.default_suffix(&g, &p).as_deref(), Some(" (default x)"));
    }

    #[test]
    fn const_setter_default_marker() {
        let on = ConstSetter::new(Lens::<G, P, bool>::global(|g| &g.flag, |g| &mut g.flag), true);
        let (mut g, mut p) = (G::default(), P::default());
        assert_eq!(on.default_suffix(&g, &p), None);
        on.call(&[], &mut g, &mut p);
        assert!(g.flag);
        assert_eq!(on.default_suffix(&g, &p).as_deref(), Some(" (default)"));
    }

    #[test]
    fn map_setter_appends_pairs() {
        let h: MapSetter<G, P, StringPairCreator> =
            MapSetter::new(Lens::page(|p| &p.list, |p| &mut p.list), "name", "value");
        let (mut g, mut p) = (G::default(), P::default());
        h.call(&a("k v"), &mut g, &mut p);
        h.call(&a("k2 v2"), &mut g, &mut p);
        assert_eq!(p.list, vec![("k".into(), "v".into()), ("k2".into(), "v2".into())]);
        assert_eq!(h.argn(), vec!["name".to_string(), "value".to_string()]);
    }
}
