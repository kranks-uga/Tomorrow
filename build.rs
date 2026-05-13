fn main() {
    println!("cargo:rerun-if-changed=src/switch.s");
    println!("cargo:rerun-if-changed=src/timer.s");
    println!("cargo:rerun-if-changed=src/syscall_entry.s");
    println!("cargo:rerun-if-changed=src/keyboard.s");
    cc::Build::new()
        .file("src/switch.s")
        .file("src/timer.s")
        .file("src/syscall_entry.s")
        .file("src/keyboard.s")
        .compile("switch");
}
