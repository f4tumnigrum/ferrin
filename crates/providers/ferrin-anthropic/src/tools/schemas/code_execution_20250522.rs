//! Schemas derived from Vercel AI SDK `packages/anthropic/src/tool/code-execution_20250522.ts` at `6c6c221`.
//! Apache-2.0, Copyright 2023 Vercel, Inc.; converted from the locked Zod input schema.

use ferrin_spec::JsonValue;
use serde_json::json;

pub(super) fn input() -> JsonValue {
    json!({
      "type": "object",
      "properties": {
        "code": {
          "type": "string"
        }
      },
      "required": [
        "code"
      ]
    })
}

pub(super) fn output() -> JsonValue {
    json!({
      "type": "object",
      "properties": {
        "type": {
          "type": "string",
          "const": "code_execution_result"
        },
        "stdout": {
          "type": "string"
        },
        "stderr": {
          "type": "string"
        },
        "return_code": {
          "type": "number"
        },
        "content": {
          "default": [],
          "type": "array",
          "items": {
            "type": "object",
            "properties": {
              "type": {
                "type": "string",
                "const": "code_execution_output"
              },
              "file_id": {
                "type": "string"
              }
            },
            "required": [
              "type",
              "file_id"
            ]
          }
        }
      },
      "required": [
        "type",
        "stdout",
        "stderr",
        "return_code"
      ]
    })
}
