//! Phase 5.4 — YoloDAO integration tests against a live SigmaChain node.
//!
//! Everything in this crate that talks to the node is gated behind the
//! `live` feature so a default `cargo check` stays cheap and CI without
//! a running node compiles without skipping anything silently.

pub mod client;
pub mod config;
pub mod error;

pub use client::NodeClient;
pub use config::TestConfig;
pub use error::{NodeError, Result};
