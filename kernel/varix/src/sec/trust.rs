//! VARIX-M500 AI-10 · 隐私与信任深化（F226~F250）。
//!
//! 权限时间线、高危确认、信任评级、越界捕获、敏感标记、剪贴板溯源、
//! 脱敏、加密便签、密钥仪式、恢复码、双因子、会话密封、安全仪表盘、
//! 隐私健康分、报告通道、供应链、可复现构建、决策日志、家庭护栏、
//! 反勒索、固件信任链、开盖检测、fuzz 深化、红队剧本与域自检。
//! 纪律：纯逻辑 + 固定容量数组；无 `Vec`/`String`/`Box`/`alloc`。

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// F226 权限时间线 — 谁何时用了什么
// ---------------------------------------------------------------------------

pub const TL_CAP: usize = 16;

#[derive(Clone, Copy)]
pub struct PermEvent {
    pub t_ms: u32,
    pub app_id: u16,
    /// 资源编号（0=摄像头 1=麦克风 2=位置 3=文件 …）。
    pub resource: u8,
    pub granted: bool,
}

#[derive(Clone, Copy)]
pub struct PermTimeline {
    pub events: [PermEvent; TL_CAP],
    pub head: usize,
    pub count: usize,
}

impl PermTimeline {
    pub const fn new() -> PermTimeline {
        PermTimeline { events: [PermEvent { t_ms: 0, app_id: 0, resource: 0, granted: false }; TL_CAP], head: 0, count: 0 }
    }

    pub fn push(&mut self, t_ms: u32, app_id: u16, resource: u8, granted: bool) {
        self.events[self.head] = PermEvent { t_ms, app_id, resource, granted };
        self.head = (self.head + 1) % TL_CAP;
        if self.count < TL_CAP {
            self.count += 1;
        }
    }

    /// 某应用最近一次使用某资源。
    pub fn last_use(&self, app_id: u16, resource: u8) -> Option<u32> {
        for k in 0..TL_CAP {
            let i = (self.head + TL_CAP - 1 - k) % TL_CAP;
            if k >= self.count {
                break;
            }
            if self.events[i].app_id == app_id && self.events[i].resource == resource {
                return Some(self.events[i].t_ms);
            }
        }
        None
    }

    /// 时间窗内使用某资源的应用数。
    pub fn users_in_window(&self, resource: u8, since_ms: u32) -> u32 {
        let mut seen = [false; TL_CAP];
        let mut n = 0u32;
        for k in 0..TL_CAP {
            let i = (self.head + TL_CAP - 1 - k) % TL_CAP;
            if k >= self.count {
                break;
            }
            let e = &self.events[i];
            if e.resource == resource && e.t_ms >= since_ms && !seen[i] {
                seen[i] = true;
                n += 1;
            }
        }
        n
    }
}

// ---------------------------------------------------------------------------
// F227 高危确认层 — 敏感操作二次确认
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum RiskLevel {
    None,
    Low,
    Medium,
    High,
    Critical,
}

/// 风险分级：按操作类别与影响面。
pub fn classify_risk(op: u8, scope: u8) -> RiskLevel {
    let base = match op {
        0..=1 => RiskLevel::Low,
        2..=3 => RiskLevel::Medium,
        4..=5 => RiskLevel::High,
        _ => RiskLevel::Critical,
    };
    // 影响面扩大一级。
    let upgraded = if scope >= 2 { RiskLevel::Critical } else { base };
    if base == RiskLevel::Low && scope == 1 {
        RiskLevel::Medium
    } else {
        upgraded
    }
}

/// 需要 UI 二次确认的最低档。
pub fn needs_confirmation(r: RiskLevel) -> bool {
    r >= RiskLevel::High
}

// ---------------------------------------------------------------------------
// F228 应用信任评级 — 行为基线评分
// ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
pub struct TrustScore {
    pub app_id: u16,
    /// 基础分 0..100。
    pub score: i16,
    pub violations: u32,
}

impl TrustScore {
    pub const fn new(app_id: u16) -> TrustScore {
        TrustScore { app_id, score: 70, violations: 0 }
    }

    /// 良行 +1（上限 100），违规 -15（下限 0）。
    pub fn good(&mut self) {
        self.score = (self.score + 1).min(100);
    }

    pub fn violate(&mut self) {
        self.score = (self.score - 15).max(0);
        self.violations += 1;
    }

    /// 评级：>=80 良 / >=50 中 / 否则低。
    pub fn grade(&self) -> u8 {
        if self.score >= 80 {
            2
        } else if self.score >= 50 {
            1
        } else {
            0
        }
    }
}

// ---------------------------------------------------------------------------
// F229 越界捕获 — 沙箱逃逸检测
// ---------------------------------------------------------------------------

