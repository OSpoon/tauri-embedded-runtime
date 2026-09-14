//! Application-owned Python and Node.js runtimes.
//!
//! The renderer observes this state and requests setup through the public
//! commands below. Runtime files, service scripts, logs, and locks all live
//! below the Tauri app data directory.

mod archive;
mod artifacts;
mod cleanup;
pub mod commands;
mod download;
mod events;
mod install;
mod operation;
mod plan;
mod probe;
mod process;
mod project;
mod projects;
mod services;
mod storage;
mod types;
mod util;

#[cfg(test)]
mod tests;

pub use types::ServiceState;
