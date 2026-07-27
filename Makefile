CARGO   ?= cargo

# --- Install layout (override for packaging: make install DESTDIR=/pkg PREFIX=/usr) ---
PREFIX  ?= $(HOME)/.local
DESTDIR ?=
BINDIR  ?= $(PREFIX)/bin

CLI_PKG     := vtk6800-cli
CLI_BIN     := vtk6800
RELEASE_BIN := target/release/$(CLI_BIN)

# Extra args: `make run ARGS="keymap show"`, `make test TEST_ARGS=-- --nocapture`.
ARGS      ?=
TEST_ARGS ?= --workspace

.DEFAULT_GOAL := help

###############
# Build / Run #
###############
.PHONY: build
build: ## Build the CLI (debug)
	$(CARGO) build -p $(CLI_PKG)

.PHONY: release
release: ## Build the CLI (optimized)
	$(CARGO) build -p $(CLI_PKG) --release

.PHONY: run
run: ## Run the CLI: make run ARGS="keymap show"
	$(CARGO) run -p $(CLI_PKG) -- $(ARGS)

#########
# Tests #
#########
.PHONY: test
test: ## Run tests (workspace by default)
	$(CARGO) test $(TEST_ARGS)

.PHONY: check
check: ## Type-check without producing binaries
	$(CARGO) check --workspace --all-targets

.PHONY: fmt
fmt: ## Format all crates
	$(CARGO) fmt --all

.PHONY: fmt-check
fmt-check: ## Verify formatting (CI-friendly)
	$(CARGO) fmt --all --check

.PHONY: lint
lint: ## Clippy with warnings denied
	$(CARGO) clippy --workspace --all-targets -- -D warnings

.PHONY: doc
doc: ## Build API docs
	$(CARGO) doc --workspace --no-deps

# Aggregate gate: run before pushing.
.PHONY: verify
verify: fmt-check lint test ## Format check + lint + test

#######################
# Install / Uninstall #
#######################
.PHONY: install
install: release ## Install the CLI binary to $(BINDIR)
	install -Dm755 $(RELEASE_BIN) $(DESTDIR)$(BINDIR)/$(CLI_BIN)
	@echo "Installed $(CLI_BIN) to $(DESTDIR)$(BINDIR)/$(CLI_BIN)"

.PHONY: uninstall
uninstall: ## Remove the installed CLI binary
	rm -f $(DESTDIR)$(BINDIR)/$(CLI_BIN)

.PHONY: udev-install
udev-install: release ## Install the hidraw udev rule (may prompt for sudo)
	$(RELEASE_BIN) udev install

########
# Misc #
########
.PHONY: clean
clean: ## Remove build artifacts
	$(CARGO) clean

.PHONY: help
help: ## Show this help
	@grep -E '^[a-zA-Z_-]+:.*?## .*$$' $(MAKEFILE_LIST) \
		| awk 'BEGIN{FS=":.*?## "}{printf "  \033[36m%-14s\033[0m %s\n", $$1, $$2}'
