//! Warning types for non-fatal errors during JSONL processing.
//!
//! This module provides types for collecting and reporting non-fatal errors
//! that occur during JSONL reading operations, enabling resilient loading
//! that continues despite malformed data.
//!
//! # Overview
//!
//! When processing JSONL files, it's often desirable to continue reading
//! even when individual lines contain malformed JSON or other issues.
//! The [`Warning`] type represents these non-fatal errors, and the
//! [`WarningCollector`] accumulates them during processing.
//!
//! # Examples
//!
//! ```
//! use rivets_jsonl::warning::{Warning, WarningCollector};
//!
//! let collector = WarningCollector::new();
//!
//! // Simulate collecting warnings during processing
//! collector.add(Warning::MalformedJson {
//!     line_number: 5,
//!     error: "unexpected end of input".to_string(),
//! });
//!
//! collector.add(Warning::SkippedLine {
//!     line_number: 10,
//!     reason: "empty line after trim".to_string(),
//! });
//!
//! let warnings = collector.into_warnings();
//! assert_eq!(warnings.len(), 2);
//! ```

use std::sync::{Arc, Mutex};

/// A non-fatal warning that occurred during JSONL processing.
///
/// Warnings represent issues that don't prevent continued processing
/// but should be reported to the caller. Each variant includes the
/// line number where the issue occurred for debugging purposes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Warning {
    /// A line contained malformed JSON that could not be parsed.
    ///
    /// This occurs when a line is non-empty but contains invalid JSON syntax.
    /// The line is skipped and processing continues with the next line.
    MalformedJson {
        /// The 1-based line number where the error occurred.
        line_number: usize,
        /// A description of the JSON parsing error.
        error: String,
    },

    /// A line was skipped for a reason other than malformed JSON.
    ///
    /// This can occur when a line is explicitly skipped due to validation
    /// rules or other processing logic.
    SkippedLine {
        /// The 1-based line number that was skipped.
        line_number: usize,
        /// The reason the line was skipped.
        reason: String,
    },
}

impl Warning {
    /// Returns the line number associated with this warning.
    ///
    /// # Examples
    ///
    /// ```
    /// use rivets_jsonl::warning::Warning;
    ///
    /// let warning = Warning::MalformedJson {
    ///     line_number: 42,
    ///     error: "unexpected token".to_string(),
    /// };
    /// assert_eq!(warning.line_number(), 42);
    /// ```
    #[must_use]
    pub fn line_number(&self) -> usize {
        match self {
            Self::MalformedJson { line_number, .. } | Self::SkippedLine { line_number, .. } => {
                *line_number
            }
        }
    }

    /// Returns a human-readable description of the warning.
    ///
    /// # Examples
    ///
    /// ```
    /// use rivets_jsonl::warning::Warning;
    ///
    /// let warning = Warning::MalformedJson {
    ///     line_number: 5,
    ///     error: "unexpected end of input".to_string(),
    /// };
    /// let desc = warning.description();
    /// assert!(desc.contains("line 5"));
    /// assert!(desc.contains("unexpected end of input"));
    /// ```
    #[must_use]
    pub fn description(&self) -> String {
        match self {
            Self::MalformedJson { line_number, error } => {
                format!("line {}: malformed JSON: {}", line_number, error)
            }
            Self::SkippedLine {
                line_number,
                reason,
            } => {
                format!("line {}: skipped: {}", line_number, reason)
            }
        }
    }

    /// Returns a static string identifying the warning kind.
    ///
    /// This is useful for programmatic filtering and grouping of warnings
    /// without pattern matching on the enum variants.
    ///
    /// # Examples
    ///
    /// ```
    /// use rivets_jsonl::warning::Warning;
    ///
    /// let warning = Warning::MalformedJson {
    ///     line_number: 5,
    ///     error: "parse error".to_string(),
    /// };
    /// assert_eq!(warning.kind(), "malformed_json");
    ///
    /// let warning2 = Warning::SkippedLine {
    ///     line_number: 10,
    ///     reason: "empty".to_string(),
    /// };
    /// assert_eq!(warning2.kind(), "skipped_line");
    /// ```
    #[must_use]
    pub fn kind(&self) -> &'static str {
        match self {
            Self::MalformedJson { .. } => "malformed_json",
            Self::SkippedLine { .. } => "skipped_line",
        }
    }
}

