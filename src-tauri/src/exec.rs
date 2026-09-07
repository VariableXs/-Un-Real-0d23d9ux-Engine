//! L2 隔离执行档（Isolation Profile，批次 B-4/B-5/B-6，BLUEPRINT 3.3 / 7.2）：
//! - 一切受管进程经 `spawn_profiled` 启动：环境变量重定向进容器（{container}/{home} 占位符展开）
//! - 凭据零落宿主的统一底座：HOME/USERPROFILE/各工具 CONFIG_DIR 指向容器内镜像
//! - 模板库 v1：Claude Code / Codex / ZCode / Git / Node（B-5）
//! - 残留扫描器：会话基线差集，验证「宿主零残留」承诺（B-6）
//! - 如实边界：执行档 = 环境变量级隔离；对不走 Command 通道的 .lnk（ShellExecute）
//!   无法注入环境，该路径维持旧行为并在 UI 标注（见 launcher::tp_launch_inner）。

use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use crate::error::{AppError, CmdResult};
use crate::state::AppState;

// ---------- 执行档模型（B-3 登记 v2 的核心结构） ----------

/// 隔离执行档：登记表 apps.json v2 新增字段（全部 serde default，v1 文件平滑升级）。
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PortableProfile {
    /// 强制重定向表（值支持 {container}/{home} 占位符），对子进程递归生效
    #[serde(default)]
    pub env_redirect: BTreeMap<String, String>,
    /// 附加设置（不重定向宿主值，直接注入）
    #[serde(default)]
    pub env_set: BTreeMap<String, String>,
    /// 出站白名单建议（M8 网络层启用前仅登记，不执行）
    #[serde(default)]
    pub net_allow: Vec<String>,
    /// 敏感档：残留扫描/剪贴板策略联动标记
    #[serde(default)]
    pub sensitive: bool,
}

impl PortableProfile {
    pub fn is_empty(&self) -> bool {
        self.env_redirect.is_empty() && self.env_set.is_empty() && self.net_allow.is_empty() && !self.sensitive
    }
}

/// spawn 时的执行档视图（BLUEPRINT 7.2 ExecProfile 的运行时形态）。
#[derive(Clone, Debug, Default)]
pub struct ExecProfile {
    pub id: String,
    pub env_redirect: BTreeMap<String, String>,
    pub env_set: BTreeMap<String, String>,
    pub net_allow: Vec<String>,
    pub sensitive: bool,
}

impl ExecProfile {
    pub fn is_empty(&self) -> bool {
        self.env_redirect.is_empty() && self.env_set.is_empty() && self.net_allow.is_empty() && !self.sensitive
    }
}

impl From<(&str, PortableProfile)> for ExecProfile {
    fn from((id, p): (&str, PortableProfile)) -> Self {
        ExecProfile {
            id: id.to_string(),
            env_redirect: p.env_redirect,
            env_set: p.env_set,
            net_allow: p.net_allow,
            sensitive: p.sensitive,
        }
    }
}

// ---------- 占位符展开 ----------

/// `{container}` = 数据目录（Uxv 容器落地后的容器根）；`{home}` = {container}/home。
pub fn expand_placeholders(raw: &str, container_root: &Path) -> String {
    raw.replace("{container}", &container_root.to_string_lossy())
        .replace("{home}", &container_root.join("home").to_string_lossy())
        .replace("{envhome}", &env_home(container_root).to_string_lossy())
}

/// B-24：当前活动环境档的 home 根。无 envs.json / active=main / 缺目录时
/// 回退容器全局 home——默认环境行为与 B-3…B-23 完全一致。
pub fn env_home(container_root: &Path) -> PathBuf {
    let registry = container_root.join("envs.json");
    let Ok(bytes) = std::fs::read(&registry) else {
        return container_root.join("home");
    };
    let Ok(v) = serde_json::from_slice::<serde_json::Value>(&bytes) else {
        return container_root.join("home");
    };
    let active = v.get("active").and_then(|x| x.as_str()).unwrap_or("main");
    if active == "main" {
        return container_root.join("home");
    }
    let dir = container_root.join("envs").join(active).join("home");
    if dir.is_dir() {
        dir
    } else {
        container_root.join("home")
    }
}

