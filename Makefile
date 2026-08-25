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

CARGO := cargo
NICE  := nice -n 19
JOBS  := -j 2

.DEFAULT_GOAL := help

.PHONY: help
help: ## List the development commands
	@grep -hE '^[a-z-]+:.*?## ' $(MAKEFILE_LIST) \
	  | awk 'BEGIN{FS=":.*?## "}{printf "  \033[36m%-14s\033[0m %s\n", $$1, $$2}'

.PHONY: check
check: fmt-check lint test ## The gates CI runs, fastest-failing first

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

.PHONY: check-reqs
check-reqs: ## Report missing system requirements
	@printf '%-16s ' 'X11 session:'; \
	  [ "$$XDG_SESSION_TYPE" = x11 ] && echo 'yes' \
	  || echo "NO ($${XDG_SESSION_TYPE:-unknown}) — Glimpse is X11-only by design"
	@printf '%-16s ' 'libgtk-4-dev:'; pkg-config --modversion gtk4 2>/dev/null || echo 'MISSING'
	@printf '%-16s ' 'ffmpeg:'; ffmpeg -version 2>/dev/null | head -1 || echo 'MISSING (needed to record)'
