//! m600sdk — VARIX-M600 AI-15 应用 SDK 与开发域 (F351~F375)
//!
//! 应用生命周期契约/窗口 API 精装版/通知 API 公约/权限网关/沙盒应用容器/
//! 应用清单规范/声明式 UI 框架/组件画廊/主题跟随 API/系统字体服务/
//! 图标服务总线/文件关联中枢/深链路由器/后台任务公约/应用商店骨架/
//! 签名与公证/更新频道管理/崩溃遥测公约/性能预算门/SDK 文档门户/
//! 示例应用舰队/API 版本化协议/兼容垫片层/开发者仪表盘/生态年报。
//!
//! 硬约束：no_std / 无 alloc / 无浮点（全部 permille/定点）/ 纯逻辑。

use crate::checks::CheckSet;

// ===========================================================================
// F351 — 应用生命周期契约：状态机统一
// ===========================================================================

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum AppLifecycle {
    Registered,
    Starting,
    Foreground,
    Background,
    Suspended,
    Terminated,
}

/// 生命周期迁移合法性表。
pub fn lifecycle_transition_ok(from: AppLifecycle, to: AppLifecycle) -> bool {
    use AppLifecycle::*;
    matches!(
        (from, to),
        (Registered, Starting)
            | (Starting, Foreground)
            | (Foreground, Background)
            | (Background, Foreground)
            | (Background, Suspended)
            | (Suspended, Background)
            | (Suspended, Terminated)
            | (Foreground, Terminated)
    )
}

// ===========================================================================
// F352 — 窗口 API 精装版：统一窗口描述符
// ===========================================================================

#[derive(Clone, Copy)]
pub struct WindowDesc {
    pub min_w: u16,
    pub min_h: u16,
    pub max_w: u16,
    pub max_h: u16,
    pub resizable: bool,
}

impl WindowDesc {
    /// 描述符自洽：min ≤ max 且最小可用面积 ≥ 64×64。
    pub fn sane(&self) -> bool {
        self.min_w <= self.max_w
            && self.min_h <= self.max_h
            && self.min_w >= 64
            && self.min_h >= 64
    }
    /// 请求尺寸被夹取到合法范围（定点钳制）。
    pub fn clamp(&self, w: u16, h: u16) -> (u16, u16) {
        (
            w.clamp(self.min_w, self.max_w),
            h.clamp(self.min_h, self.max_h),
        )
    }
}

pub const WINDOW_DEFAULT: WindowDesc =
    WindowDesc { min_w: 320, min_h: 240, max_w: 7680, max_h: 4320, resizable: true };

// ===========================================================================
// F353 — 通知 API 公约：通知必须可归组/可静默
// ===========================================================================

#[derive(Clone, Copy)]
pub struct Notification {
    pub app: &'static str,
    pub group: &'static str,
    pub silent: bool,
}

pub fn notification_compliant(n: &Notification) -> bool {
    !n.app.is_empty() && !n.group.is_empty()
}

// ===========================================================================
// F354 — 权限网关：申请→授予→校验
// ===========================================================================

pub const PERM_KINDS: [&str; 8] =
    ["camera", "mic", "files", "location", "net", "contacts", "notif", "automation"];

/// 位掩码权限：bit i 对应 PERM_KINDS[i]。
pub fn perm_bit(kind_index: usize) -> u32 {
    if kind_index < PERM_KINDS.len() {
        1u32 << kind_index
    } else {
        0
    }
}

/// 网关裁决：请求集必须是已授予集的子集才放行。
pub fn perm_granted(granted: u32, requested: u32) -> bool {
    requested & !granted == 0
}

// ===========================================================================
// F355 — 沙盒应用容器：能力集最小化
// ===========================================================================

#[derive(Clone, Copy)]
pub struct SandboxCaps {
    pub fs_read: bool,
    pub fs_write: bool,
    pub net: bool,
    pub ipc: bool,
}

/// 沙盒基线：默认只读 + 无网；写与网必须显式声明。
pub fn sandbox_baseline(caps: SandboxCaps) -> bool {
    caps.fs_read && !caps.fs_write && !caps.net
}

