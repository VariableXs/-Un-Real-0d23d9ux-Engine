//! VarixAutoPilot2 · Tauri 主进程
//!
//! 职责：把 CDP 连接、采集、引擎、模板串起来，通过 Tauri command 暴露给前端。
//!
//! 设计取舍：
//! - **CDP 连接常驻**（`Arc<Cdp>`）。每次快照重连会让端口上的会话句柄堆积；
//!   断线时惰性重连一次，不循环。
//!   ★ 为什么用 Arc 而不是直接放 Mutex<Cdp> ★
//!   `State<T>` 的生命周期不跨 `.await`——如果把 `MutexGuard` 拿在手里跨越
//!   await 点，编译器直接拒绝（guard 不 Send）。所以内部必须是
//!   `Mutex<Option<Arc<Cdp>>>`：**只在锁内clone Arc，锁立刻释放**，
//!   然后用Arc 独立发请求。早先我在这里写了个 `unreachable!()` 占位想绕过去，
//!   那是把设计问题藏成崩溃，必须改。
//! - **异常零静默**：每个错误都带「发生了什么 / 为什么 / 下一步怎么办」三要素，
//!   前端直接展示，不让异常裸奔到 UI。

// ★ pub 而非私有★：examples/ 下的集成实测（live_probe.rs）要用到它们。
// Rust的 module 私有性是"crate 内可见"，但 example 是**独立 crate**，
// 私有模块对它不可见⇒ 不改 pub 的话 examples/ 根本编不过。
pub mod cdp;
pub mod collect;
pub mod engine;
pub mod template;

use std::sync::Arc;
use std::sync::Mutex;
use tokio::sync::Mutex as AsyncMutex;

use serde::{Deserialize, Serialize};
use tauri::State;

use cdp::Cdp;
use collect::{CwdCache, Snapshot};

/// 全局应用状态。
pub struct AppState {
    /// 常驻 CDP 连接。`None` = 未连接/已断开。
    cdp: Mutex<Option<Arc<Cdp>>>,
    /// cwd 缓存（跨快照复用，避免每轮重读上百个会话文件）
    cache: Arc<AsyncMutex<CwdCache>>,
}

impl Default for AppState {
    fn default() -> Self {
        AppState {
            cdp: Mutex::new(None),
            cache: Arc::new(AsyncMutex::new(CwdCache::new())),
        }
    }
}

/// 取缓存的独立 Arc。**锁只用来 clone，跨 await 前必须释放。**
fn cache_arc(state: &State<'_, AppState>) -> Result<Arc<AsyncMutex<CwdCache>>, ErrPayload> {
    Ok(state.cache.clone())
}

/// 统一的错误载荷：三要素齐全，前端直接渲染。
#[derive(Debug, Serialize)]
pub struct ErrPayload {
    /// 发生了什么
    pub what: String,
    /// 为什么
    pub why: String,
    /// 下一步怎么办
    pub next: String,
    /// 是否为「忙时拒绝」（前端可用不同色）
    pub busy: bool,
}

impl ErrPayload {
    fn new(what: &str, why: &str, next: &str) -> Self {
        ErrPayload {
            what: what.to_string(),
            why: why.to_string(),
            next: next.to_string(),
            busy: false,
        }
    }
    fn busy_err(what: &str, next: &str) -> Self {
        ErrPayload {
            what: what.to_string(),
            why: "对方正在生成，发送键语义是「停止」，此时发送会打断它".to_string(),
            next: next.to_string(),
            busy: true,
        }
    }
}

/// 内部锁辅助：把 anyhow / String 错误统一转成三要素载荷。
fn to_payload(e: anyhow::Error, what: &str) -> ErrPayload {
    // 从 anyhow 的因果链里挖出最有用的那一段
    let why = e
        .chain()
        .map(|c| c.to_string())
        .collect::<Vec<_>>()
        .join(" ← ");
    ErrPayload::new(what, &why, "看autopilot.log 最后几行；若选择器失效，核对 WorkBuddy 是否升级过")
}

/// 取 CDP 连接，必要时建立。
///
/// ★ 锁只用来 clone Arc，立刻释放 ★
/// 跨 await 持有 `MutexGuard` 会编译失败（guard 非 Send）。
/// 所以：进锁 → clone Arc → 出锁 → 用 Arc 发请求。
async fn ensure_cdp(state: &State<'_, AppState>) -> Result<Arc<Cdp>, ErrPayload> {
    {
        let g = state
            .cdp
            .lock()
            .map_err(|_| ErrPayload::new("内部状态锁被毒化", "上一轮 panic 残留", "重启软件"))?;
        if let Some(c) = g.as_ref() {
            return Ok(c.clone());
        }
    }
    match Cdp::connect(cdp::CDP_ENDPOINT).await {
        Ok(c) => {
            let arc = Arc::new(c);
            let mut g = state
                .cdp
                .lock()
                .map_err(|_| ErrPayload::new("内部状态锁被毒化", "上一轮 panic 残留", "重启软件"))?;
            // 双检：可能并发时已有人连上了，此时复用先到的那条
            if let Some(existing) = g.as_ref() {
                return Ok(existing.clone());
            }
            *g = Some(arc.clone());
            log::info!("CDP 已连接 {}", cdp::CDP_ENDPOINT);
            Ok(arc)
        }
        Err(e) => Err(ErrPayload::new(
            &format!("连不上 WorkBuddy 的调试端口 {}", cdp::CDP_ENDPOINT),
            &format!("{e:#}"),
            "确认 WorkBuddy 以 --remote-debugging-port=9222 启动；\
             且必须先彻底退出旧实例（参数对已运行实例不生效）",
        )),
    }
}

