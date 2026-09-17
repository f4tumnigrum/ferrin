//! Schemas derived from Vercel AI SDK `packages/openai/src/tool/computer.ts` at `6c6c221`.
//! Apache-2.0, Copyright 2023 Vercel, Inc.; converted from the locked Zod input schema.

use ferrin_spec::JsonValue;
use serde_json::json;

pub(super) fn input() -> JsonValue {
    json!({
      "type": "object",
      "properties": {
        "actions": {
          "type": "array",
          "items": {
            "anyOf": [
              {
                "type": "object",
                "properties": {
                  "type": {
                    "type": "string",
                    "const": "click"
                  },
                  "button": {
                    "type": "string",
                    "enum": [
                      "left",
                      "right",
                      "wheel",
                      "back",
                      "forward"
                    ]
                  },
                  "x": {
                    "type": "number"
                  },
                  "y": {
                    "type": "number"
                  },
                  "keys": {
                    "type": "array",
                    "items": {
                      "type": "string"
                    }
                  }
                },
                "required": [
                  "type",
                  "button",
                  "x",
                  "y"
                ]
              },
              {
                "type": "object",
                "properties": {
                  "type": {
                    "type": "string",
                    "const": "double_click"
                  },
                  "x": {
                    "type": "number"
                  },
                  "y": {
                    "type": "number"
                  },
                  "keys": {
                    "type": "array",
                    "items": {
                      "type": "string"
                    }
                  }
                },
                "required": [
                  "type",
                  "x",
                  "y"
                ]
              },
              {
                "type": "object",
                "properties": {
                  "type": {
                    "type": "string",
                    "const": "drag"
                  },
                  "path": {
                    "type": "array",
                    "items": {
                      "type": "object",
                      "properties": {
                        "x": {
                          "type": "number"
                        },
                        "y": {
                          "type": "number"
                        }
                      },
                      "required": [
                        "x",
                        "y"
                      ]
                    }
                  },
                  "keys": {
                    "type": "array",
                    "items": {
                      "type": "string"
                    }
                  }
                },
                "required": [
                  "type",
                  "path"
                ]
              },
              {
                "type": "object",
                "properties": {
                  "type": {
                    "type": "string",
                    "const": "keypress"
                  },
                  "keys": {
                    "type": "array",
                    "items": {
                      "type": "string"
                    }
                  }
                },
                "required": [
                  "type",
                  "keys"
                ]
              },
              {
                "type": "object",
                "properties": {
                  "type": {
                    "type": "string",
                    "const": "move"
                  },
                  "x": {
                    "type": "number"
                  },
                  "y": {
                    "type": "number"
                  },
                  "keys": {
                    "type": "array",
                    "items": {
                      "type": "string"
                    }
                  }
                },
                "required": [
                  "type",
                  "x",
                  "y"
                ]
              },
              {
                "type": "object",
                "properties": {
                  "type": {
                    "type": "string",
                    "const": "screenshot"
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
                    "const": "scroll"
                  },
                  "x": {
                    "type": "number"
                  },
                  "y": {
                    "type": "number"
                  },
                  "scrollX": {
                    "type": "number"
                  },
                  "scrollY": {
                    "type": "number"
                  },
                  "keys": {
                    "type": "array",
                    "items": {
                      "type": "string"
                    }
                  }
                },
                "required": [
                  "type",
                  "x",
                  "y",
                  "scrollX",
                  "scrollY"
                ]
              },
              {
                "type": "object",
                "properties": {
                  "type": {
                    "type": "string",
                    "const": "type"
                  },
                  "text": {
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
                    "const": "wait"
                  }
                },
                "required": [
                  "type"
                ]
              }
            ]
          }
        },
        "pendingSafetyChecks": {
          "type": "array",
          "items": {
            "type": "object",
            "properties": {
              "id": {
                "type": "string"
              },
              "code": {
                "type": "string"
              },
              "message": {
                "type": "string"
              }
            },
            "required": [
              "id"
            ]
          }
        },
        "status": {
          "type": "string",
          "enum": [
            "in_progress",
            "completed",
            "incomplete"
          ]
        }
      },
      "required": [
        "actions",
        "pendingSafetyChecks",
        "status"
      ]
    })
}

pub(super) fn output() -> JsonValue {
    json!({
      "type": "object",
      "properties": {
        "output": {
          "anyOf": [
            {
              "type": "object",
              "properties": {
                "type": {
                  "type": "string",
                  "const": "computer_screenshot"
                },
                "imageUrl": {
                  "type": "string"
                },
                "fileId": {
                  "type": "string"
                },
                "detail": {
                  "type": "string",
                  "enum": [
                    "auto",
                    "low",
                    "high",
                    "original"
                  ]
                }
              },
              "required": [
                "type",
                "imageUrl"
              ]
            },
            {
              "type": "object",
              "properties": {
                "type": {
                  "type": "string",
                  "const": "computer_screenshot"
                },
                "fileId": {
                  "type": "string"
                },
                "imageUrl": {
                  "type": "string"
                },
                "detail": {
                  "type": "string",
                  "enum": [
                    "auto",
                    "low",
                    "high",
                    "original"
                  ]
                }
              },
              "required": [
                "type",
                "fileId"
              ]
            }
          ]
        },
        "acknowledgedSafetyChecks": {
          "type": "array",
          "items": {
            "type": "object",
            "properties": {
              "id": {
                "type": "string"
              },
              "code": {
                "type": "string"
              },
              "message": {
                "type": "string"
              }
            },
            "required": [
              "id"
            ]
          }
        }
      },
      "required": [
        "output"
      ]
    })
}
