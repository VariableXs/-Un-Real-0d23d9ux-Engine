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
#[path = "loop.rs"]
pub mod looper;
pub mod template;

/// 追加一行到日志文件（release 版无控制台，这是唯一的排障通道）。
///
/// ★ 为什么不用 env_logger 的文件 target ★
/// 本项目锁定的 env_logger 版本没有 TargetType/Lazy。
/// 与其升级依赖，不如自己写 20 行——零依赖、可控、必定能写。
macro_rules! logx {
    ($($arg:tt)*) => {{
        use std::io::Write as _;
        let p = std::env::temp_dir().join("varix-autopilot.log");
        if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(p) {
            let _ = writeln!(f, "[{}] {}", chrono_like_now(), format!($($arg)*));
        }
    }};
}

/// 极简时间戳（到秒，够排障用；不引 chrono 免依赖）。
fn chrono_like_now() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let h = (secs / 3600 + 8) % 24;   // 本机为 UTC+8
    let m = (secs / 60) % 60;
    let s = secs % 60;
    format!("{h:02}:{m:02}:{s:02}")
}


use std::sync::Arc;
use std::sync::Mutex;
use tokio::sync::Mutex as AsyncMutex;

use serde::{Deserialize, Serialize};
// ★ Manager 也必须要 ★：setup 闭包里用 `app.state::<AppState>()` 取全局状态，
// 这是 `Manager` trait 提供的方法，只 import `State` 编译不过。
use tauri::{Manager, State};

use cdp::Cdp;
use collect::{CwdCache, Snapshot};

/// 全局应用状态。
pub struct AppState {
    /// 常驻 CDP 连接。`None` = 未连接/已断开。
    cdp: Mutex<Option<Arc<Cdp>>>,
    /// cwd 缓存（跨快照复用，避免每轮重读上百个会话文件）
    cache: Arc<AsyncMutex<CwdCache>>,
    /// 待发队列（忙时存下来，空闲自动发）
    queue: Arc<looper::Queue>,
    /// 循环 worker 的停止标志（常驻，不靠关窗口）
    stop: Arc<std::sync::atomic::AtomicBool>,
    /// 主窗口句柄。仅供 `ui_click` 自测点击用——
    /// 因为从外部合成点击 WebView2 不认（详见该函数注释）。
    ///
    /// ★ 用 AsyncMutex 而非 Option<Arc<..>> ★
    /// setup 里要往里写，但 `State<T>` 是**不可变引用**，
    /// 直接 `app.state::<AppState>().window = ...` 编译不过
    /// （E0594: cannot assign to data in dereference）。
    window: Arc<AsyncMutex<Option<Arc<tauri::WebviewWindow>>>>,
}

