//! Schemas derived from Vercel AI SDK `packages/openai/src/tool/tool-search.ts` at `6c6c221`.
//! Apache-2.0, Copyright 2023 Vercel, Inc.; converted from the locked Zod input schema.

use ferrin_spec::JsonValue;
use serde_json::json;

pub(super) fn input() -> JsonValue {
    json!({
      "type": "object",
      "properties": {
        "arguments": {},
        "call_id": {
          "anyOf": [
            {
              "type": "string"
            },
            {
              "type": "null"
            }
          ]
        }
      }
    })
}

pub(super) fn output() -> JsonValue {
    json!({
      "type": "object",
      "properties": {
        "tools": {
          "type": "array",
          "items": {
            "type": "object",
            "propertyNames": {
              "type": "string"
            },
            "additionalProperties": {}
          }
        }
      },
      "required": [
        "tools"
      ]
    })
}

pub(super) fn arguments() -> JsonValue {
    json!({
      "type": "object",
      "properties": {
        "execution": {
          "type": "string",
          "enum": [
            "server",
            "client"
          ]
        },
        "description": {
          "type": "string"
        },
        "parameters": {
          "type": "object",
          "propertyNames": {
            "type": "string"
          },
          "additionalProperties": {}
        }
      }
    })
}
