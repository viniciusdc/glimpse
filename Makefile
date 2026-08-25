# ─────────────────────────────────────────────────────────────────────────────
#  glimpse — Makefile
# ─────────────────────────────────────────────────────────────────────────────
#
#  Two rules are baked in here rather than left to memory:
#
#    * The developer is usually using this machine, so anything that compiles
#      runs under `nice -n 19` with `-j 2` and yields instead of competing.
#    * Nothing is piped through `head` or `tail`. A pipeline reports the pager's
#      exit status, so a failed build would look like a success.
#
#  `make check` is the gate. "It compiles" is not the bar.

CARGO  := cargo
NICE   := nice -n 19
JOBS   := -j 2

# Honour the usual GNU prefix conventions so a packager does not have to patch.
PREFIX  ?= $(HOME)/.local
BINDIR  ?= $(PREFIX)/bin
DATADIR ?= $(PREFIX)/share

.DEFAULT_GOAL := help

.PHONY: help
help: ## List the development commands
	@grep -hE '^[a-z-]+:.*?## ' $(MAKEFILE_LIST) \
	  | awk 'BEGIN{FS=":.*?## "}{printf "  \033[36m%-14s\033[0m %s\n", $$1, $$2}'

.PHONY: check
check: fmt-check lint test docs-check ## The gates CI runs, fastest-failing first

.PHONY: build
build: ## Debug build
	$(NICE) $(CARGO) build $(JOBS)

.PHONY: run
run: ## Run Glimpse
	$(NICE) $(CARGO) run $(JOBS)

.PHONY: test
test: ## Unit and integration tests
	$(NICE) $(CARGO) test $(JOBS)

.PHONY: fmt
fmt: ## Format
	$(CARGO) fmt

.PHONY: fmt-check
fmt-check: ## Fail if formatting is off
	$(CARGO) fmt --check

.PHONY: lint
lint: ## Clippy, warnings are errors
	$(NICE) $(CARGO) clippy $(JOBS) --all-targets -- -D warnings

.PHONY: selftest
selftest: ## Verify geometry against a real capture — then LOOK at the PNG
	GLIMPSE_SELFTEST=1 $(NICE) $(CARGO) run $(JOBS)
	@echo
	@echo "Now open /tmp/glimpse-selftest.png. Any Glimpse chrome in it means"
	@echo "the capture rect is wrong, whatever the numbers above said."

.PHONY: docs
docs: ## Build the API documentation
	$(NICE) $(CARGO) doc $(JOBS) --no-deps --document-private-items
	@echo "open target/doc/glimpse/index.html"

.PHONY: docs-sync
docs-sync: ## Regenerate generated doc sections and report any drift
	@scripts/sync-docs.sh

.PHONY: docs-check
docs-check: ## Fail if the docs have drifted from the code (runs in check + CI)
	@scripts/sync-docs.sh --check

.PHONY: install
install: ## Install the binary and desktop entry under PREFIX (default ~/.local)
	$(NICE) $(CARGO) build $(JOBS) --release
	install -Dm755 target/release/glimpse $(DESTDIR)$(BINDIR)/glimpse
	install -Dm644 data/glimpse.desktop $(DESTDIR)$(DATADIR)/applications/glimpse.desktop
	@echo "installed to $(DESTDIR)$(BINDIR)/glimpse"
	@case ":$$PATH:" in *":$(BINDIR):"*) ;; \
	  *) echo "note: $(BINDIR) is not on your PATH" ;; esac

.PHONY: uninstall
uninstall: ## Remove what install put down
	rm -f $(DESTDIR)$(BINDIR)/glimpse
	rm -f $(DESTDIR)$(DATADIR)/applications/glimpse.desktop
	@echo "removed"

.PHONY: check-reqs
check-reqs: ## Report missing system requirements
	@printf '%-16s ' 'X11 session:'; \
	  [ "$$XDG_SESSION_TYPE" = x11 ] && echo 'yes' \
	  || echo "NO ($${XDG_SESSION_TYPE:-unknown}) — Glimpse is X11-only by design"
	@printf '%-16s ' 'libgtk-4-dev:'; pkg-config --modversion gtk4 2>/dev/null || echo 'MISSING'
	@printf '%-16s ' 'ffmpeg:'; command -v ffmpeg >/dev/null \
	  && ffmpeg -version 2>/dev/null | awk 'NR==1' \
	  || echo 'MISSING (needed to record)'
	@printf '%-16s ' 'xdotool:'; command -v xdotool >/dev/null \
	  && echo 'yes (optional — used by the click-through check)' \
	  || echo 'missing (optional — only needed for docs/development.md checks)'
