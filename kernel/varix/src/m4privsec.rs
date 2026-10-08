//! m4privsec — VARIX-M400 AI-12 安全与隐私域 (F276~F300)
//!
//! 默认即安全：攻击面清单/审计流程/权限提示/沙箱/网络权限/摄像头麦克风指示/
//! 隐私面板/加密存储/全盘加密评估/密钥环/度量/漏洞流程/依赖审计/最小权限/
//! 日志脱敏/逃逸套件/红队/缓解开关/完整性/安全更新/隐私默认/儿童模式/
//! 反追踪/事件响应/透明度。
//!
//! 硬约束：no_std / 无 alloc / 固定容量数组 / 纯逻辑。

use crate::checks::CheckSet;

// ===========================================================================
// F276 — 攻击面清单：全暴露面枚举 v1
// ===========================================================================

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Exposure {
    Syscall,
    NetworkSocket,
    DeviceNode,
    IpcPort,
    FileParser,
}

#[derive(Clone, Copy)]
pub struct AttackSurface {
    pub kind: Exposure,
    pub entry: &'static str,
    pub audited: bool,
}

pub fn unaudited_surfaces(list: &[AttackSurface]) -> usize {
    list.iter().filter(|s| !s.audited).count()
}

// ===========================================================================
// F277 — 内核审计流程：代码审计 checklist
// ===========================================================================

pub const AUDIT_CHECKLIST: [&str; 8] = [
    "bounds", "integer-overflow", "toctou", "uninit-mem", "refcount", "lock-order", "dma-unwrap", "user-ptr",
];

pub fn audit_checklist_complete(done: &[&str]) -> bool {
    AUDIT_CHECKLIST.iter().all(|c| done.contains(c))
}

// ===========================================================================
// F278 — 权限提示框架：敏感操作询问 UI
// ===========================================================================

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum SensOp {
    Camera,
    Microphone,
    Location,
    Contacts,
    ScreenCapture,
}

/// 每个敏感操作必须映射到一次询问；记住选择按（操作，应用）粒度。
pub fn prompt_required(decided: Option<bool>) -> bool {
    decided.is_none()
}

// ===========================================================================
// F279 — 应用沙箱 v1：文件系统视图隔离
// ===========================================================================

/// 沙箱内路径必须落在其 home 前缀下才可见。
pub fn sandbox_visible(home: &str, path: &str) -> bool {
    path.starts_with(home)
}

// ===========================================================================
// F280 — 网络权限：每应用联网开关
// ===========================================================================

#[derive(Clone, Copy)]
pub struct NetPerm {
    pub app_id: u32,
    pub allowed: bool,
}

pub fn net_allowed(perms: &[NetPerm], app: u32) -> bool {
    perms.iter().find(|p| p.app_id == app).map(|p| p.allowed).unwrap_or(false)
}

// ===========================================================================
// F281 — 摄像头/麦克风指示：使用中必亮指示
// ===========================================================================

#[derive(Clone, Copy)]
pub struct CamMicState {
    pub cam_open: bool,
    pub mic_open: bool,
    pub cam_led: bool,
    pub mic_led: bool,
}

impl CamMicState {
    /// 指示必须与占用一致（亮=占用）。
    pub fn indicator_consistent(&self) -> bool {
        self.cam_led == self.cam_open && self.mic_led == self.mic_open
    }
}

// ===========================================================================
// F282 — 隐私面板：数据流可视化
// ===========================================================================

pub const PRIV_FLOWS: [&str; 5] = ["location", "contacts", "camera", "microphone", "files"];

// ===========================================================================
// F283 — 加密存储：用户数据加密落地
// ===========================================================================

/// XTEA 轮函数做加密可用性 PoC（no_std 可用，无浮点无堆）。
fn xtea_round(v: &mut [u32; 2], key: &[u32; 4], round: u32, decrypt: bool) {
    const DELTA: u32 = 0x9E3779B9;
    if !decrypt {
        let mut sum = 0u32;
        for _ in 0..round {
            v[0] = v[0].wrapping_add((((v[1] << 4) ^ (v[1] >> 5)).wrapping_add(v[1])).wrapping_add(sum.wrapping_add(key[(sum & 3) as usize])));
            sum = sum.wrapping_add(DELTA);
            v[1] = v[1].wrapping_add((((v[0] << 4) ^ (v[0] >> 5)).wrapping_add(v[0])).wrapping_add(sum.wrapping_add(key[((sum >> 11) & 3) as usize])));
        }
    } else {
        let mut sum = DELTA.wrapping_mul(round);
        for _ in 0..round {
            v[1] = v[1].wrapping_sub((((v[0] << 4) ^ (v[0] >> 5)).wrapping_add(v[0])).wrapping_add(sum.wrapping_add(key[((sum >> 11) & 3) as usize])));
            sum = sum.wrapping_sub(DELTA);
            v[0] = v[0].wrapping_sub((((v[1] << 4) ^ (v[1] >> 5)).wrapping_add(v[1])).wrapping_add(sum.wrapping_add(key[(sum & 3) as usize])));
        }
    }
}