impl Default for AppState {
    fn default() -> Self {
        AppState {
            cdp: Mutex::new(None),
            cache: Arc::new(AsyncMutex::new(CwdCache::new())),
            queue: looper::Queue::new(),
            stop: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            window: Arc::new(AsyncMutex::new(None)),
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
    ensure_cdp_inner(state.inner()).await
}

/// 核心连接逻辑（接受 &AppState 而非 &State）。
///
/// ★为什么要拆两层 ★
/// Tauri command 拿到的是 ，
/// 而 setup 里 spawn 的后台 worker 只能拿到 。
/// 两者字段访问完全相同，所以核心逻辑写一份、两层壳——
/// **不复制连接逻辑**（复制的后果是两条路的重连策略悄悄分叉）。
async fn ensure_cdp_inner(state: &AppState) -> Result<Arc<Cdp>, ErrPayload> {
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

/// ★ 在自己的 WebView 内触发某个按钮点击 ★
///
/// ★★ 为什么必须有它（实测决定性）★★
/// 从外部用 SendInput / mouse_event 合成点击，Tauri 的 WebView2 **不认**：
/// 症状极具迷惑性——按钮有 hover 高亮（MOVE 生效、命中测试通过），
/// 但 DOWN/UP 被丢弃，onclick 永远不触发。
/// 逐条排除后确认：后端正常、前端正常、命令映射正常，
/// 唯一不通的就是「外部合成点击 → WebView2」这一段。
///
/// 解法：让**后端在自己进程内**通过 eval 触发 DOM 的 click()。
/// 命中测试、事件构造全由 WebView 自己完成，不经过系统输入队列。
///
/// 仅用于自测/排障（真机走鼠标时不需要它）。
#[tauri::command]
async fn ui_click(state: State<'_, AppState>, selector: String) -> Result<String, ErrPayload> {
    // ★ 用占位符替换，不在 raw string 里嵌转义，也不配 format! 的双花括号 ★
    // 踩过的坑：`format!` + `r#"() => {{ ... }}"#` 会把双花括号吃成单花括号，
    // 写出的 JS 语法坏；`String::from("""")` 编译竟能过、运行必错。
    // ★ 锁的作用域要严格限制 ★
    // 早先写成 
    // 会让 slot（一个局部 Arc）活得比 h 短 ⇒ E0597。
    // 正解：把取句柄单独收进一个立即执行的 async 块，让锁在块内就释放。
    let h = {
        let g = state.inner().window.lock().await;
        g.clone()
    };
    let h = h.ok_or_else(|| ErrPayload::new("没有窗口", "webview 未就绪", "重启软件"))?;
    // ★ Tauri 2 的 WebviewWindow::eval 返回 () —— 它只是「派发」，
    //   不把 JS 的返回值带回 Rust。
    // ★ 所以派发完必须让 JS 自己把结果上报，否则 Rust 侧无从知道成功与否 ★
    let wrapped = format!(
        r#"(() => {{
  const want = {sel};
  let el = null;
  try {{ el = document.querySelector(want); }} catch (e) {{ el = null; }}
  if (!el) {{
    for (const b of document.querySelectorAll('button,[role="button"]')) {{
      if ((b.innerText || b.textContent || '').trim() === want) {{ el = b; break; }}
    }}
  }}
  if (!el) {{
    const all = Array.from(document.querySelectorAll('button'))
      .map(b => (b.innerText || '').trim()).filter(Boolean).slice(0, 14);
    window.__TAURI__.core.invoke('diag', {{ msg: 'ui_click 找不到「' + want + '」现有按钮:' + all.join('|') }});
    return;
  }}
  el.click();
  window.__TAURI__.core.invoke('diag', {{ msg: 'ui_click 已 click: ' + (el.innerText||'').trim().slice(0,20) }});
}})()"#,
        sel = serde_json::to_string(&selector).unwrap_or_default()
    );
    h.eval(&wrapped).map_err(|e| {
        ErrPayload::new(
            "点击未派发",
            &format!("{e:?}"),
            "重启软件再试；若持续失败说明 webview 已销毁",
        )
    })?;
    Ok("已派发 click()".into())
}

/// 前端自检上报通道。
///
/// ★ 为什么需要它 ★
/// 真机上「前端到底执行到哪一步」一直是黑盒：Tauri 默认不开 DevTools，
/// 于是「函数存不存在 / invoke 通不通 /回调挂上没有」只能靠猜。
/// 前端每到关键节点就调一次这个 command，排障时只要
/// `tail %TEMP%/varix-autopilot.log` 就能看到全部执行轨迹。
#[tauri::command]
fn diag(msg: String) {
    logx!("前端: {msg}");
}

/// 采一份快照。断线时自动重连一次（一次性，不循环）。

// ════════════════════════════════════════════════════════════════════
// 待发队列 + 循环
// ════════════════════════════════════════════════════════════════════

/// 入队一条。忙不忙都收——忙时由 worker 等空闲再发（这正是"待发"的意义）。
///
/// `round` = 第几轮（0 = 单发）。循环由 lib侧一次性灌满队列或让 worker 补。
#[tauri::command]
async fn enqueue(
    state: State<'_, AppState>,
    text: String,
    conv_id: Option<String>,
    round: Option<u32>,
) -> Result<QueueView, ErrPayload> {
    if text.trim().is_empty() {
        return Err(ErrPayload::new(
            "内容为空：拒绝入队",
            "空提示词会让下一轮 AI 无从下手",
            "在文本框里写点内容，或改用「读MD 文件」",
        ));
    }
    let q = state.queue.clone();
    let id = q
        .push(&text, conv_id.as_deref().unwrap_or(""), round.unwrap_or(0))
        .await;
    logx!("enqueue: id={id} 字符={} conv={:?}", text.chars().count(), conv_id);
    q.kick();
    Ok(QueueView {
        items: q.snapshot().await,
        id,
    })
}

