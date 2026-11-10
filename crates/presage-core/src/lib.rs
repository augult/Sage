//! Presage: an expectation-driven agent harness.
//!
//! The model states what it expects each action to do; the harness checks that mechanically and spends
//! the model's attention (and the user's money) only where reality diverged. Start with `harness.rs`
//! for the public facade and `script.rs` for the execution semantics.

pub mod agent;
pub mod anchors;
pub mod bootstrap;
pub mod config;
pub mod context;
pub mod demo;
pub mod economy;
pub mod events;
pub mod guard;
pub mod harness;
pub mod lessons;
pub mod locate;
pub mod prompt;
pub mod provider;
pub mod repair;
pub mod script;
pub mod state;
pub mod text;
pub mod tools;
pub mod util;
pub mod watches;
pub mod workspace;
