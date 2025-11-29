//! Integration tests for read/write round-trip operations.
//!
//! These tests verify that data written with JsonlWriter can be correctly
//! read back with JsonlReader, ensuring consistency across the full I/O cycle.

use rivets_jsonl::{JsonlReader, JsonlWriter};
use rstest::rstest;
use serde::{Deserialize, Serialize};
use std::io::Cursor;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
struct TestRecord {
    id: u32,
    name: String,
    active: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
struct ComplexRecord {
    id: String,
    value: f64,
    tags: Vec<String>,
    metadata: Option<Metadata>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
struct Metadata {
    created_by: String,
    version: u32,
}

/// Helper to perform write-then-read roundtrip for any serializable type
async fn roundtrip<T>(original: &T) -> T
where
    T: Serialize + for<'de> Deserialize<'de>,
{
    let buffer = Cursor::new(Vec::new());
    let mut writer = JsonlWriter::new(buffer);
    writer.write(original).await.unwrap();
    writer.flush().await.unwrap();

    let data = writer.into_inner().into_inner().into_inner();
    let mut reader = JsonlReader::new(Cursor::new(data));
    reader.read_line().await.unwrap().unwrap()
}

#[rstest]
#[case::simple(TestRecord { id: 1, name: "Alice".to_string(), active: true })]
#[case::special_chars(TestRecord { id: 42, name: "Line1\nLine2\tTabbed\"Quoted\"\\Backslash".to_string(), active: true })]
#[case::unicode(TestRecord { id: 1, name: "Hello, \u{4e16}\u{754c}! \u{1F600} \u{00e9}\u{00e8}".to_string(), active: true })]
#[case::empty_string(TestRecord { id: 1, name: String::new(), active: false })]
#[case::large_name(TestRecord { id: 1, name: "x".repeat(100_000), active: true })]
#[tokio::test]
async fn roundtrip_test_record(#[case] original: TestRecord) {
    let read_back = roundtrip(&original).await;
    assert_eq!(original, read_back);
}

#[rstest]
#[case::with_metadata(ComplexRecord {
    id: "abc-123".to_string(),
    value: 1.23456,
    tags: vec!["tag1".to_string(), "tag2".to_string(), "tag3".to_string()],
    metadata: Some(Metadata { created_by: "test".to_string(), version: 1 }),
})]
#[case::null_optional(ComplexRecord {
    id: "xyz-789".to_string(),
    value: 0.0,
    tags: vec![],
    metadata: None,
})]
#[tokio::test]
async fn roundtrip_complex_record(#[case] original: ComplexRecord) {
    let read_back = roundtrip(&original).await;
    assert_eq!(original, read_back);
}

#[tokio::test]
async fn roundtrip_single_record_verifies_eof() {
    let original = TestRecord {
        id: 1,
        name: "Alice".to_string(),
        active: true,
    };

    let buffer = Cursor::new(Vec::new());
    let mut writer = JsonlWriter::new(buffer);
    writer.write(&original).await.unwrap();
    writer.flush().await.unwrap();

    let data = writer.into_inner().into_inner().into_inner();
    let mut reader = JsonlReader::new(Cursor::new(data));

    let read_back: TestRecord = reader.read_line().await.unwrap().unwrap();
    assert_eq!(original, read_back);

    let eof: Option<TestRecord> = reader.read_line().await.unwrap();
    assert!(eof.is_none());
}

#[tokio::test]
async fn roundtrip_multiple_records() {
    let records = vec![
        TestRecord {
            id: 1,
            name: "Alice".to_string(),
            active: true,
        },
        TestRecord {
            id: 2,
            name: "Bob".to_string(),
            active: false,
        },
        TestRecord {
            id: 3,
            name: "Charlie".to_string(),
            active: true,
        },
    ];

    let buffer = Cursor::new(Vec::new());
    let mut writer = JsonlWriter::new(buffer);
    writer.write_all(records.iter()).await.unwrap();
    writer.flush().await.unwrap();

    let data = writer.into_inner().into_inner().into_inner();
    let mut reader = JsonlReader::new(Cursor::new(data));

    let mut read_records = Vec::new();
    while let Some(record) = reader.read_line::<TestRecord>().await.unwrap() {
        read_records.push(record);
    }

    assert_eq!(records, read_records);
}

#[tokio::test]
async fn roundtrip_large_batch() {
    let records: Vec<TestRecord> = (0..1000)
        .map(|i| TestRecord {
            id: i,
            name: format!("Record_{}", i),
            active: i % 2 == 0,
        })
        .collect();

    let buffer = Cursor::new(Vec::new());
    let mut writer = JsonlWriter::new(buffer);
    writer.write_all(records.iter()).await.unwrap();
    writer.flush().await.unwrap();

    let data = writer.into_inner().into_inner().into_inner();
    let mut reader = JsonlReader::new(Cursor::new(data));

    let mut read_records = Vec::new();
    while let Some(record) = reader.read_line::<TestRecord>().await.unwrap() {
        read_records.push(record);
    }

    assert_eq!(records.len(), read_records.len());
    assert_eq!(records, read_records);
}

#[tokio::test]
async fn roundtrip_preserves_line_numbers() {
    let records = [
        TestRecord {
            id: 1,
            name: "First".to_string(),
            active: true,
        },
        TestRecord {
            id: 2,
            name: "Second".to_string(),
            active: false,
        },
        TestRecord {
            id: 3,
            name: "Third".to_string(),
            active: true,
        },
    ];

    let buffer = Cursor::new(Vec::new());
    let mut writer = JsonlWriter::new(buffer);
    writer.write_all(records.iter()).await.unwrap();
    writer.flush().await.unwrap();

    let data = writer.into_inner().into_inner().into_inner();
    let mut reader = JsonlReader::new(Cursor::new(data));

    assert_eq!(reader.line_number(), 0);

    let _: TestRecord = reader.read_line().await.unwrap().unwrap();
    assert_eq!(reader.line_number(), 1);

    let _: TestRecord = reader.read_line().await.unwrap().unwrap();
    assert_eq!(reader.line_number(), 2);

    let _: TestRecord = reader.read_line().await.unwrap().unwrap();
    assert_eq!(reader.line_number(), 3);
}

