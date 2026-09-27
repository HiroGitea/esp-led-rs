//! WS2812/SK6812 输出。
//!
//! ESP32-S3 只有 4 个 RMT 发送通道，板子上却有 9 个灯带口，所以分成 4 条"车道"：
//! 每条车道一个线程，轮流为分到的几个口临时创建 RMT 通道 → 发送 → 释放。
//! 空闲的数据脚保持输出低电平，灯带不会收到杂波。

use std::time::{Duration, Instant};

use esp_idf_svc::hal::cpu::Core;
use esp_idf_svc::hal::gpio::AnyOutputPin;
use esp_idf_svc::hal::rmt::config::{MemoryAccess, TransmitConfig, TxChannelConfig};
use esp_idf_svc::hal::rmt::encoder::{BytesEncoder, BytesEncoderConfig};
use esp_idf_svc::hal::rmt::{PinState, Pulse, PulseTicks, Symbol, TxChannelDriver};
use esp_idf_svc::hal::task::thread::ThreadSpawnConfiguration;
use esp_idf_svc::hal::units::FromValueType;
use esp_idf_svc::sys;
use log::{info, warn};

use crate::config::{Shared, Strip, CHANNELS};
use crate::effects::{self, EffectState, Px};

const LANES: usize = 4;
const FRAME: Duration = Duration::from_millis(20);
const RMT_HZ: u32 = 10_000_000;

/// 每条灯带在渲染线程里的运行时状态
struct Output {
    idx: usize,
    gpio: u8,
    effect: EffectState,
    // 当前正在渐变中的亮度/颜色（0..=1 / 0..=255）
    level: f32,
    color: [f32; 4],
    pixels: Vec<Px>,
    bytes: Vec<u8>,
    dark_since: Option<Instant>,
    last_sent: Option<Instant>,
    /// 上次发出的字节数；灯数改小或关闭口时，多出来的灯要补发一次黑色
    sent_len: usize,
    parked: bool,
}

pub fn start(shared: Shared) -> anyhow::Result<()> {
    for ch in CHANNELS.iter() {
        init_pin(ch.gpio);
    }

    // 渲染线程固定在 core 1，WiFi 在 core 0，互不打扰
    ThreadSpawnConfiguration {
        name: Some(c"led"),
        stack_size: 8192,
        priority: 20,
        pin_to_core: Some(Core::Core1),
        ..Default::default()
    }
    .set()?;

    for lane in 0..LANES {
        let shared = shared.clone();
        let outputs: Vec<Output> = CHANNELS
            .iter()
            .enumerate()
            .filter(|(i, _)| i % LANES == lane)
            .map(|(idx, ch)| Output {
                idx,
                gpio: ch.gpio,
                effect: EffectState::default(),
                level: 0.0,
                color: [0.0; 4],
                pixels: Vec::new(),
                bytes: Vec::new(),
                dark_since: None,
                last_sent: None,
                sent_len: 0,
                parked: true,
            })
            .collect();
        std::thread::Builder::new()
            .stack_size(8192)
            .spawn(move || lane_loop(shared, outputs))?;
    }

    ThreadSpawnConfiguration::default().set()?;
    info!("灯带输出已启动：{} 个口，{} 条 RMT 车道", CHANNELS.len(), LANES);
    Ok(())
}

fn lane_loop(shared: Shared, mut outputs: Vec<Output>) {
    let mut encoder = new_encoder().expect("创建 RMT 编码器失败");
    let gamma = gamma_table(2.8);
    let start = Instant::now();
    let mut last = Instant::now();

    loop {
        let frame_start = Instant::now();
        let dt = frame_start.duration_since(last).as_secs_f32();
        last = frame_start;
        // 按天取模，避免 f32 时间长时间运行后精度下降
        let t = (frame_start.duration_since(start).as_secs_f64() % 86_400.0) as f32;

        for out in outputs.iter_mut() {
            let (strip, transition_ms) = {
                let s = shared.settings.lock().unwrap();
                (s.strips[out.idx].clone(), s.transition_ms)
            };
            if !strip.enabled {
                if !out.parked {
                    // 关闭前把整条灯带清黑
                    let _ = transmit(out.gpio, &vec![0; out.sent_len], &mut encoder);
                    out.sent_len = 0;
                    out.parked = true;
                }
                out.last_sent = None;
                continue;
            }

            let dark = out.render(&strip, transition_ms, t, dt, &gamma);

            // 灯全灭且已经发过几帧黑色时，降到每秒刷新一次（防止热插拔后残留）
            let now = Instant::now();
            if dark {
                let since = *out.dark_since.get_or_insert(now);
                let recently = out.last_sent.is_some_and(|l| now - l < Duration::from_secs(1));
                if now - since > Duration::from_millis(200) && recently {
                    continue;
                }
            } else {
                out.dark_since = None;
            }

            let len = out.bytes.len();
            if out.sent_len > len {
                out.bytes.resize(out.sent_len, 0);
            }
            if let Err(e) = transmit(out.gpio, &out.bytes, &mut encoder) {
                warn!("GPIO{} 发送失败: {e}", out.gpio);
            }
            out.sent_len = len;
            out.parked = false;
            out.last_sent = Some(now);
        }

        let spent = frame_start.elapsed();
        std::thread::sleep(FRAME.saturating_sub(spent).max(Duration::from_millis(1)));
    }
}

