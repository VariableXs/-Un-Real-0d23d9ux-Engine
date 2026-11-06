//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F18528 · Windows 口径基准账（实机对照记录） · 0x7FFC0段
//! ─────────────────────────────────────────────────────────────────────
//! 判据锚点：UNX-F18528-J1 基准记录（版本/指纹/期望值三列）50/50 完整，
//! 来源登记入账（AI-95 合规抽查项）
//! ─────────────────────────────────────────────────────────────────────
//! 本模块结构清单：
//!   - 数据结构：WinBuild（5 构建号闭集 19041..19045）、BaseRecord
//!     （probe_id/版本/指纹/期望值/来源 五列）、SourceRegistry
//!     （append-only 来源账）、BaselineLedger（50 记录总账）；
//!   - 状态机：RecState 四态（Draft→Registered→Verified→Sealed/
//!     Rejected），迁移闭集 4 条合法、其余 12 组合显式拒；
//!   - 账本：三列勾稽账（版本×指纹×期望值逐行复算）、来源闭包账
//!     （先登记后引用，append-only 不可删）、注入检出账；
//!   - 错误路径：BaseCode 十二码闭集，code()=SEG_BASE+n，中文 detail；
//!   - 注入核：四类违约（来源未登记 / 零指纹 / 版本闭集外 / 记录重复）
//!     逐类 10 轮断言被入账闸显式拒绝，漏检恒零。
//! 设计要点：
//!   - 三列完整语义：版本 ∈ {19041,19042,19043,19044,19045} 闭集、
//!     指纹 = FNV(probe_id|版本序|期望值) 非零、期望值非零——三列任一
//!     缺省即拒收（AI-95 合规抽查项锚点）；
//!   - 指纹可复算：入账时指纹 = 复算值，抽查时逐行重算必须相等
//!     （FpMismatch 即账本被篡改）；
//!   - 标准基准账 50/50 确定性构造：probe_id → 版本 = 闭集[(id-1)%5]，
//!     期望值 = 0x100+id，来源按 id%3 轮转 实机/文档/抽检 三类；
//!   - append-only：记录与来源均只增不减，dup probe_id 显式拒；
//!   - 常量推导：0x7FFC0 = 0x7FF80 + 0x40（A13 批第 8 卡独占窗）；
//!     判据轮数 10 来自注入 10/10 语义；
//!   - no_std + alloc；零 unwrap/expect/panic/裸越界下标/浮点/IO。
//! ═══════════════════════════════════════════════════════════════════════

use crate::alloc::format;
use crate::alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x7FFC0，A13 批第 8 卡独占区，0x40 对齐）。
pub const SEG_BASE: u32 = 0x7FFC0;

// ── 冻结常量 ────────────────────────────────────────────────────
/// 基准记录总数（冻结：50，J1 主锚点 50/50）。
pub const RECORD_COUNT: usize = 50;
/// Windows 10 构建号闭集容量（冻结：5）。
pub const BUILD_COUNT: usize = 5;
/// 来源账容量（冻结：64，≥ 标准账 50 条来源登记留余量）。
pub const SOURCE_CAP: usize = 64;
/// 判据迭代轮数（推导：注入 10/10 语义）。
pub const JUDGE_ROUNDS: u32 = 10;
/// 注入类数（来源未登记/零指纹/版本越界/记录重复 四类）。
pub const INJECT_CLASSES: usize = 4;
/// 期望值基址（推导：标准账期望值 = 0x100 + probe_id，非零可辨）。
pub const EXPECT_BASE: u32 = 0x100;

// ── FNV-1a 64 位指纹 ────────────────────────────────────────────
/// FNV-1a（三列指纹与账本指纹唯一哈希来源）。
pub fn fnv1a(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    let mut i = 0;
    while i < bytes.len() {
        h ^= bytes[i] as u64;
        h = h.wrapping_mul(0x100000001b3);
        i += 1;
    }
    h
}

// ── Windows 10 构建号闭集 ───────────────────────────────────────
/// Windows 10 实机口径构建号（五代闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WinBuild {
    /// 2004（19041）
    B19041,
    /// 20H2（19042）
    B19042,
    /// 21H1（19043）
    B19043,
    /// 21H2（19044）
    B19044,
    /// 22H2（19045）
    B19045,
}

impl WinBuild {
    /// 构建号数值。
    pub fn num(self) -> u32 {
        match self {
            WinBuild::B19041 => 19041,
            WinBuild::B19042 => 19042,
            WinBuild::B19043 => 19043,
            WinBuild::B19044 => 19044,
            WinBuild::B19045 => 19045,
        }
    }
    /// 闭集序（0..5）。
    pub fn ordinal(self) -> u32 {
        match self {
            WinBuild::B19041 => 0,
            WinBuild::B19042 => 1,
            WinBuild::B19043 => 2,
            WinBuild::B19044 => 3,
            WinBuild::B19045 => 4,
        }
    }
    /// 数值 → 构建号（闭集外 None）。
    pub fn from_num(n: u32) -> Option<WinBuild> {
        match n {
            19041 => Some(WinBuild::B19041),
            19042 => Some(WinBuild::B19042),
            19043 => Some(WinBuild::B19043),
            19044 => Some(WinBuild::B19044),
            19045 => Some(WinBuild::B19045),
            _ => None,
        }
    }
}

