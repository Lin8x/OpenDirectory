#!/bin/sh
# Endpoint Agent pre-remove: stop and remove the service on a full removal,
# but not on an upgrade. Enrollment state in /var/lib/opendirectory-agent is
# kept, so a reinstall picks up where it left off.
#   deb: $1 = remove | upgrade | deconfigure | failed-upgrade
#   rpm: $1 = 0 (erase) | 1 (upgrade)
#   archlinux: called on removal only
case "${1:-remove}" in
  upgrade | failed-upgrade | 1) exit 0 ;;
esac
/usr/bin/opendirectory-agent uninstall >/dev/null 2>&1 || true
exit 0
