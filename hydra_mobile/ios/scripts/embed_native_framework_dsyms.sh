#!/bin/sh
# Copy or generate dSYMs for Flutter native-asset frameworks (e.g. objective_c.framework).
# Runs as an Xcode build phase on Runner after frameworks are embedded.

set -e

if [ "${ACTION}" = "install" ] || [ "${CONFIGURATION}" = "Release" ] || [ "${CONFIGURATION}" = "Profile" ]; then
  :
else
  exit 0
fi

FRAMEWORKS_DIR="${TARGET_BUILD_DIR}/${FRAMEWORKS_FOLDER_PATH}"
DSYM_DIR="${DWARF_DSYM_FOLDER_PATH}"
NATIVE_ASSETS_DSYM_DIR="${PROJECT_DIR}/../build/native_assets/ios"

if [ ! -d "${FRAMEWORKS_DIR}" ] || [ -z "${DSYM_DIR}" ]; then
  exit 0
fi

mkdir -p "${DSYM_DIR}"

for fw in objective_c; do
  BINARY="${FRAMEWORKS_DIR}/${fw}.framework/${fw}"
  OUT="${DSYM_DIR}/${fw}.framework.dSYM"
  PREFAB="${NATIVE_ASSETS_DSYM_DIR}/${fw}.framework.dSYM"

  if [ -d "${OUT}" ]; then
    continue
  fi

  if [ -d "${PREFAB}" ]; then
    echo "Copying ${fw}.framework.dSYM from native_assets"
    rm -rf "${OUT}"
    cp -R "${PREFAB}" "${OUT}"
    continue
  fi

  if [ -f "${BINARY}" ]; then
    echo "Generating ${fw}.framework.dSYM with dsymutil"
    xcrun dsymutil "${BINARY}" -o "${OUT}" || true
  fi
done
