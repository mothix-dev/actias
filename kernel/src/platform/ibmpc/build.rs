// this file isn't included in the module here, it's the part of the build script for this platform

println!("cargo:rustc-link-arg=-Tkernel/src/platform/ibmpc/kernel.ld"); // use our linker script for ibmpc

cc::Build::new().file("src/platform/ibmpc/boot.S").compile("boot");
