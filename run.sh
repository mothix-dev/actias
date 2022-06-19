#!/bin/bash

echo "(ctrl+c to exit)"

#qemu-system-i386 -machine type=pc-i440fx-3.1 -kernel target/i586-unknown-none/release/ockernel -display none -serial stdio

cargo build

mkdir -p iso_root
cp -v target/i586-unknown-none/release/ockernel limine/limine.sys \
	      limine/limine-cd.bin limine/limine-cd-efi.bin limine.cfg iso_root 

xorriso -as mkisofs -b limine-cd.bin \
	        -no-emul-boot -boot-load-size 4 -boot-info-table \
	        --efi-boot limine-cd-efi.bin \
	        -efi-boot-part --efi-boot-image --protective-msdos-label \
	        iso_root -o image.iso

./limine/limine-deploy image.iso


qemu-system-i386 image.iso -serial stdio