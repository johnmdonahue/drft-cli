# Contributing to drft

## Development setup

```bash
# Clone the repo
git clone https://github.com/johnmdonahue/drft-cli.git
cd drft-cli

# Build
cargo build

# Run tests
cargo test

# Run the binary
cargo run -- check -C examples/simple
```

## Project structure

See [ARCHITECTURE.md](ARCHITECTURE.md) for the module layout and design.

## Testing

Unit tests are inline (`#[cfg(test)]` modules). Integration tests are in [`tests/`](tests/README.md) and run the binary as a subprocess against temp directories.

```bash
cargo test                    # all tests
cargo test scenario_5         # specific test
```

## Code style

- Run `cargo fmt` before committing
- Run `cargo clippy -- -D warnings` (must pass cleanly)
- Keep one module per concern
- Write diagnostics to stdout, errors to stderr
- Use exit codes: 0 clean, 1 violations, 2 usage error

## Examples

See the [examples](examples/README.md) for sample projects used in manual testing.
