#!/usr/bin/env bash
set -e

readonly MAGENTA="$(tput setaf 5 2>/dev/null || echo '')"
readonly GREEN="$(tput setaf 2 2>/dev/null || echo '')"
readonly CYAN="$(tput setaf 6 2>/dev/null || echo '')"
readonly ORANGE="$(tput setaf 3 2>/dev/null || echo '')"
readonly NO_COLOR="$(tput sgr0 2>/dev/null || echo '')"

if [[ $# -lt 1 ]]; then
  echo "${ORANGE}Usage: $0 <kernel-version>{.y|.Z}${NO_COLOR}"
  echo "Example: ./build.sh 6.1 | 6.1.12 | 6.1.y | v6.1.12"
  exit 1
fi

INPUT="$1"
NUM="${INPUT#v}"  # normalize by stripping optional leading 'v'

# Validate: X.Y, X.Y.Z, or X.Y.y
if [[ ! "$NUM" =~ ^[0-9]+\.[0-9]+(\.(y|[0-9]+))?$ ]]; then
  echo "${ORANGE}Error: Invalid kernel version '${INPUT}'. Expected X.Y, X.Y.Z, or X.Y.y${NO_COLOR}"
  echo "Examples: 6.1 | 6.1.12 | 6.1.y | v6.1.12"
  exit 1
fi

echo "Building vmlinux for Linux kernel ${CYAN}${NUM}${NO_COLOR}"

type apt-get >/dev/null 2>&1 && sudo apt-get install -y git build-essential flex bison libncurses5-dev libssl-dev gcc bc libelf-dev pahole || true

REPO_URL="git://git.kernel.org/pub/scm/linux/kernel/git/stable/linux-stable.git"

# Decide ref: maintenance branch vs tag
if [[ "$NUM" == *".y" ]]; then
  REF="linux-${NUM}"         # e.g. linux-6.16.y
  VERSION="${NUM%.y}"        # e.g. 6.16
else
  REF="v${NUM}"              # e.g. v6.16.2 (ensure leading v)
  VERSION="${NUM}"           # e.g. 6.16.2 (no leading v)
fi

if [[ ! -d linux-stable ]]; then
  # Clone directly at the desired ref (branch or tag)
  git clone --depth=1 --branch "$REF" "$REPO_URL" linux-stable
else
  # Update existing checkout to the desired ref
  git -C linux-stable fetch --tags --force origin
  # Shallow-fetch the specific ref (works for both branches and tags)
  git -C linux-stable fetch --depth=1 origin "$REF":"$REF" || git -C linux-stable fetch origin "$REF":"$REF"
  git -C linux-stable checkout -f "$REF"
fi

cp .config linux-stable/.config

cd linux-stable

# Build
yes '' | make vmlinux -j"$(nproc)" < /dev/null

VMLINUX="vmlinux-${VERSION}"
mv vmlinux "${VMLINUX}.$(uname -m)"

echo "${GREEN}vmlinux built successfully!${NO_COLOR}"
echo "You can find the vmlinux file in ${CYAN}$(pwd)/${VMLINUX}.$(uname -m)${NO_COLOR}"

exit 0
