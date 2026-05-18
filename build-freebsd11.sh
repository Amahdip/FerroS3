#!/bin/bash
set -e

echo "Building Docker image for FreeBSD 11.2 cross-compilation..."
docker build -t ferros3-freebsd11-builder -f Dockerfile.freebsd11 .

echo "Compiling the project inside Docker..."
docker run --rm -v "$(pwd):/app" ferros3-freebsd11-builder \
    bash -c "rm -f /app/Cargo.lock && cargo build --release --target x86_64-unknown-freebsd -Z build-std"

echo "Build successful! The binary is located at:"
ls -lh target/x86_64-unknown-freebsd/release/ferros3
