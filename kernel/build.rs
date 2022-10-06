// build script for i586 platform

fn main() {
    println!("cargo:rustc-link-arg=-Tkernel/src/platform/ibmpc/kernel.ld"); // use our linker script for ibmpc

    cc::Build::new().file("src/platform/ibmpc/boot.S").compile("boot");
}
