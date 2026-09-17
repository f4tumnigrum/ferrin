//! Schemas derived from Vercel AI SDK `packages/openai/src/tool/custom.ts` at `6c6c221`.
//! Apache-2.0, Copyright 2023 Vercel, Inc.; converted from the locked Zod input schema.

use ferrin_spec::JsonValue;
use serde_json::json;

pub(super) fn input() -> JsonValue {
    json!({
      "type": "string"
    })
}

pub(super) fn arguments() -> JsonValue {
    json!({
      "type": "object",
      "properties": {
        "description": {
          "type": "string"
        },
        "async": {
          "type": "boolean"
        },
        "format": {
          "anyOf": [
            {
              "type": "object",
              "properties": {
                "type": {
                  "type": "string",
                  "const": "grammar"
                },
                "syntax": {
                  "type": "string",
                  "enum": [
                    "regex",
                    "lark"
                  ]
                },
                "definition": {
                  "type": "string"
                }
              },
              "required": [
                "type",
                "syntax",
                "definition"
              ]
            },
            {
              "type": "object",
              "properties": {
                "type": {
                  "type": "string",
                  "const": "text"
                }
              },
              "required": [
                "type"
              ]
            }
          ]
        }
      }
    })
}
