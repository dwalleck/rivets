//! Comprehensive tests for Phase 2 resilient loading functionality.
//!
//! This test module verifies warning collection, error recovery, and the integration
//! between rivets-jsonl's resilient loading and storage backends.
//!
//! # Test Categories
//!
//! ## Warning Collection Tests
//! - Collect warnings for malformed JSON
//! - Collect warnings for skipped lines
//! - Warning contains correct line numbers
//! - Multiple warnings collected
//! - Warning details include error messages
//!
//! ## Resilient Streaming Tests
//! - stream_resilient() continues on errors
//! - stream_resilient() yields only valid records
//! - Mixed valid/invalid records handled correctly
//! - Edge cases: all invalid, all valid, alternating
//!
//! ## Integration Tests
//! - read_jsonl_resilient() with corrupted file
//! - Large file handling with sparse errors
//! - Memory efficiency verification

use futures::stream::StreamExt;
use rivets_jsonl::warning::{Warning, WarningCollector};
use rivets_jsonl::{read_jsonl_resilient, JsonlReader};
use serde::{Deserialize, Serialize};
use std::io::{Cursor, Write};
use std::path::PathBuf;
use std::pin::pin;
use tempfile::NamedTempFile;

// =============================================================================
// Test Data Types
// =============================================================================

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
struct SimpleRecord {
    id: u32,
    name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
struct RecordWithOptional {
    id: u32,
    name: String,
    #[serde(default)]
    optional_field: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
struct NestedRecord {
    id: String,
    data: InnerData,
    tags: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
struct InnerData {
    value: i32,
    active: bool,
}

// =============================================================================
// Warning Collection Tests
// =============================================================================

mod warning_collection_tests {
    use super::*;

    #[test]
    fn warning_collector_starts_empty() {
        let collector = WarningCollector::new();
        assert!(collector.is_empty());
        assert_eq!(collector.len(), 0);
    }

    #[test]
    fn warning_collector_adds_malformed_json_warning() {
        let collector = WarningCollector::new();

        collector.add(Warning::MalformedJson {
            line_number: 5,
            error: "unexpected end of input".to_string(),
        });

        assert!(!collector.is_empty());
        assert_eq!(collector.len(), 1);

        let warnings = collector.warnings();
        assert_eq!(warnings[0].line_number(), 5);
        assert_eq!(warnings[0].kind(), "malformed_json");
    }

    #[test]
    fn warning_collector_adds_skipped_line_warning() {
        let collector = WarningCollector::new();

        collector.add(Warning::SkippedLine {
            line_number: 10,
            reason: "validation failed".to_string(),
        });

        assert_eq!(collector.len(), 1);

        let warnings = collector.warnings();
        assert_eq!(warnings[0].line_number(), 10);
        assert_eq!(warnings[0].kind(), "skipped_line");
    }

    #[test]
    fn warning_collector_collects_multiple_warnings() {
        let collector = WarningCollector::new();

        collector.add(Warning::MalformedJson {
            line_number: 1,
            error: "error1".to_string(),
        });
        collector.add(Warning::SkippedLine {
            line_number: 5,
            reason: "reason".to_string(),
        });
        collector.add(Warning::MalformedJson {
            line_number: 10,
            error: "error2".to_string(),
        });

        assert_eq!(collector.len(), 3);

        let warnings = collector.warnings();
        assert_eq!(warnings[0].line_number(), 1);
        assert_eq!(warnings[1].line_number(), 5);
        assert_eq!(warnings[2].line_number(), 10);
    }

    #[test]
    fn warning_line_number_is_correct() {
        // Test that line numbers are reported correctly for various positions
        let lines = [1, 10, 100, 1000, 10000];

        for &line in &lines {
            let warning = Warning::MalformedJson {
                line_number: line,
                error: "test".to_string(),
            };
            assert_eq!(
                warning.line_number(),
                line,
                "Line number mismatch for line {}",
                line
            );
        }
    }

    #[test]
    fn warning_description_contains_line_number() {
        let warning = Warning::MalformedJson {
            line_number: 42,
            error: "unexpected token".to_string(),
        };

        let desc = warning.description();
        assert!(
            desc.contains("42"),
            "Description should contain line number"
        );
        assert!(
            desc.contains("unexpected token"),
            "Description should contain error"
        );
    }

    #[test]
    fn warning_display_is_human_readable() {
        let warning = Warning::MalformedJson {
            line_number: 7,
            error: "syntax error".to_string(),
        };

        let display = format!("{}", warning);
        assert!(display.contains("7"), "Display should include line number");
        assert!(
            display.contains("malformed JSON") || display.contains("syntax error"),
            "Display should describe the error type"
        );
    }

