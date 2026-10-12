use std::path::PathBuf;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let protoc = protoc_bin_vendored::protoc_bin_path()?;
    unsafe {
        std::env::set_var("PROTOC", protoc);
    }

    let out_dir = PathBuf::from(std::env::var("OUT_DIR")?);
    let descriptor_path = out_dir.join("proto_descriptor.bin");

    let proto_root = PathBuf::from("../../contracts/proto");
    let protos = [
        proto_root.join("geostream/v1/common.proto"),
        proto_root.join("geostream/v1/location.proto"),
        proto_root.join("geostream/v1/session.proto"),
        proto_root.join("geostream/v1/stream.proto"),
    ];

    let mut config = prost_build::Config::new();
    config
        .compile_well_known_types()
        .extern_path(".google.protobuf", "::pbjson_types")
        .file_descriptor_set_path(&descriptor_path);
    config.compile_protos(&protos, &[&proto_root])?;

    let descriptor_set = std::fs::read(descriptor_path)?;
    pbjson_build::Builder::new()
        .register_descriptors(&descriptor_set)?
        .ignore_unknown_fields()
        .build(&[".geostream.v1"])?;

    println!("cargo:rerun-if-changed=../../contracts/proto");
    Ok(())
}
