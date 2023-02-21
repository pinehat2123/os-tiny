# Basic directory/firl path definitions used throughout the Makefiel

OS_ARCH                   ?= riscv64gc
TARGET                    ?= $(OS_ARCH)-unknown-none-elf
BUILD_MODE                ?= release

ROOT_DIR                  := $(abspath $(dir $(lastword $(MAKEFILE_LIST)))/../..)

BOOTLOADER                := rustsbi-qemu.bin

BUILD_DIR                 :=  $(ROOT_DIR)/target
KERNEL_BUILD_DIR          :=  $(BUILD_DIR)/$(TARGET)/$(BUILD_MODE)

BUILD_TARGET              := $(ROOT_DIR)/build

include script/makefile/lib_kernel_config.mk