impl std::fmt::Display for Warning {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.description())
    }
}

/// Implement the Error trait to allow Warning to be used in error contexts.
///
/// This enables warnings to be used with `Result` types and error handling
/// utilities that expect `std::error::Error`.
impl std::error::Error for Warning {}

/// A thread-safe collector for accumulating warnings during JSONL processing.
///
/// `WarningCollector` uses interior mutability with `Arc<Mutex<...>>` to allow
/// warnings to be added from multiple contexts while sharing the same collector.
/// It implements `Clone` to allow the collector to be shared across async
/// stream processing boundaries.
///
/// # Thread Safety
///
/// This type is `Send` and `Sync`, making it safe to use across threads.
/// The internal mutex ensures that concurrent additions are properly serialized.
///
/// # Mutex Poisoning
///
/// All methods will panic if the internal mutex is poisoned, which only occurs
/// if another thread panicked while holding the lock. In typical usage, this
/// should not occur.
///
/// # Memory Management
///
/// The collector accumulates warnings unboundedly in memory. For large files
/// with many errors, consider periodically calling [`clear()`](Self::clear)
/// to process and discard accumulated warnings, or use
/// [`warnings()`](Self::warnings) to copy and process warnings in batches
/// while continuing to collect new ones.
///
/// # Examples
///
/// ```
/// use rivets_jsonl::warning::{Warning, WarningCollector};
///
/// let collector = WarningCollector::new();
///
/// // Clone for use in a closure or async block
/// let collector_clone = collector.clone();
///
/// collector.add(Warning::MalformedJson {
///     line_number: 1,
///     error: "parse error".to_string(),
/// });
///
/// // Both references see the same warnings
/// assert_eq!(collector_clone.len(), 1);
///
/// // Extract warnings when done
/// let warnings = collector.into_warnings();
/// assert_eq!(warnings.len(), 1);
/// ```
#[derive(Debug, Clone, Default)]
pub struct WarningCollector {
    warnings: Arc<Mutex<Vec<Warning>>>,
}

