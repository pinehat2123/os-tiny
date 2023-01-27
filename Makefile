# This is `Makefile`

include config/mkEnv.mk
include config/config.mk

.PHONY:  kernel run clean dir gitlab



kernel: 
	@${INFO} "DEAL WITH Kernel"

	
## This builds the kernel binary itself, which is the fully-linked code that first runs right after the bootloader
$(kernel_binary): cargo $(kernel_static_lib) $(linker_script)
	@$(CROSS)ld -n -T $(linker_script) -o $(kernel_binary) $(compiled_kernel_asm) $(kernel_static_lib)

dir:
	@${PRINT} "info echo"
clean:
	bash ./script/clean
gitlab:
	@${INFO} "${AUTHOR}Just Simple git push to gitlab."
	bash ./script/quick-push

gitStatus:
	@git status $(ROOT_DIR)