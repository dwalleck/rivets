//! Hash-based ID generation system for rivets.
//!
//! This module implements the adaptive hash-based ID generation system from beads,
//! which creates collision-resistant IDs using SHA256 and base36 encoding.
//!
//! # Features
//!
//! - **Adaptive length**: ID length grows with database size (4-6 characters)
//! - **Collision resistant**: Uses SHA256 hashing with nonce retry
//! - **Hierarchical IDs**: Supports parent-child relationships with dot notation
//! - **Format**: `{prefix}-{hash}` (e.g., "rivets-a3f8")
//!
//! # Example
//!
//! ```
//! use rivets::id_generation::{IdGenerator, IdGeneratorConfig};
//!
//! let config = IdGeneratorConfig {
//!     prefix: "rivets".to_string(),
//!     database_size: 100,
//! };
//!
//! let mut generator = IdGenerator::new(config);
//!
//! let id = generator.generate(
//!     "My Issue Title",
//!     "Issue description",
//!     Some("alice"),
//!     None, // parent_id
//! ).unwrap();
//!
//! println!("Generated ID: {}", id); // e.g., "rivets-a3f8"
//! ```

use chrono::Utc;
use sha2::{Digest, Sha256};
use std::collections::HashSet;

const BASE36_CHARS: &[u8] = b"0123456789abcdefghijklmnopqrstuvwxyz";
const MAX_NONCE: u32 = 100;

/// Configuration for ID generation
#[derive(Debug, Clone)]
pub struct IdGeneratorConfig {
    /// Prefix for all IDs (e.g., "rivets")
    pub prefix: String,

    /// Current size of the database (affects adaptive length)
    pub database_size: usize,
}

/// Hash-based ID generator with collision detection
pub struct IdGenerator {
    config: IdGeneratorConfig,
    existing_ids: HashSet<String>,
    child_counters: std::collections::HashMap<String, u32>,
}

impl IdGenerator {
    /// Create a new ID generator with the given configuration
    pub fn new(config: IdGeneratorConfig) -> Self {
        Self {
            config,
            existing_ids: HashSet::new(),
            child_counters: std::collections::HashMap::new(),
        }
    }

    /// Register an existing ID to prevent collisions
    pub fn register_id(&mut self, id: String) {
        self.existing_ids.insert(id);
    }

    /// Generate a new unique ID
    ///
    /// # Arguments
    ///
    /// * `title` - Issue title
    /// * `description` - Issue description
    /// * `creator` - Optional creator/assignee
    /// * `parent_id` - Optional parent ID for hierarchical IDs
    ///
    /// # Errors
    ///
    /// Returns an error if unable to generate a unique ID after trying all nonces.
    pub fn generate(
        &mut self,
        title: &str,
        description: &str,
        creator: Option<&str>,
        parent_id: Option<&str>,
    ) -> Result<String, String> {
        // If parent_id is provided, generate hierarchical ID
        if let Some(parent) = parent_id {
            return self.generate_hierarchical_id(parent);
        }

        let id_length = self.adaptive_length();

        // Try generating with different nonces
        for nonce in 0..MAX_NONCE {
            let id = self.generate_hash_id(title, description, creator, nonce, id_length)?;

            if !self.existing_ids.contains(&id) {
                self.existing_ids.insert(id.clone());
                return Ok(id);
            }
        }

        // If all nonces collide, try with increased length
        if id_length < 6 {
            let longer_id = self.generate_hash_id(title, description, creator, 0, id_length + 1)?;
            self.existing_ids.insert(longer_id.clone());
            return Ok(longer_id);
        }

        Err(format!(
            "Unable to generate unique ID after {} attempts",
            MAX_NONCE
        ))
    }

    /// Generate hierarchical ID (e.g., "rivets-a3f8.1", "rivets-a3f8.1.2")
    fn generate_hierarchical_id(&mut self, parent_id: &str) -> Result<String, String> {
        let counter = self
            .child_counters
            .entry(parent_id.to_string())
            .or_insert(0);
        *counter += 1;

        let child_id = format!("{}.{}", parent_id, counter);
        self.existing_ids.insert(child_id.clone());

        Ok(child_id)
    }

