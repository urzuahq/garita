SHELL := /bin/bash
.DEFAULT_GOAL := ci

RUST_DIR := rust

.PHONY: build test fmt fmt-check lint deny contract ci ci-gate-test test-no-leak release-invariants release-guard knope-validate install-script-test

build:
	cd $(RUST_DIR) && cargo build --workspace --release --locked

test:
	.github/scripts/ci-tests.sh

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

ci-gate-test:
	.github/scripts/ci-gate.test.sh

test-no-leak:
	.github/scripts/test-no-leak.test.sh

release-invariants:
	.github/scripts/release-invariants.test.sh
	.github/scripts/release-invariants.sh .changeset $(RUST_DIR)/Cargo.toml "$$(git tag --sort=-v:refname | head -1)"

release-guard:
	.github/scripts/release-guard.test.sh

knope-validate:
	knope --validate

install-script-test:
	scripts/install-test.sh

ci: fmt-check lint deny build test test-no-leak contract ci-gate-test release-invariants release-guard install-script-test
