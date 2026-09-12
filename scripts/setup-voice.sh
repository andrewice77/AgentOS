#!/usr/bin/env bash
# Setup locale voice deps for AgentOS v2 (Linux).
set -euo pipefail

ROOT="${HOME}/.agentos/voice"
PIPER_DIR="${ROOT}/piper"
WHISPER_DIR="${ROOT}/whisper"
VOICE="${1:-it_IT-riccardo-x_low}"

mkdir -p "$PIPER_DIR" "$WHISPER_DIR" "${ROOT}/tmp"

echo "==> Piper TTS"
if python3 -c "import piper" 2>/dev/null; then
  echo "piper-tts già installato"
else
  echo "Installa Piper: pip install --user piper-tts"
  pip install --user piper-tts || pip3 install --user piper-tts
fi

echo "==> Download voce ${VOICE}"
python3 -m piper.download_voices "$VOICE" --data-dir "$PIPER_DIR" || \
  python3 -m piper.download_voices "$VOICE" --data-dir "$PIPER_DIR"

echo "==> Whisper.cpp (opzionale)"
if command -v whisper-cli >/dev/null 2>&1; then
  echo "whisper-cli trovato: $(command -v whisper-cli)"
elif [[ -x "${WHISPER_DIR}/whisper-cli" ]]; then
  echo "whisper-cli locale ok"
else
  cat <<EOF
Whisper non trovato.
Opzioni:
  1) Installa whisper.cpp e copia whisper-cli in ${WHISPER_DIR}/
  2) Scarica un modello ggml (es. ggml-base.bin) in ${WHISPER_DIR}/
EOF
fi

echo
echo "Fatto. In AgentOS → Settings → Voce:"
echo "  - Abilita TTS"
echo "  - Motore: piper"
echo "  - Voce: ${VOICE}"
echo "  - Salva config"
echo
echo "Directory: ${ROOT}"