/// 写权限允许时必须同时具备读权限。
pub fn sandbox_consistent(caps: SandboxCaps) -> bool {
    !caps.fs_write || caps.fs_read
}

// ===========================================================================
// F356 — 应用清单规范：清单字段完整性
// ===========================================================================

#[derive(Clone, Copy)]
pub struct AppManifest {
    pub id: &'static str,
    pub version: u32,
    pub api_min: u16,
    pub perms: u32,
    pub entry: &'static str,
}

impl AppManifest {
    /// id 形如 "vendor.app"（含一个点，两端非空）。
    pub fn id_valid(&self) -> bool {
        let id = self.id.as_bytes();
        match id.iter().position(|&b| b == b'.') {
            Some(dot) => dot > 0 && dot + 1 < id.len(),
            None => false,
        }
    }
    pub fn complete(&self) -> bool {
        self.id_valid() && self.version > 0 && !self.entry.is_empty()
    }
}

// ===========================================================================
// F357 — 声明式 UI 框架：节点树静态校验
// ===========================================================================

pub const UI_NODE_KINDS: [&str; 6] = ["box", "text", "button", "image", "list", "input"];

#[derive(Clone, Copy)]
pub struct UiNode {
    pub kind_index: u8, // UI_NODE_KINDS 下标
    pub child_count: u8,
}

impl UiNode {
    pub fn kind_ok(&self) -> bool {
        (self.kind_index as usize) < UI_NODE_KINDS.len()
    }
    /// list 最多 64 项，其余节点最多 8 子；input 是叶子。
    pub fn arity_ok(&self) -> bool {
        if !self.kind_ok() {
            return false;
        }
        match UI_NODE_KINDS[self.kind_index as usize] {
            "input" => self.child_count == 0,
            "list" => self.child_count <= 64,
            _ => self.child_count <= 8,
        }
    }
}

// ===========================================================================
// F358 — 组件画廊：组件示例齐备
// ===========================================================================

pub const GALLERY_COMPONENTS: [&str; 8] =
    ["button", "card", "list", "dialog", "toggle", "slider", "tab", "toast"];

pub fn gallery_complete(present: &[&str]) -> bool {
    GALLERY_COMPONENTS.iter().all(|c| present.contains(c))
}

// ===========================================================================
// F359 — 主题跟随 API：应用取色跟随系统
// ===========================================================================

#[derive(Clone, Copy)]
pub struct ThemeColors {
    pub accent: u32,
    pub bg: u32,
    pub fg: u32,
}

/// 跟随合规：前景背景对比达标（亮度差 ≥ 400 permille）。
pub fn theme_follows_ok(t: ThemeColors) -> bool {
    let lum = |c: u32| {
        let r = (c >> 16) & 0xFF;
        let g = (c >> 8) & 0xFF;
        let b = c & 0xFF;
        (r as u32 * 299 + g as u32 * 587 + b as u32 * 114) / 1000
    };
    let (a, b) = (lum(t.bg), lum(t.fg));
    let d = if a > b { a - b } else { b - a };
    d * 1000 / 255 >= 400
}

// ===========================================================================
// F360 — 系统字体服务：字重族完备
// ===========================================================================

pub const FONT_WEIGHTS: [u16; 5] = [300, 400, 500, 700, 900];

pub fn font_weight_served(w: u16) -> bool {
    FONT_WEIGHTS.contains(&w)
}

// ===========================================================================
// F361 — 图标服务总线：尺寸阶梯
// ===========================================================================

pub const ICON_SIZES: [u16; 6] = [16, 24, 32, 48, 64, 128];

pub fn icon_size_served(size: u16) -> bool {
    ICON_SIZES.contains(&size)
}

// ===========================================================================
// F362 — 文件关联中枢：MIME→应用 裁决
// ===========================================================================

#[derive(Clone, Copy)]
pub struct FileAssociation {
    pub ext: &'static str,
    pub app_id: &'static str,
    pub user_pinned: bool,
}

/// 关联有效：扩展名与目标应用非空；用户钉选优先级最高。
pub fn association_valid(a: &FileAssociation) -> bool {
    !a.ext.is_empty() && a.ext.as_bytes()[0] != b'.' && !a.app_id.is_empty()
}

