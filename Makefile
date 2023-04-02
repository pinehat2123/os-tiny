# This is `Makefile`

include script/makefile/mkEnv.mk

.PHONY: check \
		simple \
		clean \
		gitlab gitStatus

simple:
	@${MAKE} clean && cargo build -p kernel --release --target riscv64gc-unknown-none-elf

check:
	${CROSS_AS} --version;
	${CROSS_LD} --version;
	${CROSS_GDB} --version;

include  script/makefile/run.mk
include  script/makefile/debug.mk

clean:
# for easy-fs-fuse
	@cd application/easy-fs-fuse && cargo clean
	@${PERL} ./script/simple-clean clean

gitlab:
	@${INFO} "${AUTHOR}Just Simple git push to gitlab."
	@${PERL} ./script/simple-git pipeline
gitStatus:
	@${INFO} "${AUTHOR}Just git status for the pj."
	@${PERL} ./script/simple-git status