/// 计算将要注入的完整环境表（干跑「验证重定向」与真实 spawn 共用同一实现）。
pub fn env_map(profile: &ExecProfile, container_root: &Path) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    for (k, v) in &profile.env_redirect {
        out.insert(k.clone(), expand_placeholders(v, container_root));
    }
    for (k, v) in &profile.env_set {
        out.insert(k.clone(), expand_placeholders(v, container_root));
    }
    out
}

/// 校验环境变量名：非空、不含 `=`/NUL（键值都不能带控制字符）。
pub fn valid_env_pair(k: &str, v: &str) -> bool {
    !k.is_empty() && !k.contains(['=', '\0']) && !v.contains('\0')
}

/// 注入执行档并启动（唯一受管进程入口）。失败如实上抛，由调用方降级旧通道。
pub fn spawn_profiled(
    container_root: &Path,
    profile: &ExecProfile,
    program: &Path,
    args: &[String],
) -> std::io::Result<Option<u32>> {
    let mut c = std::process::Command::new(program);
    // 仅在有目录前缀时设置工作目录；"cmd.exe" 这类裸程序名的 parent() 是空路径，
    // 设置空 cwd 会在 CreateProcess 报 InvalidFilename（端到端测试抓到的真实 bug）
    if let Some(parent) = program.parent() {
        if !parent.as_os_str().is_empty() {
            c.current_dir(parent);
        }
    }
    for (k, v) in env_map(profile, container_root) {
        if valid_env_pair(&k, &v) {
            c.env(k, v);
        }
    }
    // B-21：容器工具链 PATH 注入——存在的 runtime 目录前置到 PATH，
    // 终端/AI CLI/浏览器等一切受管进程天然继承容器工具链。
    if let Some(prefix) = runtime_path_prefix(container_root) {
        let inherited = std::env::var("PATH").unwrap_or_default();
        c.env("PATH", format!("{prefix};{inherited}"));
    }
    // B-28：代理运行中时注入 HTTP(S)_PROXY——受管出站汇聚环回代理（白名单执行点）
    if let Some(proxy) = crate::shell::network::proxy_env_value() {
        for k in ["HTTP_PROXY", "HTTPS_PROXY", "http_proxy", "https_proxy"] {
            c.env(k, &proxy);
        }
    }
    for a in args {
        c.arg(a);
    }
    crate::shell::launcher::spawn_detached(&mut c)
}

/// 容器 runtime 工具链目录（存在者才入 PATH 前缀；B-21 冻结顺序）。
pub(crate) fn runtime_path_prefix(container_root: &Path) -> Option<String> {
    const REL: &[&str] = &[
        "runtime/npm-global",
        "runtime/node",
        "runtime/python",
        "runtime/python/Scripts",
        "runtime/go/bin",
        "runtime/cargo/bin",
        "runtime/vscode/bin",
    ];
    let dirs: Vec<String> = REL
        .iter()
        .map(|r| container_root.join(r))
        .filter(|p| p.is_dir())
        .map(|p| p.to_string_lossy().into_owned())
        .collect();
    if dirs.is_empty() {
        None
    } else {
        Some(dirs.join(";"))
    }
}

// ---------- 模板库 v1（B-5，蓝图 3.3.4；14.2：HOME 与 USERPROFILE 指向不同镜像防互踩） ----------

pub struct ProfileTemplate {
    pub id: &'static str,
    pub name: &'static str,
    pub description: &'static str,
    pub env_redirect: &'static [(&'static str, &'static str)],
    pub env_set: &'static [(&'static str, &'static str)],
    /// 建议出站白名单（M8 前仅登记）
    pub net_allow: &'static [&'static str],
    pub sensitive: bool,
}