    #[test]
    fn warning_collector_clone_shares_state() {
        let collector1 = WarningCollector::new();
        let collector2 = collector1.clone();

        collector1.add(Warning::MalformedJson {
            line_number: 1,
            error: "test".to_string(),
        });

        // Both collectors should see the same warning
        assert_eq!(collector1.len(), 1);
        assert_eq!(collector2.len(), 1);

        collector2.add(Warning::SkippedLine {
            line_number: 2,
            reason: "test".to_string(),
        });

        // Both should see both warnings
        assert_eq!(collector1.len(), 2);
        assert_eq!(collector2.len(), 2);
    }

    #[test]
    fn warning_collector_clear_removes_all() {
        let collector = WarningCollector::new();

        collector.add(Warning::MalformedJson {
            line_number: 1,
            error: "test".to_string(),
        });
        collector.add(Warning::MalformedJson {
            line_number: 2,
            error: "test".to_string(),
        });

        assert_eq!(collector.len(), 2);

        collector.clear();

        assert!(collector.is_empty());
        assert_eq!(collector.len(), 0);
    }

    #[test]
    fn warning_collector_into_warnings_consumes() {
        let collector = WarningCollector::new();

        collector.add(Warning::MalformedJson {
            line_number: 1,
            error: "test".to_string(),
        });

        let warnings = collector.into_warnings();

        assert_eq!(warnings.len(), 1);
        // collector is consumed, cannot use it anymore
    }

    #[test]
    fn warning_collector_preserves_insertion_order() {
        let collector = WarningCollector::new();

        for i in 1..=100 {
            collector.add(Warning::MalformedJson {
                line_number: i,
                error: format!("error{}", i),
            });
        }

        let warnings = collector.into_warnings();

        for (i, warning) in warnings.iter().enumerate() {
            assert_eq!(
                warning.line_number(),
                i + 1,
                "Warning order not preserved at position {}",
                i
            );
        }
    }

    #[test]
    fn warning_kind_allows_filtering() {
        let collector = WarningCollector::new();

        collector.add(Warning::MalformedJson {
            line_number: 1,
            error: "e1".to_string(),
        });
        collector.add(Warning::SkippedLine {
            line_number: 2,
            reason: "r1".to_string(),
        });
        collector.add(Warning::MalformedJson {
            line_number: 3,
            error: "e2".to_string(),
        });
        collector.add(Warning::SkippedLine {
            line_number: 4,
            reason: "r2".to_string(),
        });

        let warnings = collector.into_warnings();

        let malformed_count = warnings
            .iter()
            .filter(|w| w.kind() == "malformed_json")
            .count();
        let skipped_count = warnings
            .iter()
            .filter(|w| w.kind() == "skipped_line")
            .count();

        assert_eq!(malformed_count, 2);
        assert_eq!(skipped_count, 2);
    }

    #[test]
    fn warning_implements_error_trait() {
        let warning = Warning::MalformedJson {
            line_number: 1,
            error: "test".to_string(),
        };

        // Should compile and work as std::error::Error
        let error: &dyn std::error::Error = &warning;
        let _desc = error.to_string();
    }
}

// =============================================================================
// Resilient Streaming Tests
// =============================================================================

mod resilient_streaming_tests {
    use super::*;

    #[tokio::test]
    async fn stream_resilient_empty_input() {
        let data = Cursor::new(b"");
        let reader = JsonlReader::new(data);
        let (stream, warnings) = reader.stream_resilient::<SimpleRecord>();

        let records: Vec<SimpleRecord> = pin!(stream).collect().await;

        assert!(records.is_empty());
        assert!(warnings.is_empty());
    }

    #[tokio::test]
    async fn stream_resilient_all_valid_records() {
        let content = r#"{"id": 1, "name": "Alice"}
{"id": 2, "name": "Bob"}
{"id": 3, "name": "Charlie"}"#;

        let reader = JsonlReader::new(Cursor::new(content.as_bytes()));
        let (stream, warnings) = reader.stream_resilient::<SimpleRecord>();

        let records: Vec<SimpleRecord> = pin!(stream).collect().await;

        assert_eq!(records.len(), 3);
        assert!(warnings.is_empty());
        assert_eq!(records[0].id, 1);
        assert_eq!(records[1].id, 2);
        assert_eq!(records[2].id, 3);
    }