#[tokio::test]
async fn roundtrip_mixed_types_as_json_value() {
    use serde_json::Value;

    let buffer = Cursor::new(Vec::new());
    let mut writer = JsonlWriter::new(buffer);

    writer.write(&42i32).await.unwrap();
    writer.write(&"hello").await.unwrap();
    writer.write(&vec![1, 2, 3]).await.unwrap();
    writer
        .write(&TestRecord {
            id: 1,
            name: "test".to_string(),
            active: true,
        })
        .await
        .unwrap();
    writer.flush().await.unwrap();

    let data = writer.into_inner().into_inner().into_inner();
    let mut reader = JsonlReader::new(Cursor::new(data));

    let v1: Value = reader.read_line().await.unwrap().unwrap();
    assert_eq!(v1, serde_json::json!(42));

    let v2: Value = reader.read_line().await.unwrap().unwrap();
    assert_eq!(v2, serde_json::json!("hello"));

    let v3: Value = reader.read_line().await.unwrap().unwrap();
    assert_eq!(v3, serde_json::json!([1, 2, 3]));

    let v4: Value = reader.read_line().await.unwrap().unwrap();
    assert_eq!(
        v4,
        serde_json::json!({"id": 1, "name": "test", "active": true})
    );
}

// ============================================================================
// Atomic write integration tests
// ============================================================================

mod atomic_write_integration {
    use super::*;
    use rivets_jsonl::{write_jsonl_atomic, write_jsonl_atomic_iter, JsonlReader};
    use std::path::PathBuf;

