# Build Stage
FROM rust:1.75-slim-bookworm AS builder

WORKDIR /app
COPY . .

# Build the application
RUN cargo build --release

# Runtime Stage
FROM debian:bookworm-slim

WORKDIR /app

# Copy the binary from the builder stage
COPY --from=builder /app/target/release/ferros3 /app/ferros3

# Create a default config file if needed or expect one to be mounted
# EXPOSE 8080

ENTRYPOINT ["/app/ferros3"]