/// 模板是「首个官方 .uxpack 包」的雏形（附录 I：AI 提供方包，B-37 迁移）。
pub const TEMPLATES: &[ProfileTemplate] = &[
    ProfileTemplate {
        id: "claude-code",
        name: "Claude Code CLI",
        description: "凭据与会话进容器 home/.claude，出站 api.anthropic.com",
        env_redirect: &[
            ("HOME", "{home}"),
            ("USERPROFILE", "{home}"),
            ("CLAUDE_CONFIG_DIR", "{home}/.claude"),
            ("npm_config_cache", "{container}/cache/npm"),
        ],
        env_set: &[("VARIABLE_ENV", "claude-code")],
        net_allow: &["api.anthropic.com"],
        sensitive: true,
    },
    ProfileTemplate {
        id: "codex",
        name: "Codex CLI",
        description: "凭据与会话进容器 home/.codex，出站 api.openai.com",
        env_redirect: &[
            ("HOME", "{home}"),
            ("USERPROFILE", "{home}"),
            ("CODEX_HOME", "{home}/.codex"),
            ("npm_config_cache", "{container}/cache/npm"),
        ],
        env_set: &[("VARIABLE_ENV", "codex")],
        net_allow: &["api.openai.com"],
        sensitive: true,
    },
    ProfileTemplate {
        id: "zcode",
        name: "ZCode CLI",
        description: "凭据与会话进容器 home/.zcode，出站 api.z.ai",
        env_redirect: &[
            ("HOME", "{home}"),
            ("USERPROFILE", "{home}"),
            ("ZCODE_CONFIG_DIR", "{home}/.zcode"),
            ("npm_config_cache", "{container}/cache/npm"),
        ],
        env_set: &[("VARIABLE_ENV", "zcode")],
        net_allow: &["api.z.ai"],
        sensitive: true,
    },
    ProfileTemplate {
        id: "git",
        name: "Git",
        description: "全局配置与凭据进容器（HOME 走 msys 镜像，防与 Node 系互踩）",
        env_redirect: &[
            ("HOME", "{home}/msys"),
            ("USERPROFILE", "{home}"),
            ("GIT_CONFIG_GLOBAL", "{home}/.gitconfig"),
        ],
        env_set: &[("VARIABLE_ENV", "git")],
        net_allow: &[],
        sensitive: true,
    },
    ProfileTemplate {
        id: "node",
        name: "Node.js",
        description: "npm 缓存进容器 cache/npm，全局安装随容器走",
        env_redirect: &[
            ("HOME", "{home}"),
            ("USERPROFILE", "{home}"),
            ("npm_config_cache", "{container}/cache/npm"),
            ("npm_config_prefix", "{container}/runtime/npm-global"),
        ],
        env_set: &[("VARIABLE_ENV", "node")],
        net_allow: &[],
        sensitive: false,
    },
];

fn template_by_id(id: &str) -> Option<&'static ProfileTemplate> {
    TEMPLATES.iter().find(|t| t.id == id)
}

fn template_dto(t: &ProfileTemplate) -> serde_json::Value {
    serde_json::json!({
        "id": t.id,
        "name": t.name,
        "description": t.description,
        "envRedirect": t.env_redirect.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect::<BTreeMap<_, _>>(),
        "envSet": t.env_set.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect::<BTreeMap<_, _>>(),
        "netAllow": t.net_allow,
        "sensitive": t.sensitive,
    })
}

// ---------- 命令面（B-5/B-6） ----------

/// 模板列表（设置页「执行档」标签下拉用）。
#[tauri::command]
pub fn profile_templates() -> CmdResult<Vec<serde_json::Value>> {
    Ok(TEMPLATES.iter().map(template_dto).collect())
}

/// 套用模板：整体覆写该登记项的执行档（UI 已有确认）。
#[tauri::command]
pub fn profile_apply(st: tauri::State<AppState>, id: String, template_id: String) -> CmdResult<crate::shell::launcher::ThirdApp> {
    let tpl = template_by_id(&template_id).ok_or_else(|| {
        AppError::validation(format!("未知模板 / Unknown template: {template_id}"))
    })?;
    let mut apps = crate::shell::launcher::load_registry(&st);
    let app = apps
        .iter_mut()
        .find(|a| a.id == id)
        .ok_or_else(|| AppError::not_found(format!("未找到登记项 / Not found: {id}")))?;
    app.profile = PortableProfile {
        env_redirect: tpl.env_redirect.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect(),
        env_set: tpl.env_set.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect(),
        net_allow: tpl.net_allow.iter().map(|s| s.to_string()).collect(),
        sensitive: tpl.sensitive,
    };
    let out = app.clone();
    crate::shell::launcher::save_registry(&st, &apps)?;
    Ok(out)
}

/// 手工编辑执行档（重定向表/附加表/敏感标记）。
#[tauri::command]
pub fn profile_set(
    st: tauri::State<AppState>,
    id: String,
    env_redirect: BTreeMap<String, String>,
    env_set: BTreeMap<String, String>,
    sensitive: bool,
) -> CmdResult<crate::shell::launcher::ThirdApp> {
    for (k, v) in env_redirect.iter().chain(env_set.iter()) {
        if !valid_env_pair(k, v) {
            return Err(AppError::validation(format!("非法环境变量名 / Invalid env key: {k}")));
        }
    }
    let mut apps = crate::shell::launcher::load_registry(&st);
    let app = apps
        .iter_mut()
        .find(|a| a.id == id)
        .ok_or_else(|| AppError::not_found(format!("未找到登记项 / Not found: {id}")))?;
    app.profile = PortableProfile { env_redirect, env_set, net_allow: app.profile.net_allow.clone(), sensitive };
    let out = app.clone();
    crate::shell::launcher::save_registry(&st, &apps)?;
    Ok(out)
}

