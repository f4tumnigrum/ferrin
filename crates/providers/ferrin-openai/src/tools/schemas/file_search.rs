//! Schemas derived from Vercel AI SDK `packages/openai/src/tool/file-search.ts` at `6c6c221`.
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
        "queries": {
          "type": "array",
          "items": {
            "type": "string"
          }
        },
        "results": {
          "anyOf": [
            {
              "type": "array",
              "items": {
                "type": "object",
                "properties": {
                  "attributes": {
                    "type": "object",
                    "propertyNames": {
                      "type": "string"
                    },
                    "additionalProperties": {}
                  },
                  "fileId": {
                    "type": "string"
                  },
                  "filename": {
                    "type": "string"
                  },
                  "score": {
                    "type": "number"
                  },
                  "text": {
                    "type": "string"
                  }
                },
                "required": [
                  "attributes",
                  "fileId",
                  "filename",
                  "score",
                  "text"
                ]
              }
            },
            {
              "type": "null"
            }
          ]
        }
      },
      "required": [
        "queries",
        "results"
      ]
    })
}

pub(super) fn arguments() -> JsonValue {
    json!({
      "type": "object",
      "properties": {
        "vectorStoreIds": {
          "type": "array",
          "items": {
            "type": "string"
          }
        },
        "maxNumResults": {
          "type": "number"
        },
        "ranking": {
          "type": "object",
          "properties": {
            "ranker": {
              "type": "string"
            },
            "scoreThreshold": {
              "type": "number"
            }
          }
        },
        "filters": {
          "anyOf": [
            {
              "type": "object",
              "properties": {
                "key": {
                  "type": "string"
                },
                "type": {
                  "type": "string",
                  "enum": [
                    "eq",
                    "ne",
                    "gt",
                    "gte",
                    "lt",
                    "lte",
                    "in",
                    "nin"
                  ]
                },
                "value": {
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
                      "type": "array",
                      "items": {
                        "type": "string"
                      }
                    }
                  ]
                }
              },
              "required": [
                "key",
                "type",
                "value"
              ]
            },
            {
              "$ref": "#/definitions/__schema0"
            }
          ]
        }
      },
      "required": [
        "vectorStoreIds"
      ],
      "definitions": {
        "__schema0": {
          "type": "object",
          "properties": {
            "type": {
              "type": "string",
              "enum": [
                "and",
                "or"
              ]
            },
            "filters": {
              "type": "array",
              "items": {
                "anyOf": [
                  {
                    "type": "object",
                    "properties": {
                      "key": {
                        "type": "string"
                      },
                      "type": {
                        "type": "string",
                        "enum": [
                          "eq",
                          "ne",
                          "gt",
                          "gte",
                          "lt",
                          "lte",
                          "in",
                          "nin"
                        ]
                      },
                      "value": {
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
                            "type": "array",
                            "items": {
                              "type": "string"
                            }
                          }
                        ]
                      }
                    },
                    "required": [
                      "key",
                      "type",
                      "value"
                    ]
                  },
                  {
                    "allOf": [
                      {
                        "$ref": "#/definitions/__schema0"
                      }
                    ]
                  }
                ]
              }
            }
          },
          "required": [
            "type",
            "filters"
          ]
        }
      }
    })
}
