use crate::error::LedgerError;
use cargo_metadata::semver::Version;
use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Custom targets installed by `cargo ledger setup`.
const TARGETS: [&str; 5] = ["nanox", "nanosplus", "stax", "flex", "apex_p"];

/// Linker wrapper referenced by the target specs (`"linker": "link_wrap.sh"`).
const LINK_SCRIPT: &str = "link_wrap.sh";

/// Crate shipping the target specs and the linker wrapper.
const SYS_CRATE: &str = "ledger_secure_sdk_sys";

/// Sparse index entry of `ledger_secure_sdk_sys`
/// (path layout: <https://doc.rust-lang.org/cargo/reference/registry-index.html#index-files>).
const SYS_CRATE_INDEX_URL: &str =
    "https://index.crates.io/le/dg/ledger_secure_sdk_sys";

/// Where the target specs and the linker wrapper are taken from.
pub enum TargetSource {
    /// A git tag or branch of the Rust SDK repository (legacy behaviour).
    GitRef(String),
    /// A published `ledger_secure_sdk_sys` crate: the given version, or the
    /// latest non-yanked release when `None`.
    CratesIo(Option<String>),
}

pub fn install_targets(source: TargetSource) -> Result<(), LedgerError> {
    println!("[ ] Install custom targets...");
    let sysroot = rustc_sysroot()?;
    let rustlib = Path::new(&sysroot).join("lib").join("rustlib");
    // link_wrap.sh must sit next to rust-lld.
    let link_script_path = rust_lld_dir(&sysroot)?.join(LINK_SCRIPT);

    match source {
        TargetSource::GitRef(git_ref) => {
            install_from_git_ref(&git_ref, &rustlib, &link_script_path)?
        }
        TargetSource::CratesIo(version) => install_from_crates_io(
            version.as_deref(),
            &rustlib,
            &link_script_path,
        )?,
    }

    println!("* Custom link script is {}", link_script_path.display());
    make_executable(&link_script_path)
}

fn install_from_git_ref(
    git_ref: &str,
    rustlib: &Path,
    link_script_path: &Path,
) -> Result<(), LedgerError> {
    let base_url = format!(
        "https://raw.githubusercontent.com/LedgerHQ/ledger-device-rust-sdk/{git_ref}/ledger_secure_sdk_sys"
    );
    println!("* Source: git ref {git_ref}");
    for target in TARGETS {
        let dest = rustlib.join(target).join("target.json");
        print_adding(target, &dest);
        curl(
            &format!("{base_url}/devices/{target}/{target}.json"),
            &dest,
            true,
        )?;
    }
    println!("[ ] Install custom link script...");
    curl(
        &format!("{base_url}/{LINK_SCRIPT}"),
        link_script_path,
        false,
    )
}

fn install_from_crates_io(
    version: Option<&str>,
    rustlib: &Path,
    link_script_path: &Path,
) -> Result<(), LedgerError> {
    let work_dir = TempDir::new()?;

    let index_path = work_dir.path().join("index");
    curl(SYS_CRATE_INDEX_URL, &index_path, false)?;
    let index = fs::read_to_string(&index_path)?;
    let release = select_release(&index, version)?;
    println!("* Source: {SYS_CRATE} {} (crates.io)", release.version);

    let crate_name = format!("{SYS_CRATE}-{}", release.version);
    let crate_path = work_dir.path().join(format!("{crate_name}.crate"));
    curl(
        &format!(
            "https://static.crates.io/crates/{SYS_CRATE}/{crate_name}.crate"
        ),
        &crate_path,
        false,
    )?;
    verify_sha256(&crate_path, &release.cksum)?;

    // Extract only the files we need, by exact path, from the verified archive.
    let mut members: Vec<String> = TARGETS
        .iter()
        .map(|t| format!("{crate_name}/devices/{t}/{t}.json"))
        .collect();
    members.push(format!("{crate_name}/{LINK_SCRIPT}"));
    let tar_out = Command::new("tar")
        .arg("-xzf")
        .arg(&crate_path)
        .arg("-C")
        .arg(work_dir.path())
        .args(&members)
        .output()?;
    if !tar_out.status.success() {
        return Err(LedgerError::CommandFailure {
            cmd: "tar",
            status: tar_out.status.code(),
            stderr: String::from_utf8_lossy(&tar_out.stderr).into(),
        });
    }

    let extracted = work_dir.path().join(&crate_name);
    for target in TARGETS {
        let dest = rustlib.join(target).join("target.json");
        print_adding(target, &dest);
        fs::create_dir_all(rustlib.join(target))?;
        fs::copy(
            extracted
                .join("devices")
                .join(target)
                .join(format!("{target}.json")),
            &dest,
        )?;
    }
    println!("[ ] Install custom link script...");
    fs::copy(extracted.join(LINK_SCRIPT), link_script_path)?;
    Ok(())
}

