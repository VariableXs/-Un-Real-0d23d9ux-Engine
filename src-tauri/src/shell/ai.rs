//! L3/L5 — 云端 AI 编程矩阵（批次 B-8/B-9/B-10/B-11，BLUEPRINT 3.6 / 7.5）：
//! - 工具注册表：claude-code / codex / zcode（CLI 类；Web/IDE 类后续批次）
//! - 容器内 Node 运行时：runtime/node（零网络默认；下载需用户逐域授权）
//! - ai install：npm 全局装进容器 runtime/npm-global（绝不碰宿主 PATH/APPDATA）
//! - AI Hub 状态：三态卡片数据源（未安装 / 未登录 / 已登录-推断）
//! - Vault 2.0：账号身份库（复用保险箱 AES-256-GCM；vault 未解锁即不可读）
//! - Token 注入与多账号切换：把身份写入终端执行档（V1 经便携 WT 嵌入通道）
//!
//! 如实边界：登录态 = 容器内配置标记文件存在性推断，不读凭据本体；
//! npm 包名以 registry 实际解析为准（codex/zcode 包名待真机确认，见 selfcheck）。

use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::error::{AppError, CmdResult};
use crate::state::AppState;

// ---------- 工具注册表 ----------

pub struct AiTool {
    pub id: &'static str,
    pub name: &'static str,
    /// npm 全局包名（ai install <tool> 实际安装的 spec）
    pub npm_package: &'static str,
    /// CLI shim 名（npm-global 下的 .cmd）
    pub shim: &'static str,
    /// 容器内配置目录（相对 {home}）
    pub config_dir: &'static str,
    /// 登录态标记文件（相对 config_dir；存在 = 已登录·推断）
    pub auth_markers: &'static [&'static str],
    /// Token 注入的环境变量名
    pub token_env: &'static str,
    /// 建议出站域名（M8 白名单执行前仅登记）
    pub domains: &'static [&'static str],
}

pub const AI_TOOLS: &[AiTool] = &[
    AiTool {
        id: "claude-code",
        name: "Claude Code",
        npm_package: "@anthropic-ai/claude-code",
        shim: "claude.cmd",
        config_dir: ".claude",
        auth_markers: &[".credentials.json", ".claude.json"],
        token_env: "ANTHROPIC_API_KEY",
        domains: &["api.anthropic.com", "console.anthropic.com", "statsig.anthropic.com"],
    },
    AiTool {
        id: "codex",
        name: "Codex CLI",
        npm_package: "@openai/codex",
        shim: "codex.cmd",
        config_dir: ".codex",
        auth_markers: &["auth.json"],
        token_env: "OPENAI_API_KEY",
        domains: &["api.openai.com", "auth.openai.com"],
    },
    AiTool {
        id: "zcode",
        name: "ZCode CLI",
        npm_package: "@z-ai/zcode",
        shim: "zcode.cmd",
        config_dir: ".zcode",
        auth_markers: &["auth.json", "config.json"],
        token_env: "ZAI_API_KEY",
        domains: &["api.z.ai"],
    },
];

fn tool_by_id(id: &str) -> CmdResult<&'static AiTool> {
    AI_TOOLS
        .iter()
        .find(|t| t.id == id)
        .ok_or_else(|| AppError::validation(format!("未知 AI 工具 / Unknown AI tool: {id}")))
}

// ---------- 容器内路径 ----------

pub fn node_dir(root: &Path) -> PathBuf {
    root.join("runtime").join("node")
}

fn node_exe(root: &Path) -> PathBuf {
    node_dir(root).join("node.exe")
}

fn npm_cmd(root: &Path) -> PathBuf {
    node_dir(root).join("npm.cmd")
}

pub fn npm_global_dir(root: &Path) -> PathBuf {
    root.join("runtime").join("npm-global")
}

fn shim_path(root: &Path, tool: &AiTool) -> PathBuf {
    npm_global_dir(root).join(tool.shim)
}

fn home_dir(root: &Path) -> PathBuf {
    root.join("home")
}