    /// Generate a hash-based ID with the given parameters
    fn generate_hash_id(
        &self,
        title: &str,
        description: &str,
        creator: Option<&str>,
        nonce: u32,
        length: usize,
    ) -> Result<String, String> {
        // Combine inputs for hashing
        let timestamp = Utc::now().timestamp();
        let content = format!(
            "{}|{}|{}|{}|{}",
            title,
            description,
            creator.unwrap_or(""),
            timestamp,
            nonce
        );

        // SHA256 hash
        let mut hasher = Sha256::new();
        hasher.update(content.as_bytes());
        let hash_bytes = hasher.finalize();

        // Base36 encode to desired length
        let hash_str = encode_base36(&hash_bytes[..8], length)?;

        // Format: {prefix}-{hash}
        Ok(format!("{}-{}", self.config.prefix, hash_str))
    }

    /// Determine ID length based on database size
    ///
    /// - 0-500 issues: 4 chars
    /// - 500-1,500: 5 chars
    /// - 1,500+: 6 chars
    fn adaptive_length(&self) -> usize {
        match self.config.database_size {
            0..=500 => 4,
            501..=1500 => 5,
            _ => 6,
        }
    }
}

/// Encode bytes as base36 string
fn encode_base36(bytes: &[u8], length: usize) -> Result<String, String> {
    if length == 0 {
        return Err("Length must be greater than 0".to_string());
    }

    // Convert bytes to a large number
    let mut num: u64 = 0;
    for &byte in bytes {
        num = num.wrapping_shl(8).wrapping_add(u64::from(byte));
    }

    // Convert to base36
    let mut result = Vec::new();
    let mut n = num;

    while result.len() < length {
        let remainder = (n % 36) as usize;
        result.push(BASE36_CHARS[remainder]);
        n /= 36;
    }

    result.reverse();

    String::from_utf8(result).map_err(|e| format!("Failed to create base36 string: {}", e))
}

