#!/bin/sh

set -e

cargo build --release

strip target/i586-unknown-none/release/kernel

echo "(ctrl+c to exit)"

qemu-system-i386 -cpu pentium -machine type=pc-i440fx-3.1 -device isa-debug-exit -kernel target/i586-unknown-none/release/kernel -display none -serial stdio $@
