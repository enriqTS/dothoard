//! Shared backend and interface modules for the `dothoard` binary.

pub mod app;
pub mod backup;
pub mod cli;
pub mod config;
pub mod diagnostics;
pub mod git;
pub mod locking;
pub mod notification;
pub mod paths;
pub mod state;
pub mod systemd;
pub mod tui;
