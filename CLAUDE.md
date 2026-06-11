# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Overview

`cargo-ledger` is a `cargo` subcommand (cargo plugin) that builds embedded Rust apps for Ledger hardware devices and produces the artifacts needed to sideload them. It is invoked as `cargo ledger <command>`.

## Commands

```sh
cargo build                       # build the plugin itself
cargo clippy --no-deps            # lint (CI gate)
cargo fmt --all --check           # format check (CI gate); max_width = 80 (.rustfmt.toml)
cargo test --all --all-features   # run tests (CI gate)
```

Run a single test: `cargo test <test_name>`.

Note: the integration tests in `src/main.rs` are currently commented out and reference a `retrieve_metadata` function that no longer exists (see "Metadata source" below). CI runs `cargo test` but there are effectively no active tests at present.

### Using the plugin (end-user commands)

```sh
cargo ledger setup                # install custom target.json files + link_wrap.sh into the rustc sysroot
cargo ledger build <device>       # build an app for a device
cargo ledger build <device> -l    # build and load onto a connected device (--load)
```

Devices: `nanox`, `nanosplus`, `stax`, `flex`, `apex_p`.

Arguments after `--` are forwarded to the underlying `cargo build`, e.g.:
```sh
cargo ledger build nanosplus --load -- --features one -Z unstable-options --artifact-dir ./output/
```
A `--artifact-dir=<dir>` argument in the forwarded args triggers copying of the `.hex`, `.apdu`, and `.sha256` outputs into that directory.

## Architecture

The whole tool is a thin orchestrator around external command-line programs. There is no library crate; everything is a binary in `src/`.

- **`main.rs`** — clap CLI definition (`Cli` → `MainCommand::{Setup, Build}`) and the `build_app` pipeline.
- **`utils.rs`** — ELF inspection and the external-tool wrappers.
- **`setup.rs`** — `cargo ledger setup`: fetches per-device `target.json` files and the `link_wrap.sh` linker script from the `ledger-device-rust-sdk` repo via `curl`, into the `rustc --print sysroot` location.
- **`error.rs`** — `LedgerError` enum used everywhere. `CommandFailure { cmd, status, stderr }` is the standard way external-process failures are surfaced.

### The `build_app` pipeline (main.rs)

1. Spawn `cargo build --release --target=<device> --message-format=json-diagnostic-rendered-ansi`, parse the JSON message stream with `cargo_metadata::Message`, and capture the produced executable path (the ELF).
2. Resolve the package directory via `cargo_metadata::MetadataCommand`.
3. `export_binary` — run `arm-none-eabi-objcopy -O ihex` to produce the `.hex`, and `arm-none-eabi-size` for size info.
4. `retrieve_infos` — parse the ELF to extract app metadata (see below).
5. Adjust `app_flags`: for every device **except** `nanosplus`, OR in `0x200` to enable BLE.
6. `dump_with_ledgerblue` — run `python3 -m ledgerblue.loadApp --offline <app>.apdu` to produce the `.apdu`. Its stdout is also scanned for the `Application full hash :` line, whose hex value is written to a sibling `.sha256` file.
7. If `--load`, `install_with_ledgerblue` runs `python3 -m ledgerblue.runScript` to push the APDUs to the device.
8. If `--artifact-dir=` was passed, copy `.hex` / `.apdu` / `.sha256` there.

### Metadata source (important)

App metadata (`app_name`, `app_version`, `api_level`, `app_flags`, `target_id`, plus `data_size` / `install_params_size` computed from symbols) is read **from the compiled ELF's sections and symbols** in `retrieve_infos`, not from `[package.metadata.ledger]` in `Cargo.toml`. The SDK emits `ledger.<field>` sections (or legacy `.ledger.<field>` for Rust SDK ≤ 1.0.0). The `[package.metadata.ledger]` blocks in `tests/*/Cargo.toml` are fixtures for the old, now-removed Cargo-metadata-based approach.

## External dependencies (must be on PATH)

The tool shells out to: `cargo`, `rustc`, `arm-none-eabi-objcopy`, `arm-none-eabi-size`, `python3` with the `ledgerblue` module, `curl`, `find`, `chmod`. The objcopy/size binaries can be overridden via `CARGO_TARGET_THUMBV6M_NONE_EABI_OBJCOPY` and `CARGO_TARGET_THUMBV6M_NONE_EABI_SIZE`. CI runs inside the `ghcr.io/ledgerhq/ledger-app-builder/ledger-app-dev-tools` container, which provides these.

## Conventions

- Edition 2024, `max_width = 80`.
- PRs must be rebased on the target branch and contain no merge commits (enforced by a CI workflow); keep history linear.
- The CLI version string is hard-coded in the `#[clap(version = ...)]` attribute in `main.rs` and must be kept in sync with `Cargo.toml`'s `version`.