/// Validate ID format
///
/// Valid formats:
/// - Base: `{prefix}-{hash}` (e.g., "rivets-a3f8")
/// - Hierarchical: `{prefix}-{hash}.{child}` (e.g., "rivets-a3f8.1", "rivets-a3f8.1.2")
pub fn validate_id(id: &str, prefix: &str) -> bool {
    // Check if it starts with prefix and hyphen
    if !id.starts_with(&format!("{}-", prefix)) {
        return false;
    }

    let after_prefix = &id[prefix.len() + 1..];

    // Split on dots for hierarchical IDs
    let parts: Vec<&str> = after_prefix.split('.').collect();

    // First part must be the hash (alphanumeric, 4-6 chars)
    if parts.is_empty() {
        return false;
    }

    let hash = parts[0];
    if hash.len() < 4 || hash.len() > 6 {
        return false;
    }

    if !hash.chars().all(|c| c.is_ascii_alphanumeric()) {
        return false;
    }

    // If hierarchical, validate child indices
    for part in &parts[1..] {
        if part.parse::<u32>().is_err() {
            return false;
        }
    }

    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_base36_encoding() {
        let bytes = &[0x12, 0x34, 0x56, 0x78];
        let result = encode_base36(bytes, 4).unwrap();
        assert_eq!(result.len(), 4);
        assert!(result.chars().all(|c| c.is_ascii_alphanumeric()));
    }

    #[test]
    fn test_adaptive_length() {
        let config_small = IdGeneratorConfig {
            prefix: "test".to_string(),
            database_size: 100,
        };
        let generator_small = IdGenerator::new(config_small);
        assert_eq!(generator_small.adaptive_length(), 4);

        let config_medium = IdGeneratorConfig {
            prefix: "test".to_string(),
            database_size: 800,
        };
        let generator_medium = IdGenerator::new(config_medium);
        assert_eq!(generator_medium.adaptive_length(), 5);

        let config_large = IdGeneratorConfig {
            prefix: "test".to_string(),
            database_size: 2000,
        };
        let generator_large = IdGenerator::new(config_large);
        assert_eq!(generator_large.adaptive_length(), 6);
    }

    #[test]
    fn test_id_generation() {
        let config = IdGeneratorConfig {
            prefix: "rivets".to_string(),
            database_size: 100,
        };
        let mut generator = IdGenerator::new(config);

        let id = generator
            .generate("Test Title", "Test Description", Some("alice"), None)
            .unwrap();

        assert!(id.starts_with("rivets-"));
        assert!(validate_id(&id, "rivets"));
    }

    #[test]
    fn test_collision_handling() {
        let config = IdGeneratorConfig {
            prefix: "test".to_string(),
            database_size: 100,
        };
        let mut generator = IdGenerator::new(config);

        // Generate multiple IDs with same input - should get unique IDs
        let id1 = generator
            .generate("Same Title", "Same Description", Some("alice"), None)
            .unwrap();
        let id2 = generator
            .generate("Same Title", "Same Description", Some("alice"), None)
            .unwrap();

        // IDs should be different due to timestamp/nonce
        // Or if same timestamp, collision detection should handle it
        assert_ne!(id1, id2);
    }

    #[test]
    fn test_hierarchical_ids() {
        let config = IdGeneratorConfig {
            prefix: "rivets".to_string(),
            database_size: 100,
        };
        let mut generator = IdGenerator::new(config);

        let parent_id = generator
            .generate("Parent", "Parent issue", None, None)
            .unwrap();

        let child_id1 = generator
            .generate("Child 1", "Child description", None, Some(&parent_id))
            .unwrap();
        let child_id2 = generator
            .generate("Child 2", "Child description", None, Some(&parent_id))
            .unwrap();

        assert_eq!(child_id1, format!("{}.1", parent_id));
        assert_eq!(child_id2, format!("{}.2", parent_id));

        assert!(validate_id(&child_id1, "rivets"));
        assert!(validate_id(&child_id2, "rivets"));
    }

    #[test]
    fn test_nested_hierarchical_ids() {
        let config = IdGeneratorConfig {
            prefix: "rivets".to_string(),
            database_size: 100,
        };
        let mut generator = IdGenerator::new(config);

        let parent_id = generator.generate("Parent", "P", None, None).unwrap();
        let child_id = generator
            .generate("Child", "C", None, Some(&parent_id))
            .unwrap();
        let grandchild_id = generator
            .generate("Grandchild", "G", None, Some(&child_id))
            .unwrap();

        assert_eq!(grandchild_id, format!("{}.1", child_id));
        assert!(validate_id(&grandchild_id, "rivets"));
    }

    #[test]
    fn test_id_validation() {
        assert!(validate_id("rivets-a3f8", "rivets"));
        assert!(validate_id("rivets-abc123", "rivets"));
        assert!(validate_id("rivets-a3f8.1", "rivets"));
        assert!(validate_id("rivets-a3f8.1.2", "rivets"));

        assert!(!validate_id("invalid", "rivets"));
        assert!(!validate_id("rivets-", "rivets"));
        assert!(!validate_id("rivets-ab", "rivets")); // Too short
        assert!(!validate_id("rivets-abcdefg", "rivets")); // Too long
        assert!(!validate_id("rivets-a3f8.x", "rivets")); // Invalid child index
        assert!(!validate_id("wrong-a3f8", "rivets")); // Wrong prefix
    }

    #[test]
    fn test_register_existing_ids() {
        let config = IdGeneratorConfig {
            prefix: "test".to_string(),
            database_size: 100,
        };
        let mut generator = IdGenerator::new(config);

        // Register some existing IDs
        generator.register_id("test-a3f8".to_string());
        generator.register_id("test-b4g9".to_string());

        // Generate a new ID - should not collide with registered ones
        let new_id = generator.generate("New", "Issue", None, None).unwrap();
        assert_ne!(new_id, "test-a3f8");
        assert_ne!(new_id, "test-b4g9");
    }
}