#[derive(Debug, PartialEq)]
struct Release {
    version: Version,
    cksum: String,
}

/// Picks a release from the sparse index content (one JSON object per line):
/// the requested version, or the highest non-yanked, non-pre-release one.
fn select_release(
    index: &str,
    requested: Option<&str>,
) -> Result<Release, LedgerError> {
    let mut releases = Vec::new();
    for line in index.lines().filter(|l| !l.trim().is_empty()) {
        let entry: serde_json::Value =
            serde_json::from_str(line).map_err(|e| {
                LedgerError::Other(format!(
                    "invalid crates.io index entry: {e}"
                ))
            })?;
        let (Some(vers), Some(cksum)) =
            (entry["vers"].as_str(), entry["cksum"].as_str())
        else {
            return Err(LedgerError::MissingField("vers/cksum"));
        };
        let version = Version::parse(vers).map_err(|e| {
            LedgerError::Other(format!("invalid version {vers} in index: {e}"))
        })?;
        let yanked = entry["yanked"].as_bool().unwrap_or(false);
        releases.push((version, cksum.to_string(), yanked));
    }

    match requested {
        Some(req) => {
            let req = Version::parse(req).map_err(|e| {
                LedgerError::Other(format!("invalid --sys-version {req}: {e}"))
            })?;
            let (version, cksum, yanked) = releases
                .into_iter()
                .find(|(v, _, _)| *v == req)
                .ok_or_else(|| {
                    LedgerError::Other(format!(
                        "{SYS_CRATE} {req} is not published on crates.io"
                    ))
                })?;
            if yanked {
                eprintln!("Warning: {SYS_CRATE} {version} is yanked");
            }
            Ok(Release { version, cksum })
        }
        None => releases
            .into_iter()
            .filter(|(v, _, yanked)| !yanked && v.pre.is_empty())
            .max_by(|a, b| a.0.cmp(&b.0))
            .map(|(version, cksum, _)| Release { version, cksum })
            .ok_or_else(|| {
                LedgerError::Other(format!(
                    "no non-yanked release of {SYS_CRATE} on crates.io"
                ))
            }),
    }
}

fn verify_sha256(path: &Path, expected: &str) -> Result<(), LedgerError> {
    let actual = Sha256::digest(fs::read(path)?)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect::<String>();
    if !actual.eq_ignore_ascii_case(expected) {
        return Err(LedgerError::Other(format!(
            "checksum mismatch for {}: expected {expected}, got {actual}",
            path.display()
        )));
    }
    Ok(())
}

fn rustc_sysroot() -> Result<String, LedgerError> {
    let out = Command::new("rustc")
        .args(["--print", "sysroot"])
        .output()?;
    if !out.status.success() {
        return Err(LedgerError::CommandFailure {
            cmd: "rustc",
            status: out.status.code(),
            stderr: String::from_utf8_lossy(&out.stderr).into(),
        });
    }
    Ok(std::str::from_utf8(&out.stdout)
        .map_err(|e| LedgerError::Other(format!("utf8 sysroot error: {e}")))?
        .trim()
        .to_string())
}

