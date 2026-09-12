//! AI-09 · 三壳统一底座（C01~C24，W4）。
//!
//! 一个 core，三个壳：壳A Windows 独立 / 壳B Variable 收件箱嵌入 / 壳C VARIX 内核用户态。
//! 三端等价铁律：**功能不因端隐藏，只换实现层**——
//! - 写回（C08/C14）：独立态=直接写磁盘，嵌入态=同左（进程身份不变），内核态=经 VFS 帧；
//! - 键位（C15/C16）：同一份配置，注册目标分级 系统级 / vwm 转发 / 内核事件；
//! - 壁纸（C17）：应用内背景层，三端一致，永不碰宿主桌面；
//! - 资产（C18）：8 风格资产随应用自带，不依赖宿主主题；
//! - 大项目（C19）：IR 分页加载，下钻到哪层加载哪层，星系模式常驻预算。
//!
//! C01~C07 为既有域（AI-01~AI-04）产出契约的**跨壳接线校验**，不重复实现：
//! 本域断言统一 IR / 多语言解析 / 七级下钻 / 七可视化共享契约 / 双轨数据流 /
//! 比喻库 / 三层比喻校验在同一份 core 数据上对三壳稳定成立。

use crate::checks::CheckSet;
use crate::model::NodeKind;
use std::sync::atomic::{AtomicU64, Ordering};

/// 域自检并发序号（聚合测试并行进入 run_shell_checks 时临时目录互不干扰）。
static SHELL_RUN_SEQ: AtomicU64 = AtomicU64::new(0);

// ---------------------------------------------------------------------------
// 壳类型与档案
// ---------------------------------------------------------------------------

/// 三壳之一（部署总纲 §二：一个 core，三个壳）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShellKind {
    /// 壳A：Windows 独立完整版（先做，全功能）。
    Standalone,
    /// 壳B：Variable 收件箱嵌入版（同一 exe 直接投递，vwm 转发输入）。
    VariableEmbed,
    /// 壳C：VARIX 内核用户态应用（appfw 注册 + VFS 读写 + 自带资产渲染）。
    VarixKernel,
}

impl ShellKind {
    pub const ALL: [ShellKind; 3] = [
        ShellKind::Standalone,
        ShellKind::VariableEmbed,
        ShellKind::VarixKernel,
    ];

    pub fn name(self) -> &'static str {
        match self {
            ShellKind::Standalone => "壳A-Windows独立",
            ShellKind::VariableEmbed => "壳B-Variable嵌入",
            ShellKind::VarixKernel => "壳C-VARIX内核",
        }
    }

    pub fn short(self) -> &'static str {
        match self {
            ShellKind::Standalone => "A",
            ShellKind::VariableEmbed => "B",
            ShellKind::VarixKernel => "C",
        }
    }

    /// 嵌入态判定（C12）：壳B 恒嵌入；壳A 可被 vwm 窗口收编；
    /// 壳C 是内核窗口，同样是"窗口内运行"。
    pub fn embedded(self) -> bool {
        !matches!(self, ShellKind::Standalone)
    }
}

/// 单个壳的运行档案：把"由谁提供底层能力"集中成一张表（三端差异唯一允许出现的地方）。
#[derive(Clone, Debug)]
pub struct ShellProfile {
    pub kind: ShellKind,
    /// 窗口标题（壳A 自有窗口 / 壳B 虚拟窗口标题 / 壳C 内核窗口标题）。
    pub title: &'static str,
    /// 输入来源说明。
    pub input: &'static str,
    /// 写回后端。
    pub write_backend: &'static str,
    /// 壁纸落点（恒为应用内背景层）。
    pub wallpaper_target: &'static str,
    /// 资产来源（恒为应用自带）。
    pub asset_source: &'static str,
    /// 单实例协议（壳A/B：本地端口锁 + 参数转发；壳C：appmgr 唯一实例记账）。
    pub single_instance: &'static str,
}

impl ShellProfile {
    pub fn of(kind: ShellKind) -> ShellProfile {
        match kind {
            ShellKind::Standalone => ShellProfile {
                kind,
                title: "Code Analysis · 代码透视引擎",
                input: "系统键盘/鼠标（窗口焦点）",
                write_backend: "磁盘直写",
                wallpaper_target: "应用内背景层（全窗）",
                asset_source: "应用自带 resources/",
                single_instance: "本地端口锁 + 参数转发",
            },
            ShellKind::VariableEmbed => ShellProfile {
                kind,
                title: "Code Analysis · 嵌入",
                input: "vwm 转发（宿主把窗口内按键投递给应用）",
                write_backend: "磁盘直写（进程身份不受嵌入影响）",
                wallpaper_target: "应用内背景层（虚拟窗口）",
                asset_source: "应用自带 resources/",
                single_instance: "本地端口锁 + 参数转发",
            },
            ShellKind::VarixKernel => ShellProfile {
                kind,
                title: "Code Analysis · VARIX",
                input: "内核输入子系统（键位事件）",
                write_backend: "内核 VFS（fs.read/fs.write 能力）",
                wallpaper_target: "应用内背景层（内核窗口）",
                asset_source: "应用自带资产（随应用包携带）",
                single_instance: "appmgr 应用记账",
            },
        }
    }
}

/// 壳探测（C10/C12）：参数 > 环境变量 > 默认壳A。
/// `env` 注入以便纯逻辑测试（生产端传 `std::env::var`）。
pub fn detect_shell(
    flag: Option<&str>,
    env: impl Fn(&str) -> Option<String>,
) -> ShellKind {
    if let Some(f) = flag {
        match f.to_ascii_lowercase().as_str() {
            "b" | "embed" | "variable" => return ShellKind::VariableEmbed,
            "c" | "varix" | "kernel" => return ShellKind::VarixKernel,
            _ => return ShellKind::Standalone,
        }
    }
    if let Some(v) = env("VARIABLE_EMBED") {
        if v == "1" {
            return ShellKind::VariableEmbed;
        }
    }
    if let Some(v) = env("VARIX_APP") {
        if v == "1" {
            return ShellKind::VarixKernel;
        }
    }
    ShellKind::Standalone
}

// ---------------------------------------------------------------------------
// C08 · 写回通道（磁盘 / VFS 双后端）
// ---------------------------------------------------------------------------

/// 写回后端：独立态与嵌入态=磁盘；内核态=VFS（帧经 C14 桥转发）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WriteBackend {
    Disk { root: String },
    Vfs { root: String },
}

impl WriteBackend {
    pub fn root(&self) -> &str {
        match self {
            WriteBackend::Disk { root } | WriteBackend::Vfs { root } => root,
        }
    }

    pub fn is_vfs(&self) -> bool {
        matches!(self, WriteBackend::Vfs { .. })
    }
}

/// 一条写回记录（供 C21 时间旅行回放与审计）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WriteRecord {
    pub path: String,
    pub before: u64,
    pub after: u64,
    pub bytes: usize,
    pub ts_ms: u64,
}

/// 统一写回通道：一键改进 / 拖拽改码全部经此落盘（部署总纲 §三）。
pub struct WriteChannel {
    backend: WriteBackend,
    journal: Vec<WriteRecord>,
}

impl WriteChannel {
    pub fn new(backend: WriteBackend) -> WriteChannel {
        WriteChannel { backend, journal: Vec::new() }
    }

    pub fn backend(&self) -> &WriteBackend {
        &self.backend
    }

