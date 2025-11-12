
COMMAND=cargo run -p mm1-multinode-tests --example multinode-example -- 

node-aa:
	$(COMMAND) --local '<aa:>/8' --bind '127.0.0.1:5551' --connect '127.0.0.1:5554' -r '<aaf:1>' -d '<bbf:1>' -d '<ccf:1>' -d '<ddf:1>'

node-bb:
	$(COMMAND) --local '<bb:>/8' --bind '127.0.0.1:5552' --connect '127.0.0.1:5551' -d '<aaf:1>' -r '<bbf:1>' -d '<ccf:1>' -d '<ddf:1>'

node-cc:
	$(COMMAND) --local '<cc:>/8' --bind '127.0.0.1:5553' --connect '127.0.0.1:5552' -d '<aaf:1>' -d '<bbf:1>' -r '<ccf:1>' -d '<ddf:1>'

node-dd:
	$(COMMAND) --local '<cc:>/8' --bind '127.0.0.1:5554' --connect '127.0.0.1:5553' -d '<aaf:1>' -d '<bbf:1>' -d '<ccf:1>' -r '<ddf:1>'