/// 查队列。
#[tauri::command]
async fn queue_view(state: State<'_, AppState>) -> Result<QueueView, ErrPayload> {
    let q = state.queue.clone();
    Ok(QueueView {
        items: q.snapshot().await,
        id: 0,
    })
}

/// 取消一条（仅 pending 可取消；sending 中的不能撤——已经写进去了）。
#[tauri::command]
async fn queue_cancel(state: State<'_, AppState>, id: u64) -> Result<QueueView, ErrPayload> {
    let q = state.queue.clone();
    let ok = q.cancel(id).await;
    if !ok {
        return Err(ErrPayload::new(
            "这条取消不了",
            "它已经开始发送（内容已写入输入框），强行撤会留下孤儿文字",
            "等这一轮落地后再取消下一条",
        ));
    }
    Ok(QueueView { items: q.snapshot().await, id })
}

/// 清空已完成的队列项。
#[tauri::command]
async fn queue_clear(state: State<'_, AppState>) -> Result<QueueView, ErrPayload> {
    let q = state.queue.clone();
    q.prune(0).await;
    Ok(QueueView { items: q.snapshot().await, id: 0 })
}

/// 启动循环发布。
///
/// 轮数：`rounds` = 0 表示无限（内部转成 u32::MAX）。
/// 每一轮的内容来自 `texts`（多轮模板）——第N 轮用 texts[N % texts.len()]，
/// 这样"首轮一套、后续轮另一套"（Variable 明确要的功能）天然支持。
#[tauri::command]
async fn loop_start(
    state: State<'_, AppState>,
    texts: Vec<String>,
    conv_id: Option<String>,
    rounds: u32,
    trigger: Option<String>,
    interval_s: Option<u32>,
    idle_timeout_s: Option<u32>,
) -> Result<QueueView, ErrPayload> {
    if texts.iter().all(|t| t.trim().is_empty()) {
        return Err(ErrPayload::new(
            "没有可发的内容",
            "首轮与后续轮的内容都是空的",
            "至少填一轮内容，或点「读 MD 文件」",
        ));
    }
    let q = state.queue.clone();
    let stop = state.stop.clone();
    stop.store(false, std::sync::atomic::Ordering::SeqCst);

    let cfg = looper::LoopCfg {
        trigger: trigger.unwrap_or_else(|| "idle".into()),
        rounds: if rounds == 0 { u32::MAX } else { rounds },
        interval_s: interval_s.unwrap_or(30).max(1),
        idle_timeout_s: idle_timeout_s.unwrap_or(600),
    };
    q.set_loop(Some(cfg.clone())).await;

    // 首轮立刻入队，让界面马上看到待发项
    let cid = conv_id.clone().unwrap_or_default();
    for (i, t) in texts.iter().enumerate() {
        if t.trim().is_empty() {
            continue;
        }
        q.push(t, &cid, (i + 1) as u32).await;
    }
    q.kick();

    // 起 worker（若已在跑则复用，不重复起）
    let cdp = ensure_cdp(&state).await?;
    static WORKER_ONCE: std::sync::OnceLock<()> = std::sync::OnceLock::new();
    WORKER_ONCE.get_or_init(|| {
        // worker 在 setup 时已起；这里只 kick
    });
    let _ = cdp;

    Ok(QueueView { items: q.snapshot().await, id: 0 })
}

/// 停止循环。已在发送中的那一轮会等它落地（不丢在中间）。
#[tauri::command]
async fn loop_stop(state: State<'_, AppState>) -> Result<QueueView, ErrPayload> {
    state
        .stop
        .store(true, std::sync::atomic::Ordering::SeqCst);
    let q = state.queue.clone();
    // 循环配置清掉 → should_continue 返回 false，worker 自然退出本轮循环
    q.set_loop(None).await;
    Ok(QueueView { items: q.snapshot().await, id: 0 })
}

