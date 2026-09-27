#!/usr/bin/env bash
# Build the same firmware bundle locally and in GitHub Actions.
set -euo pipefail

cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.."

# espup's export file supplies the Xtensa compiler and libclang paths locally.
# The GitHub Action exports these variables directly.
if [[ -f "$HOME/export-esp.sh" ]]; then
  source "$HOME/export-esp.sh"
fi

cargo build --release --locked --target xtensa-esp32s3-espidf

build_dir=target/xtensa-esp32s3-espidf/release
mkdir -p dist

# Application image only: accepted by the firmware's /api/ota endpoint.
espflash save-image --chip esp32s3 --flash-size 16mb --skip-update-check \
  "$build_dir/led-controller" dist/led-controller-ota.bin

# Use the project's ESP-IDF bootloader so rollback settings match the app.
espflash save-image --chip esp32s3 --flash-size 16mb --skip-update-check \
  --merge --skip-padding \
  --bootloader "$build_dir/bootloader.bin" \
  --partition-table partitions.csv \
  "$build_dir/led-controller" dist/led-controller-full.bin

cp "$build_dir/led-controller" dist/led-controller.elf
cp partitions.csv dist/partitions.csv
cp LICENSE dist/LICENSE
cat > dist/README.txt <<'USAGE'
led-controller / ESP32-S3-WROOM-1-N16R8 (16 MB flash)

led-controller-ota.bin
  Upload through Settings > Firmware update on an existing installation.
  This is an application-only image.

led-controller-full.bin
  Initial USB installation: write this merged image at flash address 0x0.
  It includes the bootloader, partition table, and application.
  espflash write-bin --chip esp32s3 0x0 led-controller-full.bin
  The merged image can overwrite existing settings. Do not upload it via OTA.

led-controller.elf
  ELF file for debugging and symbol lookup.

partitions.csv
  Partition layout used to generate the full image.

SHA256SUMS
  Verify on Linux with: sha256sum --check SHA256SUMS
USAGE

(
  cd dist
  sha256sum led-controller-ota.bin led-controller-full.bin \
    led-controller.elf partitions.csv LICENSE README.txt > SHA256SUMS
)
printf 'Firmware bundle written to dist/\n'
