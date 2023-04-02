include script/makefile/config.mk
.PHONY: kernel run run-inner ring_scheduler $(kernel_binary) $(ring_scheduler_binary)
kernel:
	@${INFO} "DEAL WITH Kernel"
	@${INFO} "Build/Kernel"
	@${MKDIR} -p build/kernel
	@${CARGO} build -p kernel --${BUILD_MODE} --target  $(BUILD_TARGET_ABI)
	@${INFO} "move to Build/Kernel and \e[35mKernel Static Lib OK\e[0m"
	@${CP} ${KERNEL_BUILD_DIR}/libkernel.a ${BUILD_TARGET_KERNEL}/
	@${INFO} "Kernel build finish."

ring_scheduler:
	@${INFO} "DEAL WITH Ring Scheduler"
	@${INFO} "Build/Ring_Scheduler"
	@${MKDIR} -p build/ring_scheduler
	@${CARGO} build -p ring_scheduler --${BUILD_MODE} --target  $(BUILD_TARGET_ABI)
	@${INFO} "move to Build/ring_scheduler and \e[35mring_scheduler Static Lib OK\e[0m"
	@${CP} ${RING_SCHEDULER_BUILD_DIR}/libring_scheduler.a ${BUILD_TARGET_RING_SCHEDULER}/
	@${INFO} "Ring Scheduler build finish."


## This builds the kernel binary itself, which is the fully-linked code that first runs right after the bootloader
$(kernel_binary): $(kernel_static_lib) $(linker_script)
	$(CROSS_LD) -n --static -T $(linker_script) -o $(kernel_binary) $(compiled_kernel_asm) $(kernel_static_lib)

$(ring_scheduler_binary): $(ring_scheduer_static_lib) $(linker_script_ring_scheduler)
	$(CROSS_LD) -n --static -T $(linker_script_ring_scheduler) -o $(ring_scheduler_binary) $(ring_scheduler_static_lib)

run-debug:
	@${MAKE} clean && ${MAKE} kernel && ${MAKE} ${kernel_binary} && ${MAKE} ring_scheduler && ${MAKE} ${ring_scheduler_binary} && ${MAKE} fs-img
	@${MAKE} run-debug-inner --no-print-directory
	@${NEWLINE}
	@${INFO} "Kernel Run finish."
	@${INFO} "You Need Clearly the file by yourself"

run:
	@${MAKE} clean && ${MAKE} kernel && ${MAKE} ${kernel_binary} && ${MAKE} ring_scheduler && ${MAKE} ${ring_scheduler_binary} && ${MAKE} fs-img
	@${MAKE} run-inner --no-print-directory
	@${NEWLINE}
	@${INFO} "Kernel Run finish."
	@${MAKE} clean

FS_IMG                    := target/$(TARGET)/$(MODE)fs.img
APPS                      := application/user/src/bin/*

fs-img: $(APPS)
	@mkdir -p build/apps
	@cd application/user && make build TEST=$(TEST)
	@rm -f $(FS_IMG)
	@cd application/easy-fs-fuse && cargo run --release -- -s ../user/src/bin/ -t ../../target/riscv64gc-unknown-none-elf/release/
	@mv target/riscv64gc-unknown-none-elf/release/fs.img build/apps

run-inner:
	qemu-system-riscv64 \
		-machine virt \
		-display none\
		-nographic \
		-bios $(BOOTLOADER) \
		-device loader,file=$(kernel_binary),addr=80200000\
		-device loader,file=$(ring_scheduler_binary),addr=86000000\
		-drive file=build/apps/fs.img,if=none,format=raw,id=x0 \
        -device virtio-blk-device,drive=x0 \
		-device virtio-gpu-device  \
		-device virtio-keyboard-device  \
		-device virtio-mouse-device

# -drive file=$(FS_IMG),if=none,format=raw,id=x0
run-debug-inner:
	qemu-system-riscv64 \
		-machine virt \
		-display none\
		-nographic \
		-bios $(BOOTLOADER) \
		-device loader,file=$(kernel_binary),addr=80200000\
		-device loader,file=$(ring_scheduler_binary),addr=86000000\
		-drive file=build/apps/fs.img,if=none,format=raw,id=x0 \
        -device virtio-blk-device,drive=x0 \
		-device virtio-gpu-device  \
		-device virtio-keyboard-device  \
		-device virtio-mouse-device \
        -s -S
