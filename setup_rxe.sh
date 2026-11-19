#!/bin/bash
# Creates the software RDMA device "rxe0" (SoftRoCE). Needs sudo.
# It does not survive a reboot: run it again after restarting the computer.
set -e

# Network card to attach to: first argument, or the one used for internet.
NETDEV="${1:-$(ip route show default | awk '{print $5; exit}')}"
echo "Using network interface: $NETDEV"

# 1. Load the SoftRoCE kernel driver.
sudo modprobe rdma_rxe

# 2. Create rxe0 on top of that network card (skip if it already exists).
if rdma link show | grep -q rxe0; then
    echo "rxe0 already exists"
else
    sudo rdma link add rxe0 type rxe netdev "$NETDEV"
fi

# 3. Show the result. You should see rxe0 with state ACTIVE.
rdma link show
ibv_devices
