#include <sys/types.h>
#include <fcntl.h>
#include <unistd.h>
#include <pthread_np.h>

ssize_t getrandom(void *buf, size_t buflen, unsigned int flags) {
    int fd = open("/dev/urandom", O_RDONLY | O_CLOEXEC);
    if (fd < 0) return -1;
    ssize_t ret = read(fd, buf, buflen);
    close(fd);
    return ret;
}

void pthread_setname_np(pthread_t thread, const char *name) {
    pthread_set_name_np(thread, name);
}
