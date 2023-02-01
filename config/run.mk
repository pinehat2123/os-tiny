include config/config.mk
.PHONY: kernel run run-inner $(kernel_binary)
kernel: 
	@${INFO} "DEAL WITH Kernel"
	@${INFO} "Build/Kernel"
	@${MKDIR} -p build/kernel
	@${CARGO} build -p kernel --${BUILD_MODE}
	@${INFO} "move to Build/Kernel and \e[35mKernel Static Lib OK\e[0m"
	@${CP} ${KERNEL_BUILD_DIR}/libkernel.a ${BUILD_TARGET_KERNEL}/
	@${INFO} "Build/Asm"
	@${MKDIR} -p build/asm
	${CROSS_AS} -c $(kernel_asm) -o $(compiled_kernel_asm)
	@${INFO} "move to Build/Asm and \e[35mKernel ASM OK\e[0m"
	@${INFO} "Kernel build finish."


	
## This builds the kernel binary itself, which is the fully-linked code that first runs right after the bootloader
$(kernel_binary): $(kernel_static_lib) $(linker_script)
	$(CROSS_LD) -n --static -T $(linker_script) -o $(kernel_binary) $(compiled_kernel_asm) $(kernel_static_lib)


run:
	@${MAKE} clean && ${MAKE} kernel && ${MAKE} ${kernel_binary}
	@${MAKE} run-inner --no-print-directory
	@${NEWLINE}
	@${INFO} "Kernel Run finish."
	@${MAKE} clean

run-inner:
	qemu-system-riscv64 \
		-machine virt \
		-nographic \
		-bios $(BOOTLOADER) \
		-device loader,file=$(kernel_binary),addr=80200000
