//! Schemas derived from Vercel AI SDK `packages/anthropic/src/tool/web-search_20260209.ts` at `6c6c221`.
//! Apache-2.0, Copyright 2023 Vercel, Inc.; converted from the locked Zod input schema.

use ferrin_spec::JsonValue;
use serde_json::json;

pub(super) fn input() -> JsonValue {
    json!({
      "type": "object",
      "properties": {
        "query": {
          "type": "string"
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
          "url": {
            "type": "string"
          },
          "title": {
            "anyOf": [
              {
                "type": "string"
              },
              {
                "type": "null"
              }
            ]
          },
          "pageAge": {
            "anyOf": [
              {
                "type": "string"
              },
              {
                "type": "null"
              }
            ]
          },
          "encryptedContent": {
            "type": "string"
          },
          "type": {
            "type": "string",
            "const": "web_search_result"
          }
        },
        "required": [
          "url",
          "title",
          "pageAge",
          "encryptedContent",
          "type"
        ]
      }
    })
}

pub(super) fn arguments() -> JsonValue {
    json!({
      "type": "object",
      "properties": {
        "maxUses": {
          "type": "number"
        },
        "allowedDomains": {
          "type": "array",
          "items": {
            "type": "string"
          }
        },
        "blockedDomains": {
          "type": "array",
          "items": {
            "type": "string"
          }
        },
        "userLocation": {
          "type": "object",
          "properties": {
            "type": {
              "type": "string",
              "const": "approximate"
            },
            "city": {
              "type": "string"
            },
            "region": {
              "type": "string"
            },
            "country": {
              "type": "string"
            },
            "timezone": {
              "type": "string"
            }
          },
          "required": [
            "type"
          ]
        }
      }
    })
}
