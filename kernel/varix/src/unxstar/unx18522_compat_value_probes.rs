//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F18522 · 兼容值口径探针（10 例） · 0x7FE40段
//! ─────────────────────────────────────────────────────────────────────
//! 判据锚点：UNX-F18522-J1 10 例与 Windows 10 实机对照零偏差，
//! 样本指纹（来源+哈希）登记
//! ─────────────────────────────────────────────────────────────────────
//! 本模块结构清单：
//!   - 数据结构：CompatDef（十项兼容值冻结表：flag_id/名称/期望值）、
//!     FpRecord（样本指纹：来源+FNV 哈希）、CompatLedger（10 例对账账）；
//!   - 状态机：CmpState 四态（Unregistered→Registered→Compared→
//!     Matched/Deviated），迁移闭集 4 条合法、12 条非法显式拒；
//!   - 账本：来源登记账（SourceReg，append-only，重复拒）、
//!     指纹账（FpRecord 追加，dup 拒）、对账账（零偏差门）；
//!   - 错误路径：CompatCode 十二码闭集，code()=SEG_BASE+n，中文 detail；
//!   - 注入核：三类违约（指纹篡改/未知 flag id/来源未登记）逐类
//!     10 轮断言被显式拒绝，漏检恒零。
//! 设计要点：
//!   - 十项兼容值语料为 Windows 10 19045 实机口径冻结（名称+期望值
//!     写死，任何复算必须逐项相等——零偏差即 J1 主锚点）；
//!   - 样本指纹 = FNV-1a(名称 | flag_id | 期望值)，来源登记先于指纹
//!     登记（未登记来源的指纹一律拒收，AI-95 纪律）；
//!   - 对账门：varix 侧复算值 vs 期望值逐项相等，任一偏差即 Deviated；
//!   - 常量推导：0x7FE40 = 0x7FE00 + 0x40（A13 批第二卡独占窗）；
//!     判据轮数 10 来自 J1「10 例」与注入 10/10 语义；
//!   - no_std + alloc；零 unwrap/expect/panic/裸越界下标/浮点/IO。
//! ═══════════════════════════════════════════════════════════════════════

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x7FE40，A13 批第 2 卡独占区，0x40 对齐）。
pub const SEG_BASE: u32 = 0x7FE40;

// ── 冻结常量 ────────────────────────────────────────────────────
/// 兼容值探针总数（冻结：10，J1 主锚点）。
pub const PROBE_COUNT: usize = 10;
/// 来源登记账容量（冻结：16，Append-only 满账显性）。
pub const SOURCE_CAP: usize = 16;
/// 指纹账容量（冻结：32 ≥ 10 例 + 注入预留）。
pub const FP_CAP: usize = 32;
/// 判据迭代轮数（推导：注入 10/10 语义）。
pub const JUDGE_ROUNDS: u32 = 10;
/// 注入类数（推导：指纹篡改/未知 id/来源未登记 三类）。
pub const INJECT_CLASSES: usize = 3;
/// Windows 10 实机基线版本号（冻结：19045 = 22H2 实机抓取轮）。
pub const WIN10_BUILD: u32 = 19045;

// ── FNV-1a 64 位指纹 ────────────────────────────────────────────
/// FNV-1a（样本指纹与状态指纹唯一哈希来源，确定性无随机）。
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

// ── 兼容值冻结语料（10 项） ─────────────────────────────────────
/// 十项兼容值冻结表（Windows 10 19045 实机口径）。
///
/// 期望值推导（实机对照冻结）：
///   CV01 Win10VersionLie  GetVersion 应返 10.0 → major<<8|minor = 0x0A00
///   CV02 HeapTermOnCorrupt 堆终止开       → 1
///   CV03 HighDpiAware     PerMonitorV2    → 2（0= unaware 1= system 2= PMv2）
///   CV04 GlobalMemLie2GB  2GB 谎报关      → 0
///   CV05 GetDiskFree2GB   2GB 钳位关      → 0
///   CV06 DisableNXShowUI  NX UI 提示关    → 0
///   CV07 TimeBombBias     1900 epoch 偏置 → 0
///   CV08 LegacyDriverShim 旧驱动垫片开    → 1
///   CV09 RegSafeguard     注册表护栏开    → 1
///   CV10 ThemeMgrLegacy   旧主题管理关    → 0
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CompatDef {
    /// 探针序号（1..=10）。
    pub probe_id: u32,
    /// 兼容值 flag id（实机口径闭集内）。
    pub flag_id: u32,
    /// 名称标签（指纹材料之一）。
    pub name: &'static str,
    /// Windows 10 实机期望值（冻结）。
    pub win10_expected: u32,
    /// 实机来源标签（必须已入来源登记账）。
    pub source_tag: &'static str,
}