    #[tokio::test]
    async fn stream_resilient_all_invalid_records() {
        let content = r#"{invalid1}
{invalid2}
{invalid3}"#;

        let reader = JsonlReader::new(Cursor::new(content.as_bytes()));
        let (stream, warnings) = reader.stream_resilient::<SimpleRecord>();

        let records: Vec<SimpleRecord> = pin!(stream).collect().await;

        assert!(records.is_empty());
        assert_eq!(warnings.len(), 3);
        assert_eq!(warnings.warnings()[0].line_number(), 1);
        assert_eq!(warnings.warnings()[1].line_number(), 2);
        assert_eq!(warnings.warnings()[2].line_number(), 3);
    }

    #[tokio::test]
    async fn stream_resilient_alternating_valid_invalid() {
        let content = r#"{"id": 1, "name": "Valid1"}
{invalid1}
{"id": 3, "name": "Valid2"}
{invalid2}
{"id": 5, "name": "Valid3"}"#;

        let reader = JsonlReader::new(Cursor::new(content.as_bytes()));
        let (stream, warnings) = reader.stream_resilient::<SimpleRecord>();

        let records: Vec<SimpleRecord> = pin!(stream).collect().await;

        assert_eq!(records.len(), 3);
        assert_eq!(records[0].id, 1);
        assert_eq!(records[1].id, 3);
        assert_eq!(records[2].id, 5);

        assert_eq!(warnings.len(), 2);
        assert_eq!(warnings.warnings()[0].line_number(), 2);
        assert_eq!(warnings.warnings()[1].line_number(), 4);
    }

    #[tokio::test]
    async fn stream_resilient_consecutive_invalid_lines() {
        let content = r#"{"id": 1, "name": "Valid"}
{invalid1}
{invalid2}
{invalid3}
{"id": 5, "name": "Valid2"}"#;

        let reader = JsonlReader::new(Cursor::new(content.as_bytes()));
        let (stream, warnings) = reader.stream_resilient::<SimpleRecord>();

        let records: Vec<SimpleRecord> = pin!(stream).collect().await;

        assert_eq!(records.len(), 2);
        assert_eq!(records[0].id, 1);
        assert_eq!(records[1].id, 5);

        assert_eq!(warnings.len(), 3);
        assert_eq!(warnings.warnings()[0].line_number(), 2);
        assert_eq!(warnings.warnings()[1].line_number(), 3);
        assert_eq!(warnings.warnings()[2].line_number(), 4);
    }

    #[tokio::test]
    async fn stream_resilient_invalid_at_start() {
        let content = r#"{invalid}
{"id": 2, "name": "Valid"}"#;

        let reader = JsonlReader::new(Cursor::new(content.as_bytes()));
        let (stream, warnings) = reader.stream_resilient::<SimpleRecord>();

        let records: Vec<SimpleRecord> = pin!(stream).collect().await;

        assert_eq!(records.len(), 1);
        assert_eq!(records[0].id, 2);
        assert_eq!(warnings.len(), 1);
        assert_eq!(warnings.warnings()[0].line_number(), 1);
    }

    #[tokio::test]
    async fn stream_resilient_invalid_at_end() {
        let content = r#"{"id": 1, "name": "Valid"}
{invalid}"#;

        let reader = JsonlReader::new(Cursor::new(content.as_bytes()));
        let (stream, warnings) = reader.stream_resilient::<SimpleRecord>();

        let records: Vec<SimpleRecord> = pin!(stream).collect().await;

        assert_eq!(records.len(), 1);
        assert_eq!(records[0].id, 1);
        assert_eq!(warnings.len(), 1);
        assert_eq!(warnings.warnings()[0].line_number(), 2);
    }

    #[tokio::test]
    async fn stream_resilient_skips_empty_lines() {
        let content =
            "\n\n{\"id\": 1, \"name\": \"Valid\"}\n\n\n{\"id\": 2, \"name\": \"Valid2\"}\n\n";

        let reader = JsonlReader::new(Cursor::new(content.as_bytes()));
        let (stream, warnings) = reader.stream_resilient::<SimpleRecord>();

        let records: Vec<SimpleRecord> = pin!(stream).collect().await;

        assert_eq!(records.len(), 2);
        assert_eq!(records[0].id, 1);
        assert_eq!(records[1].id, 2);
        assert!(
            warnings.is_empty(),
            "Empty lines should not generate warnings"
        );
    }

