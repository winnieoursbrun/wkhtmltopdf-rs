//! Sub-resource plumbing between blitz-dom and the loader. Stands in for
//! QtWebKit's `QNetworkReplyHandler`: blitz-dom asks for resources through a
//! `NetProvider`; we queue every request and serve the queue ourselves, in
//! the order WebCore would issue them, through a [`Fetcher`] (the
//! wkhtmltopdf `ResourceObject`).

use std::sync::{Arc, Mutex};

use blitz_traits::net::{Bytes, NetHandler, NetProvider, Request};
use url::Url;

/// The engine's view of the network: two-step like `createRequest` +
/// `finished`, so warnings about blocked requests and errors about failed
/// replies interleave like upstream's.
pub trait Fetcher {
    /// Start a request; returns a handle.
    fn create(&mut self, url: &Url) -> usize;
    /// Complete a request: the body, or `None` when it failed.
    fn finish(&mut self, id: usize) -> Option<Vec<u8>>;
    /// `ResourceObject::warning` from the page itself (JavaScript dialogs,
    /// console messages, stopped slow scripts).
    fn warning(&mut self, _message: &str) {}
}

pub(crate) struct Queued {
    pub url: Url,
    pub handler: Box<dyn NetHandler>,
}

/// Records requests instead of performing them.
#[derive(Clone, Default)]
pub(crate) struct QueueNet(pub Arc<Mutex<Vec<Queued>>>);

impl QueueNet {
    pub fn take(&self) -> Vec<Queued> {
        std::mem::take(&mut *self.0.lock().expect("queue lock"))
    }
}

impl NetProvider for QueueNet {
    fn fetch(&self, _doc_id: usize, request: Request, handler: Box<dyn NetHandler>) {
        self.0.lock().expect("queue lock").push(Queued { url: request.url, handler });
    }
}

/// Deliver a body (or an empty one for a failure: blitz then treats the
/// resource as an empty stylesheet or an undecodable image, and stops
/// waiting for it).
pub(crate) fn deliver(q: Queued, body: Option<Vec<u8>>) {
    q.handler.bytes(q.url.to_string(), Bytes::from(body.unwrap_or_default()));
}
