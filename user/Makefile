
SYSROOT     := $(CURDIR)/sysroot
MUSL_VER    := musl-1.2.6
MUSL_URL    := https://musl.libc.org/releases/$(MUSL_VER).tar.gz

MUSL_CC     := $(SYSROOT)/usr/bin/x86_64-unnamed-gcc
USER_CFLAGS := -static -no-pie -O2 -Wall -Wextra -Wl,-z,noexecstack

OUTPUT_BIN 	:= bin

toolchain: $(MUSL_CC)

$(MUSL_CC):
	@echo "downloading musl..."
	@mkdir -p build_musl $(SYSROOT)/usr/bin
	@cd build_musl && wget -nc $(MUSL_URL) && tar -xf $(MUSL_VER).tar.gz
	
	@echo "building musl for x86_64-unnamed-gcc..."
	@cd build_musl/$(MUSL_VER) && \
		CC=x86_64-elf-gcc \
		AR=x86_64-elf-ar \
		RANLIB=x86_64-elf-ranlib \
		LD=x86_64-elf-ld \
		./configure \
			--target=x86_64-linux-musl \
			--prefix=$(SYSROOT)/usr \
			--syslibdir=$(SYSROOT)/lib \
			--disable-shared
	
	@$(MAKE) -C build_musl/$(MUSL_VER) -j$$(nproc)
	@$(MAKE) -C build_musl/$(MUSL_VER) install
	
	@cd build_musl/$(MUSL_VER) && \
		sh tools/musl-gcc.specs.sh "$(SYSROOT)/usr/include" "$(SYSROOT)/usr/lib" "/lib/ld-musl-x86_64.so.1" \
		> "$(SYSROOT)/usr/lib/musl-gcc.specs"
	@sed -i 's/crtbeginS\.o/crtbegin.o/g; s/crtendS\.o/crtend.o/g; s/-lgcc_eh//g' "$(SYSROOT)/usr/lib/musl-gcc.specs"
	
	@printf '#!/bin/sh\nexec x86_64-elf-gcc -specs "$(SYSROOT)/usr/lib/musl-gcc.specs" "$$@"\n' > $(MUSL_CC)
	@chmod +x $(MUSL_CC)
	@rm -rf build_musl

userspace: toolchain
	@mkdir -p $(OUTPUT_BIN)
	$(MUSL_CC) $(USER_CFLAGS) init.c -o $(OUTPUT_BIN)/init

clean:
	rm -rf $(OUTPUT_BIN)


.PHONY: toolchain userspace
