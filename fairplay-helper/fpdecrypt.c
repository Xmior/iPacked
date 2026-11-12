// fpdecrypt <in-macho> <out-macho>
// Decrypts the FairPlay-encrypted arm64 slice of an iOS app installed on this
// Apple silicon Mac (from the Mac App Store), using the kernel's
// mremap_encrypted() — the same approach as appdecrypt. Copies the file and
// rewrites the encrypted range + sets cryptid = 0. Must run natively (arm64).
// Exit 0: decrypted, 2: not encrypted (copied as-is), 1: error.
#include <errno.h>
#include <fcntl.h>
#include <mach-o/fat.h>
#include <mach-o/loader.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/mman.h>
#include <sys/stat.h>
#include <unistd.h>
#include <libkern/OSByteOrder.h>

extern int mremap_encrypted(void *addr, size_t len, uint32_t cryptid, uint32_t cputype, uint32_t cpusubtype);

static int copy_file(const char *in, const char *out, uint8_t **buf, size_t *len) {
    int fd = open(in, O_RDONLY);
    if (fd < 0) { perror("open in"); return -1; }
    struct stat st;
    fstat(fd, &st);
    *len = (size_t)st.st_size;
    *buf = malloc(*len);
    if (read(fd, *buf, *len) != (ssize_t)*len) { perror("read"); close(fd); return -1; }
    close(fd);
    (void)out;
    return 0;
}

// Decrypt one thin 64-bit Mach-O located at `slice_off` in the file.
static int decrypt_slice(int fd, uint8_t *buf, size_t slice_off, int *did) {
    struct mach_header_64 *mh = (struct mach_header_64 *)(buf + slice_off);
    if (mh->magic != MH_MAGIC_64) return 0; // 32-bit slices can't run on macOS
    uint8_t *lc = (uint8_t *)(mh + 1);
    for (uint32_t i = 0; i < mh->ncmds; i++) {
        struct load_command *c = (struct load_command *)lc;
        if (c->cmd == LC_ENCRYPTION_INFO_64) {
            struct encryption_info_command_64 *e = (struct encryption_info_command_64 *)c;
            if (e->cryptid == 0) return 0;
            size_t page = (size_t)getpagesize();
            size_t map_off = slice_off & ~(page - 1);
            size_t delta = slice_off - map_off;
            size_t map_len = delta + e->cryptoff + e->cryptsize;
            uint8_t *base = mmap(NULL, map_len, PROT_READ | PROT_EXEC, MAP_PRIVATE, fd, (off_t)map_off);
            if (base == MAP_FAILED) { perror("mmap"); return -1; }
            uint8_t *enc = base + delta + e->cryptoff;
            if (mremap_encrypted(enc, e->cryptsize, e->cryptid, mh->cputype, mh->cpusubtype) != 0) {
                fprintf(stderr, "mremap_encrypted failed: %s (uygulama bu Mac'te Mac App Store'dan, aynı Apple ID ile kurulu olmalı)\n", strerror(errno));
                munmap(base, map_len);
                return -1;
            }
            memcpy(buf + slice_off + e->cryptoff, enc, e->cryptsize);
            munmap(base, map_len);
            e->cryptid = 0;
            *did = 1;
            return 0;
        }
        lc += c->cmdsize;
    }
    return 0;
}

int main(int argc, char **argv) {
    if (argc != 3) { fprintf(stderr, "usage: fpdecrypt <in> <out>\n"); return 1; }
    uint8_t *buf; size_t len;
    if (copy_file(argv[1], argv[2], &buf, &len)) return 1;
    int fd = open(argv[1], O_RDONLY);
    int did = 0, err = 0;
    uint32_t magic = *(uint32_t *)buf;
    if (magic == FAT_CIGAM || magic == FAT_MAGIC) {
        struct fat_header *fh = (struct fat_header *)buf;
        uint32_t n = OSSwapBigToHostInt32(fh->nfat_arch);
        struct fat_arch *fa = (struct fat_arch *)(fh + 1);
        for (uint32_t i = 0; i < n && !err; i++)
            err = decrypt_slice(fd, buf, OSSwapBigToHostInt32(fa[i].offset), &did);
    } else {
        err = decrypt_slice(fd, buf, 0, &did);
    }
    close(fd);
    if (err) return 1;
    int out = open(argv[2], O_WRONLY | O_CREAT | O_TRUNC, 0755);
    if (out < 0 || write(out, buf, len) != (ssize_t)len) { perror("write out"); return 1; }
    close(out);
    return did ? 0 : 2;
}