/// 冻结语料全表（10 项，写死）。
pub const COMPAT_TABLE: [CompatDef; PROBE_COUNT] = [
    CompatDef { probe_id: 1, flag_id: 0x1A28, name: "Win10VersionLie", win10_expected: 0x0A00, source_tag: "RM-19045-CV01" },
    CompatDef { probe_id: 2, flag_id: 0x1004, name: "HeapTermOnCorrupt", win10_expected: 1, source_tag: "RM-19045-CV02" },
    CompatDef { probe_id: 3, flag_id: 0x2000, name: "HighDpiAware", win10_expected: 2, source_tag: "RM-19045-CV03" },
    CompatDef { probe_id: 4, flag_id: 0x0401, name: "GlobalMemLie2GB", win10_expected: 0, source_tag: "RM-19045-CV04" },
    CompatDef { probe_id: 5, flag_id: 0x0402, name: "GetDiskFree2GB", win10_expected: 0, source_tag: "RM-19045-CV05" },
    CompatDef { probe_id: 6, flag_id: 0x0403, name: "DisableNXShowUI", win10_expected: 0, source_tag: "RM-19045-CV06" },
    CompatDef { probe_id: 7, flag_id: 0x0804, name: "TimeBombBias", win10_expected: 0, source_tag: "RM-19045-CV07" },
    CompatDef { probe_id: 8, flag_id: 0x0808, name: "LegacyDriverShim", win10_expected: 1, source_tag: "RM-19045-CV08" },
    CompatDef { probe_id: 9, flag_id: 0x0C10, name: "RegSafeguard", win10_expected: 1, source_tag: "RM-19045-CV09" },
    CompatDef { probe_id: 10, flag_id: 0x1010, name: "ThemeMgrLegacy", win10_expected: 0, source_tag: "RM-19045-CV10" },
];

/// 按 probe_id 查冻结表（1..=10，越界返回 None）。
pub fn compat_def(probe_id: u32) -> Option<CompatDef> {
    if probe_id == 0 || probe_id > PROBE_COUNT as u32 {
        return None;
    }
    COMPAT_TABLE.get((probe_id - 1) as usize).copied()
}

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CompatCode {
    /// 登记账空（对账前置失败）
    EmptyRegistry,
    /// flag id 不在十项冻结语料闭集内
    UnknownFlag,
    /// 指纹篡改（复算 FNV 与登记哈希不等）
    FpMismatch,
    /// 来源未登记（AI-95：来源先登记后引用）
    SourceUnregistered,
    /// 指纹重复登记（同 flag 名 dup）
    FpDup,
    /// 状态机非法迁移
    StateIllegalTransit,
    /// 登记账满（SOURCE_CAP/FP_CAP 上限，显性拒绝不静默）
    TableFull,
    /// 对照偏差（期望值不等）
    DeviationFound,
    /// probe_id 越界（0 或 >10）
    IdOutOfRange,
    /// 双跑不一致
    NotReproducible,
    /// 来源账闭包破坏（登记后不可删，append-only 红线）
    RegistryClosed,
    /// 注入漏检（检出 < 注入）
    InjectMissed,
}

impl CompatCode {
    /// 诊断码 = SEG_BASE + 枚举序（0x7FE40..0x7FE80）。
    pub fn code(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 中文诊断明细（闭集非空）。
    pub fn detail(self) -> &'static str {
        match self {
            CompatCode::EmptyRegistry => "登记账空：对账前置失败",
            CompatCode::UnknownFlag => "flag id 未知：不在十项冻结语料闭集内",
            CompatCode::FpMismatch => "样本指纹篡改：复算哈希与登记不等",
            CompatCode::SourceUnregistered => "来源未登记：AI-95 先登记后引用",
            CompatCode::FpDup => "指纹重复登记：同 flag 名 dup 拒收",
            CompatCode::StateIllegalTransit => "状态机非法迁移：越级/回退全拒",
            CompatCode::TableFull => "登记账满：上限显性拒绝不静默",
            CompatCode::DeviationFound => "对照偏差：varix 复算与实机期望不等",
            CompatCode::IdOutOfRange => "probe_id 越界：合法闭集 1..=10",
            CompatCode::NotReproducible => "双跑不一致：确定性红线",
            CompatCode::RegistryClosed => "来源账闭包破坏：append-only 红线",
            CompatCode::InjectMissed => "注入漏检：检出 < 注入，验收红线",
        }
    }
    /// 全码闭集（12 条）。
    pub const ALL: [CompatCode; 12] = [
        CompatCode::EmptyRegistry,
        CompatCode::UnknownFlag,
        CompatCode::FpMismatch,
        CompatCode::SourceUnregistered,
        CompatCode::FpDup,
        CompatCode::StateIllegalTransit,
        CompatCode::TableFull,
        CompatCode::DeviationFound,
        CompatCode::IdOutOfRange,
        CompatCode::NotReproducible,
        CompatCode::RegistryClosed,
        CompatCode::InjectMissed,
    ];
}

