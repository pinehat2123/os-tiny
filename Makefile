# This is `Makefile`

PJ ?= STARFISH

include config/mkEnv.mk

.PHONY: check \
		clean \
		gitlab gitStatus

check:
	${CROSS_AS} --version;
	${CROSS_LD} --version;
	${CROSS_GDB} --version;

ifeq ($(PJ), LIB)
include  config/lib_run.mk
else
include  config/run.mk
endif

clean:
	@${PERL} ./script/simple-clean clean

gitlab:
	@${INFO} "${AUTHOR}Just Simple git push to gitlab."
	@${PERL} ./script/simple-git pipeline
gitStatus:
	@${INFO} "${AUTHOR}Just git status for the pj."
	@${PERL} ./script/simple-git status