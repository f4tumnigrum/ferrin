//! Schemas derived from Vercel AI SDK `packages/openai/src/tool/web-search-preview.ts` at `6c6c221`.
//! Apache-2.0, Copyright 2023 Vercel, Inc.; converted from the locked Zod input schema.

use ferrin_spec::JsonValue;
use serde_json::json;

pub(super) fn input() -> JsonValue {
    json!({
      "type": "object",
      "properties": {}
    })
}

pub(super) fn output() -> JsonValue {
    json!({
      "type": "object",
      "properties": {
        "action": {
          "anyOf": [
            {
              "type": "object",
              "properties": {
                "type": {
                  "type": "string",
                  "const": "search"
                },
                "query": {
                  "type": "string"
                }
              },
              "required": [
                "type"
              ]
            },
            {
              "type": "object",
              "properties": {
                "type": {
                  "type": "string",
                  "const": "openPage"
                },
                "url": {
                  "anyOf": [
                    {
                      "type": "string"
                    },
                    {
                      "type": "null"
                    }
                  ]
                }
              },
              "required": [
                "type"
              ]
            },
            {
              "type": "object",
              "properties": {
                "type": {
                  "type": "string",
                  "const": "findInPage"
                },
                "url": {
                  "anyOf": [
                    {
                      "type": "string"
                    },
                    {
                      "type": "null"
                    }
                  ]
                },
                "pattern": {
                  "anyOf": [
                    {
                      "type": "string"
                    },
                    {
                      "type": "null"
                    }
                  ]
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

pub(super) fn arguments() -> JsonValue {
    json!({
      "type": "object",
      "properties": {
        "searchContextSize": {
          "type": "string",
          "enum": [
            "low",
            "medium",
            "high"
          ]
        },
        "userLocation": {
          "type": "object",
          "properties": {
            "type": {
              "type": "string",
              "const": "approximate"
            },
            "country": {
              "type": "string"
            },
            "city": {
              "type": "string"
            },
            "region": {
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
