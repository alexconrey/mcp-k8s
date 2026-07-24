.PHONY: build test fmt clippy check docker docs docs-serve deploy clean run run-http lint setup deny hooks

# Build release binary
build:
	cargo build --release

# Run all tests
test:
	cargo test

# Format code
fmt:
	cargo fmt

# Run clippy
clippy:
	cargo clippy --all-targets -- -D warnings

# Run all checks (fmt, clippy, test) — same as CI
check: fmt clippy test

# Build Docker image
docker:
	docker build -t mcp-k8s:local .

# Build documentation
docs:
	mdbook build docs/

# Serve docs locally with hot reload
docs-serve:
	mdbook serve docs/

# Deploy to k3d (assumes k3d-deckwatch context)
deploy: docker
	k3d image import mcp-k8s:local -c deckwatch
	kubectl --context k3d-deckwatch -n mcp-k8s rollout restart deployment/mcp-k8s

# Clean build artifacts
clean:
	cargo clean
	rm -rf docs/book

# Run the server locally in stdio mode
run:
	cargo run --release

# Run the server locally in HTTP mode
run-http:
	cargo run --release -- --http

# Lint and format check (no modifications)
lint:
	cargo fmt --check
	cargo clippy --all-targets -- -D warnings

# Install development tools
setup:
	cargo install mdbook
	@echo "Development tools installed"

# Audit licenses and vulnerabilities (requires cargo-deny)
deny:
	cargo deny check

# Install git pre-commit hook
hooks:
	@echo '#!/bin/sh' > .git/hooks/pre-commit
	@echo 'cargo fmt --check && cargo clippy --all-targets -- -D warnings' >> .git/hooks/pre-commit
	@chmod +x .git/hooks/pre-commit
	@echo "Pre-commit hook installed"
