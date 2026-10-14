//! Command line infrastructure shared by the wkhtmltox tools: the
//! hand-written switch parser, argument handlers, documentation outputters
//! (text, man, html) and terminal progress feedback.
//!
//! Ports upstream `src/shared/*`. The parser is deliberately not built on a
//! generic argument library: upstream's quirks (grouped short switches that
//! consume following arguments, `--0` default mode, usage newlines on stdout,
//! …) are part of the contract (spec §1.2, §2.4).

pub mod arghandler;
pub mod commandlineparserbase;
pub mod commonarguments;
pub mod commondocparts;
pub mod htmloutputter;
pub mod locale;
pub mod manoutputter;
pub mod outputter;
pub mod progressfeedback;
pub mod textoutputter;

pub use arghandler::{Action, ArgHandler, Handle, Lens, Outcome};
pub use commandlineparserbase::{CommandLineParserBase, FULL_VERSION, ParseError, app_version};
pub use outputter::{Fd, Outputter, Streams, SwitchDoc};
