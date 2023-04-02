TOOLS_DIR           := ./tools
RISCV_DIR           := ${TOOLS_DIR}/riscv
RISCV_GDB_DIR       := ${TOOLS_DIR}/riscv-gdb
RISCV_BIN_DIR       := ${RISCV_DIR}/bin
RISCV_GDB_BIN_DIR   := ${RISCV_GDB_DIR}/bin

AUTHOR := [my]

MAKE        ?= make
MKDIR       ?= mkdir
CP          ?= cp
CARGO       ?= cargo
BASH        ?= /bin/bash
PERL        ?= perl
CROSS       ?=  riscv64-unknown-linux-gnu-
CROSS_AS    ?= ${RISCV_BIN_DIR}/${CROSS}as
CROSS_LD    ?= ${RISCV_BIN_DIR}/${CROSS}ld
CROSS_GDB   ?= ${RISCV_GDB_BIN_DIR}/riscv64-unknown-elf-gdb
PRINT       ?= /bin/echo -e "\e[37m\e[4mPRINT\e[0m "
INFO        ?= /bin/echo -e "\e[34mNOTE\e[0m "
NEWLINE     ?= /bin/echo -e "\n"
