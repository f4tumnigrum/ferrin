//! Schemas derived from Vercel AI SDK `packages/openai/src/tool/apply-patch.ts` at `6c6c221`.
//! Apache-2.0, Copyright 2023 Vercel, Inc.; converted from the locked Zod input schema.

use ferrin_spec::JsonValue;
use serde_json::json;

pub(super) fn input() -> JsonValue {
    json!({
      "type": "object",
      "properties": {
        "callId": {
          "type": "string"
        },
        "operation": {
          "anyOf": [
            {
              "type": "object",
              "properties": {
                "type": {
                  "type": "string",
                  "const": "create_file"
                },
                "path": {
                  "type": "string"
                },
                "diff": {
                  "type": "string"
                }
              },
              "required": [
                "type",
                "path",
                "diff"
              ]
            },
            {
              "type": "object",
              "properties": {
                "type": {
                  "type": "string",
                  "const": "delete_file"
                },
                "path": {
                  "type": "string"
                }
              },
              "required": [
                "type",
                "path"
              ]
            },
            {
              "type": "object",
              "properties": {
                "type": {
                  "type": "string",
                  "const": "update_file"
                },
                "path": {
                  "type": "string"
                },
                "diff": {
                  "type": "string"
                }
              },
              "required": [
                "type",
                "path",
                "diff"
              ]
            }
          ]
        }
      },
      "required": [
        "callId",
        "operation"
      ]
    })
}

pub(super) fn output() -> JsonValue {
    json!({
      "type": "object",
      "properties": {
        "status": {
          "type": "string",
          "enum": [
            "completed",
            "failed"
          ]
        },
        "output": {
          "type": "string"
        }
      },
      "required": [
        "status"
      ]
    })
}

pub(super) fn arguments() -> JsonValue {
    json!({
      "type": "object",
      "properties": {}
    })
}