/// 干跑：列出将被注入的环境变量（「验证重定向」按钮）。
#[tauri::command]
pub fn profile_dryrun(st: tauri::State<AppState>, id: String) -> CmdResult<serde_json::Value> {
    let apps = crate::shell::launcher::load_registry(&st);
    let app = apps
        .iter()
        .find(|a| a.id == id)
        .ok_or_else(|| AppError::not_found(format!("未找到登记项 / Not found: {id}")))?;
    let profile = ExecProfile::from((app.id.as_str(), app.profile.clone()));
    let root = st.data_dir.clone();
    Ok(serde_json::json!({
        "id": id,
        "name": app.name,
        "sensitive": profile.sensitive,
        "envRedirect": env_map(&profile, &root),
    }))
}

// ---------- 残留扫描器（B-6，BLUEPRINT 3.3.3 / 10.4；E-2 深度扩展） ----------

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ResidueEntry {
    pub path: String,
    pub size: u64,
    pub modified_ms: u64,
    /// E-2：file = 文件落盘；reg = HKCU\Software 新增键（只读 diff）
    #[serde(default = "default_kind")]
    pub kind: String,
}

fn default_kind() -> String {
    "file".to_string()
}

/// 会话基线：环境启动时对宿主观测面（%USERPROFILE% 顶层 + Recent +
/// AppData\Local\Temp + HKCU\Software 顶层子键）做快照。
static RESIDUE_BASELINE: OnceLock<Mutex<HashMap<String, (u64, u64)>>> = OnceLock::new();

fn baseline() -> &'static Mutex<HashMap<String, (u64, u64)>> {
    RESIDUE_BASELINE.get_or_init(|| Mutex::new(HashMap::new()))
}

/// 观测面：仅顶层文件名与 Recent/Temp（已知落盘点），不递归、不读内容——
/// 性能与隐私双保守。注册表只记录 HKCU\Software 顶层子键名（只读 diff）。
fn residue_targets() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Ok(up) = std::env::var("USERPROFILE") {
        dirs.push(PathBuf::from(&up));
    }
    if let Ok(appdata) = std::env::var("APPDATA") {
        dirs.push(PathBuf::from(&appdata).join("Microsoft").join("Windows").join("Recent"));
    }
    if let Ok(local) = std::env::var("LOCALAPPDATA") {
        dirs.push(PathBuf::from(&local).join("Temp"));
    }
    dirs
}

/// E-2：已知无害项差集白名单（默认内置 + 用户自定义，落盘 residue-whitelist.json）。
const BUILTIN_WHITELIST: &[&str] = &[
    // Windows 自身会话波动（thumbcache/图标缓存/最近跳表）
    "thumbcache",
    "iconcache",
    "recentcustomitems",
    "usbstor",
];

fn whitelist_path(st: &AppState) -> PathBuf {
    st.data_dir.join("residue-whitelist.json")
}

fn load_whitelist(st: &AppState) -> Vec<String> {
    let mut out: Vec<String> = BUILTIN_WHITELIST.iter().map(|s| s.to_string()).collect();
    if let Ok(bytes) = std::fs::read(whitelist_path(st)) {
        if let Ok(v) = serde_json::from_slice::<Vec<String>>(&bytes) {
            out.extend(v);
        }
    }
    out
}

/// 白名单命中：路径/键名包含任一模式（大小写不敏感）即视为已知无害。
fn whitelist_hit(st: &AppState, path: &str) -> bool {
    let lower = path.to_ascii_lowercase();
    load_whitelist(st).iter().any(|w| !w.is_empty() && lower.contains(&w.to_ascii_lowercase()))
}

fn snapshot_dir(dir: &Path, out: &mut HashMap<String, (u64, u64)>) {
    let Ok(entries) = fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let Ok(meta) = entry.metadata() else { continue };
        if meta.is_dir() {
            continue; // 顶层只观测文件；目录级观测误报高（系统自身波动）
        }
        let size = meta.len();
        let modified_ms = meta
            .modified()
            .ok()
            .and_then(|m| m.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);
        out.insert(entry.path().to_string_lossy().into_owned(), (size, modified_ms));
    }
}

