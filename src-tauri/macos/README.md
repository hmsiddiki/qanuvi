# macOS files

- `master-Info.plist`: the master's own helper app inside Qanuvi.app (`Contents/Helpers/Qanuvi Master.app`), placed by
  `bundle.macOS.files` in `tauri.conf.json`. macOS keeps it apart from the app, so the Dock, `open` and the tray's
  "Open Qanuvi" reach the window (m1).
- `macicon.swift`, `mac-icon-1024.png`: the Mac app icon. macOS 26 and later put every app icon that is not a full
  rounded square on a light plate, so the Mac icon is the logo on a dark rounded square (Apple's 1024 grid: 824 px
  square, 100 px margin). `../icons/icon.icns` is made from it; Windows and Linux keep the transparent logo, and the
  menu-bar (tray) icon stays transparent.

After a new logo (`npx tauri icon` rewrites `icons/icon.icns`), make the Mac icon again on a Mac:

    swiftc -O macicon.swift -o /tmp/macicon && /tmp/macicon ../app-icon.png mac-icon-1024.png
    mkdir -p /tmp/icon.iconset && for s in 16 32 128 256 512; do
      sips -z $s $s mac-icon-1024.png --out /tmp/icon.iconset/icon_${s}x${s}.png
      sips -z $((s*2)) $((s*2)) mac-icon-1024.png --out /tmp/icon.iconset/icon_${s}x${s}@2x.png
    done && iconutil -c icns /tmp/icon.iconset -o ../icons/icon.icns
