mod config;
mod effects;
mod hw;
mod led;
mod net;
mod web;

use esp_idf_svc::eventloop::EspSystemEventLoop;
use esp_idf_svc::hal::peripherals::Peripherals;
use esp_idf_svc::mdns::EspMdns;
use esp_idf_svc::nvs::EspDefaultNvsPartition;
use esp_idf_svc::ota::EspOta;
use log::{info, warn};

fn main() -> anyhow::Result<()> {
    esp_idf_svc::sys::link_patches();
    esp_idf_svc::log::EspLogger::initialize_default();
    info!("LED 控制器 v{} 启动", env!("CARGO_PKG_VERSION"));

    let peripherals = Peripherals::take()?;
    let sys_loop = EspSystemEventLoop::take()?;
    let nvs = EspDefaultNvsPartition::take()?;

    let store = config::Store::new(nvs.clone())?;
    let settings = store.load();
    let shared = config::Shared::new(settings.clone());

    // 先点灯，WiFi 慢慢连
    led::start(shared.clone())?;
    config::spawn_saver(shared.clone(), store.clone())?;
    hw::spawn_button(shared.clone(), store.clone())?;
    let voltage = hw::spawn_voltage(peripherals.adc1, peripherals.pins.gpio4)?;

    let net = net::start(peripherals.modem, sys_loop, nvs, settings.wifi.clone(), &settings.hostname)?;

    let mut mdns = EspMdns::take()?;
    mdns.set_hostname(&settings.hostname)?;
    mdns.set_instance_name("LED 控制器")?;
    mdns.add_service(None, "_http", "_tcp", 80, &[])?;
    info!("网页地址：http://{}.local", settings.hostname);

    let _server = web::start(web::Ctx { shared, store, net, voltage })?;

    // 能跑到这里说明新固件正常，确认 OTA（否则下次重启会回滚到旧固件）
    if let Err(e) = EspOta::new().and_then(|mut o| o.mark_running_slot_valid()) {
        warn!("标记固件有效失败: {e}");
    }

    loop {
        std::thread::sleep(std::time::Duration::from_secs(60));
    }
}
