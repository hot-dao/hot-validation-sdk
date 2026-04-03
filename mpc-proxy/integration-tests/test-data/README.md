# Integration Test Data

This directory should contain the following files for running integration tests:

- `secrets-config.yml` - Configuration file (see docker-compose.yml for structure)
- `enc.key` - Encryption key (base58 encoded)
- `rpc_config.yaml` - RPC endpoint configuration
- `cluster-config.yml` - MPC cluster configuration

## Quick Start

```bash
cd mpc-proxy/integration-tests
docker compose up
```

## Notes

- The `secrets-config.yml.enc` file is the encrypted version of the config
- Decryption key should be provided via `ENCRYPTION_KEY_PATH` environment variable
- See `docker-compose.yml` for the full list of required environment variables
