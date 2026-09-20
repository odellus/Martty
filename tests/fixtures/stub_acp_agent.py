#!/usr/bin/env python3
"""Stub ACP v1 agent for the startup end-to-end tests.

Answers the bare minimum, logs every request it receives as JSONL, and takes
its behavior from the environment:

  STUB_LOG          path of the JSONL wire log (required)
  STUB_CAPS         load | resume | both | none — which re-attach methods exist
  STUB_LOAD_FAIL=1  the implemented re-attach methods refuse the id (-32602)
"""
import json
import os
import sys
import time

LOG = os.environ["STUB_LOG"]
CAPS = os.environ.get("STUB_CAPS", "load")
HAS_LOAD = CAPS in ("load", "both")
HAS_RESUME = CAPS in ("resume", "both")
REFUSE = os.environ.get("STUB_LOAD_FAIL") == "1"

log = open(LOG, "a", buffering=1)
MODES = {
    "currentModeId": "default",
    "availableModes": [{"id": "default", "name": "Default", "description": ""}],
}
MODEL_OPTIONS = [
    {"value": "stub-default", "name": "stub-default"},
    {"value": "qwen3.8-max", "name": "qwen3.8-max"},
]


def note(msg):
    log.write(json.dumps({"t": round(time.time(), 3), "msg": msg}) + "\n")


def send(obj):
    sys.stdout.write(json.dumps(obj) + "\n")
    sys.stdout.flush()


def update(sid, kind, **extra):
    send({
        "jsonrpc": "2.0",
        "method": "session/update",
        "params": {"sessionId": sid, "update": {"sessionUpdate": kind, **extra}},
    })


def config_options(current):
    return [{
        "type": "select",
        "id": "model",
        "name": "Model",
        "currentValue": current,
        "options": MODEL_OPTIONS,
    }]


def session_setup(sid):
    return {
        "sessionId": sid,
        "modes": MODES,
        "models": {
            "currentModelId": "stub-default",
            "availableModels": MODEL_OPTIONS,
        },
        "configOptions": config_options("stub-default"),
    }


for line in sys.stdin:
    line = line.strip()
    if not line:
        continue
    try:
        msg = json.loads(line)
    except ValueError:
        continue
    note(msg)
    method = msg.get("method")
    if method is None:
        continue
    rid = msg.get("id")
    params = msg.get("params") or {}
    if method == "initialize":
        send({"jsonrpc": "2.0", "id": rid, "result": {
            "protocolVersion": 1,
            "agentCapabilities": {
                "loadSession": HAS_LOAD,
                "sessionCapabilities": {"list": {}, **({"resume": {}} if HAS_RESUME else {})},
            },
            "agentInfo": {"name": "stub-agent", "title": "Stub", "version": "0.0.1"},
            "authMethods": [],
        }})
    elif method == "session/new":
        send({"jsonrpc": "2.0", "id": rid, "result": session_setup("stub-new-0001")})
    elif method in ("session/load", "session/resume"):
        implemented = HAS_LOAD if method == "session/load" else HAS_RESUME
        if not implemented:
            send({"jsonrpc": "2.0", "id": rid,
                  "error": {"code": -32601, "message": "Method not found: " + method}})
            continue
        sid = params.get("sessionId") or "stub-loaded"
        if REFUSE:
            send({"jsonrpc": "2.0", "id": rid,
                  "error": {"code": -32602, "message": "no such session: " + sid}})
            continue
        # A replayed tool call: the pane should frame the command and keep it
        # on screen after the result lands.
        send({"jsonrpc": "2.0", "method": "session/update", "params": {
            "sessionId": sid,
            "update": {
                "sessionUpdate": "tool_call",
                "toolCallId": "t1",
                "title": "execute",
                "kind": "execute",
                "status": "in_progress",
                "rawInput": {"code": "print(6 * 7)"},
            },
        }})
        send({"jsonrpc": "2.0", "method": "session/update", "params": {
            "sessionId": sid,
            "update": {
                "sessionUpdate": "tool_call_update",
                "toolCallId": "t1",
                "status": "completed",
                "rawOutput": {"output": "42"},
            },
        }})
        # crow-cli's shape: no rawInput at all. The cell rides the call's
        # `content`, already fenced, and the completion repeats it ahead of
        # the output because clients merge update fields over the start.
        fence = {"type": "content",
                 "content": {"type": "text", "text": "```python\nprint(6 * 9)\n```"}}
        send({"jsonrpc": "2.0", "method": "session/update", "params": {
            "sessionId": sid,
            "update": {
                "sessionUpdate": "tool_call",
                "toolCallId": "t2",
                "title": "execute",
                "kind": "execute",
                "status": "pending",
                "content": [fence],
            },
        }})
        send({"jsonrpc": "2.0", "method": "session/update", "params": {
            "sessionId": sid,
            "update": {
                "sessionUpdate": "tool_call_update",
                "toolCallId": "t2",
                "status": "completed",
                "content": [fence, {"type": "content",
                                    "content": {"type": "text", "text": "54"}}],
            },
        }})
        update(sid, "agent_message_chunk", messageId="m1",
               content={"type": "text", "text": "replayed history line"})
        send({"jsonrpc": "2.0", "id": rid, "result": session_setup(sid)})
    elif method == "session/set_config_option":
        value = params.get("value")
        current = value.get("value") if isinstance(value, dict) else value
        send({"jsonrpc": "2.0", "id": rid,
              "result": {"configOptions": config_options(current or "stub-default")}})
    elif method == "session/prompt":
        sid = params.get("sessionId") or "stub-new-0001"
        update(sid, "agent_message_chunk", messageId="m2",
               content={"type": "text", "text": "stub reply ok"})
        send({"jsonrpc": "2.0", "id": rid, "result": {"stopReason": "end_turn"}})
    elif method == "exit":
        break
    elif rid is not None:
        send({"jsonrpc": "2.0", "id": rid, "result": {}})
