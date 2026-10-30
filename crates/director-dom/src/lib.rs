//! The document GSAP page scripts run against, shared by the recorder helper
//! process that runs the scripts and the compiler that hosts it. Neither
//! Skia nor a JavaScript engine links here, so both sides can depend on it.

mod doc;
pub mod protocol;
mod wire;

pub use doc::{compact, declarations, Doc, Placement, Prober, Saved};
pub use wire::{load, save, Tree};

/// Attribute lookup by text. `Element::attr` interns its argument, and a name
/// outside html5ever's static atoms takes string_cache's process-wide lock,
/// which serializes parallel GSAP recording.
pub trait Attr {
    fn attr_text(&self, name: &str) -> Option<&str>;
}

impl Attr for scraper::node::Element {
    fn attr_text(&self, name: &str) -> Option<&str> {
        self.attrs.iter().find(|(q, _)| q.ns.is_empty() && &*q.local == name).map(|(_, v)| &**v)
    }
}
