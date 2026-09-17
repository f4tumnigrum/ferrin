//! Schemas derived from Vercel AI SDK `packages/openai/src/tool/shell.ts` at `6c6c221`.
//! Apache-2.0, Copyright 2023 Vercel, Inc.; converted from the locked Zod input schema.

use ferrin_spec::JsonValue;
use serde_json::json;

pub(super) fn input() -> JsonValue {
    json!({
      "type": "object",
      "properties": {
        "action": {
          "type": "object",
          "properties": {
            "commands": {
              "type": "array",
              "items": {
                "type": "string"
              }
            },
            "timeoutMs": {
              "type": "number"
            },
            "maxOutputLength": {
              "type": "number"
            }
          },
          "required": [
            "commands"
          ]
        }
      },
      "required": [
        "action"
      ]
    })
}

pub(super) fn output() -> JsonValue {
    json!({
      "type": "object",
      "properties": {
        "output": {
          "type": "array",
          "items": {
            "type": "object",
            "properties": {
              "stdout": {
                "type": "string"
              },
              "stderr": {
                "type": "string"
              },
              "outcome": {
                "anyOf": [
                  {
                    "type": "object",
                    "properties": {
                      "type": {
                        "type": "string",
                        "const": "timeout"
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
                        "const": "exit"
                      },
                      "exitCode": {
                        "type": "number"
                      }
                    },
                    "required": [
                      "type",
                      "exitCode"
                    ]
                  }
                ]
              }
            },
            "required": [
              "stdout",
              "stderr",
              "outcome"
            ]
          }
        }
      },
      "required": [
        "output"
      ]
    })
}

pub(super) fn arguments() -> JsonValue {
    json!({
      "type": "object",
      "properties": {
        "environment": {
          "anyOf": [
            {
              "type": "object",
              "properties": {
                "type": {
                  "type": "string",
                  "const": "containerAuto"
                },
                "fileIds": {
                  "type": "array",
                  "items": {
                    "type": "string"
                  }
                },
                "memoryLimit": {
                  "type": "string",
                  "enum": [
                    "1g",
                    "4g",
                    "16g",
                    "64g"
                  ]
                },
                "networkPolicy": {
                  "anyOf": [
                    {
                      "type": "object",
                      "properties": {
                        "type": {
                          "type": "string",
                          "const": "disabled"
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
                          "const": "allowlist"
                        },
                        "allowedDomains": {
                          "type": "array",
                          "items": {
                            "type": "string"
                          }
                        },
                        "domainSecrets": {
                          "type": "array",
                          "items": {
                            "type": "object",
                            "properties": {
                              "domain": {
                                "type": "string"
                              },
                              "name": {
                                "type": "string"
                              },
                              "value": {
                                "type": "string"
                              }
                            },
                            "required": [
                              "domain",
                              "name",
                              "value"
                            ]
                          }
                        }
                      },
                      "required": [
                        "type",
                        "allowedDomains"
                      ]
                    }
                  ]
                },
                "skills": {
                  "type": "array",
                  "items": {
                    "anyOf": [
                      {
                        "type": "object",
                        "properties": {
                          "type": {
                            "type": "string",
                            "const": "skillReference"
                          },
                          "providerReference": {
                            "type": "object",
                            "propertyNames": {
                              "type": "string"
                            },
                            "additionalProperties": {
                              "type": "string"
                            }
                          },
                          "version": {
                            "type": "string"
                          }
                        },
                        "required": [
                          "type",
                          "providerReference"
                        ]
                      },
                      {
                        "type": "object",
                        "properties": {
                          "type": {
                            "type": "string",
                            "const": "inline"
                          },
                          "name": {
                            "type": "string"
                          },
                          "description": {
                            "type": "string"
                          },
                          "source": {
                            "type": "object",
                            "properties": {
                              "type": {
                                "type": "string",
                                "const": "base64"
                              },
                              "mediaType": {
                                "type": "string",
                                "const": "application/zip"
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
                        },
                        "required": [
                          "type",
                          "name",
                          "description",
                          "source"
                        ]
                      }
                    ]
                  }
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
                  "const": "containerReference"
                },
                "containerId": {
                  "type": "string"
                }
              },
              "required": [
                "type",
                "containerId"
              ]
            },
            {
              "type": "object",
              "properties": {
                "type": {
                  "type": "string",
                  "const": "local"
                },
                "skills": {
                  "type": "array",
                  "items": {
                    "type": "object",
                    "properties": {
                      "name": {
                        "type": "string"
                      },
                      "description": {
                        "type": "string"
                      },
                      "path": {
                        "type": "string"
                      }
                    },
                    "required": [
                      "name",
                      "description",
                      "path"
                    ]
                  }
                }
              }
            }
          ]
        }
      }
    })
}
