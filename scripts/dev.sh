#!/usr/bin/env bash
# Wrapper comodo: delega al Makefile (fonte di verità per lo sviluppo).
# Se cambi come si avvia AgentOS, aggiorna il Makefile — non questo script.
set -euo pipefail
cd "$(dirname "$0")/.."
exec make run
