//! Code Analysis 壳A · Windows 独立版（AI-09 W4：真实外壳）。
//!
//! 一个 core，三个壳——本入口是「壳A：Windows 独立完整版」：
//! std-only HTTP 服务器伺服 `ui/`，浏览器即窗口（C09 布局由 CSS 落地）。
//!
//! - **C08**：写回通道落在这里——`/api/write` 用 `ca_core::shell::WriteChannel`
//!   磁盘后端真实写项目目录，before/after 记账可查（`/api/journal`）。
//! - **C10**：单实例 = 本地端口锁。第二个实例发现端口被占，把路径参数
//!   转发给首个实例（`/api/open`）后退出——行为与「再开一个文件」等价。
//! - **C13~C24 等价物**：`/api/shell` 输出三壳档案与壁纸预设，
//!   `/api/parity` 输出三端等价验收清单（DoD 物化）。
//! - 浏览器拉起：默认 `cmd /c start`，`--no-open` 关闭。
//!
//! 用法：`CodeAnalysis.exe [项目路径] [--port N] [--shell A|B|C] [--no-open]`

use ca_core::ir::open_project;
use ca_core::model::NodeKind;
use ca_core::shell::{
    detect_shell, parity_checklist, parity_markdown, wallpaper_css, wallpaper_for_style,
    ShellKind, ShellProfile, WriteBackend, WriteChannel,
};
use std::io::{Read, Write as IoWrite};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::Mutex;

const DEFAULT_PORT: u16 = 8977;
const INDEX_HTML: &str = "index.html";

/// 服务器状态：当前打开的项目 IR + 写回通道（壳A 全局唯一）。
struct State {
    ui_dir: PathBuf,
    project_root: PathBuf,
    ir: Option<ca_core::model::ProjectIR>,
    channel: Option<WriteChannel>,
    detected: ShellKind,
}

impl State {
    fn open_project(&mut self, path: &str) -> Result<String, String> {
        let root = PathBuf::from(path);
        if !root.is_dir() {
            return Err(format!("不是目录：{path}"));
        }
        let ir = open_project(&root).map_err(|e| format!("打开失败：{e}"))?;
        let summary = format!(
            "{} · {} 文件 · {} 行 · {} 调用边",
            ir.name,
            ir.file_count,
            ir.loc,
            ir.calls.len()
        );
        self.channel = Some(WriteChannel::new(WriteBackend::Disk {
            root: root.to_string_lossy().into_owned(),
        }));
        self.project_root = root;
        self.ir = Some(ir);
        Ok(summary)
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut path = String::from(".");
    let mut port = DEFAULT_PORT;
    let mut shell_flag: Option<String> = None;
    let mut no_open = false;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--port" if i + 1 < args.len() => {
                port = args[i + 1].parse().unwrap_or(DEFAULT_PORT);
                i += 1;
            }
            "--shell" if i + 1 < args.len() => {
                shell_flag = Some(args[i + 1].clone());
                i += 1;
            }
            "--no-open" => no_open = true,
            p if !p.starts_with("--") => path = p.to_string(),
            other => {
                eprintln!("未知参数：{other}");
            }
        }
        i += 1;
    }

    let ui_dir = resolve_ui_dir();
    if !ui_dir.join(INDEX_HTML).is_file() {
        eprintln!("找不到 ui 目录（{}）：用 CA_UI_DIR 指定", ui_dir.display());
        std::process::exit(1);
    }

    let detected = detect_shell(shell_flag.as_deref(), |k| std::env::var(k).ok());
    let mut state = State {
        ui_dir,
        project_root: PathBuf::from("."),
        ir: None,
        channel: None,
        detected,
    };
    if let Err(e) = state.open_project(&path) {
        eprintln!("{e}");
        std::process::exit(1);
    }

    // C10 单实例：端口即锁。被占 → 参数转发给首实例后退出。
    let listener = match TcpListener::bind(("127.0.0.1", port)) {
        Ok(l) => l,
        Err(_) => {
            if forward_to_first_instance(port, &path) {
                println!("已转发给运行中的 Code Analysis（端口 {port}），本实例退出。");
            } else {
                eprintln!("端口 {port} 被非 Code Analysis 程序占用。");
                std::process::exit(1);
            }
            return;
        }
    };

    let url = format!("http://127.0.0.1:{port}/");
    println!("Code Analysis 壳A · {}", state.ir.as_ref().map(|ir| ir.name.clone()).unwrap_or_default());
    println!("伺服 ui：{} · 壳：{}", state.ui_dir.display(), detected.name());
    println!("浏览器打开 {url}（--no-open 关闭自动拉起）");
    if !no_open {
        launch_browser(&url);
    }

    let state = std::sync::Arc::new(Mutex::new(state));
    for stream in listener.incoming() {
        let Ok(stream) = stream else { continue };
        let st = std::sync::Arc::clone(&state);
        // 每连接一线程：一个客户端挂起不拖垮整个壳。
        std::thread::spawn(move || {
            if let Ok(mut s) = st.lock() {
                let _ = handle_conn(stream, &mut s);
            }
        });
    }
}

