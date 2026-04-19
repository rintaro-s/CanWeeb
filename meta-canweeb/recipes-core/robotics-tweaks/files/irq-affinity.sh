#!/bin/sh
# Assign all IRQs to CPU 0-1, leaving CPU 2-3 isolated for CanWeeb FIFO tasks.
# On a 4-core RPi4, cores 2 and 3 are reserved for real-time work.

HOUSEKEEPING_CPUMASK=3   # 0b0011 = CPU 0,1
RT_CPUMASK=12            # 0b1100 = CPU 2,3

# Move all IRQs to housekeeping CPUs
for irq_dir in /proc/irq/[0-9]*/; do
    irq=$(basename "$irq_dir")
    [ -f "${irq_dir}smp_affinity" ] && \
        printf '%x\n' "$HOUSEKEEPING_CPUMASK" > "${irq_dir}smp_affinity" 2>/dev/null
done

# Move kernel housekeeping threads to CPU 0-1
if [ -d /sys/bus/workqueue/devices/writeback ]; then
    echo "$HOUSEKEEPING_CPUMASK" > /sys/bus/workqueue/devices/writeback/cpumask 2>/dev/null
fi

# Set rcu threads to housekeeping CPUs
for pid in $(pgrep rcu); do
    taskset -p "$HOUSEKEEPING_CPUMASK" "$pid" 2>/dev/null
done

# Set kworker and ksoftirqd to housekeeping CPUs
for pid in $(pgrep kworker); do
    taskset -p "$HOUSEKEEPING_CPUMASK" "$pid" 2>/dev/null
done
for pid in $(pgrep ksoftirqd); do
    taskset -p "$HOUSEKEEPING_CPUMASK" "$pid" 2>/dev/null
done

echo "IRQ affinity configured: IRQs -> CPU 0-1, RT reserved -> CPU 2-3"