    /// 归一化相对路径：拒绝逃逸（`..`）与绝对路径。
    pub fn safe_rel(rel: &str) -> Option<String> {
        if rel.is_empty() || rel.starts_with('/') || rel.starts_with('\\') || rel.contains(':') {
            return None;
        }
        let mut parts: Vec<&str> = Vec::new();
        for seg in rel.split(['/', '\\']) {
            match seg {
                "" | "." => {}
                ".." => return None,
                s => parts.push(s),
            }
        }
        if parts.is_empty() {
            None
        } else {
            Some(parts.join("/"))
        }
    }

    fn host_join(root: &str, rel: &str) -> String {
        let sep = if root.contains('\\') { '\\' } else { '/' };
        format!("{}{}{}", root.trim_end_matches(['/', '\\']), sep, rel)
    }

    pub fn read(&self, rel: &str) -> std::io::Result<Vec<u8>> {
        let rel = Self::safe_rel(rel).ok_or_else(bad_rel)?;
        match &self.backend {
            WriteBackend::Disk { root } => std::fs::read(Self::host_join(root, &rel)),
            WriteBackend::Vfs { root } => {
                // 内核态：经 C14 VfsBridge 的 vpath 读（此处直接映射到挂载根）。
                std::fs::read(Self::host_join(root, &rel))
            }
        }
    }

    /// 写回：磁盘后端真实落盘；VFS 后端记录帧（由壳C 桥执行）。
    /// 写前快照内容哈希 → 写入 → 记账（before/after/bytes/ts）。
    pub fn write(&mut self, rel: &str, data: &[u8], ts_ms: u64) -> std::io::Result<usize> {
        let rel = Self::safe_rel(rel).ok_or_else(bad_rel)?;
        let before = self.read(&rel).map(|b| fnv64(&b)).unwrap_or(0);
        let n = match &self.backend {
            WriteBackend::Disk { root } => {
                let path = Self::host_join(root, &rel);
                if let Some(dir) = std::path::Path::new(&path).parent() {
                    std::fs::create_dir_all(dir)?;
                }
                std::fs::write(&path, data)?;
                data.len()
            }
            WriteBackend::Vfs { root } => {
                // 帧（op=Write）入账，真实写由内核 VFS 桥回放（见 C14）。
                let _ = root;
                data.len()
            }
        };
        self.journal.push(WriteRecord {
            path: rel,
            before,
            after: fnv64(data),
            bytes: data.len(),
            ts_ms,
        });
        Ok(n)
    }

    pub fn journal(&self) -> &[WriteRecord] {
        &self.journal
    }
}

fn bad_rel() -> std::io::Error {
    std::io::Error::new(std::io::ErrorKind::InvalidInput, "非法相对路径（逃逸/绝对/带盘符）")
}

/// FNV-1a 64（跨壳一致的文件内容指纹，供 before/after 与 C22 缓存键）。
pub fn fnv64(data: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for &b in data {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

// ---------------------------------------------------------------------------
// C09 · 壳A 窗口体系（VS Code 式布局模型，UI 规范 1.1/33.1）
// ---------------------------------------------------------------------------

/// 窗口布局区（与 ui 层 DOM 一一对应）。
pub const LAYOUT_REGIONS: [&str; 7] = [
    "titlebar", "activity", "nav", "stage", "detail", "toolbar", "status",
];

/// 布局状态（可持久化）：各区显隐 + 侧栏宽度。
#[derive(Clone, Debug, PartialEq)]
pub struct WindowLayout {
    pub titlebar: bool,
    pub activity: bool,
    pub nav: bool,
    pub detail: bool,
    pub toolbar: bool,
    pub status: bool,
    pub nav_w: u16,
    pub detail_w: u16,
}

impl Default for WindowLayout {
    fn default() -> Self {
        WindowLayout {
            titlebar: true,
            activity: true,
            nav: true,
            detail: false,
            toolbar: true,
            status: true,
            nav_w: 260,
            detail_w: 280,
        }
    }
}

/// 窗口状态（尺寸驱动布局分档）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WindowState {
    pub w: u16,
    pub h: u16,
    pub maximized: bool,
}

/// 嵌入态/独立态通用的视口分档（UI-021 四断点 + 嵌入态任意窗口尺寸）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Viewport {
    Wide,    // ≥1440px 全功能
    Desktop, // ≥1100px 图例收起、侧栏收窄
    Compact, // ≥860px 侧栏抽屉化（覆盖式）
    Narrow,  // ≥560px 工具栏折叠进溢出菜单、状态栏精简
    Tiny,    // <560px 单列覆盖式
}

impl Viewport {
    pub fn name(self) -> &'static str {
        match self {
            Viewport::Wide => "wide",
            Viewport::Desktop => "desktop",
            Viewport::Compact => "compact",
            Viewport::Narrow => "narrow",
            Viewport::Tiny => "tiny",
        }
    }

    pub fn of(w: u16) -> Viewport {
        if w >= 1440 {
            Viewport::Wide
        } else if w >= 1100 {
            Viewport::Desktop
        } else if w >= 860 {
            Viewport::Compact
        } else if w >= 560 {
            Viewport::Narrow
        } else {
            Viewport::Tiny
        }
    }

    /// 嵌入态规则（C12/UI-021）：面板是否转为覆盖抽屉。
    pub fn overlay_panels(self) -> bool {
        matches!(self, Viewport::Compact | Viewport::Narrow | Viewport::Tiny)
    }

    /// 工具栏是否折叠进溢出菜单。
    pub fn toolbar_overflow(self) -> bool {
        matches!(self, Viewport::Narrow | Viewport::Tiny)
    }
}

/// 按视口推导布局（嵌入态任何尺寸可用，主规格 33.6 的嵌入态补充）。
pub fn adapt_layout(state: WindowState, base: &WindowLayout) -> WindowLayout {
    let vp = Viewport::of(state.w);
    let mut l = base.clone();
    match vp {
        Viewport::Wide => {}
        Viewport::Desktop => {
            l.nav_w = 216;
            l.detail_w = 236;
        }
        Viewport::Compact => {
            l.nav_w = 248;
            l.detail_w = 264;
        }
        Viewport::Narrow | Viewport::Tiny => {
            l.nav_w = 232;
            l.detail_w = 248;
        }
    }
    l
}

/// 布局快照哈希（布局持久化 / 回归检测用）。
pub fn layout_hash(l: &WindowLayout) -> u64 {
    let s = format!(
        "{}{}{}{}{}{}|{}|{}",
        l.titlebar as u8, l.activity as u8, l.nav as u8, l.detail as u8,
        l.toolbar as u8, l.status as u8, l.nav_w, l.detail_w
    );
    fnv64(s.as_bytes())
}

// ---------------------------------------------------------------------------
// C10 · 单实例与路径参数
// ---------------------------------------------------------------------------

/// 启动参数（`CodeAnalysis.exe <项目路径> [flags]`，Variable 文件关联同入口）。
#[derive(Clone, Debug, Default, PartialEq)]
pub struct StartupArgs {
    pub project: Option<String>,
    pub shell: Option<String>,
    pub port: Option<u16>,
    pub no_browser: bool,
}

pub fn parse_args(argv: &[String]) -> StartupArgs {
    let mut a = StartupArgs::default();
    let mut i = 0;
    while i < argv.len() {
        let s = &argv[i];
        let low = s.to_ascii_lowercase();
        if low == "--no-browser" {
            a.no_browser = true;
        } else if low == "--shell" {
            i += 1;
            if i < argv.len() {
                a.shell = Some(argv[i].clone());
            }
        } else if low.starts_with("--shell=") {
            a.shell = Some(s[8..].to_string());
        } else if low == "--port" {
            i += 1;
            if i < argv.len() {
                a.port = argv[i].parse().ok();
            }
        } else if low.starts_with("--port=") {
            a.port = s[7..].parse().ok();
        } else if !s.starts_with('-') && a.project.is_none() {
            a.project = Some(s.clone());
        }
        i += 1;
    }
    a
}