fn resolve_ui_dir() -> PathBuf {
    if let Ok(d) = std::env::var("CA_UI_DIR") {
        let p = PathBuf::from(d);
        if p.join(INDEX_HTML).is_file() {
            return p;
        }
    }
    // exe 同级或任一祖先目录（发布布局 / cargo target 树）→ 当前目录。
    if let Ok(exe) = std::env::current_exe() {
        if let Some(parent) = exe.parent() {
            for base in parent.ancestors() {
                let p = base.join("ui");
                if p.join(INDEX_HTML).is_file() {
                    return p;
                }
            }
        }
    }
    PathBuf::from("ui")
}

fn forward_to_first_instance(port: u16, path: &str) -> bool {
    let Ok(mut s) = TcpStream::connect(("127.0.0.1", port)) else {
        return false;
    };
    let req = format!(
        "GET /api/open?path={} HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n",
        urlencode(path)
    );
    s.write_all(req.as_bytes()).is_ok()
        && s.read(&mut [0u8; 64]).is_ok()
}

fn launch_browser(url: &str) {
    let mut cmd = std::process::Command::new("cmd");
    cmd.args(["/c", "start", "", url]);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    let _ = cmd.spawn();
}

// ---------------------------------------------------------------------------
// HTTP（std-only：解析请求行 + 头，Content-Length 定长读体）
// ---------------------------------------------------------------------------

fn handle_conn(mut stream: TcpStream, state: &mut State) -> std::io::Result<()> {
    let mut buf = [0u8; 16384];
    let n = stream.read(&mut buf)?;
    let raw = &buf[..n];
    // 头与体以 \r\n\r分隔；体可能已随首包到达，禁止再阻塞读。
    let header_end = raw
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .map(|p| p + 4)
        .unwrap_or(raw.len());
    let req = String::from_utf8_lossy(&raw[..header_end]);
    let mut lines = req.lines();
    let Some(first) = lines.next() else { return Ok(()) };
    let mut parts = first.split_whitespace();
    let method = parts.next().unwrap_or("GET").to_string();
    let target = parts.next().unwrap_or("/").to_string();

    let mut content_len = 0usize;
    for l in lines {
        if let Some(v) = l.to_ascii_lowercase().strip_prefix("content-length:") {
            content_len = v.trim().parse().unwrap_or(0);
        }
    }
    // 已在首包里的体字节；不足部分继续读（上限 4 MiB）。
    let mut body: Vec<u8> = raw[header_end.min(raw.len())..].to_vec();
    let cap = content_len.min(4 * 1024 * 1024);
    while body.len() < cap {
        let mut chunk = [0u8; 8192];
        match stream.read(&mut chunk) {
            Ok(0) | Err(_) => break,
            Ok(m) => body.extend_from_slice(&chunk[..m]),
        }
    }
    body.truncate(cap);

    let (path_part, query) = match target.split_once('?') {
        Some((p, q)) => (p.to_string(), q.to_string()),
        None => (target.clone(), String::new()),
    };
    let query_map = parse_query(&query);

    let (status, ctype, payload) = route(&method, &path_part, &query_map, &body, state);
    let head = format!(
        "HTTP/1.1 {status}\r\nContent-Type: {ctype}; charset=utf-8\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n",
        payload.len()
    );
    stream.write_all(head.as_bytes())?;
    stream.write_all(&payload)?;
    Ok(())
}

