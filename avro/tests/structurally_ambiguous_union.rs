// Licensed to the Apache Software Foundation (ASF) under one
// or more contributor license agreements.  See the NOTICE file
// distributed with this work for additional information
// regarding copyright ownership.  The ASF licenses this file
// to you under the Apache License, Version 2.0 (the
// "License"); you may not use this file except in compliance
// with the License.  You may obtain a copy of the License at
//
//   http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing,
// software distributed under the License is distributed on an
// "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied.  See the License for the
// specific language governing permissions and limitations
// under the License.

//! Demonstrates that `Reader`'s schema resolution rewrites the writer's union
//! index when two named branches are structurally identical. The writer
//! correctly serializes index 1, but `Value::resolve_union` discards the
//! wire-level tag and re-derives it by structural matching, which picks the
//! first compatible branch (index 0).

use apache_avro::{
    Reader, Schema, Writer,
    types::Value,
};
use apache_avro_test_helper::TestResult;
use pretty_assertions::assert_eq;

static WRITER_SCHEMA: &str = r#"{
    "type": "record",
    "name": "Envelope",
    "namespace": "test",
    "fields": [
        {
            "name": "payload",
            "type": [
                {
                    "type": "record",
                    "name": "ModuleBundleRequest",
                    "fields": [
                        {"name": "path", "type": "string"}
                    ]
                },
                {
                    "type": "record",
                    "name": "RegisterRequest",
                    "fields": [
                        {"name": "path", "type": "string"}
                    ]
                }
            ]
        }
    ]
}"#;

// Same union shape, but adds an extra envelope-level field with a default so
// the schemas are unequal (forcing `Reader` to invoke resolution) while
// staying compatible with the writer's data.
static READER_SCHEMA: &str = r#"{
    "type": "record",
    "name": "Envelope",
    "namespace": "test",
    "fields": [
        {
            "name": "payload",
            "type": [
                {
                    "type": "record",
                    "name": "ModuleBundleRequest",
                    "fields": [
                        {"name": "path", "type": "string"}
                    ]
                },
                {
                    "type": "record",
                    "name": "RegisterRequest",
                    "fields": [
                        {"name": "path", "type": "string"}
                    ]
                }
            ]
        },
        {"name": "trailer", "type": "string", "default": ""}
    ]
}"#;

#[test]
fn reader_preserves_union_index_for_structurally_identical_records() -> TestResult {
    let writer_schema = Schema::parse_str(WRITER_SCHEMA)?;
    let reader_schema = Schema::parse_str(READER_SCHEMA)?;

    // Writer encodes index 1 (RegisterRequest). The wire format carries the
    // tag explicitly, so the bytes are unambiguous.
    let register_request = Value::Record(vec![(
        "path".to_string(),
        Value::String("/some/path".to_string()),
    )]);
    let envelope = Value::Record(vec![(
        "payload".to_string(),
        Value::Union(1, Box::new(register_request.clone())),
    )]);

    let mut writer = Writer::new(&writer_schema, Vec::new())?;
    writer.append_value(envelope)?;
    let bytes = writer.into_inner()?;

    let mut reader = Reader::builder(&bytes[..])
        .reader_schema(&reader_schema)
        .build()?;
    let value = reader.next().expect("one record")?;

    let Value::Record(fields) = value else {
        panic!("expected record at top level");
    };
    let payload = fields
        .iter()
        .find(|(name, _)| name == "payload")
        .map(|(_, v)| v)
        .expect("payload field");

    // BUG: the reader returns Value::Union(0, ...) — ModuleBundleRequest —
    // because resolve_union threw away the writer's index (1) and picked the
    // first structurally compatible named branch instead.
    assert_eq!(
        payload,
        &Value::Union(1, Box::new(register_request)),
        "Reader corrupted the union index: writer wrote RegisterRequest (idx 1) \
         but resolution rewrote it to the first structurally compatible branch"
    );

    Ok(())
}
