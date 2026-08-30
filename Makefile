# Tomorrow OS - Makefile
# ========================

# Компиляторы и инструменты
CC = gcc
LD = ld
NASM = nasm
OBJCOPY = objcopy

# Флаги компиляции
CFLAGS_COMMON = -ffreestanding -mno-red-zone -m64 -fno-pie -fno-stack-protector
CFLAGS_BOOT = $(CFLAGS_COMMON) -fshort-wchar -fno-ident
CFLAGS_KERNEL = $(CFLAGS_COMMON)

# Флаги линковки
LDFLAGS_BOOT = -m i386pep -nostdlib --subsystem=10 --image-base 0x100000 -pie -e efi_main
LDFLAGS_KERNEL = -m elf_x86_64 -Ttext 0x1000000

# Пути
BOOT_DIR = boot
KERNEL_DIR = kernel
ESP_DIR = esp
BUILD_DIR = build

# Файлы
BOOT_EFI = $(ESP_DIR)/EFI/BOOT/BOOTX64.EFI
KERNEL_BIN = $(KERNEL_DIR)/kernel.bin

# Цели по умолчанию
.PHONY: all clean run boot kernel

all: boot kernel
	@echo "========================================="
	@echo "  Tomorrow OS built successfully!"
	@echo "========================================="

# Сборка загрузчика
boot: $(BOOT_EFI)

$(BOOT_EFI): $(BOOT_DIR)/efi_main.c $(BOOT_DIR)/efi.h
	@echo "[1/4] Compiling bootloader..."
	@mkdir -p $(BUILD_DIR)   
	@mkdir -p $(ESP_DIR)/EFI/BOOT
	$(CC) $(CFLAGS_BOOT) -c $(BOOT_DIR)/efi_main.c -o $(BUILD_DIR)/efi_main.o
	$(LD) $(LDFLAGS_BOOT) $(BUILD_DIR)/efi_main.o -o $@
	@echo "[1/4] Bootloader compiled: $@"

# Сборка ядра
kernel: $(KERNEL_BIN)

$(KERNEL_BIN): $(KERNEL_DIR)/kernel.asm $(KERNEL_DIR)/kernel.c
	@echo "[2/4] Assembling kernel..."
	$(NASM) -f elf64 $(KERNEL_DIR)/kernel.asm -o $(BUILD_DIR)/kernel.o
	@echo "[3/4] Compiling kernel C code..."
	$(CC) $(CFLAGS_KERNEL) -c $(KERNEL_DIR)/kernel.c -o $(BUILD_DIR)/kernel_c.o
	@echo "[4/4] Linking kernel..."
	$(LD) $(LDFLAGS_KERNEL) -o $(BUILD_DIR)/kernel.elf $(BUILD_DIR)/kernel.o $(BUILD_DIR)/kernel_c.o
	$(OBJCOPY) -O binary $(BUILD_DIR)/kernel.elf $@
	@echo "[4/4] Kernel built: $@"
	cp $@ esp/kernel.bin

# Запуск в QEMU
run: all
	@echo "Starting QEMU..."
	qemu-system-x86_64 \
		-drive if=pflash,format=raw,readonly=on,file=/usr/share/edk2/x64/OVMF_CODE.4m.fd \
		-drive if=pflash,format=raw,file=OVMF_VARS.4m.fd \
		-drive format=raw,file=fat:rw:$(ESP_DIR)

# Очистка
clean:
	@echo "Cleaning build artifacts..."
	rm -rf $(BUILD_DIR)
	rm -f $(KERNEL_BIN)
	rm -f $(BOOT_EFI)
	@echo "Clean complete."