fn route(
    method: &str,
    path: &str,
    q: &[(String, String)],
    body: &[u8],
    st: &mut State,
) -> (&'static str, &'static str, Vec<u8>) {
    match (method, path) {
        ("GET", "/") | ("GET", "/index.html") => serve_static(st, INDEX_HTML),
        ("GET", "/api/ping") => ("200 OK", "application/json", br#"{"app":"codeanalysis","ok":true}"#.to_vec()),
        ("GET", "/api/open") => {
            let p = qget(q, "path").unwrap_or_default();
            match st.open_project(&p) {
                Ok(summary) => ("200 OK", "application/json", jobj(&[("ok", "true"), ("summary", &jstr(&summary))])),
                Err(e) => ("400 Bad Request", "application/json", jobj(&[("ok", "false"), ("error", &jstr(&e))])),
            }
        }
        ("GET", "/api/ir") => {
            if let Some(p) = qget(q, "path") {
                if let Err(e) = st.open_project(&p) {
                    return ("400 Bad Request", "application/json", jobj(&[("ok", "false"), ("error", &jstr(&e))]));
                }
            }
            match &st.ir {
                Some(ir) => ("200 OK", "application/json", ir_to_json(ir).into_bytes()),
                None => ("404 Not Found", "application/json", jobj(&[("error", "未打开项目")])),
            }
        }
        ("GET", "/api/shell") => ("200 OK", "application/json", shell_to_json(st.detected).into_bytes()),
        ("GET", "/api/parity") => ("200 OK", "application/json", parity_to_json().into_bytes()),
        ("GET", "/api/journal") => ("200 OK", "application/json", journal_to_json(st).into_bytes()),
        ("POST", "/api/write") => {
            let Some(rel) = qget(q, "path") else {
                return ("400 Bad Request", "application/json", jobj(&[("error", "缺 path 参数")]));
            };
            let Some(ch) = &mut st.channel else {
                return ("409 Conflict", "application/json", jobj(&[("error", "未打开项目")]));
            };
            let ts = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis() as u64)
                .unwrap_or(0);
            match ch.write(&rel, body, ts) {
                Ok(n) => {
                    let rec = &ch.journal()[ch.journal().len() - 1];
                    (
                        "200 OK",
                        "application/json",
                        jobj(&[
                            ("ok", "true"),
                            ("bytes", &n.to_string()),
                            ("before", &rec.before.to_string()),
                            ("after", &rec.after.to_string()),
                        ]),
                    )
                }
                Err(e) => ("400 Bad Request", "application/json", jobj(&[("ok", "false"), ("error", &jstr(&e.to_string()))])),
            }
        }
        ("GET", p) => serve_static(st, p.trim_start_matches('/')),
        _ => ("405 Method Not Allowed", "application/json", jobj(&[("error", "方法不支持")])),
    }
}

fn serve_static(st: &State, rel: &str) -> (&'static str, &'static str, Vec<u8>) {
    // 防穿越：拒绝 .. / 盘符 / 绝对路径。
    if rel.contains("..") || rel.contains(':') || rel.starts_with('/') || rel.starts_with('\\') {
        return ("403 Forbidden", "text/plain", b"forbidden".to_vec());
    }
    let full = st.ui_dir.join(rel);
    match std::fs::read(&full) {
        Ok(data) => {
            let mime = match full.extension().and_then(|e| e.to_str()) {
                Some("html") => "text/html",
                Some("css") => "text/css",
                Some("js") | Some("mjs") => "text/javascript",
                Some("json") => "application/json",
                Some("svg") => "image/svg+xml",
                Some("png") => "image/png",
                Some("ico") => "image/x-icon",
                _ => "application/octet-stream",
            };
            ("200 OK", mime, data)
        }
        Err(_) => ("404 Not Found", "text/plain", b"not found".to_vec()),
    }
}

