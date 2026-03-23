#!/usr/bin/env bash
set -euo pipefail

if [[ "$(uname -s)" != "Darwin" ]]; then
  echo "This script only supports macOS." >&2
  exit 1
fi

CERT_NAME="Phaselith Local Driver Dev"
DEV_DIR="${HOME}/.phaselith-dev-signing"
KEYCHAIN_PATH="${HOME}/Library/Keychains/PhaselithDev.keychain-db"
LOGIN_KEYCHAIN_PATH="${HOME}/Library/Keychains/login.keychain-db"

ADMIN_SCRIPT="$(mktemp -t phaselith-revert-signing)"
cat >"${ADMIN_SCRIPT}" <<EOF
#!/bin/sh
set -eu
/usr/bin/security delete-certificate -c "${CERT_NAME}" /Library/Keychains/System.keychain >/dev/null 2>&1 || true
rm -rf /Library/Audio/Plug-Ins/HAL/PhaselithAudio.driver
/bin/launchctl kickstart -k system/com.apple.audio.coreaudiod >/dev/null 2>&1 || /usr/bin/killall coreaudiod >/dev/null 2>&1 || true
EOF
chmod 755 "${ADMIN_SCRIPT}"
/usr/bin/osascript -e "do shell script \"/bin/sh ${ADMIN_SCRIPT}\" with administrator privileges"
rm -f "${ADMIN_SCRIPT}"

security delete-certificate -c "${CERT_NAME}" "${LOGIN_KEYCHAIN_PATH}" >/dev/null 2>&1 || true
security delete-keychain "${KEYCHAIN_PATH}" >/dev/null 2>&1 || true
rm -rf "${DEV_DIR}"

echo "Local Phaselith signing setup removed."
echo "PhaselithAudio.driver was removed and coreaudiod restarted."
