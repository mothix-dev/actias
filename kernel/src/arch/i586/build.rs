// this file isn't included in the module here, it's the part of the build script for this architecture

use std::process::Command;

println!("cargo:rerun-if-changed=../target/cpu_bootstrap.o");
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
