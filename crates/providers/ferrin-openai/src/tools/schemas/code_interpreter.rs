//! Schemas derived from Vercel AI SDK `packages/openai/src/tool/code-interpreter.ts` at `6c6c221`.
//! Apache-2.0, Copyright 2023 Vercel, Inc.; converted from the locked Zod input schema.

use ferrin_spec::JsonValue;
use serde_json::json;

pub(super) fn input() -> JsonValue {
    json!({
      "type": "object",
      "properties": {
        "code": {
          "anyOf": [
            {
              "type": "string"
            },
            {
              "type": "null"
            }
          ]
        },
        "containerId": {
          "type": "string"
        }
      },
      "required": [
        "containerId"
      ]
    })
}

pub(super) fn output() -> JsonValue {
    json!({
      "type": "object",
      "properties": {
        "outputs": {
          "anyOf": [
            {
              "type": "array",
              "items": {
                "anyOf": [
                  {
                    "type": "object",
                    "properties": {
                      "type": {
                        "type": "string",
                        "const": "logs"
                      },
                      "logs": {
                        "type": "string"
                      }
                    },
                    "required": [
                      "type",
                      "logs"
                    ]
                  },
                  {
                    "type": "object",
                    "properties": {
                      "type": {
                        "type": "string",
                        "const": "image"
                      },
                      "url": {
                        "type": "string"
                      }
                    },
                    "required": [
                      "type",
                      "url"
                    ]
                  }
                ]
              }
            },
            {
              "type": "null"
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
        "container": {
          "anyOf": [
            {
              "type": "string"
            },
            {
              "type": "object",
              "properties": {
                "fileIds": {
                  "type": "array",
                  "items": {
                    "type": "string"
                  }
                }
              }
            }
          ]
        }
      }
    })
}