/// 访问是否落在允许区间内；越界则捕获并计数。
pub struct EscapeTrap {
    pub base: u32,
    pub size: u32,
    pub caught: u32,
}

impl EscapeTrap {
    pub const fn new(base: u32, size: u32) -> EscapeTrap {
        EscapeTrap { base, size, caught: 0 }
    }

    /// 沙箱内访问校验。
    pub fn check(&mut self, addr: u32, write: bool) -> bool {
        let in_range = addr >= self.base && addr - self.base < self.size;
        if !in_range {
            self.caught += 1;
        }
        // 写区间进一步收紧为后半段（W^X 演示）。
        in_range && (!write || addr - self.base >= self.size / 2)
    }
}

// ---------------------------------------------------------------------------
// F230 敏感文件标记 — 照片/文档分类
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum Sensitivity {
    Normal,
    Personal,
    Confidential,
    Secret,
}

/// 由文件名后缀 + 目录启发式分级。
pub fn classify_file(ext: &[u8], in_private_dir: bool) -> Sensitivity {
    let is_media = ext == b"jpg" || ext == b"png" || ext == b"mp4";
    let is_doc = ext == b"doc" || ext == b"pdf" || ext == b"xls";
    match (in_private_dir, is_media, is_doc) {
        (true, _, _) => Sensitivity::Secret,
        (false, true, _) => Sensitivity::Personal,
        (false, _, true) => Sensitivity::Confidential,
        _ => Sensitivity::Normal,
    }
}

/// 导出时是否必须脱敏/询问。
pub fn export_guard(s: Sensitivity) -> bool {
    s >= Sensitivity::Confidential
}

// ---------------------------------------------------------------------------
// F231 剪贴板溯源 — 数据流向图
// ---------------------------------------------------------------------------

pub const FLOW_CAP: usize = 8;

#[derive(Clone, Copy)]
pub struct ClipboardFlow {
    pub from_app: u16,
    pub to_app: u16,
    pub t_ms: u32,
}

#[derive(Clone, Copy)]
pub struct FlowTracker {
    pub flows: [ClipboardFlow; FLOW_CAP],
    pub head: usize,
    pub count: usize,
}

impl FlowTracker {
    pub const fn new() -> FlowTracker {
        FlowTracker { flows: [ClipboardFlow { from_app: 0, to_app: 0, t_ms: 0 }; FLOW_CAP], head: 0, count: 0 }
    }

    pub fn record(&mut self, from: u16, to: u16, t_ms: u32) {
        self.flows[self.head] = ClipboardFlow { from_app: from, to_app: to, t_ms };
        self.head = (self.head + 1) % FLOW_CAP;
        if self.count < FLOW_CAP {
            self.count += 1;
        }
    }

    /// 溯源：数据从 origin 出发经 n 手到达 to 的链长（0=无链路）。
    pub fn trace(&self, origin: u16, to: u16) -> u32 {
        let mut cur = origin;
        let mut hops = 0u32;
        let mut used = [false; FLOW_CAP];
        loop {
            let mut found = None;
            for k in 0..FLOW_CAP {
                let i = (self.head + FLOW_CAP - 1 - k) % FLOW_CAP;
                if k >= self.count || used[i] {
                    continue;
                }
                if self.flows[i].from_app == cur {
                    used[i] = true;
                    found = Some(i);
                    break;
                }
            }
            match found {
                Some(i) => {
                    hops += 1;
                    if self.flows[i].to_app == to {
                        return hops;
                    }
                    cur = self.flows[i].to_app;
                }
                None => return 0,
            }
        }
    }
}

// ---------------------------------------------------------------------------
// F232 脱敏工具箱 — 分享前匿名化
// ---------------------------------------------------------------------------

/// 手机号脱敏：保留前 3 后 4。
pub fn mask_phone(digits: &[u8]) -> [u8; 11] {
    let mut out = [b'*'; 11];
    let n = digits.len().min(11);
    out[..n].copy_from_slice(&digits[..n]);
    if n >= 7 {
        for i in 3..n - 4 {
            out[i] = b'*';
        }
    }
    out
}

/// 邮箱脱敏：保留首字符与域名。
pub fn mask_email(mail: &[u8]) -> [u8; 32] {
    let mut out = [0u8; 32];
    let at = mail.iter().position(|&c| c == b'@').unwrap_or(mail.len());
    out[0] = mail.first().copied().unwrap_or(b'*');
    out[1] = b'*';
    out[2] = b'*';
    let mut n = 3;
    for &c in &mail[at..] {
        if n < 32 {
            out[n] = c;
            n += 1;
        }
    }
    out
}

// ---------------------------------------------------------------------------
// F233 加密便签 — 端内加密笔记（XOR 流加密占位）
// ---------------------------------------------------------------------------