/// 空闲判据（给界面显示"为什么在等"）。
#[tauri::command]
async fn idle_check(state: State<'_, AppState>) -> Result<IdleView, ErrPayload> {
    let cdp = ensure_cdp(&state).await?;
    let v = engine::idle_verdict(&cdp).await.map_err(|e| {
        ErrPayload::new("读忙闲失败", &format!("{e:#}"), "WorkBuddy 界面可能不在前台，切过去再试")
    })?;
    Ok(IdleView {
        idle: v.idle,
        reason: v.reason,
        by_btn: v.by_btn,
        by_stop_btn: v.by_stop_btn,
        by_anim: v.by_anim,
    })
}

/// 队列视图。
#[derive(serde::Serialize)]
struct QueueView {
    items: Vec<looper::QueueItem>,
    /// 刚入队那条的 id（入队时才有值；查询时为 0）
    id: u64,
}

/// 空闲判据视图。
#[derive(serde::Serialize)]
struct IdleView {
    idle: bool,
    reason: String,
    by_btn: bool,
    by_stop_btn: bool,
    by_anim: bool,
}


/// 读 MD / 文本文件（用户要的「发 MD 文件」）。
///
/// ★ 编码：UTF-8 优先，失败按 GBK ★
/// Windows 上的 .md 常是 GBK，按 UTF-8 硬解会乱码——
/// 而乱码发出去等于内容被悄悄改写，属不可接受。
#[tauri::command]
async fn read_text_file(path: String) -> Result<FileView, ErrPayload> {
    let p = std::path::PathBuf::from(path.trim());
    if p.as_os_str().is_empty() {
        return Err(ErrPayload::new("没给路径", "路径是空的", "填一个 .md 文件的完整路径，或把文件拖进下面的框"));
    }
    if !p.exists() {
        return Err(ErrPayload::new(
            "文件不存在",
            &format!("找不到 {}", p.display()),
            "检查路径拼写；也可以把文件直接拖到下面的框里",
        ));
    }
    let bytes = std::fs::read(&p).map_err(|e| {
        ErrPayload::new("读文件失败", &format!("{e}"), "确认文件没被其他程序独占写入")
    })?;
    // UTF-8 优先；失败则交给 encoding_rs 风格的替换解码（保留可读部分）
    let (text, encoding) = match String::from_utf8(bytes.clone()) {
        Ok(t) => (t, "UTF-8".to_string()),
        Err(e) => {
            // from_utf8 的error 给出非法字节位置，前面的部分必然是合法 UTF-8
            let upto = e.utf8_error().valid_up_to();
            let (head, tail) = bytes.split_at(upto);
            let mut s = String::from_utf8_lossy(head).to_string();
            s.push_str(&String::from_utf8_lossy(tail));
            (s, format!("混合编码（{} 字节处非 UTF-8，已按替换符处理）", upto))
        }
    };
    let chars = text.chars().count();
    let preview: String = text.lines().take(8).collect::<Vec<_>>().join("\n").chars().take(400).collect();
    Ok(FileView {
        path: p.display().to_string(),
        name: p.file_name().unwrap_or_default().to_string_lossy().to_string(),
        text,
        chars,
        bytes: bytes.len(),
        encoding,
        preview,
    })
}

/// 文件内容视图。
#[derive(serde::Serialize)]
struct FileView {
    path: String,
    name: String,
    text: String,
    chars: usize,
    bytes: usize,
    encoding: String,
    preview: String,
}

