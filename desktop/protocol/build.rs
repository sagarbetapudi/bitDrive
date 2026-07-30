use std::path::PathBuf;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let proto_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../proto");

    let protos = [
        "common.proto",
        "frame.proto",
        "session.proto",
        "capability.proto",
        "auth.proto",
        "mux.proto",
        "registry.proto",
        "filesystem.proto",
        "sync.proto",
        "photo_backup.proto",
        "shell.proto",
        "media_control.proto",
        "phone_fs.proto",
        "proximity.proto",
        "file_stream.proto",
        "app_launcher.proto",
        "config.proto",
    ];

    let includes = [proto_dir.clone()];

    prost_build::Config::new()
        .out_dir("src/pb")
        .compile_protos(
            &protos.iter().map(|p| proto_dir.join(p)).collect::<Vec<_>>(),
            &includes,
        )?;

    // Tell cargo to rebuild if any proto file changes
    for proto in &protos {
        println!("cargo:rerun-if-changed=../../proto/{}", proto);
    }

    Ok(())
}