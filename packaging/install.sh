#!/bin/sh
set -eu

root=$(cd "$(dirname "$0")/.." && pwd)
binaries=${BINARIES:-$root/target/release}
destination=${DESTDIR:-}
id=app.nekolab.asura

install -Dm755 "$binaries/asura" "$destination/usr/bin/asura"
install -Dm755 "$binaries/asura-run" "$destination/usr/bin/asura-run"
install -Dm755 "$binaries/asura-helper" "$destination/usr/lib/asura/asura-helper"

install -Dm644 "$root/packaging/$id.desktop" "$destination/usr/share/applications/$id.desktop"
install -Dm644 "$root/packaging/$id.metainfo.xml" "$destination/usr/share/metainfo/$id.metainfo.xml"
install -Dm644 "$root/packaging/$id.policy" "$destination/usr/share/polkit-1/actions/$id.policy"
install -Dm644 "$root/packaging/$id.mime.xml" "$destination/usr/share/mime/packages/$id.xml"
install -Dm644 "$root/packaging/icons/$id.svg" "$destination/usr/share/icons/hicolor/scalable/apps/$id.svg"
install -Dm644 "$root/packaging/icons/$id-symbolic.svg" "$destination/usr/share/icons/hicolor/symbolic/apps/$id-symbolic.svg"