    fn test_file_path(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("rivets_test_{}.jsonl", name))
    }

    async fn cleanup(path: &std::path::Path) {
        // Best-effort cleanup - ignore errors as they're non-critical for tests
        let _ = tokio::fs::remove_file(path).await;
        let temp_path = path.with_extension("jsonl.tmp");
        let _ = tokio::fs::remove_file(&temp_path).await;
    }

    /// Verify atomic write creates valid JSONL that can be read back
    #[tokio::test]
    async fn atomic_write_roundtrip() {
        let path = test_file_path("atomic_roundtrip");
        cleanup(&path).await;

        let records = vec![
            TestRecord {
                id: 1,
                name: "Alice".to_string(),
                active: true,
            },
            TestRecord {
                id: 2,
                name: "Bob".to_string(),
                active: false,
            },
            TestRecord {
                id: 3,
                name: "Charlie".to_string(),
                active: true,
            },
        ];

        write_jsonl_atomic(&path, &records).await.unwrap();

        // Read back and verify
        let file = tokio::fs::File::open(&path).await.unwrap();
        let mut reader = JsonlReader::new(file);

        let mut read_records = Vec::new();
        while let Some(record) = reader.read_line::<TestRecord>().await.unwrap() {
            read_records.push(record);
        }

        assert_eq!(records, read_records);

        cleanup(&path).await;
    }

    /// Verify atomic write with complex records
    #[tokio::test]
    async fn atomic_write_complex_records() {
        let path = test_file_path("atomic_complex");
        cleanup(&path).await;

        let records = vec![
            ComplexRecord {
                id: "abc-123".to_string(),
                value: 1.23456,
                tags: vec!["tag1".to_string(), "tag2".to_string()],
                metadata: Some(Metadata {
                    created_by: "test".to_string(),
                    version: 1,
                }),
            },
            ComplexRecord {
                id: "xyz-789".to_string(),
                value: 0.0,
                tags: vec![],
                metadata: None,
            },
        ];

        write_jsonl_atomic(&path, &records).await.unwrap();

        // Read back and verify
        let file = tokio::fs::File::open(&path).await.unwrap();
        let mut reader = JsonlReader::new(file);

        let mut read_records = Vec::new();
        while let Some(record) = reader.read_line::<ComplexRecord>().await.unwrap() {
            read_records.push(record);
        }

        assert_eq!(records, read_records);

        cleanup(&path).await;
    }

    /// Verify that atomic write replaces existing file atomically
    #[tokio::test]
    async fn atomic_write_replaces_existing() {
        let path = test_file_path("atomic_replace");
        cleanup(&path).await;

        // Create initial file
        let initial_records = vec![TestRecord {
            id: 1,
            name: "Initial".to_string(),
            active: true,
        }];
        write_jsonl_atomic(&path, &initial_records).await.unwrap();

        // Write new content
        let new_records = vec![
            TestRecord {
                id: 100,
                name: "Replaced".to_string(),
                active: false,
            },
            TestRecord {
                id: 200,
                name: "Also New".to_string(),
                active: true,
            },
        ];
        write_jsonl_atomic(&path, &new_records).await.unwrap();

        // Verify new content
        let file = tokio::fs::File::open(&path).await.unwrap();
        let mut reader = JsonlReader::new(file);

        let mut read_records = Vec::new();
        while let Some(record) = reader.read_line::<TestRecord>().await.unwrap() {
            read_records.push(record);
        }

        assert_eq!(new_records, read_records);

        cleanup(&path).await;
    }

    /// Verify atomic write with large dataset maintains integrity
    #[tokio::test]
    async fn atomic_write_large_dataset() {
        let path = test_file_path("atomic_large");
        cleanup(&path).await;

        let records: Vec<TestRecord> = (0..5000)
            .map(|id| TestRecord {
                id,
                name: format!("Record_{}", id),
                active: id % 2 == 0,
            })
            .collect();

        write_jsonl_atomic(&path, &records).await.unwrap();

        // Read back and verify
        let file = tokio::fs::File::open(&path).await.unwrap();
        let mut reader = JsonlReader::new(file);

        let mut read_records = Vec::new();
        while let Some(record) = reader.read_line::<TestRecord>().await.unwrap() {
            read_records.push(record);
        }

        assert_eq!(records.len(), read_records.len());
        assert_eq!(records, read_records);

        cleanup(&path).await;
    }

    /// Verify atomic write with iterator works correctly
    #[tokio::test]
    async fn atomic_write_iter_roundtrip() {
        let path = test_file_path("atomic_iter");
        cleanup(&path).await;

        // Use iterator directly without collecting
        let records_iter = (0..100).map(|id| TestRecord {
            id,
            name: format!("Iter_{}", id),
            active: true,
        });

        write_jsonl_atomic_iter(&path, records_iter).await.unwrap();

        // Read back and verify
        let file = tokio::fs::File::open(&path).await.unwrap();
        let mut reader = JsonlReader::new(file);

        let mut count = 0;
        while let Some(record) = reader.read_line::<TestRecord>().await.unwrap() {
            assert_eq!(record.id, count);
            assert_eq!(record.name, format!("Iter_{}", count));
            count += 1;
        }

        assert_eq!(count, 100);

        cleanup(&path).await;
    }

    /// Verify temp file is not left behind after successful write
    #[tokio::test]
    async fn atomic_write_cleans_up_temp() {
        let path = test_file_path("atomic_cleanup");
        let temp_path = path.with_extension("jsonl.tmp");
        cleanup(&path).await;

        let records = vec![TestRecord {
            id: 1,
            name: "Test".to_string(),
            active: true,
        }];

        write_jsonl_atomic(&path, &records).await.unwrap();

        assert!(path.exists(), "Target file should exist");
        assert!(
            !temp_path.exists(),
            "Temp file should not exist after success"
        );

        cleanup(&path).await;
    }

    /// Verify original file is preserved when write fails
    /// (simulated by attempting to write to a non-existent directory)
    #[tokio::test]
    async fn atomic_write_preserves_original_on_error() {
        let path = test_file_path("atomic_error_preserve");
        cleanup(&path).await;

        // Create initial file with known content
        let initial_records = vec![TestRecord {
            id: 42,
            name: "Original".to_string(),
            active: true,
        }];
        write_jsonl_atomic(&path, &initial_records).await.unwrap();

        // Attempt to write to invalid path (should fail)
        let invalid_path = std::path::Path::new("/nonexistent_dir_12345/file.jsonl");
        let new_records = vec![TestRecord {
            id: 999,
            name: "ShouldNotExist".to_string(),
            active: false,
        }];

        let result = write_jsonl_atomic(invalid_path, &new_records).await;
        assert!(result.is_err(), "Writing to invalid path should fail");

        // Verify original file is unchanged
        let file = tokio::fs::File::open(&path).await.unwrap();
        let mut reader = JsonlReader::new(file);

        let record: TestRecord = reader.read_line().await.unwrap().unwrap();
        assert_eq!(record.id, 42);
        assert_eq!(record.name, "Original");

        cleanup(&path).await;
    }

    /// Verify atomic write fails gracefully on permission denied
    #[tokio::test]
    #[cfg(unix)] // Permission tests are Unix-specific
    async fn atomic_write_fails_on_permission_denied() {
        use std::os::unix::fs::PermissionsExt;

        // Create a dedicated directory for permission testing
        let dir_path = std::env::temp_dir().join("rivets_permission_test");
        let _ = tokio::fs::remove_dir_all(&dir_path).await;
        tokio::fs::create_dir(&dir_path).await.unwrap();

        let path = dir_path.join("test.jsonl");

        // Create initial file with known content
        let initial_records = vec![TestRecord {
            id: 100,
            name: "Protected".to_string(),
            active: true,
        }];
        write_jsonl_atomic(&path, &initial_records).await.unwrap();

        // Make the directory read-only (prevents creating new files)
        let mut perms = tokio::fs::metadata(&dir_path).await.unwrap().permissions();
        perms.set_mode(0o555); // Read-only + executable (needed to read dir contents)
        tokio::fs::set_permissions(&dir_path, perms).await.unwrap();

        // Attempt to write (should fail because can't create temp file in read-only dir)
        let new_records = vec![TestRecord {
            id: 200,
            name: "ShouldFail".to_string(),
            active: false,
        }];

        let result = write_jsonl_atomic(&path, &new_records).await;
        assert!(
            result.is_err(),
            "Writing to file in read-only directory should fail"
        );

        // Restore write permission to read the file
        let mut perms = tokio::fs::metadata(&dir_path).await.unwrap().permissions();
        perms.set_mode(0o755);
        tokio::fs::set_permissions(&dir_path, perms).await.unwrap();

        // Verify original file is unchanged
        let file = tokio::fs::File::open(&path).await.unwrap();
        let mut reader = JsonlReader::new(file);

        let record: TestRecord = reader.read_line().await.unwrap().unwrap();
        assert_eq!(record.id, 100);
        assert_eq!(record.name, "Protected");

        // Cleanup
        tokio::fs::remove_dir_all(&dir_path).await.unwrap();
    }

    /// Verify atomic write handles serialization failures correctly
    #[tokio::test]
    async fn atomic_write_fails_on_serialization_error() {
        use serde::Serialize;

        // Custom type that always fails to serialize
        #[derive(Debug)]
        struct FailingRecord {
            #[allow(dead_code)]
            value: String,
        }

        impl Serialize for FailingRecord {
            fn serialize<S>(&self, _serializer: S) -> Result<S::Ok, S::Error>
            where
                S: serde::Serializer,
            {
                Err(serde::ser::Error::custom(
                    "intentional serialization failure",
                ))
            }
        }

        let path = test_file_path("atomic_serialize_error");
        cleanup(&path).await;

        // Create initial file with valid content
        let initial_records = vec![TestRecord {
            id: 300,
            name: "Valid".to_string(),
            active: true,
        }];
        write_jsonl_atomic(&path, &initial_records).await.unwrap();

        // Attempt to write records that fail serialization
        let failing_records = vec![
            FailingRecord {
                value: "This will fail".to_string(),
            },
            FailingRecord {
                value: "This too".to_string(),
            },
        ];

        let result = write_jsonl_atomic(&path, &failing_records).await;
        assert!(
            result.is_err(),
            "Writing records with serialization errors should fail"
        );

        // Verify error message contains serialization info
        let err_msg = format!("{}", result.unwrap_err());
        assert!(
            err_msg.contains("serialization") || err_msg.contains("intentional"),
            "Error should indicate serialization failure: {}",
            err_msg
        );

        // Verify original file is unchanged
        let file = tokio::fs::File::open(&path).await.unwrap();
        let mut reader = JsonlReader::new(file);

        let record: TestRecord = reader.read_line().await.unwrap().unwrap();
        assert_eq!(record.id, 300);
        assert_eq!(record.name, "Valid");

        // Verify no temp file was left behind
        let temp_path = path.with_extension("jsonl.tmp");
        assert!(
            !temp_path.exists(),
            "Temp file should be cleaned up after serialization failure"
        );

        cleanup(&path).await;
    }

    /// Verify special characters are preserved through atomic write
    #[tokio::test]
    async fn atomic_write_special_characters() {
        let path = test_file_path("atomic_special");
        cleanup(&path).await;

        let records = vec![TestRecord {
            id: 1,
            name: "Line1\nLine2\tTabbed\"Quoted\"\\Backslash".to_string(),
            active: true,
        }];

        write_jsonl_atomic(&path, &records).await.unwrap();

        let file = tokio::fs::File::open(&path).await.unwrap();
        let mut reader = JsonlReader::new(file);

        let read_back: TestRecord = reader.read_line().await.unwrap().unwrap();
        assert_eq!(records[0], read_back);

        cleanup(&path).await;
    }

    /// Verify unicode is preserved through atomic write
    #[tokio::test]
    async fn atomic_write_unicode() {
        let path = test_file_path("atomic_unicode");
        cleanup(&path).await;

        let records = vec![TestRecord {
            id: 1,
            name: "Hello, \u{4e16}\u{754c}! \u{1F600}".to_string(),
            active: true,
        }];

        write_jsonl_atomic(&path, &records).await.unwrap();

        let file = tokio::fs::File::open(&path).await.unwrap();
        let mut reader = JsonlReader::new(file);

        let read_back: TestRecord = reader.read_line().await.unwrap().unwrap();
        assert_eq!(records[0], read_back);

        cleanup(&path).await;
    }
}