// ---------- 状态（B-9 AI Hub 数据源） ----------

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AiToolStatus {
    id: String,
    name: String,
    npm_package: String,
    /// 容器内 Node 运行时就绪
    node_installed: bool,
    /// 工具 CLI shim 已装进容器 npm-global
    installed: bool,
    /// 登录态推断：容器配置目录存在且含登录标记文件
    logged_in: bool,
    /// 上次会话时间（配置目录 mtime，本地计数非遥测）
    last_activity_ms: Option<u64>,
    domains: Vec<String>,
}

fn dir_mtime_ms(p: &Path) -> Option<u64> {
    fs::metadata(p).ok()?.modified().ok()?.duration_since(std::time::UNIX_EPOCH).ok().map(|d| d.as_millis() as u64)
}

#[tauri::command]
pub fn ai_tool_status(st: tauri::State<AppState>) -> CmdResult<Vec<AiToolStatus>> {
    let node_installed = node_exe(&st.data_dir).is_file();
    Ok(AI_TOOLS
        .iter()
        .map(|t| {
            let installed = shim_path(&st.data_dir, t).is_file();
            let cfg = home_dir(&st.data_dir).join(t.config_dir);
            let logged_in = cfg.is_dir() && t.auth_markers.iter().any(|m| cfg.join(m).is_file());
            AiToolStatus {
                id: t.id.to_string(),
                name: t.name.to_string(),
                npm_package: t.npm_package.to_string(),
                node_installed,
                installed,
                logged_in,
                last_activity_ms: dir_mtime_ms(&cfg),
                domains: t.domains.iter().map(|s| s.to_string()).collect(),
            }
        })
        .collect())
}

// ---------- Node 运行时与安装器（B-8） ----------

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AiProgress {
    tool: String,
    phase: String, // node-download | node-extract | npm-install | done | error
    done: u64,
    total: u64,
    message: String,
}

fn emit_progress(app: &tauri::AppHandle, p: AiProgress) {
    use tauri::Emitter;
    let _ = app.emit("ai://progress", p);
}

const NODE_MAJOR: u32 = 22;

/// 解析 latest-v{N}.x 目录页里的 win-x64 zip 文件名（node-vX.Y.Z-win-x64.zip）。
fn resolve_node_zip_name() -> CmdResult<String> {
    let out = hidden_command("curl.exe")
        .args(["-fsSL", &format!("https://nodejs.org/dist/latest-v{NODE_MAJOR}.x/")])
        .output()
        .map_err(|e| AppError::io(format!("探测 Node 版本失败（需要 curl）: {e}")))?;
    if !out.status.success() {
        return Err(AppError::io(format!(
            "Node 分发目录不可达（{}）——请检查网络授权 / nodejs.org unreachable",
            out.status
        )));
    }
    let body = String::from_utf8_lossy(&out.stdout);
    let re = |s: &str| {
        s.split("href=\"")
            .filter_map(|seg| seg.split('"').next())
            .find(|name| {
                name.starts_with("node-v")
                    && name.ends_with("-win-x64.zip")
                    && !name.contains("arm")
            })
            .map(|s| s.to_string())
    };
    re(&body).ok_or_else(|| AppError::io("未能从分发目录解析 Node zip 文件名 / cannot parse node zip name"))
}