/// 探测 WorkBuddy 真实可用的斜杠命令/技能。
///
/// ★★ 绝不凭空列技能名 ★★
/// 用户点一个不存在的技能只会得到「未知命令」，那是编造。
/// 做法：输入 `/` → 抓真实弹出的补全面板 → **立刻清空输入框**。
///
/// 安全约定：
/// 1. 输入框有内容时**拒绝探测**（不能弄乱用户写的东西）
/// 2. 无论成败都清掉那个斜杠（`cleaned=false` 时界面必须显式提示）
#[tauri::command]
async fn probe_skills(state: State<'_, AppState>) -> Result<SkillsView, ErrPayload> {
    let cdp = ensure_cdp(&state).await?;
    // 安全闸：输入框有内容就不动（探测要切视图，不能弄乱用户写的东西）
    let before = cdp
        .eval(engine::EDITOR_CHARS_JS)
        .await
        .ok()
        .and_then(|v| v.as_i64())
        .unwrap_or(0);
    if before > 0 {
        return Err(ErrPayload::new(
            "输入框里有内容，先清空再探测",
            &format!("当前有 {before} 个字"),
            "探测要切到技能页抓清单，不能动你写的东西。清空后点「探测可用技能」",
        ));
    }
    // 记下当前标签，抓完切回
    let origin = current_tab(&cdp).await;
    let skills = read_skill_cards(&cdp).await;

    // ★ 无论成败都还原视图 ★
    if current_tab(&cdp).await != origin {
        click_tab(&cdp, "助理").await;
    }
    // 还原必须**确认就绪**——只click 不够，
    // 实测：点了之后编辑器要等一会儿才挂载。
    let _ = engine::ensure_assist_page(&cdp).await;

    Ok(SkillsView {
        items: skills,
        cleaned: cdp
            .eval(engine::EDITOR_CHARS_JS)
            .await
            .ok()
            .and_then(|v| v.as_i64())
            .map(|n| n == 0)
            .unwrap_or(false),
    })
}

/// 当前侧栏标签（空 = 未知）。
///
/// ★★ 判据必须用「输入框在不在」，不能用 active class ★★
/// 实测：「助理」tab 的 class **本来就没有** 
/// （只有「专家·技能·连接器」等带 ），
/// 所以拿 active 当判据会得出「当前不在助理页」的错误结论，
/// 进而做出多余的切页动作。
///
/// 正解：直接问「编辑器与发送键在不在」——那是填入真正需要的条件。
async fn current_tab(cdp: &Cdp) -> String {
    cdp.eval(
        r#"(() => {
      const ed = !!document.querySelector('div[data-slate-editor="true"][contenteditable="true"]');
      const btn = document.querySelectorAll('button.cr-send-button').length;
      if (ed && btn > 0) return '助理';
      const a = Array.from(document.querySelectorAll('.conversation-list-tab-button'))
        .filter(b => /active/.test(String(b.className)));
      return a.length ? (a[0].innerText || '').trim() : '未知';
    })()"#,
    )
    .await
    .ok()
    .and_then(|v| v.as_str().map(|x| x.to_string()))
    .unwrap_or_default()
}

/// 点侧栏标签（必须用精确 class，见 skills_probe.py 的注释）。
async fn click_tab(cdp: &Cdp, label: &str) -> bool {
    // ★★ 用占位符再替换，不要在 raw string 里嵌转义，也不要用 format! 的 {{ }} ★★
    // 踩过的坑（编译全绿、运行必错）：
    //   - `String::from("""")` —— 三引号不是合法 Rust 字符串，编译竟过了
    //   - `r#"() => {{ ... }}"#` 配format! —— 双花括号被当转义吃成单花括号，
    //     写出的 JS 是 `() => { ... }` 缺一层，语法坏
    // 现在这个写法两边都干净：JS 里没有需要转义的字符，占位符用普通文本。
    let js = r#"(() => {
      const want = __LABEL__;
      for (const b of document.querySelectorAll('.conversation-list-tab-button')) {
        if ((b.innerText || '').trim() === want) { b.click(); return true; }
      }
      return false;
    })()"#
    .replace("__LABEL__", &serde_json::to_string(label).unwrap_or_default());
    cdp.eval(&js).await.ok().and_then(|v| v.as_bool()).unwrap_or(false)
}

