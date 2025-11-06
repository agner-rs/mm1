
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
	cargo clean
	cargo check --all-targets --no-default-features -p $@

mm1-core-tests:
	cargo clean
	cargo check --all-targets --no-default-features -p $@

mm1-multinode-tests:
	cargo clean
	cargo check --all-targets --no-default-features -p $@

mm1-name-service-tests:
	cargo clean
	cargo check --all-targets --no-default-features -p $@

mm1-node-tests:
	cargo clean
	cargo check --all-targets --no-default-features -p $@

mm1-sup-tests:
	cargo clean
	cargo check --all-targets --no-default-features -p $@

mm1-test-rt-tests:
	cargo clean
	cargo check --all-targets --no-default-features -p $@

mm1-timer-tests:
	cargo clean
	cargo check --all-targets --no-default-features -p $@





