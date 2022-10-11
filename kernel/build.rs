// build script for i586 platform

use std::process::Command;

fn main() {
    println!("cargo:rustc-link-arg=-Tkernel/src/platform/ibmpc/kernel.ld"); // use our linker script for ibmpc

    cc::Build::new().file("src/platform/ibmpc/boot.S").compile("boot");

    println!("cargo:rerun-if-changed=../target/cpu_bootstrap.on");
    println!("cargo:rerun-if-changed=../target/cpu_bootstrap.bin");
    println!("cargo:rerun-if-changed=src/arch/i586/cpu_bootstrap.S");

    assert!(Command::new("as")
        .arg("-32")
        .arg("src/arch/i586/cpu_bootstrap.S")
        .arg("-o")
        .arg("../target/cpu_bootstrap.o")
        .spawn()
        .expect("failed to execute process")
        .wait()
        .expect("failed to wait on child")
        .success());

    assert!(Command::new("ld")
        .arg("-melf_i386")
        .arg("-T")
        .arg("src/arch/i586/cpu_bootstrap.ld")
        .arg("../target/cpu_bootstrap.o")
        .arg("-o")
        .arg("../target/cpu_bootstrap.bin")
        .spawn()
        .expect("failed to execute process")
        .wait()
        .expect("failed to wait on child")
        .success());
}
