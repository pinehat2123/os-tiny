include config/config.mk
.PHONY: kernel run run-inner $(kernel_binary)
kernel: 
	@${INFO} "DEAL WITH Kernel"
	@${INFO} "Build/Kernel"
	@${MKDIR} -p build/kernel
	@${CARGO} build -p kernel --${BUILD_MODE} --target  $(BUILD_TARGET_ABI)
	@${INFO} "move to Build/Kernel and \e[35mKernel Static Lib OK\e[0m"
	@${CP} ${KERNEL_BUILD_DIR}/libkernel.a ${BUILD_TARGET_KERNEL}/
# @${INFO} "Build/Asm"
# @${MKDIR} -p build/asm
# ${CROSS_AS} -c $(kernel_asm) -o $(compiled_kernel_asm)
# @${INFO} "move to Build/Asm and \e[35mKernel ASM OK\e[0m"
	@${INFO} "Kernel build finish."


	
## This builds the kernel binary itself, which is the fully-linked code that first runs right after the bootloader
$(kernel_binary): $(kernel_static_lib) $(linker_script)
	$(CROSS_LD) -n --static -T $(linker_script) -o $(kernel_binary) $(compiled_kernel_asm) $(kernel_static_lib)


run:
	@${MAKE} clean && ${MAKE} kernel && ${MAKE} ${kernel_binary} && ${MAKE} fs-img
	@${MAKE} run-inner --no-print-directory
	@${NEWLINE}
	@${INFO} "Kernel Run finish."
	@${MAKE} clean

FS_IMG                    := target/$(TARGET)/$(MODE)fs.img
APPS                      := user/src/bin/*

fs-img: $(APPS)
	@mkdir -p build/apps
	@cd user && make build TEST=$(TEST)
	@rm -f $(FS_IMG)
	@cd easy-fs-fuse && cargo run --release -- -s ../user/src/bin/ -t ../target/riscv64gc-unknown-none-elf/release/
	@mv target/riscv64gc-unknown-none-elf/release/fs.img build/apps

run-inner:
	qemu-system-riscv64 \
		-machine virt \
		-display none\
		-nographic \
		-bios $(BOOTLOADER) \
		-device loader,file=$(kernel_binary),addr=80200000\
		-drive file=build/apps/fs.img,if=none,format=raw,id=x0 \
        -device virtio-blk-device,drive=x0 \
		-device virtio-gpu-device  \
		-device virtio-keyboard-device  \
		-device virtio-mouse-device

# -drive file=$(FS_IMG),if=none,format=raw,id=x0