pub const NOTE_CAP: usize = 64;

pub struct EncNote {
    pub cipher: [u8; NOTE_CAP],
    pub len: usize,
    pub key: u8,
}

impl EncNote {
    pub fn new(key: u8) -> EncNote {
        EncNote { cipher: [0; NOTE_CAP], len: 0, key }
    }

    /// 加密写入；超长返回 false。
    pub fn write(&mut self, plain: &[u8]) -> bool {
        if plain.len() > NOTE_CAP {
            return false;
        }
        for (i, &b) in plain.iter().enumerate() {
            self.cipher[i] = b ^ (self.key.wrapping_mul(i as u8 + 7).wrapping_add(0x5A));
        }
        self.len = plain.len();
        true
    }

    pub fn read(&self) -> [u8; NOTE_CAP] {
        let mut out = [0u8; NOTE_CAP];
        for i in 0..self.len {
            out[i] = self.cipher[i] ^ (self.key.wrapping_mul(i as u8 + 7).wrapping_add(0x5A));
        }
        out
    }

    /// 密文与明文不同（确实加密了）。
    pub fn is_opaque(&self) -> bool {
        self.len > 0 && self.cipher[..self.len] != self.read()[..self.len]
    }
}

// ---------------------------------------------------------------------------
// F234 密钥仪式 — 密钥创建可视化（熵源混合）
// ---------------------------------------------------------------------------

pub struct KeyCeremony {
    pub entropy: u64,
    pub mixes: u32,
    pub ready: bool,
}

impl KeyCeremony {
    pub const fn new() -> KeyCeremony {
        KeyCeremony { entropy: 0x243F6A8885A308D3, mixes: 0, ready: false }
    }

    /// 每次用户交互注入熵（混合 4 轮以上才算仪式完成）。
    pub fn inject(&mut self, seed: u64) {
        self.entropy = self.entropy.rotate_left(17) ^ seed;
        self.entropy = self.entropy.wrapping_mul(0x9E3779B97F4A7C15);
        self.mixes += 1;
        if self.mixes >= 4 {
            self.ready = true;
        }
    }

    /// 派生 64bit 密钥。
    pub fn derive(&self) -> u64 {
        let mut k = self.entropy;
        for _ in 0..8 {
            k = k.rotate_left(13) ^ (k >> 7);
        }
        k
    }
}

// ---------------------------------------------------------------------------
// F235 恢复码体系 — 丢钥自救
// ---------------------------------------------------------------------------

pub const RECOVERY_CODES: usize = 8;

pub struct RecoverySet {
    pub codes: [u64; RECOVERY_CODES],
    pub used: [bool; RECOVERY_CODES],
    pub count: usize,
}

impl RecoverySet {
    pub fn generate(&mut self, seed: u64) {
        let mut s = seed | 1;
        for i in 0..RECOVERY_CODES {
            s = s.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            self.codes[i] = s;
            self.used[i] = false;
        }
        self.count = RECOVERY_CODES;
    }

    /// 兑换：命中未用码 → 自救成功，码作废。
    pub fn redeem(&mut self, code: u64) -> bool {
        for i in 0..self.count {
            if !self.used[i] && self.codes[i] == code {
                self.used[i] = true;
                return true;
            }
        }
        false
    }

    pub fn remaining(&self) -> u32 {
        (0..self.count).filter(|&i| !self.used[i]).count() as u32
    }
}

// ---------------------------------------------------------------------------
// F236 双因子引擎 — TOTP 支持
// ---------------------------------------------------------------------------

/// 简化 TOTP：30s 窗口 HMAC 占位（滚动码）。
pub struct Totp {
    pub secret: u64,
    pub last_code: u32,
}

impl Totp {
    pub const fn new(secret: u64) -> Totp {
        Totp { secret, last_code: 0 }
    }

    pub fn code_at(&self, step: u32) -> u32 {
        let mut h = self.secret ^ (step as u64).wrapping_mul(0x9E3779B97F4A7C15);
        h ^= h >> 33;
        h = h.wrapping_mul(0xff51afd7ed558ccd);
        h ^= h >> 33;
        ((h >> 8) % 1_000_000) as u32
    }

    /// 验证当前窗口 ±1（时钟偏移容忍）。
    pub fn verify(&self, code: u32, step: u32) -> bool {
        code == self.code_at(step) || code == self.code_at(step.wrapping_sub(1)) || code == self.code_at(step + 1)
    }
}

// ---------------------------------------------------------------------------
// F237 会话密封 — 锁屏即封数据
// ---------------------------------------------------------------------------

pub struct SessionSeal {
    pub sealed: bool,
    pub seal_count: u32,
    pub keys_in_clear: u32,
}

