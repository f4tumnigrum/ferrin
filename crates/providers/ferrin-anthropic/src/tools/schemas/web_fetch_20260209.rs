//! Schemas derived from Vercel AI SDK `packages/anthropic/src/tool/web-fetch-20260209.ts` at `6c6c221`.
//! Apache-2.0, Copyright 2023 Vercel, Inc.; converted from the locked Zod input schema.

use ferrin_spec::JsonValue;
use serde_json::json;

pub(super) fn input() -> JsonValue {
    json!({
      "type": "object",
      "properties": {
        "url": {
          "type": "string"
        }
      },
      "required": [
        "url"
      ]
    })
}

pub(super) fn output() -> JsonValue {
    json!({
      "type": "object",
      "properties": {
        "type": {
          "type": "string",
          "const": "web_fetch_result"
        },
        "url": {
          "type": "string"
        },
        "content": {
          "type": "object",
          "properties": {
            "type": {
              "type": "string",
              "const": "document"
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
            "citations": {
              "type": "object",
              "properties": {
                "enabled": {
                  "type": "boolean"
                }
              },
              "required": [
                "enabled"
              ]
            },
            "source": {
              "anyOf": [
                {
                  "type": "object",
                  "properties": {
                    "type": {
                      "type": "string",
                      "const": "base64"
                    },
                    "mediaType": {
                      "type": "string",
                      "const": "application/pdf"
                    },
                    "data": {
                      "type": "string"
                    }
                  },
                  "required": [
                    "type",
                    "mediaType",
                    "data"
                  ]
                },
                {
                  "type": "object",
                  "properties": {
                    "type": {
                      "type": "string",
                      "const": "text"
                    },
                    "mediaType": {
                      "type": "string",
                      "const": "text/plain"
                    },
                    "data": {
                      "type": "string"
                    }
                  },
                  "required": [
                    "type",
                    "mediaType",
                    "data"
                  ]
                }
              ]
            }
          },
          "required": [
            "type",
            "title",
            "source"
          ]
        },
        "retrievedAt": {
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
        "type",
        "url",
        "content",
        "retrievedAt"
      ]
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
        "citations": {
          "type": "object",
          "properties": {
            "enabled": {
              "type": "boolean"
            }
          },
          "required": [
            "enabled"
          ]
        },
        "maxContentTokens": {
          "type": "number"
        }
      }
    })
}
