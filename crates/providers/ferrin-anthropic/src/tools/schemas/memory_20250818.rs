//! Schemas derived from Vercel AI SDK `packages/anthropic/src/tool/memory_20250818.ts` at `6c6c221`.
//! Apache-2.0, Copyright 2023 Vercel, Inc.; converted from the locked Zod input schema.

use ferrin_spec::JsonValue;
use serde_json::json;

pub(super) fn input() -> JsonValue {
    json!({
      "anyOf": [
        {
          "type": "object",
          "properties": {
            "command": {
              "type": "string",
              "const": "view"
            },
            "path": {
              "type": "string"
            },
            "view_range": {
              "type": "array",
              "items": [
                {
                  "type": "number"
                },
                {
                  "type": "number"
                }
              ]
            }
          },
          "required": [
            "command",
            "path"
          ]
        },
        {
          "type": "object",
          "properties": {
            "command": {
              "type": "string",
              "const": "create"
            },
            "path": {
              "type": "string"
            },
            "file_text": {
              "type": "string"
            }
          },
          "required": [
            "command",
            "path",
            "file_text"
          ]
        },
        {
          "type": "object",
          "properties": {
            "command": {
              "type": "string",
              "const": "str_replace"
            },
            "path": {
              "type": "string"
            },
            "old_str": {
              "type": "string"
            },
            "new_str": {
              "type": "string"
            }
          },
          "required": [
            "command",
            "path",
            "old_str",
            "new_str"
          ]
        },
        {
          "type": "object",
          "properties": {
            "command": {
              "type": "string",
              "const": "insert"
            },
            "path": {
              "type": "string"
            },
            "insert_line": {
              "type": "number"
            },
            "insert_text": {
              "type": "string"
            }
          },
          "required": [
            "command",
            "path",
            "insert_line",
            "insert_text"
          ]
        },
        {
          "type": "object",
          "properties": {
            "command": {
              "type": "string",
              "const": "delete"
            },
            "path": {
              "type": "string"
            }
          },
          "required": [
            "command",
            "path"
          ]
        },
        {
          "type": "object",
          "properties": {
            "command": {
              "type": "string",
              "const": "rename"
            },
            "old_path": {
              "type": "string"
            },
            "new_path": {
              "type": "string"
            }
          },
          "required": [
            "command",
            "old_path",
            "new_path"
          ]
        }
      ]
    })
}