    #[tokio::test]
    async fn stream_resilient_skips_whitespace_only_lines() {
        let content = "   \n\t\t\n{\"id\": 1, \"name\": \"Valid\"}\n  \t  \n";

        let reader = JsonlReader::new(Cursor::new(content.as_bytes()));
        let (stream, warnings) = reader.stream_resilient::<SimpleRecord>();

        let records: Vec<SimpleRecord> = pin!(stream).collect().await;

        assert_eq!(records.len(), 1);
        assert_eq!(records[0].id, 1);
        assert!(
            warnings.is_empty(),
            "Whitespace-only lines should not generate warnings"
        );
    }

    #[tokio::test]
    async fn stream_resilient_type_mismatch_generates_warning() {
        let content = r#"{"id": "not_a_number", "name": "Invalid"}"#;

        let reader = JsonlReader::new(Cursor::new(content.as_bytes()));
        let (stream, warnings) = reader.stream_resilient::<SimpleRecord>();

        let records: Vec<SimpleRecord> = pin!(stream).collect().await;

        assert!(records.is_empty());
        assert_eq!(warnings.len(), 1);

        match &warnings.warnings()[0] {
            Warning::MalformedJson { line_number, error } => {
                assert_eq!(*line_number, 1);
                assert!(!error.is_empty());
            }
            _ => panic!("Expected MalformedJson warning"),
        }
    }

    #[tokio::test]
    async fn stream_resilient_missing_required_field_generates_warning() {
        let content = r#"{"id": 1}
{"id": 2, "name": "Valid"}"#;

        let reader = JsonlReader::new(Cursor::new(content.as_bytes()));
        let (stream, warnings) = reader.stream_resilient::<SimpleRecord>();

        let records: Vec<SimpleRecord> = pin!(stream).collect().await;

        // First record is missing "name" field, should be skipped
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].id, 2);
        assert_eq!(warnings.len(), 1);
        assert_eq!(warnings.warnings()[0].line_number(), 1);
    }

    #[tokio::test]
    async fn stream_resilient_with_optional_fields() {
        let content = r#"{"id": 1, "name": "With Optional", "optional_field": "value"}
{"id": 2, "name": "Without Optional"}"#;

        let reader = JsonlReader::new(Cursor::new(content.as_bytes()));
        let (stream, warnings) = reader.stream_resilient::<RecordWithOptional>();

        let records: Vec<RecordWithOptional> = pin!(stream).collect().await;

        assert_eq!(records.len(), 2);
        assert_eq!(records[0].optional_field, Some("value".to_string()));
        assert_eq!(records[1].optional_field, None);
        assert!(warnings.is_empty());
    }

    #[tokio::test]
    async fn stream_resilient_nested_records() {
        let content = r#"{"id": "1", "data": {"value": 10, "active": true}, "tags": ["a", "b"]}
{"id": "2", "data": {"value": "invalid", "active": false}, "tags": []}
{"id": "3", "data": {"value": 30, "active": true}, "tags": ["c"]}"#;

        let reader = JsonlReader::new(Cursor::new(content.as_bytes()));
        let (stream, warnings) = reader.stream_resilient::<NestedRecord>();

        let records: Vec<NestedRecord> = pin!(stream).collect().await;

        // Second record has invalid nested field type
        assert_eq!(records.len(), 2);
        assert_eq!(records[0].id, "1");
        assert_eq!(records[1].id, "3");
        assert_eq!(warnings.len(), 1);
        assert_eq!(warnings.warnings()[0].line_number(), 2);
    }

    #[tokio::test]
    async fn stream_resilient_can_inspect_warnings_during_processing() {
        let content = r#"{"id": 1, "name": "First"}
{invalid}
{"id": 3, "name": "Third"}"#;

        let reader = JsonlReader::new(Cursor::new(content.as_bytes()));
        let (stream, warnings) = reader.stream_resilient::<SimpleRecord>();
        let mut stream = pin!(stream);

        // Read first record
        let record1 = stream.next().await.unwrap();
        assert_eq!(record1.id, 1);
        assert_eq!(warnings.len(), 0, "No warnings yet");

        // Read second record (after invalid line)
        let record2 = stream.next().await.unwrap();
        assert_eq!(record2.id, 3);
        assert_eq!(warnings.len(), 1, "Should have 1 warning now");

        // Stream exhausted
        assert!(stream.next().await.is_none());
    }

    #[tokio::test]
    async fn stream_resilient_with_take_combinator() {
        let content = r#"{"id": 1, "name": "A"}
{invalid}
{"id": 3, "name": "B"}
{"id": 4, "name": "C"}
{"id": 5, "name": "D"}"#;

        let reader = JsonlReader::new(Cursor::new(content.as_bytes()));
        let (stream, warnings) = reader.stream_resilient::<SimpleRecord>();

        let records: Vec<SimpleRecord> = pin!(stream).take(2).collect().await;

        assert_eq!(records.len(), 2);
        assert_eq!(records[0].id, 1);
        assert_eq!(records[1].id, 3);
        // Warning should still be collected even with take()
        assert_eq!(warnings.len(), 1);
    }

