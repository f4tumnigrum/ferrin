"""Minimal legacy MCP server over stdio used by the ferrin-mcp test suite.

Speaks newline-delimited JSON-RPC. Supports `initialize`, `ping`,
`tools/list` and `tools/call` (tools `echo`, `env`, `fail`, `ask`) and
sends one server-initiated `ping` request plus one notification after the
client's `notifications/initialized`.
"""

import json
import os
import sys

SUPPORTED = {"2025-11-25", "2025-06-18", "2025-03-26", "2024-11-05"}
TOOLS = [
    {
        "name": "echo",
        "description": "Echoes text",
        "inputSchema": {
            "type": "object",
            "properties": {"text": {"type": "string"}},
            "required": ["text"],
        },
    },
    {"name": "env", "inputSchema": {"type": "object"}},
    {"name": "fail", "inputSchema": {"type": "object"}},
    {"name": "ask", "inputSchema": {"type": "object"}},
]


def write(message):
    sys.stdout.write(json.dumps(message) + "\n")
    sys.stdout.flush()


def result(request_id, value):
    write({"jsonrpc": "2.0", "id": request_id, "result": value})


def error(request_id, code, message):
    write({"jsonrpc": "2.0", "id": request_id, "error": {"code": code, "message": message}})


def text_result(text, is_error=False):
    return {"content": [{"type": "text", "text": text}], "isError": is_error}


def read_message():
    line = sys.stdin.readline()
    if not line:
        return None
    line = line.strip()
    if not line:
        return read_message()
    return json.loads(line)


def wait_for_response(request_id, pending):
    while True:
        message = read_message()
        if message is None:
            return None
        if "method" not in message and message.get("id") == request_id:
            return message
        pending.append(message)


def call_tool(request_id, params, pending):
    name = params.get("name")
    arguments = params.get("arguments") or {}
    if name == "echo":
        result(request_id, text_result(arguments.get("text", "")))
    elif name == "env":
        result(request_id, text_result(os.environ.get("FERRIN_TEST_ENV", "<unset>")))
    elif name == "fail":
        result(request_id, text_result("failure requested", True))
    elif name == "ask":
        write(
            {
                "jsonrpc": "2.0",
                "id": "srv-elicit",
                "method": "elicitation/create",
                "params": {
                    "message": "What is your name?",
                    "requestedSchema": {
                        "type": "object",
                        "properties": {"name": {"type": "string"}},
                    },
                },
            }
        )
        response = wait_for_response("srv-elicit", pending)
        if response is None:
            return
        answer = response.get("result", {})
        result(request_id, text_result(json.dumps(answer, sort_keys=True)))
    else:
        error(request_id, -32602, "unknown tool: %s" % name)


def handle(message, pending):
    method = message.get("method")
    request_id = message.get("id")
    if method is None:
        return
    if method == "initialize":
        requested = message.get("params", {}).get("protocolVersion")
        version = requested if requested in SUPPORTED else "2025-03-26"
        result(
            request_id,
            {
                "protocolVersion": version,
                "capabilities": {"tools": {"listChanged": False}},
                "serverInfo": {"name": "python-stdio", "version": "0.1.0"},
                "instructions": "stdio instructions",
            },
        )
    elif method == "notifications/initialized":
        write({"jsonrpc": "2.0", "id": "srv-ping", "method": "ping"})
        write(
            {
                "jsonrpc": "2.0",
                "method": "notifications/message",
                "params": {"level": "info", "data": "hello from stdio"},
            }
        )
    elif method == "ping":
        result(request_id, {})
    elif method == "tools/list":
        result(request_id, {"tools": TOOLS})
    elif method == "tools/call":
        call_tool(request_id, message.get("params", {}), pending)
    elif request_id is not None:
        error(request_id, -32601, "method not found: %s" % method)


def main():
    pending = []
    while True:
        if pending:
            message = pending.pop(0)
        else:
            message = read_message()
        if message is None:
            return
        handle(message, pending)


if __name__ == "__main__":
    main()
