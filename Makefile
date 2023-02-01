# This is `Makefile`

include config/mkEnv.mk
include config/config.mk

.PHONY:  kernel run clean dir gitlab check

simple:
	${MAKE} clean && ${MAKE} kernel

check:
	${CROSS_AS} --version;
	${CROSS_LD} --version;
	${CROSS_GDB} --version;

kernel: 
	@${INFO} "DEAL WITH Kernel"
	@${INFO} "Build/Kernel"
	@${MKDIR} -p build/kernel
	@${CARGO} build -p kernel --${BUILD_MODE}
	@${INFO} "move to Build/Kernel"
	@${CP} ${KERNEL_BUILD_DIR}/libkernel.a ${BUILD_TARGET_KERNEL}/
	@${INFO} "Kernel build finish."


	
## This builds the kernel binary itself, which is the fully-linked code that first runs right after the bootloader
$(kernel_binary): cargo $(kernel_static_lib) $(linker_script)
	@$(CROSS_LD) -n -T $(linker_script) -o $(kernel_binary) $(compiled_kernel_asm) $(kernel_static_lib)

dir:
	@${PRINT} "info echo"

clean:
	@${PERL} ./script/simple-clean clean

gitlab:
	@${INFO} "${AUTHOR}Just Simple git push to gitlab."
	@${PERL} ./script/simple-git pipeline
gitStatus:
	@${INFO} "${AUTHOR}Just git status for the pj."
	@${PERL} ./script/simple-git status