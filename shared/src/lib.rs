//! Shared contracts for the AI-native engine workspace.
//!
//! RULE: this crate is frozen after Phase 0. Sub-agents must not modify it.

pub mod components;
pub mod animation;
pub mod suggestions;
pub mod mcp_protocol;
pub mod mesh;
pub mod mocks;
pub mod scene;
