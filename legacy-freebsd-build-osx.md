# Building for Legacy FreeBSD (11.2)

This guide explains how to cross-compile **FerroS3** for older FreeBSD kernels, specifically **FreeBSD 11.2**, from a macOS or Linux host using Docker.

## Why this is necessary?
While Rust natively supports cross-compilation via `cross`, recent versions of the Rust compiler (and its pre-compiled standard library) target FreeBSD 12+ by default. If you try to run a standard `x86_64-unknown-freebsd` binary on FreeBSD 11.2, you will encounter linker errors such as:

- `/lib/libthr.so.3: version FBSD_1.6 required not found`
- `undefined symbol: getrandom` (Introduced in FreeBSD 12.0)
- `undefined symbol: pthread_setname_np` (Introduced in FreeBSD 12.2)

To overcome this, we need to build the Rust standard library from source using `-Z build-std` against a true **FreeBSD 11.2 sysroot**, and provide C shims for the missing POSIX and Kernel APIs.

## The Solution
We use a Docker-based approach to ensure a perfectly isolated build environment:
1. **FreeBSD 11.2 Sysroot**: We download the official `base.txz` from the FreeBSD 11.2 release archive to provide the exact `libc` and `libthr` headers and shared objects.
2. **C Shims**: We provide a custom `src/freebsd11_shim.c` that implements fallbacks for `getrandom` (using `/dev/urandom`) and maps `pthread_setname_np` to the older `pthread_set_name_np` function available in FreeBSD 11.
3. **Rust Nightly**: We use the Rust Nightly toolchain to enable the `-Z build-std` flag, which forces Cargo to rebuild the `std` and `core` libraries against our older FreeBSD sysroot rather than using the pre-compiled FreeBSD 12+ versions.

## Build Steps

### 1. Prerequisites
Ensure you have Docker installed and running on your host machine.

### 2. Run the Build Script
Simply run the provided shell script from the root of the repository:

```bash
./build-freebsd11.sh
```

### What the script does:
- Builds a Docker image (`ferros3-freebsd11-builder`) based on `rustlang/rust:nightly-slim`.
- Installs `clang`, `lld`, and `curl`.
- Downloads and extracts the FreeBSD 11.2 `base.txz` into `/freebsd-sysroot`.
- Injects our custom C shims during the Cargo build phase via `build.rs`.
- Compiles the final binary using `-Z build-std`.

### 3. Retrieve the Binary
Once the build completes successfully, you will find the compiled binary at:
`target/x86_64-unknown-freebsd/release/ferros3`

This binary is now fully ABI-compatible with FreeBSD 11.2 and can be seamlessly deployed to your legacy servers (like older TrueNAS/FreeNAS systems).
