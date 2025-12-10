//! Example storage implementation using file-per-issue pattern.
//!
//! This shows how to integrate rivets-format into a storage backend.

use rivets_format::*;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Storage backend that stores each issue as a separate .rivet file.
pub struct FilePerIssueStorage {
    /// Root directory (e.g., `.rivets/issues/`)
    issues_dir: PathBuf,

    /// Issue ID prefix (e.g., "rivets")
    prefix: String,

    /// In-memory cache of loaded issues
    cache: HashMap<String, RivetDocument>,

    /// Counter for generating IDs
    next_id: u32,
}

impl FilePerIssueStorage {
    /// Create a new storage backend.
    pub fn new(issues_dir: PathBuf, prefix: String) -> std::io::Result<Self> {
        // Ensure directory exists
        std::fs::create_dir_all(&issues_dir)?;

        let mut storage = Self {
            issues_dir,
            prefix,
            cache: HashMap::new(),
            next_id: 1,
        };

        // Load existing issues to find max ID
        storage.load_all()?;

        Ok(storage)
    }

    /// Load all issues from disk.
    pub fn load_all(&mut self) -> std::io::Result<()> {
        self.cache.clear();

        let entries = std::fs::read_dir(&self.issues_dir)?;

        for entry in entries {
            let entry = entry?;
            let path = entry.path();

            if path.extension().map_or(false, |ext| ext == "rivet") {
                match read_rivet_file(&path) {
                    Ok(doc) => {
                        // Update next_id based on existing IDs
                        if let Some(num) = self.extract_id_number(&doc.meta.id) {
                            self.next_id = self.next_id.max(num + 1);
                        }
                        self.cache.insert(doc.meta.id.clone(), doc);
                    }
                    Err(e) => {
                        eprintln!("Warning: failed to load {:?}: {}", path, e);
                    }
                }
            }
        }

        Ok(())
    }

    /// Generate a new issue ID.
    fn generate_id(&mut self) -> String {
        let id = format!("{}-{:04x}", self.prefix, self.next_id);
        self.next_id += 1;
        id
    }

    /// Extract numeric portion from issue ID.
    fn extract_id_number(&self, id: &str) -> Option<u32> {
        let prefix_with_dash = format!("{}-", self.prefix);
        if let Some(hex_part) = id.strip_prefix(&prefix_with_dash) {
            u32::from_str_radix(hex_part, 16).ok()
        } else {
            None
        }
    }

    /// Get the file path for an issue.
    fn issue_path(&self, id: &str) -> PathBuf {
        self.issues_dir.join(format!("{}.rivet", id))
    }

    /// Create a new issue.
    pub fn create(
        &mut self,
        title: String,
        description: String,
        priority: u8,
        labels: Vec<String>,
    ) -> std::io::Result<RivetDocument> {
        let id = self.generate_id();
        let now = chrono::Utc::now();

        let doc = RivetDocument {
            meta: IssueMeta {
                id: id.clone(),
                status: IssueStatus::Open,
                priority,
                created: now,
                updated: None,
                closed: None,
            },
            title,
            description,
            labels,
            assignees: vec![],
            dependencies: vec![],
            notes: None,
            design: None,
        };

        // Write to disk
        let path = self.issue_path(&id);
        write_rivet_file(&path, &doc)?;

        // Update cache
        self.cache.insert(id, doc.clone());

        Ok(doc)
    }

    /// Get an issue by ID.
    pub fn get(&self, id: &str) -> Option<&RivetDocument> {
        self.cache.get(id)
    }

    /// Update an issue.
    pub fn update(&mut self, id: &str, f: impl FnOnce(&mut RivetDocument)) -> std::io::Result<Option<RivetDocument>> {
        if let Some(doc) = self.cache.get_mut(id) {
            f(doc);
            doc.meta.updated = Some(chrono::Utc::now());

            // Write to disk
            let path = self.issue_path(id);
            write_rivet_file(&path, doc)?;

            Ok(Some(doc.clone()))
        } else {
            Ok(None)
        }
    }

    /// Delete an issue.
    pub fn delete(&mut self, id: &str) -> std::io::Result<bool> {
        if self.cache.remove(id).is_some() {
            let path = self.issue_path(id);
            if path.exists() {
                std::fs::remove_file(path)?;
            }
            Ok(true)
        } else {
            Ok(false)
        }
    }

    /// List all issues.
    pub fn list(&self) -> impl Iterator<Item = &RivetDocument> {
        self.cache.values()
    }

    /// List issues matching a filter.
    pub fn list_filtered<F>(&self, predicate: F) -> impl Iterator<Item = &RivetDocument>
    where
        F: Fn(&RivetDocument) -> bool,
    {
        self.cache.values().filter(move |doc| predicate(doc))
    }

    /// Get issues by status.
    pub fn by_status(&self, status: IssueStatus) -> impl Iterator<Item = &RivetDocument> {
        self.list_filtered(move |doc| doc.meta.status == status)
    }

    /// Get issues by label.
    pub fn by_label<'a>(&'a self, label: &'a str) -> impl Iterator<Item = &RivetDocument> + 'a {
        self.list_filtered(move |doc| doc.labels.iter().any(|l| l == label))
    }

    /// Get open issues sorted by priority.
    pub fn open_by_priority(&self) -> Vec<&RivetDocument> {
        let mut issues: Vec<_> = self.by_status(IssueStatus::Open).collect();
        issues.sort_by_key(|doc| doc.meta.priority);
        issues
    }
}

fn main() -> std::io::Result<()> {
    // Example usage
    let temp_dir = std::env::temp_dir().join("rivets-example");
    let mut storage = FilePerIssueStorage::new(temp_dir.clone(), "demo".to_string())?;

    // Create some issues
    let issue1 = storage.create(
        "Implement parser".to_string(),
        "Create a parser for the .rivet format.".to_string(),
        1,
        vec!["parser".to_string(), "core".to_string()],
    )?;
    println!("Created: {} - {}", issue1.meta.id, issue1.title);

    let issue2 = storage.create(
        "Add serializer".to_string(),
        "Create a serializer to write .rivet files.".to_string(),
        2,
        vec!["serializer".to_string(), "core".to_string()],
    )?;
    println!("Created: {} - {}", issue2.meta.id, issue2.title);

    // Update an issue
    storage.update(&issue1.meta.id, |doc| {
        doc.meta.status = IssueStatus::InProgress;
        doc.assignees.push("dwalleck".to_string());
    })?;

    // List open issues
    println!("\nOpen issues:");
    for issue in storage.by_status(IssueStatus::Open) {
        println!("  [{}] {} (P{})", issue.meta.id, issue.title, issue.meta.priority);
    }

    // List by label
    println!("\nIssues with 'core' label:");
    for issue in storage.by_label("core") {
        println!("  [{}] {}", issue.meta.id, issue.title);
    }

    // Show file contents
    println!("\nFile contents of {}:", issue1.meta.id);
    let path = temp_dir.join(format!("{}.rivet", issue1.meta.id));
    println!("{}", std::fs::read_to_string(&path)?);

    // Cleanup
    std::fs::remove_dir_all(&temp_dir)?;

    Ok(())
}
