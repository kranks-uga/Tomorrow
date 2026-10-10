#!/usr/bin/bash
set -e

# С rustup берём nightly из rust-toolchain.toml. Без rustup (Rust из пакетов
# дистрибутива — stable) разрешаем -Z флаги через RUSTC_BOOTSTRAP.
if ! command -v rustup &> /dev/null; then
    export RUSTC_BOOTSTRAP=1
fi

cargo build -Zbuild-std=core,compiler_builtins,alloc \
    -Zbuild-std-features=compiler-builtins-mem

mkdir -p iso/boot/grub
cp target/x86_64-unknown-none/debug/tomorrow iso/boot/tomorrow.elf
cp initrd.tar iso/boot/initrd.tar

cp boot/grub/grub.cfg iso/boot/grub/grub.cfg

grub-mkrescue -o tomorrow.iso iso