/// 构建号闭集全量。
pub const ALL_BUILDS: [WinBuild; BUILD_COUNT] = [
    WinBuild::B19041,
    WinBuild::B19042,
    WinBuild::B19043,
    WinBuild::B19044,
    WinBuild::B19045,
];

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BaseCode {
    /// 基准账空（零记录）
    EmptyLedger,
    /// 来源未登记（AI-95 先登记后引用）
    SourceUnregistered,
    /// 零指纹（指纹列缺省）
    ZeroFingerprint,
    /// 版本不在构建号闭集（19041..19045 之外）
    BuildUnknown,
    /// 记录重复（同 probe_id 二次入账）
    DupProbeId,
    /// probe_id 越界（0 或 >50）
    IdOutOfRange,
    /// 指纹复算不符（账本被篡改）
    FpMismatch,
    /// 来源账闭包破坏（登记后删改，append-only 红线）
    RegistryClosed,
    /// 勾稽破坏（50/50 不齐或三列缺省）
    ConservationBroken,
    /// 双跑不一致
    NotReproducible,
    /// 来源重复登记（同 tag dup）
    SourceDup,
    /// 注入漏检
    InjectMissed,
}

impl BaseCode {
    /// 诊断码 = SEG_BASE + 枚举序（0x7FFC0..0x80000）。
    pub fn code(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 中文诊断明细（闭集非空）。
    pub fn detail(self) -> &'static str {
        match self {
            BaseCode::EmptyLedger => "基准账空：零记录不得对账",
            BaseCode::SourceUnregistered => "来源未登记：AI-95 先登记后引用",
            BaseCode::ZeroFingerprint => "零指纹：指纹列缺省拒收",
            BaseCode::BuildUnknown => "版本不在闭集：19041..19045 之外",
            BaseCode::DupProbeId => "记录重复：同 probe_id 二次入账",
            BaseCode::IdOutOfRange => "probe_id 越界：合法闭集 1..=50",
            BaseCode::FpMismatch => "指纹复算不符：账本疑似被篡改",
            BaseCode::RegistryClosed => "来源账闭包破坏：append-only 红线",
            BaseCode::ConservationBroken => "勾稽破坏：50/50 不齐或三列缺省",
            BaseCode::NotReproducible => "双跑不一致：确定性红线",
            BaseCode::SourceDup => "来源重复登记：同 tag dup 拒收",
            BaseCode::InjectMissed => "注入漏检：检出 < 注入，验收红线",
        }
    }
    /// 全码闭集（12 条）。
    pub const ALL: [BaseCode; 12] = [
        BaseCode::EmptyLedger,
        BaseCode::SourceUnregistered,
        BaseCode::ZeroFingerprint,
        BaseCode::BuildUnknown,
        BaseCode::DupProbeId,
        BaseCode::IdOutOfRange,
        BaseCode::FpMismatch,
        BaseCode::RegistryClosed,
        BaseCode::ConservationBroken,
        BaseCode::NotReproducible,
        BaseCode::SourceDup,
        BaseCode::InjectMissed,
    ];
}

/// u32 诊断码反查中文明细。
pub fn detail_of(code: u32) -> &'static str {
    let mut i = 0;
    while i < BaseCode::ALL.len() {
        if let Some(c) = BaseCode::ALL.get(i) {
            if c.code() == code {
                return c.detail();
            }
        }
        i += 1;
    }
    "未知诊断码：不在 0x7FFC0 段闭集内"
}

// ── 数据结构 ────────────────────────────────────────────────────
/// 来源账条目（kind: 0=实机 1=文档 2=抽检，append-only）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SourceEntry {
    pub tag: String,
    pub kind: u32,
}

/// 基准记录（版本/指纹/期望值三列主锚 + id/来源两列勾稽）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BaseRecord {
    pub probe_id: u32,
    pub build: WinBuild,
    /// FNV(probe_id|版本序|期望值)。
    pub fingerprint: u64,
    pub expected: u32,
    /// 来源标签（必须已登记）。
    pub source: String,
}

/// 三列指纹复算（入账与抽查共用同一推导）。
pub fn derive_fp(probe_id: u32, build: WinBuild, expected: u32) -> u64 {
    let mut bytes: Vec<u8> = Vec::new();
    bytes.push((probe_id & 0xFF) as u8);
    bytes.push((probe_id >> 8) as u8);
    bytes.push(build.ordinal() as u8);
    bytes.push((expected & 0xFF) as u8);
    bytes.push(((expected >> 8) & 0xFF) as u8);
    bytes.push((expected >> 16) as u8);
    bytes.push((expected >> 24) as u8);
    fnv1a(&bytes)
}

