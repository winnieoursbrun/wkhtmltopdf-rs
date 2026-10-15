use crate::HtmlParserProvider;
use blitz_traits::{
    navigation::NavigationProvider,
    net::{AbortSignal, NetProvider},
    shell::{ShellProvider, Viewport},
};
use parley::FontContext;
use std::sync::Arc;
use style::media_queries::MediaType;

/// wkhtmltopdf-rs: number of threads a document may use while resolving (1 = everything on
/// the calling thread). See [`set_parallelism`].
static PARALLELISM: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(1);

/// wkhtmltopdf-rs: let large documents be styled and have their inline layouts built on the
/// rayon pool of the resolving thread (the global pool from a thread outside any pool) when
/// `threads` > 1. Small documents stay on the calling thread. The results do not depend on it.
pub fn set_parallelism(threads: usize) {
    PARALLELISM.store(threads.max(1), std::sync::atomic::Ordering::Relaxed);
}

/// Documents with at least this many nodes are styled on Stylo's pool.
pub(crate) const PARALLEL_STYLE_MIN_NODES: usize = 2000;

/// Inline layouts are built on the rayon pool when at least this many are (re)built.
pub(crate) const PARALLEL_CONSTRUCT_MIN_TASKS: usize = 256;

/// The value given to [`set_parallelism`] (1 by default).
pub fn parallelism() -> usize {
    PARALLELISM.load(std::sync::atomic::Ordering::Relaxed)
}

/// Strategy for Stylo's style traversal during `resolve`.
///
/// Two `Document`s resolving on [`StyleThreading::Parallel`] concurrently
/// share Stylo's global thread pool and can panic with
/// `already mutably borrowed` — see
/// <https://github.com/DioxusLabs/blitz/issues/430>. Set
/// [`StyleThreading::Sequential`] on documents that may resolve from a
/// user thread while another `Parallel` resolve is in flight.
#[derive(Default, Clone, Copy, PartialEq, Eq, Debug)]
pub enum StyleThreading {
    /// Use Stylo's parallel traversal via its global rayon thread pool.
    /// Fastest for a single document; panics if another `Parallel` resolve
    /// is in flight on a different thread.
    Parallel,
    /// Run style traversal sequentially on the calling thread, bypassing
    /// the global pool. Safe to use from many user threads concurrently.
    #[default]
    Sequential,
}

/// Options used when constructing a [`BaseDocument`](crate::BaseDocument)
#[derive(Default)]
pub struct DocumentConfig {
    /// The initial `Viewport`
    pub viewport: Option<Viewport>,
    /// The base url which relative URLs are resolved against
    pub base_url: Option<String>,
    /// User Agent stylesheets
    pub ua_stylesheets: Option<Vec<String>>,
    /// Net provider to handle network requests for resources
    pub net_provider: Option<Arc<dyn NetProvider>>,
    /// Navigation provider to handle link clicks and form submissions
    pub navigation_provider: Option<Arc<dyn NavigationProvider>>,
    /// Shell provider to redraw requests, clipboard, etc
    pub shell_provider: Option<Arc<dyn ShellProvider>>,
    /// HTML parser provider. Used to parse HTML for setInnerHTML
    pub html_parser_provider: Option<Arc<dyn HtmlParserProvider>>,
    /// Parley `FontContext`
    pub font_ctx: Option<FontContext>,
    /// The CSS media type used to evaluate `@media` rules.
    /// Defaults to [`MediaType::screen`].
    pub media_type: Option<MediaType>,
    /// Strategy for Stylo's style traversal.
    /// Defaults to [`StyleThreading::Parallel`].
    pub style_threading: StyleThreading,
    /// Whether incremental layout is enabled. Defaults to `true`.
    ///
    /// When disabled, every node is treated as damaged on each `resolve` and the
    /// whole tree is reconstructed and laid out from scratch through the same
    /// damage-driven pipeline used by incremental mode.
    pub incremental: Option<bool>,
    /// If set, every sub-resource `Request` blitz-dom creates for this
    /// document will carry this signal. Aborting it cancels every in-flight
    /// fetch tied to this document.
    pub abort_signal: Option<AbortSignal>,
    /// How deeply this document is nested within other documents
    /// (0 for a root document). Used to limit `<iframe>` nesting depth.
    pub subdocument_depth: usize,
}
