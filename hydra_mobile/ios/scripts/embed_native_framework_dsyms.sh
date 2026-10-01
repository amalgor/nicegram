#!/bin/sh
# Copy or generate dSYMs for Flutter native-asset frameworks (e.g. objective_c.framework).
# Runs as an Xcode build phase on Runner after frameworks are embedded.

set -e

# Flutter caches native-asset frameworks in build/native_assets/ios without
# keying on the SDK, so a device build after a simulator build can embed the
# simulator objective_c.framework (App Store Connect errors 90087 / 91169, and
# the app would not load it on a phone). Fail the build instead.
FRAMEWORKS_CHECK_DIR="${TARGET_BUILD_DIR}/${FRAMEWORKS_FOLDER_PATH}"
if [ "${PLATFORM_NAME}" = "iphoneos" ] && [ -d "${FRAMEWORKS_CHECK_DIR}" ]; then
  for binary in "${FRAMEWORKS_CHECK_DIR}"/*.framework/*; do
    [ -f "${binary}" ] && file -b "${binary}" | grep -q "Mach-O" || continue
    if xcrun vtool -show-build "${binary}" 2>/dev/null | grep -q "platform IOSSIMULATOR" \
      || lipo -archs "${binary}" 2>/dev/null | grep -q "x86_64"; then
      echo "error: ${binary#${FRAMEWORKS_CHECK_DIR}/} is a simulator build ($(lipo -archs "${binary}")) inside a device build."
      echo "error: Stale native assets. Run: rm -rf build/native_assets .dart_tool/flutter_build (or tool/build_ipa.sh) and rebuild."
      exit 1
    fi
  done
fi

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