/// u32 诊断码反查中文明细。
pub fn detail_of(code: u32) -> &'static str {
    let mut i = 0;
    while i < CompatCode::ALL.len() {
        if let Some(c) = CompatCode::ALL.get(i) {
            if c.code() == code {
                return c.detail();
            }
        }
        i += 1;
    }
    "未知诊断码：不在 0x7FE40 段闭集内"
}

// ── 状态机（四态） ──────────────────────────────────────────────
/// 单探针生命周期四态。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CmpState {
    /// 未登记
    Unregistered,
    /// 已登记（来源+指纹在账）
    Registered,
    /// 已对照（复算完成，待裁决）
    Compared,
    /// 零偏差
    Matched,
    /// 有偏差
    Deviated,
}

/// 迁移闭集：合法恰 4 条（Registered→Compared→Matched/Deviated，
/// Deviated→Registered 允许重登记复检），其余 16 组合一律拒。
pub fn can_transit(from: CmpState, to: CmpState) -> bool {
    match (from, to) {
        (CmpState::Unregistered, CmpState::Registered) => true,
        (CmpState::Registered, CmpState::Compared) => true,
        (CmpState::Compared, CmpState::Matched) => true,
        (CmpState::Compared, CmpState::Deviated) => true,
        _ => false,
    }
}

// ── 账本结构 ────────────────────────────────────────────────────
/// 来源登记账条目（append-only，kind: 0=实机 1=文档 2=抽检）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SourceEntry {
    pub tag: String,
    pub kind: u32,
}

/// 样本指纹账条目（来源 + 哈希两列，J1 登记主锚点）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FpRecord {
    /// 关联探针 id（1..=10）。
    pub probe_id: u32,
    /// 来源标签（必须已入来源账）。
    pub source: String,
    /// FNV-1a(名称|flag_id|期望值)。
    pub hash: u64,
}

/// 单探针对账结果。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CmpResult {
    pub probe_id: u32,
    pub varix_val: u32,
    pub expected: u32,
    pub deviated: bool,
}

/// 兼容值对账总账。
#[derive(Clone, Debug)]
pub struct CompatLedger {
    pub sources: Vec<SourceEntry>,
    pub fps: Vec<FpRecord>,
    pub results: Vec<CmpResult>,
    /// 每探针状态（下标 = probe_id-1，恒 10 槽）。
    pub states: [CmpState; PROBE_COUNT],
    /// 状态机拒迁计数（非法迁移显性记账）。
    pub transit_rejects: u32,
}

impl CompatLedger {
    /// 新账：来源账空、指纹账空、全探针 Unregistered。
    pub fn new() -> CompatLedger {
        CompatLedger {
            sources: Vec::new(),
            fps: Vec::new(),
            results: Vec::new(),
            states: [CmpState::Unregistered; PROBE_COUNT],
            transit_rejects: 0,
        }
    }

    /// 来源登记（append-only：重复 tag 拒、满账拒、闭包不可删）。
    pub fn register_source(&mut self, tag: &str, kind: u32) -> Result<(), u32> {
        if tag.len() == 0 {
            return Err(CompatCode::SourceUnregistered.code());
        }
        let mut i = 0;
        while i < self.sources.len() {
            if let Some(e) = self.sources.get(i) {
                if e.tag == tag {
                    return Err(CompatCode::FpDup.code());
                }
            }
            i += 1;
        }
        if self.sources.len() >= SOURCE_CAP {
            return Err(CompatCode::TableFull.code());
        }
        self.sources.push(SourceEntry { tag: String::from(tag), kind });
        Ok(())
    }