fn rust_lld_dir(sysroot: &str) -> Result<PathBuf, LedgerError> {
    let out = Command::new("find")
        .args([sysroot, "-name", "rust-lld"])
        .output()?;
    if !out.status.success() {
        return Err(LedgerError::CommandFailure {
            cmd: "find",
            status: out.status.code(),
            stderr: String::from_utf8_lossy(&out.stderr).into(),
        });
    }
    let stdout = std::str::from_utf8(&out.stdout).map_err(|e| {
        LedgerError::Other(format!("utf8 rust-lld path error: {e}"))
    })?;
    stdout
        .lines()
        .next()
        .and_then(|p| Path::new(p).parent())
        .map(Path::to_path_buf)
        .ok_or(LedgerError::Other(
            "Could not determine rust-lld directory".into(),
        ))
}

fn curl(url: &str, dest: &Path, create_dirs: bool) -> Result<(), LedgerError> {
    let mut cmd = Command::new("curl");
    cmd.args([
        "--fail",
        "--silent",
        "--show-error",
        "--location",
        "--retry",
        "3",
        "--retry-delay",
        "2",
        "--retry-all-errors",
    ])
    .arg(url)
    .arg("-o")
    .arg(dest);
    if create_dirs {
        cmd.arg("--create-dirs");
    }
    let out = cmd.output()?;
    if !out.status.success() {
        return Err(LedgerError::CommandFailure {
            cmd: "curl",
            status: out.status.code(),
            stderr: String::from_utf8_lossy(&out.stderr).into(),
        });
    }
    Ok(())
}

fn make_executable(path: &Path) -> Result<(), LedgerError> {
    let out = Command::new("chmod").arg("+x").arg(path).output()?;
    if !out.status.success() {
        return Err(LedgerError::CommandFailure {
            cmd: "chmod",
            status: out.status.code(),
            stderr: String::from_utf8_lossy(&out.stderr).into(),
        });
    }
    Ok(())
}

fn print_adding(target: &str, dest: &Path) {
    println!(
        "* Adding \x1b[1;32m{target}\x1b[0m in \x1b[1;33m{}\x1b[0m",
        dest.display()
    );
}

/// Scratch directory removed on drop, including on error paths.
struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Result<Self, LedgerError> {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or_default();
        let path = std::env::temp_dir()
            .join(format!("cargo-ledger-setup-{}-{nanos}", std::process::id()));
        fs::create_dir_all(&path)?;
        Ok(Self(path))
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const INDEX: &str = r#"{"name":"ledger_secure_sdk_sys","vers":"1.17.0","cksum":"aa","yanked":false}
{"name":"ledger_secure_sdk_sys","vers":"1.18.1","cksum":"bb","yanked":false}
{"name":"ledger_secure_sdk_sys","vers":"1.19.0-rc.1","cksum":"cc","yanked":false}
{"name":"ledger_secure_sdk_sys","vers":"1.18.2","cksum":"dd","yanked":true}
"#;

    #[test]
    fn latest_skips_yanked_and_prereleases() {
        let r = select_release(INDEX, None).unwrap();
        assert_eq!(r.version, Version::new(1, 18, 1));
        assert_eq!(r.cksum, "bb");
    }

    #[test]
    fn exact_version_is_selected() {
        let r = select_release(INDEX, Some("1.17.0")).unwrap();
        assert_eq!(r.cksum, "aa");
    }

    #[test]
    fn exact_version_may_be_yanked_or_prerelease() {
        assert_eq!(select_release(INDEX, Some("1.18.2")).unwrap().cksum, "dd");
        assert_eq!(
            select_release(INDEX, Some("1.19.0-rc.1")).unwrap().cksum,
            "cc"
        );
    }

    #[test]
    fn unknown_or_invalid_version_fails() {
        assert!(select_release(INDEX, Some("9.9.9")).is_err());
        assert!(select_release(INDEX, Some("not-a-version")).is_err());
    }

    #[test]
    fn checksum_is_verified() {
        let dir = TempDir::new().unwrap();
        let f = dir.path().join("f");
        fs::write(&f, b"abc").unwrap();
        // sha256("abc")
        let good =
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";
        assert!(verify_sha256(&f, good).is_ok());
        assert!(verify_sha256(&f, &good.replace('b', "c")).is_err());
    }
}
