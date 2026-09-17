//! Schemas derived from Vercel AI SDK `packages/anthropic/src/tool/tool-search-bm25_20251119.ts` at `6c6c221`.
//! Apache-2.0, Copyright 2023 Vercel, Inc.; converted from the locked Zod input schema.

use ferrin_spec::JsonValue;
use serde_json::json;

pub(super) fn input() -> JsonValue {
    json!({
      "type": "object",
      "properties": {
        "query": {
          "type": "string"
        },
        "limit": {
          "type": "number"
        }
      },
      "required": [
        "query"
      ]
    })
}

pub(super) fn output() -> JsonValue {
    json!({
      "type": "array",
      "items": {
        "type": "object",
        "properties": {
          "type": {
            "type": "string",
            "const": "tool_reference"
          },
          "toolName": {
            "type": "string"
          }
        },
        "required": [
          "type",
          "toolName"
        ]
      }
    })
}
