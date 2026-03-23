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
KEYCHAIN_PASSWORD="phaselith-local-dev"
P12_PASSWORD="phaselith-local-p12"
CERT_PEM="${DEV_DIR}/phaselith-local-dev-cert.pem"
KEY_PEM="${DEV_DIR}/phaselith-local-dev-key.pem"
P12_PATH="${DEV_DIR}/phaselith-local-dev.p12"
OPENSSL_CNF="${DEV_DIR}/openssl.cnf"

mkdir -p "${DEV_DIR}"

HAS_IDENTITY=0
if security find-identity -p codesigning -v "${KEYCHAIN_PATH}" 2>/dev/null | grep -Fq "${CERT_NAME}"; then
  HAS_IDENTITY=1
fi

if [[ "${HAS_IDENTITY}" -eq 0 ]]; then
  security delete-keychain "${KEYCHAIN_PATH}" >/dev/null 2>&1 || true
fi

if [[ ! -f "${KEYCHAIN_PATH}" ]]; then
  security create-keychain -p "${KEYCHAIN_PASSWORD}" "${KEYCHAIN_PATH}"
fi

if [[ ! -f "${CERT_PEM}" || ! -f "${KEY_PEM}" || ! -f "${P12_PATH}" || "${HAS_IDENTITY}" -eq 0 ]]; then
cat >"${OPENSSL_CNF}" <<EOF
[req]
distinguished_name = dn
x509_extensions = v3_codesign
prompt = no

[dn]
CN = ${CERT_NAME}
O = Phaselith Local Dev
OU = Local Driver Signing

[v3_codesign]
basicConstraints = critical,CA:false
keyUsage = critical,digitalSignature
extendedKeyUsage = critical,codeSigning
subjectKeyIdentifier = hash
authorityKeyIdentifier = keyid,issuer
EOF

openssl req -x509 -newkey rsa:2048 -sha256 -days 3650 -nodes \
  -config "${OPENSSL_CNF}" \
  -keyout "${KEY_PEM}" \
  -out "${CERT_PEM}"

openssl pkcs12 -export \
  -legacy \
  -inkey "${KEY_PEM}" \
  -in "${CERT_PEM}" \
  -out "${P12_PATH}" \
  -passout "pass:${P12_PASSWORD}"
fi

security unlock-keychain -p "${KEYCHAIN_PASSWORD}" "${KEYCHAIN_PATH}"
security set-keychain-settings -lut 21600 "${KEYCHAIN_PATH}"

if ! security find-identity -p codesigning -v "${KEYCHAIN_PATH}" | grep -Fq "${CERT_NAME}"; then
  security import "${P12_PATH}" \
    -k "${KEYCHAIN_PATH}" \
    -P "${P12_PASSWORD}" \
    -T /usr/bin/codesign \
    -T /usr/bin/security
fi

security set-key-partition-list \
  -S apple-tool:,apple: \
  -s \
  -k "${KEYCHAIN_PASSWORD}" \
  "${KEYCHAIN_PATH}" >/dev/null

if ! security find-certificate -a -c "${CERT_NAME}" "${LOGIN_KEYCHAIN_PATH}" >/dev/null 2>&1; then
  security import "${P12_PATH}" \
    -k "${LOGIN_KEYCHAIN_PATH}" \
    -P "${P12_PASSWORD}" \
    -T /usr/bin/codesign \
    -T /usr/bin/security
fi

TRUST_SCRIPT="$(mktemp -t phaselith-trust-cert)"
cat >"${TRUST_SCRIPT}" <<EOF
#!/bin/sh
set -eu
/usr/bin/security delete-certificate -c "${CERT_NAME}" /Library/Keychains/System.keychain >/dev/null 2>&1 || true
/usr/bin/security add-trusted-cert -d -r trustAsRoot -k /Library/Keychains/System.keychain "${CERT_PEM}"
EOF
chmod 755 "${TRUST_SCRIPT}"
/usr/bin/osascript -e "do shell script \"/bin/sh ${TRUST_SCRIPT}\" with administrator privileges"
rm -f "${TRUST_SCRIPT}"

echo "Local Phaselith code-signing certificate is ready."
echo "Keychain: ${KEYCHAIN_PATH}"
echo "Trusted root installed in: /Library/Keychains/System.keychain"
