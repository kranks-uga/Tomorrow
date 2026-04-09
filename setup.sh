#!/bin/bash
set -e

echo "=== Tomorrow OS — установка зависимостей ==="

# Определяем дистрибутив
if [ -f /etc/os-release ]; then
    . /etc/os-release
    DISTRO=$ID
else
    echo "Не удалось определить дистрибутив"
    exit 1
fi

echo "Дистрибутив: $DISTRO"

# Системные пакеты
install_packages() {
    case $DISTRO in
        ubuntu|debian)
            sudo apt-get update
            sudo apt-get install -y \
                curl git build-essential \
                gcc nasm grub-pc-bin \
                grub-common xorriso \
                qemu-system-x86 \
                mtools
            ;;
        arch|manjaro)
            sudo pacman -Sy --noconfirm \
                curl git base-devel \
                gcc nasm grub \
                xorriso qemu-system-x86 \
                mtools
            ;;
        fedora)
            sudo dnf install -y \
                curl git gcc \
                nasm grub2-tools \
                xorriso qemu-system-x86 \
                mtools
            ;;
        *)
            echo "Неизвестный дистрибутив: $DISTRO"
            echo "Установи вручную: gcc, nasm, grub, xorriso, qemu"
            exit 1
            ;;
    esac
}

# Rust
install_rust() {
    if command -v rustup &> /dev/null; then
        echo "rustup уже установлен — обновляем"
        rustup update
    else
        echo "Устанавливаем rustup..."
        curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
        source "$HOME/.cargo/env"
    fi

    # nightly toolchain
    rustup toolchain install nightly
    rustup default nightly

    # цель для сборки ядра
    rustup target add x86_64-unknown-none

    # компонент для сборки core
    rustup component add rust-src
}

echo ""
echo "--- Устанавливаем системные пакеты ---"
install_packages

echo ""
echo "--- Устанавливаем Rust ---"
install_rust

echo ""
echo "--- Проверяем установку ---"
echo -n "gcc: ";    gcc --version | head -1
echo -n "nasm: ";   nasm --version | head -1
echo -n "grub: ";   grub-mkrescue --version 2>&1 | head -1
echo -n "xorriso: ";xorriso --version 2>&1 | head -1
echo -n "qemu: ";   qemu-system-x86_64 --version | head -1
echo -n "rust: ";   rustc --version
echo -n "cargo: ";  cargo --version

echo ""
echo "=== Готово! Теперь можно собирать: ./make.sh ==="