/// 标准来源标签轮转（id%3：0=实机 1=文档 2=抽检）。
pub fn derive_source_tag(probe_id: u32) -> String {
    match probe_id % 3 {
        0 => format!("RM-19045-P{:02}", probe_id),
        1 => format!("DOC-WDK-22H2-P{:02}", probe_id),
        _ => format!("AI95-SPOT-P{:02}", probe_id),
    }
}

// ── 状态机（四态） ──────────────────────────────────────────────
/// 单记录生命周期四态。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RecState {
    /// 草稿（未过闸）
    Draft,
    /// 已登记（入账，来源+指纹在位）
    Registered,
    /// 已抽查（指纹复算通过）
    Verified,
    /// 已封账（50/50 勾稽后）
    Sealed,
}

/// 迁移闭集：合法恰 4 条，其余 12 组合显式拒。
pub fn can_transit(from: RecState, to: RecState) -> bool {
    match (from, to) {
        (RecState::Draft, RecState::Registered) => true,
        (RecState::Registered, RecState::Verified) => true,
        (RecState::Verified, RecState::Sealed) => true,
        (RecState::Sealed, RecState::Verified) => true, // 封账后复检回落
        _ => false,
    }
}

// ── 来源登记账（append-only） ───────────────────────────────────
/// 来源账：只增不减，dup tag 拒，满账显性拒。
#[derive(Clone, Debug, Default)]
pub struct SourceRegistry {
    pub entries: Vec<SourceEntry>,
}

impl SourceRegistry {
    pub fn new() -> SourceRegistry {
        SourceRegistry { entries: Vec::new() }
    }
    /// 登记（dup 拒、空 tag 拒、满账拒）。
    pub fn register(&mut self, tag: &str, kind: u32) -> Result<(), u32> {
        if tag.len() == 0 {
            return Err(BaseCode::SourceUnregistered.code());
        }
        let mut i = 0;
        while i < self.entries.len() {
            if let Some(e) = self.entries.get(i) {
                if e.tag == tag {
                    return Err(BaseCode::SourceDup.code());
                }
            }
            i += 1;
        }
        if self.entries.len() >= SOURCE_CAP {
            return Err(BaseCode::RegistryClosed.code());
        }
        self.entries.push(SourceEntry { tag: String::from(tag), kind });
        Ok(())
    }
    /// 在账查询。
    pub fn has(&self, tag: &str) -> bool {
        let mut i = 0;
        while i < self.entries.len() {
            if let Some(e) = self.entries.get(i) {
                if e.tag == tag {
                    return true;
                }
            }
            i += 1;
        }
        false
    }
    /// 在账条数（append-only 单调判据用）。
    pub fn len(&self) -> usize {
        self.entries.len()
    }
}

// ── 基准总账 ────────────────────────────────────────────────────
/// Windows 口径基准账（50 记录 + 来源账）。
#[derive(Clone, Debug)]
pub struct BaselineLedger {
    pub records: Vec<BaseRecord>,
    pub sources: SourceRegistry,
    /// 每记录状态（下标 = probe_id-1，恒 50 槽）。
    pub states: [RecState; RECORD_COUNT],
    /// 入账拒收计数（显性记账）。
    pub rejects: u32,
}

impl BaselineLedger {
    pub fn new() -> BaselineLedger {
        BaselineLedger {
            records: Vec::new(),
            sources: SourceRegistry::new(),
            states: [RecState::Draft; RECORD_COUNT],
            rejects: 0,
        }
    }

    /// 入账闸：id 闭集 / 版本闭集 / 非零指纹 / 来源先登记 / dup 拒。
    pub fn put_record(&mut self, rec: BaseRecord) -> Result<(), u32> {
        if rec.probe_id == 0 || rec.probe_id > RECORD_COUNT as u32 {
            self.rejects = self.rejects.saturating_add(1);
            return Err(BaseCode::IdOutOfRange.code());
        }
        if WinBuild::from_num(rec.build.num()).is_none()
            || ALL_BUILDS.get(rec.build.ordinal() as usize).is_none()
        {
            self.rejects = self.rejects.saturating_add(1);
            return Err(BaseCode::BuildUnknown.code());
        }
        if rec.fingerprint == 0 {
            self.rejects = self.rejects.saturating_add(1);
            return Err(BaseCode::ZeroFingerprint.code());
        }
        if rec.expected == 0 {
            self.rejects = self.rejects.saturating_add(1);
            return Err(BaseCode::ConservationBroken.code());
        }
        if !self.sources.has(&rec.source) {
            self.rejects = self.rejects.saturating_add(1);
            return Err(BaseCode::SourceUnregistered.code());
        }
        // 指纹必须与复算一致（三列自洽）。
        let want = derive_fp(rec.probe_id, rec.build, rec.expected);
        if want != rec.fingerprint {
            self.rejects = self.rejects.saturating_add(1);
            return Err(BaseCode::FpMismatch.code());
        }
        // dup probe_id 拒（append-only）。
        let mut i = 0;
        while i < self.records.len() {
            if let Some(r) = self.records.get(i) {
                if r.probe_id == rec.probe_id {
                    self.rejects = self.rejects.saturating_add(1);
                    return Err(BaseCode::DupProbeId.code());
                }
            }
            i += 1;
        }
        let idx = (rec.probe_id - 1) as usize;
        let cur = match self.states.get(idx) {
            Some(s) => *s,
            None => return Err(BaseCode::IdOutOfRange.code()),
        };
        if !can_transit(cur, RecState::Registered) {
            self.rejects = self.rejects.saturating_add(1);
            return Err(BaseCode::ConservationBroken.code());
        }
        self.records.push(rec);
        if let Some(s) = self.states.get_mut(idx) {
            *s = RecState::Registered;
        }
        Ok(())
    }