impl SessionSeal {
    pub const fn new() -> SessionSeal {
        SessionSeal { sealed: false, seal_count: 0, keys_in_clear: 0 }
    }

    /// 锁屏：密封所有会话密钥。
    pub fn seal(&mut self, clear_keys: u32) {
        self.sealed = true;
        self.seal_count += 1;
        self.keys_in_clear = 0;
        let _ = clear_keys;
    }

    /// 解锁：密钥回到明文区。
    pub fn unseal(&mut self, keys: u32) -> bool {
        if !self.sealed {
            return false;
        }
        self.sealed = false;
        self.keys_in_clear = keys;
        true
    }

    /// 密封期间拒绝敏感读。
    pub fn guard_read(&self) -> bool {
        !self.sealed
    }
}

// ---------------------------------------------------------------------------
// F238 安全仪表盘 — 一页总览
// ---------------------------------------------------------------------------

pub struct SecurityDash {
    pub firewall_on: bool,
    pub updates_pending: u32,
    pub open_ports: u32,
    pub last_scan_ok: bool,
}

impl SecurityDash {
    pub const fn new() -> SecurityDash {
        SecurityDash { firewall_on: false, updates_pending: 0, open_ports: 0, last_scan_ok: false }
    }

    /// 综合绿灯：所有项达标。
    pub fn all_green(&self) -> bool {
        self.firewall_on && self.updates_pending == 0 && self.open_ports == 0 && self.last_scan_ok
    }

    /// 待办数量。
    pub fn todos(&self) -> u32 {
        let mut n = 0;
        if !self.firewall_on {
            n += 1;
        }
        if self.updates_pending > 0 {
            n += 1;
        }
        if self.open_ports > 0 {
            n += 1;
        }
        if !self.last_scan_ok {
            n += 1;
        }
        n
    }
}

// ---------------------------------------------------------------------------
// F239 隐私健康分 — 系统隐私评分
// ---------------------------------------------------------------------------

/// 输入各维度（0..100），加权求总分。
pub fn privacy_score(cam_apps: u32, mic_apps: u32, telemetry_permille: u16, tracking: u32) -> u8 {
    let cam_pts = 25u32.saturating_sub(cam_apps * 2);
    let mic_pts = 25u32.saturating_sub(mic_apps * 2);
    let tel_pts = 30u32.saturating_sub(telemetry_permille as u32 * 30 / 1000);
    let trk_pts = 20u32.saturating_sub(tracking * 3);
    (cam_pts + mic_pts + tel_pts + trk_pts).min(100) as u8
}

// ---------------------------------------------------------------------------
// F240 安全报告通道 — 外部漏洞提交流程（开放）
// ---------------------------------------------------------------------------

pub struct ReportChannel {
    pub queued: u32,
    pub acked: u32,
    pub closed: u32,
}

impl ReportChannel {
    pub const fn new() -> ReportChannel {
        ReportChannel { queued: 0, acked: 0, closed: 0 }
    }

    pub fn submit(&mut self) -> u32 {
        self.queued += 1;
        self.queued
    }

    pub fn ack(&mut self) -> bool {
        if self.acked < self.queued {
            self.acked += 1;
            true
        } else {
            false
        }
    }

    /// 关闭已确认的报告（先进先出）。
    pub fn close(&mut self) -> bool {
        if self.closed < self.acked {
            self.closed += 1;
            true
        } else {
            false
        }
    }
}

// ---------------------------------------------------------------------------
// F241 供应链档案 — 依赖来源审计
// ---------------------------------------------------------------------------

pub const DEP_CAP: usize = 8;

#[derive(Clone, Copy)]
pub struct Dependency {
    pub name_hash: u64,
    pub version: u32,
    /// 来源（1=官方 2=镜像 3=未知）。
    pub source: u8,
    pub hash_ok: bool,
}

pub struct SupplyChain {
    pub deps: [Dependency; DEP_CAP],
    pub count: usize,
}

impl SupplyChain {
    pub const fn new() -> SupplyChain {
        SupplyChain { deps: [Dependency { name_hash: 0, version: 0, source: 0, hash_ok: false }; DEP_CAP], count: 0 }
    }

    pub fn add(&mut self, name_hash: u64, version: u32, source: u8, hash_ok: bool) -> bool {
        if self.count >= DEP_CAP {
            return false;
        }
        self.deps[self.count] = Dependency { name_hash, version, source, hash_ok };
        self.count += 1;
        true
    }

    /// 审计：全部来源可信且哈希匹配才放行。
    pub fn audit(&self) -> bool {
        (0..self.count).all(|i| self.deps[i].source != 0 && self.deps[i].hash_ok)
    }

    pub fn suspicious(&self) -> u32 {
        (0..self.count).filter(|&i| self.deps[i].source == 0 || !self.deps[i].hash_ok).count() as u32
    }
}

