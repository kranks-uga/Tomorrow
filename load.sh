#!/usr/bin/bash
set -e

sudo dd if=tomorrow.iso of=/dev/sda bs=4M status=progress && sync