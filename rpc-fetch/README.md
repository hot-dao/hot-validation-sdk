# Overview

This is an auxiliary script that collects RPC endpoints from specified providers. Respective API keys are expected as
the input.
As the output we get a config with collected servers alongside with a `threshold` field, which defines a number of
servers' responses needed to build consensus. This is a user-defined value.