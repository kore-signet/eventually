FROM rust:1.54-buster as builder

WORKDIR /usr/src/rustventually
COPY . .

RUN cargo build --release

FROM debian:buster-slim

RUN apt-get update && \
    apt-get dist-upgrade -y && \
    apt-get install wget -y

COPY --from=builder /usr/src/rustventually/target/release/server .
COPY --from=builder /usr/src/rustventually/target/release/monitor .
COPY --from=builder /usr/src/rustventually/target/release/fill .
COPY schema.yaml .
USER 1000
CMD ["./server"]
