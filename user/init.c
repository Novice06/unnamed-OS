#include <unistd.h>
#include <fcntl.h>
#include <stdio.h>

int main(int argc, char** argv, char** envp) 
{
    open("/dev/console", O_RDONLY); // stdin
    open("/dev/console", O_WRONLY); // stdout
    open("/dev/console", O_WRONLY); // stderr
    

    printf("\n=== ARGV ===\n");
    for (int i = 0; i < argc; i++) {
        printf("argv[%d] = %s\n", i, argv[i]);
    }

    printf("\n=== ENVP ===\n");
    for (int i = 0; envp[i] != NULL; i++) {
        printf("envp[%d] = %s\n", i, envp[i]);
    }

    printf("\nopening: /home/notfound.txt\n");
    fflush(stdout);

    int fd = open("/home/notfound.txt", O_RDWR);
    if (fd < 1) {
        perror("open failed");
    }

    for (;;) {
        __builtin_ia32_pause();
    }
    

    return 0;
}