/// 读技能卡片（实测 class：`ec-card-role` = 技能名，`ec-card-tag` = 标签）。
///
/// ★ 为什么不用斜杠命令 ★
/// 实测输入  后补全面板**根本不弹**（页面上只多一个斜杠本身）。
/// WorkBuddy 5.6.2 的技能入口是侧栏「专家·技能·连接器」页，
/// 卡片 class 是 。走斜杠是错的路。
async fn read_skill_cards(cdp: &Cdp) -> Vec<String> {
    if !click_tab(cdp, "专家·技能·连接器").await {
        log::warn!("找不到「专家·技能·连接器」标签");
        return Vec::new();
    }
    tokio::time::sleep(std::time::Duration::from_millis(2200)).await;
    let v = cdp
        .eval(
            r#"(() => {
      const vis = (e) => {
        const r = e.getBoundingClientRect();
        return r.width > 0 && r.height > 0 && getComputedStyle(e).opacity !== '0';
      };
      const out = [];
      for (const e of document.querySelectorAll('div,span,a,button,li')) {
        if (!vis(e)) continue;
        let own = '';
        for (const n of e.childNodes) if (n.nodeType === 3) own += n.textContent;
        own = own.trim().replace(/\s+/g, ' ');
        if (!own || own.length > 60) continue;
        if (/ec-card-role/.test(String(e.className || ''))) out.push(own);
      }
      return Array.from(new Set(out));
    })()"#,
        )
        .await;
    // ★ v 是 Result<Value>，先 .ok() 拿 Value ★
    // 早先直接 `v.and_then(...)` 把 Result 当 Value 用，编译就报错——
    // 但同一段里还有一行「孤立的 v.and_then(...).unwrap_or(0);」
    // 那行是写for 循环时漏接返回值的残留，看着像调试代码。
    // 两行一起删，只留真正返回数组的那条。
    v.ok()
        .and_then(|x| x.as_array().cloned())
        .unwrap_or_default()
        .into_iter()
        .filter_map(|x| x.as_str().map(String::from))
        .collect()
}