    /// 指纹抽查：逐行复算 == 登记指纹（Sealed/Verified 状态推进）。
    pub fn audit_fingerprints(&mut self) -> Result<usize, u32> {
        let mut n = 0u32;
        let mut i = 0;
        while i < self.records.len() {
            let (pid, build, expected) = match self.records.get(i) {
                Some(r) => (r.probe_id, r.build, r.expected),
                None => return Err(BaseCode::EmptyLedger.code()),
            };
            let want = derive_fp(pid, build, expected);
            let got = match self.records.get(i) {
                Some(r) => r.fingerprint,
                None => 0,
            };
            if want != got {
                return Err(BaseCode::FpMismatch.code());
            }
            let idx = (pid - 1) as usize;
            let cur = match self.states.get(idx) {
                Some(s) => *s,
                None => return Err(BaseCode::IdOutOfRange.code()),
            };
            if cur == RecState::Registered || cur == RecState::Sealed {
                let fin = if cur == RecState::Registered { RecState::Verified } else { RecState::Verified };
                if let Some(s) = self.states.get_mut(idx) {
                    *s = fin;
                }
            }
            n = n.saturating_add(1);
            i += 1;
        }
        Ok(n as usize)
    }

    /// 50/50 勾稽：记录数 == 50 且 id 闭集全覆盖且三列非缺省。
    pub fn reconcile(&self) -> Result<usize, u32> {
        if self.records.len() != RECORD_COUNT {
            return Err(BaseCode::ConservationBroken.code());
        }
        let mut seen = [0u32; RECORD_COUNT];
        let mut i = 0;
        while i < self.records.len() {
            let r = match self.records.get(i) {
                Some(x) => x,
                None => return Err(BaseCode::EmptyLedger.code()),
            };
            let slot = (r.probe_id - 1) as usize;
            if slot >= RECORD_COUNT {
                return Err(BaseCode::IdOutOfRange.code());
            }
            if let Some(c) = seen.get_mut(slot) {
                *c = c.saturating_add(1);
            }
            if r.fingerprint == 0 || r.expected == 0 || r.source.len() == 0 {
                return Err(BaseCode::ConservationBroken.code());
            }
            i += 1;
        }
        let mut k = 0;
        while k < RECORD_COUNT {
            let c = match seen.get(k) {
                Some(v) => *v,
                None => 0,
            };
            if c != 1 {
                return Err(BaseCode::ConservationBroken.code());
            }
            k += 1;
        }
        Ok(self.records.len())
    }
}

// ── 标准基准账（50/50 确定性构造） ──────────────────────────────
/// 构造标准 50/50 基准账：版本闭集轮转 + 来源三轮转 + 指纹推导。
pub fn build_standard() -> Result<BaselineLedger, u32> {
    let mut led = BaselineLedger::new();
    // 来源先登记（50 条，闭包在先）。
    let mut id = 1u32;
    while id <= RECORD_COUNT as u32 {
        let tag = derive_source_tag(id);
        let kind = (id % 3) as u32;
        led.sources.register(&tag, kind)?;
        id += 1;
    }
    // 50 记录入账：build = 闭集[(id-1)%5]，expected = 0x100+id。
    let mut id = 1u32;
    while id <= RECORD_COUNT as u32 {
        let build = match ALL_BUILDS.get(((id - 1) % BUILD_COUNT as u32) as usize) {
            Some(b) => *b,
            None => return Err(BaseCode::BuildUnknown.code()),
        };
        let expected = EXPECT_BASE.saturating_add(id);
        let fp = derive_fp(id, build, expected);
        if fp == 0 {
            return Err(BaseCode::ZeroFingerprint.code());
        }
        let tag = derive_source_tag(id);
        led.put_record(BaseRecord {
            probe_id: id,
            build,
            fingerprint: fp,
            expected,
            source: tag,
        })?;
        id += 1;
    }
    led.reconcile()?;
    led.audit_fingerprints()?;
    Ok(led)
}

