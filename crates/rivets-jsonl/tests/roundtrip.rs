//! Integration tests for read/write round-trip operations.
//!
//! These tests verify that data written with JsonlWriter can be correctly
//! read back with JsonlReader, ensuring consistency across the full I/O cycle.

use rivets_jsonl::{JsonlReader, JsonlWriter};
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

#[tokio::test]
async fn roundtrip_single_record() {
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
async fn roundtrip_complex_record() {
    let original = ComplexRecord {
        id: "abc-123".to_string(),
        value: 1.23456,
        tags: vec!["tag1".to_string(), "tag2".to_string(), "tag3".to_string()],
        metadata: Some(Metadata {
            created_by: "test".to_string(),
            version: 1,
        }),
    };

    let buffer = Cursor::new(Vec::new());
    let mut writer = JsonlWriter::new(buffer);
    writer.write(&original).await.unwrap();
    writer.flush().await.unwrap();

    let data = writer.into_inner().into_inner().into_inner();
    let mut reader = JsonlReader::new(Cursor::new(data));

    let read_back: ComplexRecord = reader.read_line().await.unwrap().unwrap();
    assert_eq!(original, read_back);
}

#[tokio::test]
async fn roundtrip_with_null_optional() {
    let original = ComplexRecord {
        id: "xyz-789".to_string(),
        value: 0.0,
        tags: vec![],
        metadata: None,
    };

    let buffer = Cursor::new(Vec::new());
    let mut writer = JsonlWriter::new(buffer);
    writer.write(&original).await.unwrap();
    writer.flush().await.unwrap();

    let data = writer.into_inner().into_inner().into_inner();
    let mut reader = JsonlReader::new(Cursor::new(data));

    let read_back: ComplexRecord = reader.read_line().await.unwrap().unwrap();
    assert_eq!(original, read_back);
}

#[tokio::test]
async fn roundtrip_special_characters() {
    let original = TestRecord {
        id: 42,
        name: "Line1\nLine2\tTabbed\"Quoted\"\\Backslash".to_string(),
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
}

#[tokio::test]
async fn roundtrip_unicode() {
    let original = TestRecord {
        id: 1,
        name: "Hello, \u{4e16}\u{754c}! \u{1F600} \u{00e9}\u{00e8}".to_string(),
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
}

#[tokio::test]
async fn roundtrip_empty_string() {
    let original = TestRecord {
        id: 1,
        name: String::new(),
        active: false,
    };

    let buffer = Cursor::new(Vec::new());
    let mut writer = JsonlWriter::new(buffer);
    writer.write(&original).await.unwrap();
    writer.flush().await.unwrap();

    let data = writer.into_inner().into_inner().into_inner();
    let mut reader = JsonlReader::new(Cursor::new(data));

    let read_back: TestRecord = reader.read_line().await.unwrap().unwrap();
    assert_eq!(original, read_back);
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
async fn roundtrip_large_record() {
    let large_name = "x".repeat(100_000);
    let original = TestRecord {
        id: 1,
        name: large_name,
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
