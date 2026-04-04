fn main() {
    println!("cargo:rerun-if-changed=src/switch.s");
    cc::Build::new().file("src/switch.s").compile("switch");
}
