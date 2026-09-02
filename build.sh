#!/bin/bash

# Tomorrow OS - Build Script
# ===========================

echo "========================================="
echo "  Building Tomorrow OS..."
echo "========================================="

# Настройки
CC=gcc
LD=ld
NASM=nasm
OBJCOPY=objcopy

# Флаги
CFLAGS_BOOT="-ffreestanding -mno-red-zone -m64 -fno-pie -fno-stack-protector -fshort-wchar -fno-ident"
# ВАЖНО: без -fno-pie — gcc генерирует RIP-relative обращения к глобалам/
# строковым литералам (lea reg, [rip+offset]) вместо абсолютных адресов.
# Благодаря этому ядро можно грузить по ЛЮБОМУ адресу без релокаций.
CFLAGS_KERNEL="-ffreestanding -mno-red-zone -m64 -fno-stack-protector -fno-asynchronous-unwind-tables"
LDFLAGS_BOOT="-m i386pep -nostdlib --subsystem=10 --image-base 0x100000 -pie -e efi_main"
# Фиксированный -Ttext больше не нужен — код position-independent
LDFLAGS_KERNEL="-m elf_x86_64"

# Пути
BOOT_DIR="boot"
KERNEL_DIR="kernel"
ESP_DIR="esp"
BUILD_DIR="build"

# Создаем папку для временных файлов
mkdir -p $BUILD_DIR
mkdir -p $ESP_DIR/EFI/BOOT

echo ""
echo "[1/4] Compiling bootloader..."
$CC $CFLAGS_BOOT -c $BOOT_DIR/efi_main.c -o $BUILD_DIR/efi_main.o
$LD $LDFLAGS_BOOT $BUILD_DIR/efi_main.o -o $ESP_DIR/EFI/BOOT/BOOTX64.EFI
echo "[1/4] Bootloader compiled: $ESP_DIR/EFI/BOOT/BOOTX64.EFI"

echo ""
echo "[2/4] Assembling kernel..."
$NASM -f elf64 $KERNEL_DIR/kernel.asm -o $BUILD_DIR/kernel.o

echo "[3/4] Compiling kernel C code..."
$CC $CFLAGS_KERNEL -c $KERNEL_DIR/kernel.c -o $BUILD_DIR/kernel_c.o

echo "[4/4] Linking kernel..."
$LD $LDFLAGS_KERNEL -o $BUILD_DIR/kernel.elf $BUILD_DIR/kernel.o $BUILD_DIR/kernel_c.o
$OBJCOPY -O binary $BUILD_DIR/kernel.elf $KERNEL_DIR/kernel.bin
cp $KERNEL_DIR/kernel.bin $ESP_DIR/kernel.bin
echo "[4/4] Kernel built: $KERNEL_DIR/kernel.bin"

echo ""
echo "========================================="
echo "  Build complete!"
echo "========================================="
echo ""
echo "To run in QEMU:"
echo "  qemu-system-x86_64 \\"
echo "    -drive if=pflash,format=raw,readonly=on,file=/usr/share/edk2/x64/OVMF_CODE.4m.fd \\"
echo "    -drive if=pflash,format=raw,file=OVMF_VARS.4m.fd \\"
echo "    -drive format=raw,file=fat:rw:$ESP_DIR"