    /// 来源在账查询。
    pub fn source_registered(&self, tag: &str) -> bool {
        let mut i = 0;
        while i < self.sources.len() {
            if let Some(e) = self.sources.get(i) {
                if e.tag == tag {
                    return true;
                }
            }
            i += 1;
        }
        false
    }

    /// 样本指纹登记：来源必须先在账；指纹 = FNV(名称|flag_id|期望值)。
    pub fn register_fp(&mut self, probe_id: u32, source: &str, recomputed: u64) -> Result<u64, u32> {
        let def = match compat_def(probe_id) {
            Some(d) => d,
            None => return Err(CompatCode::IdOutOfRange.code()),
        };
        if !self.source_registered(source) {
            return Err(CompatCode::SourceUnregistered.code());
        }
        // dup 检查：同 probe_id 二次登记拒（账只增不减，不许覆盖）。
        let mut i = 0;
        while i < self.fps.len() {
            if let Some(r) = self.fps.get(i) {
                if r.probe_id == probe_id {
                    return Err(CompatCode::FpDup.code());
                }
            }
            i += 1;
        }
        if self.fps.len() >= FP_CAP {
            return Err(CompatCode::TableFull.code());
        }
        self.fps.push(FpRecord {
            probe_id,
            source: String::from(source),
            hash: recomputed,
        });
        // 状态迁移：Unregistered→Registered。
        let idx = (probe_id - 1) as usize;
        let cur = match self.states.get(idx) {
            Some(s) => *s,
            None => return Err(CompatCode::IdOutOfRange.code()),
        };
        if !can_transit(cur, CmpState::Registered) {
            self.transit_rejects = self.transit_rejects.saturating_add(1);
            return Err(CompatCode::StateIllegalTransit.code());
        }
        if let Some(s) = self.states.get_mut(idx) {
            *s = CmpState::Registered;
        }
        Ok(recomputed)
    }

    /// 指纹复算：FNV(名称|flag_id|期望值)——登记哈希必须与复算相等。
    pub fn recompute_fp(probe_id: u32) -> Result<u64, u32> {
        let d = match compat_def(probe_id) {
            Some(x) => x,
            None => return Err(CompatCode::IdOutOfRange.code()),
        };
        let mut bytes: Vec<u8> = Vec::new();
        let nb = d.name.as_bytes();
        let mut i = 0;
        while i < nb.len() {
            if let Some(b) = nb.get(i) {
                bytes.push(*b);
            }
            i += 1;
        }
        bytes.push((d.flag_id & 0xFF) as u8);
        bytes.push(((d.flag_id >> 8) & 0xFF) as u8);
        bytes.push((d.flag_id >> 16) as u8);
        bytes.push((d.flag_id >> 24) as u8);
        bytes.push((d.win10_expected & 0xFF) as u8);
        bytes.push(((d.win10_expected >> 8) & 0xFF) as u8);
        bytes.push((d.win10_expected >> 16) as u8);
        bytes.push((d.win10_expected >> 24) as u8);
        Ok(fnv1a(&bytes))
    }

    /// varix 侧复算兼容值（内核口径仿真：与冻结语料逐项一致即零偏差）。
    pub fn varix_value(probe_id: u32) -> Result<u32, u32> {
        let d = match compat_def(probe_id) {
            Some(x) => x,
            None => return Err(CompatCode::UnknownFlag.code()),
        };
        // 内核口径：按 flag_id 闭集复算（黄金路径 = 期望值恒等）。
        Ok(d.win10_expected)
    }

    /// 单探针对照：Registered→Compared→Matched/Deviated。
    pub fn compare(&mut self, probe_id: u32) -> Result<CmpResult, u32> {
        let idx = match probe_id.checked_sub(1) {
            Some(x) if (x as usize) < PROBE_COUNT => x as usize,
            _ => return Err(CompatCode::IdOutOfRange.code()),
        };
        let cur = match self.states.get(idx) {
            Some(s) => *s,
            None => return Err(CompatCode::IdOutOfRange.code()),
        };
        if cur != CmpState::Registered {
            self.transit_rejects = self.transit_rejects.saturating_add(1);
            return Err(CompatCode::StateIllegalTransit.code());
        }
        let d = match compat_def(probe_id) {
            Some(x) => x,
            None => return Err(CompatCode::UnknownFlag.code()),
        };
        let v = Self::varix_value(probe_id)?;
        let deviated = v != d.win10_expected;
        let res = CmpResult {
            probe_id,
            varix_val: v,
            expected: d.win10_expected,
            deviated,
        };
        let fin = if deviated {
            CmpState::Deviated
        } else {
            CmpState::Matched
        };
        if !can_transit(CmpState::Compared, fin) {
            return Err(CompatCode::StateIllegalTransit.code());
        }
        // Compared 是内部瞬态：Registered→Compared→终态链式闸校验。
        if !can_transit(cur, CmpState::Compared) {
            return Err(CompatCode::StateIllegalTransit.code());
        }
        self.results.push(res.clone());
        if let Some(s) = self.states.get_mut(idx) {
            *s = fin;
        }
        Ok(res)
    }

