SHELL := /bin/bash
.DEFAULT_GOAL := ci

RUST_DIR := rust

.PHONY: build test fmt fmt-check lint deny contract ci ci-gate-test test-no-leak knope-pin nextest-pin release-invariants knope-validate

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

nextest-pin:
	.github/scripts/nextest-pin.test.sh

knope-pin:
	.github/scripts/knope-pin.test.sh

release-invariants:
	.github/scripts/release-invariants.test.sh
	.github/scripts/release-invariants.sh .changeset $(RUST_DIR)/Cargo.toml "$$(git tag --sort=-v:refname | head -1)"

knope-validate:
	knope --validate

ci: fmt-check lint deny build test test-no-leak contract ci-gate-test nextest-pin knope-pin release-invariants
