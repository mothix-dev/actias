#!/bin/sh

set -e

cargo build --release

strip target/i586-unknown-none/release/kernel
strip target/i586-unknown-none/release/test-bin

mkdir -p initrd
cp target/i586-unknown-none/release/test-bin initrd/init
cd initrd
tar cvf ../initrd.tar *
cd ..

echo "(ctrl+c to exit)"

qemu-system-i386 -cpu pentium -machine type=pc-i440fx-3.1 -device isa-debug-exit -kernel target/i586-unknown-none/release/kernel -initrd initrd.tar -display none -serial stdio $@
