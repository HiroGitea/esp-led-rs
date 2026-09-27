//! WiFi：优先连路由器；没配置或连不上时开热点，手机连上热点就能进网页配置。

use std::net::Ipv4Addr;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use esp_idf_svc::eventloop::EspSystemEventLoop;
use esp_idf_svc::handle::RawHandle;
use esp_idf_svc::hal::modem::Modem;
use esp_idf_svc::nvs::EspDefaultNvsPartition;
use esp_idf_svc::wifi::{
    AccessPointConfiguration, AuthMethod, BlockingWifi, ClientConfiguration, Configuration, EspWifi,
};
use log::{info, warn};
use serde::Serialize;

use crate::config::Wifi;

/// 热点密码（WPA2 至少 8 位）
pub const AP_PASSWORD: &str = "ledctl1234";
/// 连不上路由器多久后打开热点
const FALLBACK_AFTER: Duration = Duration::from_secs(30);

#[derive(Serialize, Clone, Default)]
pub struct NetStatus {
    pub sta_ssid: String,
    pub sta_connected: bool,
    pub sta_ip: Option<Ipv4Addr>,
    pub rssi: Option<i32>,
    pub ap_ssid: Option<String>,
    pub ap_ip: Option<Ipv4Addr>,
}

pub type SharedStatus = Arc<Mutex<NetStatus>>;

pub fn start(
    modem: Modem<'static>,
    sys_loop: EspSystemEventLoop,
    nvs: EspDefaultNvsPartition,
    creds: Wifi,
    hostname: &str,
) -> anyhow::Result<SharedStatus> {
    let esp_wifi = EspWifi::new(modem, sys_loop.clone(), Some(nvs))?;
    // DHCP 里上报的主机名，路由器的设备列表里会显示这个
    let host = std::ffi::CString::new(hostname)?;
    esp_idf_svc::sys::esp!(unsafe {
        esp_idf_svc::sys::esp_netif_set_hostname(esp_wifi.sta_netif().handle(), host.as_ptr())
    })?;
    let mut wifi = BlockingWifi::wrap(esp_wifi, sys_loop)?;

    let mac = wifi.wifi().sta_netif().get_mac()?;
    let ap_ssid = format!("LEDCTL-{:02X}{:02X}", mac[4], mac[5]);
    let status: SharedStatus = Arc::new(Mutex::new(NetStatus {
        sta_ssid: creds.ssid.clone(),
        ..Default::default()
    }));

    let client = (!creds.ssid.is_empty()).then(|| ClientConfiguration {
        ssid: creds.ssid.as_str().try_into().unwrap_or_default(),
        password: creds.password.as_str().try_into().unwrap_or_default(),
        auth_method: if creds.password.is_empty() { AuthMethod::None } else { AuthMethod::WPA2Personal },
        ..Default::default()
    });
    let ap = AccessPointConfiguration {
        ssid: ap_ssid.as_str().try_into().unwrap(),
        password: AP_PASSWORD.try_into().unwrap(),
        auth_method: AuthMethod::WPA2Personal,
        channel: 6,
        max_connections: 4,
        ..Default::default()
    };

    match &client {
        None => {
            info!("没有配置 WiFi，开启热点 {ap_ssid}（密码 {AP_PASSWORD}）");
            wifi.set_configuration(&Configuration::AccessPoint(ap.clone()))?;
            wifi.start()?;
            wifi.wait_netif_up()?;
        }
        Some(c) => {
            info!("连接 WiFi \"{}\" …", creds.ssid);
            wifi.set_configuration(&Configuration::Client(c.clone()))?;
            wifi.start()?;
            if let Err(e) = wifi.connect().and_then(|_| wifi.wait_netif_up()) {
                warn!("首次连接失败（{e}），后台继续重试");
            }
        }
    }
    refresh(&wifi, &status);

    let st = status.clone();
    std::thread::Builder::new()
        .name("wifi".into())
        .stack_size(8192)
        .spawn(move || supervise(wifi, client, ap, st))?;
    Ok(status)
}

/// 断线重连；长时间连不上就打开热点（STA+AP 同时工作），连上且热点没人用时再关掉热点。
fn supervise(
    mut wifi: BlockingWifi<EspWifi<'static>>,
    client: Option<ClientConfiguration>,
    ap: AccessPointConfiguration,
    status: SharedStatus,
) {
    let Some(client) = client else {
        loop {
            refresh(&wifi, &status);
            std::thread::sleep(Duration::from_secs(5));
        }
    };

    let mut down_since: Option<Instant> = None;
    let mut ap_on = false;
    loop {
        let connected = wifi.is_connected().unwrap_or(false) && wifi.is_up().unwrap_or(false);
        if connected {
            down_since = None;
            if ap_on && ap_station_count() == 0 {
                info!("已连上路由器，关闭热点");
                if wifi.set_configuration(&Configuration::Client(client.clone())).is_ok() {
                    ap_on = false;
                }
            }
        } else {
            let since = *down_since.get_or_insert_with(Instant::now);
            if !ap_on && since.elapsed() > FALLBACK_AFTER {
                info!("超过 {}s 没连上路由器，打开热点 {}", FALLBACK_AFTER.as_secs(), ap.ssid);
                match wifi.set_configuration(&Configuration::Mixed(client.clone(), ap.clone())) {
                    Ok(()) => ap_on = true,
                    Err(e) => warn!("打开热点失败: {e}"),
                }
            }
            if let Err(e) = wifi.connect().and_then(|_| wifi.wait_netif_up()) {
                warn!("WiFi 重连失败: {e}");
            } else {
                info!("WiFi 已重新连上");
            }
        }
        refresh(&wifi, &status);
        std::thread::sleep(Duration::from_secs(5));
    }
}

fn refresh(wifi: &BlockingWifi<EspWifi<'static>>, status: &SharedStatus) {
    let w = wifi.wifi();
    let mut s = status.lock().unwrap();
    s.sta_connected = w.is_connected().unwrap_or(false);
    s.sta_ip = w
        .sta_netif()
        .get_ip_info()
        .ok()
        .map(|i| i.ip)
        .filter(|ip| !ip.is_unspecified());
    s.rssi = s.sta_connected.then(sta_rssi).flatten();
    match wifi.get_configuration() {
        Ok(Configuration::AccessPoint(ap)) | Ok(Configuration::Mixed(_, ap)) => {
            s.ap_ssid = Some(ap.ssid.to_string());
            s.ap_ip = w.ap_netif().get_ip_info().ok().map(|i| i.ip);
        }
        _ => {
            s.ap_ssid = None;
            s.ap_ip = None;
        }
    }
}

fn sta_rssi() -> Option<i32> {
    let mut rssi = 0;
    let r = unsafe { esp_idf_svc::sys::esp_wifi_sta_get_rssi(&mut rssi) };
    (r == 0).then_some(rssi)
}

fn ap_station_count() -> i32 {
    let mut list: esp_idf_svc::sys::wifi_sta_list_t = unsafe { core::mem::zeroed() };
    let r = unsafe { esp_idf_svc::sys::esp_wifi_ap_get_sta_list(&mut list) };
    if r == 0 {
        list.num
    } else {
        0
    }
}
