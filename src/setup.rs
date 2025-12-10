use crate::error::LedgerError;
use std::path::Path;
use std::process::Command;

pub fn install_targets(t: Option<String>) -> Result<(), LedgerError> {
    println!("[ ] Install custom targets...");
    // Check if target files are installed
    let mut args: Vec<String> = vec![];

    args.push(String::from("--print"));
    args.push(String::from("sysroot"));
    let sysroot_out = Command::new("rustc").args(&args).output()?;
    if !sysroot_out.status.success() {
        return Err(LedgerError::CommandFailure {
            cmd: "rustc",
            status: sysroot_out.status.code(),
            stderr: String::from_utf8_lossy(&sysroot_out.stderr).into(),
        });
    }
    let sysroot_cmd = std::str::from_utf8(&sysroot_out.stdout)
        .map_err(|e| LedgerError::Other(format!("utf8 sysroot error: {e}")))?
        .trim();

    let git_path = format!("https://raw.githubusercontent.com/LedgerHQ/ledger-device-rust-sdk/{}/ledger_secure_sdk_sys",
            t.unwrap_or_else(|| "refs/heads/master".to_string()));
    let sys_crate_path = Path::new(&git_path);

    let target_files_url = sys_crate_path.join("devices");
    let sysroot = Path::new(sysroot_cmd).join("lib").join("rustlib");

    // Retrieve each target file independently
    // TODO: handle target.json modified upstream
    for target in &["nanox", "nanosplus", "stax", "flex", "apex_p"] {
        let outfilepath = sysroot.join(target).join("target.json");
        let targetpath =
            outfilepath.clone().into_os_string().into_string().map_err(
                |_| {
                    LedgerError::Other("Invalid target path (non UTF-8)".into())
                },
            )?;
        println!("* Adding \x1b[1;32m{target}\x1b[0m in \x1b[1;33m{targetpath}\x1b[0m");

        let target_url =
            target_files_url.join(format!("{target}/{target}.json"));
        let cmd = Command::new("curl")
            .arg(target_url)
            .arg("-o")
            .arg(outfilepath)
            .arg("--create-dirs")
            .output()?;
        if !cmd.status.success() {
            return Err(LedgerError::CommandFailure {
                cmd: "curl",
                status: cmd.status.code(),
                stderr: String::from_utf8_lossy(&cmd.stderr).into(),
            });
        }
        println!("{}", std::str::from_utf8(&cmd.stderr).unwrap());
    }

    // Install link_wrap.sh script needed for relocation
    println!("[ ] Install custom link script...");

    /*  Shall be put at the same place as rust-lld */
    let custom_link_script = "link_wrap.sh";

    let cmd = Command::new("find")
        .arg(sysroot_cmd)
        .arg("-name")
        .arg("rust-lld")
        .output()?;
    if !cmd.status.success() {
        return Err(LedgerError::CommandFailure {
            cmd: "find",
            status: cmd.status.code(),
            stderr: String::from_utf8_lossy(&cmd.stderr).into(),
        });
    }
    let cmd = cmd.stdout;

    let rust_lld_path = std::str::from_utf8(&cmd).map_err(|e| {
        LedgerError::Other(format!("utf8 rust-lld path error: {e}"))
    })?;
    let end = rust_lld_path.rfind('/').ok_or(LedgerError::Other(
        "Could not determine rust-lld directory".into(),
    ))?;

    let outfilepath =
        sysroot.join(&rust_lld_path[..end]).join(custom_link_script);

    /* Retrieve the linker script */
    let target_url = sys_crate_path.join(custom_link_script);
    let curl_out = Command::new("curl")
        .arg(target_url)
        .arg("-o")
        .arg(&outfilepath)
        .output()?;
    if !curl_out.status.success() {
        return Err(LedgerError::CommandFailure {
            cmd: "curl",
            status: curl_out.status.code(),
            stderr: String::from_utf8_lossy(&curl_out.stderr).into(),
        });
    }

    println!("* Custom link script is {}", outfilepath.display());

    /* Make the linker script executable */
    let chmod_out =
        Command::new("chmod").arg("+x").arg(&outfilepath).output()?;
    if !chmod_out.status.success() {
        return Err(LedgerError::CommandFailure {
            cmd: "chmod",
            status: chmod_out.status.code(),
            stderr: String::from_utf8_lossy(&chmod_out.stderr).into(),
        });
    }
    Ok(())
}