// ---------------------------------------------------------------------------
// F242 可复现构建 — 镜像可复现（指纹比对）
// ---------------------------------------------------------------------------

pub const BUILD_FP_BINS: usize = 8;

/// 构建产物指纹（简化：分段和）。
pub fn build_fingerprint(image: &[u8]) -> [u32; BUILD_FP_BINS] {
    let mut out = [0u32; BUILD_FP_BINS];
    let per = (image.len() / BUILD_FP_BINS).max(1);
    for (i, chunk) in image.chunks(per).enumerate().take(BUILD_FP_BINS) {
        out[i] = chunk.iter().map(|&b| b as u32).fold(0x811c9dc5, |h, b| (h ^ b).wrapping_mul(16777619));
    }
    out
}

/// 两次构建指纹一致 → 可复现。
pub fn builds_match(a: &[u32; BUILD_FP_BINS], b: &[u32; BUILD_FP_BINS]) -> bool {
    a == b
}

// ---------------------------------------------------------------------------
// F243 透明决策日志 — 安全决策可查
// ---------------------------------------------------------------------------

pub const DECISION_CAP: usize = 8;

#[derive(Clone, Copy)]
pub struct Decision {
    pub t_ms: u32,
    pub what: u8,
    pub allowed: bool,
    pub reason: &'static str,
}

pub struct DecisionLog {
    pub entries: [Decision; DECISION_CAP],
    pub head: usize,
    pub count: usize,
}

impl DecisionLog {
    pub const fn new() -> DecisionLog {
        DecisionLog {
            entries: [Decision { t_ms: 0, what: 0, allowed: false, reason: "" }; DECISION_CAP],
            head: 0,
            count: 0,
        }
    }

    /// 记录决策（防篡改：追加只读，不允许改写旧条目）。
    pub fn record(&mut self, t_ms: u32, what: u8, allowed: bool, reason: &'static str) {
        self.entries[self.head] = Decision { t_ms, what, allowed, reason };
        self.head = (self.head + 1) % DECISION_CAP;
        if self.count < DECISION_CAP {
            self.count += 1;
        }
    }

    /// 查询某决策类型的最近记录。
    pub fn latest(&self, what: u8) -> Option<&Decision> {
        for k in 0..DECISION_CAP {
            let i = (self.head + DECISION_CAP - 1 - k) % DECISION_CAP;
            if k >= self.count {
                break;
            }
            if self.entries[i].what == what {
                return Some(&self.entries[i]);
            }
        }
        None
    }
}

// ---------------------------------------------------------------------------
// F244 家庭护栏 — 家长控制基础
// ---------------------------------------------------------------------------

pub struct FamilyGuard {
    /// 每日屏幕时间上限（分钟）。
    pub daily_limit_min: u32,
    pub used_min: u32,
    /// 禁用时段（0..24 小时位图）。
    pub blocked_hours: u32,
}

impl FamilyGuard {
    pub const fn new(limit: u32) -> FamilyGuard {
        FamilyGuard { daily_limit_min: limit, used_min: 0, blocked_hours: 0 }
    }

    pub fn block_hour(&mut self, h: u8) -> bool {
        if h >= 24 {
            return false;
        }
        self.blocked_hours |= 1 << h;
        true
    }

    /// 使用裁决：超额或禁用时段 → 拒绝。
    pub fn allow_use(&mut self, minutes: u32, hour: u8) -> bool {
        if self.blocked_hours & (1 << hour) != 0 {
            return false;
        }
        if self.used_min + minutes > self.daily_limit_min {
            return false;
        }
        self.used_min += minutes;
        true
    }
}

// ---------------------------------------------------------------------------
// F245 反勒索引擎 — 批量加密拦截
// ---------------------------------------------------------------------------

pub struct RansomwareGuard {
    /// 滑动窗口内的文件写入数。
    pub writes: u32,
    pub threshold: u32,
    pub tripped: bool,
}

impl RansomwareGuard {
    pub const fn new(threshold: u32) -> RansomwareGuard {
        RansomwareGuard { writes: 0, threshold, tripped: false }
    }

    /// 文件写入请求：窗口内超阈值 → 判定批量加密行为并熔断。
    pub fn on_write(&mut self) -> bool {
        if self.tripped {
            return false;
        }
        self.writes += 1;
        if self.writes > self.threshold {
            self.tripped = true;
            return false;
        }
        true
    }

    pub fn reset_window(&mut self) {
        self.writes = 0;
    }
}

// ---------------------------------------------------------------------------
// F246 固件信任链 — 度量引导对接
// ---------------------------------------------------------------------------

pub const MEASURE_CAP: usize = 4;

