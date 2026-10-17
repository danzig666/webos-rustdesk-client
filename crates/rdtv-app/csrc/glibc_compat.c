/*
 * Link-time compatibility for the webOS SDK sysroot (glibc 2.12), which lacks symbols
 * newer code expects. Every supported TV (webOS 5+, glibc >= 2.28) has the real
 * functions: they are looked up at runtime first, and these fallbacks implement the same
 * semantics from the kernel interfaces (never a fabricated success).
 * Approach after punktfunk/client-webos src/platform/webos/glibc_compat_shim.c (MIT/Apache-2.0).
 */
#define _GNU_SOURCE
#include <dlfcn.h>
#include <errno.h>
#include <fcntl.h>
#include <unistd.h>
#include <sys/syscall.h>

unsigned long getauxval(unsigned long type) {
    static unsigned long (*real)(unsigned long) = 0;
    static int looked = 0;
    if (!looked) {
        looked = 1;
        real = (unsigned long (*)(unsigned long))dlsym(RTLD_NEXT, "getauxval");
    }
    if (real) return real(type);
    int fd = open("/proc/self/auxv", O_RDONLY);
    if (fd < 0) {
        errno = ENOENT;
        return 0;
    }
    unsigned long pair[2];
    unsigned long result = 0;
    int found = 0;
    while (read(fd, pair, sizeof(pair)) == (ssize_t)sizeof(pair)) {
        if (pair[0] == 0) break;
        if (pair[0] == type) {
            result = pair[1];
            found = 1;
            break;
        }
    }
    close(fd);
    if (!found) errno = ENOENT;
    return result;
}