/// 丢弃当前连接（断线时用）。
async fn drop_cdp(state: &State<'_, AppState>) {
    if let Ok(mut g) = state.cdp.lock() {
        *g = None;
    }
}

// ---------------------------------------------------------------------------
// Commands
// ---------------------------------------------------------------------------

/// 采一份快照。断线时自动重连一次（一次性，不循环）。
#[tauri::command]
async fn probe(state: State<'_, AppState>) -> Result<Snapshot, ErrPayload> {
    let cache = cache_arc(&state)?;
    let t0 = std::time::Instant::now();

    // 第一轮：用现有连接
    let cdp = ensure_cdp(&state).await?;
    match try_probe(&cdp, &cache).await {
        Ok(s) => {
            log::debug!("快照完成，用时 {}ms", t0.elapsed().as_millis());
            return Ok(s);
        }
        Err(first) => log::warn!("快照失败，丢弃连接后重试一次：{first:#}"),
    }

    // 第二轮：重连后再试（一次性，不循环）
    drop_cdp(&state).await;
    let cdp = ensure_cdp(&state).await?;
    try_probe(&cdp, &cache)
        .await
        .map_err(|e| to_payload(e, "采集对话信息失败"))
}

/// 采集一次。锁在函数内获取，**在 await 前释放**——
/// 否则 MutexGuard 跨 await 会让整个 future 变成非 Send，Tauri 直接编译不过。
async fn try_probe(
    cdp: &Cdp,
    cache: &Arc<AsyncMutex<CwdCache>>,
) -> Result<Snapshot, anyhow::Error> {
    // 缓存刷新是同步 IO，很快，单独加锁；随后释放再做 CDP 的网络 await
    // tokio 的 Mutex::lock() 是 async（它的 guard 是 Send，能跨 await）
    let idx = {
        let mut guard = cache.lock().await;
        guard.refresh()
        // guard 在此 drop，之后才做网络 await —— 避免持锁跨越挂起点
    };
    collect::probe_with_index(cdp, idx).await
}

/// 前端保存的设置。
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Settings {
    pub template: String,
    pub vars: serde_json::Value,
    pub open_new: bool,
    pub dry_run: bool,
}

/// 预览渲染结果。
#[derive(Debug, Serialize)]
pub struct PreviewOut {
    pub text: String,
    pub missing: Vec<String>,
    pub chars: usize,
}

/// 预览（不填不发）。
#[tauri::command]
async fn preview(tpl: String, vars: serde_json::Value) -> PreviewOut {
    let r = template::render(&tpl, &vars);
    PreviewOut {
        chars: r.text.chars().count(),
        text: r.text,
        missing: r.missing,
    }
}

/// 执行结果（带回三要素错误）。
#[derive(Debug, Serialize)]
pub struct SendOut {
    pub ok: bool,
    pub dry_run: bool,
    pub chars: usize,
    pub evidence: String,
    pub err: Option<ErrPayload>,
}

/// 填入（+ 可选发送）。
#[tauri::command]
async fn send(
    state: State<'_, AppState>,
    tpl: String,
    vars: serde_json::Value,
    dry_run: bool,
    open_new: bool,
) -> Result<SendOut, ErrPayload> {
    let r = template::render(&tpl, &vars);
    if !r.missing.is_empty() {
        return Ok(SendOut {
            ok: false,
            dry_run,
            chars: 0,
            evidence: String::new(),
            err: Some(ErrPayload::new(
                &format!("有 {} 个占位符没填，拒绝发送", r.missing.len()),
                &r.missing
                    .iter()
                    .map(|k| format!("{{{{{k}}}}}"))
                    .collect::<Vec<_>>()
                    .join("、"),
                "在面板「变量」区填好，或把模板里那些占位符删掉",
            )),
        });
    }

    let cdp = match ensure_cdp(&state).await {
        Ok(c) => c,
        Err(e) => {
            return Ok(SendOut {
                ok: false,
                dry_run,
                chars: 0,
                evidence: String::new(),
                err: Some(e),
            });
        }
    };

    match engine::run_flow(&cdp, &r.text, dry_run, open_new).await {
        Ok(f) => {
            // evidence 被 move 进结构体前先留一份引用给 err 用
            let ev = f.evidence.clone();
            Ok(SendOut {
                ok: f.ok,
                dry_run: f.dry_run,
                chars: f.chars,
                evidence: ev.clone(),
                err: if f.ok {
                    None
                } else {
                    Some(ErrPayload::new(
                        "8 秒内没能确认发送成功",
                        &ev,
                        "看面板截图确认界面状态；若是按钮选择器失效，核对 WorkBuddy 是否升级过",
                    ))
                },
            })
        }
        Err(engine::EngineError::Busy(m)) => Ok(SendOut {
            ok: false,
            dry_run,
            chars: 0,
            evidence: String::new(),
            err: Some(ErrPayload::busy_err(&m, "等对方空闲后再点发送；本轮未写入任何内容")),
        }),
        Err(engine::EngineError::Other(e)) => Ok(SendOut {
            ok: false,
            dry_run,
            chars: 0,
            evidence: String::new(),
            err: Some(to_payload(e, "执行失败")),
        }),
    }
}

/// 只查忙闲（前端轮询用，比全量快照轻）。
#[tauri::command]
async fn busy(state: State<'_, AppState>) -> Result<serde_json::Value, ErrPayload> {
    let cdp = ensure_cdp(&state).await?;
    engine::busy_state(&cdp)
        .await
        .map(|b| serde_json::json!({ "sending": b.sending, "label": b.label }))
        .map_err(|e| to_payload(e, "读发送键状态失败"))
}

/// 应用入口。
#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .manage(AppState::default())
        .invoke_handler(tauri::generate_handler![probe, preview, send, busy])
        .run(tauri::generate_context!())
        .expect("启动失败");
}
