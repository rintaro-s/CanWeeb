#!/bin/sh
# canweeb-init.sh
# First-boot initialization: reads /boot/canweeb-role to configure node role.
#
# To set role, write one of the following to /boot/canweeb-role on the SD card:
#   parent
#   child
#
# Optionally write a node ID to /boot/canweeb-node-id
# Optionally write a peer network address to /boot/canweeb-peer-addr
#
# If /etc/canweeb/.initialized exists, this script exits immediately.

INIT_FLAG="/etc/canweeb/.initialized"
ROLE_FILE="/boot/canweeb-role"
NODE_ID_FILE="/boot/canweeb-node-id"
PEER_ADDR_FILE="/boot/canweeb-peer-addr"
CONFIG_DIR="/etc/canweeb"
CONFIG_FILE="${CONFIG_DIR}/config.toml"
TEMPLATE_PARENT="${CONFIG_DIR}/config-parent.toml"
TEMPLATE_CHILD="${CONFIG_DIR}/config-child.toml"

if [ -f "${INIT_FLAG}" ]; then
    echo "canweeb-init: already initialized, skipping."
    exit 0
fi

# Determine role
ROLE="parent"
if [ -f "${ROLE_FILE}" ]; then
    ROLE=$(cat "${ROLE_FILE}" | tr -d '[:space:]' | tr '[:upper:]' '[:lower:]')
fi

# Determine node ID
if [ -f "${NODE_ID_FILE}" ]; then
    NODE_ID=$(cat "${NODE_ID_FILE}" | tr -d '[:space:]')
else
    SERIAL=$(cat /proc/cpuinfo | grep Serial | awk '{print $3}' | tail -c 9)
    NODE_ID="canweeb-${ROLE}-${SERIAL}"
fi

# Determine peer address
PEER_ADDR=""
if [ -f "${PEER_ADDR_FILE}" ]; then
    PEER_ADDR=$(cat "${PEER_ADDR_FILE}" | tr -d '[:space:]')
fi

echo "canweeb-init: role=${ROLE} node_id=${NODE_ID}"

# Copy appropriate template config
if [ "${ROLE}" = "parent" ]; then
    cp "${TEMPLATE_PARENT}" "${CONFIG_FILE}"
else
    cp "${TEMPLATE_CHILD}" "${CONFIG_FILE}"
fi

# Patch node_id in config
sed -i "s/^node_id = .*/node_id = \"${NODE_ID}\"/" "${CONFIG_FILE}"

# Patch peer network_addr if provided
if [ -n "${PEER_ADDR}" ]; then
    sed -i "s/^network_addr = .*/network_addr = \"${PEER_ADDR}\"/" "${CONFIG_FILE}"
fi

# Set hostname
echo "${NODE_ID}" > /etc/hostname
hostname "${NODE_ID}"

# Mark as initialized
touch "${INIT_FLAG}"

echo "canweeb-init: done. role=${ROLE} node_id=${NODE_ID}"
