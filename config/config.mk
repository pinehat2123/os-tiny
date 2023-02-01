# Basic directory/firl path definitions used throughout the Makefiel

OS_ARCH                   ?= riscv64gc
TARGET                    ?= $(OS_ARCH)-unknown-none-elf
BUILD_MODE                ?= release

ROOT_DIR                  := $(abspath $(dir $(lastword $(MAKEFILE_LIST)))/..)

BOOTLOADER                := rustsbi-qemu.bin

BUILD_DIR                 :=  $(ROOT_DIR)/target
KERNEL_BUILD_DIR          :=  $(BUILD_DIR)/$(TARGET)/$(BUILD_MODE)

BUILD_TARGET              := $(ROOT_DIR)/build
BUILD_TARGET_KERNEL       := $(BUILD_TARGET)/kernel
kernel_static_lib         := $(BUILD_TARGET_KERNEL)/libkernel.a
kernel_binary             := $(BUILD_TARGET_KERNEL)/kernel.bin

BUILD_TARGET_ASM          := $(BUILD_TARGET)/asm
kernel_asm                := kernel/src/plantform/arch/riscv64gc/asm/entry.S
compiled_kernel_asm       := $(BUILD_TARGET_ASM)/entry.o

linker_script             := kernel/src/plantform/arch/riscv64gc/link/linker.ld