/// 在候选里挑优：钉选 > 顺序。
pub fn association_pick(cands: &[FileAssociation]) -> usize {
    let mut best = 0usize;
    for (i, c) in cands.iter().enumerate() {
        if c.user_pinned && !cands[best].user_pinned {
            best = i;
        }
    }
    best
}

// ===========================================================================
// F363 — 深链路由器：scheme://host/path 解析
// ===========================================================================

#[derive(Clone, Copy)]
pub struct DeepLink<'a> {
    pub scheme: &'a str,
    pub host: &'a str,
}

impl<'a> DeepLink<'a> {
    /// scheme 字母开头且全小写字母；host 非空。
    pub fn routable(&self) -> bool {
        let s = self.scheme.as_bytes();
        if s.is_empty() || !(s[0].is_ascii_lowercase()) {
            return false;
        }
        s.iter().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
            && !self.host.is_empty()
    }
}

// ===========================================================================
// F364 — 后台任务公约：后台执行须声明预算
// ===========================================================================

#[derive(Clone, Copy)]
pub struct BgTask {
    pub budget_ms: u32,
    pub network: bool,
}

/// 公约：后台单任务 ≤ 30s；带网任务 ≤ 10s。
pub fn bg_task_compliant(t: BgTask) -> bool {
    if t.network {
        t.budget_ms <= 10_000
    } else {
        t.budget_ms <= 30_000
    }
}

// ===========================================================================
// F365 — 应用商店骨架：上架审核门
// ===========================================================================

#[derive(Clone, Copy)]
pub struct StoreListing {
    pub manifest_ok: bool,
    pub signed: bool,
    pub screenshots: u8,
}

pub fn store_admittable(l: StoreListing) -> bool {
    l.manifest_ok && l.signed && l.screenshots >= 3
}

// ===========================================================================
// F366 — 签名与公证：摘要校验（简化 FNV-1a 32 位）
// ===========================================================================

