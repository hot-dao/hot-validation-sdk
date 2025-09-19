# Overview

This repo contains logic that authorizes for message signing based on supplied proof. Proof is being tested against on-chain 
defined authorization methods. 

# Tests

Some tests require accessing RPC endpoints. By default public servers are used which may fail due to rate
limiting/ip-blocking.
For that case you can supply your own RPC endpoints in `.env`. You can use a template for that `cp .env.template .env`,
and
fill in required servers.