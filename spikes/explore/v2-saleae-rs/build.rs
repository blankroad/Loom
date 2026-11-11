fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("cargo:rerun-if-changed=proto/saleae.proto");
    tonic_prost_build::configure()
        .build_server(false)
        .compile_protos(&["proto/saleae.proto"], &["proto"])?;
    Ok(())
}
