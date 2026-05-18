// FreeBSD 11.2 compatibility shim.
//
// pthread_setname_np: Added in FreeBSD 12.2 (POSIX name).
// FreeBSD 11 only has pthread_set_name_np (without "thread" in the middle).
// We provide this wrapper so the Rust binary can link against it when built
// for legacy FreeBSD 11.x environments.

#include <pthread_np.h>

void pthread_setname_np(pthread_t thread, const char *name) {
    pthread_set_name_np(thread, name);
}
