
RELEASE=
COMMAND=cargo run $(RELEASE) -p mm1-multinode-tests --example multinode-example -- 
RM=rm -f
UNIX_SOCKET_1=target/example-1.sock
UNIX_SOCKET_2=target/example-2.sock
TICK_INTERVAL=1s

ring-4-aa:
	$(COMMAND) \
		--tick $(TICK_INTERVAL) \
		--local '<aa:>/8' \
		--bind 'tcp://127.0.0.1:5551' \
		--connect 'tcp://127.0.0.1:5554' \
		--proto proto \
		-r '<aaf:1>' -d '<bbf:1>' -d '<ccf:1>' -d '<ddf:1>'

ring-4-bb:
	$(COMMAND) \
		--tick $(TICK_INTERVAL) \
		--local '<bb:>/8' \
		--bind 'tcp://127.0.0.1:5552' \
		--connect 'tcp://127.0.0.1:5551' \
		--proto proto \
		-d '<aaf:1>' -r '<bbf:1>' -d '<ccf:1>' -d '<ddf:1>'

ring-4-cc:
	$(COMMAND) \
		--tick $(TICK_INTERVAL) \
		--local '<cc:>/8' \
		--bind 'tcp://127.0.0.1:5553' \
		--connect 'tcp://127.0.0.1:5552' \
		--proto proto -d \
		'<aaf:1>' -d '<bbf:1>' -r '<ccf:1>' -d '<ddf:1>'

ring-4-dd:
	$(COMMAND) --local '<cc:>/8' --bind 'tcp://127.0.0.1:5554' --connect 'tcp://127.0.0.1:5553' --proto proto -d '<aaf:1>' -d '<bbf:1>' -d '<ccf:1>' -r '<ddf:1>'

star-center-1:
	$(RM) $(UNIX_SOCKET_1)
	$(COMMAND) \
		--tick $(TICK_INTERVAL) \
		--local '<aa0:>/12' -r '<aa0f:1>' \
		--bind "uds://$(UNIX_SOCKET_1)"

star-center-2:
	$(RM) $(UNIX_SOCKET_2)
	$(COMMAND) \
		--tick $(TICK_INTERVAL) \
		--local '<aa1:>/12' -r '<aa1f:1>' \
		--bind "uds://$(UNIX_SOCKET_2)"


star-bb: $(UNIX_SOCKET)
	$(COMMAND) \
		--tick $(TICK_INTERVAL) \
		--local '<bb:>/8' -r '<bbf:1>' \
		--connect "uds://$(UNIX_SOCKET_1)" \
		--connect "uds://$(UNIX_SOCKET_2)" \
		-p proto \
		-d '<bbf:1>'  -d '<ccf:1>' -d '<ddf:1>'

star-cc: $(UNIX_SOCKET)
	$(COMMAND) \
		--tick $(TICK_INTERVAL) \
		--local '<cc:>/8' -r '<ccf:1>' \
		--connect "uds://$(UNIX_SOCKET_1)" \
		--connect "uds://$(UNIX_SOCKET_2)" \
		-p proto \
		-d '<bbf:1>'  -d '<ccf:1>' -d '<ddf:1>'

star-dd: $(UNIX_SOCKET)
	$(COMMAND) \
		--tick $(TICK_INTERVAL) \
		--local '<dd:>/8' -r '<ddf:1>' \
		--connect "uds://$(UNIX_SOCKET_1)" \
		--connect "uds://$(UNIX_SOCKET_2)" \
		-p proto \
		-d '<bbf:1>'  -d '<ccf:1>' -d '<ddf:1>'

