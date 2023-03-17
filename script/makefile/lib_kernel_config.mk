BUILD_TARGET_KERNEL               := $(BUILD_TARGET)/kernel
kernel_static_lib                 := $(BUILD_TARGET_KERNEL)/libkernel.a
kernel_binary                     := $(BUILD_TARGET_KERNEL)/kernel.bin
BUILD_TARGET_RING_SCHEDULER       := $(BUILD_TARGET)/ring_scheduler
ring_scheduler_static_lib         := $(BUILD_TARGET_RING_SCHEDULER)/libring_scheduler.a
ring_scheduler_binary             := $(BUILD_TARGET_RING_SCHEDULER)/ring_scheduler.bin

BUILD_TARGET_ASM                  := $(BUILD_TARGET)/asm

linker_script                     := kernel/src/linker.ld
linker_script_ring_scheduler      := ring_scheduler/src/linker.ld
BUILD_TARGET_ABI                  := riscv64gc-unknown-none-elf