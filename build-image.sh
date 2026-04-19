#!/bin/bash
# build-image.sh
# CanWeeb Yocto image build script.
# Packages the current CanWeeb source, sets up the Yocto build environment,
# and builds the canweeb-image for Raspberry Pi.
#
# Usage:
#   ./build-image.sh [options]
#
# Options:
#   --machine   MACHINE   Target machine (default: raspberrypi4-64)
#   --role      ROLE      Node role: parent or child (default: parent)
#   --jobs      N         Number of parallel build jobs (default: nproc)
#   --clean               Clean tmp before build
#   --help                Show this help

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
CANWEEBROOT="${SCRIPT_DIR}"
BUILD_DIR="${CANWEEBROOT}/build"
DOWNLOADS_DIR="${CANWEEBROOT}/.yocto-downloads"
SSTATE_DIR="${CANWEEBROOT}/.yocto-sstate"
MACHINE="raspberrypi4-64"
ROLE="parent"
JOBS="$(nproc)"
CLEAN=0

# ---- argument parsing -------------------------------------------------------
while [[ $# -gt 0 ]]; do
    case "$1" in
        --machine)  MACHINE="$2";  shift 2 ;;
        --role)     ROLE="$2";     shift 2 ;;
        --jobs)     JOBS="$2";     shift 2 ;;
        --clean)    CLEAN=1;       shift   ;;
        --help)
            sed -n '2,20p' "$0" | sed 's/^# \?//'
            exit 0
            ;;
        *) echo "Unknown option: $1"; exit 1 ;;
    esac
done

echo "======================================================"
echo "  CanWeeb Image Builder"
echo "  Machine : ${MACHINE}"
echo "  Role    : ${ROLE}"
echo "  Jobs    : ${JOBS}"
echo "  Root    : ${CANWEEBROOT}"
echo "======================================================"

# ---- sanity checks ----------------------------------------------------------
if [ ! -f "${CANWEEBROOT}/poky/oe-init-build-env" ]; then
    echo "ERROR: poky not found at ${CANWEEBROOT}/poky"
    echo "       Clone poky alongside CanWeeb or run: git submodule update --init"
    exit 1
fi

if [ ! -d "${CANWEEBROOT}/meta-raspberrypi" ]; then
    echo "ERROR: meta-raspberrypi not found at ${CANWEEBROOT}/meta-raspberrypi"
    exit 1
fi

if [ ! -d "${CANWEEBROOT}/meta-canweeb" ]; then
    echo "ERROR: meta-canweeb not found at ${CANWEEBROOT}/meta-canweeb"
    exit 1
fi

# ---- package CanWeeb source into tarball for the recipe ---------------------
TARBALL_DIR="${CANWEEBROOT}/meta-canweeb/recipes-canweeb/canweeb/files"
TARBALL="${TARBALL_DIR}/canweeb-src.tar.gz"
mkdir -p "${TARBALL_DIR}"

echo ""
echo "[1/4] Packaging CanWeeb source..."
tar --exclude='.git' \
    --exclude='target' \
    --exclude='build' \
    --exclude='.yocto-*' \
    --exclude='*.log' \
    -czf "${TARBALL}" \
    -C "${CANWEEBROOT}" \
    --transform 's,^\.,canweeb-src,' \
    .
echo "      -> ${TARBALL}"

# ---- set up build directory -------------------------------------------------
echo ""
echo "[2/4] Setting up Yocto build environment..."

mkdir -p "${DOWNLOADS_DIR}"
mkdir -p "${SSTATE_DIR}"
mkdir -p "${BUILD_DIR}/conf"

# Generate bblayers.conf
cat > "${BUILD_DIR}/conf/bblayers.conf" <<EOF
BBLAYERS ?= " \\
    ${CANWEEBROOT}/poky/meta \\
    ${CANWEEBROOT}/poky/meta-poky \\
    ${CANWEEBROOT}/poky/meta-yocto-bsp \\
    ${CANWEEBROOT}/meta-raspberrypi \\
    ${CANWEEBROOT}/meta-canweeb \\
"
EOF

# Generate local.conf from sample, substituting runtime values
sed \
    -e "s|MACHINE ?= .*|MACHINE ?= \"${MACHINE}\"|" \
    -e "s|BB_NUMBER_THREADS ?= .*|BB_NUMBER_THREADS ?= \"${JOBS}\"|" \
    -e "s|PARALLEL_MAKE ?= .*|PARALLEL_MAKE ?= \"-j${JOBS}\"|" \
    "${CANWEEBROOT}/meta-canweeb/conf/local.conf.sample" \
    > "${BUILD_DIR}/conf/local.conf"

# Append DL_DIR and SSTATE_DIR overrides with resolved absolute paths
cat >> "${BUILD_DIR}/conf/local.conf" <<EOF

# Resolved by build-image.sh
DL_DIR = "${DOWNLOADS_DIR}"
SSTATE_DIR = "${SSTATE_DIR}"
TMPDIR = "${BUILD_DIR}/tmp"
EOF

# ---- clean if requested -----------------------------------------------------
if [ "${CLEAN}" -eq 1 ]; then
    echo ""
    echo "[*] --clean requested: removing ${BUILD_DIR}/tmp"
    rm -rf "${BUILD_DIR}/tmp"
fi

# ---- run bitbake ------------------------------------------------------------
echo ""
echo "[3/4] Running bitbake canweeb-image (this will take a long time on first run)..."

# Source the Yocto environment and run bitbake inside it.
# oe-init-build-env changes directory, so we cd back explicitly.
(
    source "${CANWEEBROOT}/poky/oe-init-build-env" "${BUILD_DIR}" > /dev/null
    bitbake canweeb-image
)

# ---- locate and report output -----------------------------------------------
echo ""
echo "[4/4] Build complete."

DEPLOY_DIR="${BUILD_DIR}/tmp/deploy/images/${MACHINE}"
if [ -d "${DEPLOY_DIR}" ]; then
    WIC=$(ls "${DEPLOY_DIR}"/*.wic.bz2 2>/dev/null | head -1 || true)
    if [ -n "${WIC}" ]; then
        echo ""
        echo "  SD card image (compressed):"
        echo "    ${WIC}"
        echo ""
        echo "  Flash to SD card:"
        echo "    bzcat ${WIC} | sudo dd of=/dev/sdX bs=4M conv=fsync status=progress"
        echo ""
        echo "  Or use bmaptool (faster):"
        BMAP="${WIC%.bz2}.bmap"
        echo "    sudo bmaptool copy ${WIC%.bz2} /dev/sdX"
        echo ""
    fi
    echo "  All deploy artifacts: ${DEPLOY_DIR}"
else
    echo "  WARNING: deploy directory not found. Check build logs in ${BUILD_DIR}/tmp/log"
fi

echo ""
echo "  After flashing, set node role by writing to the SD boot partition:"
echo "    echo parent > /boot/canweeb-role"
echo "    echo child  > /boot/canweeb-role"
echo ""
echo "  Optionally set node ID:"
echo "    echo my-node-name > /boot/canweeb-node-id"
echo ""
echo "  Optionally set peer address:"
echo "    echo 192.168.1.100:7002 > /boot/canweeb-peer-addr"
echo "======================================================"
