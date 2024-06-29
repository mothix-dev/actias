fn main() {
    if std::env::var("CARGO_CFG_TARGET_ARCH").as_deref() == Ok("x86") {
        // x86-specific compilation options

        println!("cargo:rustc-link-arg=-Tactias/src/arch/x86/kernel.ld");
        println!("cargo::rerun-if-changed=actias/src/arch/x86/kernel.ld");
        cc::Build::new().compiler("clang").file("src/arch/x86/init.S").compile("init");
    }
}
