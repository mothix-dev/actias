// build script for i586 platform

fn main() {
    println!("cargo:rustc-link-arg=-Tloader/src/boot/ibmpc/boot.ld"); // use our linker script for ibmpc

    cc::Build::new().file("src/boot/ibmpc/boot.S").compile("boot");
}