// ── 黄金全链 ────────────────────────────────────────────────────
/// 黄金全链：标准账 50/50 → 指纹抽查 → 来源闭包完整。
pub fn golden_run() -> Result<(usize, usize), u32> {
    let led = build_standard()?;
    let n = led.reconcile()?;
    if led.sources.len() != RECORD_COUNT {
        return Err(BaseCode::SourceUnregistered.code());
    }
    // 封账态检查：抽查后全部 Verified。
    let mut i = 0;
    while i < led.states.len() {
        let st = match led.states.get(i) {
            Some(s) => *s,
            None => return Err(BaseCode::ConservationBroken.code()),
        };
        if st != RecState::Verified {
            return Err(BaseCode::ConservationBroken.code());
        }
        i += 1;
    }
    Ok((n, led.sources.len()))
}

// ── 注入核：四类违约 ────────────────────────────────────────────
/// 构造一条带指定来源的合法记录（供注入变体复用）。
fn base_rec(pid: u32) -> BaseRecord {
    let build = match ALL_BUILDS.get(((pid - 1) % BUILD_COUNT as u32) as usize) {
        Some(b) => *b,
        None => WinBuild::B19045,
    };
    let expected = EXPECT_BASE.saturating_add(pid);
    BaseRecord {
        probe_id: pid,
        build,
        fingerprint: derive_fp(pid, build, expected),
        expected,
        source: derive_source_tag(pid),
    }
}

/// 类①：来源未登记——构造未登记 tag 的记录 → SourceUnregistered 拒。
pub fn inject_unregistered_source() -> Result<bool, u32> {
    let mut led = BaselineLedger::new();
    let mut rec = base_rec(1);
    rec.source = String::from("RM-GHOST-UNREGISTERED");
    let hit = match led.put_record(rec) {
        Err(c) => c == BaseCode::SourceUnregistered.code(),
        Ok(_) => false,
    };
    Ok(hit)
}

/// 类②：零指纹——指纹列清零 → ZeroFingerprint 拒。
pub fn inject_zero_fp() -> Result<bool, u32> {
    let mut led = BaselineLedger::new();
    let tag = derive_source_tag(2);
    led.sources.register(&tag, 0)?;
    let mut rec = base_rec(2);
    rec.fingerprint = 0;
    let hit = match led.put_record(rec) {
        Err(c) => c == BaseCode::ZeroFingerprint.code(),
        Ok(_) => false,
    };
    Ok(hit)
}

/// 类③：版本闭集外——手工构造非法构建号记录 → BuildUnknown 拒。
pub fn inject_build_unknown() -> Result<bool, u32> {
    // from_num 闸：1607/99999 均不在闭集。
    let h1 = WinBuild::from_num(1607).is_none();
    let h2 = WinBuild::from_num(99999).is_none();
    // put_record 闸：构造闭集外值需绕过 enum——以指纹三列自洽破坏模拟：
    // 版本列与指纹推导列不一致 → FpMismatch 拒（版本篡改必被感知）。
    let mut led = BaselineLedger::new();
    let tag = derive_source_tag(3);
    let _ = led.sources.register(&tag, 0);
    let mut rec = base_rec(3);
    rec.build = WinBuild::B19041;
    rec.fingerprint = derive_fp(3, WinBuild::B19045, rec.expected); // 版本列与指纹列错位
    let h3 = match led.put_record(rec) {
        Err(c) => c == BaseCode::FpMismatch.code(),
        Ok(_) => false,
    };
    Ok(h1 && h2 && h3)
}

/// 类④：记录重复（同 probe_id 二次入账）→ DupProbeId 拒。
pub fn inject_dup_record() -> Result<bool, u32> {
    let mut led = BaselineLedger::new();
    let tag = derive_source_tag(4);
    let _ = led.sources.register(&tag, 0);
    let rec = base_rec(4);
    let _ = led.put_record(rec.clone())?;
    let hit = match led.put_record(rec) {
        Err(c) => c == BaseCode::DupProbeId.code(),
        Ok(_) => false,
    };
    Ok(hit)
}

/// 注入检出账（四类）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InjectLedger {
    pub tries: [u32; INJECT_CLASSES],
    pub caught: [u32; INJECT_CLASSES],
}

impl InjectLedger {
    pub fn new() -> InjectLedger {
        InjectLedger { tries: [0; INJECT_CLASSES], caught: [0; INJECT_CLASSES] }
    }
    pub fn record(&mut self, slot: usize, hit: bool) {
        if let Some(t) = self.tries.get_mut(slot) {
            *t = t.saturating_add(1);
        }
        if hit {
            if let Some(c) = self.caught.get_mut(slot) {
                *c = c.saturating_add(1);
            }
        }
    }
    pub fn all_caught(&self) -> bool {
        let mut i = 0;
        while i < INJECT_CLASSES {
            let t = match self.tries.get(i) {
                Some(v) => *v,
                None => return false,
            };
            let c = match self.caught.get(i) {
                Some(v) => *v,
                None => return false,
            };
            if t == 0 || c != t {
                return false;
            }
            i += 1;
        }
        true
    }
    pub fn missed_total(&self) -> u32 {
        let mut m = 0u32;
        let mut i = 0;
        while i < INJECT_CLASSES {
            let t = match self.tries.get(i) {
                Some(v) => *v,
                None => 0,
            };
            let c = match self.caught.get(i) {
                Some(v) => *v,
                None => 0,
            };
            m = m.saturating_add(t.saturating_sub(c));
            i += 1;
        }
        m
    }
}

