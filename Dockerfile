# Dockerfile for BITCH - MAVLINK Interceptor
# Multi-stage build with cargo-chef for dependency caching
# Cross-compilation for ARM64 without QEMU

ARG TARGETARCH

# Stage 1: Planner - Generate recipe for dependencies
FROM rust:1.83-slim AS planner
WORKDIR /app
RUN cargo install cargo-chef --version 0.1.67
COPY . .
RUN cargo chef prepare --recipe-path recipe.json

# Stage 2: Cacher - Build dependencies only (cached layer)
FROM rust:1.83-slim AS cacher
WORKDIR /app
ARG TARGETARCH

RUN cargo install cargo-chef --version 0.1.67

# Install cross-compilation tools for ARM64
RUN if [ "$TARGETARCH" = "arm64" ]; then \
    dpkg --add-architecture arm64 && \
    apt-get update && \
    apt-get install -y gcc-aarch64-linux-gnu libc6-dev-arm64-cross && \
    rustup target add aarch64-unknown-linux-gnu && \
    rm -rf /var/lib/apt/lists/*; \
    fi

COPY --from=planner /app/recipe.json recipe.json

# Build dependencies based on target architecture
RUN if [ "$TARGETARCH" = "arm64" ]; then \
    CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_LINKER=aarch64-linux-gnu-gcc \
    cargo chef cook --release --target aarch64-unknown-linux-gnu --recipe-path recipe.json; \
    else \
    cargo chef cook --release --recipe-path recipe.json; \
    fi

# Stage 3: Builder - Build the actual application
FROM rust:1.83-slim AS builder
WORKDIR /app
ARG TARGETARCH

# Install cross-compilation tools for ARM64
RUN if [ "$TARGETARCH" = "arm64" ]; then \
    dpkg --add-architecture arm64 && \
    apt-get update && \
    apt-get install -y gcc-aarch64-linux-gnu libc6-dev-arm64-cross && \
    rustup target add aarch64-unknown-linux-gnu && \
    rm -rf /var/lib/apt/lists/*; \
    fi

# Copy cached dependencies from cacher stage
COPY --from=cacher /app/target target
COPY --from=cacher /usr/local/cargo /usr/local/cargo

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
