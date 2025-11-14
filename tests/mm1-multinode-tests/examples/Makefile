
RELEASE=
COMMAND=cargo run $(RELEASE) -p mm1-multinode-tests --example multinode-example -- 
RM=rm -f
UNIX_SOCKET=target/example.sock

ring-4-aa:
	$(COMMAND) \
		--local '<aa:>/8' \
		--bind 'tcp://127.0.0.1:5551' \
		--connect 'tcp://127.0.0.1:5554' \
		--proto proto \
		-r '<aaf:1>' -d '<bbf:1>' -d '<ccf:1>' -d '<ddf:1>'

ring-4-bb:
	$(COMMAND) \
		--local '<bb:>/8' \
		--bind 'tcp://127.0.0.1:5552' \
		--connect 'tcp://127.0.0.1:5551' \
		--proto proto \
		-d '<aaf:1>' -r '<bbf:1>' -d '<ccf:1>' -d '<ddf:1>'

ring-4-cc:
	$(COMMAND) \
		--local '<cc:>/8' \
		--bind 'tcp://127.0.0.1:5553' \
		--connect 'tcp://127.0.0.1:5552' \
		--proto proto -d \
		'<aaf:1>' -d '<bbf:1>' -r '<ccf:1>' -d '<ddf:1>'

ring-4-dd:
	$(COMMAND) --local '<cc:>/8' --bind 'tcp://127.0.0.1:5554' --connect 'tcp://127.0.0.1:5553' --proto proto -d '<aaf:1>' -d '<bbf:1>' -d '<ccf:1>' -r '<ddf:1>'

star-center:
	$(RM) $(UNIX_SOCKET)
	$(COMMAND) \
		--local '<aa:>/8' -r '<aaf:1>' \
		--bind "uds://$(UNIX_SOCKET)"

star-bb: $(UNIX_SOCKET)
	$(COMMAND) \
		--local '<bb:>/8' -r '<bbf:1>' \
		--connect "uds://$(UNIX_SOCKET)" \
		-p proto \
		-d '<bbf:1>'  -d '<ccf:1>' -d '<ddf:1>'

star-cc: $(UNIX_SOCKET)
	$(COMMAND) \
		--local '<cc:>/8' -r '<ccf:1>' \
		--connect "uds://$(UNIX_SOCKET)" \
		-p proto \
		-d '<bbf:1>'  -d '<ccf:1>' -d '<ddf:1>'

star-dd: $(UNIX_SOCKET)
	$(COMMAND) \
		--local '<dd:>/8' -r '<ddf:1>' \
		--connect "uds://$(UNIX_SOCKET)" \
		-p proto \
		-d '<bbf:1>'  -d '<ccf:1>' -d '<ddf:1>'

