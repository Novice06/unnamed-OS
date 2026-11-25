/*
 * utest.c : userspace test suite for the syscall layer.
 *
 * Build it as /bin/test:   $(MUSL_CC) $(USER_CFLAGS) test.c -o $(OUTPUT_BIN)/test
 * init.c runs it with:     execve("/bin/test", {"/bin/test", "--once"}, ...)
 * With "--once" it exits with 0 (all good) or 1; without it, it idles forever.
 *
 * It only needs: open, close, read, write, writev, fork, execve, wait4,
 * ioctl, exit_group (+ what musl does at startup). Missing syscalls are
 * reported by the "syscall_inventory" test instead of making everything fail.
 *
 * Output goes through write(1) directly (no stdio buffering).
 * The same file also runs on a real Linux to check the tests themselves:
 *   gcc -O2 -DHOST_TEST test.c -o /tmp/utest && rm -f /tmp/ut_* && /tmp/utest
 */
#define _GNU_SOURCE
#include <errno.h>
#include <fcntl.h>
#include <stdarg.h>
#include <stdint.h>
#include <stdio.h>
#include <signal.h>
#include <stdlib.h>
#include <string.h>
#include <sys/auxv.h>
#include <sys/syscall.h>
#include <sys/types.h>
#include <sys/uio.h>
#include <sys/wait.h>
#include <unistd.h>

#ifdef HOST_TEST
#define ROOT    "/tmp/ut_"
#define CONSOLE "/dev/null"
#else
#define ROOT    "/"
#define CONSOLE "/dev/console"
#endif
#define FP(name) ROOT name /* path of a scratch file */

/* the bad-pointer tests pass invalid pointers on purpose */
#pragma GCC diagnostic ignored "-Wnonnull"
#pragma GCC diagnostic ignored "-Wstringop-overflow"
#pragma GCC diagnostic ignored "-Wstringop-overread"

enum { T_FAIL = 0, T_PASS = 1, T_SKIP = 2 };

static char *self_path;
static int g_argc;
static char **g_argv;

/* ------------------------------------------------------------ harness */

static void say(const char *fmt, ...)
{
    char buf[256];
    va_list ap;
    va_start(ap, fmt);
    int n = vsnprintf(buf, sizeof buf, fmt, ap);
    va_end(ap);
    if (n > (int)sizeof buf - 1)
        n = (int)sizeof buf - 1;
    if (n > 0) {
        ssize_t r = write(1, buf, (size_t)n);
        (void)r;
    }
}

static const char *errname(int e)
{
    switch (e) {
    case 0:       return "none";
    case EPERM:   return "EPERM";
    case ENOENT:  return "ENOENT";
    case ECHILD:  return "ECHILD";
    case EBADF:   return "EBADF";
    case ENOMEM:  return "ENOMEM";
    case EACCES:  return "EACCES";
    case EFAULT:  return "EFAULT";
    case EEXIST:  return "EEXIST";
    case ENOTDIR: return "ENOTDIR";
    case EISDIR:  return "EISDIR";
    case EINVAL:  return "EINVAL";
    case EMFILE:  return "EMFILE";
    case ENOTTY:  return "ENOTTY";
    case ENOSYS:  return "ENOSYS";
    case ENOEXEC: return "ENOEXEC";
    default:      return "?";
    }
}

#define FAILF(...)                                      \
    do {                                                \
        say("    FAIL line %d: ", __LINE__);            \
        say(__VA_ARGS__);                               \
        say("\n");                                      \
        return T_FAIL;                                  \
    } while (0)