/// E-2：HKCU\Software 顶层子键快照（只读 diff；键名以 reg: 前缀入图，值为占位）。
#[cfg(windows)]
fn snapshot_hkcu_software(out: &mut HashMap<String, (u64, u64)>) {
    use winreg::enums::{HKEY_CURRENT_USER, KEY_READ};
    let hk = winreg::RegKey::predef(HKEY_CURRENT_USER);
    if let Ok(software) = hk.open_subkey_with_flags("Software", KEY_READ) {
        for name in software.enum_keys().flatten() {
            out.insert(format!("reg:HKCU\\Software\\{name}"), (0, 0));
        }
    }
}

fn residue_snapshot() -> HashMap<String, (u64, u64)> {
    let mut out = HashMap::new();
    for dir in residue_targets() {
        snapshot_dir(&dir, &mut out);
    }
    #[cfg(windows)]
    snapshot_hkcu_software(&mut out);
    out
}

/// 环境启动时调用（lib.rs setup）。已有基线则覆盖（重开环境=新会话）。
pub fn residue_baseline_take() {
    let snap = residue_snapshot();
    if let Ok(mut guard) = baseline().lock() {
        *guard = snap;
    }
}

/// E-2 差集：本次会话在宿主观测面的新增/变化（文件 + HKCU 新键），
/// 已知无害项（内置 + 用户白名单）不报。
pub fn residue_snapshot_diff_st(st: &AppState) -> Vec<ResidueEntry> {
    let now = residue_snapshot();
    if let Ok(guard) = baseline().lock() {
        residue_diff(&guard, &now, st)
    } else {
        Vec::new()
    }
}

/// 可测纯函数：baseline → now 的差集（新增或 size/mtime 变化，白名单过滤）。
fn residue_diff(
    guard: &HashMap<String, (u64, u64)>,
    now: &HashMap<String, (u64, u64)>,
    st: &AppState,
) -> Vec<ResidueEntry> {
    let mut hits = Vec::new();
    for (path, (size, mtime)) in now {
        let is_new_or_changed = match guard.get(path) {
            None => true,
            Some((bsize, bmtime)) => bsize != size || bmtime != mtime,
        };
        if is_new_or_changed && !whitelist_hit(st, path) {
            let kind = if path.starts_with("reg:") { "reg" } else { "file" };
            hits.push(ResidueEntry {
                path: path.clone(),
                size: *size,
                modified_ms: *mtime,
                kind: kind.to_string(),
            });
        }
    }
    hits.sort_by(|a, b| a.path.cmp(&b.path));
    hits
}

#[tauri::command]
pub fn residue_scan(st: tauri::State<AppState>) -> CmdResult<Vec<ResidueEntry>> {
    Ok(residue_snapshot_diff_st(&st))
}

/// E-2：清理单条残留。文件 → 删除；注册表 → 删除 HKCU\Software 新增子键
/// （只允许清 diff 里出现的键，且只删顶层——用户已确认，不递归值）。
#[tauri::command]
pub fn residue_resolve(st: tauri::State<AppState>, path: String) -> CmdResult<()> {
    if whitelist_hit(&st, &path) {
        return Err(AppError::validation("该项在白名单内，无需清理"));
    }
    if let Some(sub) = path.strip_prefix("reg:HKCU\\Software\\") {
        // 防误删：只允许一级子键名，且必须是基线中不存在的新键
        if sub.contains('\\') || sub.contains('/') || sub.is_empty() {
            return Err(AppError::validation("只允许清理 HKCU\\Software 顶层新增键"));
        }
        if baseline().lock().map(|g| g.contains_key(&path)).unwrap_or(false) {
            return Err(AppError::validation("该键在会话基线中已存在，不属于本次会话残留"));
        }
        #[cfg(windows)]
        {
            use winreg::enums::{HKEY_CURRENT_USER, KEY_WRITE};
            let hk = winreg::RegKey::predef(HKEY_CURRENT_USER);
            let software = hk
                .open_subkey_with_flags("Software", KEY_WRITE)
                .map_err(|e| AppError::io(format!("打开注册表失败: {e}")))?;
            software
                .delete_subkey(sub)
                .map_err(|e| AppError::io(format!("删除注册表键失败: {e}")))?;
        }
        return Ok(());
    }
    // 文件残留：只删除，不改名不移动；失败如实上抛
    let p = PathBuf::from(&path);
    if p.is_file() {
        fs::remove_file(&p).map_err(|e| AppError::io(format!("删除残留文件失败: {e}")))?;
        Ok(())
    } else {
        Err(AppError::not_found(format!("残留项已不存在或类型不支持: {path}")))
    }
}

