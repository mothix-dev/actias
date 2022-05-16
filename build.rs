// build script for i586 platform

use std::process::Command;

fn main() {
    println!("cargo:rustc-link-arg=-Tsrc/arch/i586/boot.ld"); // use our linker script for i586
    println!("cargo:rustc-link-arg=target/boot.o"); // link our asm boot shim

    println!("cargo:rustc-cfg=target_arch=\"i586\""); // specify target arch
    println!("cargo:rustc-cfg=target_platform=\"ibmpc\""); // specify target platform

    // compile our asm boot shim
    // FIXME: find better assembler
    Command::new("as").arg("-32").arg("src/arch/i586/boot.S").arg("-o").arg("target/boot.o").output().expect("failed to execute process");
}
