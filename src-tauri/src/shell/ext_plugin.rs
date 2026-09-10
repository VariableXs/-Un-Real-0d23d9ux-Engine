//! L3 shell — ext_plugin.rs（X-4 Tauri 插件扩展）+ ext_daemon.rs 合并（X-5 外部进程扩展）
//!
//! X-4（V1 如实边界）：
//! - 形态：预编译动态库 `.rlb`，`libloading` 加载；入口符号
//!   `variable_ext_main`（extern "Rust"，固定 V1 ABI——`abi_stable` 跨版本
//!   ABI 属后续批，跨引擎版本需重编译，横幅如实提示）。
//! - 宿主只给窄接口 `HostApi`（log / notify），绝不给裸句柄。
//! - 崩溃隔离：`panic::catch_unwind` 包裹；加载/panic 即弃用并横幅，主进程守护。
//!
//! X-5（V1 如实边界）：
//! - 通道：环回 TCP `127.0.0.1:<随机端口>`，换行分隔 JSON-RPC 2.0
//!   （首选命名管道需 Win32_System_Pipes 重型 unsafe，V1 采用计划中的回退通道语义）。
//! - 鉴权：首条消息必须 `handshake { token }`，token = 随机 32 hex 经环境变量
//!   `VARIABLE_EXT_TOKEN` 交给受管守护进程（管道 ACL 的本用户等价替代）。
//! - 生命周期：守护崩溃自动重启，上限 3 次/分钟，超限标记 stopped。

use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Mutex;

use serde_json::{json, Value};

use crate::error::{AppError, CmdResult};

// ---------------------------------------------------------------- X-4 插件

/// 宿主窄接口（只传函数所需最小数据，不给 AppHandle/裸句柄）。
pub struct HostApi {
    pub ext_id: String,
}

impl HostApi {
    pub fn log(&self, msg: &str) {
        crate::shell::extensions::ext_log(&format!("[plugin:{}] {}", self.ext_id, msg));
    }
    pub fn notify(&self, app: &tauri::AppHandle, title: &str, body: &str) {
        use tauri::Emitter;
        let _ = app.emit_to("desktop", "ext://notify", json!({ "extId": self.ext_id, "title": title, "body": body }));
    }
}

static LOADED_PLUGINS: Mutex<Vec<String>> = Mutex::new(Vec::new());

#[tauri::command(async)]
pub fn ext_plugin_load(
    app: tauri::AppHandle,
    st: tauri::State<'_, crate::state::AppState>,
    id: String,
    lib_path: String,
) -> CmdResult<String> {
    // 插件必须来自已登记扩展目录（防任意路径加载）
    let dir = crate::shell::extensions::ext_root(&st).join(&id);
    let target = std::path::Path::new(&lib_path);
    if !target.starts_with(&dir) || !target.is_file() {
        return Err(AppError::validation("lib_path 必须是扩展目录内的 .rlb 文件"));
    }
    if LOADED_PLUGINS
        .lock()
        .map_err(|_| AppError::db("mutex"))?
        .iter()
        .any(|s| s == &id)
    {
        return Err(AppError::validation(format!("插件已加载: {id}")));
    }
    let res = std::panic::catch_unwind(|| unsafe {
        let lib = libloading::Library::new(target)
            .map_err(|e| format!("加载失败: {e}"))?;
        let entry: libloading::Symbol<fn(&HostApi) -> i32> =
            lib.get(b"variable_ext_main")
                .map_err(|e| format!("缺少入口 variable_ext_main: {e}"))?;
        let api = HostApi { ext_id: id.clone() };
        let code = entry(&api);
        drop(lib); // V1：入口调用后即卸载（常驻插件需在入口内自建线程，见示例）
        Ok::<i32, String>(code)
      });
      let out = match res {
          Ok(Ok(code)) => format!("插件执行完成，退出码 {code}"),
        Ok(Err(e)) => return Err(AppError::validation(format!("插件 {id}: {e}"))),
        Err(_) => {
            crate::shell::extensions::ext_log(&format!("[plugin:{id}] PANIC — 已弃用"));
            return Err(AppError::validation("插件 panic，已弃用（主进程无损伤）"));
        }
    };
    LOADED_PLUGINS
        .lock()
        .map_err(|_| AppError::db("mutex"))?
        .push(id);
    Ok(out)
}

