# cargo-ledger
![Dynamic TOML Badge](https://img.shields.io/badge/dynamic/toml?url=https%3A%2F%2Fraw.githubusercontent.com%2FLedgerHQ%2Fcargo-ledger%2Frefs%2Fheads%2Fmain%2FCargo.toml&query=%24.package.version&label=version)

Builds a Ledger device embedded app and uses [ledgerblue](https://github.com/LedgerHQ/blue-loader-python) to generate the APDU file used to install the application directly on a device.

In order to build for Nano X, Nano S Plus, Stax, Flex and Apex P (Gen5) [custom target files](https://docs.rust-embedded.org/embedonomicon/custom-target.html) are used. They can be found at the root of the [ledger_secure_sdk_sys](https://github.com/LedgerHQ/ledger_secure_sdk_sys/) and can be installed automatically with the command `cargo ledger setup`.

## Installation

This program requires:

- `arm-none-eabi-objcopy`
- [`ledgerblue`](https://github.com/LedgerHQ/blue-loader-python) (installable with `pip install ledgerblue`)

Install this repo with:

```
cargo install --git https://github.com/LedgerHQ/cargo-ledger 
```

or download it manually and install with:

```
cargo install cargo-ledger
```

Note that `cargo`'s dependency resolver may behave differently when installing, and you may end up with errors.
In order to fix those and force usage of the versions specified in the tagged `Cargo.lock`, append `--locked` to the above commands.

## Usage

General usage is displayed when invoking `cargo ledger`.

### Setup

This will install custom target files from the SDK directly into your environment.

```
cargo ledger setup
```

### Building

```
cargo ledger build nanox
cargo ledger build nanosplus
cargo ledger build stax
cargo ledger build flex
cargo ledger build apex_p
```

Loading on device can optionally be performed by appending `--load` or `-l` to the command.

By default, this program will attempt to build the current program with in `release` mode (full command: `cargo build --release --target=nanosplus --message-format=json`)

Arguments can be passed to modify this behaviour after inserting a `--` like so:

```
cargo ledger build nanosplus --load -- --features one --artifact-dir=./output/
```

#### Build outputs

During the build, `ledgerblue` is invoked to generate an `.apdu` file next to the
application ELF. In addition, the application hash reported by `ledgerblue`
(the `Application full hash` line) is extracted and stored in a `.sha256` file
alongside the `.apdu` file (e.g. `myapp.apdu` and `myapp.sha256`).

When an artifact directory is provided (`--artifact-dir=<dir>`), the `.hex`,
`.apdu` and `.sha256` files are also copied there.