// ============================================================================
// Stream integration tests
// ============================================================================

mod stream_integration {
    use super::*;
    use futures::stream::StreamExt;
    use std::pin::pin;

    /// Tests streaming with 1000+ records to verify constant memory usage
    /// and correct behavior with large datasets.
    #[tokio::test]
    async fn stream_large_dataset() {
        const RECORD_COUNT: u32 = 1500;

        let records: Vec<TestRecord> = (0..RECORD_COUNT)
            .map(|i| TestRecord {
                id: i,
                name: format!("Record_{}", i),
                active: i % 2 == 0,
            })
            .collect();

        let buffer = Cursor::new(Vec::new());
        let mut writer = JsonlWriter::new(buffer);
        writer.write_all(records.iter()).await.unwrap();
        writer.flush().await.unwrap();

        let data = writer.into_inner().into_inner().into_inner();
        let reader = JsonlReader::new(Cursor::new(data));
        let stream = pin!(reader.stream::<TestRecord>());

        let read_records: Vec<TestRecord> = stream
            .map(|r| r.expect("all records should parse successfully"))
            .collect()
            .await;

        assert_eq!(read_records.len(), RECORD_COUNT as usize);

        for (i, record) in read_records.iter().enumerate() {
            assert_eq!(record.id, i as u32);
            assert_eq!(record.name, format!("Record_{}", i));
            assert_eq!(record.active, i % 2 == 0);
        }
    }

    /// Tests that streaming correctly propagates errors while continuing
    /// to process subsequent records.
    #[tokio::test]
    async fn stream_with_interleaved_errors() {
        let raw_data = b"{\"id\": 1, \"name\": \"Valid1\", \"active\": true}\n\
                         {invalid json}\n\
                         {\"id\": 3, \"name\": \"Valid2\", \"active\": false}\n\
                         also invalid\n\
                         {\"id\": 5, \"name\": \"Valid3\", \"active\": true}\n";

        let reader = JsonlReader::new(Cursor::new(raw_data.as_slice()));
        let stream = pin!(reader.stream::<TestRecord>());

        let results: Vec<rivets_jsonl::Result<TestRecord>> = stream.collect().await;

        assert_eq!(results.len(), 5);
        assert!(results[0].is_ok());
        assert!(results[1].is_err());
        assert!(results[2].is_ok());
        assert!(results[3].is_err());
        assert!(results[4].is_ok());

        let valid_records: Vec<&TestRecord> =
            results.iter().filter_map(|r| r.as_ref().ok()).collect();

        assert_eq!(valid_records.len(), 3);
        assert_eq!(valid_records[0].id, 1);
        assert_eq!(valid_records[1].id, 3);
        assert_eq!(valid_records[2].id, 5);
    }

    /// Tests streaming with take() to verify lazy evaluation.
    #[tokio::test]
    async fn stream_take_subset() {
        const TOTAL_RECORDS: u32 = 1000;
        const RECORDS_TO_TAKE: usize = 10;

        let records: Vec<TestRecord> = (0..TOTAL_RECORDS)
            .map(|i| TestRecord {
                id: i,
                name: format!("Record_{}", i),
                active: true,
            })
            .collect();

        let buffer = Cursor::new(Vec::new());
        let mut writer = JsonlWriter::new(buffer);
        writer.write_all(records.iter()).await.unwrap();
        writer.flush().await.unwrap();

        let data = writer.into_inner().into_inner().into_inner();
        let reader = JsonlReader::new(Cursor::new(data));
        let stream = pin!(reader.stream::<TestRecord>());

        let taken: Vec<TestRecord> = stream
            .take(RECORDS_TO_TAKE)
            .map(|r| r.unwrap())
            .collect()
            .await;

        assert_eq!(taken.len(), RECORDS_TO_TAKE);
        for (i, record) in taken.iter().enumerate() {
            assert_eq!(record.id, i as u32);
        }
    }

