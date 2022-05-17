#!/bin/bash
shopt -s extglob

cargo test --no-run || exit 1

qemu-system-i386 -cpu pentium -machine type=pc-i440fx-3.1 -kernel target/i586-unknown-none/debug/deps/ockernel-!(*.d) -display none -serial stdio -device isa-debug-exit,iobase=0xf4,iosize=0x01

TEST_RESULT="$?"

echo "process exited with code ${TEST_RESULT}"

if [ ${TEST_RESULT} -eq 33 ]; then
    echo "Tests successful"
else
    echo "Tests failed" >&2
    exit 1
fi
