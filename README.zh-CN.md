<div align="center">

# led-controller

**使用 Rust 编写的 ESP32-S3 独立灯带控制固件。**

![Rust](https://img.shields.io/badge/Rust-2021-DEA584?logo=rust)
![Platform](https://img.shields.io/badge/ESP32--S3-ESP--IDF-E7352C)
[![License: Apache-2.0](https://img.shields.io/badge/License-Apache--2.0-blue)](LICENSE)

[English](README.md) · **简体中文** · [日本語](README.ja.md)

在浏览器中控制九路灯带，无需 Home Assistant。

</div>

---

## 概览

`led-controller` 面向基于 ESP32-S3-WROOM-1-N16R8 的定制控制板，内置 Web 界面和 HTTP API，可直接配置灯带、切换灯效和更新固件。

- **九路输出** — 每路独立配置灯珠数量、颜色顺序、亮度和灯效，支持通过 API 批量控制。
- **19 种灯效** — 包含纯色、渐变、彩虹和动态效果，支持速度调节和颜色过渡。
- **RGB 与 RGBW** — 支持 WS2812/SK6812 输出，可配置颜色字节顺序。
- **Wi-Fi 配网** — 内置配网热点、自动重连和 mDNS 发现。
- **本地存储** — 配置以 JSON 保存在 NVS 中，固件可通过网页上传更新。
- **板载控制** — BOOT 按键控制灯带开关或重置 Wi-Fi，支持输入电压监测。

内置 Web 界面目前使用中文。

## 快速开始

### 安装工具链

Xtensa 目标需要 Espressif 的 Rust 工具链。安装标准 Rust 工具链后，执行：

```bash
cargo +stable install espup espflash ldproxy
espup install
```

`espup` 会安装 `esp` 工具链并生成 `~/export-esp.sh`。首次编译时，ESP-IDF 构建集成会自动将项目指定的 **v5.5.5** SDK 下载到 `.embuild/`。

### 编译与烧录

通过 USB 连接开发板，在项目根目录执行：

```bash
source ~/export-esp.sh
cargo run --release
```

Cargo runner 会编译、烧录固件并打开串口监视器。[espflash.toml](espflash.toml) 指定 16 MB Flash 和[分区表](partitions.csv)，其中包含两个 4 MiB 的 OTA 应用分区。

### 首次连接

1. 开发板上电后，如果没有保存 Wi-Fi 配置，会开启 `LEDCTL-XXXX` 热点，密码为 `ledctl1234`。
2. 连接该热点，打开 [192.168.71.1](http://192.168.71.1)。
3. 进入 **设置 → WiFi**，填写网络名称和密码，保存后开发板会自动重启。
4. 重新连接日常使用的网络，打开 [ledctl.local](http://ledctl.local)。如果 mDNS 不可用，可在路由器设备列表中查看开发板的 IP 地址。

重连期间，Wi-Fi 监控任务会在持续断线约 30 秒后开启配网热点，并继续在后台尝试连接。

### 无线更新

编译完成后，导出固件镜像：

```bash
espflash save-image --chip esp32s3 --flash-size 16mb \
  target/xtensa-esp32s3-espidf/release/led-controller led-controller.bin
```

在 **设置 → 固件升级** 中上传 `led-controller.bin`。更新完成后，开发板会自动重启。

## 自动编译

[GitHub Actions](.github/workflows/firmware.yml) 会在每次推送和 Pull Request 时编译固件。工作流进入默认分支后，也可以在 **Actions** 页面手动运行 **Build firmware**。

构建成功后，在该次运行页面的 **Artifacts** 中下载 `led-controller-esp32s3-<commit>` 并解压。下载需要登录 GitHub，构建产物保留 30 天。

| 文件 | 用途 |
| --- | --- |
| `led-controller-ota.bin` | 在开发板的固件升级页面上传。 |
| `led-controller-full.bin` | USB 首次烧录：包含引导程序、分区表和应用，写入地址为 `0x0`。 |
| `led-controller.elf` | 调试和符号解析。 |
| `SHA256SUMS` | 使用 `sha256sum --check SHA256SUMS` 校验文件完整性。 |

通过 USB 烧录完整镜像：

```bash
espflash write-bin --chip esp32s3 0x0 led-controller-full.bin
```

完整镜像可能覆盖已保存的设置。网页升级请使用 OTA 镜像。

安装上述工具链后，也可以在本地生成相同的固件包：

```bash
bash scripts/build-firmware.sh
```

产物保存在 `dist/` 中。构建使用 `--locked`，遵循 `Cargo.lock` 锁定的依赖版本。

## 硬件

以下映射对应控制板[原理图](hardware/schematic.epro2)。通道编号从 0 开始，与 HTTP API 一致。

| 通道 | 接口 | GPIO | 备注 |
| --- | --- | --- | --- |
| 0 | H3 | 42 | — |
| 1 | H4 | 41 | — |
| 2 | H5 | 40 | — |
| 3 | H6 | 39 | — |
| 4 | H7 | 38 | — |
| 5 | H8 | 37 | 与八线 PSRAM 共用 |
| 6 | H9 | 36 | 与八线 PSRAM 共用 |
| 7 | H10 | 35 | 与八线 PSRAM 共用 |
| 8 | H11 | 47 | — |

每个灯带接口均为三针：**+5V / DATA / GND**。接线前请对照原理图确认板上丝印。

> **PSRAM：** N16R8 模组的 GPIO35/36/37 与八线 PSRAM 共用。本固件不启用 PSRAM，将这些引脚用于 H8/H9/H10 灯带输出。

| 外设 | 行为 |
| --- | --- |
| BOOT · GPIO0 | 短按：切换全部灯带开关；长按 5 秒：清除 Wi-Fi 配置并重启进入热点模式。 |
| 电压检测 · GPIO4 | R7 = 1 kΩ、R8 = 200 Ω，构成 6:1 分压，用于测量输入电压。 |
| 螺钉端子 U5 · GPIO48 | 不是灯带输出口，固件不使用。 |

## HTTP API

默认基础地址为 `http://ledctl.local`。发送 JSON 时，请设置 `Content-Type: application/json`。

| 方法 | 路径 | 说明 |
| --- | --- | --- |
| `GET` | `/api/state` | 读取灯带配置、通道映射、网络状态、电压、运行时间、可用堆内存及固件版本。 |
| `POST` | `/api/strips` | 将 `patch` 应用到 `ids` 指定的通道。 |
| `POST` | `/api/settings` | 设置 `hostname` 和/或 `transition_ms`；主机名更改在重启后生效。 |
| `POST` | `/api/wifi` | 保存 `ssid` 和 `password`，然后重启。 |
| `POST` | `/api/ota` | 以原始请求体上传固件 `.bin`，完成后重启。 |
| `POST` | `/api/reboot` | 保存配置并重启。 |
| `POST` | `/api/factory-reset` | 清除全部已保存配置（包括 Wi-Fi）并重启。 |

例如，开启通道 0 和 1，并切换为流动彩虹：

```bash
curl http://ledctl.local/api/strips \
  -H 'Content-Type: application/json' \
  -d '{"ids":[0,1],"patch":{"on":true,"brightness":200,"effect":"rainbow_cycle"}}'
```

其他 JSON 请求体示例：

```json
{"hostname":"ledctl","transition_ms":800}
```

```json
{"ssid":"your-network","password":"your-password"}
```

以上分别用于 `/api/settings` 和 `/api/wifi`。灯带 `patch` 支持的字段见 [src/config.rs](src/config.rs)。

### 灯效

| 分类 | 灯效 ID |
| --- | --- |
| 静态 | `solid`（纯色）、`gradient`（双色渐变） |
| 彩虹 | `rainbow`（彩虹渐变）、`rainbow_cycle`（彩虹流动）、`rainbow_chase`（彩虹跑马）、`confetti`（彩色纸屑） |
| 动感 | `breathe`（呼吸）、`heartbeat`（心跳）、`wave`（波浪）、`color_wipe`（逐个点亮）、`theater_chase`（跑马灯）、`scanner`（扫描）、`meteor`（流星）、`police`（警灯） |
| 氛围 | `twinkle`（闪烁星光）、`sparkle`（白光闪点）、`candle`（烛光）、`fire`（火焰）、`ocean`（海浪） |

`gradient` 使用 `color` 和 `color2` 两种颜色。`speed` 控制动态效果的速度，`128` 为基准速度。亮度范围为 `0`–`255`。

## 实现说明

- **输出调度：** 九路输出由固定在 core 1 的四个工作线程调度，共享四个 RMT 发送通道。每个线程依次为分配到的输出创建、使用和释放 RMT 通道。
- **帧率：** 工作线程以 20 ms 为目标帧间隔。300 颗 RGB 灯珠的发送时间约为 9 ms；同一线程驱动三条这样的灯带时，未计额外开销的理论帧率约为 30 FPS。实际帧率取决于灯带长度、颜色格式和渲染开销。
- **持久化：** 配置序列化为 JSON，连续约两秒没有改动后写入 NVS，减少调节控件时的 Flash 写入次数。
- **OTA：** 使用两个应用分区并启用回滚；应用完成初始化后，将当前固件标记为有效。

## 许可证

本项目采用 [Apache License 2.0](LICENSE)。