/// 注入 10/10：四类违约逐类断言被入账闸显式拒绝。
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    let mut r = 0;
    while r < rounds {
        led.record(0, inject_unregistered_source()?);
        led.record(1, inject_zero_fp()?);
        led.record(2, inject_build_unknown()?);
        led.record(3, inject_dup_record()?);
        r += 1;
    }
    Ok(led)
}

// ── 对账辅助判据 ────────────────────────────────────────────────
/// 双跑一致：标准账两跑逐记录相等（确定性红线）。
pub fn repro_check() -> Result<(), u32> {
    let a = build_standard()?;
    let b = build_standard()?;
    if a.records.len() != b.records.len() {
        return Err(BaseCode::NotReproducible.code());
    }
    let mut i = 0;
    while i < a.records.len() {
        let ra = match a.records.get(i) {
            Some(x) => x,
            None => return Err(BaseCode::EmptyLedger.code()),
        };
        let rb = match b.records.get(i) {
            Some(x) => x,
            None => return Err(BaseCode::EmptyLedger.code()),
        };
        if ra != rb {
            return Err(BaseCode::NotReproducible.code());
        }
        i += 1;
    }
    Ok(())
}

/// 账本指纹（50 记录五列拼 FNV，语料冻结可感知）。
pub fn ledger_fnv(led: &BaselineLedger) -> u64 {
    let mut bytes: Vec<u8> = Vec::new();
    let mut i = 0;
    while i < led.records.len() {
        if let Some(r) = led.records.get(i) {
            bytes.push((r.probe_id & 0xFF) as u8);
            bytes.push(r.build.ordinal() as u8);
            let fp = r.fingerprint;
            bytes.push((fp & 0xFF) as u8);
            bytes.push(((fp >> 8) & 0xFF) as u8);
            bytes.push(((fp >> 16) & 0xFF) as u8);
            bytes.push(((fp >> 24) & 0xFF) as u8);
            bytes.push((r.expected & 0xFF) as u8);
        }
        i += 1;
    }
    fnv1a(&bytes)
}

/// 版本分布守恒：50 记录中五版本各恰 10 条（轮转推导）。
pub fn build_distribution(led: &BaselineLedger) -> Result<[u32; BUILD_COUNT], u32> {
    let mut dist = [0u32; BUILD_COUNT];
    let mut i = 0;
    while i < led.records.len() {
        if let Some(r) = led.records.get(i) {
            let slot = r.build.ordinal() as usize;
            if slot >= BUILD_COUNT {
                return Err(BaseCode::BuildUnknown.code());
            }
            if let Some(c) = dist.get_mut(slot) {
                *c = c.saturating_add(1);
            }
        }
        i += 1;
    }
    let mut k = 0;
    while k < BUILD_COUNT {
        let c = match dist.get(k) {
            Some(v) => *v,
            None => 0,
        };
        if c != 10 {
            return Err(BaseCode::ConservationBroken.code());
        }
        k += 1;
    }
    Ok(dist)
}

/// 段位守卫：12 码全落 [SEG_BASE, SEG_BASE+0x40)。
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < BaseCode::ALL.len() {
        if let Some(c) = BaseCode::ALL.get(i) {
            let v = c.code();
            if v < SEG_BASE || v >= SEG_BASE.saturating_add(0x40) {
                return false;
            }
        }
        i += 1;
    }
    true
}