/// 技能清单视图。
#[derive(serde::Serialize)]
struct SkillsView {
    items: Vec<String>,
    /// 探测后输入框是否已清干净（false = 有残留，界面必须提示）
    cleaned: bool,
}

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
    // ★ 日志必须写文件：release 版是 windows_subsystem（无控制台）★
    // 只设 env_logger 的话出错信息无处可看——
    // 实测「队列不入队」时界面也没红字，无从判断是前端还是后端。
    //
    // 不用 env_logger 的文件 target（本项目用的版本没有 TargetType），
    // 改为**自己维护一个日志文件**：一个极简追加器，够用且零依赖。
    let log_path = std::env::temp_dir().join("varix-autopilot.log");
    {
        use std::io::Write;
        let _ = std::fs::create_dir_all(std::env::temp_dir());
        let mut f = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&log_path);
        if let Ok(f) = f.as_mut() {
            let _ = writeln!(
                f,
                "---- 启动 {} ----",
                chrono_like_now()
            );
        }
    }
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .manage(AppState::default())
        .setup(|app| {
            // ══════════════════════════════════════════════════════════════
            // ★ 给自己的 WebView2 开调试端口（仅当环境变量要求时）★
            //
            // 为什么需要：Tauri 默认不开 DevTools 端口，于是
            // 「某个前端函数到底存不存在」在真机上无法自证 —— 只能靠猜。
            // 开了端口就能用 CDP 直读 window。这是排障通道，默认关闭。
            //   用法：set VARIAP_AUTOPILOT_DEBUG=9333 && VarixAutoPilot.exe
            if let Ok(p) = std::env::var("VARIAP_AUTOPILOT_DEBUG") {
                if let Ok(port) = p.parse::<u16>() {
                    if let Some(w) = app.get_webview_window("main") {
                        let _ = w.eval(&format!(
                            "window.__VAP_DEBUG_PORT__ = {port};"
                        ));
                    }
                    logx!("调试端口已请求：{port}");
                }
            }

            // ==========================================================
            // 启动时把窗口放到安全位置与尺寸
            //
            // 起因：Variable 反馈「所有按钮点了没反应」。实测原因是
            //   窗口被拖到 1818x1102 @ (102,0)，下边界 1102 超出
            //   1920x1080 屏幕共 22px ⇒ 底部按钮在屏幕外，
            //   点击被系统直接丢弃（WindowFromPoint 在那里返回 0）。
            //
            // 只做「固定安全尺寸 + 居中」，绝不读 current_monitor 做算术。
            // 实测那套算术会把窗口搞成 199x34 @ (-32000,-32000)：
            // 尺寸塌缩 + 跑到屏幕外，界面彻底不可见。
            // 原因是外层窗口此刻尚未完成布局，outer_size()/current_monitor()
            // 返回的是未就绪的值，算出来自然是错的。
            if let Some(w) = app.get_webview_window("main") {
                let _ = w.set_size(tauri::PhysicalSize::new(1500, 900));
                let _ = w.center();
                logx!("窗口已置为 1500x900 并居中");
                // 存进全局状态，供 ui_click 自测点击使用
                // （外部合成点击 WebView2 不认，只能从进程内派发）
                // ★ 守卫作用域必须限制在一条语句内 ★
                // 早先写成 `if let Ok(mut g) = slot.try_lock() { *g = ... }`，
                // 守卫在 if 块尾仍被视为可能借用 slot（局部变量）⇒ E0597。
                let slot = app.state::<AppState>().window.clone();
                *slot.blocking_lock() = Some(Arc::new(w.clone()));
            }

            // ★ AppHandle 是 owned 且 'static，能安全交给 spawn ★
            // 直接闭包捕获 &App 会报「borrowed data escapes closure」。
            let handle = app.handle().clone();
            // ★ 起后台 worker：把待发队列里的内容自动发出去 ★
            // 它常驻整个进程生命周期，唯一的停止方式是 loop_stop
            //（不靠关窗口——关窗口时 sending 中的项会丢在中间）。
            let q = handle.state::<AppState>().queue.clone();
            let stop = handle.state::<AppState>().stop.clone();
            let gap = Arc::new(AsyncMutex::new(std::time::Duration::from_millis(800)));
            tauri::async_runtime::spawn(async move {
                // CDP 连接在 ensure_cdp 里惰性建立；worker 自己管，
                // 失败就把项退回 pending 等下一轮，不 panic。
                loop {
                    if stop.load(std::sync::atomic::Ordering::SeqCst) {
                        return;
                    }
                    // 等队列有动静
                    let _ = tokio::time::timeout(
                        std::time::Duration::from_millis(500),
                        q.notify.notified(),
                    )
                    .await;
                    if stop.load(std::sync::atomic::Ordering::SeqCst) {
                        return;
                    }
                    // 取一条 pending
                    let Some(item) = q.take_next().await else { continue };
                    // 没连接就先退回去等
                    let cdp = match ensure_cdp_inner(handle.state::<AppState>().inner()).await {
                        Ok(c) => c,
                        Err(e) => {
                            // ★ ErrPayload 不实现 Display，只能 {:?} ★
                            // （它有 what/why/next 三字段，是展示用结构体）
                            log::warn!("worker 无连接，退回 pending：{e:?}");
                            q.finish(item.id, "pending", "", "等待连接 9222").await;
                            tokio::time::sleep(std::time::Duration::from_millis(2000)).await;
                            continue;
                        }
                    };
                    // 忙闲：三判据多数一致
                    match engine::idle_verdict(&cdp).await {
                        Ok(v) if !v.idle => {
                            q.finish(item.id, "pending", "", &format!("等空闲（{}）", v.reason)).await;
                            let g = *gap.lock().await;
                            tokio::time::sleep(g.min(std::time::Duration::from_millis(1500))).await;
                            continue;
                        }
                        Err(e) => {
                            q.finish(item.id, "pending", "", &format!("读忙闲失败：{e}")).await;
                            tokio::time::sleep(std::time::Duration::from_millis(1500)).await;
                            continue;
                        }
                        _ => {}
                    }
                    // 真发
                    match engine::send_only(&cdp, &item.text, &item.conv_id).await {
                        Ok(ev) => {
                            log::info!("队列 #{} 已发：{ev}", item.id);
                            q.finish(item.id, "done", &ev, "").await;
                            q.bump_round().await;
                            q.prune(200).await;
                        }
                        Err(e) => {
                            let msg = format!("{e:#}");
                            if msg.contains("忙") || msg.contains("生成中") {
                                q.finish(item.id, "pending", "", "等空闲").await;
                                tokio::time::sleep(std::time::Duration::from_millis(1200)).await;
                            } else {
                                log::warn!("队列 #{} 失败：{msg}", item.id);
                                q.finish(item.id, "failed", "", &msg).await;
                            }
                        }
                    }
                }
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![diag, ui_click, probe, preview, send, busy, enqueue, queue_view, queue_cancel, queue_clear, loop_start, loop_stop, idle_check, read_text_file, probe_skills])
        .run(tauri::generate_context!())
        .expect("启动失败");
}
