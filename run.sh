#!/bin/sh

set -e

cargo build --release

mkdir -p test-initrd

cd test-initrd
echo "this is a test uwu" > test.txt

tar czvf ../initrd.tar.gz *
cd ..

strip target/i586-unknown-none/release/kernel

echo "(ctrl+c to exit)"

qemu-system-i386 -cpu pentium -machine type=pc-i440fx-3.1 -device isa-debug-exit -kernel target/i586-unknown-none/release/kernel -initrd initrd.tar.gz -display none -serial stdio $@