// ---------------------------------------------------------------------------
// JSON 序列化（std-only 手搓：字符串转义 + 结构拼装）
// ---------------------------------------------------------------------------

fn jstr(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

fn jobj(fields: &[(&str, &str)]) -> Vec<u8> {
    let body: Vec<String> = fields.iter().map(|(k, v)| format!("{}:{}", jstr(k), v)).collect();
    format!("{{{}}}", body.join(",")).into_bytes()
}

fn qget(q: &[(String, String)], key: &str) -> Option<String> {
    q.iter().find(|(k, _)| k == key).map(|(_, v)| v.clone())
}

fn parse_query(q: &str) -> Vec<(String, String)> {
    q.split('&')
        .filter(|s| !s.is_empty())
        .map(|kv| match kv.split_once('=') {
            Some((k, v)) => (urldecode(k), urldecode(v)),
            None => (urldecode(kv), String::new()),
        })
        .collect()
}

fn urlencode(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' | b'/' => {
                out.push(b as char)
            }
            other => out.push_str(&format!("%{other:02X}")),
        }
    }
    out
}

fn urldecode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'%' if i + 2 < bytes.len() + 1 && i + 2 < bytes.len() + 1 => {
                let hex = |h: u8| -> Option<u8> {
                    match h {
                        b'0'..=b'9' => Some(h - b'0'),
                        b'a'..=b'f' => Some(h - b'a' + 10),
                        b'A'..=b'F' => Some(h - b'A' + 10),
                        _ => None,
                    }
                };
                if i + 2 < bytes.len() {
                    if let (Some(h), Some(l)) = (hex(bytes[i + 1]), hex(bytes[i + 2])) {
                        out.push(h * 16 + l);
                        i += 3;
                        continue;
                    }
                }
                out.push(b'%');
                i += 1;
            }
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            b => {
                out.push(b);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// ProjectIR → ui 完整 IR 转储（`CA.irFromJSON` 第二形态契约）：
/// `{ name, lang, root, nodes:[{id,kind,name,parent,loc}], edges:[{from,to,freq,kind}] }`。
/// kind 映射对齐 ui KIND_NAME：Project0/Module1/Subsystem2/File3/Class4/Func5/Stmt·Line6。
fn ir_to_json(ir: &ca_core::model::ProjectIR) -> String {
    let kind_id = |k: NodeKind| match k {
        NodeKind::Project => 0,
        NodeKind::Module => 1,
        NodeKind::Subsystem => 2,
        NodeKind::File => 3,
        NodeKind::Class => 4,
        NodeKind::Func => 5,
        NodeKind::Stmt | NodeKind::Line => 6,
    };

    // parent 表（core 只有 children）+ 子树规模（文件 loc 估算）。
    let mut parent = vec![-1i64; ir.tree.nodes.len()];
    let mut desc = vec![1usize; ir.tree.nodes.len()];
    for (i, n) in ir.tree.nodes.iter().enumerate() {
        for &c in &n.children {
            if c < parent.len() {
                parent[c] = i as i64;
            }
        }
    }
    // 后序累计子树规模。
    fn fill_desc(tree: &ca_core::model::Tree, id: usize, desc: &mut [usize]) -> usize {
        let mut total = 1;
        for &c in &tree.nodes[id].children {
            total += fill_desc(tree, c, desc);
        }
        desc[id] = total;
        total
    }
    if ir.tree.root < ir.tree.nodes.len() {
        fill_desc(&ir.tree, ir.tree.root, &mut desc);
    }

    let mut nodes: Vec<String> = Vec::with_capacity(ir.tree.nodes.len());
    for (i, n) in ir.tree.nodes.iter().enumerate() {
        let loc = if n.kind == NodeKind::File { desc[i] } else { (n.span.1 + 1).max(1) };
        nodes.push(format!(
            "{{\"id\":{i},\"kind\":{},\"name\":{},\"parent\":{},\"loc\":{loc}}}",
            kind_id(n.kind),
            jstr(&n.name),
            parent[i]
        ));
    }

    // 函数名 → 节点 id（首个）；调用边 from "fn@file" / to callee 名。
    let mut by_name: Vec<(&str, usize)> = Vec::new();
    for (i, n) in ir.tree.nodes.iter().enumerate() {
        if n.kind == NodeKind::Func {
            by_name.push((n.name.as_str(), i));
        }
    }
    let fid = |name: &str| by_name.iter().find(|(n, _)| *n == name).map(|(_, i)| *i);
    let mut edges: Vec<String> = Vec::new();
    for e in &ir.calls {
        let from = e.from.split('@').next().unwrap_or("");
        if let (Some(f), Some(t)) = (fid(from), fid(&e.to)) {
            if f != t {
                edges.push(format!(
                    "{{\"from\":{f},\"to\":{t},\"freq\":0.2,\"kind\":\"call\"}}"
                ));
            }
        }
    }

    format!(
        "{{\"name\":{},\"lang\":\"rust\",\"root\":{},\"nodes\":[{}],\"edges\":[{}]}}",
        jstr(&ir.name),
        ir.tree.root,
        nodes.join(","),
        edges.join(",")
    )
}

/// 三壳档案 + 8 风格壁纸预设（/api/shell）。
fn shell_to_json(detected: ShellKind) -> String {
    let prof = |p: &ShellProfile| {
        format!(
            "{{\"kind\":\"{}\",\"title\":{},\"input\":{},\"write_backend\":{},\"wallpaper_target\":{},\"asset_source\":{},\"single_instance\":{}}}",
            p.kind.name(),
            jstr(p.title),
            jstr(p.input),
            jstr(p.write_backend),
            jstr(p.wallpaper_target),
            jstr(p.asset_source),
            jstr(p.single_instance),
        )
    };
    let profiles: Vec<String> = ShellKind::ALL.iter().map(|&k| prof(&ShellProfile::of(k))).collect();
    let wallpapers: Vec<String> = (0..8)
        .map(|i| {
            let l = wallpaper_for_style(i);
            format!(
                "{{\"style\":{i},\"css\":{}}}",
                jstr(&wallpaper_css(&l))
            )
        })
        .collect();
    format!(
        "{{\"detected\":\"{}\",\"profiles\":[{}],\"wallpapers\":[{}]}}",
        detected.name(),
        profiles.join(","),
        wallpapers.join(",")
    )
}

/// 三端等价验收清单（/api/parity，C24 DoD 物化）。
fn parity_to_json() -> String {
    let rows = parity_checklist(false);
    let items: Vec<String> = rows
        .iter()
        .map(|r| {
            format!(
                "{{\"group\":{},\"title\":{},\"shell\":\"{}\",\"method\":{},\"status\":{}}}",
                jstr(r.group),
                jstr(r.title),
                r.shell.name(),
                jstr(r.method),
                jstr(r.status),
            )
        })
        .collect();
    format!(
        "{{\"markdown\":{},\"rows\":[{}]}}",
        jstr(&parity_markdown(false)),
        items.join(",")
    )
}

fn journal_to_json(st: &State) -> String {
    let Some(ch) = &st.channel else {
        return "[]".to_string();
    };
    let items: Vec<String> = ch
        .journal()
        .iter()
        .map(|r| {
            format!(
                "{{\"path\":{},\"before\":{},\"after\":{},\"bytes\":{},\"ts_ms\":{}}}",
                jstr(&r.path),
                r.before,
                r.after,
                r.bytes,
                r.ts_ms
            )
        })
        .collect();
    format!("[{}]", items.join(","))
}
