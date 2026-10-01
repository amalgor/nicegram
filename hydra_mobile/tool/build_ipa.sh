#!/usr/bin/env bash
# Release IPA for TestFlight. Flutter installs native-asset frameworks into one
# shared build/native_assets/ios, but its up-to-date stamps (.dart_tool/
# flutter_build) are per configuration. After a simulator build the release
# stamp still looks fresh, so the simulator objective_c.framework ends up in
# the archive and App Store Connect rejects it (90087 / 91169). Clearing both
# forces a reinstall; the compiled hook outputs in .dart_tool/hooks_runner stay.
#
#   hydra_mobile/tool/build_ipa.sh
set -euo pipefail

cd "$(dirname "$0")/.."
rm -rf build/native_assets .dart_tool/flutter_build build/ios/archive build/ios/ipa
status=0
flutter build ipa --release "$@" || status=$?

APP=build/ios/archive/Runner.xcarchive/Products/Applications/Runner.app
[[ -d "$APP" ]] || { echo "No archive produced"; exit 1; }
[[ $status -eq 0 ]] || echo "IPA export failed (exit $status); checking the archive anyway. Upload it from Organizer: open build/ios/archive/Runner.xcarchive"
for binary in "$APP/Runner" "$APP"/Frameworks/*.framework/*; do
  [[ -f "$binary" ]] && file -b "$binary" | grep -q Mach-O || continue
  archs=$(lipo -archs "$binary")
  platforms=$(xcrun vtool -show-build "$binary" | awk '/platform/ {print $2}' | sort -u | tr '\n' ' ')
  printf '  %-45s %-8s %s\n' "${binary#"$APP"/}" "$archs" "$platforms"
  if [[ "$archs" != "arm64" || "$platforms" != "IOS " ]]; then
    echo "  ^ not a device-only arm64 binary"; status=1
  fi
done
nm -gU "$APP/Runner" | grep -q frb_get_rust_content_hash && echo "  Rust linked (frb_get_rust_content_hash)" || { echo "  Rust missing from Runner"; status=1; }
exit $status
