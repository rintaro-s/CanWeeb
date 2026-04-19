SUMMARY = "CanWeeb mesh messaging runtime"
DESCRIPTION = "Node-to-node communication runtime for robotics and sensors, with built-in HTTP/WebSocket API."
LICENSE = "MIT"
LIC_FILES_CHKSUM = "file://${COREBASE}/meta/files/common-licenses/MIT;md5=0835ade698e0bcf8506ecda2f7b4f302"

SRC_URI = " \
    file://canweeb-src.tar.gz \
    file://canweeb.service \
    file://config-parent.toml \
    file://config-child.toml \
"

S = "${WORKDIR}/canweeb-src"

inherit cargo systemd

CARGO_SRC_DIR = ""

SYSTEMD_SERVICE:${PN} = "canweeb.service"
SYSTEMD_AUTO_ENABLE:${PN} = "enable"

do_install:append() {
    install -d ${D}${bindir}
    install -m 0755 ${S}/target/${TARGET_SYS}/release/canweeb ${D}${bindir}/canweeb

    install -d ${D}${sysconfdir}/canweeb
    install -m 0644 ${WORKDIR}/config-parent.toml ${D}${sysconfdir}/canweeb/config-parent.toml
    install -m 0644 ${WORKDIR}/config-child.toml  ${D}${sysconfdir}/canweeb/config-child.toml

    # canweeb-init.sh will write the active config.toml on first boot.
    # Install a safe default so the service can start even without canweeb-init.
    install -m 0644 ${WORKDIR}/config-parent.toml ${D}${sysconfdir}/canweeb/config.toml

    install -d ${D}${systemd_system_unitdir}
    install -m 0644 ${WORKDIR}/canweeb.service ${D}${systemd_system_unitdir}/canweeb.service

    install -d ${D}/var/lib/canweeb/data
}

FILES:${PN} += " \
    ${bindir}/canweeb \
    ${sysconfdir}/canweeb \
    ${systemd_system_unitdir}/canweeb.service \
    /var/lib/canweeb \
"

RDEPENDS:${PN} = ""
