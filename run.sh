#!/usr/bin/bash
set -e

# Запуск tomorrow.iso в QEMU.
#
# Нужен UEFI (OVMF) + q35: ядро включает APIC только через XSDT (ACPI 2.0,
# multiboot2-тег 15). SeaBIOS по умолчанию отдаёт ACPI 1.0 (тег 14) — тогда
# APIC не поднимается, прерываний нет, клавиатура и таймер мертвы.
# q35 даёт PCIe/MCFG, без него не найдётся xHCI.

OVMF_CANDIDATES=(
    /usr/share/edk2/x64/OVMF.4m.fd        # Arch
    /usr/share/ovmf/OVMF.fd               # Debian/Ubuntu
    /usr/share/OVMF/OVMF.fd
    /usr/share/edk2/ovmf/OVMF_CODE.fd     # Fedora
)

OVMF=""
for f in "${OVMF_CANDIDATES[@]}"; do
    if [ -f "$f" ]; then
        OVMF="$f"
        break
    fi
done

if [ -z "$OVMF" ]; then
    echo "OVMF не найден. Установи пакет edk2-ovmf (Arch/Fedora) или ovmf (Debian/Ubuntu)."
    exit 1
fi

if [ ! -f tomorrow.iso ]; then
    echo "tomorrow.iso не найден — сначала ./make.sh"
    exit 1
fi

# Окно через GTK, если QEMU собран с ним; иначе — VNC на localhost:5900.
if qemu-system-x86_64 -display help 2>/dev/null | grep -qx gtk; then
    DISPLAY_OPT=(-display gtk)
else
    echo "GTK-дисплей недоступен — экран через VNC: vncviewer ::1:5900"
    DISPLAY_OPT=(-display vnc=localhost:0)
fi

exec qemu-system-x86_64 \
    -machine q35 \
    -bios "$OVMF" \
    -cdrom tomorrow.iso \
    -m 256M \
    -serial stdio \
    "${DISPLAY_OPT[@]}"
