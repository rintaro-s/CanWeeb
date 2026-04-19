SUMMARY = "Robotics competition system tweaks for CanWeeb"
DESCRIPTION = "CPU, IRQ, memory, and systemd tuning for low-latency real-time operation on Raspberry Pi."
LICENSE = "MIT"
LIC_FILES_CHKSUM = "file://${COREBASE}/meta/files/common-licenses/MIT;md5=0835ade698e0bcf8506ecda2f7b4f302"

SRC_URI = " \
    file://99-robotics-rt.conf \
    file://cpufreq-performance.service \
    file://irq-affinity.service \
    file://irq-affinity.sh \
    file://robotics-sysctl.conf \
    file://robotics-limits.conf \
"

S = "${WORKDIR}"

inherit systemd

SYSTEMD_SERVICE:${PN} = " \
    cpufreq-performance.service \
    irq-affinity.service \
"
SYSTEMD_AUTO_ENABLE:${PN} = "enable"

do_install() {
    # systemd drop-in: tune journald and disable unnecessary targets
    install -d ${D}${sysconfdir}/systemd/system.conf.d
    install -m 0644 ${WORKDIR}/99-robotics-rt.conf ${D}${sysconfdir}/systemd/system.conf.d/

    # cpu frequency service
    install -d ${D}${systemd_system_unitdir}
    install -m 0644 ${WORKDIR}/cpufreq-performance.service ${D}${systemd_system_unitdir}/
    install -m 0644 ${WORKDIR}/irq-affinity.service ${D}${systemd_system_unitdir}/

    # IRQ affinity script
    install -d ${D}${sbindir}
    install -m 0755 ${WORKDIR}/irq-affinity.sh ${D}${sbindir}/irq-affinity.sh

    # sysctl tunables
    install -d ${D}${sysconfdir}/sysctl.d
    install -m 0644 ${WORKDIR}/robotics-sysctl.conf ${D}${sysconfdir}/sysctl.d/99-robotics.conf

    # ulimit / memlock for RT processes
    install -d ${D}${sysconfdir}/security/limits.d
    install -m 0644 ${WORKDIR}/robotics-limits.conf ${D}${sysconfdir}/security/limits.d/99-robotics.conf
}

FILES:${PN} = " \
    ${sysconfdir}/systemd/system.conf.d/99-robotics-rt.conf \
    ${systemd_system_unitdir}/cpufreq-performance.service \
    ${systemd_system_unitdir}/irq-affinity.service \
    ${sbindir}/irq-affinity.sh \
    ${sysconfdir}/sysctl.d/99-robotics.conf \
    ${sysconfdir}/security/limits.d/99-robotics.conf \
"
