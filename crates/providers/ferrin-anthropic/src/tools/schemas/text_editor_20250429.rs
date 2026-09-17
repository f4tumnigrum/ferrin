//! Schemas derived from Vercel AI SDK `packages/anthropic/src/tool/text-editor_20250429.ts` at `6c6c221`.
//! Apache-2.0, Copyright 2023 Vercel, Inc.; converted from the locked Zod input schema.

use ferrin_spec::JsonValue;
use serde_json::json;

pub(super) fn input() -> JsonValue {
    json!({
      "type": "object",
      "properties": {
        "command": {
          "type": "string",
          "enum": [
            "view",
            "create",
            "str_replace",
            "insert"
          ]
        },
        "path": {
          "type": "string"
        },
        "file_text": {
          "type": "string"
        },
        "insert_line": {
          "type": "integer",
          "minimum": -9_007_199_254_740_991_i64,
          "maximum": 9_007_199_254_740_991_i64
        },
        "new_str": {
          "type": "string"
        },
        "insert_text": {
          "type": "string"
        },
        "old_str": {
          "type": "string"
        },
        "view_range": {
          "type": "array",
          "items": {
            "type": "integer",
            "minimum": -9_007_199_254_740_991_i64,
            "maximum": 9_007_199_254_740_991_i64
          }
        }
      },
      "required": [
        "command",
        "path"
      ]
    })
}