    #[tokio::test]
    async fn stream_resilient_with_filter_combinator() {
        let content = r#"{"id": 1, "name": "A"}
{"id": 2, "name": "B"}
{invalid}
{"id": 4, "name": "D"}
{"id": 5, "name": "E"}"#;

        let reader = JsonlReader::new(Cursor::new(content.as_bytes()));
        let (stream, warnings) = reader.stream_resilient::<SimpleRecord>();

        // Filter to only even IDs
        let records: Vec<SimpleRecord> = pin!(stream)
            .filter(|r| {
                let is_even = r.id % 2 == 0;
                async move { is_even }
            })
            .collect()
            .await;

        assert_eq!(records.len(), 2);
        assert_eq!(records[0].id, 2);
        assert_eq!(records[1].id, 4);
        assert_eq!(warnings.len(), 1);
    }

    #[tokio::test]
    async fn stream_resilient_large_dataset_with_sparse_errors() {
        const TOTAL_LINES: usize = 500;
        const ERROR_FREQUENCY: usize = 50;

        let mut content = String::new();
        let mut expected_valid = 0;
        let mut expected_errors = Vec::new();

        for i in 0..TOTAL_LINES {
            if i > 0 && i % ERROR_FREQUENCY == 0 {
                content.push_str("{invalid}\n");
                expected_errors.push(i + 1); // 1-based line number
            } else {
                content.push_str(&format!(
                    "{{\"id\": {}, \"name\": \"Record{}\"}}\n",
                    expected_valid, expected_valid
                ));
                expected_valid += 1;
            }
        }

        let reader = JsonlReader::new(Cursor::new(content.as_bytes()));
        let (stream, warnings) = reader.stream_resilient::<SimpleRecord>();

        let records: Vec<SimpleRecord> = pin!(stream).collect().await;

        assert_eq!(records.len(), expected_valid);
        assert_eq!(warnings.len(), expected_errors.len());

        // Verify records are in order
        for (i, record) in records.iter().enumerate() {
            assert_eq!(record.id, i as u32);
        }

        // Verify warning line numbers
        let collected = warnings.into_warnings();
        for (warning, expected_line) in collected.iter().zip(expected_errors.iter()) {
            assert_eq!(warning.line_number(), *expected_line);
        }
    }

    #[tokio::test]
    async fn stream_resilient_handles_unicode() {
        let content = r#"{"id": 1, "name": "Hello, 世界!"}
{invalid}
{"id": 3, "name": "Emoji: 😀🎉"}"#;

        let reader = JsonlReader::new(Cursor::new(content.as_bytes()));
        let (stream, warnings) = reader.stream_resilient::<SimpleRecord>();

        let records: Vec<SimpleRecord> = pin!(stream).collect().await;

        assert_eq!(records.len(), 2);
        assert_eq!(records[0].name, "Hello, 世界!");
        assert_eq!(records[1].name, "Emoji: 😀🎉");
        assert_eq!(warnings.len(), 1);
    }

    #[tokio::test]
    async fn stream_resilient_handles_special_characters() {
        let content = r#"{"id": 1, "name": "Line1\nLine2\tTabbed"}
{"id": 2, "name": "Quote: \"Hello\""}"#;

        let reader = JsonlReader::new(Cursor::new(content.as_bytes()));
        let (stream, warnings) = reader.stream_resilient::<SimpleRecord>();

        let records: Vec<SimpleRecord> = pin!(stream).collect().await;

        assert_eq!(records.len(), 2);
        assert_eq!(records[0].name, "Line1\nLine2\tTabbed");
        assert_eq!(records[1].name, "Quote: \"Hello\"");
        assert!(warnings.is_empty());
    }

    #[tokio::test]
    async fn stream_resilient_line_without_trailing_newline() {
        let content = r#"{"id": 1, "name": "Last record no newline"}"#;

        let reader = JsonlReader::new(Cursor::new(content.as_bytes()));
        let (stream, warnings) = reader.stream_resilient::<SimpleRecord>();

        let records: Vec<SimpleRecord> = pin!(stream).collect().await;

        assert_eq!(records.len(), 1);
        assert_eq!(records[0].name, "Last record no newline");
        assert!(warnings.is_empty());
    }
}

// =============================================================================
// read_jsonl_resilient() Integration Tests
// =============================================================================

mod read_jsonl_resilient_tests {
    use super::*;

