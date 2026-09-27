//! 网页和 JSON API。

use std::sync::atomic::Ordering;
use std::time::Duration;

use esp_idf_svc::http::server::{Configuration, EspHttpConnection, EspHttpServer, Request};
use embedded_svc::http::Headers;
use esp_idf_svc::http::Method;
use esp_idf_svc::io::{Read, Write};
use esp_idf_svc::ota::EspOta;
use esp_idf_svc::sys;
use log::{info, warn};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::config::{Settings, Shared, Store, Strip, Wifi, CHANNELS};
use crate::hw::Voltage;
use crate::net::SharedStatus;

static INDEX_HTML: &str = include_str!("index.html");
const MAX_JSON: usize = 16 * 1024;

#[derive(Clone)]
pub struct Ctx {
    pub shared: Shared,
    pub store: Store,
    pub net: SharedStatus,
    pub voltage: Voltage,
}

type Req<'a, 'b> = Request<&'a mut EspHttpConnection<'b>>;

pub fn start(ctx: Ctx) -> anyhow::Result<EspHttpServer<'static>> {
    let mut server = EspHttpServer::new(&Configuration {
        stack_size: 12 * 1024,
        max_uri_handlers: 16,
        ..Default::default()
    })?;

    server.fn_handler::<anyhow::Error, _>("/", Method::Get, |req| {
        req.into_response(200, None, &[("Content-Type", "text/html; charset=utf-8")])?
            .write_all(INDEX_HTML.as_bytes())?;
        Ok(())
    })?;

    let c = ctx.clone();
    server.fn_handler::<anyhow::Error, _>("/api/state", Method::Get, move |req| {
        send_json(req, &state_json(&c))
    })?;

    // 修改灯带：{"ids":[0,3], "patch":{"on":true,"color":[255,0,0],...}}
    let c = ctx.clone();
    server.fn_handler::<anyhow::Error, _>("/api/strips", Method::Post, move |mut req| {
        #[derive(Deserialize)]
        struct Body {
            ids: Vec<usize>,
            patch: serde_json::Map<String, Value>,
        }
        let body: Body = read_json(&mut req)?;
        let result = c.shared.update(|s| -> anyhow::Result<()> {
            // 先全部验证再写入，避免一半成功一半失败
            let mut updated = Vec::new();
            for &i in &body.ids {
                let strip = s.strips.get(i).ok_or_else(|| anyhow::anyhow!("没有第 {i} 路"))?;
                updated.push((i, patch_strip(strip, &body.patch)?));
            }
            for (i, strip) in updated {
                s.strips[i] = strip;
            }
            s.sanitize();
            Ok(())
        });
        match result {
            Ok(()) => send_json(req, &state_json(&c)),
            Err(e) => send_error(req, 400, &e.to_string()),
        }
    })?;

    let c = ctx.clone();
    server.fn_handler::<anyhow::Error, _>("/api/settings", Method::Post, move |mut req| {
        #[derive(Deserialize)]
        struct Body {
            hostname: Option<String>,
            transition_ms: Option<u16>,
        }
        let body: Body = read_json(&mut req)?;
        c.shared.update(|s| {
            if let Some(h) = body.hostname {
                s.hostname = sanitize_hostname(&h);
            }
            if let Some(t) = body.transition_ms {
                s.transition_ms = t;
            }
            s.sanitize();
        });
        send_json(req, &state_json(&c))
    })?;

    // 保存 WiFi 并重启
    let c = ctx.clone();
    server.fn_handler::<anyhow::Error, _>("/api/wifi", Method::Post, move |mut req| {
        let wifi: Wifi = read_json(&mut req)?;
        if wifi.ssid.len() > 32 || wifi.password.len() > 64 {
            return send_error(req, 400, "SSID 最长 32 字节，密码最长 64 字节");
        }
        if !wifi.password.is_empty() && wifi.password.len() < 8 {
            return send_error(req, 400, "WPA 密码至少 8 位");
        }
        info!("收到新的 WiFi 设置：\"{}\"", wifi.ssid);
        let settings = c.shared.update(|s| {
            s.wifi = wifi;
            s.clone()
        });
        c.store.save(&settings)?;
        send_json(req, &json!({ "ok": true, "rebooting": true }))?;
        restart_later();
        Ok(())
    })?;

    let c = ctx.clone();
    server.fn_handler::<anyhow::Error, _>("/api/reboot", Method::Post, move |req| {
        c.store.save(&c.shared.get())?;
        send_json(req, &json!({ "ok": true }))?;
        restart_later();
        Ok(())
    })?;

    // 恢复默认设置（会清掉 WiFi）
    let c = ctx.clone();
    server.fn_handler::<anyhow::Error, _>("/api/factory-reset", Method::Post, move |req| {
        c.store.erase()?;
        send_json(req, &json!({ "ok": true }))?;
        restart_later();
        Ok(())
    })?;

    // 网页上传固件（espflash save-image 生成的 .bin）
    server.fn_handler::<anyhow::Error, _>("/api/ota", Method::Post, move |mut req| {
        match ota(&mut req) {
            Ok(len) => {
                info!("OTA 完成：{len} 字节，准备重启");
                send_json(req, &json!({ "ok": true, "bytes": len }))?;
                restart_later();
                Ok(())
            }
            Err(e) => {
                warn!("OTA 失败: {e}");
                send_error(req, 400, &format!("升级失败：{e}"))
            }
        }
    })?;

    info!("网页服务已启动");
    Ok(server)
}

