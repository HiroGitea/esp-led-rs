//! 设置的数据结构 + 在 NVS 里的持久化。

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};

use esp_idf_svc::nvs::{EspDefaultNvsPartition, EspNvs, NvsDefault};
use log::{info, warn};
use serde::{Deserialize, Serialize};

/// 板子上的灯带输出口，来自原理图 "LED输出接口"。
/// H3~H11 共 9 路，每个口都是 1x3P：+5V / DATA / GND。
/// （U5 螺钉端子接的 IO48 不是灯带口，这里不用。）
pub struct ChannelDef {
    pub gpio: u8,
    pub connector: &'static str,
    /// GPIO35/36/37 在 N16R8 模组上接着八线 PSRAM，固件没启用 PSRAM 时才能用。
    pub shared_with_psram: bool,
}

pub const CHANNELS: [ChannelDef; 9] = [
    ChannelDef { gpio: 42, connector: "H3", shared_with_psram: false },
    ChannelDef { gpio: 41, connector: "H4", shared_with_psram: false },
    ChannelDef { gpio: 40, connector: "H5", shared_with_psram: false },
    ChannelDef { gpio: 39, connector: "H6", shared_with_psram: false },
    ChannelDef { gpio: 38, connector: "H7", shared_with_psram: false },
    ChannelDef { gpio: 37, connector: "H8", shared_with_psram: true },
    ChannelDef { gpio: 36, connector: "H9", shared_with_psram: true },
    ChannelDef { gpio: 35, connector: "H10", shared_with_psram: true },
    ChannelDef { gpio: 47, connector: "H11", shared_with_psram: false },
];

pub const MAX_LEDS: u16 = 1000;

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(rename_all = "UPPERCASE")]
pub enum ColorOrder {
    Grb,
    Rgb,
    Brg,
    Rbg,
    Gbr,
    Bgr,
    Grbw,
    Rgbw,
}

impl ColorOrder {
    /// 输出字节里依次放 (r,g,b,w) 中哪一个的下标。
    pub fn layout(self) -> &'static [usize] {
        match self {
            Self::Grb => &[1, 0, 2],
            Self::Rgb => &[0, 1, 2],
            Self::Brg => &[2, 0, 1],
            Self::Rbg => &[0, 2, 1],
            Self::Gbr => &[1, 2, 0],
            Self::Bgr => &[2, 1, 0],
            Self::Grbw => &[1, 0, 2, 3],
            Self::Rgbw => &[0, 1, 2, 3],
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(rename_all = "snake_case")]
pub enum Effect {
    Solid,
    Rainbow,
    RainbowCycle,
    Breathe,
    ColorWipe,
    TheaterChase,
    Twinkle,
    Fire,
    Scanner,
    Meteor,
    RainbowChase,
    Confetti,
    Wave,
    Police,
    Heartbeat,
    Candle,
    Ocean,
    Sparkle,
    Gradient,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(default)]
pub struct Strip {
    // 硬件配置
    pub name: String,
    pub enabled: bool,
    pub count: u16,
    pub order: ColorOrder,
    pub reverse: bool,
    // 灯光状态
    pub on: bool,
    pub brightness: u8,
    pub color: [u8; 3],
    /// 副色，双色渐变用
    pub color2: [u8; 3],
    pub white: u8,
    pub effect: Effect,
    pub speed: u8,
}

impl Default for Strip {
    fn default() -> Self {
        Self {
            name: String::new(),
            enabled: true,
            count: 60,
            order: ColorOrder::Grb,
            reverse: false,
            on: false,
            brightness: 128,
            color: [255, 160, 60],
            color2: [0, 120, 255],
            white: 0,
            effect: Effect::Solid,
            speed: 128,
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(default)]
pub struct Wifi {
    pub ssid: String,
    pub password: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(default)]
pub struct Settings {
    pub hostname: String,
    pub wifi: Wifi,
    /// 开关/改颜色时的渐变时间
    pub transition_ms: u16,
    pub strips: Vec<Strip>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            hostname: "ledctl".into(),
            wifi: Wifi::default(),
            transition_ms: 800,
            strips: CHANNELS
                .iter()
                .map(|c| Strip {
                    name: format!("{} · GPIO{}", c.connector, c.gpio),
                    ..Default::default()
                })
                .collect(),
        }
    }
}

impl Settings {
    /// 从旧版本/手改的数据恢复时，保证数量和取值范围都合法。
    pub fn sanitize(&mut self) {
        self.strips.resize_with(CHANNELS.len(), Strip::default);
        for (i, s) in self.strips.iter_mut().enumerate() {
            s.count = s.count.clamp(1, MAX_LEDS);
            s.speed = s.speed.max(1);
            if s.name.trim().is_empty() {
                s.name = format!("{} · GPIO{}", CHANNELS[i].connector, CHANNELS[i].gpio);
            }
        }
        if self.hostname.trim().is_empty() {
            self.hostname = "ledctl".into();
        }
        self.transition_ms = self.transition_ms.min(10_000);
    }
}

/// 所有线程共享的设置，`version` 每次修改 +1，用于通知保存线程。
#[derive(Clone)]
pub struct Shared {
    pub settings: Arc<Mutex<Settings>>,
    version: Arc<AtomicU32>,
}

impl Shared {
    pub fn new(settings: Settings) -> Self {
        Self {
            settings: Arc::new(Mutex::new(settings)),
            version: Arc::new(AtomicU32::new(0)),
        }
    }

