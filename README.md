# Cargo-ledger

Builds a Nano App and outputs a JSON manifest file that can be used by [ledgerctl](https://github.com/LedgerHQ/ledgerctl) to install an application directly.

In order to build for Nano S, Nano X, and Nano S Plus, [custom target files](https://docs.rust-embedded.org/embedonomicon/custom-target.html) are used. They can be found at the root of the [Rust SDK](https://github.com/LedgerHQ/ledger-nanos-sdk/).

## Installation

Only `arm-none-eabi-objcopy` is needed.

Install this repo with:

```
cargo install --git https://github.com/LedgerHQ/cargo-ledger
```

or download it manually and install with:

```
cargo install --path .
```

Note that `cargo`'s dependency resolver may behave differently when installing, and you may end up with errors.
In order to fix those and force usage of the versions specified in the tagged `Cargo.lock`, append `--locked` to the above commands.

### Setting up custom targets

The preferred method is to have all custom target files (`nanos.json`, `nanox.json` and `nanosplus.json`) in a separate folder, and set an environment variable called `LEDGER_TARGETS` pointing to this folder.

`cargo ledger` will check for this environment variable (or default to "" if it is empty) to fetch the current target specification.

## Usage


```
cargo ledger nanos
cargo ledger nanox
cargo ledger nanosplus
```

Loading can optionally be performed by appending `--load` or `-l` to the command.

By default, this program will attempt to build the current program with in `release` mode (full command: `cargo build -Zbuild-std -Zbuild-std-features=compiler-builtins-mem --release --target=nanos.json --message-format=json`)


Arguments can be passed to modify this behaviour after inserting a `--` like so:

```
cargo ledger nanos --load -- --features one -Z unstable-options --out-dir ./output/
```