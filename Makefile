# Fork-local convenience wrapper. Upstream Zed drives everything through script/;
# this only shortens the loop that day-to-day UI and theme work actually uses.
# Anything not defined here still lives in script/ — this does not replace it.

CARGO ?= cargo
PACKAGE := zode

# `dev` compiles fastest but runs UNOPTIMISED — Cargo.toml's [profile.dev] sets
# opt-level 3 for proc-macros only, so gpui and the editor's own rendering are
# built at opt-level 0 and text scrolling visibly stutters. `release-fast` keeps
# release's optimisation while dropping LTO, so it still links quickly.
#   make build PROFILE=release-fast   ·   make fast   (build + launch, one step)
PROFILE ?= dev
PROFILE_DIR := $(if $(filter dev,$(PROFILE)),debug,$(PROFILE))
BIN := target/$(PROFILE_DIR)/$(PACKAGE)

# Directory zode opens. Override: make dev PROJECT=~/some/repo
# Set it EMPTY (make dev PROJECT=) to launch with no folder at all. That is the
# only way to reach the first-run screen: passing any path opens it directly and
# the onboarding branch never runs, so a fresh reset looks like it did nothing.
PROJECT ?= .

# Trims several GB of debug info while keeping backtraces readable.
export CARGO_PROFILE_DEV_DEBUG ?= line-tables-only
export CARGO_PROFILE_RELEASE_FAST_DEBUG ?= line-tables-only

# Where zode stores things. These mirror the roots in crates/paths/src/paths.rs
# — if that file moves and these do not, `make reset-*` starts deleting nothing
# while reporting success. `make paths` prints them so a drift is visible without
# reading the source.
#
# Not covered: a run started with `--user-data-dir`, which puts everything
# somewhere else entirely.
UNAME_S := $(shell uname -s)
ifeq ($(UNAME_S),Darwin)
  ZODE_CONFIG := $(HOME)/.config/zode
  ZODE_DATA := $(HOME)/Library/Application Support/Zode
  ZODE_STATE := $(HOME)/.local/state/Zode
  ZODE_CACHE := $(HOME)/Library/Caches/Zode
  # macOS is the one platform where logs do NOT live under the data directory.
  ZODE_LOGS := $(HOME)/Library/Logs/Zode
else
  ZODE_CONFIG := $(if $(XDG_CONFIG_HOME),$(XDG_CONFIG_HOME),$(HOME)/.config)/zode
  ZODE_DATA := $(if $(XDG_DATA_HOME),$(XDG_DATA_HOME),$(HOME)/.local/share)/zode
  ZODE_STATE := $(if $(XDG_STATE_HOME),$(XDG_STATE_HOME),$(HOME)/.local/state)/zode
  ZODE_CACHE := $(if $(XDG_CACHE_HOME),$(XDG_CACHE_HOME),$(HOME)/.cache)/zode
  # Elsewhere logs sit inside the data directory, so deleting that covers them.
  ZODE_LOGS :=
endif

# zode watches this directory at runtime (crates/zed/src/main.rs watch_themes)
# and reloads on any change — that is what makes theme edits rebuild-free.
THEME := assets/themes/vscode-2026/vscode-2026.json
THEMES_DIR := $(ZODE_CONFIG)/themes

.DEFAULT_GOAL := help
.PHONY: help build drivers dev fast run lint test clean trim theme-push theme-pull \
	paths reset-config reset-all onboarding bundle

help: ## Show this help
	@grep -hE '^[a-z][a-zA-Z_-]*:.*?## ' $(MAKEFILE_LIST) \
		| awk 'BEGIN {FS = ":.*?## "}; {printf "  \033[36m%-12s\033[0m %s\n", $$1, $$2}'

build: ## Compile the zode binary and the database drivers (PROFILE=dev, incremental)
	$(CARGO) build -p $(PACKAGE) --bin $(PACKAGE) --profile $(PROFILE)
	@$(MAKE) drivers PROFILE=$(PROFILE)

# The drivers are sidecar binaries outside `default-members`, so neither
# `cargo build -p zode` nor `cargo run` produces them — a fresh checkout has
# none, and zode looks for them beside its own executable. Without this every
# connection failed with `command not found: zode-db-postgres`, repeated by the
# reconnect loop, which reads as a broken database panel rather than as a
# missing build step. `script/bundle-*` already build them; only the dev loop
# did not.
drivers: ## Compile the database driver sidecars beside the zode binary
	$(CARGO) build --profile $(PROFILE) \
		-p zode-db-sqlite -p zode-db-postgres -p zode-db-mysql

dev: ## Launch the already-built binary — never recompiles (PROJECT= opens nothing)
	@test -x $(BIN) || { \
		echo "No binary at $(BIN) — run 'make build$(if $(filter dev,$(PROFILE)),, PROFILE=$(PROFILE))' first."; \
		exit 1; \
	}
	$(BIN) $(PROJECT)

fast: ## Build optimised and launch — use this when scrolling feels sluggish
	@$(MAKE) build PROFILE=release-fast
	@$(MAKE) dev PROFILE=release-fast

