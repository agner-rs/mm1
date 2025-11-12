
all:
	$(MAKE) mm1-ask-tests
	$(MAKE) mm1-core-tests
	$(MAKE) mm1-multinode-tests
	$(MAKE) mm1-name-service-tests
	$(MAKE) mm1-node-tests
	$(MAKE) mm1-sup-tests
	$(MAKE) mm1-test-rt-tests
	$(MAKE) mm1-timer-tests


mm1-ask-tests:
	CARGO_TARGET_DIR=target/$@ cargo clean
	CARGO_TARGET_DIR=target/$@ cargo check --all-targets --no-default-features -p $@

mm1-core-tests:
	CARGO_TARGET_DIR=target/$@ cargo clean
	CARGO_TARGET_DIR=target/$@ cargo check --all-targets --no-default-features -p $@

mm1-multinode-tests:
	CARGO_TARGET_DIR=target/$@ cargo clean
	CARGO_TARGET_DIR=target/$@ cargo check --all-targets --no-default-features -p $@

mm1-name-service-tests:
	CARGO_TARGET_DIR=target/$@ cargo clean
	CARGO_TARGET_DIR=target/$@ cargo check --all-targets --no-default-features -p $@

mm1-node-tests:
	CARGO_TARGET_DIR=target/$@ cargo clean
	CARGO_TARGET_DIR=target/$@ cargo check --all-targets --no-default-features -p $@

mm1-sup-tests:
	CARGO_TARGET_DIR=target/$@ cargo clean
	CARGO_TARGET_DIR=target/$@ cargo check --all-targets --no-default-features -p $@

mm1-test-rt-tests:
	CARGO_TARGET_DIR=target/$@ cargo clean
	CARGO_TARGET_DIR=target/$@ cargo check --all-targets --no-default-features -p $@

mm1-timer-tests:
	CARGO_TARGET_DIR=target/$@ cargo clean
	CARGO_TARGET_DIR=target/$@ cargo check --all-targets --no-default-features -p $@





