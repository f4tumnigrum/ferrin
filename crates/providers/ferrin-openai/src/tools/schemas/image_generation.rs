//! Schemas derived from Vercel AI SDK `packages/openai/src/tool/image-generation.ts` at `6c6c221`.
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
        "result": {
          "type": "string"
        }
      },
      "required": [
        "result"
      ]
    })
}

pub(super) fn arguments() -> JsonValue {
    json!({
      "type": "object",
      "properties": {
        "action": {
          "type": "string",
          "enum": [
            "generate",
            "edit",
            "auto"
          ]
        },
        "background": {
          "type": "string",
          "enum": [
            "auto",
            "opaque",
            "transparent"
          ]
        },
        "inputFidelity": {
          "type": "string",
          "enum": [
            "low",
            "high"
          ]
        },
        "inputImageMask": {
          "type": "object",
          "properties": {
            "fileId": {
              "type": "string"
            },
            "imageUrl": {
              "type": "string"
            }
          }
        },
        "model": {
          "type": "string"
        },
        "moderation": {
          "type": "string",
          "enum": [
            "auto",
            "low"
          ]
        },
        "outputCompression": {
          "type": "integer",
          "minimum": 0,
          "maximum": 100
        },
        "outputFormat": {
          "type": "string",
          "enum": [
            "png",
            "jpeg",
            "webp"
          ]
        },
        "partialImages": {
          "type": "integer",
          "minimum": 0,
          "maximum": 3
        },
        "quality": {
          "type": "string",
          "enum": [
            "auto",
            "low",
            "medium",
            "high",
            "xhigh",
            "max"
          ]
        },
        "size": {
          "anyOf": [
            {
              "type": "string",
              "enum": [
                "1024x1024",
                "1024x1536",
                "1536x1024",
                "auto"
              ]
            },
            {
              "type": "string",
              "pattern": "^\\d+x\\d+$"
            }
          ]
        }
      },
      "additionalProperties": false
    })
}
