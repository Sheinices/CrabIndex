# CrabIndex - build interface.
#
# Usage:
#   make help
#   make build            # debug build of the server
#   make release          # optimized binary → target/release/crabindex (+ wwwroot/)
#   make test             # Rust tests
#   make web              # site + admin panel + docs → wwwroot/
#   make docs             # documentation (Docusaurus) → wwwroot/docs

.DEFAULT_GOAL := help

CARGO        ?= cargo
DOCKER_IMAGE ?= crabindex
# Optional cross target, e.g. TARGET=aarch64-unknown-linux-gnu
TARGET       ?=
WEB_SCRIPT   := ./scripts/build-web-ui.sh

TARGET_FLAG  := $(if $(TARGET),--target $(TARGET),)
RELEASE_DIR  := target/$(if $(TARGET),$(TARGET)/,)release

.PHONY: help build release test check fmt clippy run web docs docs-serve dev-web dev-admin test-web lint-web dist docker clean clean-all

help: ## Show this help
	@printf 'CrabIndex build targets\n\n'
	@printf 'Usage:\n  make <target> [VARIABLE=value]\n\nTargets:\n'
	@awk 'BEGIN {FS = ":.*## "}; /^[a-zA-Z0-9_.-]+:.*?## / {printf "  %-12s %s\n", $$1, $$2}' $(MAKEFILE_LIST)

build: ## Debug build of the server binary
	$(CARGO) build -p crabindex $(TARGET_FLAG)

release: ## Optimized server binary (TARGET= optional)
	$(CARGO) build --release --locked -p crabindex $(TARGET_FLAG)
	@echo "Binary: $(RELEASE_DIR)/crabindex"

test: ## Run all Rust tests
	$(CARGO) test --workspace

check: ## Type-check the workspace
	$(CARGO) check --workspace --all-targets

fmt: ## Format Rust sources
	$(CARGO) fmt --all

clippy: ## Lint Rust sources
	$(CARGO) clippy --workspace --all-targets

run: ## Run the server from the repository root
	$(CARGO) run -p crabindex

web: ## Build the site (web/), admin panel (admin/) and docs into wwwroot/
	$(WEB_SCRIPT)

docs: ## Build documentation (Docusaurus) into wwwroot/docs, served at /docs/
	cd docs && (test -d node_modules || npm ci) && npm run build

docs-serve: ## Live-preview the documentation on http://localhost:3001/docs/
	cd docs && (test -d node_modules || npm ci) && npm start

dev-web: ## Start Vite dev server (web/)
	cd web && npm run dev

dev-admin: ## Start admin panel dev server with a mock API (admin/)
	cd admin && npm run dev

test-web: ## Run web and admin unit tests
	cd web && npm test
	cd admin && npm test

lint-web: ## Lint web/ and admin/
	cd web && npm run lint
	cd admin && npm run lint

dist: web release ## Release bundle in dist/: binary + wwwroot + Data templates
	rm -rf dist && mkdir -p dist/Data
	cp $(RELEASE_DIR)/crabindex dist/
	cp -a wwwroot dist/wwwroot
	cp Data/example.yaml Data/example.conf Data/crontab Data/run-job.sh dist/Data/
	install -m 755 scripts/install.sh dist/install.sh
	@echo "Bundle ready in dist/"

docker: ## Build Docker image ($(DOCKER_IMAGE))
	docker build -t $(DOCKER_IMAGE) .

clean: ## Remove build artifacts (target, wwwroot, dist, web/dist)
	$(CARGO) clean
	rm -rf wwwroot web/dist dist docs/.docusaurus docs/build

clean-all: clean ## clean + remove node_modules
	rm -rf web/node_modules admin/node_modules docs/node_modules