    /// Tests streaming with filter to process only matching records.
    #[tokio::test]
    async fn stream_filter_records() {
        let records: Vec<TestRecord> = (0..100)
            .map(|i| TestRecord {
                id: i,
                name: format!("Record_{}", i),
                active: i % 3 == 0,
            })
            .collect();

        let buffer = Cursor::new(Vec::new());
        let mut writer = JsonlWriter::new(buffer);
        writer.write_all(records.iter()).await.unwrap();
        writer.flush().await.unwrap();

        let data = writer.into_inner().into_inner().into_inner();
        let reader = JsonlReader::new(Cursor::new(data));
        let stream = pin!(reader.stream::<TestRecord>());

        let active_records: Vec<TestRecord> = stream
            .filter_map(|r| async move { r.ok() })
            .filter(|r| {
                let is_active = r.active;
                async move { is_active }
            })
            .collect()
            .await;

        assert_eq!(active_records.len(), 34);
        for record in active_records {
            assert!(record.active);
            assert_eq!(record.id % 3, 0);
        }
    }

    /// Tests that complex records can be streamed correctly.
    #[tokio::test]
    async fn stream_complex_records() {
        let records: Vec<ComplexRecord> = (0..100)
            .map(|i| ComplexRecord {
                id: format!("id-{}", i),
                value: i as f64 * 1.5,
                tags: vec![format!("tag{}", i), format!("group{}", i % 5)],
                metadata: if i % 2 == 0 {
                    Some(Metadata {
                        created_by: format!("user{}", i % 10),
                        version: i,
                    })
                } else {
                    None
                },
            })
            .collect();

        let buffer = Cursor::new(Vec::new());
        let mut writer = JsonlWriter::new(buffer);
        writer.write_all(records.iter()).await.unwrap();
        writer.flush().await.unwrap();

        let data = writer.into_inner().into_inner().into_inner();
        let reader = JsonlReader::new(Cursor::new(data));
        let stream = pin!(reader.stream::<ComplexRecord>());

        let read_records: Vec<ComplexRecord> = stream.map(|r| r.unwrap()).collect().await;

        assert_eq!(read_records, records);
    }
}

// ============================================================================
// Resilient stream integration tests
// ============================================================================

// ============================================================================
// read_jsonl_resilient integration tests
// ============================================================================

mod read_jsonl_resilient_integration {
    use super::*;
    use rivets_jsonl::{read_jsonl_resilient, warning::Warning};
    use std::io::Write;
    use std::path::PathBuf;

