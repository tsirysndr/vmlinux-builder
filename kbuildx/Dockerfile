FROM rust:1.97-alpine AS builder

COPY . /app

WORKDIR /app

RUN cargo build --release

FROM alpine:latest

COPY --from=builder /app/target/release/kbuildx /usr/local/bin/kbuildx

RUN apk update

RUN apk add --no-cache \
  git \
  build-base \
  flex \
  bison \
  ncurses-dev \
  openssl-dev \
  gcc \
  bc \
  elfutils-dev \
  pahole

ENTRYPOINT [ "/usr/local/bin/kbuildx" ]