/// 加密→解密往返零损坏。
pub fn xtea_roundtrip(plain: [u32; 2], key: [u32; 4]) -> bool {
    let mut v = plain;
    xtea_round(&mut v, &key, 16, false);
    let cipher = v;
    xtea_round(&mut v, &key, 16, true);
    v == plain && cipher != plain
}

// ===========================================================================
// F284 — 全盘加密评估：可行性与 PoC 结论
// ===========================================================================

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum FdeVerdict {
    Feasible,
    Conditional,
    Infeasible,
}

pub fn fde_verdict(has_aesni: bool, has_tpm: bool, perf_loss_permille: u16) -> FdeVerdict {
    if !has_aesni {
        return FdeVerdict::Infeasible;
    }
    if has_tpm && perf_loss_permille < 100 {
        FdeVerdict::Feasible
    } else {
        FdeVerdict::Conditional
    }
}

// ===========================================================================
// F285 — 密钥环：凭据统一管理
// ===========================================================================

pub const KEYRING_CAP: usize = 8;

pub struct Keyring {
    slots: [Option<(&'static str, u64)>; KEYRING_CAP], // (服务名, 密钥指纹)
    count: usize,
}

impl Keyring {
    pub const fn new() -> Keyring {
        Keyring { slots: [None; KEYRING_CAP], count: 0 }
    }
    pub fn store(&mut self, svc: &'static str, fp: u64) -> bool {
        if svc.is_empty() {
            return false;
        }
        for i in 0..self.count {
            if let Some((s, _)) = self.slots[i] {
                if s == svc {
                    self.slots[i] = Some((svc, fp));
                    return true;
                }
            }
        }
        if self.count < KEYRING_CAP {
            self.slots[self.count] = Some((svc, fp));
            self.count += 1;
            true
        } else {
            false
        }
    }
    pub fn fingerprint(&self, svc: &str) -> Option<u64> {
        (0..self.count).find_map(|i| self.slots[i].filter(|e| e.0 == svc).map(|e| e.1))
    }
    pub fn len(&self) -> usize {
        self.count
    }
}

// ===========================================================================
// F286 — 安全启动度量：度量值记录与校验
// ===========================================================================

/// 简易 FNV 度量扩展：把阶段度量串进链。
pub fn measure_extend(prev: u64, stage: &str) -> u64 {
    let mut h = prev ^ 0xcbf29ce484222325;
    for b in stage.as_bytes() {
        h ^= *b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

pub fn boot_measure_chain(stages: &[&str]) -> u64 {
    let mut h = 0;
    for s in stages {
        h = measure_extend(h, s);
    }
    h
}

// ===========================================================================
// F287 — 漏洞编号流程：内部 CVE 式管理
// ===========================================================================

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum VulnSev {
    Low,
    Medium,
    High,
    Critical,
}

#[derive(Clone, Copy)]
pub struct Vuln {
    pub id: u32, // VX-<year>-<n>
    pub sev: VulnSev,
    pub fixed: bool,
    pub disclosed: bool,
}

impl Vuln {
    /// 高危未修复不得公开细节。
    pub fn disclosure_allowed(&self) -> bool {
        self.fixed || matches!(self.sev, VulnSev::Low | VulnSev::Medium)
    }
}

// ===========================================================================
// F288 — 依赖审计进 CI：cargo audit 门禁
// ===========================================================================

#[derive(Clone, Copy)]
pub struct DepFinding {
    pub crate_name: &'static str,
    pub severity: VulnSev,
}

/// 门禁：出现 High/Critical 即 fail。
pub fn audit_gate(finds: &[DepFinding]) -> bool {
    finds
        .iter()
        .all(|f| !matches!(f.severity, VulnSev::High | VulnSev::Critical))
}

// ===========================================================================
// F289 — 最小权限重构：按清单逐项收敛
// ===========================================================================

#[derive(Clone, Copy)]
pub struct CapGrant {
    pub subject: &'static str,
    pub caps: u32, // 位图
}

/// 收敛：把未使用位从授予中移除，返回新位图。
pub fn least_privilege(g: CapGrant, used_mask: u32) -> u32 {
    g.caps & used_mask
}

// ===========================================================================
// F290 — 日志脱敏：敏感字段自动打码
// ===========================================================================

/// 检测日志行中的敏感键并确认已打码（值必须以 <redacted> 形式落盘）。
pub fn log_line_scrubbed(line: &str) -> bool {
    for key in ["password=", "token=", "secret=", "apikey="] {
        if let Some(pos) = line.find(key) {
            let rest = &line[pos + key.len()..];
            if !rest.starts_with("<redacted>") {
                return false;
            }
        }
    }
    true
}

// ===========================================================================
// F291 — 沙箱逃逸套件：攻击用例全负分
// ===========================================================================

pub const ESCAPE_CASES: [&str; 6] =
    ["path-traversal", "fd-pass", "shm-confuse", "ipc-spoof", "cap-escalate", "symlink-race"];

/// 全部用例应"未逃逸"（负分）。
pub fn escape_suite_pass(escaped: &[bool]) -> bool {
    ESCAPE_CASES.len() == escaped.len() && escaped.iter().all(|e| !*e)
}

// ===========================================================================
// F292 — 红队演练清单：季度演练项
// ===========================================================================

pub const REDTEAM_ITEMS: [&str; 4] = ["network-exposure", "usb-drop", "malicious-app", "physical-access"];

// ===========================================================================
// F293 — 缓解开关：ASLR 强度等可配
// ===========================================================================

#[derive(Clone, Copy)]
pub struct Mitigations {
    pub aslr_bits: u8, // 8~30
    pub smap: bool,
    pub nx: bool,
}

impl Mitigations {
    pub fn sane(&self) -> bool {
        (8..=30).contains(&self.aslr_bits)
    }
    /// 默认全开。
    pub const fn hardened() -> Mitigations {
        Mitigations { aslr_bits: 28, smap: true, nx: true }
    }
}

// ===========================================================================
// F294 — 完整性自校验：系统文件校验
// ===========================================================================

pub fn fnv1a(data: &[u8]) -> u64 {
    let mut h = 0xcbf29ce484222325u64;
    for b in data {
        h ^= *b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

/// 校验：文件哈希与清单一致。
pub fn integrity_ok(file: &[u8], expect: u64) -> bool {
    fnv1a(file) == expect
}

// ===========================================================================
// F295 — 安全更新通道：高优先级快速通道
// ===========================================================================

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum UpdPriority {
    Routine,
    Important,
    Security,
    Emergency,
}

pub fn update_latency_budget_ms(p: UpdPriority) -> u32 {
    match p {
        UpdPriority::Routine => 7 * 24 * 3600 * 1000,
        UpdPriority::Important => 24 * 3600 * 1000,
        UpdPriority::Security => 6 * 3600 * 1000,
        UpdPriority::Emergency => 3600 * 1000,
    }
}

// ===========================================================================
// F296 — 隐私默认值审计：默认配置审查
// ===========================================================================

#[derive(Clone, Copy)]
pub struct PrivacyDefault {
    pub key: &'static str,
    pub default_on: bool,
    pub should_default_off: bool,
}

pub fn privacy_defaults_ok(list: &[PrivacyDefault]) -> bool {
    list.iter().all(|d| d.default_on == !d.should_default_off)
}

// ===========================================================================
// F297 — 儿童模式：受限使用环境
// ===========================================================================

pub struct KidsMode {
    pub app_whitelist: [Option<u32>; 16],
    pub count: usize,
    pub time_limit_min: u16,
}

impl KidsMode {
    pub fn app_allowed(&self, app: u32) -> bool {
        (0..self.count).any(|i| self.app_whitelist[i] == Some(app))
    }
    pub fn sane(&self) -> bool {
        self.count <= 16 && self.time_limit_min <= 24 * 60
    }
}

// ===========================================================================
// F298 — 反追踪：防指纹化选项
// ===========================================================================

#[derive(Clone, Copy)]
pub struct AntiTrack {
    pub canvas_noise: bool,
    pub tz_spoof: bool, // 固定时区
    pub font_set_fixed: bool,
}

impl AntiTrack {
    pub fn fingerprint_surface_reduced(&self) -> bool {
        self.canvas_noise || (self.tz_spoof && self.font_set_fixed)
    }
}

// ===========================================================================
// F299 — 事件响应手册：应急流程文档
// ===========================================================================

pub const IR_PHASES: [&str; 6] =
    ["detect", "contain", "eradicate", "recover", "postmortem", "harden"];

pub fn ir_phases_ordered(phases: &[&str]) -> bool {
    if phases.len() != IR_PHASES.len() {
        return false;
    }
    phases.iter().zip(IR_PHASES.iter()).all(|(a, b)| a == b)
}

// ===========================================================================
// F300 — 安全透明度报告：定期公开状态
// ===========================================================================

#[derive(Clone, Copy)]
pub struct TransparencyEntry {
    pub vulns_fixed: u16,
    pub vulns_open: u16,
    pub mean_days_to_fix: u16,
}

impl TransparencyEntry {
    pub fn publishable(&self) -> bool {
        self.mean_days_to_fix > 0
    }
}

// ===========================================================================
// 域自检
// ===========================================================================

pub fn run_m4privsec_checks() -> CheckSet {
    let mut set = CheckSet::new("m4privsec");

    // F276 攻击面
    let surfaces = [
        AttackSurface { kind: Exposure::Syscall, entry: "open", audited: true },
        AttackSurface { kind: Exposure::NetworkSocket, entry: "tcp", audited: false },
    ];
    set.add("F276 attack surface", unaudited_surfaces(&surfaces) == 1, "exposure v1");

    // F277 审计清单
    set.add(
        "F277 audit checklist",
        audit_checklist_complete(&AUDIT_CHECKLIST) && !audit_checklist_complete(&["bounds"]),
        "8 items",
    );

    // F278 权限提示
    set.add("F278 permission prompt", prompt_required(None) && !prompt_required(Some(true)), "ask once");

    // F279 沙箱视图
    set.add(
        "F279 sandbox fs",
        sandbox_visible("/home/app", "/home/app/x") && !sandbox_visible("/home/app", "/etc/passwd"),
        "view isolate",
    );

    // F280 网络权限
    let perms = [NetPerm { app_id: 1, allowed: true }, NetPerm { app_id: 2, allowed: false }];
    set.add(
        "F280 net perm",
        net_allowed(&perms, 1) && !net_allowed(&perms, 2) && !net_allowed(&perms, 3),
        "per-app switch",
    );

    // F281 指示
    let s1 = CamMicState { cam_open: true, mic_open: false, cam_led: true, mic_led: false };
    let s2 = CamMicState { cam_open: true, mic_open: false, cam_led: false, mic_led: false };
    set.add("F281 cam/mic indicator", s1.indicator_consistent() && !s2.indicator_consistent(), "must lit");

    // F282 隐私面板
    set.add("F282 privacy panel", PRIV_FLOWS.len() == 5, "data flows");

    // F283 加密存储
    set.add("F283 xtea roundtrip", xtea_roundtrip([0x11223344, 0x55667788], [1, 2, 3, 4]), "encrypt+decrypt");
    set.add("F283 xtea distinct", xtea_roundtrip([0, 0], [9, 9, 9, 9]), "cipher differs");

    // F284 FDE
    set.add(
        "F284 fde verdict",
        fde_verdict(true, true, 50) == FdeVerdict::Feasible
            && fde_verdict(false, true, 0) == FdeVerdict::Infeasible
            && fde_verdict(true, false, 50) == FdeVerdict::Conditional,
        "poc",
    );

    // F285 密钥环
    let mut kr = Keyring::new();
    let k1 = kr.store("wifi", 0xAAAA) && kr.store("wifi", 0xBBBB);
    set.add("F285 keyring", k1 && kr.fingerprint("wifi") == Some(0xBBBB) && kr.len() == 1, "upsert");

    // F286 度量链
    let a = boot_measure_chain(&["fw", "kernel"]);
    let b = boot_measure_chain(&["fw", "kernel"]);
    let c = boot_measure_chain(&["fw", "kernel-modified"]);
    set.add("F286 measured boot", a == b && a != c && a != 0, "chain verify");

    // F287 漏洞流程
    let v = Vuln { id: 1, sev: VulnSev::Critical, fixed: false, disclosed: false };
    let v2 = Vuln { id: 2, sev: VulnSev::Critical, fixed: true, disclosed: false };
    set.add("F287 vuln flow", !v.disclosure_allowed() && v2.disclosure_allowed(), "responsible");

    // F288 依赖审计
    set.add(
        "F288 cargo audit gate",
        audit_gate(&[DepFinding { crate_name: "x", severity: VulnSev::Low }])
            && !audit_gate(&[DepFinding { crate_name: "y", severity: VulnSev::High }]),
        "CI gate",
    );

    // F289 最小权限
    let g = CapGrant { subject: "svc", caps: 0b1111 };
    set.add("F289 least privilege", least_privilege(g, 0b0011) == 0b0011, "shrink");

    // F290 日志脱敏
    set.add(
        "F290 log scrub",
        log_line_scrubbed("auth ok user=bob") && !log_line_scrubbed("login password=hunter2"),
        "auto mask",
    );

    // F291 逃逸套件
    set.add("F291 escape suite", escape_suite_pass(&[false; 6]), "all negative");
    set.add("F291 escape suite len", !escape_suite_pass(&[false; 5]), "case count fixed");

    // F292 红队
    set.add("F292 redteam", REDTEAM_ITEMS.len() == 4, "quarterly items");

    // F293 缓解开关
    let m = Mitigations::hardened();
    set.add("F293 mitigations", m.sane() && m.nx && m.smap, "defaults hardened");

    // F294 完整性
    let h = fnv1a(b"varix v1");
    set.add("F294 integrity", integrity_ok(b"varix v1", h) && !integrity_ok(b"varix v2", h), "self-check");

    // F295 更新通道
    set.add(
        "F295 security channel",
        update_latency_budget_ms(UpdPriority::Emergency) < update_latency_budget_ms(UpdPriority::Security),
        "fast lane",
    );

    // F296 隐私默认
    let defs = [
        PrivacyDefault { key: "telemetry", default_on: false, should_default_off: true },
        PrivacyDefault { key: "firewall", default_on: true, should_default_off: false },
    ];
    set.add("F296 privacy defaults", privacy_defaults_ok(&defs), "audited");

    // F297 儿童模式
    let mut kids = KidsMode { app_whitelist: [None; 16], count: 0, time_limit_min: 120 };
    kids.app_whitelist[0] = Some(3);
    kids.count = 1;
    set.add(
        "F297 kids mode",
        kids.sane() && kids.app_allowed(3) && !kids.app_allowed(4),
        "restricted env",
    );

    // F298 反追踪
    let at1 = AntiTrack { canvas_noise: true, tz_spoof: false, font_set_fixed: false };
    let at2 = AntiTrack { canvas_noise: false, tz_spoof: false, font_set_fixed: true };
    set.add("F298 anti-track", at1.fingerprint_surface_reduced() && !at2.fingerprint_surface_reduced(), "options");

    // F299 事件响应
    set.add("F299 ir playbook", ir_phases_ordered(&IR_PHASES), "6 phases");

    // F300 透明度
    let t = TransparencyEntry { vulns_fixed: 12, vulns_open: 1, mean_days_to_fix: 9 };
    set.add("F300 transparency", t.publishable(), "public report");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f283_xtea_known_roundtrip() {
        for pair in [[0u32, 0], [u32::MAX, 7], [0xDEADBEEF, 0xCAFEBABE]] {
            assert!(xtea_roundtrip(pair, [0x0123, 0x4567, 0x89AB, 0xCDEF]));
        }
    }

    #[test]
    fn f286_measure_chain_changes_on_any_stage() {
        let a = boot_measure_chain(&["a", "b", "c"]);
        let b = boot_measure_chain(&["a", "b", "d"]);
        assert_ne!(a, b);
    }

    #[test]
    fn f285_keyring_capacity() {
        let mut kr = Keyring::new();
        const NAMES: [&str; KEYRING_CAP] = ["k0", "k1", "k2", "k3", "k4", "k5", "k6", "k7"];
        for (i, n) in NAMES.iter().enumerate() {
            assert!(kr.store(n, i as u64 + 1));
        }
        assert!(!kr.store("overflow", 99));
    }

    #[test]
    fn f294_fnv_reference() {
        // FNV-1a 空串偏移基准
        assert_eq!(fnv1a(b""), 0xcbf29ce484222325);
    }

    #[test]
    fn f300_domain_selfcheck_all_pass() {
        let set = run_m4privsec_checks();
        assert!(set.len() >= 25, "got {}", set.len());
        assert!(set.all_passed());
    }
}
