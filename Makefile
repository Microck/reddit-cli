.PHONY: fmt lint test check release-dry docs

fmt:
	cargo fmt --check

lint:
	cargo clippy --locked --all-targets -- -D warnings

test:
	cargo test --locked

check: fmt lint test

release-dry: check
	cargo publish --dry-run --locked --offline
	cd npm && npm pack --dry-run --ignore-scripts --offline

docs:
	cargo build --locked --bin reddit
	python3 scripts/gen-docs-reference.py "$(or $(CARGO_TARGET_DIR),target)/debug/reddit"
