#!/bin/bash

set -e
mkdir -p target

as -32 src/arch/i586/boot.S -o target/boot.o
cargo rustc --release -- --cfg target_arch=\"x86\" -C link-args="-Tsrc/arch/i586/boot.ld target/boot.o"
