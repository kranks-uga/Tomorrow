#!/usr/bin/bash
set -e

echo "Доступные диски:"
lsblk -d -o NAME,SIZE,MODEL | grep -v loop
echo ""
read -rp "Куда записать? (например sda): " DISK
TARGET="/dev/$DISK"

if [ ! -b "$TARGET" ]; then
    echo "Ошибка: $TARGET не найден"
    exit 1
fi

echo "Запись tomorrow.iso -> $TARGET"
sudo dd if=tomorrow.iso of="$TARGET" bs=4M status=progress && sync
echo "Готово."