/// E-2：把路径/键名片段加入用户白名单（内置白名单不可移除，如实声明）。
fn residue_whitelist_add_inner(st: &AppState, pattern: &str) -> CmdResult<Vec<String>> {
    let pattern = pattern.trim().to_string();
    if pattern.len() < 3 {
        return Err(AppError::validation("白名单片段太短（≥3 字符），避免误放行"));
    }
    let mut user: Vec<String> = std::fs::read(whitelist_path(st))
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default();
    if !user.iter().any(|w| w.eq_ignore_ascii_case(&pattern)) {
        user.push(pattern);
        let bytes = serde_json::to_vec_pretty(&user)
            .map_err(|e| AppError::io(format!("序列化白名单失败: {e}")))?;
        fs::write(whitelist_path(st), bytes)
            .map_err(|e| AppError::io(format!("写入白名单失败: {e}")))?;
    }
    Ok(user)
}

#[tauri::command]
pub fn residue_whitelist_add(st: tauri::State<AppState>, pattern: String) -> CmdResult<Vec<String>> {
    residue_whitelist_add_inner(&st, &pattern)
}

/// E-2：当前生效的白名单（内置 + 用户）。
#[tauri::command]
pub fn residue_whitelist_list(st: tauri::State<AppState>) -> CmdResult<serde_json::Value> {
    let user: Vec<String> = std::fs::read(whitelist_path(&st))
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default();
    Ok(serde_json::json!({
        "builtin": BUILTIN_WHITELIST,
        "user": user,
    }))
}

// ---------- E-3 退出总时序（蓝图 11.4；16.3） ----------
//
// 后端负责「容器 checkpoint → 断代理 → 残留扫描」三步（广播保存 / VWM 冲刷 /
// 嵌入应用 WM_CLOSE 由前端 requestClose 既有编排先行）；每步超时/失败只记录
// 不中断——退出路径绝不因清理失败而被卡死（跳过路径：用户可对残留报告点
// 「我知道风险」后仍退出）。

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ExitStepReport {
    pub step: String,
    pub ok: bool,
    pub detail: String,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ExitPrepReport {
    pub steps: Vec<ExitStepReport>,
    /// 非空 = 有宿主残留（前端弹报告；用户可跳过退出）
    pub residues: Vec<ResidueEntry>,
}

/// E-3 退出前置：逐步执行并如实回报。前端在关窗前调用；
/// 返回后由前端决定「弹残留报告（可跳过）」或直接关壳。
#[tauri::command]
pub fn exit_prepare(st: tauri::State<AppState>) -> CmdResult<ExitPrepReport> {
    let mut steps = Vec::new();

    // 1) 容器 checkpoint：WAL 固化（数据落盘一致；失败不阻塞退出）
    let ckpt = st.with_conn(|conn| {
        conn.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);").map_err(AppError::from)
    });
    steps.push(ExitStepReport {
        step: "checkpoint".into(),
        ok: ckpt.is_ok(),
        detail: match ckpt {
            Ok(()) => "WAL 已固化".into(),
            Err(e) => format!("checkpoint 失败（已跳过）: {e}"),
        },
    });

    // 2) 断代理：停环回代理 + 清注入环境（失败不阻塞退出）
    let proxy = crate::shell::network::net_proxy_stop();
    steps.push(ExitStepReport {
        step: "net-off".into(),
        ok: proxy.is_ok(),
        detail: match proxy {
            Ok(()) => "代理已停止".into(),
            Err(e) => format!("停止代理失败（已跳过）: {e}"),
        },
    });

    // 3) 残留扫描：0 残留静默通过；非零交前端弹报告（可跳过）
    let residues = residue_snapshot_diff_st(&st);
    steps.push(ExitStepReport {
        step: "residue-scan".into(),
        ok: true,
        detail: if residues.is_empty() { "零残留".into() } else { format!("{} 项残留", residues.len()) },
    });

    Ok(ExitPrepReport { steps, residues })
}


#[cfg(test)]
mod path_tests {
    use super::*;

