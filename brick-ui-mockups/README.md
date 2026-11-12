# UI previews

The PNGs in `actual/` are rendered by the test-only preview generator in
`apps/brick-controller/src/ui/preview.rs`.

To render with the palette currently active on a handheld, capture the
`nextval.elf` JSON and point the preview test at it:

```sh
adb shell 'export SYSTEM_PATH=/mnt/SDCARD/.system/tg5040 \
  SHARED_USERDATA_PATH=/mnt/SDCARD/.userdata/shared \
  SDCARD_PATH=/mnt/SDCARD \
  LD_LIBRARY_PATH=/usr/trimui/lib:/mnt/SDCARD/.system/lib; \
  /mnt/SDCARD/.system/tg5040/bin/nextval.elf' > /tmp/theme.json

BRICK_CONTROLLER_PREVIEW=/tmp/preview \
BRICK_CONTROLLER_PREVIEW_THEME=/tmp/theme.json \
cargo test --workspace preview_every_screen
```

Without `BRICK_CONTROLLER_PREVIEW_THEME`, the test uses the built-in fallback
palette. The PNGs are written at 1024×768, the Brick's native resolution.