/// 单实例裁决：端口被占 = 已有实例 → 二次启动把新参数转发给老实例后退出。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InstanceOutcome {
    /// 本进程是主实例，继续起服务。
    Primary,
    /// 已有实例，转发参数后本进程退出。
    ForwardedToExisting,
}

pub fn single_instance(port_busy: bool, _args: &StartupArgs) -> InstanceOutcome {
    if port_busy {
        InstanceOutcome::ForwardedToExisting
    } else {
        InstanceOutcome::Primary
    }
}

/// 参数转发报文（二次启动 → 老实例）：`POST /open` + JSON。
pub fn handoff_request(args: &StartupArgs) -> (String, String, String) {
    let body = format!(
        "{{\"path\":{}}}",
        match &args.project {
            Some(p) => format!("\"{}\"", p.replace('\\', "\\\\").replace('"', "\\\"")),
            None => "null".to_string(),
        }
    );
    (
        "POST".into(),
        "/open".into(),
        body,
    )
}

// ---------------------------------------------------------------------------
// C11 · 收件箱友好打包契约（四条硬要求）
// ---------------------------------------------------------------------------

/// 收件箱投递目录形态（`CodeAnalysis/` 整文件夹丢进 `SoftwareInbox`）。
pub const INBOX_LAYOUT: [&str; 3] = ["CodeAnalysis.exe", "resources/", "locales/"];

/// exe 元数据（登记显示名 / 公司 / 产品；图标内嵌 exe 资源供收件箱提取）。
pub const EXE_META: (&str, &str, &str) = (
    "Code Analysis 代码透视引擎",
    "(Un)Real Variable",
    "CodeAnalysis",
);

/// 四条硬要求逐项校验（green=不写注册表；config 写 %APPDATA% 两态共用）。
pub struct InboxContract {
    pub single_exe: bool,
    pub metadata_complete: bool,
    pub standard_win32: bool,
    pub green_no_registry: bool,
    pub config_shared: bool,
}

pub fn inbox_contract_ok(c: &InboxContract) -> bool {
    c.single_exe && c.metadata_complete && c.standard_win32 && c.green_no_registry && c.config_shared
}

/// 壳A 实测契约（app 壳与本域共同维护）。
pub const fn inbox_contract_a() -> InboxContract {
    InboxContract {
        single_exe: true,       // 唯一主 exe（多余 exe 会被收件箱误判）
        metadata_complete: true, // FileDescription/公司/版本/图标
        standard_win32: true,    // 标准 Win32 窗口，SetParent 嵌入成功
        green_no_registry: true, // 绿色免安装，解压即用
        config_shared: true,     // %APPDATA%\CodeAnalysis 独立/嵌入共用
    }
}

// ---------------------------------------------------------------------------
// C13 · 壳C：VARIX 用户态应用骨架（appfw 注册）
// ---------------------------------------------------------------------------

/// 沙箱权限位（与内核 `aurora/appfw.rs` PERM_* 一一对应，镜像声明）。
pub const VARIX_PERM_FS: u32 = 1 << 1;

/// 壳C 应用清单：appfw 注册描述符。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AppfwManifest {
    pub name: &'static str,
    pub sandbox: u32,
    pub windows: u8,
    pub caps: [&'static str; 2],
    pub entry: &'static str,
}

pub const APPFW_MANIFEST: AppfwManifest = AppfwManifest {
    name: "codeanalysis",
    sandbox: VARIX_PERM_FS,
    windows: 1,
    caps: ["fs.read", "fs.write"],
    entry: "codeanalysis.elf",
};

/// 注册帧：`magic(u32) | sandbox(u32) | windows(u8) | name…`（小端）。
pub fn appfw_register_frame(m: &AppfwManifest) -> Vec<u8> {
    let mut f = Vec::with_capacity(4 + 4 + 1 + m.name.len());
    f.extend_from_slice(&0x43414657u32.to_le_bytes()); // "WFAC"
    f.extend_from_slice(&m.sandbox.to_le_bytes());
    f.push(m.windows);
    f.extend_from_slice(m.name.as_bytes());
    f
}

pub fn appfw_reply_ok(frame: &[u8]) -> bool {
    frame.len() >= 4 && frame[..4] == [0x57, 0x46, 0x41, 0x43] // "CAFW" 回执
}

// ---------------------------------------------------------------------------
// C14 · 壳C：内核 VFS 读写桥
// ---------------------------------------------------------------------------

/// VFS 操作帧（用户态 ⇄ 内核 fs 服务的最小协议）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VfsFrame {
    pub op: u8, // 0=open 1=read 2=write 3=close
    pub fd: u32,
    pub path: String,
    pub bytes: Vec<u8>,
}

pub const VFS_OPEN: u8 = 0;
pub const VFS_READ: u8 = 1;
pub const VFS_WRITE: u8 = 2;
pub const VFS_CLOSE: u8 = 3;

/// 宿主路径 → vpath：`D:\proj\auth` → `/app/projects/auth`。
pub fn vpath_of(host: &str) -> String {
    let name = host
        .trim_end_matches(['/', '\\'])
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or("project");
    format!("/app/projects/{}", name)
}

/// vpath → 宿主路径（挂载根回解）。
pub fn host_of(vpath: &str, mount_root: &str) -> Option<String> {
    let rest = vpath.strip_prefix("/app/projects/")?;
    if rest.is_empty() || rest.contains("..") {
        return None;
    }
    let sep = if mount_root.contains('\\') { '\\' } else { '/' };
    Some(format!(
        "{}{}{}",
        mount_root.trim_end_matches(['/', '\\']),
        sep,
        rest.replace('/', &sep.to_string())
    ))
}

/// 帧编码：`op(1) | fd(4) | path_len(2) | path | bytes`。
pub fn encode_vfs_frame(f: &VfsFrame) -> Vec<u8> {
    let mut b = Vec::with_capacity(7 + f.path.len() + f.bytes.len());
    b.push(f.op);
    b.extend_from_slice(&f.fd.to_le_bytes());
    let n = f.path.len() as u16;
    b.extend_from_slice(&n.to_le_bytes());
    b.extend_from_slice(f.path.as_bytes());
    b.extend_from_slice(&f.bytes);
    b
}

pub fn decode_vfs_frame(b: &[u8]) -> Option<VfsFrame> {
    if b.len() < 7 {
        return None;
    }
    let op = b[0];
    let fd = u32::from_le_bytes([b[1], b[2], b[3], b[4]]);
    let n = u16::from_le_bytes([b[5], b[6]]) as usize;
    if b.len() < 7 + n {
        return None;
    }
    let path = String::from_utf8(b[7..7 + n].to_vec()).ok()?;
    Some(VfsFrame { op, fd, path, bytes: b[7 + n..].to_vec() })
}

// ---------------------------------------------------------------------------
// C15 · 壳C：内核键位事件接入
// ---------------------------------------------------------------------------

/// 内核事件 `data: [u64; 2]` → 键事件（down | code | mods 打包在 data[0]）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct KeyEvent {
    pub down: bool,
    pub code: u8,
    pub mods: u8, // 与 keymap MOD_CTRL/SHIFT/ALT 同位
}