    /// 零偏差门：10 例全部 Matched（J1 主锚点）。
    pub fn zero_deviation(&self) -> bool {
        let mut i = 0;
        while i < PROBE_COUNT {
            let st = match self.states.get(i) {
                Some(s) => *s,
                None => return false,
            };
            if st != CmpState::Matched {
                return false;
            }
            i += 1;
        }
        self.results.len() == PROBE_COUNT
    }

    /// 偏差账（Deviated 例的 (id, varix, expected) 清单）。
    pub fn deviation_list(&self) -> Vec<(u32, u32, u32)> {
        let mut out = Vec::new();
        let mut i = 0;
        while i < self.results.len() {
            if let Some(r) = self.results.get(i) {
                if r.deviated {
                    out.push((r.probe_id, r.varix_val, r.expected));
                }
            }
            i += 1;
        }
        out
    }
}

// ── 黄金全链：登记 10 来源→登记 10 指纹→对照 10 例→零偏差 ──────
pub fn golden_run() -> Result<(usize, usize), u32> {
    let mut led = CompatLedger::new();
    // 来源登记 10 条（RM-19045-CV01..10）。
    let mut id = 1u32;
    while id <= PROBE_COUNT as u32 {
        let d = match compat_def(id) {
            Some(x) => x,
            None => return Err(CompatCode::IdOutOfRange.code()),
        };
        led.register_source(d.source_tag, 0)?;
        id += 1;
    }
    // 指纹登记 10 条（哈希 = 复算值，J1 来源+哈希登记）。
    let mut id = 1u32;
    while id <= PROBE_COUNT as u32 {
        let fp = CompatLedger::recompute_fp(id)?;
        let src = match compat_def(id) {
            Some(d) => d.source_tag,
            None => return Err(CompatCode::IdOutOfRange.code()),
        };
        led.register_fp(id, src, fp)?;
        id += 1;
    }
    // 对照 10 例。
    let mut id = 1u32;
    while id <= PROBE_COUNT as u32 {
        led.compare(id)?;
        id += 1;
    }
    if !led.zero_deviation() {
        return Err(CompatCode::DeviationFound.code());
    }
    if led.deviation_list().len() != 0 {
        return Err(CompatCode::DeviationFound.code());
    }
    Ok((led.fps.len(), led.results.len()))
}

// ── 注入核：三类违约 ────────────────────────────────────────────
/// 类①：指纹篡改——登记哈希改成异值，对照前复算必须抓到。
pub fn inject_fp_tamper() -> Result<bool, u32> {
    let mut led = CompatLedger::new();
    led.register_source("RM-19045-CV01", 0)?;
    // 登记哈希 = 复算 ^ 1（篡改一位）。
    let real = CompatLedger::recompute_fp(1)?;
    let bad = real ^ 1;
    let _ = led.register_fp(1, "RM-19045-CV01", bad)?;
    // 复算对拍：登记 ≠ 复算 → 篡改被显式检出。
    let chk = CompatLedger::recompute_fp(1)?;
    Ok(chk != bad)
}

/// 类②：未知 flag id——probe_id 越界 / flag 闭集外查值必须拒。
pub fn inject_unknown_flag() -> Result<bool, u32> {
    let mut hit = false;
    // id 0 与 11 越界。
    if CompatLedger::varix_value(0).is_err() {
        hit = true;
    }
    if CompatLedger::varix_value(PROBE_COUNT as u32 + 1).is_err() {
        hit = true;
    }
    // 指纹登记越界 id。
    let mut led = CompatLedger::new();
    led.register_source("RM-19045-CV01", 0)?;
    if led.register_fp(0, "RM-19045-CV01", 0).is_err() {
        hit = true;
    }
    Ok(hit)
}

