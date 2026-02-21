.PHONY: fmt
fmt:
	cargo fmt --all
	git add -u
	cargo clippy --fix --allow-staged --all --all-targets

.PHONY: check-fmt
check-fmt:
	cargo fmt --check --all
	cargo clippy --all --all-targets
