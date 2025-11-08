FROM denoland/deno:latest AS builder

WORKDIR /app

# Copy only the files needed for installing dependencies
COPY deno.json deno.lock ./

RUN deno install

COPY . .

RUN deno compile -A -o vmlinux-builder ./build.ts

FROM ubuntu:latest

RUN apt-get update && apt-get install -y \
  git \
  build-essential \
  flex \
  bison \
  libncurses5-dev \
  libssl-dev \
  gcc \
  bc \
  libelf-dev \
  pahole \
  && rm -rf /var/lib/apt/lists/*

COPY --from=builder /app/vmlinux-builder /usr/local/bin/vmlinux-builder

ENTRYPOINT ["vmlinux-builder"]