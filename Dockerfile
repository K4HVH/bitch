# Dockerfile for BITCH - MAVLINK Interceptor
# Multi-stage build for cross-compilation
# Cross-compilation for ARM64 without QEMU

ARG TARGETARCH

# Stage 1: Builder - Build the application
FROM rust:1.83-slim AS builder
WORKDIR /app
ARG TARGETARCH

# Install build dependencies
RUN apt-get update && \
    apt-get install -y pkg-config libssl-dev git && \
    rm -rf /var/lib/apt/lists/*

# Install cross-compilation tools for ARM64
RUN if [ "$TARGETARCH" = "arm64" ]; then \
    dpkg --add-architecture arm64 && \
    apt-get update && \
    apt-get install -y gcc-aarch64-linux-gnu libc6-dev-arm64-cross && \
    rustup target add aarch64-unknown-linux-gnu && \
    rm -rf /var/lib/apt/lists/*; \
    fi

# Copy source code
COPY . .

# Build for the target architecture
RUN if [ "$TARGETARCH" = "arm64" ]; then \
    CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_LINKER=aarch64-linux-gnu-gcc \
    cargo build --release --target aarch64-unknown-linux-gnu && \
    cp target/aarch64-unknown-linux-gnu/release/bitch /app/bitch; \
    else \
    cargo build --release && \
    cp target/release/bitch /app/bitch; \
    fi

# Stage 4: Runtime - Minimal final image
FROM debian:bookworm-slim AS runtime
WORKDIR /app

# Install runtime dependencies
RUN apt-get update && \
    apt-get install -y ca-certificates && \
    rm -rf /var/lib/apt/lists/*

# Copy the binary from builder
COPY --from=builder /app/bitch /usr/local/bin/bitch

# Copy default config (can be overridden with volume mount)
COPY config.toml /app/config.toml

# Create non-root user
RUN useradd -m -u 1000 bitch && \
    chown -R bitch:bitch /app

USER bitch

# Expose the GCS listening port
EXPOSE 14550/udp

# Run the application
CMD ["bitch"]