fn ota(req: &mut Req) -> anyhow::Result<usize> {
    let total = req.content_len().unwrap_or(0) as usize;
    if total == 0 {
        anyhow::bail!("固件文件为空");
    }
    let mut ota = EspOta::new()?;
    let mut update = ota.initiate_update()?;
    let mut buf = vec![0u8; 4096];
    let mut done = 0;
    let result = (|| -> anyhow::Result<()> {
        while done < total {
            let n = req.read(&mut buf)?;
            if n == 0 {
                anyhow::bail!("上传中断（{done}/{total} 字节）");
            }
            update.write(&buf[..n])?;
            done += n;
        }
        Ok(())
    })();
    match result {
        Ok(()) => {
            update.complete()?;
            Ok(done)
        }
        Err(e) => {
            let _ = update.abort();
            Err(e)
        }
    }
}

/// 用 JSON 合并的方式修改一路灯带，类型不对会报错
fn patch_strip(strip: &Strip, patch: &serde_json::Map<String, Value>) -> anyhow::Result<Strip> {
    let mut v = serde_json::to_value(strip)?;
    let obj = v.as_object_mut().unwrap();
    for (k, val) in patch {
        if !obj.contains_key(k) {
            anyhow::bail!("未知字段 {k}");
        }
        obj.insert(k.clone(), val.clone());
    }
    Ok(serde_json::from_value(v)?)
}

fn state_json(c: &Ctx) -> Value {
    let s: Settings = c.shared.get();
    let net = c.net.lock().unwrap().clone();
    let channels: Vec<Value> = CHANNELS
        .iter()
        .map(|ch| json!({ "gpio": ch.gpio, "connector": ch.connector, "psram_pin": ch.shared_with_psram }))
        .collect();
    json!({
        "hostname": s.hostname,
        "transition_ms": s.transition_ms,
        "wifi_ssid": s.wifi.ssid,
        "strips": s.strips,
        "channels": channels,
        "net": net,
        "voltage_mv": c.voltage.load(Ordering::Relaxed),
        "uptime_s": unsafe { sys::esp_timer_get_time() } / 1_000_000,
        "free_heap": unsafe { sys::esp_get_free_heap_size() },
        "version": env!("CARGO_PKG_VERSION"),
    })
}

fn read_json<T: for<'de> Deserialize<'de>>(req: &mut Req) -> anyhow::Result<T> {
    let len = req.content_len().unwrap_or(0) as usize;
    if len == 0 || len > MAX_JSON {
        anyhow::bail!("请求体大小不对：{len}");
    }
    let mut buf = vec![0u8; len];
    req.read_exact(&mut buf)?;
    Ok(serde_json::from_slice(&buf)?)
}

fn send_json(req: Req, v: &Value) -> anyhow::Result<()> {
    req.into_response(200, None, &[("Content-Type", "application/json")])?
        .write_all(serde_json::to_string(v)?.as_bytes())?;
    Ok(())
}

fn send_error(req: Req, status: u16, msg: &str) -> anyhow::Result<()> {
    req.into_response(status, None, &[("Content-Type", "application/json")])?
        .write_all(json!({ "error": msg }).to_string().as_bytes())?;
    Ok(())
}

fn sanitize_hostname(h: &str) -> String {
    let s: String = h
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-')
        .take(31)
        .collect::<String>()
        .to_ascii_lowercase();
    if s.is_empty() {
        "ledctl".into()
    } else {
        s
    }
}

fn restart_later() {
    std::thread::spawn(|| {
        std::thread::sleep(Duration::from_millis(800));
        unsafe { sys::esp_restart() };
    });
}
