#!/usr/bin/env bash
# AgentOS MVP smoke tests (no GUI required)
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"
source "$HOME/.cargo/env" 2>/dev/null || true

PASS=0
FAIL=0
ok() { echo "  OK  $*"; PASS=$((PASS + 1)); }
bad() { echo "  FAIL $*"; FAIL=$((FAIL + 1)); }

echo "== AgentOS smoke =="

# 1) Ollama
if curl -sf -m 5 http://127.0.0.1:11434/api/tags >/tmp/agentos-ollama-tags.json; then
  ok "Ollama /api/tags"
  if curl -sf -m 60 http://127.0.0.1:11434/api/chat \
    -d '{"model":"qwen3.5:9b-q4km","stream":false,"think":false,"messages":[{"role":"user","content":"Rispondi solo: PONG"}]}' \
    | grep -qi 'PONG'; then
    ok "Ollama chat think:false → PONG"
  else
    bad "Ollama chat did not return PONG (model missing or slow?)"
  fi
else
  bad "Ollama non raggiungibile su 127.0.0.1:11434"
fi

# 2) MCP echo stdio
MCP_OUT=$(printf '%s\n' \
  '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"smoke","version":"0"}}}' \
  '{"jsonrpc":"2.0","method":"notifications/initialized"}' \
  '{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"echo","arguments":{"text":"SMOKE"}}}' \
  | python3 "$ROOT/scripts/mcp_echo.py" 2>/dev/null || true)
if echo "$MCP_OUT" | grep -q 'SMOKE'; then
  ok "MCP echo server stdio"
else
  bad "MCP echo server"
fi

# 3) Rust unit tests (organizer / sessions)
if (cd "$ROOT/src-tauri" && CARGO_TARGET_DIR="$ROOT/src-tauri/target" cargo test --release --lib -- --nocapture) >/tmp/agentos-cargo-test.log 2>&1; then
  ok "cargo test --lib"
else
  bad "cargo test — vedi /tmp/agentos-cargo-test.log"
  tail -n 40 /tmp/agentos-cargo-test.log || true
fi

# 4) Release binary markers
BIN="$ROOT/src-tauri/target/release/agentos"
if [[ -x "$BIN" ]]; then
  ok "binary exists ($BIN)"
  for s in "chat:resolve_session" "ollama:stream_post" "mcp call" "ricordami"; do
    if grep -a -F -q "$s" "$BIN"; then
      ok "binary contains '$s'"
    else
      bad "binary missing '$s' (rebuild needed?)"
    fi
  done
else
  bad "binary missing — run: npm run build && cargo build --release --manifest-path src-tauri/Cargo.toml"
fi

# 5) Frontend build present
if [[ -f "$ROOT/build/index.html" ]]; then
  ok "frontend build/index.html"
else
  bad "frontend build missing — run npm run build"
fi

echo
echo "Result: $PASS passed, $FAIL failed"
[[ "$FAIL" -eq 0 ]]