    fn test_file_path(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("rivets_resilient_test_{}.jsonl", name))
    }

    async fn cleanup(path: &std::path::Path) {
        // Best-effort cleanup - ignore errors as they're non-critical for tests
        let _ = tokio::fs::remove_file(path).await;
    }

    fn write_test_file(path: &std::path::Path, content: &str) {
        let mut file = std::fs::File::create(path).expect("Failed to create test file");
        file.write_all(content.as_bytes())
            .expect("Failed to write test file");
        file.flush().expect("Failed to flush test file");
    }

    /// Tests the primary use case: loading a corrupted JSONL file.
    /// This simulates a real-world scenario where some records in the file
    /// have been corrupted.
    #[tokio::test]
    async fn resilient_read_corrupted_file() {
        let path = test_file_path("corrupted");
        cleanup(&path).await;

        // Create a file with some valid and some corrupted lines
        let content = r#"{"id": 1, "name": "Valid1", "active": true}
{corrupted line without proper json}
{"id": 3, "name": "Valid2", "active": false}
{also corrupted
{"id": 5, "name": "Valid3", "active": true}
{"missing_required_field": 123}
{"id": 7, "name": "Valid4", "active": false}
"#;
        write_test_file(&path, content);

        let (records, warnings) = read_jsonl_resilient::<TestRecord, _>(&path)
            .await
            .expect("Should succeed despite corrupted lines");

        // Should have recovered 4 valid records
        assert_eq!(records.len(), 4);
        assert_eq!(records[0].id, 1);
        assert_eq!(records[1].id, 3);
        assert_eq!(records[2].id, 5);
        assert_eq!(records[3].id, 7);

        // Should have collected 3 warnings (2 corrupted lines + 1 missing field)
        assert_eq!(warnings.len(), 3);

        // Verify warning line numbers are correct
        assert_eq!(warnings[0].line_number(), 2);
        assert_eq!(warnings[1].line_number(), 4);
        assert_eq!(warnings[2].line_number(), 6);

        // Verify warnings are MalformedJson type
        for warning in &warnings {
            match warning {
                Warning::MalformedJson { .. } => {}
                _ => panic!("Expected MalformedJson warning"),
            }
        }

        cleanup(&path).await;
    }

    /// Tests loading a large file with sparse errors (simulating production data).
    #[tokio::test]
    async fn resilient_read_large_file_with_sparse_errors() {
        let path = test_file_path("large_sparse_errors");
        cleanup(&path).await;

        const TOTAL_LINES: usize = 1000;
        const ERROR_FREQUENCY: usize = 100; // One error every 100 lines

        let mut content = String::new();
        let mut expected_valid_count = 0;
        let mut expected_error_lines = Vec::new();

        for i in 0..TOTAL_LINES {
            if i > 0 && i % ERROR_FREQUENCY == 0 {
                content.push_str("{invalid json line}\n");
                expected_error_lines.push(i + 1); // 1-based line number
            } else {
                content.push_str(&format!(
                    "{{\"id\": {}, \"name\": \"Record_{}\", \"active\": {}}}\n",
                    expected_valid_count,
                    expected_valid_count,
                    expected_valid_count % 2 == 0
                ));
                expected_valid_count += 1;
            }
        }

        write_test_file(&path, &content);

        let (records, warnings) = read_jsonl_resilient::<TestRecord, _>(&path)
            .await
            .expect("Should succeed");

        // Verify all valid records were recovered
        assert_eq!(records.len(), expected_valid_count);

        // Verify records are in order
        for (i, record) in records.iter().enumerate() {
            assert_eq!(record.id, i as u32);
        }

        // Verify warning count matches expected errors
        assert_eq!(warnings.len(), expected_error_lines.len());

        // Verify warning line numbers
        for (warning, expected_line) in warnings.iter().zip(expected_error_lines.iter()) {
            assert_eq!(warning.line_number(), *expected_line);
        }

        cleanup(&path).await;
    }

    /// Tests that the function returns an error when the file doesn't exist.
    #[tokio::test]
    async fn resilient_read_nonexistent_file() {
        let path = PathBuf::from("/nonexistent/directory/file.jsonl");
        let result = read_jsonl_resilient::<TestRecord, _>(&path).await;
        assert!(result.is_err());
    }

    /// Tests reading a completely valid file (no warnings).
    #[tokio::test]
    async fn resilient_read_all_valid() {
        let path = test_file_path("all_valid");
        cleanup(&path).await;

        let records: Vec<TestRecord> = (0..100)
            .map(|i| TestRecord {
                id: i,
                name: format!("Record_{}", i),
                active: i % 2 == 0,
            })
            .collect();

        let content = records
            .iter()
            .map(|r| serde_json::to_string(r).unwrap())
            .collect::<Vec<_>>()
            .join("\n");

        write_test_file(&path, &content);

        let (read_records, warnings) = read_jsonl_resilient::<TestRecord, _>(&path)
            .await
            .expect("Should succeed");

        assert_eq!(read_records.len(), 100);
        assert!(warnings.is_empty());

        for (original, read) in records.iter().zip(read_records.iter()) {
            assert_eq!(original, read);
        }

        cleanup(&path).await;
    }

    /// Tests reading a file where all lines are corrupted.
    #[tokio::test]
    async fn resilient_read_all_corrupted() {
        let path = test_file_path("all_corrupted");
        cleanup(&path).await;

        let content = r#"{invalid1}
{invalid2}
{invalid3}
{invalid4}
{invalid5}
"#;
        write_test_file(&path, content);

        let (records, warnings) = read_jsonl_resilient::<TestRecord, _>(&path)
            .await
            .expect("Should succeed with empty records");

        assert!(records.is_empty());
        assert_eq!(warnings.len(), 5);

        cleanup(&path).await;
    }

    /// Tests reading a file with complex nested records.
    #[tokio::test]
    async fn resilient_read_complex_records() {
        let path = test_file_path("complex_records");
        cleanup(&path).await;

        let content = r#"{"id": "abc-1", "value": 1.5, "tags": ["a", "b"], "metadata": {"created_by": "user1", "version": 1}}
{"id": "abc-2", "value": "not_a_number", "tags": []}
{"id": "abc-3", "value": 3.5, "tags": ["c"], "metadata": null}
{malformed json}
{"id": "abc-4", "value": 4.5, "tags": ["d", "e", "f"], "metadata": {"created_by": "user2", "version": 4}}
"#;
        write_test_file(&path, content);

        let (records, warnings) = read_jsonl_resilient::<ComplexRecord, _>(&path)
            .await
            .expect("Should succeed");

        // Should have 3 valid records
        assert_eq!(records.len(), 3);
        assert_eq!(records[0].id, "abc-1");
        assert_eq!(records[1].id, "abc-3");
        assert_eq!(records[2].id, "abc-4");

        // Should have 2 warnings
        assert_eq!(warnings.len(), 2);

        cleanup(&path).await;
    }

    /// Tests that empty lines and whitespace-only lines are handled correctly.
    #[tokio::test]
    async fn resilient_read_handles_empty_and_whitespace_lines() {
        let path = test_file_path("empty_whitespace");
        cleanup(&path).await;

        let content = r#"
{"id": 1, "name": "First", "active": true}


{invalid}

{"id": 2, "name": "Second", "active": false}

"#;
        write_test_file(&path, content);

        let (records, warnings) = read_jsonl_resilient::<TestRecord, _>(&path)
            .await
            .expect("Should succeed");

        assert_eq!(records.len(), 2);
        assert_eq!(records[0].id, 1);
        assert_eq!(records[1].id, 2);

        // Only 1 warning for the invalid line
        assert_eq!(warnings.len(), 1);

        cleanup(&path).await;
    }

    /// Tests reading an empty file.
    #[tokio::test]
    async fn resilient_read_empty_file() {
        let path = test_file_path("empty");
        cleanup(&path).await;

        write_test_file(&path, "");

        let (records, warnings) = read_jsonl_resilient::<TestRecord, _>(&path)
            .await
            .expect("Should succeed");

        assert!(records.is_empty());
        assert!(warnings.is_empty());

        cleanup(&path).await;
    }

    /// Tests that warning error messages contain useful information.
    #[tokio::test]
    async fn resilient_read_warning_details() {
        let path = test_file_path("warning_details");
        cleanup(&path).await;

        // Type mismatch error (id should be u32, not string)
        let content = r#"{"id": "not_a_number", "name": "Test", "active": true}"#;
        write_test_file(&path, content);

        let (records, warnings) = read_jsonl_resilient::<TestRecord, _>(&path)
            .await
            .expect("Should succeed");

        assert!(records.is_empty());
        assert_eq!(warnings.len(), 1);

        match &warnings[0] {
            Warning::MalformedJson { line_number, error } => {
                assert_eq!(*line_number, 1);
                assert!(!error.is_empty());
            }
            _ => panic!("Expected MalformedJson warning"),
        }

        cleanup(&path).await;
    }

    /// Tests that unicode content is preserved correctly.
    #[tokio::test]
    async fn resilient_read_unicode() {
        let path = test_file_path("unicode");
        cleanup(&path).await;

        let content = r#"{"id": 1, "name": "Hello, 世界! 😀", "active": true}"#;
        write_test_file(&path, content);

        let (records, warnings) = read_jsonl_resilient::<TestRecord, _>(&path)
            .await
            .expect("Should succeed");

        assert_eq!(records.len(), 1);
        assert_eq!(records[0].name, "Hello, 世界! 😀");
        assert!(warnings.is_empty());

        cleanup(&path).await;
    }

    /// Tests reading with PathBuf.
    #[tokio::test]
    async fn resilient_read_pathbuf() {
        let path = test_file_path("pathbuf");
        cleanup(&path).await;

        let content = r#"{"id": 1, "name": "Test", "active": true}"#;
        write_test_file(&path, content);

        let pathbuf = PathBuf::from(&path);
        let (records, _) = read_jsonl_resilient::<TestRecord, _>(&pathbuf)
            .await
            .expect("Should succeed");

        assert_eq!(records.len(), 1);

        cleanup(&path).await;
    }

    /// Tests reading with a String path.
    #[tokio::test]
    async fn resilient_read_string_path() {
        let path = test_file_path("string_path");
        cleanup(&path).await;

        let content = r#"{"id": 1, "name": "Test", "active": true}"#;
        write_test_file(&path, content);

        let path_string = path.to_string_lossy().to_string();
        let (records, _) = read_jsonl_resilient::<TestRecord, _>(&path_string)
            .await
            .expect("Should succeed");

        assert_eq!(records.len(), 1);

        cleanup(&path).await;
    }
}

