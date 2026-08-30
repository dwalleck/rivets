//! MCP server for rivets issue tracking.
//!
//! This crate provides an MCP (Model Context Protocol) server that exposes
//! rivets issue tracking functionality to AI assistants like Claude.
//!
//! # Quick Start
//!
//! Run the server:
//!
//! ```bash
//! rivets-mcp
//! ```
//!
//! Configure in Claude Code (`~/.config/claude-code/mcp.json`):
//!
//! ```json
//! {
//!   "mcpServers": {
//!     "rivets": {
//!       "command": "rivets-mcp",
//!       "args": []
//!     }
//!   }
//! }
//! ```
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
//! - `create` - Create a new Issue
//! - `update` - Update Issue fields
//! - `close` - Mark an Issue as complete
//! - `blocking_dependency_add` / `blocking_dependency_remove` - Manage Blocking Dependencies
//! - `related_add` / `related_remove` - Manage symmetric Related Associations
//! - `discovery_add` / `discovery_remove` - Manage directed Discovery Origins
//!
//! # Debugging
//!
//! Enable debug logging with the `RUST_LOG` environment variable:
//!
//! ```bash
//! RUST_LOG=debug rivets-mcp
//! ```

pub mod context;
pub mod error;
pub mod models;
pub mod server;
pub mod tools;

pub use error::{Error, Result};
pub use server::RivetsMcpServer;
