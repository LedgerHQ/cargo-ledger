use cargo_metadata::camino::Utf8Path;
use cargo_metadata::camino::Utf8PathBuf;
use std::collections::HashMap;
use std::env;
use std::fs;
use std::io::{self, Write};
use std::process::Command;

use crate::error::LedgerError;

#[derive(Default, Debug)]
pub struct LedgerAppInfos {
    pub app_name: String,
    pub app_version: String,
    pub api_level: String,
    pub app_flags: String,
    pub target_id: String,
    pub data_size: u64,
    pub install_params_size: u64,
}

fn get_string_from_offset(
    vector: &[u8],
    offset: &usize,
) -> Result<String, LedgerError> {
    // Find the end of the string (search for a line feed character)
    let end_index = vector[*offset..]
        .iter()
        .position(|&x| x == b'\n')
        .map(|pos| *offset + pos)
        .unwrap_or(*offset); // Use the start offset if the delimiter position is not found
    String::from_utf8(vector[*offset..end_index].to_vec())
        .map_err(|e| LedgerError::Other(format!("Invalid UTF-8: {e}")))
}

pub fn retrieve_infos(
    file: &Utf8PathBuf,
) -> Result<LedgerAppInfos, LedgerError> {
    println!("Retrieving Ledger app infos from ELF: {}", file);
    let buffer = fs::read(file)?;
    let elf = goblin::elf::Elf::parse(&buffer)?;

    let mut infos = LedgerAppInfos::default();

    // All infos coming from the SDK are expected to be regrouped
    // in various `.ledger.<field_name>` (rust SDK <= 1.0.0) or
    // `ledger.<field_name> (rust SDK > 1.0.0) section of the binary.
    for section in elf.section_headers.iter() {
        if let Some(name) = elf.shdr_strtab.get_at(section.sh_name) {
            match name {
                "ledger.app_name" => {
                    infos.app_name = get_string_from_offset(
                        &buffer,
                        &(section.sh_offset as usize),
                    )?;
                }
                "ledger.app_version" => {
                    infos.app_version = get_string_from_offset(
                        &buffer,
                        &(section.sh_offset as usize),
                    )?;
                }
                "ledger.api_level" => {
                    infos.api_level = get_string_from_offset(
                        &buffer,
                        &(section.sh_offset as usize),
                    )?;
                }
                "ledger.app_flags" => {
                    infos.app_flags = get_string_from_offset(
                        &buffer,
                        &(section.sh_offset as usize),
                    )?;
                }
                "ledger.target_id" => {
                    infos.target_id = get_string_from_offset(
                        &buffer,
                        &(section.sh_offset as usize),
                    )?;
                }
                _ => (),
            }
        }
    }

    let mut nvram_data = 0;
    let mut envram_data = 0;
    let mut install_parameters_data = 0;
    let mut einstall_parameters_data = 0;
    for s in elf.syms.iter() {
        let name = elf
            .strtab
            .get_at(s.st_name)
            .ok_or_else(|| LedgerError::Other("Missing symbol name".into()))?;
        match name {
            "_nvram_data" => nvram_data = s.st_value,
            "_envram_data" => envram_data = s.st_value,
            "_install_parameters" => install_parameters_data = s.st_value,
            "_einstall_parameters" => einstall_parameters_data = s.st_value,
            _ => (),
        }
    }
    infos.data_size = envram_data - nvram_data;
    infos.install_params_size =
        einstall_parameters_data - install_parameters_data;
    Ok(infos)
}

pub fn export_binary(
    elf_path: &Utf8PathBuf,
    dest_bin: &Utf8PathBuf,
) -> Result<(), LedgerError> {
    let objcopy = env::var_os("CARGO_TARGET_THUMBV6M_NONE_EABI_OBJCOPY")
        .unwrap_or_else(|| "arm-none-eabi-objcopy".into());
    let copy_out = Command::new(&objcopy)
        .arg(elf_path)
        .arg(dest_bin)
        .args(["-O", "ihex"])
        .output()?;
    if !copy_out.status.success() {
        return Err(LedgerError::CommandFailure {
            cmd: "objcopy",
            status: copy_out.status.code(),
            stderr: String::from_utf8_lossy(&copy_out.stderr).into(),
        });
    }

    let size = env::var_os("CARGO_TARGET_THUMBV6M_NONE_EABI_SIZE")
        .unwrap_or_else(|| "arm-none-eabi-size".into());

    // print some size info while we're here
    let out = Command::new(&size).arg(elf_path).output()?;
    if !out.status.success() {
        return Err(LedgerError::CommandFailure {
            cmd: "size",
            status: out.status.code(),
            stderr: String::from_utf8_lossy(&out.stderr).into(),
        });
    }

    io::stdout().write_all(&out.stdout)?;
    io::stderr().write_all(&out.stderr)?;
    Ok(())
}

pub fn dump_with_ledgerblue(
    dir: &Utf8Path,
    params: &HashMap<String, String>,
    out_file_name: &Utf8PathBuf,
) -> Result<(), LedgerError> {
    let out = Command::new("python3")
        .current_dir(dir)
        .args(["-m", "ledgerblue.loadApp"])
        .args(["--targetId", params["targetId"].as_str()])
        .args(["--targetVersion", ""])
        .args(["--apiLevel", params["apiLevel"].as_str()])
        .args(["--fileName", params["binary"].as_str()])
        .args(["--appName", params["name"].as_str()])
        .args(["--appFlags", params["flags"].as_str()])
        .arg("--delete")
        .arg("--tlv")
        .args(["--dataSize", params["dataSize"].as_str()])
        .args(["--installparamsSize", params["installParamsSize"].as_str()])
        .args(["--offline", out_file_name.as_str()])
        .output()?;
    if !out.status.success() {
        return Err(LedgerError::CommandFailure {
            cmd: "python3 -m ledgerblue.loadApp",
            status: out.status.code(),
            stderr: String::from_utf8_lossy(&out.stderr).into(),
        });
    }
    io::stdout().write_all(&out.stdout)?;
    io::stderr().write_all(&out.stderr)?;
    Ok(())
}

pub fn install_with_ledgerblue(
    dir: &Utf8Path,
    params: &HashMap<String, String>,
    out_file_name: &Utf8PathBuf,
) -> Result<(), LedgerError> {
    let out = Command::new("python3")
        .current_dir(dir)
        .args(["-m", "ledgerblue.runScript"])
        .args(["--targetId", params["targetId"].as_str()])
        .args(["--fileName", out_file_name.as_str()])
        .args(["--apdu", "--scp"])
        .output()?;
    if !out.status.success() {
        return Err(LedgerError::CommandFailure {
            cmd: "python3 -m ledgerblue.runScript",
            status: out.status.code(),
            stderr: String::from_utf8_lossy(&out.stderr).into(),
        });
    }
    io::stdout().write_all(&out.stdout)?;
    io::stderr().write_all(&out.stderr)?;
    Ok(())
}
