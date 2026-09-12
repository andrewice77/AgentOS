# =============================================================================
# AgentOS — launcher sviluppo locale
# =============================================================================
# Uso tipico:
#   make          → build + avvia (companion: avatar floating)
#   make kill     → chiude l'istanza in esecuzione
#   make restart  → kill + build + avvia
#
# ⚠️  AGGIORNA QUESTO FILE quando cambi uno di questi punti:
#   - comando di build frontend / path `build/`
#   - CARGO_TARGET_DIR o nome binario release
#   - variabili d'ambiente (WEBKIT_*, APPDIR/APPIMAGE)
#   - modalità shell (avatar/chat/main) o flag Tauri
#
# Tieni allineati anche: package.json `start:release`, README "Build & avvio",
# scripts/dev.sh
# =============================================================================

.PHONY: help run build kill restart check ui rust

ROOT            := $(abspath $(dir $(lastword $(MAKEFILE_LIST))))
CARGO_TARGET_DIR := $(ROOT)/src-tauri/target
BIN             := $(CARGO_TARGET_DIR)/release/agentos
CARGO_MANIFEST  := $(ROOT)/src-tauri/Cargo.toml

export CARGO_TARGET_DIR
export PATH := $(HOME)/.cargo/bin:/usr/local/bin:$(PATH)

# WebKit su NVIDIA/GBM: evita finestra nera. Non rimuovere senza verificare su KDE+X11.
WEBKIT_ENV := WEBKIT_DISABLE_DMABUF_RENDERER=1

help:
	@echo "AgentOS — target disponibili:"
	@echo "  make / make run   build UI+Rust e avvia (companion)"
	@echo "  make build        solo build (npm + cargo --release)"
	@echo "  make ui           solo frontend (npm run build)"
	@echo "  make rust         solo binario Rust (dopo ui se hai cambiato Svelte)"
	@echo "  make kill         termina processi agentos"
	@echo "  make restart      kill + run"
	@echo "  make check        svelte-check"
	@echo ""
	@echo "Nota: il frontend è embedded nel binario release → dopo modifiche UI"
	@echo "      serve sempre 'make build' (o ui + rust), non basta npm run build."

run: build
	@$(MAKE) --no-print-directory kill
	@echo "→ avvio $(BIN) (detached)"
	@cd $(ROOT) && \
	  ulimit -n 65536 2>/dev/null || true; \
	  nohup env -u APPDIR -u APPIMAGE $(WEBKIT_ENV) $(BIN) \
	    >/tmp/agentos-run.log 2>&1 & \
	  echo "PID=$$!"; \
	  sleep 0.6; \
	  pgrep -x agentos >/dev/null && echo "✓ AgentOS in esecuzione" || \
	    (echo "✗ avvio fallito — vedi /tmp/agentos-run.log"; exit 1)

build: ui rust
	@echo "✓ build pronto: $(BIN)"

ui:
	@echo "→ npm run build"
	@cd $(ROOT) && npm run build

rust:
	@echo "→ cargo build --release"
	@cargo build --release --manifest-path $(CARGO_MANIFEST) --bin agentos

kill:
	@pids=$$(pgrep -x agentos 2>/dev/null || true); \
	if [ -n "$$pids" ]; then \
	  echo "→ kill agentos ($$pids)"; \
	  kill $$pids 2>/dev/null || true; \
	  sleep 0.4; \
	else \
	  echo "→ nessun agentos in esecuzione"; \
	fi

restart: kill run

check:
	@cd $(ROOT) && npm run check
