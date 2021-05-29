FROM rust:1.52-slim AS build
WORKDIR /usr/src

RUN apt-get update && \
    apt-get dist-upgrade -y && \
    apt-get install -y musl-tools

RUN rustup target add x86_64-unknown-linux-musl

RUN USER=root cargo new rustventually
WORKDIR /usr/src/rustventually
COPY Cargo.toml Cargo.lock ./
COPY src ./src

RUN cargo install --target x86_64-unknown-linux-musl --path .

FROM debian:buster-slim
COPY --from=build /usr/local/cargo/bin/server .
COPY --from=build /usr/local/cargo/bin/fill .
COPY schema.yaml .
USER 1000
CMD ["./server"]
