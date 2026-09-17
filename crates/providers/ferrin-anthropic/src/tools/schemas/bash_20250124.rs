//! Schemas derived from Vercel AI SDK `packages/anthropic/src/tool/bash_20250124.ts` at `6c6c221`.
//! Apache-2.0, Copyright 2023 Vercel, Inc.; converted from the locked Zod input schema.

use ferrin_spec::JsonValue;
use serde_json::json;

pub(super) fn input() -> JsonValue {
    json!({
      "type": "object",
      "properties": {
        "command": {
          "type": "string"
        },
        "restart": {
          "type": "boolean"
        }
      },
      "required": [
        "command"
      ]
    })
}
