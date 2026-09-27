<div align="center">

# led-controller

**Rust で書かれた、ESP32-S3 向けのスタンドアロン LED ストリップ制御ファームウェア。**

![Rust](https://img.shields.io/badge/Rust-2021-DEA584?logo=rust)
![Platform](https://img.shields.io/badge/ESP32--S3-ESP--IDF-E7352C)
[![License: Apache-2.0](https://img.shields.io/badge/License-Apache--2.0-blue)](LICENSE)

[English](README.md) · [简体中文](README.zh-CN.md) · **日本語**

ブラウザーから 9 系統の LED ストリップを制御。Home Assistant は不要です。

</div>

---

## 概要

`led-controller` は、ESP32-S3-WROOM-1-N16R8 を搭載した専用基板向けのファームウェアです。Web UI と HTTP API を内蔵し、ストリップの設定、エフェクトの選択、ファームウェアの更新を行えます。

- **9 系統の出力** — LED 数、色順、明るさ、エフェクトを個別に設定。API による一括制御にも対応。
- **19 種類のエフェクト** — 単色、グラデーション、虹色、アニメーションを搭載。速度と色の遷移を調整可能。
- **RGB / RGBW** — WS2812/SK6812 出力に対応し、色のバイト順を設定可能。
- **Wi-Fi 設定** — 設定用アクセスポイント、自動再接続、mDNS による名前解決。
- **設定の保存** — JSON 形式の設定を NVS に保存。Web UI からファームウェアを更新可能。
- **基板上の操作** — BOOT ボタンによる点灯切り替えと Wi-Fi リセット、入力電圧の監視。

内蔵 Web UI の表示言語は現在、中国語です。

## はじめに

### ツールチェーン

Xtensa ターゲットには Espressif の Rust ツールチェーンが必要です。標準の Rust ツールチェーンをインストールした環境で、次を実行します。

```bash
cargo +stable install espup espflash ldproxy
espup install
```

`espup` は `esp` ツールチェーンをインストールし、`~/export-esp.sh` を生成します。初回ビルド時に、ESP-IDF のビルド機構がプロジェクトで指定された **v5.5.5** の SDK を `.embuild/` にダウンロードします。

### ビルドと書き込み

基板を USB で接続し、リポジトリのルートで実行します。

```bash
source ~/export-esp.sh
cargo run --release
```

Cargo runner がファームウェアのビルドと書き込みを行い、シリアルモニターを起動します。[espflash.toml](espflash.toml) で 16 MB のフラッシュと[パーティションテーブル](partitions.csv)を指定しています。OTA 用のアプリケーション領域は 4 MiB が 2 つあります。

### 初回接続

1. 基板の電源を入れます。Wi-Fi 設定が未保存の場合、`LEDCTL-XXXX` というアクセスポイントが起動します。パスワードは `ledctl1234` です。
2. このネットワークに接続し、[192.168.71.1](http://192.168.71.1) を開きます。
3. **设置 → WiFi**（設定 → WiFi）でネットワーク名とパスワードを入力して保存します。基板は自動的に再起動します。
4. 普段使っているネットワークに接続し直し、[ledctl.local](http://ledctl.local) を開きます。mDNS が利用できない場合は、ルーターの接続機器一覧で基板の IP アドレスを確認してください。

再接続中、Wi-Fi 監視タスクは約 30 秒間接続できない状態が続くと設定用アクセスポイントを有効にし、バックグラウンドで接続を再試行します。

### Wi-Fi 経由の更新

ビルド後にファームウェアイメージを生成します。

```bash
espflash save-image --chip esp32s3 --flash-size 16mb \
  target/xtensa-esp32s3-espidf/release/led-controller led-controller.bin
```

**设置 → 固件升级**（設定 → ファームウェア更新）から `led-controller.bin` をアップロードします。更新が完了すると基板が再起動します。

## 自動ビルド

[GitHub Actions](.github/workflows/firmware.yml) が push と Pull Request ごとにファームウェアをビルドします。ワークフローがデフォルトブランチに追加されると、**Actions** タブから **Build firmware** を手動実行することもできます。

ビルド成功後、実行結果ページの **Artifacts** から `led-controller-esp32s3-<commit>` をダウンロードして展開します。ダウンロードには GitHub へのログインが必要です。成果物は 30 日間保存されます。

| ファイル | 用途 |
| --- | --- |
| `led-controller-ota.bin` | 基板のファームウェア更新ページからアップロード。 |
| `led-controller-full.bin` | USB 経由の初回書き込み用。ブートローダー、パーティションテーブル、アプリケーションを含み、`0x0` に書き込み。 |
| `led-controller.elf` | デバッグとシンボルの参照。 |
| `SHA256SUMS` | `sha256sum --check SHA256SUMS` でファイルの整合性を確認。 |

USB 経由で完全イメージを書き込むには：

```bash
espflash write-bin --chip esp32s3 0x0 led-controller-full.bin
```

完全イメージの書き込みは保存済み設定を上書きする場合があります。Web 更新には OTA イメージを使用してください。

上記のツールチェーンを使って、ローカルでも同じパッケージを生成できます。

```bash
bash scripts/build-firmware.sh
```

成果物は `dist/` に出力されます。ビルドは `--locked` を指定し、`Cargo.lock` の依存バージョンを使用します。

## ハードウェア

以下は基板の[回路図](hardware/schematic.epro2)に対応するピン割り当てです。チャンネル ID は 0 始まりで、HTTP API と共通です。

| チャンネル | コネクター | GPIO | 備考 |
| --- | --- | --- | --- |
| 0 | H3 | 42 | — |
| 1 | H4 | 41 | — |
| 2 | H5 | 40 | — |
| 3 | H6 | 39 | — |
| 4 | H7 | 38 | — |
| 5 | H8 | 37 | Octal PSRAM と共用 |
| 6 | H9 | 36 | Octal PSRAM と共用 |
| 7 | H10 | 35 | Octal PSRAM と共用 |
| 8 | H11 | 47 | — |

各ストリップコネクターは **+5V / DATA / GND** の 3 ピンです。配線前に、基板のシルク印刷と回路図を照合してください。

> **PSRAM:** N16R8 モジュールの GPIO35/36/37 は Octal PSRAM と共用です。このファームウェアでは PSRAM を無効にし、H8/H9/H10 のストリップ出力に使用しています。

| 周辺機能 | 動作 |
| --- | --- |
| BOOT · GPIO0 | 短押し：全ストリップの点灯・消灯を切り替え。5 秒長押し：Wi-Fi 設定を消去し、アクセスポイントモードで再起動。 |
| 電圧検出 · GPIO4 | R7 = 1 kΩ、R8 = 200 Ω の 6:1 分圧回路で入力電圧を測定。 |
| ネジ端子 U5 · GPIO48 | ストリップ出力ではありません。ファームウェアでは使用しません。 |

## HTTP API

デフォルトのベース URL は `http://ledctl.local` です。JSON リクエストには `Content-Type: application/json` を指定します。

| メソッド | エンドポイント | 説明 |
| --- | --- | --- |
| `GET` | `/api/state` | ストリップ設定、ピン割り当て、ネットワーク状態、電圧、稼働時間、空きヒープ、ファームウェアバージョンを取得。 |
| `POST` | `/api/strips` | `ids` で指定したチャンネルに `patch` を適用。 |
| `POST` | `/api/settings` | `hostname` と `transition_ms` の一方または両方を設定。ホスト名の変更は再起動後に反映。 |
| `POST` | `/api/wifi` | `ssid` と `password` を保存して再起動。 |
| `POST` | `/api/ota` | ファームウェアの `.bin` をリクエストボディに直接指定してアップロードし、再起動。 |
| `POST` | `/api/reboot` | 設定を保存して再起動。 |
| `POST` | `/api/factory-reset` | Wi-Fi を含むすべての保存済み設定を消去して再起動。 |

チャンネル 0 と 1 を点灯し、流れる虹色のエフェクトに切り替える例：

```bash
curl http://ledctl.local/api/strips \
  -H 'Content-Type: application/json' \
  -d '{"ids":[0,1],"patch":{"on":true,"brightness":200,"effect":"rainbow_cycle"}}'
```

その他の JSON リクエストボディの例：

```json
{"hostname":"ledctl","transition_ms":800}
```

```json
{"ssid":"your-network","password":"your-password"}
```

上記はそれぞれ `/api/settings` と `/api/wifi` に送信します。ストリップの `patch` で指定できるフィールドは [src/config.rs](src/config.rs) を参照してください。

### エフェクト

| 分類 | エフェクト ID |
| --- | --- |
| 静的 | `solid`（単色）、`gradient`（2 色グラデーション） |
| 虹色 | `rainbow`（虹色グラデーション）、`rainbow_cycle`（流れる虹色）、`rainbow_chase`（虹色チェイス）、`confetti`（紙吹雪） |
| 動き | `breathe`（呼吸）、`heartbeat`（鼓動）、`wave`（波）、`color_wipe`（順次点灯）、`theater_chase`（シアターチェイス）、`scanner`（スキャン）、`meteor`（流星）、`police`（警告灯） |
| 環境 | `twinkle`（星のまたたき）、`sparkle`（白いきらめき）、`candle`（ろうそく）、`fire`（炎）、`ocean`（海） |

`gradient` は `color` と `color2` を使用します。`speed` はアニメーション速度を指定し、`128` が基準速度です。明るさは `0`〜`255` で指定します。

## 実装について

- **出力のスケジューリング：** core 1 に固定した 4 つのワーカースレッドで、9 系統の出力を処理します。各ワーカーは担当する出力ごとに RMT 送信チャンネルの作成・送信・解放を順番に行い、4 つの送信チャンネルを共有します。
- **フレーム周期：** 目標間隔は 20 ms です。RGB LED 300 個の送信には約 9 ms かかり、1 ワーカーで同じ長さのストリップを 3 本処理する場合、追加の処理時間を除いた理論値は約 30 FPS です。実際のフレームレートは LED 数、色形式、描画負荷によって変わります。
- **永続化：** 設定を JSON にシリアライズし、約 2 秒間変更がなければ NVS に保存します。操作中のフラッシュ書き込み回数を抑えます。
- **OTA：** 2 つのアプリケーション領域を使用し、ロールバックを有効にしています。アプリケーションは初期化完了後に実行中のイメージを有効としてマークします。

## ライセンス

[Apache License 2.0](LICENSE) の下で公開しています。
