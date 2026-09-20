CC = x86_64-elf-gcc
LD = x86_64-elf-ld
ASM = nasm

CFLAGS = \
    -Wall \
    -Wextra \
    -std=gnu11 \
    -ffreestanding \
    -fno-stack-protector \
    -fno-stack-check \
    -fno-lto \
    -fno-PIC \
    -ffunction-sections \
    -fdata-sections \
    -m64 \
    -march=x86-64 \
    -mabi=sysv \
    -mno-80387 \
    -mno-mmx \
    -mno-sse \
    -mno-sse2 \
    -mno-red-zone \
    -mcmodel=kernel \
	-I src

LDFLAGS = \
    -m elf_x86_64 \
    -nostdlib \
    -static \
    -z max-page-size=0x1000 \
    --gc-sections \
    -T kernel.ld

ASMFLAGS = -f elf64 -Wall

SRCFILES = $(shell find -L src -type f 2>/dev/null | LC_ALL=C sort)
CFILES = $(filter %.c,$(SRCFILES))
ASMFILES = $(filter %.asm,$(SRCFILES))
RUSTFILES = $(filter %.rs,$(SRCFILES))
OBJFILES = $(addprefix build/, $(CFILES:.c=.c.o) $(ASMFILES:.asm=.asm.o))
RUST_LIB = target/x86_64-unknown-none/release/libunnamed.a

all: dependencies image

build/unnamed.elf: $(OBJFILES) $(RUST_LIB)
	$(LD) $(LDFLAGS) $^ -o $@

$(RUST_LIB): $(RUSTFILES)
	cargo build --release --target x86_64-unknown-none

build/%.c.o: %.c
	mkdir -p $(@D)
	$(CC) $(CFLAGS) -c $< -o $@

build/%.asm.o: %.asm
	mkdir -p $(@D)
	$(ASM) $(ASMFLAGS) $< -o $@

dependencies:
	if [ ! -d "limine-binary" ]; then \
		curl -L https://github.com/Limine-Bootloader/Limine/releases/latest/download/limine-binary.tar.gz | gunzip | tar -xf -; \
		make -C limine-binary; \
	fi

image: build/unnamed.elf
	# Create an empty zeroed-out 64MiB image file.
	dd if=/dev/zero of=build/image.hdd bs=1M count=64 conv=fsync

	# Create a partition table.
	PATH=$PATH:/usr/sbin:/sbin sgdisk build/image.hdd -n 1:2048 -t 1:ef00 -m 1

	# Install the Limine BIOS stages onto the image.
	./limine-binary/limine bios-install build/image.hdd

	# Format the image as fat32.
	mformat -i build/image.hdd@@1M

	# Make relevant subdirectories.
	mmd -i build/image.hdd@@1M ::/EFI ::/EFI/BOOT ::/boot ::/boot/limine

	# Copy over the relevant files.
	mcopy -i build/image.hdd@@1M build/unnamed.elf ::/boot
	mcopy -i build/image.hdd@@1M limine.conf limine-binary/limine-bios.sys ::/boot/limine
	mcopy -i build/image.hdd@@1M limine-binary/BOOTX64.EFI ::/EFI/BOOT
	mcopy -i build/image.hdd@@1M limine-binary/BOOTIA32.EFI ::/EFI/BOOT

run: image
	qemu-system-x86_64 \
	-m 128M \
	-drive if=pflash,format=raw,readonly=on,file=OVMF_CODE_4M.fd \
    -drive if=pflash,format=raw,file=OVMF_VARS_4M.fd \
    -drive if=ide,file=build/image.hdd,format=raw \
	-serial stdio 2>&1 \
	-smp 2

clean:
	rm -rf build/
	cargo clean

.PHONY: all clean run image dependencies