#!/usr/bin/env bash
# Setup Audio8 TTS ONNX (CPU) for AgentOS — lascia la GPU a Ollama.
# Usa sempre un venv (PEP 668 / Debian externally-managed-environment).
set -euo pipefail

ROOT="${HOME}/.agentos/voice/audio8"
REPO="${ROOT}/Audio8_TTS"
ONNX="${REPO}/onnx_runtime"

mkdir -p "$ROOT"

if [[ -d "$REPO" && ! -w "$REPO" ]]; then
  echo "ERRORE: $REPO non è scrivibile (probabilmente creato con sudo)." >&2
  echo "Esegui: sudo chown -R \"\$USER:\$USER\" \"$ROOT\"" >&2
  exit 1
fi

ensure_venv() {
  local dir="$1"
  local pybin="${PYTHON_BIN:-python3}"

  if [[ -x "$dir/.venv/bin/python" ]] && "$dir/.venv/bin/python" -m pip --version >/dev/null 2>&1; then
    echo "venv già pronto: $dir/.venv"
    return 0
  fi

  rm -rf "$dir/.venv"

  if "$pybin" -c 'import ensurepip' 2>/dev/null; then
    "$pybin" -m venv "$dir/.venv"
  else
    echo "ensurepip assente — creo venv --without-pip e bootstrap pip"
    echo "(opzionale, più pulito: sudo apt install python3.11-venv)"
    "$pybin" -m venv --without-pip "$dir/.venv"
    local getpip
    getpip="$(mktemp)"
    curl -fsSL https://bootstrap.pypa.io/get-pip.py -o "$getpip"
    "$dir/.venv/bin/python" "$getpip" --disable-pip-version-check
    rm -f "$getpip"
  fi

  "$dir/.venv/bin/python" -m pip install --upgrade pip
}

echo "==> Clone / update Audio8_TTS"
if [[ -d "$REPO/.git" ]]; then
  git -C "$REPO" pull --ff-only || true
else
  git clone --depth 1 https://github.com/Audio8-AI/Audio8_TTS.git "$REPO"
fi

cd "$ONNX"

echo "==> Crea .venv + dipendenze runtime"
ensure_venv "$ONNX"
"$ONNX/.venv/bin/python" -m pip install -r "$ONNX/requirements.txt"
# Come setup.sh upstream: tokenizers senza deps Hub
"$ONNX/.venv/bin/python" -m pip install --no-deps tokenizers==0.22.2

echo "==> huggingface_hub nel venv"
"$ONNX/.venv/bin/python" -m pip install -U "huggingface_hub[cli]"

echo "==> Download modello ONNX INT4 (~1GB) → ${ONNX}/model"
"$ONNX/.venv/bin/hf" download Audio8/Audio8-TTS-Preview-0.6B-ONNX-INT4 --local-dir "${ONNX}/model"

if [[ ! -f "${ONNX}/model/runtime_manifest.json" ]]; then
  echo "ATTENZIONE: runtime_manifest.json assente — verifica ${ONNX}/model" >&2
fi

cat <<EOF

Fatto (CPU / ONNX).

Avvia il servizio (terminale dedicato, SENZA sudo):
  cd ${ONNX}
  bash start_server.sh

Poi apri http://127.0.0.1:8024 e registra una voce.

In AgentOS → Settings → Voce:
  - Motore: audio8 · Device: cpu · Voce Audio8: <nome registrato>

EOF