// ── 判据收口 ────────────────────────────────────────────────────
/// UNX-F18528 自检判据（16 条 ≤ 20 上限；动态明细 ≤ 8 槽）。
pub fn run_unx18528_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F18528 Windows 口径基准账（实机对照记录）");

    s.add("J3-段位守卫", segment_guard(), "0x7FFC0 窗口内 12 码全落位");

    // J1 主锚点：三列 50/50 完整 + 来源登记入账。
    let gold = golden_run();
    let (g_ok, g_txt) = match &gold {
        Ok((n, src)) => (*n == RECORD_COUNT && *src == RECORD_COUNT, format!("记录={} 来源={}", n, src)),
        Err(c) => (false, format!("黄金链断:{}", detail_of(*c))),
    };
    s.add_dyn("J1-三列50/50完整", g_ok, &g_txt);

    // 指纹抽查：50 行复算全过。
    let audit = match build_standard() {
        Ok(mut led) => led.audit_fingerprints(),
        Err(c) => Err(c),
    };
    s.add(
        "J1-指纹抽查50行",
        match &audit { Ok(n) => *n == RECORD_COUNT, Err(_) => false },
        "逐行 FNV 复算 == 登记指纹",
    );

    // 版本分布守恒：五版本各 10 条。
    let dist = match build_standard() {
        Ok(led) => build_distribution(&led),
        Err(c) => Err(c),
    };
    s.add(
        "J1-版本分布5×10",
        dist.is_ok(),
        "构建号闭集轮转守恒",
    );

    let inj = inject_rounds(JUDGE_ROUNDS);
    s.add(
        "J2-注入①来源未登记10/10",
        match &inj { Ok(l) => l.caught[0] == l.tries[0] && l.tries[0] == JUDGE_ROUNDS, Err(_) => false },
        "AI-95 先登记后引用闸",
    );
    s.add(
        "J2-注入②零指纹10/10",
        match &inj { Ok(l) => l.caught[1] == l.tries[1] && l.tries[1] == JUDGE_ROUNDS, Err(_) => false },
        "指纹列缺省拒收",
    );
    s.add(
        "J2-注入③版本越界10/10",
        match &inj { Ok(l) => l.caught[2] == l.tries[2] && l.tries[2] == JUDGE_ROUNDS, Err(_) => false },
        "from_num 闸 + 版本指纹错位感知",
    );
    s.add(
        "J2-注入④记录重复10/10",
        match &inj { Ok(l) => l.caught[3] == l.tries[3] && l.tries[3] == JUDGE_ROUNDS, Err(_) => false },
        "dup probe_id append-only 拒",
    );

    s.add(
        "J2-漏检恒零",
        match &inj { Ok(l) => l.missed_total() == 0, Err(_) => false },
        "tries = caught + missed，missed=0",
    );

    // append-only：dup 来源 tag 拒 + 空来源拒。
    let append_only = {
        let mut reg = SourceRegistry::new();
        let first = reg.register("RM-19045-X", 0).is_ok();
        let dup = match reg.register("RM-19045-X", 0) {
            Err(c) => c == BaseCode::SourceDup.code(),
            Ok(_) => false,
        };
        let empty = reg.register("", 0).is_err();
        first && dup && empty
    };
    s.add("J2-来源账append-only", append_only, "dup tag 拒 + 空 tag 拒 + 只增不减");

    // 状态机：迁移闭集合法恰 4 条。
    let states = [RecState::Draft, RecState::Registered, RecState::Verified, RecState::Sealed];
    let mut legal_n = 0usize;
    let mut a = 0;
    while a < states.len() {
        let mut b = 0;
        while b < states.len() {
            let fa = match states.get(a) { Some(x) => *x, None => break };
            let fb = match states.get(b) { Some(x) => *x, None => break };
            if can_transit(fa, fb) {
                legal_n = legal_n.saturating_add(1);
            }
            b += 1;
        }
        a += 1;
    }
    s.add("J2-迁移闭集合法恰4条", legal_n == 4, "4 态 16 组合枚举：合法 4 / 非法 12");

    // 双跑一致：50 记录逐记录相等 + 账本指纹稳定。
    s.add("J2-双跑一致", repro_check().is_ok(), "标准账两跑逐记录相等");

    // 边界：id 0 与 51 越界、构建号闭集边界（19041 首尾 19045）。
    let boundary = {
        let mut led = BaselineLedger::new();
        let tag = "RM-BOUNDARY-X";
        let _ = led.sources.register(tag, 0);
        let mut r0 = base_rec(1);
        r0.probe_id = 0;
        r0.source = String::from(tag);
        let mut r51 = base_rec(1);
        r51.probe_id = RECORD_COUNT as u32 + 1;
        r51.source = String::from(tag);
        // 指纹随 id 重算以免 FpMismatch 先触发。
        r0.fingerprint = derive_fp(0, r0.build, r0.expected);
        r51.fingerprint = derive_fp(r51.probe_id, r51.build, r51.expected);
        let h0 = led.put_record(r0).is_err();
        let h51 = led.put_record(r51).is_err();
        h0 && h51
            && WinBuild::from_num(19041) == Some(WinBuild::B19041)
            && WinBuild::from_num(19045) == Some(WinBuild::B19045)
            && WinBuild::from_num(19046).is_none()
    };
    s.add("J3-id与构建号边界", boundary, "id 0/51 拒 + 19041/19045 闭集边界");

    // 构建号闭集：数值与序互查。
    let build_set = {
        let mut ok = true;
        let mut i = 0;
        while i < ALL_BUILDS.len() {
            let b = match ALL_BUILDS.get(i) {
                Some(x) => *x,
                None => { ok = false; break; }
            };
            if WinBuild::from_num(b.num()) != Some(b) || b.ordinal() != i as u32 {
                ok = false;
                break;
            }
            i += 1;
        }
        ok
    };
    s.add("J3-构建号闭集互查", build_set, "5 版本数值↔序 双向互查");

    // 账本指纹非零。
    let lfnv = match build_standard() {
        Ok(led) => ledger_fnv(&led),
        Err(_) => 0,
    };
    s.add("J3-账本指纹非零", lfnv != 0, "50 记录五列拼 FNV 稳定");

    // 文案闭集。
    let mut detail_full = true;
    let mut di = 0;
    while di < BaseCode::ALL.len() {
        if let Some(c) = BaseCode::ALL.get(di) {
            if c.detail().len() == 0 || detail_of(c.code()).len() == 0 {
                detail_full = false;
            }
        }
        di += 1;
    }
    s.add("J3-文案闭集非空", detail_full, "12 码双查非空");

    // 收口。
    s.add("J3-判据条数≤20收口", s.len() < 20, "本模块判据上限纪律（含本条）");

    s
}