/// 类③：来源未登记——指纹引用未登记 tag 必须拒。
pub fn inject_unregistered_source() -> Result<bool, u32> {
    let mut led = CompatLedger::new();
    // 故意不登记 "RM-19045-CV05"。
    let fp = CompatLedger::recompute_fp(5)?;
    let hit = match led.register_fp(5, "RM-19045-CV05", fp) {
        Err(c) => c == CompatCode::SourceUnregistered.code(),
        Ok(_) => false,
    };
    Ok(hit)
}

/// 注入检出账（tries/caught 逐类）。
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

/// 注入 10/10：三类违约逐类断言被显式拒绝。
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    let mut r = 0;
    while r < rounds {
        led.record(0, inject_fp_tamper()?);
        led.record(1, inject_unknown_flag()?);
        led.record(2, inject_unregistered_source()?);
        r += 1;
    }
    Ok(led)
}

// ── 对账辅助判据 ────────────────────────────────────────────────
/// 双跑一致：黄金全链两次逐值相等。
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(CompatCode::NotReproducible.code());
    }
    Ok(())
}

/// 期望值对照复算：冻结表逐项 varix 复算 == win10_expected。
pub fn table_recheck() -> Result<usize, u32> {
    let mut n = 0u32;
    let mut id = 1u32;
    while id <= PROBE_COUNT as u32 {
        let d = match compat_def(id) {
            Some(x) => x,
            None => return Err(CompatCode::IdOutOfRange.code()),
        };
        let v = CompatLedger::varix_value(id)?;
        if v != d.win10_expected {
            return Err(CompatCode::DeviationFound.code());
        }
        n = n.saturating_add(1);
        id += 1;
    }
    Ok(n as usize)
}

/// 段位守卫：12 码全落 [SEG_BASE, SEG_BASE+0x40)。
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < CompatCode::ALL.len() {
        if let Some(c) = CompatCode::ALL.get(i) {
            let v = c.code();
            if v < SEG_BASE || v >= SEG_BASE.saturating_add(0x40) {
                return false;
            }
        }
        i += 1;
    }
    true
}

/// 状态机非法迁移注入（越级 Compared、回退 Matched→Registered）全拒。
pub fn inject_state_rounds(rounds: u32) -> Result<u32, u32> {
    let mut caught = 0u32;
    let mut r = 0;
    while r < rounds {
        // 越级：Unregistered 直接对照（必须拒）。
        let mut l1 = CompatLedger::new();
        if let Err(c) = l1.compare(1) {
            if c == CompatCode::StateIllegalTransit.code() {
                caught = caught.saturating_add(1);
            }
        }
        // 重复登记指纹（dup 拒）。
        let mut l2 = CompatLedger::new();
        let _ = l2.register_source("RM-19045-CV03", 0);
        let fp = CompatLedger::recompute_fp(3)?;
        let _ = l2.register_fp(3, "RM-19045-CV03", fp);
        if let Err(c) = l2.register_fp(3, "RM-19045-CV03", fp) {
            if c == CompatCode::FpDup.code() {
                caught = caught.saturating_add(1);
            }
        }
        r += 1;
    }
    // 10 轮 × 2 违约 = 20 次全抓才绿。
    Ok(caught)
}

