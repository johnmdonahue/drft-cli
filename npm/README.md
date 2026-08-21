# drft

A structural integrity checker for linked file systems.

This is the npm distribution of [drft](https://github.com/johnmdonahue/drft-cli). It downloads the prebuilt binary for your platform.

## Install

```bash
npm install drft-cli
```

## Usage

```bash
npx drft check
npx drft lock --all
npx drft graph
npx drft impact setup.md
```

See the [full documentation](https://github.com/johnmdonahue/drft-cli) for all commands and options.

## Supported platforms

- macOS (arm64, x64)
- Linux (arm64, x64)
- Windows (x64)

For other platforms, install from source: `cargo install drft-cli`
