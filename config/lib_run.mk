include config/config.mk
.PHONY: kernel_lib lib-run lib-run-inner $(kernel_lib_binary)
kernel_lib: 
	@${INFO} "DEAL WITH Kernel Lib"
	@${INFO} "Build/Kernel_Lib"
	@${MKDIR} -p build/kernel_lib
	@${CARGO} build -p kernel_lib --${BUILD_MODE}
	@${INFO} "move to Build/Kernel_Lib and \e[35mKernel Static Lib OK\e[0m"
	@${CP} ${KERNEL_LIB_BUILD_DIR}/libkernel_lib.a ${BUILD_TARGET_KERNEL_LIB}/
	@${INFO} "Build/Asm"
	@${MKDIR} -p build/asm
	${CROSS_AS} -c $(kernel_lib_asm) -o $(compiled_kernel_lib_asm)
	@${INFO} "move to Build/Asm and \e[35mKernel ASM OK\e[0m"
	@${INFO} "Kernel Lib build finish."


	
## This builds the kernel binary itself, which is the fully-linked code that first runs right after the bootloader
$(kernel_lib_binary): $(kernel_lib_static_lib) $(linker_script_lib)
	$(CROSS_LD) -n -T $(linker_script_lib) -o $(kernel_lib_binary) $(compiled_kernel_lib_asm) $(kernel_lib_static_lib)


lib-run:
	@${MAKE} clean && ${MAKE} kernel_lib && ${MAKE} ${kernel_lib_binary}
	@${MAKE} lib-run-inner --no-print-directory
	@${NEWLINE}
	@${INFO} "Kernel Lib Run finish."
	@${MAKE} clean

lib-run-inner:
	qemu-system-riscv64 \
		-machine virt \
		-nographic \
		-bios $(BOOTLOADER) \
		-device loader,file=$(kernel_lib_binary),addr=80200000