impl Output {
    /// 渲染一帧到 `self.bytes`，返回是否全黑。
    fn render(&mut self, strip: &Strip, transition_ms: u16, t: f32, dt: f32, gamma: &[u8; 256]) -> bool {
        // 渐变：线性逼近目标亮度和颜色
        let step = if transition_ms == 0 { 1.0 } else { dt * 1000.0 / transition_ms as f32 };
        let target_level = if strip.on { strip.brightness as f32 / 255.0 } else { 0.0 };
        self.level = approach(self.level, target_level, step);
        let target_color = [strip.color[0], strip.color[1], strip.color[2], strip.white];
        for (c, tc) in self.color.iter_mut().zip(target_color) {
            *c = approach(*c / 255.0, tc as f32 / 255.0, step) * 255.0;
        }

        let mut s = strip.clone();
        s.color = [self.color[0] as u8, self.color[1] as u8, self.color[2] as u8];
        s.white = self.color[3] as u8;

        if self.level <= 0.0 {
            self.pixels.clear();
            self.pixels.resize(strip.count as usize, [0; 4]);
        } else {
            effects::render(&mut self.effect, &s, t, &mut self.pixels);
        }

        let layout = strip.order.layout();
        let k = (self.level * 256.0) as u32;
        self.bytes.clear();
        let mut dark = true;
        for p in &self.pixels {
            for &ci in layout {
                let v = gamma[((p[ci] as u32 * k) >> 8) as usize];
                dark &= v == 0;
                self.bytes.push(v);
            }
        }
        dark
    }
}

fn approach(cur: f32, target: f32, step: f32) -> f32 {
    if (target - cur).abs() <= step {
        target
    } else if target > cur {
        cur + step
    } else {
        cur - step
    }
}

fn gamma_table(g: f32) -> [u8; 256] {
    let mut t = [0u8; 256];
    for (i, v) in t.iter_mut().enumerate() {
        *v = ((i as f32 / 255.0).powf(g) * 255.0 + 0.5) as u8;
        // 非零输入至少给 1，低亮度时不会直接熄灭
        if i > 0 && *v == 0 {
            *v = 1;
        }
    }
    t
}

/// WS2812 时序（10MHz 计数）：0 码 0.3µs 高 + 0.9µs 低，1 码 0.9µs 高 + 0.3µs 低。
/// SK6812 也兼容这个时序。
fn new_encoder() -> anyhow::Result<BytesEncoder> {
    let ticks = |n: u16| PulseTicks::new(n).unwrap();
    let bit0 = Symbol::new(Pulse::new(PinState::High, ticks(3)), Pulse::new(PinState::Low, ticks(9)));
    let bit1 = Symbol::new(Pulse::new(PinState::High, ticks(9)), Pulse::new(PinState::Low, ticks(3)));
    Ok(BytesEncoder::with_config(&BytesEncoderConfig {
        bit0,
        bit1,
        msb_first: true,
        ..Default::default()
    })?)
}

fn transmit(gpio: u8, data: &[u8], encoder: &mut BytesEncoder) -> anyhow::Result<()> {
    if data.is_empty() {
        return Ok(());
    }
    {
        // SAFETY: 每个 GPIO 只属于一条车道，同一时间只有这里在用它
        let pin = unsafe { AnyOutputPin::steal(gpio) };
        let config = TxChannelConfig {
            resolution: RMT_HZ.Hz().into(),
            memory_access: MemoryAccess::Indirect { memory_block_symbols: 48 },
            transaction_queue_depth: 1,
            ..Default::default()
        };
        let mut channel = TxChannelDriver::new(pin, &config)?;
        // 不能用 send_and_wait：它会把编码器包一层 Rust 包装，esp-idf-hal 0.47 的包装
        // 在中断里遇到组合状态（COMPLETE|MEM_FULL）会 panic。start_send 直接把 C 编码器交给驱动。
        // SAFETY: encoder 和 data 在 wait_all_done 返回前一直有效且不会被修改
        unsafe { channel.start_send(encoder, data, &TransmitConfig::default())? };
        channel.wait_all_done(Some(Duration::from_millis(200)))?;
    }
    park_pin(gpio);
    Ok(())
}

fn init_pin(gpio: u8) {
    unsafe { sys::gpio_reset_pin(gpio as sys::gpio_num_t) };
    park_pin(gpio);
}

/// 通道释放后脚会变成浮空输入，这里把它拉回输出低电平（也就是 WS2812 的"复位/空闲"电平）。
/// 不能用 gpio_reset_pin：它会短暂打开上拉。
fn park_pin(gpio: u8) {
    let pin = gpio as sys::gpio_num_t;
    unsafe {
        sys::gpio_set_pull_mode(pin, sys::gpio_pull_mode_t_GPIO_PULLDOWN_ONLY);
        sys::gpio_set_level(pin, 0);
        sys::gpio_set_direction(pin, sys::gpio_mode_t_GPIO_MODE_OUTPUT);
    }
}
