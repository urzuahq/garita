SHELL := /bin/bash
.DEFAULT_GOAL := ci

RUST_DIR := rust

.PHONY: build test fmt fmt-check lint deny contract ci

build:
	cd $(RUST_DIR) && cargo build --workspace --release --locked

test:
	cd $(RUST_DIR) && cargo test --workspace --locked

fmt:
	cd $(RUST_DIR) && cargo fmt --all

fmt-check:
	cd $(RUST_DIR) && cargo fmt --all -- --check

lint:
	cd $(RUST_DIR) && cargo clippy --workspace --all-targets --locked -- -D warnings

deny:
	cd $(RUST_DIR) && cargo machete && cargo deny check

contract:
	scripts/contract-check.sh

ci: fmt-check lint deny build test contract
