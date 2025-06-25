# Overivew 

This repose contains a library which is used to authorize signature generation on the MPC side.
Currently it's a wrapper around RPC calls to a different blockchains.

Usual data flow would be making a single call to a NEAR blockchain, then calling a target chain.

# Context

The message can be signed if there's a "proof." For example, we want to sign a message "Record a deposit of 5 XLM"
which is sent to the Hot Bridge.
The proof would be a transaction identifier – unique nonce. Then we check if a transaction with this
exact nonce and parameters took place. 

In the code this proof and proof verification implemented through quite abstract `hot_verify` methods in smart contract.
