//! CDP（Chrome DevTools Protocol）客户端。
//!
//! 为什么手写而不用现成 crate：Playwright 的 Rust 绑定是社区产物，
//! 而我们要的能力其实很窄——**只需要发命令、等响应**。
//! 为这点需求引入一整套浏览器抽象栈不划算，且版本风险不可控。
//!
//! 协议本身很简单：
//! - HTTP GET `/json/list` 拿到各 target 的 `webSocketDebuggerUrl`；
//! - 连上 WebSocket 后，所有消息都是 JSON，格式统一为
//!   `{ "id": <序号>, "method": "...", "params": {...} }` 收，
//!   `{ "id": <序号>, "result": {...} }` 或
//!   `{ "id": <序号>, "error": {...} }` 回。
//!
//! 唯一麻烦是**消息会乱序到达**（CDP 的事件推送与命令响应交织），
//! 所以要用 `id` 做请求-响应配对，其余消息直接丢弃。

use anyhow::{anyhow, bail, Context, Result};
use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::net::TcpStream;
use tokio::sync::{mpsc, oneshot, Mutex};
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::Message;

/// 默认 CDP 端点（与 WorkBuddy 启动参数一致）。
pub const CDP_ENDPOINT: &str = "http://127.0.0.1:9222";

/// 一条挂起的命令等待回执。
type Pending = oneshot::Sender<Result<Value>>;

/// CDP 连接。
///
/// 用法：
/// ```ignore
/// let mut c = Cdp::connect(CDP_ENDPOINT).await?;
/// let v = c.call("Runtime.evaluate", json!({...})).await?;
/// ```
pub struct Cdp {
    tx: mpsc::UnboundedSender<Message>,
    next_id: AtomicU64,
    /// id → 回执信箱。收到响应时按 id 派发。
    pending: Arc<Mutex<HashMap<u64, Pending>>>,
}

impl Cdp {
    /// 连上并附着到页面 target。
    ///
    /// ★ 为什么不用 `/json/new` ★
    /// 它会**新建**一个标签页——而我们要操作的是 WorkBuddy 已有的那个界面。
    /// 早先在 Node 版踩过：WorkBuddy 直接返回
    /// `Protocol error (Target.createTarget): Not supported`。
    /// 所以必须用 `/json/list` 找到已有 target 再 attach。
    pub async fn connect(endpoint: &str) -> Result<Self> {
        let base = endpoint.trim_end_matches('/');
        // 1) 找页面 target
        let list: Value = ureq_get(&format!("{base}/json/list")).await?;
        let target = pick_page_target(&list)?;
        let ws_url = target
            .get("webSocketDebuggerUrl")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow!("目标 target 没有 webSocketDebuggerUrl：{}", target))?;

        // 2) 连 WebSocket
        let req = ws_url.into_client_request().context("构造 WS 请求失败")?;
        // ws:// 走纯 TCP（CDP 端点永远是本机 ws://，不用 TLS）
        let (stream, _resp) = tokio_tungstenite::connect_async(req)
            .await
            .with_context(|| format!("连 CDP WebSocket 失败：{ws_url}"))?;

        let (tx, mut out_rx) = mpsc::unbounded_channel::<Message>();
        let pending: Arc<Mutex<HashMap<u64, Pending>>> = Arc::new(Mutex::new(HashMap::new()));

        // 3) 后台读循环：按 id 派发响应，其余丢弃
        let p2 = pending.clone();
        // split() 返回 (SplitSink=可写, SplitStream=可读)——别搞反
        let (mut sink, mut source) = stream.split();
        tokio::spawn(async move {
            while let Some(Ok(msg)) = source.next().await {
                if let Message::Text(t) = msg {
                    let Ok(v) = serde_json::from_str::<Value>(&t) else { continue };
                    let Some(id) = v.get("id").and_then(|x| x.as_u64()) else {
                        continue;   // 事件推送，无 id，直接忽略
                    };
                    let mut map = p2.lock().await;
                    // ★必须 take ★：oneshot::Sender 只能发送一次，
                    // 而 map.get() 拿到的是 &Sender，直接 send 编译不过。
                    // remove 同时把这个id 从待回执表里摘掉，避免泄漏。
                    if let Some(tx) = map.remove(&id) {
                        let out = if let Some(e) = v.get("error") {
                            Err(anyhow!("CDP 错误：{e}"))
                        } else {
                            Ok(v.get("result").cloned().unwrap_or(Value::Null))
                        };
                        let _ = tx.send(out);
                    }
                }
            }
        });