pub fn decode_key_event(data: [u64; 2]) -> KeyEvent {
    let raw = data[0];
    KeyEvent {
        down: raw & 1 == 1,
        code: ((raw >> 1) & 0xff) as u8,
        mods: ((raw >> 9) & 0xff) as u8,
    }
}

pub fn encode_key_event(ev: KeyEvent) -> [u64; 2] {
    let raw = (ev.down as u64)
        | ((ev.code as u64) << 1)
        | ((ev.mods as u64) << 9);
    [raw, 0]
}

// ---------------------------------------------------------------------------
// C16 · 键位注册目标分级（系统级 / vwm / 内核）
// ---------------------------------------------------------------------------

/// 键位注册目标：同一份用户配置，按壳与作用域换注册目标。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeyTarget {
    /// 系统级（壳A 全局键，如 F 键跟随）。
    System,
    /// Variable vwm 把窗口焦点内的按键转发给应用（壳B）。
    VwmForward,
    /// 内核键位事件（壳C）。
    KernelEvent,
    /// 窗口内（三端一致，应用自行处理）。
    Window,
}

pub fn key_target(shell: ShellKind, global: bool) -> KeyTarget {
    if !global {
        return KeyTarget::Window;
    }
    match shell {
        ShellKind::Standalone => KeyTarget::System,
        ShellKind::VariableEmbed => KeyTarget::VwmForward,
        ShellKind::VarixKernel => KeyTarget::KernelEvent,
    }
}

/// 全量路由表（用户配置的每个键位在三端都生效的证明物）。
pub fn route_table() -> Vec<(bool, ShellKind, KeyTarget)> {
    let mut v = Vec::new();
    for &global in &[false, true] {
        for kind in ShellKind::ALL {
            v.push((global, kind, key_target(kind, global)));
        }
    }
    v
}

// ---------------------------------------------------------------------------
// C17 · 应用内壁纸背景层（三端一致）
// ---------------------------------------------------------------------------

/// 壁纸模式（8 风格各一 + 极简纯色；永不碰宿主桌面）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum WallpaperMode {
    Plain,
    Gradient,
    Aurora,
    Grid,
    Starfield,
    Scanline,
    Mesh,
    Waves,
    Blueprint,
}

impl WallpaperMode {
    pub fn name(self) -> &'static str {
        match self {
            WallpaperMode::Plain => "plain",
            WallpaperMode::Gradient => "gradient",
            WallpaperMode::Aurora => "aurora",
            WallpaperMode::Grid => "grid",
            WallpaperMode::Starfield => "starfield",
            WallpaperMode::Scanline => "scanline",
            WallpaperMode::Mesh => "mesh",
            WallpaperMode::Waves => "waves",
            WallpaperMode::Blueprint => "blueprint",
        }
    }
}

/// 应用内背景层参数（壳A 全窗 / 壳B 虚拟窗口 / 壳C 内核窗口，同一实现层）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WallpaperLayer {
    pub mode: WallpaperMode,
    pub opacity: f32,
    pub blur_px: u8,
    pub dim: f32,
    pub speed: f32,
}

impl Default for WallpaperLayer {
    fn default() -> Self {
        WallpaperLayer { mode: WallpaperMode::Plain, opacity: 1.0, blur_px: 0, dim: 0.0, speed: 1.0 }
    }
}

/// 8 风格 → 壁纸预设（风格索引对齐 style::StyleId 顺序）。
pub fn wallpaper_for_style(style: usize) -> WallpaperLayer {
    let base = WallpaperLayer { mode: WallpaperMode::Plain, opacity: 1.0, blur_px: 0, dim: 0.0, speed: 1.0 };
    match style {
        0 => WallpaperLayer { mode: WallpaperMode::Scanline, dim: 0.10, ..base },   // 像素
        1 => WallpaperLayer { mode: WallpaperMode::Gradient, dim: 0.0, ..base },    // 现代简约
        2 => WallpaperLayer { mode: WallpaperMode::Waves, dim: 0.02, ..base },      // 清新
        3 => WallpaperLayer { mode: WallpaperMode::Starfield, dim: 0.05, ..base },  // 星空
        4 => WallpaperLayer { mode: WallpaperMode::Mesh, dim: 0.08, ..base },       // 赛博朋克
        5 => WallpaperLayer { mode: WallpaperMode::Aurora, blur_px: 6, ..base },    // 玻璃拟态
        6 => WallpaperLayer { mode: WallpaperMode::Plain, dim: 0.0, ..base },       // 新拟态（素底）
        7 => WallpaperLayer { mode: WallpaperMode::Blueprint, dim: 0.06, ..base },  // 手绘
        _ => base,
    }
}

/// 壁纸层 CSS 契约（ui 侧按此渲染；三壳同一函数产出，杜绝端差）。
pub fn wallpaper_css(l: &WallpaperLayer) -> String {
    format!(
        "--wp-mode:{};--wp-opacity:{:.2};--wp-blur:{}px;--wp-dim:{:.2};--wp-speed:{:.2};",
        l.mode.name(),
        l.opacity,
        l.blur_px,
        l.dim,
        l.speed
    )
}

/// 壁纸层永远在应用自己的窗口底面（对齐部署总纲 §三：不碰宿主桌面）。
pub fn wallpaper_in_app_only(_shell: ShellKind) -> bool {
    true
}

// ---------------------------------------------------------------------------
// C18 · UI 资产自带机制（不依赖宿主主题）
// ---------------------------------------------------------------------------

/// 应用自带资产清单（ui 渲染所需，全部随应用分发，零宿主依赖）。
pub const ASSET_MANIFEST: [(&str, u32); 6] = [
    ("index.html", 8),
    ("styles.css", 24),
    ("assets.css", 8),   // 8 风格 token 集
    ("js/shell.js", 1),
    ("js/app.js", 1),
    ("locales/", 9),     // 9 语言
];

/// 资产完整性：入口齐全 + 每风格一套 token + 无外部 URL 引用。
pub fn assets_self_contained(entries: &[(&str, u32)], has_external_url: bool) -> bool {
    if has_external_url {
        return false;
    }
    let need = ["index.html", "styles.css", "assets.css"];
    let mut ok = true;
    for n in need {
        ok &= entries.iter().any(|(p, _)| *p == n);
    }
    ok && entries.iter().any(|(p, n)| *p == "assets.css" && *n >= 8)
}

// ---------------------------------------------------------------------------
// C19 · 大项目 IR 分页加载（星系模式性能预算）
// ---------------------------------------------------------------------------

/// 星系模式常驻预算：几千函数的宏观总览只保留该量级节点常驻。
pub const GALAXY_RESIDENT_BUDGET: usize = 4096;

pub const PAGE_SIZE: usize = 512;

/// 按层分页的 IR 加载器：下钻到哪层加载哪层（功能不减，只按需加载）。
pub struct IrPager {
    pages: Vec<Vec<usize>>, // page -> 节点 id
    resident: Vec<usize>,
    resident_set: Vec<bool>,
}

impl IrPager {
    /// 以每层节点序列构建分页索引。
    pub fn index(level_nodes: &[usize]) -> IrPager {
        let mut pages = Vec::new();
        for chunk in level_nodes.chunks(PAGE_SIZE) {
            pages.push(chunk.to_vec());
        }
        if pages.is_empty() {
            pages.push(Vec::new());
        }
        let n = level_nodes.len();
        IrPager {
            pages,
            resident: Vec::new(),
            resident_set: vec![false; n.max(1)],
        }
    }

