//! Schemas derived from Vercel AI SDK `packages/openai/src/tool/programmatic-tool-calling.ts` at `6c6c221`.
//! Apache-2.0, Copyright 2023 Vercel, Inc.; converted from the locked Zod input schema.

use ferrin_spec::JsonValue;
use serde_json::json;

pub(super) fn input() -> JsonValue {
    json!({
      "type": "object",
      "properties": {
        "code": {
          "type": "string"
        },
        "fingerprint": {
          "type": "string"
        }
      },
      "required": [
        "code",
        "fingerprint"
      ]
    })
}

pub(super) fn output() -> JsonValue {
    json!({
      "type": "object",
      "properties": {
        "result": {
          "type": "string"
        },
        "status": {
          "type": "string",
          "enum": [
            "completed",
            "incomplete"
          ]
        }
      },
      "required": [
        "result",
        "status"
      ]
    })
}
