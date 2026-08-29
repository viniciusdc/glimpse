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

# Which packages the gates cover, by platform.
#
# Not `--workspace` unconditionally: `glimpse-x11` depends on `gdk4-x11`, which
# cannot build against a Quartz-backend GTK, so asking for it off Linux fails at
# the -sys crate rather than telling you anything. And not the bare default
# either — this workspace's default member is the binary alone, so a plain
# `cargo test` would run zero tests and report success.
#
# `glimpse-core` is deliberately in both lists. It is toolkit-free by manifest
# (ADR 0010), so it builds and tests anywhere, which is what keeps a Linux
# assumption from quietly settling into it while a second frontend is written.
#
# `glimpse-macos` is in both too, and for a related reason: turning a rectangle
# into avfoundation arguments is string building, so Linux CI checks the macOS
# argument construction. It picks up AppKit, and stops being portable, when the
# window model lands (ADR 0011).
UNAME := $(shell uname -s)
ifeq ($(UNAME),Linux)
  PKGS := --workspace
else
  PKGS := -p glimpse-core -p glimpse-ui -p glimpse -p glimpse-macos
endif

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
check: fmt-check lint test docs-check check-journeys ## The gates CI runs, fastest-failing first

.PHONY: build
build: ## Debug build
	$(NICE) $(CARGO) build $(JOBS) $(PKGS)

.PHONY: run
run: ## Run Glimpse
	$(NICE) $(CARGO) run $(JOBS)

.PHONY: test
test: ## Unit and integration tests
	$(NICE) $(CARGO) test $(JOBS) $(PKGS)

.PHONY: fmt
fmt: ## Format
	$(CARGO) fmt

.PHONY: fmt-check
fmt-check: ## Fail if formatting is off
	$(CARGO) fmt --check

.PHONY: lint
lint: ## Clippy, warnings are errors
	$(NICE) $(CARGO) clippy $(JOBS) $(PKGS) --all-targets -- -D warnings

.PHONY: headless
headless: ## Run Glimpse on a private X server (never touches your screen)
	@scripts/headless.sh $(NICE) $(CARGO) run $(JOBS)

.PHONY: selftest-headless
selftest-headless: ## Geometry + input-region self-test, off-screen
	@scripts/selftest.sh --headless $(NICE) $(CARGO) run $(JOBS)

# Every journey the app implements, driven through the buttons a user presses.
#
# `snapshot`, `cancel-encode` and `retry` used to be implemented and run by
# nothing. They are the durability guarantees ADR 0002 was written for, and the
# state machine covers them as policy while nothing checked the UI was wired to
# that policy. `make journeys` is what stops them drifting back out;
# `scripts/check-journeys.sh` is what stops a new one being added unwired.
.PHONY: smoke
smoke: ## Full record -> GIF and record -> MP4, off-screen
	@scripts/smoke.sh record $(NICE) $(CARGO) run $(JOBS)
	@scripts/smoke.sh record-mp4 $(NICE) $(CARGO) run $(JOBS)

.PHONY: journeys
journeys: smoke ## Every user journey off-screen, including the durability paths
	@scripts/smoke.sh snapshot $(NICE) $(CARGO) run $(JOBS)
	@scripts/smoke.sh cancel-encode $(NICE) $(CARGO) run $(JOBS)
	@scripts/smoke.sh retry $(NICE) $(CARGO) run $(JOBS)

.PHONY: check-journeys
check-journeys: ## Fail if a journey exists that nothing drives
	@scripts/check-journeys.sh

.PHONY: selftest
selftest: ## Verify geometry against a real capture — then LOOK at the PNG
	@scripts/selftest.sh $(NICE) $(CARGO) run $(JOBS)

.PHONY: docs
docs: ## Build the API documentation
	$(NICE) $(CARGO) doc $(JOBS) $(PKGS) --no-deps --document-private-items
	@echo "open target/doc/glimpse/index.html"

.PHONY: demo
demo: ## Redraw the README animation (an illustration, not a capture)
	@scripts/make-demo.py

.PHONY: docs-sync
docs-sync: ## Regenerate generated doc sections and report any drift
	@scripts/sync-docs.sh

.PHONY: docs-check
docs-check: ## Fail if the docs have drifted from the code (runs in check + CI)
	@scripts/sync-docs.sh --check

# Not part of `make check`, and not part of the Docs job's default path either:
# it needs the network, and a gate that depends on somebody else's uptime is a
# gate that eventually gets ignored. CI runs it against the changed files only.
.PHONY: docs-links
docs-links: ## Check external links in the docs (network; 404 fails, outages do not)
	@scripts/check-links-external.sh $(FILES)

# `mkdir -p` then `install -m`, never `install -D`: -D is a GNU extension, and
# BSD install does not create parent directories — the GNU form fails on macOS
# with a bare "No such file or directory" and exit 71.
.PHONY: install
install: ## Install the binary and desktop entry under PREFIX (default ~/.local)
	$(NICE) $(CARGO) build $(JOBS) --release
	mkdir -p $(DESTDIR)$(BINDIR)
	install -m 755 target/release/glimpse $(DESTDIR)$(BINDIR)/glimpse
ifeq ($(UNAME),Linux)
	mkdir -p $(DESTDIR)$(DATADIR)/applications
	install -m 644 data/glimpse.desktop $(DESTDIR)$(DATADIR)/applications/glimpse.desktop
endif
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
	@printf '%-16s %s\n' 'platform:' '$(UNAME)'
ifeq ($(UNAME),Linux)
	@printf '%-16s ' 'X11 session:'; \
	  [ "$$XDG_SESSION_TYPE" = x11 ] && echo 'yes' \
	  || echo "NO ($${XDG_SESSION_TYPE:-unknown}) — the X11 frontend needs one"
	@printf '%-16s ' 'libgtk-4-dev:'; pkg-config --modversion gtk4 2>/dev/null || echo 'MISSING'
	@printf '%-16s ' 'xdotool:'; command -v xdotool >/dev/null \
	  && echo 'yes (optional — used by the click-through check)' \
	  || echo 'missing (optional — only needed for docs/development.md checks)'
else
	@printf '%-16s %s\n' 'frontend:' 'NONE — no framing window is implemented for $(UNAME) yet'
	@printf '%-16s %s\n' '' 'glimpse-core still builds and tests here; see make test'
endif
	@printf '%-16s ' 'ffmpeg:'; command -v ffmpeg >/dev/null \
	  && ffmpeg -version 2>/dev/null | awk 'NR==1' \
	  || echo 'MISSING (needed to record)'