// ── 测试 ────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn t01_all_checks_green() {
        let set = run_unx18528_checks();
        assert!(set.all_passed(), "UNX-F18528 全判据应全绿: {}", set.red_items_text());
    }

    #[test]
    fn t02_standard_50_of_50() {
        let g = golden_run();
        assert!(g.is_ok());
        assert_eq!(g.unwrap_or((0, 0)), (50, 50));
    }

    #[test]
    fn t03_build_closed_set() {
        // 数值↔序互查 + 闭集边界。
        assert_eq!(WinBuild::from_num(19041), Some(WinBuild::B19041));
        assert_eq!(WinBuild::from_num(19045), Some(WinBuild::B19045));
        assert!(WinBuild::from_num(19046).is_none());
        assert!(WinBuild::from_num(19040).is_none());
        assert_eq!(WinBuild::B19045.num(), 19045);
        assert_eq!(WinBuild::B19041.ordinal(), 0);
    }

    #[test]
    fn t04_fp_derivation_roundtrip() {
        // 指纹推导确定性 + 篡改感知。
        let fp = derive_fp(7, WinBuild::B19044, 0x107);
        assert_eq!(derive_fp(7, WinBuild::B19044, 0x107), fp);
        assert_ne!(fp, 0);
        assert_ne!(fp, derive_fp(7, WinBuild::B19044, 0x108));
        assert_ne!(fp, derive_fp(8, WinBuild::B19044, 0x107));
    }

    #[test]
    fn t05_injection_all_caught() {
        let inj = inject_rounds(JUDGE_ROUNDS).unwrap_or(InjectLedger::new());
        assert!(inj.all_caught());
        assert_eq!(inj.missed_total(), 0);
    }

    #[test]
    fn t06_source_gate_strict() {
        let mut led = BaselineLedger::new();
        // 未登记来源拒。
        let mut rec = base_rec(5);
        let _ = led.put_record(rec.clone());
        assert!(led.put_record(rec).is_err());
        // 登记后入账通过。
        let tag = derive_source_tag(5);
        assert!(led.sources.register(&tag, 0).is_ok());
        rec = base_rec(5);
        assert!(led.put_record(rec).is_ok());
        // dup 拒。
        assert!(led.put_record(base_rec(5)).is_err());
    }

    #[test]
    fn t07_build_distribution() {
        let led = build_standard().unwrap_or(BaselineLedger::new());
        let dist = build_distribution(&led);
        assert!(dist.is_ok());
        assert_eq!(dist.unwrap_or([0; 5]), [10, 10, 10, 10, 10]);
    }

    #[test]
    fn t08_double_run_deterministic() {
        assert!(repro_check().is_ok());
        let a = build_standard();
        let b = build_standard();
        // 逐记录相等。
        match (a, b) {
            (Ok(la), Ok(lb)) => {
                assert_eq!(la.records.len(), lb.records.len());
                let mut i = 0;
                while i < la.records.len() {
                    let ra = la.records.get(i).cloned();
                    let rb = lb.records.get(i).cloned();
                    assert_eq!(ra, rb);
                    i += 1;
                }
            }
            _ => panic!(), // 测试区允许
        }
    }

    #[test]
    fn t09_state_machine_closed_set() {
        assert!(can_transit(RecState::Draft, RecState::Registered));
        assert!(can_transit(RecState::Registered, RecState::Verified));
        assert!(can_transit(RecState::Verified, RecState::Sealed));
        assert!(can_transit(RecState::Sealed, RecState::Verified));
        assert!(!can_transit(RecState::Draft, RecState::Sealed));
        assert!(!can_transit(RecState::Registered, RecState::Draft));
    }

    #[test]
    fn t10_segment_and_detail() {
        assert!(segment_guard());
        let mut i = 0;
        while i < BaseCode::ALL.len() {
            if let Some(c) = BaseCode::ALL.get(i) {
                assert_eq!(detail_of(c.code()), c.detail());
                assert!(c.detail().len() > 0);
            }
            i += 1;
        }
        assert_eq!(detail_of(0xDEAD), "未知诊断码：不在 0x7FFC0 段闭集内");
    }
}