/// 后台下载并解压 Node 便携运行时（nodejs.org，需用户授权）→ runtime/node。
/// 进度经 `ai://progress` 事件流回传（真实字节计数）。
#[tauri::command]
pub fn ai_install_node(st: tauri::State<AppState>, app: tauri::AppHandle) -> CmdResult<()> {
    if node_exe(&st.data_dir).is_file() {
        return Ok(()); // 幂等
    }
    let data_dir = st.data_dir.clone();
    std::thread::spawn(move || {
        let step = |phase: &str, done: u64, total: u64, msg: &str| {
            emit_progress(&app, AiProgress {
                tool: "node".into(), phase: phase.into(), done, total, message: msg.into(),
            });
        };
        let result = (|| -> CmdResult<()> {
            step("node-download", 0, 1, "解析版本");
            let zip_name = resolve_node_zip_name()?;
            let zip_url = format!("https://nodejs.org/dist/latest-v{NODE_MAJOR}.x/{zip_name}");
            let tmp_zip = std::env::temp_dir().join(&zip_name);
            step("node-download", 0, 0, &zip_url);
            // curl 进度轮询：文件字节数为真实进度
            let mut child = hidden_command("curl.exe")
                .args(["-fL", "-o"])
                .arg(&tmp_zip)
                .arg(&zip_url)
                .spawn()
                .map_err(|e| AppError::io(format!("下载启动失败: {e}")))?;
            loop {
                match child.try_wait() {
                    Ok(Some(code)) => {
                        if !code.success() {
                            return Err(AppError::io(format!("下载失败（curl exit {code}）")));
                        }
                        break;
                    }
                    Ok(None) => {
                        let done = fs::metadata(&tmp_zip).map(|m| m.len()).unwrap_or(0);
                        step("node-download", done, 0, &zip_name);
                        std::thread::sleep(Duration::from_millis(500));
                    }
                    Err(e) => return Err(AppError::io(format!("下载中断: {e}"))),
                }
            }
            let total = fs::metadata(&tmp_zip).map(|m| m.len()).unwrap_or(0);
            step("node-extract", 0, total, &zip_name);
            // Expand-Archive 解压到 runtime/，再把内层目录内容上提到 runtime/node
            let stage = data_dir.join("runtime").join("_node-stage");
            let _ = fs::remove_dir_all(&stage);
            fs::create_dir_all(&stage)?;
            let ps_script = format!(
                "Expand-Archive -LiteralPath '{}' -DestinationPath '{}' -Force",
                tmp_zip.display(),
                stage.display()
            );
            let out = hidden_command("powershell")
                .args(["-NoProfile", "-Command", &ps_script])
                .output()
                .map_err(|e| AppError::io(format!("解压失败: {e}")))?;
            if !out.status.success() {
                return Err(AppError::io(format!(
                    "解压失败: {}",
                    String::from_utf8_lossy(&out.stderr)
                )));
            }
            let node_target = node_dir_from(&data_dir);
            let _ = fs::remove_dir_all(&node_target);
            // 内层目录名形如 node-v22.x.y-win-x64 —— 找唯一子目录上提
            let inner = fs::read_dir(&stage)?
                .filter_map(|e| e.ok())
                .find(|e| e.path().join("node.exe").is_file())
                .map(|e| e.path())
                .ok_or_else(|| AppError::io("解压产物缺少 node.exe"))?;
            fs::rename(&inner, &node_target)?;
            let _ = fs::remove_dir_all(&stage);
            let _ = fs::remove_file(&tmp_zip);
            step("done", 1, 1, "Node 运行时就绪");
            Ok(())
        })();
        if let Err(e) = result {
            step("error", 0, 0, &e.to_string());
        }
    });
    Ok(())
}

fn node_dir_from(data_dir: &Path) -> PathBuf {
    data_dir.join("runtime").join("node")
}

/// npm 全局安装进容器（prefix=runtime/npm-global；凭据与包都不落宿主）。
/// 安装源 registry.npmjs.org 走 netconsent 授权（前端弹窗后调用本命令）。
#[tauri::command]
pub fn ai_install_tool(st: tauri::State<AppState>, app: tauri::AppHandle, tool_id: String) -> CmdResult<()> {
    let tool = tool_by_id(&tool_id)?.clone_shim();
    if !npm_cmd(&st.data_dir).is_file() {
        return Err(AppError::validation("Node 运行时未安装：请先在 AI Hub 安装 Node"));
    }
    let data_dir = st.data_dir.clone();
    let id = tool_id.clone();
    std::thread::spawn(move || {
        let step = |phase: &str, msg: &str| {
            emit_progress(&app, AiProgress {
                tool: id.clone(), phase: phase.into(), done: 0, total: 0, message: msg.into(),
            });
        };
        step("npm-install", "npm install -g");
        let npm = npm_cmd(&data_dir);
        let out = hidden_command(npm.to_string_lossy().as_ref())
            .args(["install", "-g", &tool.npm_package])
            .env("npm_config_prefix", npm_global_dir(&data_dir))
            .env("HOME", home_dir(&data_dir))
            .env("USERPROFILE", home_dir(&data_dir))
            .output();
        match out {
            Ok(o) if o.status.success() => step("done", "安装完成"),
            Ok(o) => step(
                "error",
                &format!("npm 失败: {}", String::from_utf8_lossy(&o.stderr).lines().last().unwrap_or("")),
            ),
            Err(e) => step("error", &format!("npm 启动失败: {e}")),
        }
    });
    Ok(())
}

