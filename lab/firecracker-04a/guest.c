#include <stdio.h>
#include <sys/socket.h>
#include <linux/vm_sockets.h>
#include <time.h>
#include <unistd.h>

int main(void)
{
    struct timespec boot;
    struct sockaddr_vm address = {
        .svm_family = AF_VSOCK,
        .svm_port = 4040,
        .svm_cid = VMADDR_CID_ANY,
    };
    int server = socket(AF_VSOCK, SOCK_STREAM, 0);
    if (server < 0 || bind(server, (struct sockaddr *)&address, sizeof(address)) ||
        listen(server, 4) || clock_gettime(CLOCK_REALTIME, &boot)) {
        perror("guest initialization");
        return 1;
    }

    dprintf(STDOUT_FILENO, "READY %lld.%09ld\n", (long long)boot.tv_sec, boot.tv_nsec);
    for (;;) {
        int client = accept(server, NULL, NULL);
        if (client < 0) {
            perror("accept");
            return 1;
        }
        FILE *stream = fdopen(client, "r+");
        if (!stream) {
            close(client);
            continue;
        }
        char receipt[128];
        if (fgets(receipt, sizeof(receipt), stream))
            dprintf(client, "PROOF %lld.%09ld %s", (long long)boot.tv_sec, boot.tv_nsec, receipt);
        fclose(stream);
    }
}