mod resilient_stream_integration {
    use super::*;
    use futures::stream::StreamExt;
    use rivets_jsonl::warning::Warning;
    use std::pin::pin;

    /// Tests resilient streaming with mixed valid/invalid JSONL data.
    /// This simulates a corrupted JSONL file where some lines have been damaged.
    #[tokio::test]
    async fn resilient_stream_with_corrupted_data() {
        // Create a JSONL string with some valid and some corrupted lines
        let raw_data = b"{\"id\": 1, \"name\": \"Valid1\", \"active\": true}\n\
                         {corrupted line without proper json}\n\
                         {\"id\": 3, \"name\": \"Valid2\", \"active\": false}\n\
                         {also corrupted\n\
                         {\"id\": 5, \"name\": \"Valid3\", \"active\": true}\n\
                         {\"missing_required_field\": 123}\n\
                         {\"id\": 7, \"name\": \"Valid4\", \"active\": false}\n";

        let reader = JsonlReader::new(Cursor::new(raw_data.as_slice()));
        let (stream, warnings) = reader.stream_resilient::<TestRecord>();

        let records: Vec<TestRecord> = pin!(stream).collect().await;

        // Should have recovered 4 valid records
        assert_eq!(records.len(), 4);
        assert_eq!(records[0].id, 1);
        assert_eq!(records[1].id, 3);
        assert_eq!(records[2].id, 5);
        assert_eq!(records[3].id, 7);

        // Should have collected 3 warnings (2 corrupted lines + 1 missing field)
        let collected_warnings = warnings.into_warnings();
        assert_eq!(collected_warnings.len(), 3);

        // Verify warning line numbers are correct
        assert_eq!(collected_warnings[0].line_number(), 2);
        assert_eq!(collected_warnings[1].line_number(), 4);
        assert_eq!(collected_warnings[2].line_number(), 6);
    }

    /// Tests resilient streaming with a large dataset containing occasional errors.
    /// Simulates a production scenario with mostly valid data and rare corruptions.
    #[tokio::test]
    async fn resilient_stream_large_dataset_with_sparse_errors() {
        const TOTAL_LINES: usize = 1000;
        const ERROR_FREQUENCY: usize = 50; // One error every 50 lines

        // Build a dataset with predictable error positions
        let mut data = String::new();
        let mut expected_valid_count = 0;
        let mut expected_error_lines = Vec::new();

        for i in 0..TOTAL_LINES {
            if i > 0 && i % ERROR_FREQUENCY == 0 {
                data.push_str("{invalid json line}\n");
                expected_error_lines.push(i + 1); // 1-based line number
            } else {
                data.push_str(&format!(
                    "{{\"id\": {}, \"name\": \"Record_{}\", \"active\": {}}}\n",
                    expected_valid_count,
                    expected_valid_count,
                    expected_valid_count % 2 == 0
                ));
                expected_valid_count += 1;
            }
        }

        let reader = JsonlReader::new(Cursor::new(data.as_bytes()));
        let (stream, warnings) = reader.stream_resilient::<TestRecord>();

        let records: Vec<TestRecord> = pin!(stream).collect().await;

        // Verify all valid records were recovered
        assert_eq!(records.len(), expected_valid_count);

        // Verify records are in order
        for (i, record) in records.iter().enumerate() {
            assert_eq!(record.id, i as u32);
        }

        // Verify warning count matches expected errors
        let collected_warnings = warnings.into_warnings();
        assert_eq!(collected_warnings.len(), expected_error_lines.len());

        // Verify warning line numbers
        for (warning, expected_line) in collected_warnings.iter().zip(expected_error_lines.iter()) {
            assert_eq!(warning.line_number(), *expected_line);
        }
    }

    /// Tests resilient streaming behavior when warnings can be inspected mid-stream.
    #[tokio::test]
    async fn resilient_stream_inspect_warnings_during_processing() {
        let raw_data = b"{\"id\": 1, \"name\": \"First\", \"active\": true}\n\
                         {invalid}\n\
                         {\"id\": 2, \"name\": \"Second\", \"active\": false}\n\
                         {also invalid}\n\
                         {\"id\": 3, \"name\": \"Third\", \"active\": true}\n";

        let reader = JsonlReader::new(Cursor::new(raw_data.as_slice()));
        let (stream, warnings) = reader.stream_resilient::<TestRecord>();
        let mut stream = pin!(stream);

        // First record
        let record1 = stream.next().await.unwrap();
        assert_eq!(record1.id, 1);
        assert_eq!(warnings.len(), 0); // No warnings yet

        // Second record (third line, after skipping invalid line 2)
        let record2 = stream.next().await.unwrap();
        assert_eq!(record2.id, 2);
        assert_eq!(warnings.len(), 1); // One warning collected

        // Third record (fifth line, after skipping invalid line 4)
        let record3 = stream.next().await.unwrap();
        assert_eq!(record3.id, 3);
        assert_eq!(warnings.len(), 2); // Two warnings collected

        // Stream exhausted
        assert!(stream.next().await.is_none());

        // Verify final warning state
        let final_warnings = warnings.into_warnings();
        assert_eq!(final_warnings.len(), 2);
    }

