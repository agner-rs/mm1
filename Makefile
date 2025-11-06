
mm1-timer-tests:
	cargo clean
	cargo check --examples --no-default-features -p mm1-timer-tests
	cargo nextest run --all-targets --no-default-features -p mm1-timer-tests