impl WarningCollector {
    /// Creates a new empty `WarningCollector`.
    ///
    /// # Examples
    ///
    /// ```
    /// use rivets_jsonl::warning::WarningCollector;
    ///
    /// let collector = WarningCollector::new();
    /// assert!(collector.is_empty());
    /// ```
    #[must_use]
    pub fn new() -> Self {
        Self {
            warnings: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// Adds a warning to the collector.
    ///
    /// This method acquires the internal lock briefly to add the warning.
    /// If the lock is poisoned (a previous holder panicked), this method
    /// will panic. In typical usage, this should not occur.
    ///
    /// # Examples
    ///
    /// ```
    /// use rivets_jsonl::warning::{Warning, WarningCollector};
    ///
    /// let collector = WarningCollector::new();
    /// collector.add(Warning::SkippedLine {
    ///     line_number: 5,
    ///     reason: "test".to_string(),
    /// });
    /// assert_eq!(collector.len(), 1);
    /// ```
    pub fn add(&self, warning: Warning) {
        self.warnings
            .lock()
            .expect("warning collector mutex should not be poisoned")
            .push(warning);
    }

    /// Returns the number of warnings collected.
    ///
    /// # Examples
    ///
    /// ```
    /// use rivets_jsonl::warning::{Warning, WarningCollector};
    ///
    /// let collector = WarningCollector::new();
    /// assert_eq!(collector.len(), 0);
    ///
    /// collector.add(Warning::MalformedJson {
    ///     line_number: 1,
    ///     error: "error".to_string(),
    /// });
    /// assert_eq!(collector.len(), 1);
    /// ```
    #[must_use]
    pub fn len(&self) -> usize {
        self.warnings
            .lock()
            .expect("warning collector mutex should not be poisoned")
            .len()
    }

    /// Returns `true` if no warnings have been collected.
    ///
    /// # Examples
    ///
    /// ```
    /// use rivets_jsonl::warning::WarningCollector;
    ///
    /// let collector = WarningCollector::new();
    /// assert!(collector.is_empty());
    /// ```
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Returns a copy of all collected warnings.
    ///
    /// Unlike [`into_warnings`](Self::into_warnings), this method does not
    /// consume the collector, allowing continued use after inspection.
    ///
    /// # Examples
    ///
    /// ```
    /// use rivets_jsonl::warning::{Warning, WarningCollector};
    ///
    /// let collector = WarningCollector::new();
    /// collector.add(Warning::SkippedLine {
    ///     line_number: 1,
    ///     reason: "test".to_string(),
    /// });
    ///
    /// let warnings = collector.warnings();
    /// assert_eq!(warnings.len(), 1);
    ///
    /// // Collector can still be used
    /// collector.add(Warning::SkippedLine {
    ///     line_number: 2,
    ///     reason: "another".to_string(),
    /// });
    /// assert_eq!(collector.len(), 2);
    /// ```
    #[must_use]
    pub fn warnings(&self) -> Vec<Warning> {
        self.warnings
            .lock()
            .expect("warning collector mutex should not be poisoned")
            .clone()
    }

    /// Clears all collected warnings.
    ///
    /// # Examples
    ///
    /// ```
    /// use rivets_jsonl::warning::{Warning, WarningCollector};
    ///
    /// let collector = WarningCollector::new();
    /// collector.add(Warning::SkippedLine {
    ///     line_number: 1,
    ///     reason: "test".to_string(),
    /// });
    /// assert_eq!(collector.len(), 1);
    ///
    /// collector.clear();
    /// assert!(collector.is_empty());
    /// ```
    pub fn clear(&self) {
        self.warnings
            .lock()
            .expect("warning collector mutex should not be poisoned")
            .clear();
    }

    /// Consumes the collector and returns all collected warnings.
    ///
    /// If this is the last reference to the underlying warning storage,
    /// the warnings are moved out directly. Otherwise, they are cloned.
    ///
    /// # Examples
    ///
    /// ```
    /// use rivets_jsonl::warning::{Warning, WarningCollector};
    ///
    /// let collector = WarningCollector::new();
    /// collector.add(Warning::MalformedJson {
    ///     line_number: 1,
    ///     error: "error".to_string(),
    /// });
    /// collector.add(Warning::SkippedLine {
    ///     line_number: 2,
    ///     reason: "reason".to_string(),
    /// });
    ///
    /// let warnings = collector.into_warnings();
    /// assert_eq!(warnings.len(), 2);
    /// ```
    #[must_use]
    pub fn into_warnings(self) -> Vec<Warning> {
        Arc::try_unwrap(self.warnings)
            .map(|mutex| mutex.into_inner().expect("mutex should not be poisoned"))
            .unwrap_or_else(|arc| {
                arc.lock()
                    .expect("warning collector mutex should not be poisoned")
                    .clone()
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    mod warning_tests {
        use super::*;

        #[test]
        fn malformed_json_stores_line_number_and_error() {
            let warning = Warning::MalformedJson {
                line_number: 42,
                error: "unexpected token".to_string(),
            };

            assert_eq!(warning.line_number(), 42);

            if let Warning::MalformedJson { line_number, error } = &warning {
                assert_eq!(*line_number, 42);
                assert_eq!(error, "unexpected token");
            } else {
                panic!("Expected MalformedJson variant");
            }
        }

        #[test]
        fn skipped_line_stores_line_number_and_reason() {
            let warning = Warning::SkippedLine {
                line_number: 10,
                reason: "validation failed".to_string(),
            };

            assert_eq!(warning.line_number(), 10);

            if let Warning::SkippedLine {
                line_number,
                reason,
            } = &warning
            {
                assert_eq!(*line_number, 10);
                assert_eq!(reason, "validation failed");
            } else {
                panic!("Expected SkippedLine variant");
            }
        }

        #[test]
        fn description_formats_malformed_json() {
            let warning = Warning::MalformedJson {
                line_number: 5,
                error: "unexpected end of input".to_string(),
            };

            let desc = warning.description();
            assert!(desc.contains("line 5"));
            assert!(desc.contains("malformed JSON"));
            assert!(desc.contains("unexpected end of input"));
        }

        #[test]
        fn description_formats_skipped_line() {
            let warning = Warning::SkippedLine {
                line_number: 15,
                reason: "empty after trim".to_string(),
            };

            let desc = warning.description();
            assert!(desc.contains("line 15"));
            assert!(desc.contains("skipped"));
            assert!(desc.contains("empty after trim"));
        }

        #[test]
        fn display_matches_description() {
            let warning = Warning::MalformedJson {
                line_number: 1,
                error: "test error".to_string(),
            };

            assert_eq!(format!("{}", warning), warning.description());
        }

        #[test]
        fn warning_is_clone() {
            let warning = Warning::MalformedJson {
                line_number: 1,
                error: "error".to_string(),
            };

            let cloned = warning.clone();
            assert_eq!(warning, cloned);
        }

        #[test]
        fn warning_is_debug() {
            let warning = Warning::MalformedJson {
                line_number: 1,
                error: "error".to_string(),
            };

            let debug_str = format!("{:?}", warning);
            assert!(debug_str.contains("MalformedJson"));
            assert!(debug_str.contains("line_number"));
            assert!(debug_str.contains("error"));
        }

        #[test]
        fn warning_equality() {
            let w1 = Warning::MalformedJson {
                line_number: 1,
                error: "error".to_string(),
            };
            let w2 = Warning::MalformedJson {
                line_number: 1,
                error: "error".to_string(),
            };
            let w3 = Warning::MalformedJson {
                line_number: 2,
                error: "error".to_string(),
            };
            let w4 = Warning::SkippedLine {
                line_number: 1,
                reason: "reason".to_string(),
            };

            assert_eq!(w1, w2);
            assert_ne!(w1, w3);
            assert_ne!(w1, w4);
        }

        #[test]
        fn kind_returns_correct_variant_name() {
            let malformed = Warning::MalformedJson {
                line_number: 1,
                error: "error".to_string(),
            };
            assert_eq!(malformed.kind(), "malformed_json");

            let skipped = Warning::SkippedLine {
                line_number: 2,
                reason: "reason".to_string(),
            };
            assert_eq!(skipped.kind(), "skipped_line");
        }

        #[test]
        fn kind_enables_filtering_by_type() {
            let warnings = [
                Warning::MalformedJson {
                    line_number: 1,
                    error: "error1".to_string(),
                },
                Warning::SkippedLine {
                    line_number: 2,
                    reason: "reason1".to_string(),
                },
                Warning::MalformedJson {
                    line_number: 3,
                    error: "error2".to_string(),
                },
            ];

            let malformed_count = warnings
                .iter()
                .filter(|w| w.kind() == "malformed_json")
                .count();
            let skipped_count = warnings
                .iter()
                .filter(|w| w.kind() == "skipped_line")
                .count();

            assert_eq!(malformed_count, 2);
            assert_eq!(skipped_count, 1);
        }
    }

    mod collector_tests {
        use super::*;

        #[test]
        fn new_creates_empty_collector() {
            let collector = WarningCollector::new();
            assert!(collector.is_empty());
            assert_eq!(collector.len(), 0);
        }

        #[test]
        fn default_creates_empty_collector() {
            let collector = WarningCollector::default();
            assert!(collector.is_empty());
        }

        #[test]
        fn add_increases_count() {
            let collector = WarningCollector::new();

            collector.add(Warning::MalformedJson {
                line_number: 1,
                error: "error1".to_string(),
            });
            assert_eq!(collector.len(), 1);
            assert!(!collector.is_empty());

            collector.add(Warning::SkippedLine {
                line_number: 2,
                reason: "reason".to_string(),
            });
            assert_eq!(collector.len(), 2);

            collector.add(Warning::MalformedJson {
                line_number: 3,
                error: "error3".to_string(),
            });
            assert_eq!(collector.len(), 3);
        }

        #[test]
        fn warnings_returns_copy() {
            let collector = WarningCollector::new();

            collector.add(Warning::MalformedJson {
                line_number: 1,
                error: "error".to_string(),
            });

            let warnings1 = collector.warnings();
            let warnings2 = collector.warnings();

            assert_eq!(warnings1.len(), 1);
            assert_eq!(warnings2.len(), 1);
            assert_eq!(warnings1, warnings2);

            // Original collector still works
            collector.add(Warning::SkippedLine {
                line_number: 2,
                reason: "reason".to_string(),
            });
            assert_eq!(collector.len(), 2);
        }

        #[test]
        fn clear_removes_all_warnings() {
            let collector = WarningCollector::new();

            collector.add(Warning::MalformedJson {
                line_number: 1,
                error: "error".to_string(),
            });
            collector.add(Warning::SkippedLine {
                line_number: 2,
                reason: "reason".to_string(),
            });

            assert_eq!(collector.len(), 2);

            collector.clear();

            assert!(collector.is_empty());
            assert_eq!(collector.len(), 0);
        }

        #[test]
        fn into_warnings_consumes_and_returns() {
            let collector = WarningCollector::new();

            collector.add(Warning::MalformedJson {
                line_number: 1,
                error: "error1".to_string(),
            });
            collector.add(Warning::SkippedLine {
                line_number: 2,
                reason: "reason".to_string(),
            });

            let warnings = collector.into_warnings();

            assert_eq!(warnings.len(), 2);
            assert_eq!(
                warnings[0],
                Warning::MalformedJson {
                    line_number: 1,
                    error: "error1".to_string(),
                }
            );
            assert_eq!(
                warnings[1],
                Warning::SkippedLine {
                    line_number: 2,
                    reason: "reason".to_string(),
                }
            );
        }

        #[test]
        fn clone_shares_state() {
            let collector1 = WarningCollector::new();
            let collector2 = collector1.clone();

            collector1.add(Warning::MalformedJson {
                line_number: 1,
                error: "error".to_string(),
            });

            // Both see the same warning
            assert_eq!(collector1.len(), 1);
            assert_eq!(collector2.len(), 1);

            collector2.add(Warning::SkippedLine {
                line_number: 2,
                reason: "reason".to_string(),
            });

            // Both see both warnings
            assert_eq!(collector1.len(), 2);
            assert_eq!(collector2.len(), 2);
        }

        #[test]
        fn into_warnings_with_clones_clones_data() {
            let collector1 = WarningCollector::new();
            let collector2 = collector1.clone();

            collector1.add(Warning::MalformedJson {
                line_number: 1,
                error: "error".to_string(),
            });

            // collector2 still holds a reference, so into_warnings will clone
            let warnings = collector1.into_warnings();
            assert_eq!(warnings.len(), 1);

            // collector2 still works
            assert_eq!(collector2.len(), 1);
        }

        #[test]
        fn collector_is_debug() {
            let collector = WarningCollector::new();
            collector.add(Warning::MalformedJson {
                line_number: 1,
                error: "error".to_string(),
            });

            let debug_str = format!("{:?}", collector);
            assert!(debug_str.contains("WarningCollector"));
        }

        #[test]
        fn warnings_preserves_order() {
            let collector = WarningCollector::new();

            for i in 1..=10 {
                collector.add(Warning::MalformedJson {
                    line_number: i,
                    error: format!("error{}", i),
                });
            }

            let warnings = collector.into_warnings();

            for (i, warning) in warnings.iter().enumerate() {
                assert_eq!(warning.line_number(), i + 1);
            }
        }
    }

    mod thread_safety_tests {
        use super::*;
        use std::thread;

        #[test]
        fn collector_is_send_and_sync() {
            fn assert_send_sync<T: Send + Sync>() {}
            assert_send_sync::<WarningCollector>();
        }

        #[test]
        fn concurrent_adds() {
            let collector = WarningCollector::new();
            let mut handles = vec![];

            for i in 0..10 {
                let collector_clone = collector.clone();
                let handle = thread::spawn(move || {
                    for j in 0..100 {
                        collector_clone.add(Warning::MalformedJson {
                            line_number: i * 100 + j,
                            error: format!("error-{}-{}", i, j),
                        });
                    }
                });
                handles.push(handle);
            }

            for handle in handles {
                handle.join().unwrap();
            }

            assert_eq!(collector.len(), 1000);
        }
    }
}
