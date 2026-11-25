/*
 * init.c : PID 1.
 * Opens the console on fds 0/1/2, runs /bin/test in a child (fork + execve),
 * reports its exit code, then stays alive reaping orphans: init never exits.
 *
 * Build:  $(MUSL_CC) $(USER_CFLAGS) init.c -o $(OUTPUT_BIN)/init
 *         $(MUSL_CC) $(USER_CFLAGS) test.c -o $(OUTPUT_BIN)/test   (both go in the initrd)
 */
#include <fcntl.h>
#include <stdio.h>
#include <string.h>
#include <sys/wait.h>
#include <unistd.h>

#define CONSOLE "/dev/console"

static void say(const char *s)
{
    ssize_t r = write(1, s, strlen(s));
    (void)r;
}

int main(void)
{
    open(CONSOLE, O_RDONLY); /* fd 0 */
    open(CONSOLE, O_WRONLY); /* fd 1 */
    open(CONSOLE, O_WRONLY); /* fd 2 */

    say("[init] pid 1 started\n");

    pid_t p = fork();
    if (p < 0) {
        say("[init] fork failed\n");
    } else if (p == 0) {
        char *argv[] = { "/bin/test", "--once", NULL };
        char *envp[] = { "OSNAME=unnamed", NULL };
        execve("/bin/test", argv, envp);
        say("[init] execve /bin/test failed\n");
        _exit(127);
    } else {
        int st = 0;
        pid_t r = waitpid(p, &st, 0);
        char msg[96];
        if (r == p && WIFEXITED(st))
            snprintf(msg, sizeof msg, "[init] /bin/test exited with code %d%s\n",
                     WEXITSTATUS(st), WEXITSTATUS(st) == 0 ? " (all good)" : " (FAILURES)");
        else
            snprintf(msg, sizeof msg, "[init] waitpid returned %d, status 0x%x\n", (int)r, st);
        say(msg);
    }

    for (;;) { /* never exit: reap any orphan that gets re-parented to us */
        int st;
        if (waitpid(-1, &st, 0) < 0)
            __builtin_ia32_pause(); /* no child at all: nothing to block on yet */
    }
}