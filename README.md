<div align="center">

# led-controller

**Standalone LED strip firmware for the ESP32-S3, written in Rust.**

![Rust](https://img.shields.io/badge/Rust-2021-DEA584?logo=rust)
![Platform](https://img.shields.io/badge/ESP32--S3-ESP--IDF-E7352C)
[![License: Apache-2.0](https://img.shields.io/badge/License-Apache--2.0-blue)](LICENSE)

**English** · [简体中文](README.zh-CN.md) · [日本語](README.ja.md)

Control nine LED strips from your browser. No Home Assistant required.

</div>

---

## Overview

`led-controller` is firmware for a custom ESP32-S3-WROOM-1-N16R8 board. It serves its own web interface and HTTP API for configuring strips, choosing effects, and updating firmware.

- **Nine outputs** — independent LED count, color order, brightness, and effect settings; batch control through the API.
- **19 effects** — solid colors, gradients, rainbows, and animated effects, with adjustable speed and color transitions.
- **RGB and RGBW** — WS2812/SK6812 output with configurable byte order.
- **Wi-Fi provisioning** — a setup access point, automatic reconnection, and mDNS discovery.
- **On-device settings** — JSON configuration stored in NVS, plus firmware uploads through the web interface.
- **Board controls** — a BOOT button for power toggling and Wi-Fi reset, with input-voltage monitoring.

The built-in web interface currently uses Chinese labels.

## Getting started

### Toolchain

The Xtensa target requires Espressif's Rust toolchain. Run these commands with a standard Rust toolchain installed:

```bash
cargo +stable install espup espflash ldproxy
espup install
```

`espup` installs the `esp` toolchain and creates `~/export-esp.sh`. The ESP-IDF build integration downloads the configured SDK version, **v5.5.5**, into `.embuild/` on the first build.

### Build and flash

From the repository root, with the board connected over USB:

```bash
source ~/export-esp.sh
cargo run --release
```

The Cargo runner builds and flashes the firmware, then opens the serial monitor. [espflash.toml](espflash.toml) selects 16 MB flash and the [partition table](partitions.csv), which contains two 4 MiB OTA application slots.

### Connect

1. Power on the board. Without saved Wi-Fi credentials, it starts an access point named `LEDCTL-XXXX` with password `ledctl1234`.
2. Join that network and open [192.168.71.1](http://192.168.71.1).
3. Open **设置 → WiFi** (Settings → WiFi), enter your network credentials, and save. The board restarts automatically.
4. Rejoin your usual network and open [ledctl.local](http://ledctl.local). If mDNS is unavailable, use the address shown in your router's device list.

During reconnection, the Wi-Fi supervisor enables the setup access point after approximately 30 seconds without a connection and continues retrying in the background.

### Update over Wi-Fi

After building, export a firmware image:

```bash
espflash save-image --chip esp32s3 --flash-size 16mb \
  target/xtensa-esp32s3-espidf/release/led-controller led-controller.bin
```

Upload `led-controller.bin` under **设置 → 固件升级** (Settings → Firmware update). The board restarts when the update completes.

## Automated builds

[GitHub Actions](.github/workflows/firmware.yml) builds the firmware on every push and pull request. You can also start **Build firmware** manually from the **Actions** tab once the workflow is on the default branch.

After a successful run, download `led-controller-esp32s3-<commit>` from **Artifacts** on the run page and extract the archive. Downloads require signing in to GitHub; artifacts are retained for 30 days.

| File | Use |
| --- | --- |
| `led-controller-ota.bin` | Upload through the board's firmware-update page. |
| `led-controller-full.bin` | Initial USB installation: merged bootloader, partition table, and application. Write at `0x0`. |
| `led-controller.elf` | Debugging and symbol lookup. |
| `SHA256SUMS` | Check file integrity with `sha256sum --check SHA256SUMS`. |

To flash the full image over USB:

```bash
espflash write-bin --chip esp32s3 0x0 led-controller-full.bin
```

The full image can overwrite saved settings. Use the OTA image for web updates.

To create the same bundle locally with the toolchain above:

```bash
bash scripts/build-firmware.sh
```

Output is written to `dist/`. The build uses `Cargo.lock` with `--locked`.

## Hardware

The mapping below follows the board [schematic](hardware/schematic.epro2). Channel IDs are zero-based and match the HTTP API.

| Channel | Connector | GPIO | Notes |
| --- | --- | --- | --- |
| 0 | H3 | 42 | — |
| 1 | H4 | 41 | — |
| 2 | H5 | 40 | — |
| 3 | H6 | 39 | — |
| 4 | H7 | 38 | — |
| 5 | H8 | 37 | Shared with octal PSRAM |
| 6 | H9 | 36 | Shared with octal PSRAM |
| 7 | H10 | 35 | Shared with octal PSRAM |
| 8 | H11 | 47 | — |

Each strip connector has three pins: **+5V / DATA / GND**. Check the board silkscreen against the schematic before wiring.

> **PSRAM:** GPIO35/36/37 are shared with octal PSRAM on the N16R8 module. This firmware leaves PSRAM disabled to use those pins for H8/H9/H10.

| Peripheral | Behavior |
| --- | --- |
| BOOT · GPIO0 | Short press: toggle all strips. Hold for 5 seconds: clear Wi-Fi credentials and restart into access-point mode. |
| Voltage sensing · GPIO4 | R7 = 1 kΩ and R8 = 200 Ω form a 6:1 divider for input-voltage measurement. |
| Screw terminal U5 · GPIO48 | Not a strip output; unused by the firmware. |

## HTTP API

Use `http://ledctl.local` as the default base URL. JSON requests use `Content-Type: application/json`.

| Method | Endpoint | Description |
| --- | --- | --- |
| `GET` | `/api/state` | Read strip settings, channel mappings, network status, voltage, uptime, free heap, and firmware version. |
| `POST` | `/api/strips` | Apply a `patch` to the channels listed in `ids`. |
| `POST` | `/api/settings` | Set `hostname` and/or `transition_ms`. Hostname changes take effect after a restart. |
| `POST` | `/api/wifi` | Save `ssid` and `password`, then restart. |
| `POST` | `/api/ota` | Upload a firmware `.bin` as the raw request body, then restart. |
| `POST` | `/api/reboot` | Save settings and restart. |
| `POST` | `/api/factory-reset` | Clear all saved settings, including Wi-Fi credentials, and restart. |

For example, turn on channels 0 and 1 with a moving rainbow:

```bash
curl http://ledctl.local/api/strips \
  -H 'Content-Type: application/json' \
  -d '{"ids":[0,1],"patch":{"on":true,"brightness":200,"effect":"rainbow_cycle"}}'
```

Other JSON request bodies:

```json
{"hostname":"ledctl","transition_ms":800}
```

```json
{"ssid":"your-network","password":"your-password"}
```

The examples above target `/api/settings` and `/api/wifi`, respectively. Strip `patch` fields are defined in [src/config.rs](src/config.rs).

### Effects

| Group | Effect IDs |
| --- | --- |
| Static | `solid`, `gradient` |
| Rainbow | `rainbow`, `rainbow_cycle`, `rainbow_chase`, `confetti` |
| Motion | `breathe`, `heartbeat`, `wave`, `color_wipe`, `theater_chase`, `scanner`, `meteor`, `police` |
| Ambient | `twinkle`, `sparkle`, `candle`, `fire`, `ocean` |

`gradient` uses `color` and `color2`. The `speed` field controls animation speed; `128` is the base speed. Brightness ranges from `0` to `255`.

## Implementation notes

- **Output scheduling:** nine outputs share four RMT transmit channels through four worker threads pinned to core 1. Each worker creates, uses, and releases an RMT channel for its assigned outputs in sequence.
- **Frame timing:** workers target a 20 ms frame interval. A 300-pixel RGB strip takes roughly 9 ms to transmit; three such strips on one worker put the theoretical rate around 30 FPS before overhead. Actual rates depend on strip length, color format, and rendering work.
- **Persistence:** settings are serialized as JSON and written to NVS after approximately two seconds without changes, reducing flash writes while controls are being adjusted.
- **OTA:** two application slots support firmware updates, with rollback enabled. The application marks the running image valid after initialization.

## License

Licensed under the [Apache License, Version 2.0](LICENSE).
