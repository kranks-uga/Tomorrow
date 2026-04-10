fn main() {
    println!("cargo:rerun-if-changed=src/switch.s");
    println!("cargo:rerun-if-changed=src/timer.s");
    println!("cargo:rerun-if-changed=src/syscall_entry.s");
    cc::Build::new()
        .file("src/switch.s")
        .file("src/timer.s")
        .file("src/syscall_entry.s")
        .compile("switch");
}
