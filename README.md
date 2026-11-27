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

This installs the custom target files and the `link_wrap.sh` linker script into the sysroot of the active Rust toolchain.

```
cargo ledger setup
```

By default they are taken from the latest release of the [`ledger_secure_sdk_sys`](https://crates.io/crates/ledger_secure_sdk_sys) crate on crates.io; the downloaded crate is checked against the checksum published in the crates.io index. Other sources:

```
# A specific ledger_secure_sdk_sys release (recommended for reproducible environments such as Docker images)
cargo ledger setup --sys-version 1.18.1

# A git tag or branch of the Rust SDK repository
cargo ledger setup -t <git-ref>
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

#### Built-in Rust triples

By default, apps are built with the custom target spec of the device
(`--target=<device>`). An app can instead be built with the built-in Rust
triple of the device's CPU, which needs no custom target spec:

| Device | Triple |
|---|---|
| `nanox` | `thumbv6m-none-eabi` |
| `nanosplus`, `stax`, `flex`, `apex_p` | `thumbv8m.main-none-eabi` |

It requires `ledger_device_sdk` 1.47.0 or later, and opts in from the app's
`Cargo.toml`:

```toml
[package.metadata.ledger]
builtin_target = true
# Optional per-device rustflags (the triple is shared by several devices,
# so [target.<device>] sections of .cargo/config.toml do not apply)
nanox = { icon = "...", rustflags = ["-Copt-level=s"] }
```

`cargo ledger` then passes the settings of the custom spec and the
`ledger_device` cfg as `build.rustflags` (`--config`), added to the app's own
`[build] rustflags`:

```
-Crelocation-model=ropi-rwpi -Clinker=link_wrap.sh
-Clink-arg=-T<device>_layout.ld -Clink-arg=-Tlink.ld
-Clink-arg=--wrap=__stack_chk_fail -Clink-arg=--wrap=__stack_chk_init
--cfg=ledger_device="<device>"
```

- On a built-in triple, `target_os` is `none`: select device code with
  `#[cfg(ledger_device = "...")]` (see the SDK README).
- `RUSTFLAGS`, or a `[target.<triple>]` / `[target.'cfg(..)']` rustflags
  section, replaces `build.rustflags`: the SDK build scripts then stop with an
  explanation.
- Each device builds in `target/builtin/<device>/`, as devices share triples;
  the ELF is copied to `target/<device>/release/`, where the `.hex`, `.apdu`
  and `.sha256` files are generated as usual.
- `link_wrap.sh` is still needed: `cargo ledger setup` installs it.

#### Build outputs

During the build, `ledgerblue` is invoked to generate an `.apdu` file next to the
application ELF. In addition, the application hash reported by `ledgerblue`
(the `Application full hash` line) is extracted and stored in a `.sha256` file
alongside the `.apdu` file (e.g. `myapp.apdu` and `myapp.sha256`).

When an artifact directory is provided (`--artifact-dir=<dir>`), the `.hex`,
`.apdu` and `.sha256` files are also copied there.

### Linting and checking

```
cargo ledger clippy nanox
cargo ledger check stax
```

`cargo ledger clippy <device>` and `cargo ledger check <device>` run
`cargo clippy` and `cargo check` with the same target and flags as
`cargo ledger build <device>`: the custom target spec of the device
(`--target=<device>`), or its built-in triple with the settings above when the
app opts in with `builtin_target`. Opted-in apps are then linted as they are
built, with no `[target.<device>]` section passing the `ledger_device` cfg.

Arguments after `--` go to cargo; a second `--` passes options to clippy
itself:

```
cargo ledger clippy nanosplus -- --features one -- -Dwarnings
```

They do not use `--release` unless it is passed after `--`.