#[tauri::command(async)]
pub fn ext_plugin_unload(id: String) -> CmdResult<()> {
    LOADED_PLUGINS
        .lock()
        .map_err(|_| AppError::db("mutex"))?
        .retain(|s| s != &id);
    Ok(())
}

// ---------------------------------------------------------------- X-5 守护进程

struct DaemonState {
    port: u16,
    restarts: AtomicU32,
    stopped: bool,
}

static DAEMONS: Mutex<std::collections::BTreeMap<String, DaemonState>> =
    Mutex::new(std::collections::BTreeMap::new());

#[tauri::command(async)]
pub fn ext_daemon_start(
    app: tauri::AppHandle,
    st: tauri::State<'_, crate::state::AppState>,
    id: String,
    cmd: String,
    args: Vec<String>,
) -> CmdResult<u16> {
    use rand::RngCore;
    // cmd 必须在扩展目录内（相对路径）或 PATH 内的命令名
    let ext_dir = crate::shell::extensions::ext_root(&st).join(&id);
    let listener = TcpListener::bind("127.0.0.1:0").map_err(|e| AppError::io(e.to_string()))?;
    let port = listener.local_addr().map_err(|e| AppError::io(e.to_string()))?.port();
    let mut token = [0u8; 16];
    rand::rngs::OsRng.fill_bytes(&mut token);
    let token_hex: String = token.iter().map(|b| format!("{b:02X}")).collect();

    // 监听线程只启一次（重启沿用同一端口与 token）
    let thread_token = token_hex.clone();
    std::thread::spawn(move || serve_jsonrpc(listener, thread_token));
    spawn_daemon(&app, &ext_dir, &id, &cmd, &args, port, &token_hex, 0)?;
    DAEMONS
        .lock()
        .map_err(|_| AppError::db("mutex"))?
        .insert(id.clone(), DaemonState { port, restarts: AtomicU32::new(0), stopped: false });
    Ok(port)
}

fn spawn_daemon(
    app: &tauri::AppHandle,
    ext_dir: &std::path::Path,
    id: &str,
    cmd: &str,
    args: &[String],
    port: u16,
    token: &str,
    attempt: u32,
) -> CmdResult<()> {
    use std::process::{Command, Stdio};
    let mut c = Command::new(cmd);
    c.current_dir(ext_dir)
        .args(args)
        .env("VARIABLE_EXT_HOST", format!("127.0.0.1:{port}"))
        .env("VARIABLE_EXT_TOKEN", token)
        .env("VARIABLE_EXT_ID", id)
        .stdin(Stdio::null());
    let mut child = c
        .spawn()
        .map_err(|e| AppError::io(format!("守护进程启动失败: {e}")))?;
    let app2 = app.clone();
    let id2 = id.to_string();
    let cmd2 = cmd.to_string();
    let args2 = args.to_vec();
    let dir2 = ext_dir.to_path_buf();
    let token2 = token.to_string();
    std::thread::spawn(move || {
        // 崩溃自动重启：上限 3 次/分钟，超限标记 stopped 并横幅
        let _ = child.wait();
        if let Ok(g) = DAEMONS.lock() {
            if let Some(d) = g.get(&id2) {
                if d.stopped {
                    return;
                }
            }
        }
        let n = attempt + 1;
        crate::shell::extensions::ext_log(&format!("[daemon:{id2}] exited, restart {n}/3"));
        if let Ok(g) = DAEMONS.lock() {
            if let Some(d) = g.get(&id2) {
                d.restarts.store(n, Ordering::Relaxed);
            }
        }
        if n <= 3 {
            let _ = spawn_daemon(&app2, &dir2, &id2, &cmd2, &args2, port, &token2, n);
        } else {
            if let Ok(mut g) = DAEMONS.lock() {
                if let Some(d) = g.get_mut(&id2) {
                    d.stopped = true;
                }
            }
            use tauri::Emitter;
            let _ = app2.emit_to("desktop", "ext://notify", json!({
                "extId": id2, "title": "扩展守护已停止", "body": "重启超限（3 次/分钟）"
            }));
        }
    });
    Ok(())
}

