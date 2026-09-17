//! Schemas derived from Vercel AI SDK `packages/anthropic/src/tool/code-execution_20250825.ts` at `6c6c221`.
//! Apache-2.0, Copyright 2023 Vercel, Inc.; converted from the locked Zod input schema.

use ferrin_spec::JsonValue;
use serde_json::json;

pub(super) fn input() -> JsonValue {
    json!({
      "anyOf": [
        {
          "type": "object",
          "properties": {
            "type": {
              "type": "string",
              "const": "programmatic-tool-call"
            },
            "code": {
              "type": "string"
            }
          },
          "required": [
            "type",
            "code"
          ]
        },
        {
          "type": "object",
          "properties": {
            "type": {
              "type": "string",
              "const": "bash_code_execution"
            },
            "command": {
              "type": "string"
            }
          },
          "required": [
            "type",
            "command"
          ]
        },
        {
          "anyOf": [
            {
              "type": "object",
              "properties": {
                "type": {
                  "type": "string",
                  "const": "text_editor_code_execution"
                },
                "command": {
                  "type": "string",
                  "const": "view"
                },
                "path": {
                  "type": "string"
                }
              },
              "required": [
                "type",
                "command",
                "path"
              ]
            },
            {
              "type": "object",
              "properties": {
                "type": {
                  "type": "string",
                  "const": "text_editor_code_execution"
                },
                "command": {
                  "type": "string",
                  "const": "create"
                },
                "path": {
                  "type": "string"
                },
                "file_text": {
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
                "command",
                "path"
              ]
            },
            {
              "type": "object",
              "properties": {
                "type": {
                  "type": "string",
                  "const": "text_editor_code_execution"
                },
                "command": {
                  "type": "string",
                  "const": "str_replace"
                },
                "path": {
                  "type": "string"
                },
                "old_str": {
                  "type": "string"
                },
                "new_str": {
                  "type": "string"
                }
              },
              "required": [
                "type",
                "command",
                "path",
                "old_str",
                "new_str"
              ]
            }
          ]
        }
      ]
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
              "const": "code_execution_result"
            },
            "stdout": {
              "type": "string"
            },
            "stderr": {
              "type": "string"
            },
            "return_code": {
              "type": "number"
            },
            "content": {
              "default": [],
              "type": "array",
              "items": {
                "type": "object",
                "properties": {
                  "type": {
                    "type": "string",
                    "const": "code_execution_output"
                  },
                  "file_id": {
                    "type": "string"
                  }
                },
                "required": [
                  "type",
                  "file_id"
                ]
              }
            }
          },
          "required": [
            "type",
            "stdout",
            "stderr",
            "return_code"
          ]
        },
        {
          "type": "object",
          "properties": {
            "type": {
              "type": "string",
              "const": "bash_code_execution_result"
            },
            "content": {
              "type": "array",
              "items": {
                "type": "object",
                "properties": {
                  "type": {
                    "type": "string",
                    "const": "bash_code_execution_output"
                  },
                  "file_id": {
                    "type": "string"
                  }
                },
                "required": [
                  "type",
                  "file_id"
                ]
              }
            },
            "stdout": {
              "type": "string"
            },
            "stderr": {
              "type": "string"
            },
            "return_code": {
              "type": "number"
            }
          },
          "required": [
            "type",
            "content",
            "stdout",
            "stderr",
            "return_code"
          ]
        },
        {
          "type": "object",
          "properties": {
            "type": {
              "type": "string",
              "const": "bash_code_execution_tool_result_error"
            },
            "error_code": {
              "type": "string"
            }
          },
          "required": [
            "type",
            "error_code"
          ]
        },
        {
          "type": "object",
          "properties": {
            "type": {
              "type": "string",
              "const": "text_editor_code_execution_tool_result_error"
            },
            "error_code": {
              "type": "string"
            }
          },
          "required": [
            "type",
            "error_code"
          ]
        },
        {
          "type": "object",
          "properties": {
            "type": {
              "type": "string",
              "const": "text_editor_code_execution_view_result"
            },
            "content": {
              "type": "string"
            },
            "file_type": {
              "type": "string"
            },
            "num_lines": {
              "anyOf": [
                {
                  "type": "number"
                },
                {
                  "type": "null"
                }
              ]
            },
            "start_line": {
              "anyOf": [
                {
                  "type": "number"
                },
                {
                  "type": "null"
                }
              ]
            },
            "total_lines": {
              "anyOf": [
                {
                  "type": "number"
                },
                {
                  "type": "null"
                }
              ]
            }
          },
          "required": [
            "type",
            "content",
            "file_type",
            "num_lines",
            "start_line",
            "total_lines"
          ]
        },
        {
          "type": "object",
          "properties": {
            "type": {
              "type": "string",
              "const": "text_editor_code_execution_create_result"
            },
            "is_file_update": {
              "type": "boolean"
            }
          },
          "required": [
            "type",
            "is_file_update"
          ]
        },
        {
          "type": "object",
          "properties": {
            "type": {
              "type": "string",
              "const": "text_editor_code_execution_str_replace_result"
            },
            "lines": {
              "anyOf": [
                {
                  "type": "array",
                  "items": {
                    "type": "string"
                  }
                },
                {
                  "type": "null"
                }
              ]
            },
            "new_lines": {
              "anyOf": [
                {
                  "type": "number"
                },
                {
                  "type": "null"
                }
              ]
            },
            "new_start": {
              "anyOf": [
                {
                  "type": "number"
                },
                {
                  "type": "null"
                }
              ]
            },
            "old_lines": {
              "anyOf": [
                {
                  "type": "number"
                },
                {
                  "type": "null"
                }
              ]
            },
            "old_start": {
              "anyOf": [
                {
                  "type": "number"
                },
                {
                  "type": "null"
                }
              ]
            }
          },
          "required": [
            "type",
            "lines",
            "new_lines",
            "new_start",
            "old_lines",
            "old_start"
          ]
        }
      ]
    })
}
