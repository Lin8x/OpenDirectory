#!/bin/sh
# Endpoint Agent post-install: enroll unattended when an enrollment file was
# staged before the package was installed (configuration management, golden
# images, kickstart/preseed). Otherwise print the next step.
#
# /etc/opendirectory/enroll.conf (mode 0600):
#   SERVER=https://dc01.example.com:8443
#   TOKEN=XXXX-XXXX-XXXX-XXXX
#   FINGERPRINT=<sha256 from Admin Center > Enrollment>
conf=/etc/opendirectory/enroll.conf

if [ -f "$conf" ]; then
  SERVER='' TOKEN='' FINGERPRINT=''
  . "$conf"
  if [ -n "$SERVER" ] && [ -n "$TOKEN" ]; then
    set -- enroll --server "$SERVER" --token "$TOKEN" --install
    [ -n "$FINGERPRINT" ] && set -- "$@" --fingerprint "$FINGERPRINT"
    if /usr/bin/opendirectory-agent "$@"; then
      # The token is single-use; don't leave it on disk.
      rm -f "$conf"
      echo "OpenDirectory Endpoint Agent enrolled with $SERVER and running."
    else
      echo "OpenDirectory Endpoint Agent: enrollment with $SERVER failed. Fix $conf and run: sudo opendirectory-agent enroll --server ... --token ... --install" >&2
    fi
    exit 0
  fi
fi

# Already enrolled (an upgrade): the service runs its own copy and updates
# itself from the Management Server, so there's nothing to do.
[ -f /var/lib/opendirectory-agent/agent.json ] && exit 0

echo "OpenDirectory Endpoint Agent installed. To enroll, run the command shown in Admin Center > Enrollment."
exit 0
