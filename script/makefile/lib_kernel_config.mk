BUILD_TARGET_KERNEL       := $(BUILD_TARGET)/kernel
kernel_static_lib         := $(BUILD_TARGET_KERNEL)/libkernel.a
kernel_binary             := $(BUILD_TARGET_KERNEL)/kernel.bin

BUILD_TARGET_ASM          := $(BUILD_TARGET)/asm
# kernel_asm                := kernel/src/plantform/arch/riscv64gc/asm/entry.S
# compiled_kernel_asm       := $(BUILD_TARGET_ASM)/entry.o

# linker_script             := kernel/src/plantform/arch/riscv64gc/link/linker.ld
linker_script             := kernel/src/linker.ld
BUILD_TARGET_ABI          := riscv64gc-unknown-none-elf