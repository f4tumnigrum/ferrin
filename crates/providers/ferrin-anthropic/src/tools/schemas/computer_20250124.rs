//! Schemas derived from Vercel AI SDK `packages/anthropic/src/tool/computer_20250124.ts` at `6c6c221`.
//! Apache-2.0, Copyright 2023 Vercel, Inc.; converted from the locked Zod input schema.

use ferrin_spec::JsonValue;
use serde_json::json;

pub(super) fn input() -> JsonValue {
    json!({
      "type": "object",
      "properties": {
        "action": {
          "type": "string",
          "enum": [
            "key",
            "hold_key",
            "type",
            "cursor_position",
            "mouse_move",
            "left_mouse_down",
            "left_mouse_up",
            "left_click",
            "left_click_drag",
            "right_click",
            "middle_click",
            "double_click",
            "triple_click",
            "scroll",
            "wait",
            "screenshot"
          ]
        },
        "coordinate": {
          "type": "array",
          "items": [
            {
              "type": "integer",
              "minimum": -9_007_199_254_740_991_i64,
              "maximum": 9_007_199_254_740_991_i64
            },
            {
              "type": "integer",
              "minimum": -9_007_199_254_740_991_i64,
              "maximum": 9_007_199_254_740_991_i64
            }
          ]
        },
        "duration": {
          "type": "number"
        },
        "scroll_amount": {
          "type": "number"
        },
        "scroll_direction": {
          "type": "string",
          "enum": [
            "up",
            "down",
            "left",
            "right"
          ]
        },
        "start_coordinate": {
          "type": "array",
          "items": [
            {
              "type": "integer",
              "minimum": -9_007_199_254_740_991_i64,
              "maximum": 9_007_199_254_740_991_i64
            },
            {
              "type": "integer",
              "minimum": -9_007_199_254_740_991_i64,
              "maximum": 9_007_199_254_740_991_i64
            }
          ]
        },
        "text": {
          "type": "string"
        }
      },
      "required": [
        "action"
      ]
    })
}
