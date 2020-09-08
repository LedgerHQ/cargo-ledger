use std::process::{Stdio, Command};
use cargo_metadata::Message;
use clap::{Arg,App};

use std::io;
use std::io::Write;
use std::fs;

use serde_json::json;

fn retrieve_data_size(file: &std::path::Path) -> Result<u64, io::Error> {
    let buffer = fs::read(&file)?;
    let elf = goblin::elf::Elf::parse(&buffer).unwrap();

    let mut nvram_data = 0;
    let mut envram_data = 0;
    for s in elf.syms.iter() {
        let symbol_name = elf.strtab.get(s.st_name);
        let name = symbol_name.unwrap().unwrap();
        match name {
            "_nvram_data" => nvram_data = s.st_value,
            "_envram_data" => envram_data = s.st_value,
            _ => ()
        }
    }
    Ok(envram_data - nvram_data)
}

fn export_binary(elf_path: &std::path::Path) -> std::path::PathBuf {
    let dest_bin = elf_path.parent().unwrap().to_path_buf().join("app.hex");

    Command::new("arm-none-eabi-objcopy")
                        .arg(&elf_path)
                        .arg(&dest_bin)
                        .args(&["-O", "ihex"])
                        .output()
                        .expect("Objcopy failed");

    // print some size info while we're here
    let out = Command::new("arm-none-eabi-size")
                        .arg(&elf_path)
                        .output()
                        .expect("Size failed");

    io::stdout().write_all(&out.stdout).unwrap();
    io::stderr().write_all(&out.stderr).unwrap();

    dest_bin
}

use serde_derive::Deserialize;

#[derive(Debug, Deserialize)] 
struct NanosMetadata {
    curve: String,
    flags: String,
    icon: String,
}

fn main(){
    let matches = App::new("Ledger NanoS load commands")
                        .version("0.0")
                        .about("Builds the project and emits a JSON manifest for ledgerctl.")
                        .arg(Arg::new("ledger"))
                        .subcommand(App::new("load")
                            .about("Load the app onto a nano"))
                        .get_matches();

    let is_load = matches.subcommand_matches("load").is_some();

    let mut cargo_cmd = Command::new("cargo") 
        .args(&["build", "--release", "--message-format=json"])
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();

    let mut exe_path = std::path::PathBuf::new();
    let reader = std::io::BufReader::new(cargo_cmd.stdout.take().unwrap());
    for message in cargo_metadata::Message::parse_stream(reader) {
        match message.unwrap() {
            Message::CompilerArtifact(artifact) => {
                match artifact.executable {
                    Some(n) => { exe_path = n; },
                    _ => ()
                }
            },
            _ => () // Unknown message
        }
    }

    let _output = cargo_cmd.wait().expect("Couldn't get cargo's exit status");

    let mut cmd = cargo_metadata::MetadataCommand::new();
    let res = cmd.no_deps().exec().unwrap();

    let this_pkg = res.packages.last().unwrap();
    let this_metadata: NanosMetadata = serde_json::from_value(this_pkg.metadata["nanos"].clone()).unwrap();

    export_binary(&exe_path);

    let current_dir = std::path::Path::new(&this_pkg.manifest_path).parent().unwrap();

    // app.json will be placed in the app's root directory
    let app_json = current_dir.join("app.json");

    // Find hex file path relative to 'app.json'
    let hex_file = exe_path.strip_prefix(current_dir).unwrap().parent().unwrap().join("app.hex");

    // Retrieve real 'dataSize' from ELF
    let data_size = retrieve_data_size(&exe_path).unwrap();

    // create manifest
    let file = fs::File::create(&app_json).unwrap();
    let json = json!({
        "name": &this_pkg.name,
        "version": &this_pkg.version,
        "icon": &this_metadata.icon,
        "targetId": "0x31100004",
        "flags": this_metadata.flags,
        "derivationPath": {
            "curves": [ this_metadata.curve ]
        },
        "binary": hex_file,
        "dataSize": data_size
    }); 
    serde_json::to_writer_pretty(file, &json).unwrap();

    if is_load {
        let out = Command::new("ledgerctl")
                    .current_dir(current_dir)
                    .args(&["install", "-f", app_json.as_path().to_str().unwrap()])
                    .output()
                    .expect("fail");
        
        io::stdout().write_all(&out.stdout).unwrap();
        io::stderr().write_all(&out.stderr).unwrap();
    }
}