    pub fn get(&self) -> Settings {
        self.settings.lock().unwrap().clone()
    }

    pub fn update<R>(&self, f: impl FnOnce(&mut Settings) -> R) -> R {
        let r = f(&mut self.settings.lock().unwrap());
        self.version.fetch_add(1, Ordering::SeqCst);
        r
    }

    pub fn version(&self) -> u32 {
        self.version.load(Ordering::SeqCst)
    }
}

const NVS_NAMESPACE: &str = "ledctl";
const NVS_KEY: &str = "settings";

#[derive(Clone)]
pub struct Store {
    nvs: Arc<Mutex<EspNvs<NvsDefault>>>,
}

impl Store {
    pub fn new(partition: EspDefaultNvsPartition) -> anyhow::Result<Self> {
        Ok(Self { nvs: Arc::new(Mutex::new(EspNvs::new(partition, NVS_NAMESPACE, true)?)) })
    }

    pub fn load(&self) -> Settings {
        let loaded = (|| -> anyhow::Result<Option<Settings>> {
            let nvs = self.nvs.lock().unwrap();
            let Some(len) = nvs.blob_len(NVS_KEY)? else { return Ok(None) };
            let mut buf = vec![0u8; len];
            let Some(data) = nvs.get_blob(NVS_KEY, &mut buf)? else { return Ok(None) };
            Ok(Some(serde_json::from_slice(data)?))
        })();

        let mut settings = match loaded {
            Ok(Some(s)) => {
                info!("已从 NVS 读取设置");
                s
            }
            Ok(None) => {
                info!("NVS 里没有设置，使用默认值");
                Settings::default()
            }
            Err(e) => {
                warn!("读取设置失败（{e}），使用默认值");
                Settings::default()
            }
        };
        settings.sanitize();
        settings
    }

    pub fn save(&self, settings: &Settings) -> anyhow::Result<()> {
        let json = serde_json::to_vec(settings)?;
        self.nvs.lock().unwrap().set_blob(NVS_KEY, &json)?;
        info!("设置已保存（{} 字节）", json.len());
        Ok(())
    }

    pub fn erase(&self) -> anyhow::Result<()> {
        self.nvs.lock().unwrap().remove(NVS_KEY)?;
        Ok(())
    }
}

/// 设置变动后等 2 秒没有新变动再写 flash，拖动滑条时不会反复擦写。
pub fn spawn_saver(shared: Shared, store: Store) -> anyhow::Result<()> {
    std::thread::Builder::new()
        .name("saver".into())
        .stack_size(8192)
        .spawn(move || {
            let mut saved = shared.version();
            let mut seen = saved;
            let mut quiet_ticks = 0;
            loop {
                std::thread::sleep(std::time::Duration::from_millis(500));
                let v = shared.version();
                if v != seen {
                    seen = v;
                    quiet_ticks = 0;
                    continue;
                }
                if v != saved {
                    quiet_ticks += 1;
                    if quiet_ticks >= 4 {
                        if let Err(e) = store.save(&shared.get()) {
                            warn!("保存设置失败: {e}");
                        }
                        saved = v;
                    }
                }
            }
        })?;
    Ok(())
}
