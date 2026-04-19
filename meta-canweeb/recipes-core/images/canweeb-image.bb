SUMMARY = "CanWeeb robotics image for Raspberry Pi"
DESCRIPTION = "Minimal real-time Linux image with CanWeeb pre-installed. Supports both parent and child node roles."

require recipes-core/images/core-image-minimal.bb

IMAGE_FEATURES += " \
    ssh-server-openssh \
    package-management \
"

IMAGE_INSTALL:append = " \
    canweeb \
    canweeb-init \
    robotics-tweaks \
    python3 \
    python3-websocket-client \
    curl \
    iproute2 \
    iw \
    wireless-regdb \
    wpa-supplicant \
    usbutils \
    i2c-tools \
    util-linux \
    dtc \
    kernel-modules \
    openssh-sftp-server \
    sudo \
    bash \
    procps \
"

# Remove packages that add latency or are unnecessary for robotics
IMAGE_INSTALL:remove = " \
    packagegroup-core-x11 \
    x11vnc \
    avahi-daemon \
    cups \
    blueman \
"

# SD card image (wic) + compressed for distribution
IMAGE_FSTYPES = "wic wic.bz2 wic.bmap"

# Hostname: set per role at first boot via canweeb-init
hostname_pn-base-files = "canweeb-node"

# Larger rootfs for Rust binaries and data
IMAGE_ROOTFS_SIZE ?= "524288"
IMAGE_ROOTFS_EXTRA_SPACE = "524288"

COMPATIBLE_MACHINE = "raspberrypi4-64|raspberrypi3-64|raspberrypi3|raspberrypi4"
