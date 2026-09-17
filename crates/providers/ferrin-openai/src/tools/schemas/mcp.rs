//! Schemas derived from Vercel AI SDK `packages/openai/src/tool/mcp.ts` at `6c6c221`.
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
        "type": {
          "type": "string",
          "const": "call"
        },
        "serverLabel": {
          "type": "string"
        },
        "name": {
          "type": "string"
        },
        "arguments": {
          "type": "string"
        },
        "output": {
          "anyOf": [
            {
              "type": "string"
            },
            {
              "type": "null"
            }
          ]
        },
        "error": {
          "anyOf": [
            {
              "type": "string"
            },
            {
              "$ref": "#/definitions/__schema0"
            }
          ]
        }
      },
      "required": [
        "type",
        "serverLabel",
        "name",
        "arguments"
      ],
      "definitions": {
        "__schema0": {
          "anyOf": [
            {
              "type": "string"
            },
            {
              "type": "number"
            },
            {
              "type": "boolean"
            },
            {
              "type": "null"
            },
            {
              "type": "array",
              "items": {
                "$ref": "#/definitions/__schema0"
              }
            },
            {
              "type": "object",
              "propertyNames": {
                "type": "string"
              },
              "additionalProperties": {
                "$ref": "#/definitions/__schema0"
              }
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
        "serverLabel": {
          "type": "string"
        },
        "allowedTools": {
          "anyOf": [
            {
              "type": "array",
              "items": {
                "type": "string"
              }
            },
            {
              "type": "object",
              "properties": {
                "readOnly": {
                  "type": "boolean"
                },
                "toolNames": {
                  "type": "array",
                  "items": {
                    "type": "string"
                  }
                }
              }
            }
          ]
        },
        "authorization": {
          "type": "string"
        },
        "connectorId": {
          "type": "string"
        },
        "headers": {
          "type": "object",
          "propertyNames": {
            "type": "string"
          },
          "additionalProperties": {
            "type": "string"
          }
        },
        "requireApproval": {
          "anyOf": [
            {
              "type": "string",
              "enum": [
                "always",
                "never"
              ]
            },
            {
              "type": "object",
              "properties": {
                "never": {
                  "type": "object",
                  "properties": {
                    "toolNames": {
                      "type": "array",
                      "items": {
                        "type": "string"
                      }
                    }
                  }
                }
              }
            }
          ]
        },
        "serverDescription": {
          "type": "string"
        },
        "serverUrl": {
          "type": "string"
        }
      },
      "required": [
        "serverLabel"
      ],
      "anyOf": [
        {
          "required": [
            "serverUrl"
          ]
        },
        {
          "required": [
            "connectorId"
          ]
        }
      ]
    })
}
