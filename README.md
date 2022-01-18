# Cargo-ledger

Builds a NanoS App and outputs an `app.json` manifest file for [ledgerctl](https://github.com/LedgerHQ/ledgerctl)

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

## Usage

`cargo ledger`
`cargo ledger load`
