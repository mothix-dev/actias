#!/bin/sh

set -e

mkdir -p test-initrd

cd test-initrd
cp ../test-bin/target/i586-unknown-none/debug/test-bin .
cp ../test-bin2/target/i586-unknown-none/debug/test-bin2 .

strip *

tar cvf ../initrd.tar *
cd ..

echo "(ctrl+c to exit)"

#qemu-system-i386 -machine type=pc-i440fx-3.1 -kernel target/i586-unknown-none/release/ockernel -display none -serial stdio
qemu-system-i386 -cpu pentium -machine type=pc-i440fx-3.1 -kernel target/i586-unknown-none/release/ockernel -initrd initrd.tar -serial stdio
