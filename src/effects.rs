//! 灯效渲染。每个效果输出满亮度的 RGBW 像素，亮度和 gamma 在 led.rs 里统一处理。

use core::f32::consts::TAU;

use crate::config::{Effect, Strip};

pub type Px = [u8; 4];

/// 每条灯带自己的效果状态（闪烁、火焰、扫描这些需要记住上一帧）。
#[derive(Default)]
pub struct EffectState {
    effect: Option<Effect>,
    levels: Vec<u8>,
    trail: Vec<u8>,
    colors: Vec<Px>,
}

pub fn render(state: &mut EffectState, strip: &Strip, t: f32, out: &mut Vec<Px>) {
    let n = strip.count as usize;
    out.clear();
    out.resize(n, [0; 4]);
    if state.effect != Some(strip.effect) || state.levels.len() != n {
        state.effect = Some(strip.effect);
        state.levels = vec![0; n];
        state.trail = vec![0; n];
        state.colors = vec![[0; 4]; n];
    }

    // speed 128 为 1 倍速，1..255 大约对应 0.01..2 倍
    let speed = strip.speed as f32 / 128.0;
    let ts = t * speed;
    let [r, g, b] = strip.color;
    let base: Px = [r, g, b, strip.white];

    match strip.effect {
        Effect::Solid => out.fill(base),
        Effect::Rainbow => out.fill(hue(ts * 40.0)),
        Effect::RainbowCycle => {
            for (i, p) in out.iter_mut().enumerate() {
                *p = hue(i as f32 * 360.0 / n as f32 + ts * 90.0);
            }
        }
        Effect::Breathe => {
            let k = 0.08 + 0.92 * (0.5 - 0.5 * (ts * TAU / 4.0).cos());
            out.fill(scale(base, k));
        }
        Effect::ColorWipe => {
            // 一个周期：从头铺满，再从头熄灭
            let period = (n as f32 / 30.0).max(1.0) * 2.0;
            let phase = (ts % period) / period * 2.0;
            let lit = ((phase % 1.0) * n as f32) as usize;
            for (i, p) in out.iter_mut().enumerate() {
                let on = if phase < 1.0 { i <= lit } else { i > lit };
                *p = if on { base } else { [0; 4] };
            }
        }
        Effect::TheaterChase => {
            let offset = (ts * 8.0) as usize % 3;
            for (i, p) in out.iter_mut().enumerate() {
                *p = if i % 3 == offset { base } else { [0; 4] };
            }
        }
        Effect::Twinkle => {
            // levels 高位表示在变亮
            let rate = (4.0 * speed).clamp(1.0, 30.0) as u8;
            for l in state.levels.iter_mut() {
                let v = *l & 0x7f;
                if *l & 0x80 != 0 {
                    *l = if v >= 127 - rate { 127 } else { (v + rate) | 0x80 };
                } else {
                    *l = v.saturating_sub(rate / 2 + 1);
                }
            }
            let spawns = (n / 40).max(1);
            for _ in 0..spawns {
                if rand() % 4 == 0 {
                    let i = rand() as usize % n;
                    if state.levels[i] == 0 {
                        state.levels[i] = 0x80 | 1;
                    }
                }
            }
            for (p, l) in out.iter_mut().zip(&state.levels) {
                *p = scale(base, (*l & 0x7f) as f32 / 127.0);
            }
        }
        Effect::Fire => fire(&mut state.levels, speed, out),
        Effect::Scanner => {
            // 来回移动的光点带渐隐拖尾
            let fade = (40.0 * speed).clamp(8.0, 120.0) as u8;
            for v in state.trail.iter_mut() {
                *v = v.saturating_sub(fade);
            }
            if n > 1 {
                let span = (n - 1) as f32;
                let pos = (ts * 30.0) % (2.0 * span);
                let pos = if pos > span { 2.0 * span - pos } else { pos };
                let i = pos.round() as usize;
                state.trail[i.min(n - 1)] = 255;
            } else {
                state.trail[0] = 255;
            }
            for (p, v) in out.iter_mut().zip(&state.trail) {
                *p = scale(base, *v as f32 / 255.0);
            }
        }
        Effect::Meteor => {
            // 流星：头部亮，尾巴随机衰减成星尘
            for v in state.trail.iter_mut() {
                if rand() % 3 != 0 {
                    *v = (*v as u16 * 190 / 255) as u8;
                }
            }
            let len = (n / 15).max(3);
            let span = n + len * 3;
            let head = (ts * 40.0) as usize % span;
            for j in 0..len {
                if let Some(i) = head.checked_sub(j).filter(|i| *i < n) {
                    state.trail[i] = 255;
                }
            }
            for (p, v) in out.iter_mut().zip(&state.trail) {
                *p = scale(base, *v as f32 / 255.0);
            }
        }
        Effect::RainbowChase => {
            let offset = (ts * 8.0) as usize % 3;
            for (i, p) in out.iter_mut().enumerate() {
                *p = if i % 3 == offset { hue(i as f32 * 360.0 / n as f32 + ts * 60.0) } else { [0; 4] };
            }
        }
        Effect::Confetti => {
            // 彩色纸屑：随机位置冒出随机颜色，慢慢熄灭
            for c in state.colors.iter_mut() {
                *c = scale(*c, 0.93);
            }
            for _ in 0..(n / 30).max(1) {
                if rand() % 3 == 0 {
                    let i = rand() as usize % n;
                    state.colors[i] = hue((rand() % 360) as f32);
                }
            }
            out.copy_from_slice(&state.colors);
        }
        Effect::Wave => {
            let wavelength = (n as f32 / 3.0).clamp(8.0, 40.0);
            for (i, p) in out.iter_mut().enumerate() {
                let k = 0.5 + 0.5 * (i as f32 * TAU / wavelength - ts * 4.0).sin();
                *p = scale(base, 0.05 + 0.95 * k * k);
            }
        }
        Effect::Police => {
            // 左半红、右半蓝，各闪两下
            let phase = ts % 1.0;
            let flash = ((phase % 0.5) * 8.0) as u32 % 2 == 0;
            let half = n / 2;
            for (i, p) in out.iter_mut().enumerate() {
                *p = match (phase < 0.5, i < half.max(1), flash) {
                    (true, true, true) => [255, 0, 0, 0],
                    (false, false, true) => [0, 0, 255, 0],
                    _ => [0; 4],
                };
            }
        }
        Effect::Heartbeat => {
            let phase = (ts / 1.2) % 1.0;
            let beat = |x: f32| if x < 0.0 { 0.0 } else { (-x * 14.0).exp() };
            let k = beat(phase).max(0.6 * beat(phase - 0.22));
            out.fill(scale(base, 0.04 + 0.96 * k));
        }
        Effect::Candle => {
            // 每颗灯亮度随机游走，像烛火跳动
            for (l, t) in state.levels.iter_mut().zip(state.trail.iter_mut()) {
                if rand() % 6 == 0 {
                    *t = 120 + (rand() % 136) as u8;
                }
                *l = ((*l as u16 * 3 + *t as u16) / 4) as u8;
            }
            for (p, l) in out.iter_mut().zip(&state.levels) {
                *p = scale(base, *l as f32 / 255.0);
            }
        }
        Effect::Ocean => {
            for (i, p) in out.iter_mut().enumerate() {
                let x = i as f32;
                let a = (x * 0.31 + ts * 1.3).sin();
                let b = (x * 0.13 - ts * 0.7).sin();
                *p = scale(hue(195.0 + 25.0 * a), 0.35 + 0.325 * (b + 1.0));
            }
        }
        Effect::Sparkle => {
            // 底色常亮，随机白光闪点
            for v in state.trail.iter_mut() {
                *v = v.saturating_sub(40);
            }
            for _ in 0..(n / 25).max(1) {
                if rand() % 3 == 0 {
                    state.trail[rand() as usize % n] = 255;
                }
            }
            let dim = scale(base, 0.3);
            for (p, v) in out.iter_mut().zip(&state.trail) {
                *p = dim.map(|c| c.max(*v));
            }
        }
        Effect::Gradient => {
            // 主色 ↔ 副色 的渐变，缓慢流动
            let [r2, g2, b2] = strip.color2;
            let c2: Px = [r2, g2, b2, strip.white];
            for (i, p) in out.iter_mut().enumerate() {
                let f = (i as f32 / n as f32 + ts * 0.1).rem_euclid(1.0);
                let t = 1.0 - (2.0 * f - 1.0).abs();
                *p = lerp(base, c2, t);
            }
        }
    }

    if strip.reverse {
        out.reverse();
    }
}

