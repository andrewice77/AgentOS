#!/usr/bin/env python3
"""Minimal stdio MCP server for AgentOS smoke tests (stdlib only)."""

from __future__ import annotations

import json
import sys


def send(msg: dict) -> None:
    body = json.dumps(msg, ensure_ascii=False).encode("utf-8")
    sys.stdout.buffer.write(f"Content-Length: {len(body)}\r\n\r\n".encode("ascii"))
    sys.stdout.buffer.write(body)
    sys.stdout.buffer.flush()


def reply(req_id, result=None, error=None) -> None:
    msg = {"jsonrpc": "2.0", "id": req_id}
    if error is not None:
        msg["error"] = error
    else:
        msg["result"] = result
    send(msg)


TOOLS = [
    {
        "name": "echo",
        "description": "Echo back a text string (AgentOS smoke test).",
        "inputSchema": {
            "type": "object",
            "properties": {"text": {"type": "string"}},
            "required": ["text"],
        },
    },
    {
        "name": "add",
        "description": "Add two numbers a + b.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "a": {"type": "number"},
                "b": {"type": "number"},
            },
            "required": ["a", "b"],
        },
    },
]


def handle(req: dict) -> None:
    method = req.get("method")
    req_id = req.get("id")
    params = req.get("params") or {}

    # notifications have no id
    if req_id is None:
        return

    if method == "initialize":
        reply(
            req_id,
            {
                "protocolVersion": "2024-11-05",
                "capabilities": {"tools": {}},
                "serverInfo": {"name": "agentos-echo", "version": "0.1.0"},
            },
        )
        return

    if method == "tools/list":
        reply(req_id, {"tools": TOOLS})
        return

    if method == "tools/call":
        name = params.get("name")
        args = params.get("arguments") or {}
        if name == "echo":
            text = str(args.get("text", ""))
            reply(
                req_id,
                {
                    "content": [{"type": "text", "text": text}],
                    "isError": False,
                },
            )
            return
        if name == "add":
            try:
                a = float(args.get("a", 0))
                b = float(args.get("b", 0))
                reply(
                    req_id,
                    {
                        "content": [{"type": "text", "text": str(a + b)}],
                        "isError": False,
                    },
                )
            except Exception as e:  # noqa: BLE001
                reply(req_id, error={"code": -32000, "message": str(e)})
            return
        reply(req_id, error={"code": -32601, "message": f"unknown tool: {name}"})
        return

    if method == "ping":
        reply(req_id, {})
        return

    reply(req_id, error={"code": -32601, "message": f"method not found: {method}"})


def read_message():
    while True:
        line = sys.stdin.buffer.readline()
        if not line:
            return None
        if line.lower().startswith(b"content-length:"):
            length = int(line.split(b":", 1)[1].strip())
            while True:
                blank = sys.stdin.buffer.readline()
                if blank in (b"\r\n", b"\n", b""):
                    break
            body = sys.stdin.buffer.read(length)
            return json.loads(body.decode("utf-8"))
        # NDJSON fallback (legacy AgentOS echo)
        stripped = line.strip()
        if not stripped:
            continue
        try:
            return json.loads(stripped.decode("utf-8"))
        except json.JSONDecodeError:
            continue


def main() -> None:
    while True:
        req = read_message()
        if req is None:
            break
        handle(req)


if __name__ == "__main__":
    main()
