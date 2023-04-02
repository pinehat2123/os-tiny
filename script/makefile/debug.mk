include script/makefile/mkEnv.mk

.PHONY: debug

debug:
	@${CROSS_GDB}\
    -ex 'file build/kernel/kernel.bin'\
    -ex 'set arch riscv:rv64'\
    -ex 'target remote localhost:1234'
