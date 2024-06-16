fn main() {
    if std::env::var("CARGO_FEATURE_PLATFORM_MULTIBOOT").is_ok() {
        // multiboot-specific compilation options

        println!("cargo:rustc-link-arg=-Tactias/src/platform/multiboot/kernel.ld");
        cc::Build::new().file("src/platform/multiboot/boot.S").compile("boot");
    }
}