run: ## Compile if needed, then launch
	$(CARGO) run -p $(PACKAGE) --bin $(PACKAGE) -- $(PROJECT)

# macOS reads the app icon from the bundle's Info.plist, so `build` and `dev`
# can never show it -- they produce a bare executable and the Dock falls back to
# its generic "exec" tile. Only a bundle has an icon at all.
bundle: ## Build a macOS .app so the Dock icon exists, then open it
	script/bundle-mac -d -o

lint: ## Run the project's clippy gate (never plain `cargo clippy`)
	./script/clippy

test: ## Run the workspace test suite
	$(CARGO) test --workspace

clean: ## Remove the whole target directory
	$(CARGO) clean

trim: ## Drop target/ only if it grew past 30GB
	./script/clear-target-dir-if-larger-than 30

theme-push: ## Copy the bundled theme where a running zode will hot-reload it
	@mkdir -p $(THEMES_DIR)
	cp $(THEME) $(THEMES_DIR)/
	@echo "Live at $(THEMES_DIR)/$(notdir $(THEME)) — edits reload in a running zode."

theme-pull: ## Copy hot-reload edits back into the repo, then stop shadowing the bundle
	@test -f $(THEMES_DIR)/$(notdir $(THEME)) || { \
		echo "Nothing at $(THEMES_DIR)/$(notdir $(THEME)) — nothing to pull back."; \
		exit 1; \
	}
	cp $(THEMES_DIR)/$(notdir $(THEME)) $(THEME)
	rm $(THEMES_DIR)/$(notdir $(THEME))
	@echo "Pulled into $(THEME); the user-theme copy is gone, so the bundled theme wins again."

# --- Reset ------------------------------------------------------------------

# Refuses any path whose last component is not zode/Zode. `~/.config/zode` is one
# letter from `~/.config/zed`, and a reset that hit the wrong one would delete
# the settings of an editor this repo does not ship.
define confirm_and_delete
	set -e; \
	if [ -z "$$HOME" ]; then echo "HOME is unset — refusing to guess where to delete."; exit 1; fi; \
	for dir in $(1); do \
		case "$$(basename "$$dir")" in \
			zode|Zode) ;; \
			*) echo "refusing $$dir: not a zode directory"; exit 1 ;; \
		esac; \
	done; \
	found=0; \
	echo "About to delete:"; \
	for dir in $(1); do \
		if [ -e "$$dir" ]; then \
			found=1; \
			echo "  $$(du -sh "$$dir" 2>/dev/null | cut -f1)  $$dir"; \
		fi; \
	done; \
	if [ "$$found" = "0" ]; then echo "  (nothing — zode has no state here yet)"; exit 0; fi; \
	if pgrep -x zode >/dev/null 2>&1; then \
		echo; \
		echo "zode is running. Quit it first — it rewrites its state on exit,"; \
		echo "so anything deleted now would come straight back."; \
		exit 1; \
	fi; \
	if [ "$(FORCE)" != "1" ]; then \
		printf "\nType 'yes' to confirm: "; \
		read -r reply; \
		if [ "$$reply" != "yes" ]; then echo "Cancelled — nothing was deleted."; exit 1; fi; \
	fi; \
	for dir in $(1); do \
		if [ -e "$$dir" ]; then rm -rf "$$dir"; echo "  removed $$dir"; fi; \
	done; \
	echo "Done. The next launch starts from defaults."; \
	echo; \
	echo "To actually see that, launch with no folder:  make dev PROJECT="; \
	echo "Plain 'make dev' passes this repo as an argument, which opens it"; \
	echo "straight away and skips the first-run screen entirely."; \
	echo "'make onboarding' does both steps in one go."
endef

paths: ## Print where zode stores things (mirrors crates/paths/src/paths.rs)
	@echo "  config  $(ZODE_CONFIG)"
	@echo "  data    $(ZODE_DATA)"
	@echo "  state   $(ZODE_STATE)"
	@echo "  cache   $(ZODE_CACHE)"
	@$(if $(ZODE_LOGS),echo "  logs    $(ZODE_LOGS)",echo "  logs    (inside data)")

reset-config: ## Delete settings, keymap and themes — keeps extensions and languages
	@$(call confirm_and_delete,"$(ZODE_CONFIG)")

reset-all: ## Delete everything zode stores, as if it had never run
	@$(call confirm_and_delete,"$(ZODE_CONFIG)" "$(ZODE_DATA)" "$(ZODE_STATE)" "$(ZODE_CACHE)" $(if $(ZODE_LOGS),"$(ZODE_LOGS)"))

# Wiping state is only half of it: the first-run screen sits behind an `else if`
# in main.rs, so passing ANY path on the command line skips it. `make dev` passes
# this repo by default, which is why a reset alone looks like it did nothing.
onboarding: ## Reset everything, then launch straight into the first-run screen
	@$(MAKE) --no-print-directory reset-all
	@$(MAKE) --no-print-directory dev PROJECT=
