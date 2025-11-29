//! MCP server for rivets issue tracking.
//!
//! This crate provides an MCP (Model Context Protocol) server that exposes
//! rivets issue tracking functionality to AI assistants like Claude.
//!
//! # Architecture
//!
//! The server uses the `rmcp` crate for MCP protocol handling and directly
//! wraps the `IssueStorage` trait from the rivets crate.
//!
//! # Tools
//!
//! ## Context Management
//! - `set_context` - Set the workspace root for all operations
//! - `where_am_i` - Show current workspace context
//!
//! ## Issue Queries
//! - `ready` - Find unblocked tasks ready to work on
//! - `list` - List issues with filters
//! - `show` - Show issue details with dependencies
//! - `blocked` - Get blocked issues with their blockers
//!
//! ## Issue Modification
//! - `create` - Create a new issue
//! - `update` - Update issue fields
//! - `close` - Mark an issue as complete
//! - `dep` - Add a dependency between issues

pub mod context;
pub mod error;
pub mod models;
pub mod server;
pub mod tools;

pub use error::{Error, Result};
pub use server::RivetsMcpServer;
