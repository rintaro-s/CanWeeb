SUMMARY = "WebSocket client library for Python"
HOMEPAGE = "https://github.com/websocket-client/websocket-client"
LICENSE = "Apache-2.0"
LIC_FILES_CHKSUM = "file://LICENSE;md5=86d3f3a95c324c9479bd8986968f4327"

SRC_URI = "https://files.pythonhosted.org/packages/source/w/websocket_client/websocket_client-${PV}.tar.gz"
SRC_URI[sha256sum] = "3239df9f44da632f96012472805d40a23281a991027ce11d2f45a6f24ac4c3da"

S = "${WORKDIR}/websocket_client-${PV}"

inherit setuptools3

RDEPENDS:${PN} = "python3-core python3-logging python3-io python3-threading"