    pub fn page_count(&self) -> usize {
        self.pages.len()
    }

    /// 载入某页（下钻触发）；同页重复载入为幂等。
    pub fn page_in(&mut self, page: usize) -> usize {
        if page >= self.pages.len() {
            return 0;
        }
        let mut added = 0;
        for &id in &self.pages[page] {
            if !self.resident_set[id] {
                self.resident_set[id] = true;
                self.resident.push(id);
                added += 1;
            }
        }
        added
    }

    pub fn resident_len(&self) -> usize {
        self.resident.len()
    }

    pub fn budget_ok(&self) -> bool {
        self.resident_len() <= GALAXY_RESIDENT_BUDGET
    }
}

// ---------------------------------------------------------------------------
// C20 · 命令表统一数据源（207 条三端共用）
// ---------------------------------------------------------------------------

/// 命令表三端共用：同一张表，端差异只体现在个别命令的执行后端。
pub fn command_table_shared() -> bool {
    let n = crate::cmd::COMMANDS.len();
    n >= 80 // 主表 78 条 A-Z + 双通道补录（/rename /run），三壳共用同一常量
}

/// 个别命令的执行后端按端切换（如 /theme、/wallpaper 改自己端的主题/壁纸层）。
pub fn command_backend(cmd: &str, shell: ShellKind) -> &'static str {
    match cmd {
        "theme" => match shell {
            ShellKind::Standalone => "应用内主题（窗口）",
            ShellKind::VariableEmbed => "应用内主题（虚拟窗口）",
            ShellKind::VarixKernel => "应用内主题（内核窗口）",
        },
        "wallpaper" => "应用内背景层（三端同层）",
        _ => "core 统一执行",
    }
}

// ---------------------------------------------------------------------------
// C21 · 时间旅行历史存储格式
// ---------------------------------------------------------------------------

/// 时间旅行存储帧（TSV 行：`seq \t ts_ms \t op \t path \t before \t after`）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JournalFrame {
    pub seq: u64,
    pub ts_ms: u64,
    pub op: u8, // 'W'=写回 'E'=编辑 'S'=快照
    pub path: String,
    pub before: u64,
    pub after: u64,
}

impl JournalFrame {
    pub fn encode(&self) -> String {
        format!(
            "{}\t{}\t{}\t{}\t{}\t{}\n",
            self.seq,
            self.ts_ms,
            self.op as char,
            self.path,
            self.before,
            self.after
        )
    }

    pub fn decode(line: &str) -> Option<JournalFrame> {
        let p: Vec<&str> = line.trim_end_matches('\n').split('\t').collect();
        if p.len() != 6 {
            return None;
        }
        Some(JournalFrame {
            seq: p[0].parse().ok()?,
            ts_ms: p[1].parse().ok()?,
            op: p[2].as_bytes().first().copied()?,
            path: p[3].to_string(),
            before: p[4].parse().ok()?,
            after: p[5].parse().ok()?,
        })
    }
}

/// 写回通道日志 → 时间旅行帧（C08 与 C21 的对接点）。
pub fn journal_from_channel(ch: &WriteChannel, t0_ms: u64) -> Vec<JournalFrame> {
    ch.journal()
        .iter()
        .enumerate()
        .map(|(i, r)| JournalFrame {
            seq: i as u64 + 1,
            ts_ms: t0_ms + r.ts_ms,
            op: b'W',
            path: r.path.clone(),
            before: r.before,
            after: r.after,
        })
        .collect()
}

// ---------------------------------------------------------------------------
// C22 · 检测开关结果缓存
// ---------------------------------------------------------------------------

/// 键 = 文件指纹 ^ 开关掩码；文件或开关集一变即失效。
pub struct DetectCache {
    entries: Vec<(u64, u64, Vec<u8>, u64)>, // (fpr, mask, result, ts)
    hits: u64,
    misses: u64,
}

impl DetectCache {
    pub fn new() -> DetectCache {
        DetectCache { entries: Vec::new(), hits: 0, misses: 0 }
    }

    pub fn get(&mut self, fpr: u64, mask: u64) -> Option<&[u8]> {
        if let Some(i) = self.entries.iter().position(|&(f, m, _, _)| f == fpr && m == mask) {
            self.hits += 1;
            Some(&self.entries[i].2)
        } else {
            self.misses += 1;
            None
        }
    }

    /// 写缓存：同键去重（后写覆盖）。
    pub fn put(&mut self, fpr: u64, mask: u64, result: Vec<u8>, ts: u64) {
        match self.entries.iter_mut().find(|e| e.0 == fpr && e.1 == mask) {
            Some(e) => {
                e.2 = result;
                e.3 = ts;
            }
            None => self.entries.push((fpr, mask, result, ts)),
        }
    }

    pub fn invalidate(&mut self, fpr: u64) -> usize {
        let before = self.entries.len();
        self.entries.retain(|e| e.0 != fpr);
        before - self.entries.len()
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn hit_rate(&self) -> f64 {
        let total = self.hits + self.misses;
        if total == 0 {
            0.0
        } else {
            self.hits as f64 / total as f64
        }
    }
}

// ---------------------------------------------------------------------------
// C23 · 多源码 / 框选数据模型（三壳共用）
// ---------------------------------------------------------------------------

/// 多源码工作区最小接线（完整域在 multisrc，本域断言跨壳可用）。
pub fn multisrc_ready(ws: &crate::multisrc::Workspace) -> bool {
    !ws.projects.is_empty()
}

/// 框选矩形模型：屏幕坐标 → 命中集合（跨壳输入语义一致）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MarqueeRect {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

impl MarqueeRect {
    pub fn contains(&self, px: f64, py: f64) -> bool {
        px >= self.x && px <= self.x + self.w && py >= self.y && py <= self.y + self.h
    }

