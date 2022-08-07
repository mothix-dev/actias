// build script for i586 platform

fn main() {
    println!("cargo:rustc-link-arg=-Tsrc/platform/ibmpc/boot.ld"); // use our linker script for ibmpc

    println!("cargo:rustc-cfg=target_arch=\"i586\""); // specify target arch
    println!("cargo:rustc-cfg=target_platform=\"ibmpc\""); // specify target platform
                                                           //println!("cargo:rustc-cfg=debug_messages"); // enable debug messages (useful if things break)

    cc::Build::new().file("src/arch/i586/tasks.S").compile("tasks");
    cc::Build::new().file("src/platform/ibmpc/boot.S").compile("boot");
    cc::Build::new().file("src/platform/ibmpc/irq.S").compile("irq");
}
