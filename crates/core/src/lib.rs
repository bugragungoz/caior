//! CAIOR core library. GUI-free so every part can be unit tested.

pub mod catalog;
pub mod detect;
#[cfg(windows)]
pub mod elevate;
pub mod fetch;
pub mod install;
pub mod plan;
pub mod session;
pub mod version;
