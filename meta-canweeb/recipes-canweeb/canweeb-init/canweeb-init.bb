SUMMARY = "CanWeeb first-boot node role initialization"
DESCRIPTION = "Sets CanWeeb node role (parent/child) and hostname on first boot based on /boot/canweeb-role."
LICENSE = "MIT"
LIC_FILES_CHKSUM = "file://${COREBASE}/meta/files/common-licenses/MIT;md5=0835ade698e0bcf8506ecda2f7b4f302"

SRC_URI = " \
    file://canweeb-init.sh \
    file://canweeb-init.service \
"

S = "${WORKDIR}"

inherit systemd

SYSTEMD_SERVICE:${PN} = "canweeb-init.service"
SYSTEMD_AUTO_ENABLE:${PN} = "enable"

do_install() {
    install -d ${D}${sbindir}
    install -m 0755 ${WORKDIR}/canweeb-init.sh ${D}${sbindir}/canweeb-init.sh

    install -d ${D}${systemd_system_unitdir}
    install -m 0644 ${WORKDIR}/canweeb-init.service ${D}${systemd_system_unitdir}/canweeb-init.service
}

FILES:${PN} = " \
    ${sbindir}/canweeb-init.sh \
    ${systemd_system_unitdir}/canweeb-init.service \
"
