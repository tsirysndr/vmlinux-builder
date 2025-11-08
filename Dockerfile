FROM denoland/deno:latest

RUN apt-get update

RUN apt-get install -y \
  git \
  build-essential \
  flex \
  bison \
  libncurses5-dev \
  libssl-dev \
  gcc \
  bc \
  libelf-dev \
  pahole

WORKDIR /app

COPY deno.json deno.json

COPY deno.lock deno.lock

RUN deno install

COPY . .

RUN deno compile -A -o vmlinux-builder ./build.ts

RUN mv vmlinux-builder /usr/local/bin/vmlinux-builder

ENTRYPOINT ["vmlinux-builder"]