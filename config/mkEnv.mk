TOOLS_DIR := ./tools
RISCV_DIR := ${TOOLS_DIR}/riscv
RISCV_BIN_DIR := ${RISCV_DIR}/bin

AUTHOR := [my]

MAKE        ?= make
BASH        ?= /bin/bash
CROSS       ?=  riscv64-unknown-linux-gnu
CROSS_AS    ?= ${RISCV_BIN_DIR}/${CROSS}-as
CROSS_LD    ?= ${RISCV_BIN_DIR}/${CROSS}-ld
CROSS_GDB   ?= ${RISCV_BIN_DIR}/${CROSS}-gdb
PRINT       ?= /bin/echo -e "\e[37m\e[4mPRINT\e[0m "
INFO        ?= /bin/echo -e "\e[34mNOTE\e[0m "
