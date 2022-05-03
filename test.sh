#!/bin/bash

echo "(ctrl+c to exit)"

#qemu-system-i386 -machine type=pc-i440fx-3.1 -kernel target/i586-unknown-none/release/ockernel -display none -serial stdio
qemu-system-i386 -machine type=pc-i440fx-3.1 -kernel target/i586-unknown-none/release/ockernel -serial stdio
