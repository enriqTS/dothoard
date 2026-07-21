//! Per-screen state and logic.
//!
//! Each screen that requires its own interactive state beyond what `App`
//! provides has a module here that manages input, validation, and interaction.

pub mod automation;
pub mod ignore;
pub mod preview;
pub mod repository;
pub mod sources;
