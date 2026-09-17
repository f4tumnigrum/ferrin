//! Schemas derived from Vercel AI SDK `packages/anthropic/src/tool/advisor_20260301.ts` at `6c6c221`.
//! Apache-2.0, Copyright 2023 Vercel, Inc.; converted from the locked Zod input schema.

use ferrin_spec::JsonValue;
use serde_json::json;

pub(super) fn input() -> JsonValue {
    json!({
      "type": "object",
      "properties": {},
      "additionalProperties": false
    })
}

pub(super) fn output() -> JsonValue {
    json!({
      "anyOf": [
        {
          "type": "object",
          "properties": {
            "type": {
              "type": "string",
              "const": "advisor_result"
            },
            "text": {
              "type": "string"
            },
            "stopReason": {
              "type": "string"
            }
          },
          "required": [
            "type",
            "text"
          ]
        },
        {
          "type": "object",
          "properties": {
            "type": {
              "type": "string",
              "const": "advisor_redacted_result"
            },
            "encryptedContent": {
              "type": "string"
            },
            "stopReason": {
              "type": "string"
            }
          },
          "required": [
            "type",
            "encryptedContent"
          ]
        },
        {
          "type": "object",
          "properties": {
            "type": {
              "type": "string",
              "const": "advisor_tool_result_error"
            },
            "errorCode": {
              "type": "string"
            }
          },
          "required": [
            "type",
            "errorCode"
          ]
        }
      ]
    })
}

pub(super) fn arguments() -> JsonValue {
    json!({
      "type": "object",
      "properties": {
        "model": {
          "type": "string"
        },
        "maxUses": {
          "type": "number"
        },
        "maxTokens": {
          "type": "integer",
          "minimum": 1024,
          "maximum": 9_007_199_254_740_991_i64
        },
        "caching": {
          "type": "object",
          "properties": {
            "type": {
              "type": "string",
              "const": "ephemeral"
            },
            "ttl": {
              "anyOf": [
                {
                  "type": "string",
                  "const": "5m"
                },
                {
                  "type": "string",
                  "const": "1h"
                }
              ]
            }
          },
          "required": [
            "type",
            "ttl"
          ]
        }
      },
      "required": [
        "model"
      ]
    })
}
