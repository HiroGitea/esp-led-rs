//! 板载外设：BOOT 按键（IO0）和输入电压检测（IO4）。

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use esp_idf_svc::hal::adc::attenuation::DB_12;
use esp_idf_svc::hal::adc::oneshot::config::{AdcChannelConfig, Calibration};
use esp_idf_svc::hal::adc::oneshot::{AdcChannelDriver, AdcDriver};
use esp_idf_svc::hal::adc::ADC1;
use esp_idf_svc::hal::gpio::Gpio4;
use esp_idf_svc::sys;
use log::{info, warn};

use crate::config::{Shared, Store, Wifi};

const BUTTON_GPIO: sys::gpio_num_t = 0;
const WIFI_RESET_HOLD: Duration = Duration::from_secs(5);

/// 原理图 "电压检测"：DC_VCC — R7 1kΩ — IO4 — R8 200Ω — GND，分压比 1/6
const DIVIDER: f32 = (1000.0 + 200.0) / 200.0;

/// 输入电压，单位 mV
pub type Voltage = Arc<AtomicU32>;

/// 短按：所有灯带一起开/关。长按 5 秒：清除 WiFi 设置并重启（进入热点模式）。
pub fn spawn_button(shared: Shared, store: Store) -> anyhow::Result<()> {
    unsafe {
        sys::gpio_reset_pin(BUTTON_GPIO);
        sys::gpio_set_direction(BUTTON_GPIO, sys::gpio_mode_t_GPIO_MODE_INPUT);
        sys::gpio_set_pull_mode(BUTTON_GPIO, sys::gpio_pull_mode_t_GPIO_PULLUP_ONLY);
    }
    std::thread::Builder::new()
        .name("button".into())
        .stack_size(6144)
        .spawn(move || {
            let mut pressed_at: Option<Instant> = None;
            loop {
                std::thread::sleep(Duration::from_millis(20));
                let down = unsafe { sys::gpio_get_level(BUTTON_GPIO) } == 0;
                match (down, pressed_at) {
                    (true, None) => pressed_at = Some(Instant::now()),
                    (true, Some(t)) if t.elapsed() >= WIFI_RESET_HOLD => {
                        warn!("长按 BOOT：清除 WiFi 设置并重启");
                        let mut s = shared.get();
                        s.wifi = Wifi::default();
                        if let Err(e) = store.save(&s) {
                            warn!("保存失败: {e}");
                        }
                        std::thread::sleep(Duration::from_millis(300));
                        unsafe { sys::esp_restart() };
                    }
                    (false, Some(t)) => {
                        pressed_at = None;
                        let held = t.elapsed();
                        if held >= Duration::from_millis(40) && held < Duration::from_secs(1) {
                            shared.update(|s| {
                                let any_on = s.strips.iter().any(|x| x.enabled && x.on);
                                for x in s.strips.iter_mut() {
                                    x.on = !any_on;
                                }
                                info!("按键：全部{}", if any_on { "关闭" } else { "打开" });
                            });
                        }
                    }
                    _ => {}
                }
            }
        })?;
    Ok(())
}

pub fn spawn_voltage(adc: ADC1<'static>, pin: Gpio4<'static>) -> anyhow::Result<Voltage> {
    let voltage: Voltage = Arc::new(AtomicU32::new(0));
    let v = voltage.clone();
    std::thread::Builder::new()
        .name("adc".into())
        .stack_size(6144)
        .spawn(move || {
            let run = || -> anyhow::Result<()> {
                let adc = AdcDriver::new(adc)?;
                let config = AdcChannelConfig {
                    attenuation: DB_12,
                    calibration: Calibration::Curve,
                    ..Default::default()
                };
                let mut ch = AdcChannelDriver::new(&adc, pin, &config)?;
                let mut avg: Option<f32> = None;
                loop {
                    let mut sum = 0u32;
                    for _ in 0..16 {
                        sum += ch.read()? as u32;
                    }
                    let mv = sum as f32 / 16.0 * DIVIDER;
                    let a = avg.map_or(mv, |a| a * 0.8 + mv * 0.2);
                    avg = Some(a);
                    v.store(a as u32, Ordering::Relaxed);
                    std::thread::sleep(Duration::from_millis(500));
                }
            };
            if let Err(e) = run() {
                warn!("电压检测停止: {e}");
            }
        })?;
    Ok(voltage)
}