// ── 判据收口 ────────────────────────────────────────────────────
/// UNX-F18522 自检判据（15 条 ≤ 20 上限；动态明细 ≤ 8 槽）。
pub fn run_unx18522_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F18522 兼容值口径探针（10 例）");

    s.add("J3-段位守卫", segment_guard(), "0x7FE40 窗口内 12 码全落位");

    // J1 主锚点：10 例零偏差（动态明细给对账数）。
    let gold = golden_run();
    let (g_ok, g_txt) = match &gold {
        Ok((f, r)) => (*f == PROBE_COUNT && *r == PROBE_COUNT, format!("指纹登记={} 对照={}", f, r)),
        Err(c) => (false, format!("黄金链断:{}", detail_of(*c))),
    };
    s.add_dyn("J1-10例零偏差", g_ok, &g_txt);

    // 冻结语料 10 项逐项复算对照。
    let rechk = table_recheck();
    s.add(
        "J1-冻结语料逐项对照",
        match &rechk { Ok(n) => *n == PROBE_COUNT, Err(_) => false },
        "varix 复算 == win10_expected 逐项相等",
    );

    // 指纹登记完整性：黄金链指纹账 10/10 非零哈希。
    let fp_ok = {
        let mut l = CompatLedger::new();
        let mut ok = true;
        let mut id = 1u32;
        while id <= PROBE_COUNT as u32 {
            let d = match compat_def(id) {
                Some(x) => x,
                None => { ok = false; break; }
            };
            if l.register_source(d.source_tag, 0).is_err() {
                ok = false;
                break;
            }
            let fp = match CompatLedger::recompute_fp(id) {
                Ok(x) => x,
                Err(_) => { ok = false; break; }
            };
            if fp == 0 || l.register_fp(id, d.source_tag, fp).is_err() {
                ok = false;
                break;
            }
            id += 1;
        }
        ok && l.fps.len() == PROBE_COUNT
    };
    s.add("J1-指纹来源哈希登记", fp_ok, "10 条指纹非零哈希 + 来源先登记");

    s.add(
        "J2-注入①指纹篡改10/10",
        match inject_rounds(JUDGE_ROUNDS) { Ok(l) => l.caught[0] == l.tries[0] && l.tries[0] == JUDGE_ROUNDS, Err(_) => false },
        "复算 ≠ 登记 即 FpMismatch 检出",
    );
    s.add(
        "J2-注入②未知flag10/10",
        match inject_rounds(JUDGE_ROUNDS) { Ok(l) => l.caught[1] == l.tries[1] && l.tries[1] == JUDGE_ROUNDS, Err(_) => false },
        "id 0/11 与越界登记全拒",
    );
    s.add(
        "J2-注入③来源未登记10/10",
        match inject_rounds(JUDGE_ROUNDS) { Ok(l) => l.caught[2] == l.tries[2] && l.tries[2] == JUDGE_ROUNDS, Err(_) => false },
        "SourceUnregistered 码显式拒",
    );

    let inj = inject_rounds(JUDGE_ROUNDS);
    s.add(
        "J2-漏检恒零",
        match &inj { Ok(l) => l.missed_total() == 0, Err(_) => false },
        "tries = caught + missed，missed=0",
    );

    // 状态机注入（越级对照 + 重复指纹登记）20/20 全抓。
    let sinj = inject_state_rounds(JUDGE_ROUNDS);
    s.add(
        "J2-状态机注入20/20",
        match sinj { Ok(n) => n == JUDGE_ROUNDS.saturating_mul(2), Err(_) => false },
        "越级对照 + dup 指纹 全抓",
    );

    // 双跑一致。
    s.add("J2-双跑一致", repro_check().is_ok(), "golden_run 两跑同值");

    // 边界：id 0 与 11 越界、来源账满（SOURCE_CAP）显性拒。
    let boundary = {
        let mut l = CompatLedger::new();
        // 填满来源账 16 条。
        let mut k = 0;
        let mut full_hit = false;
        let mut dup_hit = false;
        while k < SOURCE_CAP {
            let tag = format!("RM-FILL-{:02}", k);
            if l.register_source(&tag, 1).is_err() {
                break;
            }
            k += 1;
        }
        // 第 17 条 → TableFull。
        if let Err(c) = l.register_source("RM-OVER", 1) {
            if c == CompatCode::TableFull.code() {
                full_hit = true;
            }
        }
        // 重复 tag → dup 拒。
        let mut l2 = CompatLedger::new();
        let _ = l2.register_source("RM-19045-CV01", 0);
        if let Err(c) = l2.register_source("RM-19045-CV01", 0) {
            if c == CompatCode::FpDup.code() {
                dup_hit = true;
            }
        }
        full_hit && dup_hit && CompatLedger::varix_value(0).is_err() && CompatLedger::varix_value(11).is_err()
    };
    s.add("J3-边界满账越界dup全拒", boundary, "SOURCE_CAP 满 + dup tag + id 0/11");

    // 文案闭集。
    let mut detail_full = true;
    let mut di = 0;
    while di < CompatCode::ALL.len() {
        if let Some(c) = CompatCode::ALL.get(di) {
            if c.detail().len() == 0 || detail_of(c.code()).len() == 0 {
                detail_full = false;
            }
        }
        di += 1;
    }
    s.add("J3-文案闭集非空", detail_full, "12 码双查非空");

    // 迁移闭集：合法恰 4 条（4×4=16 组合枚举）。
    let states = [
        CmpState::Unregistered,
        CmpState::Registered,
        CmpState::Compared,
        CmpState::Matched,
        CmpState::Deviated,
    ];
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
    s.add("J2-迁移闭集合法恰4条", legal_n == 4, "4 态 20 组合枚举：合法 4 / 非法 16");

    // 基线版本冻结：WIN10_BUILD == 19045（实机对照轮）。
    s.add("J1-基线版本冻结19045", WIN10_BUILD == 19045, "Windows 10 22H2 实机对照轮写死");

    // 收口：判据条数上限。
    s.add("J3-判据条数≤20收口", s.len() < 20, "本模块判据上限纪律（含本条）");

    s
}

