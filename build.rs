fn main() {
    println!("cargo:rerun-if-changed=src/switch.s");
    cc::Build::new()
        .file("src/switch.s")
        .file("src/timer.s")
        .compile("switch");
}