        // 4) 后台写循环
        tokio::spawn(async move {
            while let Some(msg) = out_rx.recv().await {
                if sink.send(msg).await.is_err() {
                    break;
                }
            }
        });

        Ok(Cdp {
            tx,
            next_id: AtomicU64::new(1),
            pending,
        })
    }
}

impl Cdp {
    /// 发一条 CDP 命令并等回执。
    pub async fn call(&self, method: &str, params: Value) -> Result<Value> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let payload = json!({ "id": id, "method": method, "params": params });
        let (tx, rx) = oneshot::channel();
        self.pending.lock().await.insert(id, tx);
        self.tx
            .send(Message::text(payload.to_string()))
            .context("CDP 发送失败（连接可能已断）")?;
        match tokio::time::timeout(Duration::from_secs(15), rx).await {
            Ok(Ok(v)) => v,
            Ok(Err(_)) => Err(anyhow!("CDP 响应信箱被丢弃（连接已关闭）")),
            Err(_) => {
                self.pending.lock().await.remove(&id);
                bail!("CDP 命令 {method} 超时（15s）")
            }
        }
    }

    /// 在页面里执行 JS 并取回 JSON 值。
    ///
    /// `await_promise` 用于需要等Promise 的表达式。
    /// `return_by_value` 必须为true，否则拿不到可序列化的值。
    pub async fn eval(&self, expr: &str) -> Result<Value> {
        let r = self
            .call(
                "Runtime.evaluate",
                json!({
                    "expression": expr,
                    "returnByValue": true,
                    "awaitPromise": true,
                }),
            )
            .await?;
        // CDP 把异常塞在 result.exceptionDetails 里，而不是 error 字段
        if let Some(ex) = r.get("exceptionDetails") {
            bail!("页面内 JS 抛错：{ex}");
        }
        Ok(r.get("value").cloned().unwrap_or(Value::Null))
    }
}

/// 从 target 列表里挑出页面型 target。
fn pick_page_target(list: &Value) -> Result<Value> {
    let arr = list.as_array().ok_or_else(|| anyhow!("CDP /json/list 返回的不是数组"))?;
    // 排除 devtools 自身与后台 worker
    let mut candidates: Vec<&Value> = arr
        .iter()
        .filter(|t| {
            let ty = t.get("type").and_then(|v| v.as_str()).unwrap_or("");
            let url = t.get("url").and_then(|v| v.as_str()).unwrap_or("");
            ty == "page" && !url.starts_with("devtools://")
        })
        .collect();
    // 取 url 最短的：WorkBuddy 渲染页 url 通常最短（file://.../app.asar/renderer/...）
    candidates.sort_by_key(|t| {
        t.get("url")
            .and_then(|v| v.as_str())
            .map(|s| s.len())
            .unwrap_or(usize::MAX)
    });
    candidates
        .first()
        .map(|v| (*v).clone())
        .ok_or_else(|| anyhow!("CDP 里找不到 page 类型 target（WorkBuddy 是否在运行？）"))
}

/// 极简 HTTP GET（只用于取 /json/list，不引 reqwest）。
///
/// 为什么手写：这个请求只有一个 URL、零参数、返回 JSON。
/// 为此引入完整 HTTP 客户端 + TLS 依赖不划算（且会撑大 exe）。
/// 换来的是 exe 少 2–3MB。
async fn ureq_get(url: &str) -> Result<Value> {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    // 解析 http://host:port/path
    let rest = url.strip_prefix("http://").ok_or_else(|| anyhow!("只支持 http://，收到 {url}"))?;
    let (hostport, path) = match rest.find('/') {
        Some(i) => (&rest[..i], &rest[i..]),
        None => (rest, "/"),
    };
    let (host, port) = match hostport.rsplit_once(':') {
        Some((h, p)) => (h, p.parse::<u16>().unwrap_or(80)),
        None => (hostport, 80),
    };

    let mut stream = TcpStream::connect((host, port))
        .await
        .with_context(|| format!("连 {host}:{port} 失败"))?;
    let req = format!("GET {path} HTTP/1.1\r\nHost: {hostport}\r\nConnection: close\r\nAccept: application/json\r\n\r\n");
    stream.write_all(req.as_bytes()).await?;

    let mut buf = String::new();
    stream.read_to_string(&mut buf).await?;

    // 跳过 HTTP 头
    let body = buf
        .split_once("\r\n\r\n")
        .map(|(_, b)| b)
        .ok_or_else(|| anyhow!("CDP 响应不是合法 HTTP：{}", &buf[..buf.len().min(200)]))?;
    serde_json::from_str(body.trim()).context("CDP 响应体不是 JSON")
}