pub fn fnv1a32(data: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    for &b in data {
        h ^= b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

/// 公证通过：重算摘要与登记摘要一致。
pub fn notarize_ok(data: &[u8], recorded: u32) -> bool {
    fnv1a32(data) == recorded
}

// ===========================================================================
// F367 — 更新频道管理：频道阶梯
// ===========================================================================

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum UpdateChannel {
    Stable,
    Beta,
    Dev,
}

/// 频道序：Dev 可收 Beta/Stable 的回退，Stable 只收 Stable。
pub fn channel_fallback_ok(sub: UpdateChannel, push: UpdateChannel) -> bool {
    use UpdateChannel::*;
    match sub {
        Stable => push == Stable,
        Beta => matches!(push, Beta | Stable),
        Dev => true,
    }
}

// ===========================================================================
// F368 — 崩溃遥测公约：脱敏 + 可归因
// ===========================================================================

#[derive(Clone, Copy)]
pub struct CrashReport {
    pub app_id: &'static str,
    pub code: u16,
    pub has_stack: bool,
    pub pii_scrubbed: bool,
}

pub fn crash_report_admissible(r: CrashReport) -> bool {
    !r.app_id.is_empty() && r.has_stack && r.pii_scrubbed
}

// ===========================================================================
// F369 — 性能预算门：冷启动/内存 双预算
// ===========================================================================

pub const COLD_START_BUDGET_MS: u32 = 800;
pub const APP_MEM_BUDGET_KIB: u32 = 64 * 1024;

pub fn perf_budget_ok(start_ms: u32, mem_kib: u32) -> bool {
    start_ms <= COLD_START_BUDGET_MS && mem_kib <= APP_MEM_BUDGET_KIB
}

// ===========================================================================
// F370 — SDK 文档门户：文档结构齐备
// ===========================================================================

pub const SDK_DOC_SECTIONS: [&str; 5] =
    ["quickstart", "api-ref", "guides", "samples", "changelog"];

pub fn sdk_docs_complete(present: &[&str]) -> bool {
    SDK_DOC_SECTIONS.iter().all(|s| present.contains(s))
}

// ===========================================================================
// F371 — 示例应用舰队：示例覆盖关键 API 面
// ===========================================================================

pub const SAMPLE_APPS: [&str; 6] =
    ["hello-window", "notif-demo", "perm-demo", "files-demo", "automation-demo", "net-demo"];

pub fn sample_fleet_ready(shipped: &[&str]) -> bool {
    SAMPLE_APPS.iter().all(|a| shipped.contains(a))
}

// ===========================================================================
// F372 — API 版本化协议：semver 兼容判定
// ===========================================================================

#[derive(Clone, Copy)]
pub struct ApiVersion {
    pub major: u16,
    pub minor: u16,
}

/// 同 major 且提供的 minor ≥ 所需 minor 即兼容。
pub fn api_compatible(provided: ApiVersion, required: ApiVersion) -> bool {
    provided.major == required.major && provided.minor >= required.minor
}

// ===========================================================================
// F373 — 兼容垫片层：旧 API 转发登记
// ===========================================================================

#[derive(Clone, Copy)]
pub struct ShimEntry {
    pub old_call: &'static str,
    pub target: &'static str,
    pub removed_in_major: u16,
}

/// 垫片可用：仍处于支持期（当前主版本 < 移除主版本）且有转发目标。
pub fn shim_alive(e: &ShimEntry, current_major: u16) -> bool {
    !e.old_call.is_empty() && !e.target.is_empty() && current_major < e.removed_in_major
}

// ===========================================================================
// F374 — 开发者仪表盘：指标卡齐备
// ===========================================================================

pub const DEV_DASH_CARDS: [&str; 5] =
    ["builds", "crashes", "perf", "perms", "reviews"];

pub fn dev_dash_complete(cards: &[&str]) -> bool {
    DEV_DASH_CARDS.iter().all(|c| cards.contains(c))
}

// ===========================================================================
// F375 — 生态年报：年度统计板块
// ===========================================================================

pub const ECO_REPORT_SECTIONS: [&str; 4] = ["apps", "downloads", "crash-rate", "top-perms"];

/// 年报就绪：四大板块齐 + 崩溃率 permille ≤ 20。
pub fn eco_report_ready(sections: &[&str], crash_rate_permille: u16) -> bool {
    ECO_REPORT_SECTIONS.iter().all(|s| sections.contains(s)) && crash_rate_permille <= 20
}

// ===========================================================================
// 域自检
// ===========================================================================

pub fn run_m600sdk_checks() -> CheckSet {
    let mut set = CheckSet::new("m600sdk");

    // F351 生命周期
    set.add(
        "F351 lifecycle forward",
        lifecycle_transition_ok(AppLifecycle::Registered, AppLifecycle::Starting)
            && lifecycle_transition_ok(AppLifecycle::Foreground, AppLifecycle::Background)
            && lifecycle_transition_ok(AppLifecycle::Suspended, AppLifecycle::Terminated),
        "legal edges",
    );
    set.add(
        "F351 lifecycle reject",
        !lifecycle_transition_ok(AppLifecycle::Terminated, AppLifecycle::Starting)
            && !lifecycle_transition_ok(AppLifecycle::Registered, AppLifecycle::Foreground),
        "illegal edges",
    );

    // F352 窗口 API
    set.add("F352 window sane", WINDOW_DEFAULT.sane(), "descriptor");
    let wd = WINDOW_DEFAULT;
    let (w, h) = wd.clamp(10_000, 100);
    set.add("F352 window clamp", w == 7680 && h == 240, "clamped to bounds");

    // F353 通知公约
    set.add(
        "F353 notification",
        notification_compliant(&Notification { app: "mail", group: "inbox", silent: false })
            && !notification_compliant(&Notification { app: "", group: "inbox", silent: true }),
        "group+app",
    );

    // F354 权限网关
    let granted = perm_bit(0) | perm_bit(4);
    set.add(
        "F354 perm gateway",
        perm_granted(granted, perm_bit(0)) && !perm_granted(granted, perm_bit(1)),
        "subset rule",
    );
    set.add("F354 perm kinds", PERM_KINDS.len() == 8 && perm_bit(7) == 0x80, "8 kinds");

    // F355 沙盒
    let base = SandboxCaps { fs_read: true, fs_write: false, net: false, ipc: false };
    set.add(
        "F355 sandbox baseline",
        sandbox_baseline(base) && !sandbox_baseline(SandboxCaps { fs_read: false, fs_write: false, net: true, ipc: false }),
        "read-only default",
    );
    set.add(
        "F355 sandbox consistent",
        sandbox_consistent(base)
            && !sandbox_consistent(SandboxCaps { fs_read: false, fs_write: true, net: false, ipc: false }),
        "write implies read",
    );

    // F356 清单
    let mf = AppManifest { id: "varix.demo", version: 1, api_min: 1, perms: 0, entry: "main" };
    set.add("F356 manifest complete", mf.complete(), "all fields");
    set.add(
        "F356 manifest id rules",
        !AppManifest { id: "nodot", version: 1, api_min: 1, perms: 0, entry: "m" }.id_valid()
            && !AppManifest { id: ".leading", version: 1, api_min: 1, perms: 0, entry: "m" }.id_valid(),
        "vendor.app form",
    );

    // F357 声明式 UI
    let list = UiNode { kind_index: 4, child_count: 64 };
    let input = UiNode { kind_index: 5, child_count: 0 };
    let bad = UiNode { kind_index: 5, child_count: 1 };
    let oob = UiNode { kind_index: 9, child_count: 0 };
    set.add("F357 ui list arity", list.arity_ok() && input.arity_ok(), "leaf/list");
    set.add(
        "F357 ui reject",
        !bad.arity_ok() && !oob.arity_ok(),
        "input leaf + kind bounds",
    );

    // F358 组件画廊
    set.add("F358 gallery", gallery_complete(&GALLERY_COMPONENTS), "8 components");

    // F359 主题跟随
    set.add(
        "F359 theme follow",
        theme_follows_ok(ThemeColors { accent: 0x0A58CE, bg: 0x1E1E1E, fg: 0xEAEAEA })
            && !theme_follows_ok(ThemeColors { accent: 0x0A58CE, bg: 0x808080, fg: 0x909090 }),
        "contrast gate",
    );

    // F360 字体
    set.add("F360 font weights", font_weight_served(400) && !font_weight_served(666), "5 weights");

    // F361 图标
    set.add("F361 icon sizes", icon_size_served(128) && !icon_size_served(100), "6 sizes");

    // F362 文件关联
    let assoc = [
        FileAssociation { ext: "txt", app_id: "editor", user_pinned: false },
        FileAssociation { ext: "txt", app_id: "notes", user_pinned: true },
    ];
    set.add(
        "F362 association valid",
        association_valid(&assoc[0]) && !association_valid(&FileAssociation { ext: "", app_id: "x", user_pinned: false }),
        "ext+app",
    );
    set.add("F362 association pinned first", association_pick(&assoc) == 1, "pin wins");

    // F363 深链
    let good = DeepLink { scheme: "varix", host: "settings" };
    let bad_scheme = DeepLink { scheme: "1bad", host: "x" };
    set.add(
        "F363 deeplink route",
        good.routable() && !bad_scheme.routable() && !DeepLink { scheme: "ok", host: "" }.routable(),
        "scheme+host",
    );

    // F364 后台任务
    set.add(
        "F364 bg task",
        bg_task_compliant(BgTask { budget_ms: 30_000, network: false })
            && !bg_task_compliant(BgTask { budget_ms: 30_001, network: false })
            && !bg_task_compliant(BgTask { budget_ms: 20_000, network: true }),
        "budgets",
    );

    // F365 商店
    set.add(
        "F365 store gate",
        store_admittable(StoreListing { manifest_ok: true, signed: true, screenshots: 3 })
            && !store_admittable(StoreListing { manifest_ok: true, signed: true, screenshots: 2 }),
        "listing",
    );

    // F366 签名公证
    let payload = b"appbin-v1";
    let digest = fnv1a32(payload);
    set.add(
        "F366 notarize",
        notarize_ok(payload, digest) && !notarize_ok(b"appbin-v2", digest),
        "digest match",
    );

    // F367 更新频道
    set.add(
        "F367 channels",
        !channel_fallback_ok(UpdateChannel::Stable, UpdateChannel::Beta)
            && channel_fallback_ok(UpdateChannel::Beta, UpdateChannel::Stable)
            && channel_fallback_ok(UpdateChannel::Dev, UpdateChannel::Dev),
        "ladder",
    );

    // F368 崩溃遥测
    set.add(
        "F368 crash telemetry",
        crash_report_admissible(CrashReport { app_id: "mail", code: 11, has_stack: true, pii_scrubbed: true })
            && !crash_report_admissible(CrashReport { app_id: "mail", code: 11, has_stack: true, pii_scrubbed: false }),
        "scrubbed only",
    );

    // F369 性能预算
    set.add(
        "F369 perf budget",
        perf_budget_ok(800, APP_MEM_BUDGET_KIB)
            && !perf_budget_ok(801, 1024)
            && !perf_budget_ok(100, APP_MEM_BUDGET_KIB + 1),
        "cold start+mem",
    );

    // F370 SDK 文档
    set.add("F370 sdk docs", sdk_docs_complete(&SDK_DOC_SECTIONS), "5 sections");

    // F371 示例舰队
    set.add("F371 sample fleet", sample_fleet_ready(&SAMPLE_APPS), "6 samples");

    // F372 API 版本
    set.add(
        "F372 api version",
        api_compatible(ApiVersion { major: 2, minor: 5 }, ApiVersion { major: 2, minor: 3 })
            && !api_compatible(ApiVersion { major: 2, minor: 2 }, ApiVersion { major: 2, minor: 3 })
            && !api_compatible(ApiVersion { major: 3, minor: 9 }, ApiVersion { major: 2, minor: 3 }),
        "semver",
    );

    // F373 垫片
    let shim = ShimEntry { old_call: "win.open", target: "window.open", removed_in_major: 4 };
    set.add(
        "F373 shim alive",
        shim_alive(&shim, 3) && !shim_alive(&shim, 4),
        "support window",
    );

    // F374 仪表盘
    set.add("F374 dev dashboard", dev_dash_complete(&DEV_DASH_CARDS), "5 cards");

    // F375 生态年报
    set.add(
        "F375 eco report",
        eco_report_ready(&ECO_REPORT_SECTIONS, 12)
            && !eco_report_ready(&ECO_REPORT_SECTIONS, 25)
            && !eco_report_ready(&["apps"], 1),
        "sections+rate",
    );

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f351_state_machine_edges() {
        assert!(!lifecycle_transition_ok(AppLifecycle::Starting, AppLifecycle::Suspended));
        assert!(lifecycle_transition_ok(AppLifecycle::Background, AppLifecycle::Suspended));
    }

    #[test]
    fn f352_window_clamp_bounds() {
        let (w, h) = WINDOW_DEFAULT.clamp(0, 0);
        assert_eq!((w, h), (320, 240));
        let (w, h) = WINDOW_DEFAULT.clamp(u16::MAX, u16::MAX);
        assert_eq!((w, h), (7680, 4320));
    }

    #[test]
    fn f354_bitmask_subset() {
        let g = perm_bit(0) | perm_bit(1) | perm_bit(2);
        assert!(perm_granted(g, 0));
        assert!(perm_granted(g, perm_bit(1) | perm_bit(2)));
        assert!(!perm_granted(g, perm_bit(3)));
    }

    #[test]
    fn f362_pin_priority() {
        let c = [
            FileAssociation { ext: "png", app_id: "viewer", user_pinned: false },
            FileAssociation { ext: "png", app_id: "paint", user_pinned: true },
            FileAssociation { ext: "png", app_id: "edit", user_pinned: false },
        ];
        assert_eq!(association_pick(&c), 1);
        assert_eq!(association_pick(&c[..1]), 0);
    }

    #[test]
    fn f366_fnv_digest() {
        assert_eq!(fnv1a32(b""), 0x811C_9DC5);
        assert_ne!(fnv1a32(b"a"), fnv1a32(b"b"));
    }

    #[test]
    fn f375_domain_selfcheck_all_pass() {
        let set = run_m600sdk_checks();
        assert!(set.len() >= 25, "got {}", set.len());
        assert!(set.all_passed());
    }
}