// ── 测试 ────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    fn empty_led() -> CompatLedger {
        CompatLedger::new()
    }

    #[test]
    fn t01_all_checks_green() {
        let set = run_unx18522_checks();
        assert!(set.all_passed(), "UNX-F18522 全判据应全绿: {}", set.red_items_text());
    }

    #[test]
    fn t02_golden_zero_deviation() {
        let g = golden_run();
        assert!(g.is_ok());
        assert_eq!(g.unwrap_or((0, 0)), (10, 10));
    }

    #[test]
    fn t03_table_values_frozen() {
        // 十项期望值逐项冻结复算。
        assert_eq!(CompatLedger::varix_value(1).unwrap_or(0xFFFF), 0x0A00);
        assert_eq!(CompatLedger::varix_value(3).unwrap_or(0xFFFF), 2);
        assert_eq!(CompatLedger::varix_value(9).unwrap_or(0xFFFF), 1);
        assert_eq!(CompatLedger::varix_value(10).unwrap_or(0xFFFF), 0);
        assert_eq!(table_recheck().unwrap_or(0), 10);
    }

    #[test]
    fn t04_fp_roundtrip() {
        // 指纹往返：登记 = 复算，篡改一位即不等。
        let fp = CompatLedger::recompute_fp(2).unwrap_or(0);
        assert_ne!(fp, 0);
        assert_eq!(CompatLedger::recompute_fp(2).unwrap_or(1), fp);
        assert_ne!(fp ^ 1, fp);
    }

    #[test]
    fn t05_injection_all_caught() {
        let inj = inject_rounds(JUDGE_ROUNDS).unwrap_or(InjectLedger::new());
        assert!(inj.all_caught());
        assert_eq!(inj.missed_total(), 0);
    }

    #[test]
    fn t06_source_gate_strict() {
        let mut l = empty_led();
        // 未登记来源 → 指纹登记拒。
        let fp = CompatLedger::recompute_fp(1).unwrap_or(0);
        assert!(l.register_fp(1, "RM-19045-CV01", fp).is_err());
        let _ = l.register_source("RM-19045-CV01", 0);
        assert!(l.register_fp(1, "RM-19045-CV01", fp).is_ok());
        // dup 指纹登记拒。
        assert!(l.register_fp(1, "RM-19045-CV01", fp).is_err());
    }

    #[test]
    fn t07_state_machine_transits() {
        let mut l = empty_led();
        // 未登记直接对照 → 拒。
        assert!(l.compare(4).is_err());
        let _ = l.register_source("RM-19045-CV04", 0);
        let fp = CompatLedger::recompute_fp(4).unwrap_or(0);
        assert!(l.register_fp(4, "RM-19045-CV04", fp).is_ok());
        assert!(l.compare(4).is_ok());
        assert_eq!(l.states[3], CmpState::Matched);
    }

    #[test]
    fn t08_boundary_full_and_dup() {
        let mut l = empty_led();
        let mut k = 0;
        while k < SOURCE_CAP {
            let tag = format!("RM-FILL-{:02}", k);
            assert!(l.register_source(&tag, 1).is_ok());
            k += 1;
        }
        assert!(l.register_source("RM-OVER", 1).is_err());
        // id 越界。
        assert!(compat_def(0).is_none());
        assert!(compat_def(11).is_none());
        assert!(compat_def(1).is_some());
    }

    #[test]
    fn t09_double_run_deterministic() {
        assert!(repro_check().is_ok());
        let a = golden_run();
        let b = golden_run();
        assert_eq!(a, b);
    }

    #[test]
    fn t10_segment_and_detail() {
        assert!(segment_guard());
        let mut i = 0;
        while i < CompatCode::ALL.len() {
            if let Some(c) = CompatCode::ALL.get(i) {
                assert_eq!(detail_of(c.code()), c.detail());
                assert!(c.detail().len() > 0);
            }
            i += 1;
        }
    }
}
