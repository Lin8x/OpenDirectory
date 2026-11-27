//! Shared types for OpenDirectory: the directory model, the agent wire
//! protocol and the cross-platform policy engine.
//!
//! Everything in here is plain data plus pure functions so the server, the
//! agent and the CLI agree on exactly the same shapes.

pub mod baselines;
pub mod manage;
pub mod model;
pub mod policy;
pub mod protocol;
pub mod scripts;

pub use model::*;
