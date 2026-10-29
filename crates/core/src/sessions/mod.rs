//! Recording of hook events into projects, sessions and runs (Inv §3.4).

pub mod change_detector;
pub mod collector;
pub mod discovery;
pub mod event;
pub mod ingest;
pub mod maintenance;
pub mod projects;
pub mod recorder;
mod rows;

pub use event::{EventKind, HookEvent};
pub use recorder::Recorder;