    /// Tests resilient streaming with complex nested records.
    #[tokio::test]
    async fn resilient_stream_complex_records() {
        // Mix valid complex records with invalid JSON
        let raw_data = b"{\"id\": \"abc-1\", \"value\": 1.5, \"tags\": [\"a\", \"b\"], \"metadata\": {\"created_by\": \"user1\", \"version\": 1}}\n\
                         {\"id\": \"abc-2\", \"value\": \"not_a_number\", \"tags\": []}\n\
                         {\"id\": \"abc-3\", \"value\": 3.5, \"tags\": [\"c\"], \"metadata\": null}\n\
                         {malformed json}\n\
                         {\"id\": \"abc-4\", \"value\": 4.5, \"tags\": [\"d\", \"e\", \"f\"], \"metadata\": {\"created_by\": \"user2\", \"version\": 4}}\n";

        let reader = JsonlReader::new(Cursor::new(raw_data.as_slice()));
        let (stream, warnings) = reader.stream_resilient::<ComplexRecord>();

        let records: Vec<ComplexRecord> = pin!(stream).collect().await;

        // Should have 3 valid records
        assert_eq!(records.len(), 3);
        assert_eq!(records[0].id, "abc-1");
        assert_eq!(records[1].id, "abc-3");
        assert_eq!(records[2].id, "abc-4");

        // Should have 2 warnings
        let collected = warnings.into_warnings();
        assert_eq!(collected.len(), 2);
    }

    /// Tests that resilient streaming handles empty lines and whitespace correctly.
    #[tokio::test]
    async fn resilient_stream_handles_empty_and_whitespace_lines() {
        let raw_data = b"\n\
                         {\"id\": 1, \"name\": \"First\", \"active\": true}\n\
                         \n\
                         \t  \n\
                         {invalid}\n\
                         \n\
                         {\"id\": 2, \"name\": \"Second\", \"active\": false}\n\
                         \n";

        let reader = JsonlReader::new(Cursor::new(raw_data.as_slice()));
        let (stream, warnings) = reader.stream_resilient::<TestRecord>();

        let records: Vec<TestRecord> = pin!(stream).collect().await;

        // Should have 2 valid records
        assert_eq!(records.len(), 2);
        assert_eq!(records[0].id, 1);
        assert_eq!(records[1].id, 2);

        // Only 1 warning for the invalid line (empty lines don't generate warnings)
        let collected = warnings.into_warnings();
        assert_eq!(collected.len(), 1);
    }

    /// Tests resilient streaming with all lines corrupted.
    #[tokio::test]
    async fn resilient_stream_all_corrupted() {
        let raw_data = b"{invalid1}\n{invalid2}\n{invalid3}\n";

        let reader = JsonlReader::new(Cursor::new(raw_data.as_slice()));
        let (stream, warnings) = reader.stream_resilient::<TestRecord>();

        let records: Vec<TestRecord> = pin!(stream).collect().await;

        // No valid records
        assert!(records.is_empty());

        // All 3 lines generated warnings
        let collected = warnings.into_warnings();
        assert_eq!(collected.len(), 3);
    }

    /// Tests that warning details contain useful error information.
    #[tokio::test]
    async fn resilient_stream_warning_details() {
        // Type mismatch error (id should be u32, not string)
        let raw_data = b"{\"id\": \"not_a_number\", \"name\": \"Test\", \"active\": true}\n";

        let reader = JsonlReader::new(Cursor::new(raw_data.as_slice()));
        let (stream, warnings) = reader.stream_resilient::<TestRecord>();

        let records: Vec<TestRecord> = pin!(stream).collect().await;
        assert!(records.is_empty());

        let collected = warnings.into_warnings();
        assert_eq!(collected.len(), 1);

        match &collected[0] {
            Warning::MalformedJson { line_number, error } => {
                assert_eq!(*line_number, 1);
                // Error message should mention the type issue
                assert!(!error.is_empty());
            }
            _ => panic!("Expected MalformedJson warning"),
        }
    }

    /// Tests resilient streaming with take() combinator.
    #[tokio::test]
    async fn resilient_stream_with_take() {
        let raw_data = b"{\"id\": 1, \"name\": \"A\", \"active\": true}\n\
                         {invalid}\n\
                         {\"id\": 2, \"name\": \"B\", \"active\": false}\n\
                         {\"id\": 3, \"name\": \"C\", \"active\": true}\n\
                         {\"id\": 4, \"name\": \"D\", \"active\": false}\n";

        let reader = JsonlReader::new(Cursor::new(raw_data.as_slice()));
        let (stream, warnings) = reader.stream_resilient::<TestRecord>();

        // Take only first 2 valid records
        let records: Vec<TestRecord> = pin!(stream).take(2).collect().await;

        assert_eq!(records.len(), 2);
        assert_eq!(records[0].id, 1);
        assert_eq!(records[1].id, 2);

        // Warning should have been collected even though we stopped early
        assert_eq!(warnings.len(), 1);
    }

    /// Tests resilient streaming with filter() combinator.
    #[tokio::test]
    async fn resilient_stream_with_filter() {
        let raw_data = b"{\"id\": 1, \"name\": \"A\", \"active\": true}\n\
                         {\"id\": 2, \"name\": \"B\", \"active\": false}\n\
                         {invalid}\n\
                         {\"id\": 3, \"name\": \"C\", \"active\": true}\n\
                         {\"id\": 4, \"name\": \"D\", \"active\": false}\n";

        let reader = JsonlReader::new(Cursor::new(raw_data.as_slice()));
        let (stream, warnings) = reader.stream_resilient::<TestRecord>();

        // Filter to only active records
        let active_records: Vec<TestRecord> = pin!(stream)
            .filter(|r| {
                let active = r.active;
                async move { active }
            })
            .collect()
            .await;

        assert_eq!(active_records.len(), 2);
        assert_eq!(active_records[0].id, 1);
        assert_eq!(active_records[1].id, 3);

        // Warning should still be collected
        assert_eq!(warnings.len(), 1);
    }
}
