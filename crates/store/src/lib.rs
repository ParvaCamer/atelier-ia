//! Persistance locale (SQLite).
//!
//! Ce crate ne connaît ni Tauri, ni HTTP, ni les LLM : il expose des
//! repositories sur les types du domaine, rien de plus.

pub mod conv;
pub mod db;
pub mod error;
pub mod repo;
pub mod seed;

pub use db::Db;
pub use error::{Result, StoreError};