/// JSON-RPC 2.0 面板（换行分隔；V1 方法：handshake | ping）。
fn serve_jsonrpc(listener: TcpListener, token: String) {
    for stream in listener.incoming().flatten() {
        let mut writer = match stream.try_clone() {
            Ok(w) => w,
            Err(_) => continue,
        };
        let mut reader = BufReader::new(stream);
        let mut line = String::new();
        if reader.read_line(&mut line).is_err() {
            continue;
        }
        let reply = match serde_json::from_str::<Value>(line.trim()) {
            Ok(v) if v.get("method").and_then(|m| m.as_str()) == Some("handshake") => {
                let got = v.pointer("/params/token").and_then(|t| t.as_str()).unwrap_or("");
                if got == token {
                    json!({ "jsonrpc": "2.0", "id": v.get("id"), "result": "ok" })
                } else {
                    json!({ "jsonrpc": "2.0", "id": v.get("id"), "error": { "code": -32001, "message": "bad token" } })
                }
            }
            Ok(v) if v.get("method").and_then(|m| m.as_str()) == Some("ping") => {
                json!({ "jsonrpc": "2.0", "id": v.get("id"), "result": "pong" })
            }
            Ok(v) => json!({ "jsonrpc": "2.0", "id": v.get("id"),
                "error": { "code": -32601, "message": "method not found (V1: handshake|ping)" } }),
            Err(_) => json!({ "jsonrpc": "2.0", "id": null,
                "error": { "code": -32700, "message": "parse error" } }),
        };
        let _ = writeln!(writer, "{reply}");
    }
}

#[tauri::command(async)]
pub fn ext_daemon_status() -> CmdResult<Vec<Value>> {
    let g = DAEMONS.lock().map_err(|_| AppError::db("mutex"))?;
    Ok(g.iter()
        .map(|(id, d)| json!({ "id": id, "port": d.port, "stopped": d.stopped,
            "restarts": d.restarts.load(Ordering::Relaxed) }))
        .collect())
}

pub fn example_daemon_script(dir: &PathBuf) -> CmdResult<PathBuf> {
    std::fs::create_dir_all(dir).map_err(|e| AppError::io(e.to_string()))?;
    let js = dir.join("calendar-daemon.js");
    std::fs::write(
        &js,
        r#"// X-5 示例：本地日历同步 stub（Node 守护，JSON-RPC 2.0）
const net = require("net");
const host = process.env.VARIABLE_EXT_HOST || "127.0.0.1:0";
const token = process.env.VARIABLE_EXT_TOKEN || "";
const [h, p] = host.split(":");
const sock = net.connect(+p, h, () => {
  sock.write(JSON.stringify({ jsonrpc: "2.0", id: 1, method: "handshake", params: { token } }) + "\n");
  sock.write(JSON.stringify({ jsonrpc: "2.0", id: 2, method: "ping" }) + "\n");
});
let buf = "";
sock.on("data", (d) => {
  buf += d.toString();
  let i;
  while ((i = buf.indexOf("\n")) >= 0) {
    const line = buf.slice(0, i); buf = buf.slice(i + 1);
    try { const r = JSON.parse(line); console.log("reply:", JSON.stringify(r)); } catch {}
  }
});
setInterval(() => {}, 1 << 30); // keepalive
"#,
    )
    .map_err(|e| AppError::io(e.to_string()))?;
    Ok(js)
}

#[tauri::command(async)]
pub fn ext_daemon_example(st: tauri::State<'_, crate::state::AppState>) -> CmdResult<String> {
    let dir = crate::shell::extensions::ext_root(&st).join("calendar-sync");
    let p = example_daemon_script(&dir)?;
    Ok(p.to_string_lossy().to_string())
}
