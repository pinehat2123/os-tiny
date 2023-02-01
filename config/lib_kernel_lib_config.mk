KERNEL_LIB_BUILD_DIR          :=  $(BUILD_DIR)/$(TARGET)/$(BUILD_MODE)

BUILD_TARGET_KERNEL_LIB   := $(BUILD_TARGET)/kernel_lib
kernel_lib_static_lib     := $(BUILD_TARGET_KERNEL_LIB)/libkernel_lib.a
kernel_lib_binary         := $(BUILD_TARGET_KERNEL_LIB)/kernel_lib.bin

BUILD_TARGET_ASM          := $(BUILD_TARGET)/asm
kernel_lib_asm            := kernel_lib/src/plantform/arch/riscv64gc/asm/entry.S
compiled_kernel_lib_asm   := $(BUILD_TARGET_ASM)/entry.o

linker_script_lib         := kernel_lib/src/plantform/arch/riscv64gc/link/linker.ld