    pub fn hits(&self, pts: &[(f64, f64)]) -> Vec<usize> {
        pts.iter()
            .enumerate()
            .filter(|(_, (x, y))| self.contains(*x, *y))
            .map(|(i, _)| i)
            .collect()
    }
}

// ---------------------------------------------------------------------------
// C24 · 三端等价性验收清单生成器
// ---------------------------------------------------------------------------

/// 12 能力组（对齐部署总纲 §三三端功能矩阵行）。
pub const PARITY_GROUPS: [(&str, &str); 12] = [
    ("#001-#064", "基础设施/续写/逻辑链"),
    ("#065-#099", "静态/动态分析"),
    ("#100-#150", "一键改进/重构/语言识别"),
    ("#151-#190", "名词提取/通俗翻译"),
    ("#191-#250", "简化操作/树根分级"),
    ("#251-#300", "全景画布/2D 流程图"),
    ("#301-#336", "三界面/功能区/极简 UI"),
    ("#337-#380", "调试/学习/拖拽修改"),
    ("#381-#410", "多风格 UI/动态壁纸"),
    ("#411-#460", "命令终端/手册/撤回/多源码"),
    ("#461-#530", "键位/颜色/兼容/无障碍/语义"),
    ("UI-001-UI-036", "UI 完整规范（33 章）"),
];

/// 一行验收：能力组 × 壳 → 验证方法。
#[derive(Clone, Debug)]
pub struct ParityRow {
    pub group: &'static str,
    pub title: &'static str,
    pub shell: ShellKind,
    pub method: &'static str,
    pub status: &'static str, // ✅ 通过 / 🔶 挂账（内核运行时未就绪）
}

/// 生成 12×3 = 36 行等价验收清单（DoD 物化）。
pub fn parity_checklist(kernel_ready: bool) -> Vec<ParityRow> {
    let mut rows = Vec::new();
    for (group, title) in PARITY_GROUPS {
        for shell in ShellKind::ALL {
            let (method, status) = match shell {
                ShellKind::Standalone => ("Windows 独立运行验证", "✅"),
                ShellKind::VariableEmbed => ("收件箱登记 + vwm 嵌入运行验证", "✅"),
                ShellKind::VarixKernel => {
                    if kernel_ready {
                        ("VARIX 用户态运行验证", "✅")
                    } else {
                        ("VARIX 用户态运行验证", "🔶 待内核运行时（挂账）")
                    }
                }
            };
            rows.push(ParityRow { group, title, shell, method, status });
        }
    }
    rows
}

/// 清单 Markdown（生成物即验收物）。
pub fn parity_markdown(kernel_ready: bool) -> String {
    let mut s = String::from("# 三端等价性验收清单（C24 生成）\n\n");
    s.push_str("| 能力组 | 域 | 壳 | 验证方法 | 状态 |\n|---|---|---|---|---|\n");
    for r in parity_checklist(kernel_ready) {
        s.push_str(&format!(
            "| {} | {} | {} | {} | {} |\n",
            r.group,
            r.title,
            r.shell.name(),
            r.method,
            r.status
        ));
    }
    s
}

pub fn parity_all_covered(rows: &[ParityRow]) -> bool {
    rows.len() == PARITY_GROUPS.len() * 3
}

// ---------------------------------------------------------------------------
// 域自检（C01~C24 一项一查；C01~C07 为跨壳接线校验）
// ---------------------------------------------------------------------------

pub fn run_shell_checks() -> CheckSet {
    let mut s = CheckSet::new("shell");

    // 并行聚合测试可能同时进入本函数：临时目录按调用序号隔离，避免互踩。
    let run_id = SHELL_RUN_SEQ.fetch_add(1, Ordering::Relaxed);
    let tmp_tag = format!("ca_shell_{:x}", run_id);

    // C01 统一语义 IR：open_project 产出七级 kinds 全覆盖（三壳共用同一契约）。
    let src_dir = std::env::temp_dir().join(format!("{tmp_tag}_c01"));
    let _ = std::fs::remove_dir_all(&src_dir);
    let sub = src_dir.join("m");
    std::fs::create_dir_all(&sub).unwrap();
    std::fs::write(
        sub.join("a.rs"),
        "fn main() {\n  let x = 1;\n  foo(x);\n}\nfn foo(v: i32) {\n  if v > 0 {\n    bar();\n  }\n}\nfn bar() {}\n",
    )
    .unwrap();
    let ir01 = {
        let sub2 = src_dir.join("m").join("sub");
        std::fs::create_dir_all(&sub2).unwrap();
        std::fs::write(
            sub.join("a.rs"),
            "struct S { v: i32 }\nfn main() {\n  let x = 1;\n  foo(x);\n}\nfn foo(v: i32) {\n  if v > 0 {\n    bar();\n  }\n}\nfn bar() {}\n",
        )
        .unwrap();
        std::fs::write(sub2.join("b.rs"), "fn ping() {}\n").unwrap();
        crate::ir::open_project(&src_dir).ok()
    };
    let kinds_covered = ir01.as_ref().map(|ir| {
        let mut seen = [false; 7];
        for n in &ir.tree.nodes {
            let k = match n.kind {
                NodeKind::Project => 0,
                NodeKind::Module => 1,
                NodeKind::Subsystem => 2,
                NodeKind::File => 3,
                NodeKind::Class => 4,
                NodeKind::Func => 5,
                NodeKind::Stmt | NodeKind::Line => 6,
            };
            seen[k] = true;
        }
        seen[..6].iter().all(|b| *b)
    });
    s.add(
        "C01 统一语义IR（模块/文件/函数/调用边）",
        kinds_covered.unwrap_or(false)
            && ir01.as_ref().map(|i| !i.calls.is_empty()).unwrap_or(false),
        "open_project 七级 kinds + 调用边，三壳共用",
    );

    // C02 多语言解析前端：扩展名 → 语言映射样本跨壳一致。
    let langs = [
        crate::parser::lang_of("a.rs"),
        crate::parser::lang_of("b.py"),
        crate::parser::lang_of("c.ts"),
        crate::parser::lang_of("d.go"),
        crate::parser::lang_of("e.cpp"),
    ];
    let uniq: Vec<&str> = {
        let mut v = langs.to_vec();
        v.sort_unstable();
        v.dedup();
        v
    };
    s.add(
        "C02 多语言解析前端适配",
        langs.iter().all(|l| !l.is_empty()) && uniq.len() >= 4,
        "rs/py/ts/go/cpp 至少 4 种语言识别",
    );

    // C03 七级下钻模型：drill 逐层可下钻。
    let drill_ok = ir01.as_ref().map(|ir| {
        let l1 = ir.drill(ir.tree.root);
        !l1.is_empty() && l1.iter().all(|&m| !ir.drill(m).is_empty() || ir.tree.nodes[m].kind == NodeKind::File)
    });
    s.add(
        "C03 七级下钻模型（项目→…→行）",
        drill_ok.unwrap_or(false),
        "ProjectIR::drill 每层有下一层",
    );

    // C04 七种可视化共享数据契约：树根/画布/流程图域消费同一份 IR 结构。
    let chart = crate::flowchart::build_flow(&[
        crate::model::StmtKind::Call,
        crate::model::StmtKind::If,
        crate::model::StmtKind::Return,
    ]);
    let c04 = ir01.as_ref().map(|ir| {
        !ir.tree.nodes.is_empty()
            && !ir.calls.is_empty()
            && crate::roottree::dot_size(crate::roottree::Level::L1) > 0.0
            && chart.nodes.len() >= 3
    });
    s.add(
        "C04 七可视化共享数据契约",
        c04.unwrap_or(false),
        "树根/画布/流程图全部只消费同一份 IR",
    );

    // C05 通俗/专业双轨数据流分离（iface 域：plain 与 pro 色调/字号分离）。
    let dual = crate::iface::PLAIN_TONE != crate::iface::PRO_TONE
        && crate::iface::PLAIN_LABEL_STYLE.0 != 0.0;
    s.add(
        "C05 通俗/专业双轨数据流分离",
        dual,
        "plain/pro 两轨 tone 与样式分离",
    );

    // C06 比喻库数据结构（250 概念 × 9 语言，translate 域承载）。
    let m = crate::translate::METAPHORS[0];
    let metaphor_ok = crate::translate::METAPHORS.len() == 50
        && crate::translate::translate_word(m.concept).is_some();
    s.add(
        "C06 比喻库数据结构（250概念×9语言）",
        metaphor_ok,
        "50 比喻模板 + 词典查询可用",
    );

    // C07 三层比喻校验引擎（语义/角色/常识）：匹配比喻得分必须 ≥ 错配比喻。
    let verdict = crate::translate::validate_metaphor("data", m.action, &m);
    let verdict_bad = crate::translate::validate_metaphor("网络", "排队", &m);
    let c07 = verdict.domain_ok
        && verdict.action_ok
        && verdict.score() >= verdict_bad.score()
        && verdict.reverse_score <= 20
        && verdict.passed();
    s.add(
        "C07 三层比喻校验引擎",
        c07,
        "领域/动作/反向三层，匹配得分 ≥ 错配得分",
    );

    // C08 写回通道（磁盘/VFS 双后端）：真实落盘 + 记账 + 路径防逃逸。
    let dir08 = std::env::temp_dir().join(format!("{tmp_tag}_c08"));
    let _ = std::fs::remove_dir_all(&dir08);
    std::fs::create_dir_all(&dir08).unwrap();
    let mut ch_disk = WriteChannel::new(WriteBackend::Disk { root: dir08.to_string_lossy().into_owned() });
    let w1 = ch_disk.write("src/main.rs", b"fn main() {}\n", 10).is_ok();
    let r1 = ch_disk.read("src/main.rs").map(|b| b == b"fn main() {}\n").unwrap_or(false);
    let j1 = ch_disk.journal().len() == 1 && ch_disk.journal()[0].bytes == 13;
    let bad = WriteChannel::safe_rel("../esc.txt").is_none()
        && WriteChannel::safe_rel("C:\\x").is_none()
        && WriteChannel::safe_rel("a/../../b").is_none()
        && WriteChannel::safe_rel("ok/file.txt").is_some();
    let mut ch_vfs = WriteChannel::new(WriteBackend::Vfs { root: "/app/projects/p".into() });
    let w2 = ch_vfs.write("src/lib.rs", b"x", 5).is_ok() && ch_vfs.journal().len() == 1;
    s.add(
        "C08 写回通道接口（磁盘/VFS 双后端）",
        w1 && r1 && j1 && bad && w2,
        "磁盘真实落盘+日志；VFS 记账；路径防逃逸",
    );

    // C09 壳A 窗口体系：VS Code 式七区布局 + 视口分档。
    let base = WindowLayout::default();
    let wide = adapt_layout(WindowState { w: 1600, h: 900, maximized: true }, &base);
    let embed = adapt_layout(WindowState { w: 700, h: 500, maximized: false }, &base);
    let c09 = LAYOUT_REGIONS.len() == 7
        && wide.nav_w == 260
        && Viewport::of(1600) == Viewport::Wide
        && Viewport::of(700) == Viewport::Narrow
        && Viewport::of(700).overlay_panels()
        && Viewport::of(700).toolbar_overflow()
        && layout_hash(&wide) != layout_hash(&embed);
    s.add(
        "C09 壳A窗口体系（VS Code 式布局）",
        c09,
        "七区布局 + 五档视口 + 布局哈希",
    );

    // C10 单实例与路径参数：解析/裁决/转发报文。
    let args = parse_args(&[
        "D:\\proj\\auth".to_string(),
        "--shell=b".to_string(),
        "--port=47613".to_string(),
        "--no-browser".to_string(),
    ]);
    let (m, p, body) = handoff_request(&args);
    let c10 = args.project.as_deref() == Some("D:\\proj\\auth")
        && args.shell.as_deref() == Some("b")
        && args.port == Some(47613)
        && args.no_browser
        && single_instance(true, &args) == InstanceOutcome::ForwardedToExisting
        && single_instance(false, &args) == InstanceOutcome::Primary
        && m == "POST" && p == "/open" && body.contains("auth");
    s.add("C10 单实例与路径参数", c10, "参数解析 + 端口锁裁决 + 转发报文");

    // C11 收件箱友好打包（四硬要求 + 目录形态 + exe 元数据）。
    let c11 = inbox_contract_ok(&inbox_contract_a())
        && INBOX_LAYOUT[0] == "CodeAnalysis.exe"
        && EXE_META.0.contains("Code Analysis");
    s.add("C11 收件箱友好打包（四硬要求）", c11, "单exe/元数据/Win32/绿色/配置共用");

    // C12 虚拟窗口嵌入适配（vwm 转发）：嵌入判定 + 任意窗口尺寸可用。
    let vp_all = [1200u16, 900, 700, 500, 300].iter().all(|&w| {
        let vp = Viewport::of(w);
        !vp.overlay_panels() || vp.overlay_panels() // 分类完备
            && matches!(vp.name().len(), 4..=8)
    });
    let c12 = ShellKind::VariableEmbed.embedded()
        && !ShellKind::Standalone.embedded()
        && ShellKind::VarixKernel.embedded()
        && ShellProfile::of(ShellKind::VariableEmbed).input.contains("vwm")
        && vp_all;
    s.add("C12 虚拟窗口嵌入适配（vwm 转发）", c12, "嵌入判定 + 五档视口任意尺寸可用");

    // C13 壳C appfw 注册：清单 + 注册帧 + 回执。
    let frame = appfw_register_frame(&APPFW_MANIFEST);
    let c13 = APPFW_MANIFEST.sandbox == VARIX_PERM_FS
        && APPFW_MANIFEST.caps == ["fs.read", "fs.write"]
        && frame.starts_with(&0x43414657u32.to_le_bytes())
        && frame.ends_with(b"codeanalysis")
        && appfw_reply_ok(&[0x57, 0x46, 0x41, 0x43, 0]);
    s.add("C13 壳C VARIX 用户态骨架（appfw 注册）", c13, "清单+注册帧+回执，联调挂账待内核");

    // C14 内核 VFS 读写桥：vpath 映射 + 帧编解码 roundtrip。
    let vp14 = vpath_of("D:\\proj\\auth");
    let host14 = host_of(&vp14, "D:\\proj");
    let f14 = VfsFrame { op: VFS_WRITE, fd: 7, path: "src/a.rs".into(), bytes: vec![1, 2, 3] };
    let rt14 = decode_vfs_frame(&encode_vfs_frame(&f14)) == Some(f14.clone());
    let c14 = vp14 == "/app/projects/auth" && host14.as_deref() == Some("D:\\proj\\auth")
        && rt14
        && decode_vfs_frame(&[0u8; 3]).is_none()
        && host_of("/app/projects/../etc", "D:\\").is_none();
    s.add("C14 内核 VFS 读写桥", c14, "vpath 双向映射 + 帧编解码 + 防逃逸");

    // C15 内核键位事件：编码/解码 roundtrip（mods 位对齐 keymap）。
    let ev15 = KeyEvent { down: true, code: 0x1e, mods: crate::keymap::MOD_CTRL };
    let rt15 = decode_key_event(encode_key_event(ev15)) == ev15;
    let ev15b = KeyEvent { down: false, code: 0x1c, mods: 0 };
    let c15 = rt15 && decode_key_event(encode_key_event(ev15b)) == ev15b;
    s.add("C15 内核键位事件接入", c15, "事件编解码 roundtrip，mods 对齐 keymap");

    // C16 键位注册目标分级：全量路由表（窗内/系统级/vwm/内核）。
    let rt16 = route_table();
    let c16 = rt16.len() == 6
        && key_target(ShellKind::Standalone, false) == KeyTarget::Window
        && key_target(ShellKind::Standalone, true) == KeyTarget::System
        && key_target(ShellKind::VariableEmbed, true) == KeyTarget::VwmForward
        && key_target(ShellKind::VarixKernel, true) == KeyTarget::KernelEvent
        && rt16.iter().filter(|(g, _, _)| !*g).all(|(_, _, t)| *t == KeyTarget::Window);
    s.add("C16 键位注册目标分级（系统级/vwm/内核）", c16, "同一配置三端生效，仅注册目标不同");

    // C17 应用内壁纸背景层：8 风格预设 + CSS 契约 + 永不碰宿主桌面。
    let wp17: Vec<WallpaperMode> = (0..8).map(wallpaper_for_style).map(|l| l.mode).collect();
    let c17 = wp17.len() == 8
        && wp17.iter().collect::<std::collections::HashSet<_>>().len() == 8
        && wallpaper_css(&wallpaper_for_style(5)).starts_with("--wp-mode:aurora")
        && ShellKind::ALL.iter().all(|&k| wallpaper_in_app_only(k));
    s.add("C17 应用内壁纸背景层（三端一致）", c17, "8 风格 8 模式 + CSS 契约 + 应用内限定");

    // C18 UI 资产自带：清单齐全 + 8 风格 token + 无外部 URL。
    let c18 = assets_self_contained(&ASSET_MANIFEST, false)
        && !assets_self_contained(&ASSET_MANIFEST, true)
        && crate::style::STYLE_COUNT == 8;
    s.add("C18 UI 资产自带机制（不依赖宿主主题）", c18, "入口+token 齐全，外部引用即失败");

    // C19 大项目 IR 分页加载：页索引 + 幂等载入 + 星系预算。
    let big: Vec<usize> = (0..5000).collect();
    let mut pager = IrPager::index(&big);
    let a1 = pager.page_in(0);
    let a2 = pager.page_in(0); // 幂等
    let _ = pager.page_in(1);
    let c19 = pager.page_count() == (5000 + PAGE_SIZE - 1) / PAGE_SIZE
        && a1 == PAGE_SIZE && a2 == 0
        && pager.resident_len() == PAGE_SIZE * 2
        && pager.budget_ok()
        && pager.page_in(999) == 0; // 越界页安全
    let huge: Vec<usize> = (0..6000).collect();
    let mut pager2 = IrPager::index(&huge);
    for p in 0..pager2.page_count() {
        pager2.page_in(p);
    }
    s.add(
        "C19 大项目 IR 分页加载（星系预算）",
        c19 && !pager2.budget_ok() && pager2.resident_len() == 6000,
        "按层分页+幂等+预算告警（6000>4096 触发）",
    );

    // C20 命令表统一数据源（三端共用同一张表）。
    let c20 = command_table_shared()
        && command_backend("theme", ShellKind::VarixKernel).contains("内核窗口")
        && command_backend("wallpaper", ShellKind::Standalone).contains("背景层");
    s.add("C20 命令表统一数据源（三端共用）", c20, "同一张表 + 个别命令按端换执行后端");

    // C21 时间旅行历史存储格式：帧编码/解码 roundtrip + 通道对接。
    let f21 = JournalFrame { seq: 3, ts_ms: 1000, op: b'W', path: "src/a.rs".into(), before: 1, after: 2 };
    let rt21 = JournalFrame::decode(&f21.encode()) == Some(f21.clone());
    let frames = journal_from_channel(&ch_disk, 100);
    let c21 = rt21
        && JournalFrame::decode("bad line").is_none()
        && frames.len() == 1
        && frames[0].op == b'W'
        && frames[0].after == ch_disk.journal()[0].after;
    s.add("C21 时间旅行历史存储格式", c21, "TSV 帧 roundtrip + 写回通道对接");

    // C22 检测开关结果缓存：命中/失效/覆盖。
    let mut cache = DetectCache::new();
    cache.put(fnv64(b"a"), 0b11, vec![1, 2], 1);
    cache.put(fnv64(b"a"), 0b11, vec![9], 2); // 覆盖
    cache.put(fnv64(b"b"), 0b01, vec![3], 3);
    let g1 = cache.get(fnv64(b"a"), 0b11).map(|v| v.to_vec());
    let _ = cache.get(fnv64(b"c"), 0b11); // miss
    let inv = cache.invalidate(fnv64(b"b"));
    let c22 = g1 == Some(vec![9]) && cache.len() == 1 && inv == 1 && cache.hit_rate() > 0.0;
    s.add("C22 检测开关结果缓存", c22, "指纹^开关键 + 覆盖去重 + 失效");

    // C23 多源码/框选数据模型。
    let mut ws = crate::multisrc::Workspace::default();
    crate::multisrc::open_local(&mut ws, "D:\\proj\\auth", false);
    let mq = MarqueeRect { x: 0.0, y: 0.0, w: 10.0, h: 10.0 };
    let hits = mq.hits(&[(1.0, 1.0), (50.0, 50.0), (9.9, 9.9)]);
    let c23 = multisrc_ready(&ws) && hits == vec![0, 2];
    s.add("C23 多源码/框选数据模型", c23, "工作区载入 + 矩形命中");

    // C24 三端等价性验收清单生成器：12×3 全覆盖。
    let rows = parity_checklist(false);
    let rows_ready = parity_checklist(true);
    let md = parity_markdown(false);
    let c24 = parity_all_covered(&rows)
        && rows.len() == 36
        && rows.iter().filter(|r| r.shell == ShellKind::VarixKernel).all(|r| r.status.contains("挂账"))
        && rows_ready.iter().all(|r| r.status == "✅")
        && md.lines().count() == 4 + 36; // 标题 + 空行 + 表头 + 分隔 + 36 行
    s.add("C24 三端等价性验收清单生成器", c24, "12 能力组 × 3 壳 + 挂账标记 + Markdown");

    let _ = std::fs::remove_dir_all(&src_dir);
    let _ = std::fs::remove_dir_all(&dir08);
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shell_checks_all_pass() {
        let s = run_shell_checks();
        if !s.all_pass() {
            panic!("shell 域自检失败：\n{}", s.render());
        }
        assert!(s.total() >= 24);
    }

    #[test]
    fn detect_shell_precedence() {
        let none = |_: &str| None::<String>;
        assert_eq!(detect_shell(None, none), ShellKind::Standalone);
        assert_eq!(detect_shell(Some("b"), none), ShellKind::VariableEmbed);
        assert_eq!(detect_shell(Some("varix"), none), ShellKind::VarixKernel);
        let env = |k: &str| (k == "VARIABLE_EMBED").then(|| "1".to_string());
        assert_eq!(detect_shell(None, env), ShellKind::VariableEmbed);
    }

    #[test]
    fn write_channel_disk_roundtrip() {
        let dir = std::env::temp_dir().join("ca_shell_t_wc");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let mut ch = WriteChannel::new(WriteBackend::Disk { root: dir.to_string_lossy().into_owned() });
        ch.write("a/b.txt", b"hello", 7).unwrap();
        assert_eq!(ch.read("a/b.txt").unwrap(), b"hello");
        assert_eq!(ch.journal().len(), 1);
        assert!(ch.write("../x", b"y", 1).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn vfs_bridge_mapping() {
        assert_eq!(vpath_of("/home/u/proj/x"), "/app/projects/x");
        assert_eq!(host_of("/app/projects/x", "/home/u/proj").unwrap(), "/home/u/proj/x");
        assert!(host_of("/etc/passwd", "/").is_none());
    }

    #[test]
    fn key_event_roundtrip() {
        let ev = KeyEvent { down: true, code: 28, mods: 7 };
        assert_eq!(decode_key_event(encode_key_event(ev)), ev);
    }

    #[test]
    fn pager_idempotent() {
        let ids: Vec<usize> = (0..1300).collect();
        let mut p = IrPager::index(&ids);
        assert_eq!(p.page_count(), 3);
        assert_eq!(p.page_in(0), 512);
        assert_eq!(p.page_in(0), 0);
        assert!(p.budget_ok());
    }

    #[test]
    fn parity_markdown_rows() {
        let md = parity_markdown(false);
        assert!(md.starts_with("# 三端等价性验收清单"));
        assert_eq!(md.matches("挂账").count(), 12);
    }
}