    #[test]
    fn runtime_path_prefix_only_existing_dirs() {
        let root = std::env::temp_dir().join(format!("exec-path-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        // 无 runtime 目录 → None
        assert!(runtime_path_prefix(&root).is_none());
        // 建 node 与 go/bin → 两个目录入前缀，且顺序符合冻结清单
        std::fs::create_dir_all(root.join("runtime/node")).unwrap();
        std::fs::create_dir_all(root.join("runtime/go/bin")).unwrap();
        let p = runtime_path_prefix(&root).unwrap();
        let node = root.join("runtime/node").to_string_lossy().into_owned();
        let gobin = root.join("runtime/go/bin").to_string_lossy().into_owned();
        assert!(p.contains(&node), "prefix={p}");
        assert!(p.contains(&gobin), "prefix={p}");
        assert!(p.find(&node).unwrap() < p.find(&gobin).unwrap(), "冻结顺序：npm-global/node 在前");
        // python 未建 → 不出现
        assert!(!p.contains("runtime/python"));
        let _ = std::fs::remove_dir_all(&root);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn placeholders_expand_to_container_paths() {
        let root = std::env::temp_dir();
        let root = root.as_path();
        let expected_home = format!("{}{}", root.join("home").to_string_lossy(), "/.claude");
        assert_eq!(expand_placeholders("{home}/.claude", root), expected_home);
        // {container} 是纯字符串替换（不追加主分隔符）——这正是模板值可写
        // "{home}/.claude" 这类带 / 后缀路径的原因
        let expected_cache = format!("{}/cache", root.display());
        assert_eq!(expand_placeholders("{container}/cache", root), expected_cache);
        assert_eq!(expand_placeholders("plain", root), "plain");
    }

    #[test]
    fn env_map_expands_both_sections() {
        let mut redirect = BTreeMap::new();
        redirect.insert("HOME".to_string(), "{home}".to_string());
        let mut set = BTreeMap::new();
        set.insert("VARIABLE_ENV".to_string(), "x".to_string());
        let p = ExecProfile {
            id: "t".into(),
            env_redirect: redirect,
            env_set: set,
            net_allow: vec![],
            sensitive: false,
        };
        let root = Path::new("/c");
        let m = env_map(&p, root);
        // {home} 经 join 追加主分隔符（Windows 下为 \），与实现一致
        assert_eq!(m.get("HOME").unwrap(), &root.join("home").to_string_lossy().to_string());
        assert_eq!(m.get("VARIABLE_ENV").unwrap(), "x");
    }

    #[test]
    fn v1_registry_json_upgrades_with_default_profile() {
        // apps.json v1（无 profile 字段）→ 反序列化得到空执行档 = 平滑迁移
        let v1 = r#"[{"id":"tp-1","name":"Old","path":"C:/x/app.exe","grade":"standalone","addedAt":1,"lastLaunch":null,"icon":null,"target":null}]"#;
        let apps: Vec<crate::shell::launcher::ThirdApp> = serde_json::from_str(v1).unwrap();
        assert_eq!(apps.len(), 1);
        assert!(apps[0].profile.is_empty());
        // v2 roundtrip
        let v2 = serde_json::to_string(&apps).unwrap();
        let back: Vec<crate::shell::launcher::ThirdApp> = serde_json::from_str(&v2).unwrap();
        assert_eq!(back[0].profile, apps[0].profile);
    }

    #[test]
    fn templates_have_valid_keys_and_unique_ids() {
        let mut ids = std::collections::BTreeSet::new();
        for t in TEMPLATES {
            assert!(ids.insert(t.id), "duplicate template id {}", t.id);
            for (k, v) in t.env_redirect.iter().chain(t.env_set.iter()) {
                assert!(valid_env_pair(k, v), "bad env pair {}={}", k, v);
                assert!(v.contains("{home}") || v.contains("{container}") || v.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_'), "unexpected literal value {}={}", k, v);
            }
        }
        assert!(template_by_id("claude-code").is_some());
        assert!(template_by_id("nope").is_none());
    }

    #[test]
    fn invalid_env_names_rejected() {
        assert!(!valid_env_pair("", "x"));
        assert!(!valid_env_pair("A=B", "x"));
        assert!(valid_env_pair("HOME", "{home}"));
    }

    /// 端到端（M1 验收核心断言）：经 spawn_profiled 启动的真实子进程，
    /// 其环境里 {home} 已展开为容器路径——凭据落盘隔离的机制证明。
    #[test]
    #[cfg(windows)]
    fn spawn_profiled_injects_environment() {
        let tmp = std::env::temp_dir().join(format!("exec-e2e-{}", std::process::id()));
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(&tmp).unwrap();

        let mut redirect = BTreeMap::new();
        redirect.insert("VARIABLE_TEST_HOME".to_string(), "{home}".to_string());
        let profile = ExecProfile {
            id: "e2e".into(),
            env_redirect: redirect,
            env_set: Default::default(),
            net_allow: vec![],
            sensitive: false,
        };
        let out = tmp.join("out.txt");
        // 不加引号：cmd /c 以单参数接收时嵌套引号会被拆坏；temp 路径无空格
        let script = format!("set VARIABLE_TEST_HOME>{}", out.display());
        let pid = spawn_profiled(&tmp, &profile, Path::new("cmd.exe"), &["/c".to_string(), script])
            .expect("spawn failed");
        assert!(pid.is_some());

        let expected = format!("VARIABLE_TEST_HOME={}", tmp.join("home").display());
        let mut content = String::new();
        for _ in 0..50 {
            if let Ok(c) = fs::read_to_string(&out) {
                content = c;
                if content.contains("VARIABLE_TEST_HOME=") {
                    break;
                }
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
        assert!(content.contains(&expected), "injected env mismatch; got: {content:?}; expected: {expected:?}");
        let _ = fs::remove_dir_all(&tmp);
    }

    // ---- E-2 残留差集（可测纯函数）----

    fn st_at(dir: &Path) -> crate::state::AppState {
        crate::state::AppState::bootstrap_at(dir.to_path_buf()).expect("bootstrap test state")
    }

    #[test]
    fn residue_diff_reports_new_changed_and_filters_whitelist() {
        let tmp = std::env::temp_dir().join(format!("residue-diff-{}", std::process::id()));
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(&tmp).unwrap();
        let st = st_at(&tmp);

        let base = HashMap::new();
        let mut now = HashMap::new();
        now.insert(
            format!("{}\\AppData\\Local\\Temp\\thumbcache_1.db", tmp.display()),
            (10u64, 1u64),
        );
        now.insert(format!("{}\\AppData\\Local\\Temp\\leftover.cfg", tmp.display()), (5, 2));
        now.insert("reg:HKCU\\Software\\FakeVendor".to_string(), (0, 0));

        let hits = residue_diff(&base, &now, &st);
        let paths: Vec<&str> = hits.iter().map(|h| h.path.as_str()).collect();
        // thumbcache 命中内置白名单不报；leftover + 注册表新键均报，kind 如实
        assert_eq!(paths.len(), 2, "hits: {paths:?}");
        let leftover = hits.iter().find(|h| h.path.ends_with("leftover.cfg")).unwrap();
        assert_eq!(leftover.kind, "file");
        let reg = hits.iter().find(|h| h.path.starts_with("reg:")).unwrap();
        assert_eq!(reg.kind, "reg");

        // 基线相同条目（size/mtime 一致）不报
        let mut base2 = now.clone();
        base2.insert(format!("{}\\AppData\\Local\\Temp\\leftover.cfg", tmp.display()), (5, 2));
        let hits2 = residue_diff(&base2, &now, &st);
        assert!(hits2.is_empty(), "unchanged entries must not be reported: {:?}", hits2);

        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn residue_whitelist_roundtrip_and_min_length() {
        let tmp = std::env::temp_dir().join(format!("residue-wl-{}", std::process::id()));
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(&tmp).unwrap();
        let st = st_at(&tmp);

        assert!(residue_whitelist_add_inner(&st, "ab").is_err(), "short pattern rejected");
        let user = residue_whitelist_add_inner(&st, "fakevendor").unwrap();
        assert!(user.iter().any(|w| w.eq_ignore_ascii_case("fakevendor")));
        // 幂等
        let user2 = residue_whitelist_add_inner(&st, "FakeVendor").unwrap();
        assert_eq!(user2.iter().filter(|w| w.eq_ignore_ascii_case("fakevendor")).count(), 1);
        // 生效白名单 = 内置 + 用户
        let all = load_whitelist(&st);
        assert!(all.iter().any(|w| w == "thumbcache"));
        assert!(all.iter().any(|w| w.eq_ignore_ascii_case("fakevendor")));
        let _ = fs::remove_dir_all(&tmp);
    }
}
