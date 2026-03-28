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

```
src/
  main.rs          -- entry point, command wiring
  cli.rs           -- clap argument definitions
  config.rs        -- drft.toml loading and defaults
  discovery.rs     -- file walking, .gitignore respect
  parsing.rs       -- markdown link extraction
  graph.rs         -- dependency graph construction
  lockfile.rs      -- drft.lock serialization
  diagnostic.rs    -- output formatting (text, json, color)
  rules/
    mod.rs         -- rule trait and registry
    broken_link.rs
    containment.rs
    custom.rs      -- external script rules
    cycle.rs
    directory_link.rs
    encapsulation.rs
    indirect_link.rs
    orphan.rs
    stale.rs
```

## Testing

Unit tests are inline (`#[cfg(test)]` modules). Integration tests are in `tests/scenarios.rs` and run the binary as a subprocess against temp directories.

```bash
cargo test                    # all tests
cargo test scenario_5         # specific test
```

## Code style

- `cargo fmt` before committing
- `cargo clippy -- -D warnings` should pass
- One module per concern
- Diagnostics to stdout, errors to stderr
- Exit codes: 0 clean, 1 violations, 2 usage error

## Examples

The `examples/` directory has sample projects for manual testing:
- `simple/` -- clean project
- `broken/` -- various violations
- `cyclic/` -- circular dependencies
- `monorepo/` -- nested scopes
- `with-assets/` -- non-markdown references
- `with-config/` -- ignore patterns
- `custom-rules/` -- external script rules
