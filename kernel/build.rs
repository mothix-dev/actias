fn main() {
    cc::Build::new().file("src/arch/i586/tasks.S").compile("tasks");
    cc::Build::new().file("src/platform/ibmpc/irq.S").compile("irq");
}