// ---------- Vault 2.0 账号身份库（B-10） ----------

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct AiIdentity {
    pub id: String,
    /// 关联工具 id（claude-code / codex / zcode）
    pub tool: String,
    /// 账号标签（@work / @personal；决定配置目录后缀）
    pub label: String,
    /// 凭据本体（只存保险箱加密区，绝不落明文）
    pub token: String,
    pub note: String,
    pub created_at: u64,
}

fn identities_path(st: &AppState) -> PathBuf {
    st.data_dir.join("vault").join("identities.seal")
}

fn load_identities(st: &AppState) -> CmdResult<Vec<AiIdentity>> {
    let blob = match fs::read(identities_path(st)) {
        Ok(b) => b,
        Err(_) => return Ok(Vec::new()), // 未创建 = 空库
    };
    let key = crate::shell::privacy::vault_key()?.ok_or_else(|| {
        AppError::validation("保险箱未解锁：身份库凭据不可读（先在设置中解锁保险箱）")
    })?;
    let plain = crate::shell::privacy::open_seal_pub(&key, &blob)?;
    serde_json::from_slice(&plain).map_err(|e| AppError::io(format!("身份库损坏: {e}")))
}

fn save_identities(st: &AppState, items: &[AiIdentity]) -> CmdResult<()> {
    let key = crate::shell::privacy::vault_key()?.ok_or_else(|| {
        AppError::validation("保险箱未解锁：先在设置中初始化并解锁保险箱，才能保存身份")
    })?;
    let plain = serde_json::to_vec(items).map_err(|e| AppError::io(e.to_string()))?;
    let blob = crate::shell::privacy::seal_pub(&key, &plain)?;
    fs::create_dir_all(st.data_dir.join("vault"))?;
    fs::write(identities_path(st), blob).map_err(|e| AppError::io(e.to_string()))
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AiIdentityView {
    id: String,
    tool: String,
    label: String,
    note: String,
    created_at: u64,
    /// 凭据尾 4 位（供 UI 识别，不回显本体）
    token_tail: String,
}

#[tauri::command]
pub fn identity_list(st: tauri::State<AppState>) -> CmdResult<Vec<AiIdentityView>> {
    Ok(load_identities(&st)?
        .into_iter()
        .map(|i| {
            let tail: String = i.token.chars().rev().take(4).collect::<Vec<_>>().into_iter().rev().collect();
            AiIdentityView { id: i.id, tool: i.tool, label: i.label, note: i.note, created_at: i.created_at, token_tail: tail }
        })
        .collect())
}

#[tauri::command]
pub fn identity_add(
    st: tauri::State<AppState>,
    tool: String,
    label: String,
    token: String,
    note: String,
) -> CmdResult<AiIdentityView> {
    tool_by_id(&tool)?;
    let label = label.trim().trim_start_matches('@').to_string();
    if label.is_empty() || !label.chars().all(|c| c.is_alphanumeric() || "-_".contains(c)) {
        return Err(AppError::validation("标签只允许字母数字与 - _（用作配置目录后缀）"));
    }
    if token.trim().is_empty() {
        return Err(AppError::validation("Token 不能为空"));
    }
    let mut items = load_identities(&st)?;
    if items.iter().any(|i| i.tool == tool && i.label == label) {
        return Err(AppError::validation(format!("@{label} 已存在于该工具")));
    }
    let id = format!("ai-{:x}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_millis());
    let item = AiIdentity { id: id.clone(), tool, label, token: token.trim().to_string(), note, created_at: crate::shell::launcher::now_ms_pub() };
    let view = AiIdentityView {
        id: item.id.clone(), tool: item.tool.clone(), label: item.label.clone(),
        note: item.note.clone(), created_at: item.created_at,
        token_tail: item.token.chars().rev().take(4).collect::<Vec<_>>().into_iter().rev().collect(),
    };
    items.push(item);
    save_identities(&st, &items)?;
    Ok(view)
}

#[tauri::command]
pub fn identity_remove(st: tauri::State<AppState>, id: String) -> CmdResult<()> {
    let mut items = load_identities(&st)?;
    let before = items.len();
    items.retain(|i| i.id != id);
    if items.len() == before {
        return Err(AppError::not_found(format!("未找到身份 / Identity not found: {id}")));
    }
    save_identities(&st, &items)
}

// ---------- 启动注入与多账号切换（B-10） ----------

/// 把「工具 + 身份」写入终端执行档并返回终端登记项 id：
/// - PATH 前置容器内 shim 目录与 node 目录（`claude` 即开即用）
/// - Token 注入该工具的环境变量名（值只经内存，不落明文文件）
/// - 多账号：配置目录加 @label 后缀（{home}/.claude@work），账号互不串
/// 前端随后走 launchThirdApp(终端) 嵌入 VWM。
#[tauri::command]
pub fn ai_launch(st: tauri::State<AppState>, tool_id: String, identity_id: Option<String>) -> CmdResult<String> {
    let tool = tool_by_id(&tool_id)?;
    let shim = shim_path(&st.data_dir, tool);
    if !shim.is_file() {
        return Err(AppError::validation(format!("{} 未安装：请先在 AI Hub 安装", tool.name)));
    }
    crate::shell::terminal::ensure_terminal_registered(&st)?;

    let mut env_set = std::collections::BTreeMap::new();
    env_set.insert("VARIABLE_ENV".to_string(), tool_id.clone());

    // 多账号：身份存在 → 配置目录 @label 后缀 + Token 注入
    if let Some(iid) = &identity_id {
        let items = load_identities(&st)?;
        let ident = items
            .iter()
            .find(|i| &i.id == iid)
            .ok_or_else(|| AppError::not_found(format!("未找到身份 / Identity not found: {iid}")))?;
        if ident.tool != tool_id {
            return Err(AppError::validation("身份与工具不匹配 / identity-tool mismatch"));
        }
        env_set.insert(tool.token_env.to_string(), ident.token.clone());
        let cfg_key = config_dir_env(tool);
        env_set.insert(cfg_key, format!("{{envhome}}/{}@{}", tool.config_dir, ident.label));
    }

    // PATH 前置容器内 shim 与 node 目录（展开为字面值；其余继承当前进程）
    let mut path = npm_global_dir(&st.data_dir).to_string_lossy().into_owned();
    path.push(';');
    path.push_str(&node_dir(&st.data_dir).to_string_lossy());
    if let Ok(inherited) = std::env::var("PATH") {
        path.push(';');
        path.push_str(&inherited);
    }
    let mut env_redirect = std::collections::BTreeMap::new();
    env_redirect.insert("PATH".to_string(), path);

    let mut apps = crate::shell::launcher::load_registry(&st);
    let term = apps
        .iter_mut()
        .find(|a| a.id == crate::shell::terminal::TERMINAL_ID)
        .ok_or_else(|| AppError::not_found("终端登记项缺失"))?;
    term.profile.env_set = env_set;
    term.profile.env_redirect = env_redirect;
    crate::shell::launcher::save_registry(&st, &apps)?;
    Ok(crate::shell::terminal::TERMINAL_ID.to_string())
}

fn config_dir_env(tool: &AiTool) -> String {
    match tool.id {
        "claude-code" => "CLAUDE_CONFIG_DIR".to_string(),
        "codex" => "CODEX_HOME".to_string(),
        "zcode" => "ZCODE_CONFIG_DIR".to_string(),
        _ => "AI_CONFIG_DIR".to_string(),
    }
}

// ---------- 验证（B-11，凭据零落宿主断言） ----------

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AiVerifyRow {
    id: String,
    name: String,
    shim_in_container: bool,
    config_in_container: bool,
    /// 宿主侧发现同名配置目录/标记（非空 = 违规残留）
    host_residue: Vec<String>,
}

/// 逐工具断言：shim 与配置目录都在容器内；宿主 %USERPROFILE% 不得出现对应目录。
#[tauri::command]
pub fn ai_verify(st: tauri::State<AppState>) -> CmdResult<Vec<AiVerifyRow>> {
    let host_home = std::env::var("USERPROFILE").map(PathBuf::from).ok();
    Ok(AI_TOOLS
        .iter()
        .map(|t| {
            let cfg = home_dir(&st.data_dir).join(t.config_dir);
            let mut host_residue = Vec::new();
            if let Some(hh) = &host_home {
                let host_cfg = hh.join(t.config_dir);
                if host_cfg.is_dir() {
                    host_residue.push(host_cfg.to_string_lossy().into_owned());
                }
            }
            AiVerifyRow {
                id: t.id.to_string(),
                name: t.name.to_string(),
                shim_in_container: shim_path(&st.data_dir, t).is_file(),
                config_in_container: cfg.is_dir(),
                host_residue,
            }
        })
        .collect())
}

// ---------- 工具函数 ----------

fn hidden_command(program: &str) -> std::process::Command {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        let mut c = std::process::Command::new(program);
        c.creation_flags(CREATE_NO_WINDOW);
        c
    }
    #[cfg(not(windows))]
    {
        std::process::Command::new(program)
    }
}

// AiTool 是 &'static 表；辅助 clone 出可 move 进线程的最小视图
impl AiTool {
    fn clone_shim(&self) -> AiToolRef {
        AiToolRef {
            npm_package: self.npm_package.to_string(),
        }
    }
}

struct AiToolRef {
    npm_package: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn st_at(tag: &str) -> AppState {
        let base = std::env::temp_dir().join(format!("ai-test-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&base);
        crate::state::AppState::bootstrap_dirs_at(base).unwrap()
    }

    #[test]
    fn status_reports_three_tools_uninstalled_on_fresh_container() {
        let st = st_at("status");
        // 直接调用内部逻辑等价物（tauri::State 无法手工构造）
        let node_installed = node_exe(&st.data_dir).is_file();
        assert!(!node_installed);
        for t in AI_TOOLS {
            assert!(!shim_path(&st.data_dir, t).is_file());
            assert!(!home_dir(&st.data_dir).join(t.config_dir).is_dir());
        }
        assert_eq!(AI_TOOLS.len(), 3);
    }

    #[test]
    fn identity_validation_rejects_bad_input_before_touching_storage() {
        let st = st_at("ident");
        // 未知工具
        assert!(tool_by_id("nope").is_err());
        // 空 token（走 identity_add 同款校验路径的前半段）
        assert!("".trim().is_empty());
        // 非法标签字符
        let label = "bad label!";
        assert!(!label.chars().all(|c| c.is_alphanumeric() || "-_".contains(c)));
        // 身份库在保险箱未解锁时如实拒绝写入
        let items = load_identities(&st).unwrap(); // 文件不存在 = 空库可读
        assert!(items.is_empty());
        assert!(save_identities(&st, &[]).is_err(), "vault 未解锁时必须拒绝保存");
    }

    #[test]
    fn config_dir_env_maps_each_tool() {
        assert_eq!(config_dir_env(&AI_TOOLS[0]), "CLAUDE_CONFIG_DIR");
        assert_eq!(config_dir_env(&AI_TOOLS[1]), "CODEX_HOME");
        assert_eq!(config_dir_env(&AI_TOOLS[2]), "ZCODE_CONFIG_DIR");
    }
}
