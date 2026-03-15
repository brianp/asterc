fn main() {
    let target_arch = std::env::var("CARGO_CFG_TARGET_ARCH").unwrap();

    let mut build = cc::Build::new();

    match target_arch.as_str() {
        "aarch64" => {
            build.file("src/green/asm/aarch64.S");
        }
        "x86_64" => {
            build.file("src/green/asm/x86_64.S");
        }
        other => panic!("unsupported architecture for green threads: {other}"),
    }

    build.compile("green_asm");
}
