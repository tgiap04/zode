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
PROJECT ?= .

# Trims several GB of debug info while keeping backtraces readable.
export CARGO_PROFILE_DEV_DEBUG ?= line-tables-only
export CARGO_PROFILE_RELEASE_FAST_DEBUG ?= line-tables-only

# zode watches this directory at runtime (crates/zed/src/main.rs watch_themes)
# and reloads on any change — that is what makes theme edits rebuild-free.
THEME := assets/themes/vscode-2026/vscode-2026.json
THEMES_DIR := $(HOME)/.config/zed/themes

.DEFAULT_GOAL := help
.PHONY: help build dev fast run lint test clean trim theme-push theme-pull

help: ## Show this help
	@grep -hE '^[a-z][a-zA-Z_-]*:.*?## ' $(MAKEFILE_LIST) \
		| awk 'BEGIN {FS = ":.*?## "}; {printf "  \033[36m%-12s\033[0m %s\n", $$1, $$2}'

build: ## Compile the zode binary (PROFILE=dev by default, incremental)
	$(CARGO) build -p $(PACKAGE) --bin $(PACKAGE) --profile $(PROFILE)

dev: ## Launch the already-built binary — never recompiles
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
