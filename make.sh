#!/usr/bin/bash
set -e

cargo +nightly build -Zbuild-std=core,compiler_builtins \
    -Zbuild-std-features=compiler-builtins-mem

mkdir -p iso/boot/grub
cp target/x86_64-unknown-none/debug/tomorrow iso/boot/tomorrow.elf
cp initrd.tar iso/boot/initrd.tar

cp boot/grub/grub.cfg iso/boot/grub/grub.cfg

grub-mkrescue -o tomorrow.iso iso