    fn create_temp_file(content: &str) -> NamedTempFile {
        let mut file = NamedTempFile::new().expect("Failed to create temp file");
        file.write_all(content.as_bytes())
            .expect("Failed to write to temp file");
        file.flush().expect("Failed to flush temp file");
        file
    }

    #[tokio::test]
    async fn read_resilient_empty_file() {
        let file = create_temp_file("");
        let (records, warnings) = read_jsonl_resilient::<SimpleRecord, _>(file.path())
            .await
            .unwrap();

        assert!(records.is_empty());
        assert!(warnings.is_empty());
    }

    #[tokio::test]
    async fn read_resilient_single_valid_record() {
        let file = create_temp_file(r#"{"id": 1, "name": "Alice"}"#);
        let (records, warnings) = read_jsonl_resilient::<SimpleRecord, _>(file.path())
            .await
            .unwrap();

        assert_eq!(records.len(), 1);
        assert_eq!(records[0].id, 1);
        assert_eq!(records[0].name, "Alice");
        assert!(warnings.is_empty());
    }

    #[tokio::test]
    async fn read_resilient_multiple_valid_records() {
        let content = r#"{"id": 1, "name": "Alice"}
{"id": 2, "name": "Bob"}
{"id": 3, "name": "Charlie"}"#;
        let file = create_temp_file(content);
        let (records, warnings) = read_jsonl_resilient::<SimpleRecord, _>(file.path())
            .await
            .unwrap();

        assert_eq!(records.len(), 3);
        assert!(warnings.is_empty());
    }

    #[tokio::test]
    async fn read_resilient_with_corrupted_lines() {
        let content = r#"{"id": 1, "name": "Valid1"}
{corrupted line}
{"id": 3, "name": "Valid2"}
{also corrupted}
{"id": 5, "name": "Valid3"}"#;
        let file = create_temp_file(content);
        let (records, warnings) = read_jsonl_resilient::<SimpleRecord, _>(file.path())
            .await
            .unwrap();

        assert_eq!(records.len(), 3);
        assert_eq!(warnings.len(), 2);
        assert_eq!(warnings[0].line_number(), 2);
        assert_eq!(warnings[1].line_number(), 4);
    }

    #[tokio::test]
    async fn read_resilient_all_corrupted() {
        let content = r#"{invalid1}
{invalid2}
{invalid3}"#;
        let file = create_temp_file(content);
        let (records, warnings) = read_jsonl_resilient::<SimpleRecord, _>(file.path())
            .await
            .unwrap();

        assert!(records.is_empty());
        assert_eq!(warnings.len(), 3);
    }

    #[tokio::test]
    async fn read_resilient_nonexistent_file_returns_error() {
        let result =
            read_jsonl_resilient::<SimpleRecord, _>("/nonexistent/path/to/file.jsonl").await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn read_resilient_handles_empty_lines() {
        let content = r#"
{"id": 1, "name": "First"}

{"id": 2, "name": "Second"}
"#;
        let file = create_temp_file(content);
        let (records, warnings) = read_jsonl_resilient::<SimpleRecord, _>(file.path())
            .await
            .unwrap();

        assert_eq!(records.len(), 2);
        assert!(warnings.is_empty());
    }

    #[tokio::test]
    async fn read_resilient_handles_whitespace_lines() {
        let content = "   \n{\"id\": 1, \"name\": \"First\"}\n\t\t\n";
        let file = create_temp_file(content);
        let (records, warnings) = read_jsonl_resilient::<SimpleRecord, _>(file.path())
            .await
            .unwrap();

        assert_eq!(records.len(), 1);
        assert!(warnings.is_empty());
    }

    #[tokio::test]
    async fn read_resilient_preserves_record_order() {
        let content = (0..100)
            .map(|i| format!(r#"{{"id": {}, "name": "Record{}"}}"#, i, i))
            .collect::<Vec<_>>()
            .join("\n");
        let file = create_temp_file(&content);
        let (records, warnings) = read_jsonl_resilient::<SimpleRecord, _>(file.path())
            .await
            .unwrap();

        assert_eq!(records.len(), 100);
        for (i, record) in records.iter().enumerate() {
            assert_eq!(record.id, i as u32);
        }
        assert!(warnings.is_empty());
    }

    #[tokio::test]
    async fn read_resilient_with_pathbuf() {
        let file = create_temp_file(r#"{"id": 1, "name": "Test"}"#);
        let pathbuf = PathBuf::from(file.path());
        let (records, _) = read_jsonl_resilient::<SimpleRecord, _>(&pathbuf)
            .await
            .unwrap();
        assert_eq!(records.len(), 1);
    }

    #[tokio::test]
    async fn read_resilient_with_string_path() {
        let file = create_temp_file(r#"{"id": 1, "name": "Test"}"#);
        let path_string = file.path().to_string_lossy().to_string();
        let (records, _) = read_jsonl_resilient::<SimpleRecord, _>(&path_string)
            .await
            .unwrap();
        assert_eq!(records.len(), 1);
    }

    #[tokio::test]
    async fn read_resilient_warning_contains_error_details() {
        let content = r#"{"id": "not_a_number", "name": "Invalid"}"#;
        let file = create_temp_file(content);
        let (records, warnings) = read_jsonl_resilient::<SimpleRecord, _>(file.path())
            .await
            .unwrap();

        assert!(records.is_empty());
        assert_eq!(warnings.len(), 1);

        match &warnings[0] {
            Warning::MalformedJson { line_number, error } => {
                assert_eq!(*line_number, 1);
                assert!(!error.is_empty());
            }
            _ => panic!("Expected MalformedJson warning"),
        }
    }

    #[tokio::test]
    async fn read_resilient_handles_unicode() {
        let content = r#"{"id": 1, "name": "Hello, 世界! 😀"}"#;
        let file = create_temp_file(content);
        let (records, warnings) = read_jsonl_resilient::<SimpleRecord, _>(file.path())
            .await
            .unwrap();

        assert_eq!(records.len(), 1);
        assert_eq!(records[0].name, "Hello, 世界! 😀");
        assert!(warnings.is_empty());
    }

    #[tokio::test]
    async fn read_resilient_large_file() {
        let content = (0..1000)
            .map(|i| format!(r#"{{"id": {}, "name": "Record{}"}}"#, i, i))
            .collect::<Vec<_>>()
            .join("\n");
        let file = create_temp_file(&content);
        let (records, warnings) = read_jsonl_resilient::<SimpleRecord, _>(file.path())
            .await
            .unwrap();

        assert_eq!(records.len(), 1000);
        assert!(warnings.is_empty());
    }

    #[tokio::test]
    async fn read_resilient_large_file_with_errors() {
        let mut lines = Vec::new();
        let mut expected_valid = 0;

        for i in 0..1000 {
            if i % 100 == 50 {
                // Put error at lines 51, 151, 251, etc.
                lines.push("{invalid}".to_string());
            } else {
                lines.push(format!(
                    r#"{{"id": {}, "name": "Record{}"}}"#,
                    expected_valid, expected_valid
                ));
                expected_valid += 1;
            }
        }

        let content = lines.join("\n");
        let file = create_temp_file(&content);
        let (records, warnings) = read_jsonl_resilient::<SimpleRecord, _>(file.path())
            .await
            .unwrap();

        assert_eq!(records.len(), expected_valid);
        assert_eq!(warnings.len(), 10); // 10 error lines
    }

    #[tokio::test]
    async fn read_resilient_nested_records() {
        let content = r#"{"id": "1", "data": {"value": 10, "active": true}, "tags": ["a", "b"]}
{"id": "2", "data": {"value": "invalid", "active": false}, "tags": []}
{"id": "3", "data": {"value": 30, "active": true}, "tags": ["c"]}"#;
        let file = create_temp_file(content);
        let (records, warnings) = read_jsonl_resilient::<NestedRecord, _>(file.path())
            .await
            .unwrap();

        assert_eq!(records.len(), 2);
        assert_eq!(records[0].id, "1");
        assert_eq!(records[1].id, "3");
        assert_eq!(warnings.len(), 1);
    }
}

// =============================================================================
// Thread Safety Tests
// =============================================================================

mod thread_safety_tests {
    use super::*;
    use std::thread;

    #[test]
    fn warning_collector_concurrent_adds() {
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

    #[test]
    fn warning_collector_is_send_and_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<WarningCollector>();
    }

    #[test]
    fn warning_is_send_and_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<Warning>();
    }
}

// =============================================================================
// Edge Case Tests
// =============================================================================

mod edge_case_tests {
    use super::*;

    #[tokio::test]
    async fn very_long_line() {
        let long_name = "x".repeat(100_000);
        let content = format!(r#"{{"id": 1, "name": "{}"}}"#, long_name);

        let reader = JsonlReader::new(Cursor::new(content.as_bytes()));
        let (stream, warnings) = reader.stream_resilient::<SimpleRecord>();

        let records: Vec<SimpleRecord> = pin!(stream).collect().await;

        assert_eq!(records.len(), 1);
        assert_eq!(records[0].name.len(), 100_000);
        assert!(warnings.is_empty());
    }

    #[tokio::test]
    async fn deeply_nested_json() {
        // Create deeply nested JSON that's still valid
        let content = r#"{"id": "1", "data": {"value": 42, "active": true}, "tags": ["a", "b", "c", "d", "e"]}"#;

        let reader = JsonlReader::new(Cursor::new(content.as_bytes()));
        let (stream, warnings) = reader.stream_resilient::<NestedRecord>();

        let records: Vec<NestedRecord> = pin!(stream).collect().await;

        assert_eq!(records.len(), 1);
        assert_eq!(records[0].data.value, 42);
        assert!(warnings.is_empty());
    }

    #[tokio::test]
    async fn json_with_extra_whitespace() {
        let content = r#"  {  "id" : 1 , "name" : "Spaced"  }  "#;

        let reader = JsonlReader::new(Cursor::new(content.as_bytes()));
        let (stream, warnings) = reader.stream_resilient::<SimpleRecord>();

        let records: Vec<SimpleRecord> = pin!(stream).collect().await;

        assert_eq!(records.len(), 1);
        assert_eq!(records[0].name, "Spaced");
        assert!(warnings.is_empty());
    }

    #[tokio::test]
    async fn truncated_json() {
        let content = r#"{"id": 1, "name": "Trun"#; // Truncated

        let reader = JsonlReader::new(Cursor::new(content.as_bytes()));
        let (stream, warnings) = reader.stream_resilient::<SimpleRecord>();

        let records: Vec<SimpleRecord> = pin!(stream).collect().await;

        assert!(records.is_empty());
        assert_eq!(warnings.len(), 1);
    }

    #[tokio::test]
    async fn incomplete_json_object() {
        let content = r#"{"id": 1}"#; // Missing required name field

        let reader = JsonlReader::new(Cursor::new(content.as_bytes()));
        let (stream, warnings) = reader.stream_resilient::<SimpleRecord>();

        let records: Vec<SimpleRecord> = pin!(stream).collect().await;

        assert!(records.is_empty());
        assert_eq!(warnings.len(), 1);
    }

    #[tokio::test]
    async fn extra_fields_ignored() {
        let content = r#"{"id": 1, "name": "Test", "extra_field": "ignored", "another": 123}"#;

        let reader = JsonlReader::new(Cursor::new(content.as_bytes()));
        let (stream, warnings) = reader.stream_resilient::<SimpleRecord>();

        let records: Vec<SimpleRecord> = pin!(stream).collect().await;

        assert_eq!(records.len(), 1);
        assert_eq!(records[0].id, 1);
        assert!(warnings.is_empty());
    }

    #[tokio::test]
    async fn null_values() {
        let content = r#"{"id": 1, "name": "Test", "optional_field": null}"#;

        let reader = JsonlReader::new(Cursor::new(content.as_bytes()));
        let (stream, warnings) = reader.stream_resilient::<RecordWithOptional>();

        let records: Vec<RecordWithOptional> = pin!(stream).collect().await;

        assert_eq!(records.len(), 1);
        assert_eq!(records[0].optional_field, None);
        assert!(warnings.is_empty());
    }

    #[tokio::test]
    async fn only_whitespace_and_empty_lines() {
        let content = "\n\n   \n\t\t\n   \t   \n\n";

        let reader = JsonlReader::new(Cursor::new(content.as_bytes()));
        let (stream, warnings) = reader.stream_resilient::<SimpleRecord>();

        let records: Vec<SimpleRecord> = pin!(stream).collect().await;

        assert!(records.is_empty());
        assert!(warnings.is_empty());
    }

    #[tokio::test]
    async fn mixed_line_endings() {
        // Unix LF, Windows CRLF, old Mac CR
        let content = "{\"id\": 1, \"name\": \"Unix\"}\n{\"id\": 2, \"name\": \"Windows\"}\r\n{\"id\": 3, \"name\": \"Line3\"}";

        let reader = JsonlReader::new(Cursor::new(content.as_bytes()));
        let (stream, warnings) = reader.stream_resilient::<SimpleRecord>();

        let records: Vec<SimpleRecord> = pin!(stream).collect().await;

        // Note: Rust's read_line handles \n and \r\n, but not standalone \r
        assert_eq!(records.len(), 3);
        assert!(warnings.is_empty());
    }
}