#define EXPECT(cond)                                                          \
    do {                                                                      \
        if (!(cond))                                                          \
            FAILF("%s (errno=%d %s)", #cond, errno, errname(errno));          \
    } while (0)

/* the call must fail with -1 and exactly this errno */
#define EXPECT_ERR(call, want)                                                \
    do {                                                                      \
        errno = 0;                                                            \
        long r_ = (long)(call);                                               \
        int e_ = errno;                                                       \
        if (r_ != -1 || e_ != (want))                                         \
            FAILF("%s: got %ld errno=%d(%s), want -1 errno=%d(%s)", #call,    \
                  r_, e_, errname(e_), (want), errname(want));                \
    } while (0)

/* ------------------------------------------------------------ helpers */

static unsigned char pat(size_t i)
{
    return (unsigned char)((i * 131u + 7u) ^ (i >> 8));
}

static int write_all(int fd, const void *buf, size_t n)
{
    const char *p = buf;
    while (n) {
        ssize_t w = write(fd, p, n);
        if (w <= 0)
            return -1;
        p += w;
        n -= (size_t)w;
    }
    return 0;
}

static int make_file(const char *path, const char *data, mode_t mode)
{
    int fd = open(path, O_CREAT | O_WRONLY | O_TRUNC, mode);
    if (fd < 0)
        return -1;
    int rc = write_all(fd, data, strlen(data));
    close(fd);
    return rc;
}

/* children poll for a file instead of sleeping (no sleep syscall needed) */
static int wait_for_file(const char *path)
{
    for (long i = 0; i < 5000000; i++) {
        int fd = open(path, O_RDONLY);
        if (fd >= 0) {
            close(fd);
            return 0;
        }
    }
    return -1;
}

/* exit code of a child, or <= -1000 if it did not exit normally */
static int reap(pid_t pid)
{
    int st = 0;
    pid_t r = waitpid(pid, &st, 0);
    if (r != pid)
        return -1000;
    if (!WIFEXITED(st))
        return -1001;
    return WEXITSTATUS(st);
}

/* Runs body() in a child and polls for its result: a test that blocks forever
 * (lost wake-up, missing EOF) becomes a FAIL instead of freezing the suite. */
#define GUARD_POLLS 4000000L
static int guarded(int (*body)(void))
{
    pid_t p = fork();
    if (p < 0)
        FAILF("fork failed (errno=%d)", errno);
    if (p == 0)
        _exit(body() == T_PASS ? 0 : 1);
    for (long i = 0; i < GUARD_POLLS; i++) {
        int st = 0;
        pid_t r = waitpid(p, &st, WNOHANG);
        if (r == p)
            return (WIFEXITED(st) && WEXITSTATUS(st) == 0) ? T_PASS : T_FAIL;
        if (r < 0)
            FAILF("waitpid failed (errno=%d)", errno);
    }
    FAILF("no result after %ld polls: the test is blocked "
          "(lost wake-up? missing EOF?)", GUARD_POLLS);
}

static unsigned char wbuf[20000];
static unsigned char rbuf[20000];
static volatile unsigned char bss_probe[40000]; /* never written: must be zero */

static volatile int g_data = 1; /* .data */
static volatile int g_bss;      /* .bss  */
static volatile unsigned char g_big[30000];

/* ------------------------------------------------- 1. process start-up */

static int t_stack_auxv(void)
{
    EXPECT(g_argc >= 1);
    EXPECT(g_argv[g_argc] == NULL);
    /* argc sits at a 16-byte aligned address: argv is at 8 mod 16 */
    if (((uintptr_t)g_argv & 15) != 8)
        FAILF("initial stack is not 16-byte aligned (argv=%p)", (void *)g_argv);
    EXPECT(getauxval(AT_PAGESZ) == 4096);
    EXPECT(getauxval(AT_RANDOM) != 0);
    EXPECT(getauxval(AT_ENTRY) != 0);
    if (getauxval(AT_PHDR) == 0 || getauxval(AT_PHNUM) == 0)
        FAILF("AT_PHDR/AT_PHNUM are 0 (musl needs them to find PT_TLS)");
    return T_PASS;
}

static int t_bss_zero(void)
{
    for (size_t i = 0; i < sizeof bss_probe; i++)
        if (bss_probe[i])
            FAILF("bss byte %zu = %u (frames given to user must be zeroed)", i, bss_probe[i]);
    return T_PASS;
}

/* report-only: which common syscalls are still ENOSYS */
static int t_inventory(void)
{
    static const struct { long nr; const char *name; } probes[] = {
        {4, "stat"},  {5, "fstat"},     {8, "lseek"},       {9, "mmap"},
        {10, "mprotect"}, {11, "munmap"}, {12, "brk"},      {13, "rt_sigaction"},
        {14, "rt_sigprocmask"}, {17, "pread64"}, {18, "pwrite64"}, {19, "readv"},
        {22, "pipe"}, {24, "sched_yield"}, {32, "dup"},     {33, "dup2"},
        {35, "nanosleep"}, {39, "getpid"}, {63, "uname"},   {72, "fcntl"},
        {79, "getcwd"}, {80, "chdir"},  {83, "mkdir"},      {87, "unlink"},
        {102, "getuid"}, {110, "getppid"}, {186, "gettid"}, {202, "futex"},
        {217, "getdents64"}, {228, "clock_gettime"}, {262, "newfstatat"},
        {292, "dup3"}, {293, "pipe2"},
    };
    int missing = 0;
    say("    missing:");
    for (size_t i = 0; i < sizeof probes / sizeof probes[0]; i++) {
        errno = 0;
        long r = syscall(probes[i].nr, -1L, 0L, 0L, 0L, 0L, 0L);
        if (r == -1 && errno == ENOSYS) {
            say(" %s(%ld)", probes[i].name, probes[i].nr);
            missing++;
        }
    }
    say("%s\n", missing ? "" : " none");
    return T_PASS;
}

/* ------------------------------------------------------- 2. files / vfs */

static int t_open_errors(void)
{
    char buf[8];
    int fd;

    EXPECT_ERR(open("/does/not/exist", O_RDONLY), ENOENT);
    EXPECT_ERR(open(FP("t_missing"), O_RDONLY), ENOENT);
    EXPECT_ERR(open("", O_RDONLY), ENOENT);
    EXPECT_ERR(open("/", O_WRONLY), EISDIR);
    EXPECT_ERR(open("/", O_RDWR), EISDIR);
    EXPECT_ERR(open(CONSOLE, O_RDONLY | O_DIRECTORY), ENOTDIR);
    EXPECT_ERR(open("/no_such_dir/f", O_CREAT | O_WRONLY, 0644), ENOENT);

    fd = open(FP("t_ex"), O_CREAT | O_EXCL | O_WRONLY, 0644);
    EXPECT(fd >= 0);
    EXPECT(close(fd) == 0);
    EXPECT_ERR(open(FP("t_ex"), O_CREAT | O_EXCL | O_WRONLY, 0644), EEXIST);
    EXPECT_ERR(open(FP("t_ex/x"), O_RDONLY), ENOTDIR);

    /* access modes are enforced */
    fd = open(FP("t_ex"), O_RDONLY);
    EXPECT(fd >= 0);
    EXPECT_ERR(write(fd, "x", 1), EBADF);
    EXPECT(close(fd) == 0);

    fd = open(FP("t_ex"), O_WRONLY);
    EXPECT(fd >= 0);
    EXPECT_ERR(read(fd, buf, 1), EBADF);
    EXPECT(close(fd) == 0);

    fd = open(FP("t_ex"), O_RDONLY | O_APPEND); /* O_APPEND must not grant write access */
    EXPECT(fd >= 0);
    EXPECT_ERR(write(fd, "x", 1), EBADF);
    EXPECT(close(fd) == 0);

    /* path normalisation: "//." + ROOT + name */
    char odd[64];
    snprintf(odd, sizeof odd, "//.%s%s", ROOT, "t_ex");
    fd = open(odd, O_RDONLY);
    EXPECT(fd >= 0);
    EXPECT(close(fd) == 0);

    /* bad descriptors, double close */
    EXPECT_ERR(close(9999), EBADF);
    EXPECT_ERR(close(-1), EBADF);
    EXPECT_ERR(read(9999, buf, 1), EBADF);
    EXPECT_ERR(write(-1, "x", 1), EBADF);
    fd = open(FP("t_ex"), O_RDONLY);
    EXPECT(fd >= 0);
    EXPECT(close(fd) == 0);
    EXPECT_ERR(close(fd), EBADF);
    return T_PASS;
}

static int t_file_roundtrip(void)
{
    for (size_t i = 0; i < sizeof wbuf; i++)
        wbuf[i] = pat(i);

    int fd = open(FP("t_rw"), O_CREAT | O_EXCL | O_WRONLY, 0644);
    EXPECT(fd >= 0);
    EXPECT(write_all(fd, wbuf, sizeof wbuf) == 0); /* spans 5 user pages */
    EXPECT(close(fd) == 0);

    /* read back with awkward chunk sizes */
    fd = open(FP("t_rw"), O_RDONLY);
    EXPECT(fd >= 0);
    static const size_t sizes[] = { 1, 7, 100, 4095, 4096, 4097, 1000, 3333 };
    size_t got = 0, k = 0;
    memset(rbuf, 0, sizeof rbuf);
    while (got < sizeof rbuf) {
        size_t want = sizes[k++ % (sizeof sizes / sizeof sizes[0])];
        if (want > sizeof rbuf - got)
            want = sizeof rbuf - got;
        ssize_t n = read(fd, rbuf + got, want);
        EXPECT(n == (ssize_t)want);
        got += (size_t)n;
    }
    EXPECT(read(fd, rbuf, 16) == 0); /* EOF */
    EXPECT(memcmp(wbuf, rbuf, sizeof wbuf) == 0);
    EXPECT(close(fd) == 0);

    /* one big read */
    fd = open(FP("t_rw"), O_RDONLY);
    EXPECT(fd >= 0);
    memset(rbuf, 0, sizeof rbuf);
    EXPECT(read(fd, rbuf, 30000 > sizeof rbuf ? sizeof rbuf : 30000) == (ssize_t)sizeof rbuf);
    EXPECT(memcmp(wbuf, rbuf, sizeof wbuf) == 0);
    EXPECT(close(fd) == 0);
    return T_PASS;
}

static int t_trunc_append(void)
{
    char buf[64];
    int fd = open(FP("t_ta"), O_CREAT | O_WRONLY | O_TRUNC, 0644);
    EXPECT(fd >= 0);
    EXPECT(write(fd, "abc", 3) == 3);
    EXPECT(close(fd) == 0);

    fd = open(FP("t_ta"), O_WRONLY | O_APPEND);
    EXPECT(fd >= 0);
    EXPECT(write(fd, "def", 3) == 3);
    EXPECT(write(fd, "ghi", 3) == 3);
    EXPECT(close(fd) == 0);

    fd = open(FP("t_ta"), O_RDONLY);
    EXPECT(fd >= 0);
    EXPECT(read(fd, buf, sizeof buf) == 9);
    EXPECT(memcmp(buf, "abcdefghi", 9) == 0);
    EXPECT(close(fd) == 0);

    fd = open(FP("t_ta"), O_WRONLY | O_TRUNC);
    EXPECT(fd >= 0);
    EXPECT(close(fd) == 0);
    fd = open(FP("t_ta"), O_RDONLY);
    EXPECT(fd >= 0);
    EXPECT(read(fd, buf, sizeof buf) == 0);
    EXPECT(close(fd) == 0);
    return T_PASS;
}

static int t_console(void)
{
    int fd = open(CONSOLE, O_RDWR);
    EXPECT(fd >= 0);

    static const char msg[] = "    [console write ok]\n";
    EXPECT(write(fd, msg, sizeof msg - 1) == (ssize_t)(sizeof msg - 1));
    EXPECT(write(fd, "", 0) == 0);

    memset(wbuf, '.', 600); /* bigger than the driver's 256-byte chunks */
    wbuf[599] = '\n';
    EXPECT(write(fd, wbuf, 600) == 600);

    struct iovec iov[3] = {
        { (void *)"    [wv:", 8 }, { (void *)"abc", 3 }, { (void *)"]\n", 2 },
    };
    EXPECT(writev(fd, iov, 3) == 13);
    EXPECT(writev(fd, iov, 0) == 0);
    EXPECT_ERR(writev(fd, (struct iovec *)1, 2), EFAULT);

    char buf[16];
    EXPECT(read(fd, buf, sizeof buf) == 0); /* no input yet: EOF */
    EXPECT(close(fd) == 0);
    return T_PASS;
}

/* the console is not a tty yet: update this test when you add termios */
static int t_ioctl(void)
{
    int fd = open(CONSOLE, O_RDWR);
    EXPECT(fd >= 0);
    errno = 0;
    EXPECT(isatty(fd) == 0);
    EXPECT(errno == ENOTTY);
    EXPECT(close(fd) == 0);
    return T_PASS;
}

static int t_fd_table(void)
{
    int fds[512]; /* the user stack is small: keep locals modest */
    int n = 0;
    while (n < 512) {
        int fd = open(CONSOLE, O_WRONLY);
        if (fd < 0) {
            EXPECT(errno == EMFILE); /* the per-process limit must report EMFILE */
            break;
        }
        fds[n++] = fd;
    }
    EXPECT(n > 20);
    for (int i = 0; i < n; i++)
        EXPECT(close(fds[i]) == 0);

    /* the lowest free fd is always handed out */
    int a = open(CONSOLE, O_WRONLY);
    int b = open(CONSOLE, O_WRONLY);
    int c = open(CONSOLE, O_WRONLY);
    EXPECT(a >= 0 && b == a + 1 && c == b + 1);
    EXPECT(close(b) == 0);
    int d = open(CONSOLE, O_WRONLY);
    EXPECT(d == b);
    EXPECT(close(a) == 0 && close(c) == 0 && close(d) == 0);
    return T_PASS;
}

/* ---------------------------------------------- 3. user pointers (EFAULT) */

static int t_bad_pointers(void)
{
    /* a regular file: unlike /dev/null it really has to read the buffer */
    int fd = open(FP("t_bp0"), O_CREAT | O_WRONLY | O_TRUNC, 0644);
    EXPECT(fd >= 0);

    EXPECT_ERR(write(fd, (void *)1, 4), EFAULT);
    EXPECT_ERR(write(fd, (void *)0x10000000UL, 4), EFAULT);          /* user range, unmapped */
    EXPECT_ERR(write(fd, (void *)0x0000800000000000UL, 4), EFAULT);  /* first non-canonical */
    EXPECT_ERR(write(fd, (void *)0xFFFF800000001000UL, 4), EFAULT);  /* kernel half */
    EXPECT_ERR(write(fd, (void *)(0x0000800000000000UL - 2), 4), EFAULT); /* crosses the end */
    EXPECT_ERR(open((char *)1, O_RDONLY), EFAULT);
    EXPECT_ERR(open((char *)0x10000000UL, O_RDONLY), EFAULT);
    EXPECT_ERR(execve((char *)1, NULL, NULL), EFAULT);
    EXPECT(close(fd) == 0);

    /* read() must not write into read-only user memory (the code) */
    EXPECT(make_file(FP("t_bp"), "hello", 0644) == 0);
    fd = open(FP("t_bp"), O_RDONLY);
    EXPECT(fd >= 0);
    EXPECT_ERR(read(fd, (void *)1, 5), EFAULT);
    EXPECT_ERR(read(fd, (void *)t_bad_pointers, 5), EFAULT);
    EXPECT(close(fd) == 0);
    return T_PASS;
}

/* Linux: a read() that fails with EFAULT does not consume the data */
static int t_read_no_consume(void)
{
    char buf[8];
    EXPECT(make_file(FP("t_nc"), "hello", 0644) == 0);
    int fd = open(FP("t_nc"), O_RDONLY);
    EXPECT(fd >= 0);
    EXPECT_ERR(read(fd, (void *)1, 5), EFAULT);
    ssize_t n = read(fd, buf, sizeof buf);
    if (n != 5)
        FAILF("after a failed read the next read returned %ld (want 5): "
              "the offset was advanced although nothing reached user space", (long)n);
    EXPECT(memcmp(buf, "hello", 5) == 0);
    EXPECT(close(fd) == 0);
    return T_PASS;
}

/* ------------------------------------------------------- 4. fork / wait */

static int t_fork_basic(void)
{
    pid_t p = fork();
    EXPECT(p >= 0);
    if (p == 0)
        _exit(42);
    EXPECT(reap(p) == 42);

    static const int codes[] = { 0, 1, 2, 127, 128, 255 };
    for (size_t i = 0; i < sizeof codes / sizeof codes[0]; i++) {
        p = fork();
        EXPECT(p >= 0);
        if (p == 0)
            _exit(codes[i]);
        int got = reap(p);
        if (got != codes[i])
            FAILF("child exited with %d, reaped %d", codes[i], got);
    }

    p = fork();
    EXPECT(p >= 0);
    if (p == 0)
        _exit(300); /* only the low 8 bits count */
    EXPECT(reap(p) == (300 & 0xff));
    return T_PASS;
}

static int t_wait_errors(void)
{
    EXPECT_ERR(wait(NULL), ECHILD);
    EXPECT_ERR(waitpid(-1, NULL, 0), ECHILD);
    EXPECT_ERR(waitpid(-1, NULL, WNOHANG), ECHILD);
    EXPECT_ERR(waitpid(99999, NULL, 0), ECHILD);

    /* a NULL status pointer is legal and must not lose the child */
    pid_t p = fork();
    EXPECT(p >= 0);
    if (p == 0)
        _exit(3);
    EXPECT(waitpid(p, NULL, 0) == p);
    EXPECT_ERR(waitpid(p, NULL, 0), ECHILD); /* a child is reaped once */

    /* wait() hands out each child exactly once */
    pid_t kids[3];
    for (int i = 0; i < 3; i++) {
        kids[i] = fork();
        if (kids[i] == 0)
            _exit(10 + i);
        EXPECT(kids[i] > 0);
    }
    int seen = 0;
    for (int i = 0; i < 3; i++) {
        int st = 0;
        pid_t r = wait(&st);
        EXPECT(r > 0 && WIFEXITED(st));
        int idx = -1;
        for (int j = 0; j < 3; j++)
            if (kids[j] == r)
                idx = j;
        EXPECT(idx >= 0);
        EXPECT(WEXITSTATUS(st) == 10 + idx);
        seen |= 1 << idx;
    }
    EXPECT(seen == 7);
    EXPECT_ERR(wait(NULL), ECHILD);
    return T_PASS;
}

static int t_wnohang(void)
{
    pid_t p = fork();
    EXPECT(p >= 0);
    if (p == 0) /* cannot exit before the parent creates the flag */
        _exit(wait_for_file(FP("t_flag1")) == 0 ? 7 : 99);

    int st = 0;
    pid_t r = waitpid(p, &st, WNOHANG);
    if (r != 0)
        FAILF("WNOHANG on a running child returned %d (want 0)", (int)r);

    EXPECT(make_file(FP("t_flag1"), "go", 0644) == 0);
    int code = reap(p);
    if (code != 7)
        FAILF("child exit code %d (99 = never saw the flag)", code);
    return T_PASS;
}

static int t_fork_isolation(void)
{
    int local = 7;
    g_data = 5;
    g_bss = 6;
    for (size_t i = 0; i < sizeof g_big; i++)
        g_big[i] = pat(i);

    /* three children in a row: each must see the PARENT's values, not a
     * previous child's writes (catches a shared or reused address space) */
    for (int i = 0; i < 3; i++) {
        pid_t p = fork();
        EXPECT(p >= 0);
        if (p == 0) {
            if (g_data != 5 || g_bss != 6 || local != 7)
                _exit(1);
            if (g_big[12345] != pat(12345) || g_big[29999] != pat(29999))
                _exit(2);
            g_data = 100 + i;
            g_bss = 200 + i;
            local = 300 + i;
            g_big[12345] = 0xEE;
            if (g_data != 100 + i)
                _exit(3);
            _exit(0);
        }
        int code = reap(p);
        if (code != 0)
            FAILF("child %d failed check #%d (1 scalars, 2 big array, 3 own write)", i, code);
        if (g_data != 5 || g_bss != 6 || local != 7 || g_big[12345] != pat(12345))
            FAILF("child %d modified the parent's memory", i);
    }

    /* the parent's later writes must not reach an already running child */
    pid_t p = fork();
    EXPECT(p >= 0);
    if (p == 0) {
        if (wait_for_file(FP("t_flag2")) != 0)
            _exit(2);
        _exit(g_data == 5 ? 0 : 1);
    }
    g_data = 999;
    EXPECT(make_file(FP("t_flag2"), "go", 0644) == 0);
    int code = reap(p);
    g_data = 5;
    if (code != 0)
        FAILF("child saw the parent's write after fork (code %d)", code);
    return T_PASS;
}

static int t_fork_fds(void)
{
    char buf[32];

    /* an open file description is shared: so is its offset */
    int fd = open(FP("t_off"), O_CREAT | O_RDWR | O_TRUNC, 0644);
    EXPECT(fd >= 0);
    EXPECT(write(fd, "AAAA", 4) == 4);
    pid_t p = fork();
    EXPECT(p >= 0);
    if (p == 0)
        _exit(write(fd, "BB", 2) == 2 ? 0 : 1);
    EXPECT(reap(p) == 0);
    EXPECT(write(fd, "CC", 2) == 2);
    EXPECT(close(fd) == 0);

    fd = open(FP("t_off"), O_RDONLY);
    EXPECT(fd >= 0);
    ssize_t n = read(fd, buf, sizeof buf);
    if (n != 8 || memcmp(buf, "AAAABBCC", 8) != 0)
        FAILF("file holds %ld bytes, want \"AAAABBCC\": the offset is not shared across fork", (long)n);
    EXPECT(close(fd) == 0);

    /* each process has its own table: the child closing its copy keeps ours open */
    fd = open(FP("t_off"), O_WRONLY | O_APPEND);
    EXPECT(fd >= 0);
    p = fork();
    EXPECT(p >= 0);
    if (p == 0)
        _exit(close(fd) == 0 ? 0 : 1);
    EXPECT(reap(p) == 0);
    EXPECT(write(fd, "!", 1) == 1);
    EXPECT(close(fd) == 0);

    /* ...and a new fd opened in the child gets the same number as in the parent */
    p = fork();
    EXPECT(p >= 0);
    if (p == 0) {
        int f = open(FP("t_off"), O_RDONLY);
        say("here !");
        _exit(f < 0 ? 255 : f);
    }
    int pf = open(FP("t_off"), O_RDONLY);
    EXPECT(pf >= 0);
    int cf = reap(p);
    EXPECT(close(pf) == 0);
    if (cf != pf)
        FAILF("child's first free fd is %d, parent's is %d", cf, pf);
    return T_PASS;
}

/* adoption by init only makes sense when this program IS pid 1 */
static int t_orphans(void)
{
    errno = 0;
    long me = syscall(SYS_getpid);
    if (me < 0 && errno == ENOSYS) {
        say("    getpid is not implemented\n");
        return T_SKIP;
    }
    if (me != 1) {
        say("    not pid 1 (pid %ld)\n", me);
        return T_SKIP;
    }

    pid_t p = fork();
    EXPECT(p >= 0);
    if (p == 0) {
        pid_t g = fork();
        if (g == 0)
            _exit(wait_for_file(FP("t_flag3")) == 0 ? 33 : 99);
        _exit(g > 0 ? 0 : 1); /* dies first: the grandchild is orphaned */
    }
    EXPECT(reap(p) == 0);
    EXPECT(make_file(FP("t_flag3"), "go", 0644) == 0);

    int st = 0;
    pid_t r = waitpid(-1, &st, 0); /* the adopted orphan is now OUR child */
    EXPECT(r > 0 && r != p);
    if (!WIFEXITED(st) || WEXITSTATUS(st) != 33)
        FAILF("orphan status 0x%x (want exit code 33)", st);
    return T_PASS;
}

/* ------------------------------------------------------------------ pipes */

static int pipe_basic_body(void)
{
    int fds[2];
    char buf[32];

    EXPECT(pipe(fds) == 0);
    EXPECT(fds[0] >= 3 && fds[1] > fds[0]); /* the two lowest free fds */
    EXPECT(write(fds[1], "hello world", 11) == 11);
    EXPECT(read(fds[0], buf, 4) == 4 && memcmp(buf, "hell", 4) == 0);
    EXPECT(read(fds[0], buf, 32) == 7 && memcmp(buf, "o world", 7) == 0);
    EXPECT(write(fds[1], "", 0) == 0);  /* zero-length: must return at once... */
    EXPECT(read(fds[0], buf, 0) == 0);  /* ...even on an empty pipe */

    EXPECT(write(fds[1], "x", 1) == 1);
    EXPECT(close(fds[1]) == 0);
    EXPECT(read(fds[0], buf, 32) == 1); /* data written before the close is still there */
    EXPECT(read(fds[0], buf, 32) == 0); /* then EOF, repeatedly */
    EXPECT(read(fds[0], buf, 32) == 0);
    EXPECT(close(fds[0]) == 0);
    return T_PASS;
}

static int pipe_errors_body(void)
{
    int fds[2];
    char buf[8];

    EXPECT(pipe(fds) == 0);
    EXPECT_ERR(read(fds[1], buf, 1), EBADF);  /* the write end is not readable */
    EXPECT_ERR(write(fds[0], "x", 1), EBADF); /* the read end is not writable */
    EXPECT(close(fds[0]) == 0 && close(fds[1]) == 0);

    /* a failed pipe() must not leak descriptors */
    int a = open(CONSOLE, O_WRONLY);
    EXPECT(a >= 0);
    EXPECT(close(a) == 0);
    int *volatile bad = (int *)1, *volatile null = NULL;
    EXPECT_ERR(pipe(bad), EFAULT);
    EXPECT_ERR(pipe(null), EFAULT);
    int b = open(CONSOLE, O_WRONLY);
    EXPECT(b == a);
    EXPECT(close(b) == 0);

    /* EPIPE once every read end is closed (ignore SIGPIPE so Linux does not kill us) */
    signal(SIGPIPE, SIG_IGN);
    EXPECT(pipe(fds) == 0);
    EXPECT(close(fds[0]) == 0);
    EXPECT_ERR(write(fds[1], "x", 1), EPIPE);
    EXPECT(close(fds[1]) == 0);
    return T_PASS;
}

#define BIG_TOTAL 200000 /* far more than the pipe can hold: the writer must block */

static int pipe_transfer_body(void)
{
    int fds[2];
    EXPECT(pipe(fds) == 0);
    pid_t p = fork();
    EXPECT(p >= 0);
    if (p == 0) {
        close(fds[0]);
        unsigned char chunk[1000];
        size_t sent = 0;
        while (sent < BIG_TOTAL) {
            size_t n = BIG_TOTAL - sent < sizeof chunk ? BIG_TOTAL - sent : sizeof chunk;
            for (size_t i = 0; i < n; i++)
                chunk[i] = pat(sent + i);
            if (write_all(fds[1], chunk, n) != 0)
                _exit(1);
            sent += n;
        }
        _exit(0);
    }

    EXPECT(close(fds[1]) == 0); /* keep ONLY the read end, or EOF never comes */
    unsigned char buf[777];     /* odd size on purpose */
    size_t got = 0;
    for (;;) {
        ssize_t n = read(fds[0], buf, sizeof buf);
        EXPECT(n >= 0);
        if (n == 0)
            break;
        for (ssize_t i = 0; i < n; i++)
            if (buf[i] != pat(got + (size_t)i))
                FAILF("byte %zu corrupted", got + (size_t)i);
        got += (size_t)n;
    }
    EXPECT(got == BIG_TOTAL);
    EXPECT(close(fds[0]) == 0);
    EXPECT(reap(p) == 0);
    return T_PASS;
}

#define REC  100  /* well under PIPE_BUF (4096): each write() must stay in one piece */
#define RECS 4000

static int pipe_atomic_body(void)
{
    int fds[2];
    EXPECT(pipe(fds) == 0);
    pid_t kids[2];
    for (int k = 0; k < 2; k++) {
        kids[k] = fork();
        EXPECT(kids[k] >= 0);
        if (kids[k] == 0) {
            close(fds[0]);
            unsigned char rec[REC];
            memset(rec, 'A' + k, REC);
            for (int i = 0; i < RECS; i++)
                if (write_all(fds[1], rec, REC) != 0)
                    _exit(1);
            _exit(0);
        }
    }
    EXPECT(close(fds[1]) == 0);

    unsigned char rec[REC];
    int count = 0, seen[2] = { 0, 0 };
    for (;;) {
        size_t got = 0;
        while (got < REC) { /* never read across a record boundary */
            ssize_t n = read(fds[0], rec + got, REC - got);
            EXPECT(n >= 0);
            if (n == 0)
                break;
            got += (size_t)n;
        }
        if (got == 0)
            break;
        EXPECT(got == REC);
        for (int i = 1; i < REC; i++)
            if (rec[i] != rec[0])
                FAILF("record %d is interleaved: a %d-byte write() was split", count, REC);
        EXPECT(rec[0] == 'A' || rec[0] == 'B');
        seen[rec[0] - 'A']++;
        count++;
    }
    EXPECT(count == 2 * RECS && seen[0] == RECS && seen[1] == RECS);
    EXPECT(close(fds[0]) == 0);
    EXPECT(reap(kids[0]) == 0 && reap(kids[1]) == 0);
    return T_PASS;
}

/* A process's descriptors must be closed when it EXITS, not when the parent
 * calls wait(): shell pipelines depend on it. If this test is reported as
 * blocked, the exit path does not release the fd table. */
static int pipe_exit_body(void)
{
    int fds[2];
    char c;
    EXPECT(pipe(fds) == 0);
    pid_t p = fork();
    EXPECT(p >= 0);
    if (p == 0) {
        close(fds[0]);
        if (write(fds[1], "x", 1) != 1)
            _exit(1);
        _exit(wait_for_file(FP("t_flag4")) == 0 ? 0 : 2);
    }
    EXPECT(close(fds[1]) == 0);
    EXPECT(read(fds[0], &c, 1) == 1 && c == 'x');
    EXPECT(make_file(FP("t_flag4"), "go", 0644) == 0);
    EXPECT(read(fds[0], &c, 1) == 0); /* EOF, although we have NOT waited for the child */
    EXPECT(close(fds[0]) == 0);
    EXPECT(reap(p) == 0);
    return T_PASS;
}

static int pipe2_flags_body(void)
{
    int fds[2];
    EXPECT_ERR(pipe2(fds, O_APPEND), EINVAL); /* not a valid pipe2 flag */
    EXPECT(pipe2(fds, O_CLOEXEC) == 0);
    EXPECT(close(fds[0]) == 0 && close(fds[1]) == 0);
    return T_PASS;
}

static int t_pipe_basic(void)    { return guarded(pipe_basic_body); }
static int t_pipe_errors(void)   { return guarded(pipe_errors_body); }
static int t_pipe_transfer(void) { return guarded(pipe_transfer_body); }
static int t_pipe_atomic(void)   { return guarded(pipe_atomic_body); }
static int t_pipe_exit(void)     { return guarded(pipe_exit_body); }
static int t_pipe2_flags(void)   { return guarded(pipe2_flags_body); }

/* -------------------------------------------------------------- 5. execve */

static int exec_child(int argc, char **argv)
{
    if (argc != 7)
        return 50;
    if (strcmp(argv[2], "hello") != 0)
        return 51;
    const char *foo = getenv("FOO"), *empty = getenv("EMPTY");
    if (!foo || strcmp(foo, "bar") != 0 || !empty || *empty)
        return 52;
    if (getauxval(AT_PAGESZ) != 4096 || !getauxval(AT_RANDOM))
        return 53;
    int keep = atoi(argv[3]), cl = atoi(argv[4]);
    errno = 0;
    if (close(cl) != -1 || errno != EBADF) /* O_CLOEXEC must have closed it */
        return 54;
    int p0 = atoi(argv[5]), p1 = atoi(argv[6]);
    if (p0 >= 0) { /* pipe2(O_CLOEXEC) descriptors must be gone as well */
        errno = 0;
        if (close(p0) != -1 || errno != EBADF)
            return 58;
        errno = 0;
        if (close(p1) != -1 || errno != EBADF)
            return 58;
    }
    if (g_data != 1) /* a fresh image: .data is back to its initial value */
        return 56;
    if (write(keep, "    [exec ok]\n", 14) != 14)
        return 57;
    return 55;
}

static int t_execve(void)
{
    g_data = 77;
    int keep = open(CONSOLE, O_WRONLY);
    int cl = open(CONSOLE, O_WRONLY | O_CLOEXEC);
    EXPECT(keep >= 0 && cl >= 0);

    int pp[2] = { -1, -1 };
    if (pipe2(pp, O_CLOEXEC) != 0)
        pp[0] = pp[1] = -1; /* pipe2 missing: the child skips this check */
    char a_keep[16], a_cl[16], a_p0[16], a_p1[16];
    snprintf(a_keep, sizeof a_keep, "%d", keep);
    snprintf(a_cl, sizeof a_cl, "%d", cl);
    snprintf(a_p0, sizeof a_p0, "%d", pp[0]);
    snprintf(a_p1, sizeof a_p1, "%d", pp[1]);
    char *args[] = { self_path, "--exec-child", "hello", a_keep, a_cl, a_p0, a_p1, NULL };
    char *envs[] = { "FOO=bar", "EMPTY=", NULL };

    pid_t p = fork();
    EXPECT(p >= 0);
    if (p == 0) {
        execve(self_path, args, envs);
        _exit(120);
    }
    int code = reap(p);
    close(keep);
    close(cl);
    if (code != 55)
        FAILF("exec'd child exited with %d (50 argc, 51 argv, 52 env, 53 auxv, "
              "54 cloexec, 56 .data not fresh, 57 kept fd, 58 pipe2 cloexec, 120 execve failed)", code);
    return T_PASS;
}

static int t_exec_fail(void)
{
    EXPECT(make_file(FP("t_text"), "just text, not an ELF\n", 0755) == 0);
    EXPECT(make_file(FP("t_stub"), "\x7f" "ELF", 0755) == 0);

    pid_t p = fork();
    EXPECT(p >= 0);
    if (p == 0) { /* every failed execve must RETURN and leave us alive */
        char *argv0[] = { "x", NULL };
        char *env0[] = { NULL };
        errno = 0;
        if (execve("/definitely/missing", argv0, env0) != -1 || errno != ENOENT) _exit(1);
        errno = 0;
        if (execve("/", argv0, env0) != -1 || errno != EACCES) _exit(2);
        errno = 0;
        if (execve(FP("t_text"), argv0, env0) != -1 || errno != ENOEXEC) _exit(3);
        errno = 0;
        if (execve(FP("t_stub"), argv0, env0) != -1 || errno != ENOEXEC) _exit(4);
        errno = 0;
        if (execve((char *)1, argv0, env0) != -1 || errno != EFAULT) _exit(5);
        _exit(0);
    }
    int code = reap(p);
    if (code != 0)
        FAILF("failed-exec check #%d (1 missing, 2 directory, 3 text, 4 truncated ELF, 5 bad pointer)", code);
    return T_PASS;
}

/* -------------------------------------- 6. CPU state across context switches */

static uint64_t mix(uint64_t seed, long n)
{
    uint64_t a = seed, b = seed ^ 0x9E3779B97F4A7C15ull, c = ~seed, d = seed * 3 + 1;
    for (long i = 0; i < n; i++) {
        a += b ^ (c >> 3);
        b = (b << 7) | (b >> 57);
        b += d;
        c ^= a * 0x2545F4914F6CDD1Dull;
        d += (uint64_t)i;
        __asm__ volatile("" : "+r"(a), "+r"(b), "+r"(c), "+r"(d));
    }
    return a ^ b ^ c ^ d;
}

static int fp_ok(int k, long n)
{
    double acc = 0.0;
    for (long i = 0; i < n; i++) {
        acc += (double)i * 0.5 + (double)k; /* every partial sum is exact */
        __asm__ volatile("" : "+x"(acc));
    }
    double want = 0.25 * (double)n * (double)(n - 1) + (double)k * (double)n;
    return acc == want;
}

#define KIDS   6
#define ITERS  20000000L

/* long computations in several processes at once: with preemption the CPU
 * switches between them, and every register (integer and SSE) must survive */
static int t_cpu_state(void)
{
    pid_t kids[KIDS];
    for (int k = 0; k < KIDS; k++) {
        kids[k] = fork();
        if (kids[k] == 0) {
            if (!fp_ok(k, ITERS))
                _exit(200);
            _exit((int)(mix((uint64_t)k + 1, ITERS) & 0x7f));
        }
        EXPECT(kids[k] > 0);
    }
    for (int k = 0; k < KIDS; k++) {
        int want = (int)(mix((uint64_t)k + 1, ITERS) & 0x7f); /* the parent works too */
        int got = reap(kids[k]);
        if (got == 200)
            FAILF("child %d: SSE state corrupted (is it saved per thread at context switch?)", k);
        if (got != want)
            FAILF("child %d: integer result 0x%x, want 0x%x", k, got, want);
    }
    return T_PASS;
}

/* ------------------------------------------------------------------ main */

struct test {
    const char *name;
    int (*fn)(void);
};

static const struct test tests[] = {
    { "stack_and_auxv",    t_stack_auxv },
    { "bss_is_zero",       t_bss_zero },
    { "syscall_inventory", t_inventory },
    { "open_errors",       t_open_errors },
    { "file_roundtrip",    t_file_roundtrip },
    { "trunc_and_append",  t_trunc_append },
    { "console_write",     t_console },
    { "ioctl_notty",       t_ioctl },
    { "fd_table",          t_fd_table },
    { "bad_pointers",      t_bad_pointers },
    { "read_no_consume",   t_read_no_consume },
    { "fork_basic",        t_fork_basic },
    { "wait_errors",       t_wait_errors },
    { "wait_wnohang",      t_wnohang },
    { "fork_isolation",    t_fork_isolation },
    { "fork_fd_sharing",   t_fork_fds },
    { "orphan_adoption",   t_orphans },
    { "pipe_basic",        t_pipe_basic },
    { "pipe_errors",       t_pipe_errors },
    { "pipe_big_transfer", t_pipe_transfer },
    { "pipe_write_atomic", t_pipe_atomic },
    { "pipe_exit_closes",  t_pipe_exit },
    { "pipe2_flags",       t_pipe2_flags },
    { "execve",            t_execve },
    { "execve_failures",   t_exec_fail },
    { "cpu_state_preempt", t_cpu_state },
};

int main(int argc, char **argv)
{
    self_path = argv[0];
    g_argc = argc;
    g_argv = argv;

    if (argc >= 2 && strcmp(argv[1], "--exec-child") == 0)
        return exec_child(argc, argv);

#ifndef HOST_TEST
    /* the kernel does not open stdio for init yet */
    if (write(1, "", 0) < 0 && errno == EBADF) {
        open(CONSOLE, O_RDONLY);
        open(CONSOLE, O_WRONLY);
        open(CONSOLE, O_WRONLY);
    }
#endif

    int once = argc >= 2 && strcmp(argv[1], "--once") == 0;
    int pass = 0, fail = 0, skip = 0;
    size_t n = sizeof tests / sizeof tests[0];
    say("\n=== userspace test suite: %zu tests ===\n", n);

    for (size_t i = 0; i < n; i++) {
        say("[RUN ] %s\n", tests[i].name);
        errno = 0;
        int r = tests[i].fn();
        if (r == T_PASS) {
            say("[PASS] %s\n", tests[i].name);
            pass++;
        } else if (r == T_SKIP) {
            say("[SKIP] %s\n", tests[i].name);
            skip++;
        } else {
            say("[FAIL] %s\n", tests[i].name);
            fail++;
        }
    }

    say("=== %d passed, %d failed, %d skipped ===\n", pass, fail, skip);
    if (!fail)
        say("ALL TESTS PASSED\n");

#ifdef HOST_TEST
    (void)once;
    return fail ? 1 : 0;
#else
    if (once) /* started by init.c: report through the exit code */
        return fail ? 1 : 0;
    for (;;) /* running as pid 1 itself: never exit */
        __builtin_ia32_pause();
#endif
}