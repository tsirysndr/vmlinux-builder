#!/usr/bin/env bash

set -e

readonly MAGENTA="$(tput setaf 5 2>/dev/null || echo '')"
readonly GREEN="$(tput setaf 2 2>/dev/null || echo '')"
readonly CYAN="$(tput setaf 6 2>/dev/null || echo '')"
readonly ORANGE="$(tput setaf 3 2>/dev/null || echo '')"
readonly NO_COLOR="$(tput sgr0 2>/dev/null || echo '')"

if [[ ! "$1" =~ ^[0-9]+\.[0-9]+(\.[0-9]+|\.y)?$ ]]; then
  echo "${ORANGE}Error: Invalid kernel version format '${1}'. Expected format is X.Y or X.Y.Z (where Z can be 'y' or a number).${NO_COLOR}"
  echo "Example: ./build.sh 6.1 or ./build.sh 6.1.12 or ./build.sh 6.1.y"
  exit 1
fi

echo "Building vmlinux for Linux kernel ${CYAN}${1}${NO_COLOR}"

type apt-get > /dev/null && sudo apt-get install -y git build-essential flex bison libncurses5-dev libssl-dev gcc bc libelf-dev pahole || true

[ -d linux-stable ] || git clone --depth=1 -b linux-${1} git://git.kernel.org/pub/scm/linux/kernel/git/stable/linux-stable.git

cp .config linux-stable/.config

cd linux-stable

yes ''  | make vmlinux -j$(nproc) < /dev/null

VERSION=${1%.y}
VMLINUX=$(echo vmlinux-${VERSION})

mv vmlinux ${VMLINUX}

echo "${GREEN}vmlinux built successfully!${NO_COLOR}"
echo "You can find the vmlinux file in ${CYAN}$(pwd)/${VMLINUX}${NO_COLOR}"
