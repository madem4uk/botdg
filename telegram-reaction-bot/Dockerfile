# Use Rust official image
FROM rust:1.82-slim as builder

# Install system dependencies for TDLib
RUN apt-get update && apt-get install -y \
    build-essential \
    cmake \
    git \
    zlib1g-dev \
    libssl-dev \
    gperf \
    php-cli \
    && rm -rf /var/lib/apt/lists/*

# Set working directory
WORKDIR /app

# Copy Cargo files first (for better caching)
COPY Cargo.toml ./

# Create a dummy main.rs to build dependencies
RUN mkdir src && \
    echo "fn main() {}" > src/main.rs && \
    cargo build --release && \
    rm -rf src

# Copy source code
COPY src ./src

# Build the application
RUN cargo build --release

# Runtime stage - use Ubuntu 22.04 for better GLIBC compatibility
FROM ubuntu:22.04

# Install runtime dependencies
RUN apt-get update && apt-get install -y \
    libssl3 \
    ca-certificates \
    && rm -rf /var/lib/apt/lists/*

# Create app user
RUN useradd -m -u 1000 app

# Set working directory
WORKDIR /app

# Copy binary from builder
COPY --from=builder /app/target/release/tdlib-test /app/telegram-reaction-bot

# Copy configuration example
COPY env.example /app/env.example

# Create data directories
RUN mkdir -p /app/tdlib_data /app/tdlib_files && \
    chown -R app:app /app

# Switch to app user
USER app

# Expose environment variables
ENV TDLIB_DATA_DIR=/app/tdlib_data
ENV TDLIB_FILES_DIR=/app/tdlib_files

# Run the application
CMD ["./telegram-reaction-bot"] 