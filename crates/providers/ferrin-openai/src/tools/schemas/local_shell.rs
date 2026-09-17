//! Schemas derived from Vercel AI SDK `packages/openai/src/tool/local-shell.ts` at `6c6c221`.
//! Apache-2.0, Copyright 2023 Vercel, Inc.; converted from the locked Zod input schema.

use ferrin_spec::JsonValue;
use serde_json::json;

pub(super) fn input() -> JsonValue {
    json!({
      "type": "object",
      "properties": {
        "action": {
          "type": "object",
          "properties": {
            "type": {
              "type": "string",
              "const": "exec"
            },
            "command": {
              "type": "array",
              "items": {
                "type": "string"
              }
            },
            "timeoutMs": {
              "type": "number"
            },
            "user": {
              "type": "string"
            },
            "workingDirectory": {
              "type": "string"
            },
            "env": {
              "type": "object",
              "propertyNames": {
                "type": "string"
              },
              "additionalProperties": {
                "type": "string"
              }
            }
          },
          "required": [
            "type",
            "command"
          ]
        }
      },
      "required": [
        "action"
      ]
    })
}

pub(super) fn output() -> JsonValue {
    json!({
      "type": "object",
      "properties": {
        "output": {
          "type": "string"
        }
      },
      "required": [
        "output"
      ]
    })
}
