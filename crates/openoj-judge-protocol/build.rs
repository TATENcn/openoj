use std::env;
use std::error::Error;
use std::path::PathBuf;

fn main() -> Result<(), Box<dyn Error>> {
    let proto = "../../schemas/openoj/judge-control/v0alpha1/judge-control.proto";
    let include = "../../schemas";
    let descriptor_path = PathBuf::from(env::var("OUT_DIR")?).join("judge-control-v0alpha1.bin");
    let mut prost_config = tonic_prost_build::Config::new();
    prost_config.protoc_executable(protoc_bin_vendored::protoc_bin_path()?);
    tonic_prost_build::configure()
        .file_descriptor_set_path(descriptor_path)
        .compile_with_config(prost_config, &[proto], &[include])?;
    println!("cargo::rerun-if-changed={proto}");
    Ok(())
}
