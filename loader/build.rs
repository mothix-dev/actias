// build script for i586 platform

use std::process::Command;

fn main() {
    println!("cargo:rustc-link-arg=-Tloader/src/boot/ibmpc/boot.ld"); // use our linker script for ibmpc

    cc::Build::new().file("src/boot/ibmpc/boot.S").compile("boot");

    println!("cargo:rerun-if-changed=../target/exec_kernel.bin");
    println!("cargo:rerun-if-changed=src/boot/ibmpc/exec_kernel.S");

    assert!(Command::new("as")
        .arg("-32")
        .arg("src/boot/ibmpc/exec_kernel.S")
        .arg("-o")
        .arg("../target/exec_kernel.o")
        .spawn()
        .expect("failed to execute process")
        .wait()
        .expect("failed to wait on child")
        .success());

    assert!(Command::new("ld")
        .arg("-melf_i386")
        .arg("--section-start=.text=0xfffff000")
        .arg("--oformat")
        .arg("binary")
        .arg("../target/exec_kernel.o")
        .arg("-o")
        .arg("../target/exec_kernel.bin")
        .spawn()
        .expect("failed to execute process")
        .wait()
        .expect("failed to wait on child")
        .success());
}