/// FastLED 的 Fire2012 算法
fn fire(heat: &mut [u8], speed: f32, out: &mut [Px]) {
    let n = heat.len();
    let cooling = 55u32;
    let sparking = (120.0 * speed.min(2.0)) as u32;
    for h in heat.iter_mut() {
        let cool = rand() % ((cooling * 10) / n as u32 + 2);
        *h = h.saturating_sub(cool.min(255) as u8);
    }
    for k in (2..n).rev() {
        heat[k] = ((heat[k - 1] as u16 + 2 * heat[k - 2] as u16) / 3) as u8;
    }
    if rand() % 255 < sparking {
        let y = rand() as usize % 7.min(n);
        heat[y] = heat[y].saturating_add(160 + (rand() % 96) as u8);
    }
    for (p, h) in out.iter_mut().zip(heat.iter()) {
        let t = ((*h as u16 * 191) / 255) as u8;
        let ramp = (t & 0x3f) << 2;
        *p = match t {
            0..=63 => [ramp, 0, 0, 0],
            64..=127 => [255, ramp, 0, 0],
            _ => [255, 255, ramp, 0],
        };
    }
}

fn lerp(a: Px, b: Px, t: f32) -> Px {
    let mut o = [0; 4];
    for i in 0..4 {
        o[i] = (a[i] as f32 + (b[i] as f32 - a[i] as f32) * t) as u8;
    }
    o
}

fn scale(p: Px, k: f32) -> Px {
    p.map(|c| (c as f32 * k) as u8)
}

/// 满饱和度满亮度的色相 → RGB，h 为角度
pub fn hue(h: f32) -> Px {
    let h = h.rem_euclid(360.0) / 60.0;
    let x = ((1.0 - ((h % 2.0) - 1.0).abs()) * 255.0) as u8;
    let (r, g, b) = match h as u32 {
        0 => (255, x, 0),
        1 => (x, 255, 0),
        2 => (0, 255, x),
        3 => (0, x, 255),
        4 => (x, 0, 255),
        _ => (255, 0, x),
    };
    [r, g, b, 0]
}

fn rand() -> u32 {
    unsafe { esp_idf_svc::sys::esp_random() }
}