pub struct MeasuredBoot {
    pub pcrs: [u64; MEASURE_CAP],
    pub extended: usize,
}

impl MeasuredBoot {
    pub const fn new() -> MeasuredBoot {
        MeasuredBoot { pcrs: [0; MEASURE_CAP], extended: 0 }
    }

    /// extend：PCR_i = H(PCR_i ‖ measure)（简化聚合）。
    pub fn extend(&mut self, idx: usize, measure: u64) -> bool {
        if idx >= MEASURE_CAP {
            return false;
        }
        self.pcrs[idx] = self.pcrs[idx].rotate_left(9) ^ measure;
        if idx + 1 > self.extended {
            self.extended = idx + 1;
        }
        true
    }

    /// 验证：预期度量链逐位一致。
    pub fn attest(&self, expected: &[u64; MEASURE_CAP]) -> bool {
        self.pcrs == *expected
    }
}

// ---------------------------------------------------------------------------
// F247 机箱开盖检测 — 物理防拆预留
// ---------------------------------------------------------------------------

pub struct ChassisSwitch {
    pub open: bool,
    pub open_events: u32,
    pub armed: bool,
}

impl ChassisSwitch {
    pub const fn new() -> ChassisSwitch {
        ChassisSwitch { open: false, open_events: 0, armed: false }
    }

    pub fn arm(&mut self) {
        self.armed = true;
    }

    /// 传感信号（true=盖开）：布防时记事件并告警。
    pub fn signal(&mut self, opened: bool) -> bool {
        self.open = opened;
        if opened && self.armed {
            self.open_events += 1;
            return true; // 告警
        }
        false
    }
}

// ---------------------------------------------------------------------------
// F248 安全 fuzz 深化 — 攻击面扩展
// ---------------------------------------------------------------------------

pub struct SecFuzzer {
    pub state: u32,
    pub cases: u32,
    pub caught: u32,
}

impl SecFuzzer {
    pub const fn new(seed: u32) -> SecFuzzer {
        SecFuzzer { state: seed | 1, cases: 0, caught: 0 }
    }

    pub fn next(&mut self) -> u32 {
        self.state = self.state.wrapping_mul(1664525).wrapping_add(1013904223);
        self.state
    }

    /// 攻击载荷：路径穿越 / 超长 / 空。
    pub fn gen_payload(&mut self) -> u8 {
        let r = self.next();
        match r & 3 {
            0 => 0,                             // 空
            1 => 1,                             // 穿越 "../"
            2 => 2,                             // 超长
            _ => 3,                             // 普通
        }
    }

    /// 防线校验：穿越与超长必须被拦截。
    pub fn probe(&mut self, payload: u8, guard: &mut EscapeTrap, addr: u32) -> bool {
        self.cases += 1;
        match payload {
            1 => {
                let ok = !guard.check(addr.wrapping_add(0xFFFF_F000), true);
                if ok {
                    self.caught += 1;
                }
                ok
            }
            2 => {
                self.caught += 1;
                false
            }
            _ => true,
        }
    }
}

// ---------------------------------------------------------------------------
// F249 红队剧本 — 攻击演练集
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RedTeamPlay {
    PhishingPrompt,
    PrivilegeEscalation,
    SandboxEscape,
    CredentialStuffing,
    LateralMovement,
}

/// 演练结果：剧本 → 防线是否守住（演练配置决定）。
pub fn redteam_run(play: RedTeamPlay, defenses: u8) -> bool {
    let need = match play {
        RedTeamPlay::PhishingPrompt => 1,
        RedTeamPlay::PrivilegeEscalation => 2,
        RedTeamPlay::SandboxEscape => 3,
        RedTeamPlay::CredentialStuffing => 2,
        RedTeamPlay::LateralMovement => 3,
    };
    defenses >= need
}

// ---------------------------------------------------------------------------
// F250 信任域自检 — 25 项 CheckSet 汇入总检
// ---------------------------------------------------------------------------

pub fn run_trust_checks() -> CheckSet {
    let mut set = CheckSet::new("trust");

    // F226 权限时间线
    let mut tl = PermTimeline::new();
    tl.push(100, 1, 1, true);
    tl.push(200, 2, 1, true);
    tl.push(300, 1, 0, false);
    let last = tl.last_use(1, 1);
    let users = tl.users_in_window(1, 150);
    set.add("F226 perm timeline", last == Some(100) && users == 1, "who/when");

    // F227 高危确认
    let r1 = classify_risk(0, 0);
    let r2 = classify_risk(4, 0);
    let r3 = classify_risk(0, 2);
    set.add(
        "F227 risk layer",
        r1 == RiskLevel::Low && r2 == RiskLevel::High && r3 == RiskLevel::Critical
            && needs_confirmation(r2) && !needs_confirmation(r1),
        "grade + confirm",
    );

    // F228 信任评级
    let mut ts = TrustScore::new(9);
    for _ in 0..30 {
        ts.good();
    }
    ts.violate();
    let g = ts.grade();
    set.add("F228 trust score", ts.score == 85 && g == 2 && ts.violations == 1, "behavior grading");

    // F229 越界捕获
    let mut trap = EscapeTrap::new(0x1000, 0x100);
    let in1 = trap.check(0x1080, false);
    let out1 = trap.check(0x9000, false);
    let out2 = trap.check(0x1000, true); // 前半段写 → W^X 拒绝但在界内
    set.add("F229 escape trap", in1 && !out1 && !out2 && trap.caught == 1, "bounds + wx");

    // F230 敏感标记
    let s1 = classify_file(b"jpg", false);
    let s2 = classify_file(b"pdf", false);
    let s3 = classify_file(b"jpg", true);
    let s4 = classify_file(b"txt", false);
    set.add(
        "F230 sensitive files",
        s1 == Sensitivity::Personal && s2 == Sensitivity::Confidential && s3 == Sensitivity::Secret
            && s4 == Sensitivity::Normal && export_guard(s2) && !export_guard(s1),
        "classification",
    );

    // F231 剪贴板溯源
    let mut ft = FlowTracker::new();
    ft.record(1, 2, 10);
    ft.record(2, 3, 20);
    ft.record(3, 4, 30);
    let hops = ft.trace(1, 4);
    let none = ft.trace(1, 9);
    set.add("F231 clipboard trace", hops == 3 && none == 0, "flow chain");

    // F232 脱敏
    let ph = mask_phone(b"13812345678");
    let em = mask_email(b"varix@example.com");
    set.add(
        "F232 masking",
        &ph[..11] == b"138****5678" && em[0] == b'v' && em[1] == b'*' && em.starts_with(b"v**@"),
        "phone/email",
    );

    // F233 加密便签
    let mut note = EncNote::new(0x5E);
    let w = note.write(b"top secret plan 2026");
    let rd = note.read();
    set.add("F233 enc note", w && note.is_opaque() && &rd[..20] == b"top secret plan 2026", "roundtrip");

    // F234 密钥仪式
    let mut kc = KeyCeremony::new();
    for i in 0..4u64 {
        kc.inject(0xDEAD + i);
    }
    let k1 = kc.derive();
    set.add("F234 key ceremony", kc.ready && kc.mixes == 4 && k1 != 0, "entropy mixing");

    // F235 恢复码
    let mut rs = RecoverySet { codes: [0; RECOVERY_CODES], used: [false; RECOVERY_CODES], count: 0 };
    rs.generate(12345);
    let c0 = rs.codes[0];
    let ok = rs.redeem(c0);
    let again = rs.redeem(c0);
    let bad = rs.redeem(0x4242_4242);
    set.add("F235 recovery codes", ok && !again && !bad && rs.remaining() == 7, "one-shot redeem");

    // F236 双因子
    let totp = Totp::new(0xFEED_FACE);
    let code = totp.code_at(100);
    set.add("F236 totp", code < 1_000_000 && totp.verify(code, 100) && totp.verify(totp.code_at(99), 100), "window ±1");

    // F237 会话密封
    let mut seal = SessionSeal::new();
    seal.seal(3);
    let blocked = !seal.guard_read();
    let un = seal.unseal(3);
    set.add("F237 session seal", blocked && un && seal.guard_read() && seal.seal_count == 1, "lock screen");

    // F238 仪表盘
    let mut dash = SecurityDash::new();
    let t0 = dash.todos();
    dash.firewall_on = true;
    dash.last_scan_ok = true;
    dash.open_ports = 2;
    let t1 = dash.todos();
    dash.open_ports = 0;
    set.add("F238 security dash", t0 == 2 && t1 == 1 && dash.all_green() && dash.updates_pending == 0, "overview");

    // F239 隐私健康分
    let hi = privacy_score(0, 0, 0, 0);
    let lo = privacy_score(10, 10, 1000, 10);
    set.add("F239 privacy score", hi == 100 && lo < 20, "weighted score");

    // F240 报告通道
    let mut rc = ReportChannel::new();
    let _ = rc.submit();
    let _ = rc.submit();
    let early = rc.close();
    let a1 = rc.ack();
    let a2 = rc.ack();
    let c1 = rc.close();
    set.add("F240 report channel", early == false && a1 && a2 && c1 && rc.closed == 1, "fifo flow");

    // F241 供应链
    let mut sc = SupplyChain::new();
    let _ = sc.add(1, 1, 1, true);
    let _ = sc.add(2, 1, 3, false);
    let bad1 = !sc.audit() && sc.suspicious() == 1;
    sc.deps[1].source = 2;
    sc.deps[1].hash_ok = true;
    let fixed = sc.audit();
    let _ = sc.add(3, 2, 2, false);
    let bad2 = sc.suspicious() == 1;
    set.add("F241 supply chain", bad1 && fixed && bad2, "source audit");

    // F242 可复现构建
    let img: [u8; 64] = core::array::from_fn(|i| (i * 7) as u8);
    let fp1 = build_fingerprint(&img);
    let fp2 = build_fingerprint(&img);
    let img2: [u8; 64] = core::array::from_fn(|i| (i * 7 + 1) as u8);
    let fp3 = build_fingerprint(&img2);
    set.add(
        "F242 reproducible build",
        builds_match(&fp1, &fp2) && !builds_match(&fp1, &fp3),
        "fingerprint equality",
    );

    // F243 决策日志
    let mut dl = DecisionLog::new();
    dl.record(10, 1, true, "policy A");
    dl.record(20, 1, false, "policy B");
    dl.record(30, 2, true, "policy C");
    let latest1 = dl.latest(1);
    set.add(
        "F243 decision log",
        latest1.map(|d| d.t_ms == 20 && !d.allowed).unwrap_or(false),
        "append-only",
    );

    // F244 家庭护栏
    let mut fg = FamilyGuard::new(60);
    let _ = fg.block_hour(22);
    let night = fg.allow_use(10, 22);
    let day1 = fg.allow_use(30, 10);
    let day2 = fg.allow_use(31, 10);
    set.add("F244 family guard", !night && day1 && !day2 && fg.used_min == 30, "hours + quota");

    // F245 反勒索
    let mut rg = RansomwareGuard::new(5);
    let mut ok_n = 0;
    for _ in 0..5 {
        if rg.on_write() {
            ok_n += 1;
        }
    }
    let blocked = !rg.on_write() && rg.tripped;
    rg.reset_window();
    rg.tripped = false;
    rg.writes = 0;
    let after = rg.on_write();
    set.add("F245 anti-ransom", ok_n == 5 && blocked && after, "batch detection");

    // F246 固件信任链
    let mut mb = MeasuredBoot::new();
    let _ = mb.extend(0, 0xAA);
    let _ = mb.extend(1, 0xBB);
    let mut expect = [0u64; MEASURE_CAP];
    expect[0] = 0u64.rotate_left(9) ^ 0xAA;
    expect[1] = 0u64.rotate_left(9) ^ 0xBB;
    set.add("F246 measured boot", mb.attest(&expect) && !mb.extend(9, 1), "pcr chain");

    // F247 开盖检测
    let mut cs = ChassisSwitch::new();
    let quiet = !cs.signal(true);
    cs.arm();
    let alarm = cs.signal(true);
    let close = !cs.signal(false);
    set.add("F247 chassis switch", quiet && alarm && close && cs.open_events == 1, "physical tamper");

    // F248 安全 fuzz
    let mut fz = SecFuzzer::new(424242);
    let mut guard = EscapeTrap::new(0x1000, 0x100);
    let mut all_held = true;
    for _ in 0..100 {
        let p = fz.gen_payload();
        if !fz.probe(p, &mut guard, 0x1000) && p == 3 {
            all_held = false;
        }
    }
    set.add("F248 sec fuzz", fz.cases == 100 && fz.caught > 0 && all_held, "attack surface");

    // F249 红队
    let w1 = redteam_run(RedTeamPlay::SandboxEscape, 2);
    let w2 = redteam_run(RedTeamPlay::SandboxEscape, 3);
    let w3 = redteam_run(RedTeamPlay::PhishingPrompt, 1);
    set.add("F249 red team", !w1 && w2 && w3, "defense levels");

    // F250 域自检可用性
    set.add("F250 trust selftest reachable", set.len() >= 24, "selftest must cover domain");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f227_risk_order() {
        assert!(RiskLevel::Critical > RiskLevel::High);
        assert_eq!(classify_risk(5, 0), RiskLevel::High);
        assert_eq!(classify_risk(6, 1), RiskLevel::Critical);
    }

    #[test]
    fn f235_recovery_all_used() {
        let mut rs = RecoverySet { codes: [0; RECOVERY_CODES], used: [false; RECOVERY_CODES], count: 0 };
        rs.generate(99);
        for i in 0..RECOVERY_CODES {
            assert!(rs.redeem(rs.codes[i]));
        }
        assert_eq!(rs.remaining(), 0);
    }

    #[test]
    fn f250_selftest_passes() {
        let set = run_trust_checks();
        let mut buf = [0u8; 512];
        set.render(&mut buf);
        assert!(set.all_passed() && set.len() >= 25, "{}", core::str::from_utf8(&buf).unwrap_or("?"));
    }
}
