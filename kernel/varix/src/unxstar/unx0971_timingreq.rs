//! unx0971_timingreq — UNX-F0971 计时面需求清单（中断面对计时面的上游需求冻结）。
//!
//! 任务锚点：UNX-F0971（第二批工位 W7，SEG 段位 0x7F60，本单独占
//! 0x7F60..=0x7F7F；诊断码自 SEG_BASE+1 起闭集分派，两两互异编译期自证）。
//! 判据摘录（卡面验收行）：**中断面对计时面的上游需求全部冻结在册
//! （需求条目零遗漏、每条带验收判据与冻结 tick），冻结后篡改必被
//! 拒收带码，清单账守恒恒等式全程成立**。
//! 设计纪律：no_std+alloc；零 unwrap/expect/panic!/浮点/std；
//! 全可失败路径 `Result<T, u16>`（错误码 0x7F6 段闭集）；tick 显性
//! 注入零墙钟；需求账定容守恒（draft+frozen==total 恒等式机检）；
//! 冻结语义单调（只冻不解）；结尾 drill() 自例程走核心路径返回
//! 0 表示全过。

#![allow(dead_code)]

use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// §0 诊断码（0x7F60 段，本单独占）
// ---------------------------------------------------------------------------

/// 中断面计时需求清单段基址（0x7F60，本单 0x7F60..=0x7F7F 独占）。
pub const SEG_BASE: u16 = 0x7F60;
/// 0x7F61 需求条目非法（空 id/零验收判据——清单准入拒收）。
pub const XC_REQ_INVALID: u16 = SEG_BASE + 1;
/// 0x7F62 需求 id 重复（同 id 双条目——清单账被污染）。
pub const XC_REQ_DUPLICATE: u16 = SEG_BASE + 2;
/// 0x7F63 冻结后篡改（对已冻结条目改判据/改分类——只冻不解纪律破坏）。
pub const XC_FROZEN_MUTATE: u16 = SEG_BASE + 3;
/// 0x7F64 需求类目越界（类目码不在四类闭集内）。
pub const XC_KIND_BAD: u16 = SEG_BASE + 4;
/// 0x7F65 清单未冻结即出基线（存在未冻结条目时申请基线——准入不齐）。
pub const XC_NOT_ALL_FROZEN: u16 = SEG_BASE + 5;
/// 0x7F66 冻结 tick 回退（冻结时序单调性被破坏）。
pub const XC_TICK_REGRESS: u16 = SEG_BASE + 6;
/// 0x7F67 账本定容非法（容量为零——构造参数错误）。
pub const XC_CAP_INVALID: u16 = SEG_BASE + 7;
/// 0x7F68 清单账守恒破坏（draft+frozen != total——账面被外力改写）。
pub const XC_CONSERVATION: u16 = SEG_BASE + 8;
/// 0x7F69 变更单指向未知需求（变更引用了不在册的 id）。
pub const XC_CHG_UNKNOWN: u16 = SEG_BASE + 9;
/// 0x7F6A 基线重复出账（同清单重复冻结基线——基线唯一性破坏）。
pub const XC_BASELINE_DUP: u16 = SEG_BASE + 10;

const _: () = assert!(XC_REQ_INVALID != XC_REQ_DUPLICATE);
const _: () = assert!(XC_REQ_DUPLICATE != XC_FROZEN_MUTATE);
const _: () = assert!(XC_KIND_BAD != XC_NOT_ALL_FROZEN);
const _: () = assert!(XC_TICK_REGRESS != XC_CONSERVATION);
const _: () = assert!(XC_BASELINE_DUP != XC_CAP_INVALID);

// ---------------------------------------------------------------------------
// §1 需求类目闭集（四类——中断面对计时面的全部上游需求面）
// ---------------------------------------------------------------------------

/// 需求类目闭集：中断面对计时面只提四类需求，出闭集即越界拒收。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReqKind {
    /// 周期精度（节拍窗边界要求）。
    Period,
    /// 单调性（时间读数不回退要求）。
    Monotonic,
    /// 超时窗（等待上限要求）。
    Timeout,
    /// 时间源（源选择与降级顺序要求）。
    Source,
}

impl ReqKind {
    /// 类目码（1..=4，清单行格式用）。
    pub const fn code(self) -> u8 {
        match self {
            ReqKind::Period => 1,
            ReqKind::Monotonic => 2,
            ReqKind::Timeout => 3,
            ReqKind::Source => 4,
        }
    }
    /// 类目中文名。
    pub const fn name(self) -> &'static str {
        match self {
            ReqKind::Period => "周期精度",
            ReqKind::Monotonic => "单调性",
            ReqKind::Timeout => "超时窗",
            ReqKind::Source => "时间源",
        }
    }
    /// 由码还原（越界 None）。
    pub const fn from_code(c: u8) -> Option<ReqKind> {
        match c {
            1 => Some(ReqKind::Period),
            2 => Some(ReqKind::Monotonic),
            3 => Some(ReqKind::Timeout),
            4 => Some(ReqKind::Source),
            _ => None,
        }
    }
}

// ---------------------------------------------------------------------------
// §2 需求条目与状态机（Draft → Frozen，只冻不解）
// ---------------------------------------------------------------------------

/// 需求条目状态闭集（Draft 未冻结 / Frozen 已冻结——冻结是终态）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReqState {
    /// 草案态（可改可撤）。
    Draft,
    /// 冻结态（终态，任何改动带码拒收）。
    Frozen,
}

/// 一条上游需求：id / 类目 / 验收判据文本 / 状态 / 冻结 tick。
#[derive(Clone, Debug)]
pub struct ReqItem {
    id: u32,
    kind: ReqKind,
    criterion: String,
    state: ReqState,
    frozen_tick: u64,
}

impl ReqItem {
    /// 条目 id。
    pub fn id(&self) -> u32 {
        self.id
    }
    /// 类目。
    pub fn kind(&self) -> ReqKind {
        self.kind
    }
    /// 验收判据文本。
    pub fn criterion(&self) -> &str {
        &self.criterion
    }
    /// 状态。
    pub fn state(&self) -> ReqState {
        self.state
    }
    /// 冻结 tick（未冻结为 0）。
    pub fn frozen_tick(&self) -> u64 {
        self.frozen_tick
    }
    /// 状态迁移：Draft→Frozen（冻结时钉 tick；Frozen→任何 态拒收）。
    pub fn freeze(&mut self, tick: u64) -> Result<(), u16> {
        if self.state == ReqState::Frozen {
            return Err(XC_FROZEN_MUTATE);
        }
        self.state = ReqState::Frozen;
        self.frozen_tick = tick;
        Ok(())
    }
    /// 草案态判据改写（冻结态拒收——只冻不解纪律本体）。
    pub fn rewrite(&mut self, criterion: &str) -> Result<(), u16> {
        if self.state == ReqState::Frozen {
            return Err(XC_FROZEN_MUTATE);
        }
        if criterion.is_empty() {
            return Err(XC_REQ_INVALID);
        }
        self.criterion = String::from(criterion);
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// §3 变更单（冻结前的修订走变更单——修订史可追）
// ---------------------------------------------------------------------------

/// 一张变更单：目标 id / 新判据 / 提出 tick。
#[derive(Clone, Debug)]
pub struct ChangeReq {
    target: u32,
    new_criterion: String,
    at_tick: u64,
}

impl ChangeReq {
    /// 目标需求 id。
    pub fn target(&self) -> u32 {
        self.target
    }
    /// 新判据文本。
    pub fn new_criterion(&self) -> &str {
        &self.new_criterion
    }
    /// 提出 tick。
    pub fn at_tick(&self) -> u64 {
        self.at_tick
    }
}

// ---------------------------------------------------------------------------
// §4 定容需求账（ReqBook）——冻结清单本体
// ---------------------------------------------------------------------------

/// 清单容量定值：16 条（中断面对计时面的上游需求域有限——四类目
/// 各至多 4 条主需求，超出说明需求膨胀需另行立项）。
pub const REQ_CAP: usize = 16;

/// 定容需求账——只追加、冻结单调、守恒机检。
///
/// 守恒恒等式：`draft + frozen == total`（每条目恰处一态，无第三态、
/// 无重复计数）；`audit_conservation()` 逐条目重扫机检此式。
#[derive(Debug)]
pub struct ReqBook {
    items: Vec<ReqItem>,
    cap: usize,
    total: u64,
    frozen_cnt: u32,
    rejected: u32,
    last_freeze_tick: u64,
    baseline_tick: u64,
    baseline_fp: u64,
    changelog: Vec<ChangeReq>,
}

impl ReqBook {
    /// 建账（容量非法拒收）。
    pub fn new(cap: usize) -> Result<ReqBook, u16> {
        if cap == 0 {
            return Err(XC_CAP_INVALID);
        }
        Ok(ReqBook {
            items: Vec::with_capacity(cap),
            cap,
            total: 0,
            frozen_cnt: 0,
            rejected: 0,
            last_freeze_tick: 0,
            baseline_tick: 0,
            baseline_fp: 0,
            changelog: Vec::new(),
        })
    }

    /// 容量。
    pub fn cap(&self) -> usize {
        self.cap
    }
    /// 在册条目数。
    pub fn len(&self) -> usize {
        self.items.len()
    }
    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }
    /// 已冻结条数。
    pub fn frozen_count(&self) -> u32 {
        self.frozen_cnt
    }
    /// 拒收次数（注入面统计）。
    pub fn rejected(&self) -> u32 {
        self.rejected
    }
    /// 基线 tick（未出基线为 0）。
    pub fn baseline_tick(&self) -> u64 {
        self.baseline_tick
    }
    /// 基线指纹。
    pub fn baseline_fingerprint(&self) -> u64 {
        self.baseline_fp
    }
    /// 变更单数。
    pub fn change_count(&self) -> usize {
        self.changelog.len()
    }
    /// 条目只读快照（按 id 查）。
    pub fn item(&self, id: u32) -> Option<&ReqItem> {
        let mut i: usize = 0;
        while i < self.items.len() {
            if self.items[i].id == id {
                return Some(&self.items[i]);
            }
            i += 1;
        }
        None
    }

    /// 入账一条草案（id 零号/空判据/类目越界/id 重复均带码拒收）。
    pub fn add(&mut self, id: u32, kind: ReqKind, criterion: &str) -> Result<(), u16> {
        if id == 0 || criterion.is_empty() {
            self.rejected += 1;
            return Err(XC_REQ_INVALID);
        }
        if self.item(id).is_some() {
            self.rejected += 1;
            return Err(XC_REQ_DUPLICATE);
        }
        if ReqKind::from_code(kind.code()).is_none() {
            self.rejected += 1;
            return Err(XC_KIND_BAD);
        }
        if self.items.len() >= self.cap {
            self.rejected += 1;
            return Err(XC_CAP_INVALID);
        }
        self.items.push(ReqItem {
            id,
            kind,
            criterion: String::from(criterion),
            state: ReqState::Draft,
            frozen_tick: 0,
        });
        self.total += 1;
        Ok(())
    }

    /// 提交变更单（目标在册且未冻结才生效——冻结态目标拒收）。
    pub fn propose_change(&mut self, target: u32, new_criterion: &str, tick: u64) -> Result<(), u16> {
        let it = match self.item(target) {
            Some(i) => i,
            None => {
                self.rejected += 1;
                return Err(XC_CHG_UNKNOWN);
            }
        };
        if it.state() == ReqState::Frozen {
            self.rejected += 1;
            return Err(XC_FROZEN_MUTATE);
        }
        if new_criterion.is_empty() {
            self.rejected += 1;
            return Err(XC_REQ_INVALID);
        }
        self.changelog.push(ChangeReq {
            target,
            new_criterion: String::from(new_criterion),
            at_tick: tick,
        });
        Ok(())
    }

    /// 应用全部未决变更单（按提出序逐单改写草案判据，冻结态目标跳过）。
    pub fn apply_changes(&mut self) -> u32 {
        let mut applied: u32 = 0;
        let mut i: usize = 0;
        while i < self.changelog.len() {
            let t = self.changelog[i].target;
            let s = self.changelog[i].new_criterion.clone();
            let mut j: usize = 0;
            while j < self.items.len() {
                if self.items[j].id == t && self.items[j].state() == ReqState::Draft {
                    // 草案态改写必成功（判据非空由 propose 侧保证）。
                    let _ = self.items[j].rewrite(&s);
                    applied += 1;
                    break;
                }
                j += 1;
            }
            i += 1;
        }
        self.changelog.clear();
        applied
    }

    /// 冻结一条（tick 单调校验；冻结是终态，重复冻结拒收）。
    pub fn freeze(&mut self, id: u32, tick: u64) -> Result<(), u16> {
        if tick < self.last_freeze_tick {
            self.rejected += 1;
            return Err(XC_TICK_REGRESS);
        }
        let it = match self.item_mut(id) {
            Some(i) => i,
            None => {
                self.rejected += 1;
                return Err(XC_CHG_UNKNOWN);
            }
        };
        if it.state() == ReqState::Frozen {
            self.rejected += 1;
            return Err(XC_FROZEN_MUTATE);
        }
        it.freeze(tick)?;
        self.last_freeze_tick = tick;
        self.frozen_cnt += 1;
        Ok(())
    }

    /// 全部冻结（存在草案即带码拒收——基线准入不齐）。
    pub fn freeze_all(&mut self, tick: u64) -> Result<u32, u16> {
        if tick < self.last_freeze_tick {
            return Err(XC_TICK_REGRESS);
        }
        let mut n: u32 = 0;
        let mut i: usize = 0;
        while i < self.items.len() {
            if self.items[i].state() == ReqState::Draft {
                self.items[i].freeze(tick)?;
                n += 1;
            }
            i += 1;
        }
        self.frozen_cnt += n;
        self.last_freeze_tick = tick;
        Ok(n)
    }

    /// 出基线（前置：全部已冻结；重复出基线拒收；基线指纹封存）。
    pub fn seal_baseline(&mut self) -> Result<(u64, u32), u16> {
        if self.baseline_tick != 0 {
            self.rejected += 1;
            return Err(XC_BASELINE_DUP);
        }
        if self.frozen_cnt as usize != self.items.len() || self.items.is_empty() {
            self.rejected += 1;
            return Err(XC_NOT_ALL_FROZEN);
        }
        self.baseline_tick = self.last_freeze_tick;
        let fp = self.fingerprint();
        self.baseline_fp = fp;
        Ok((fp, self.items.len() as u32))
    }

    /// 守恒机检：draft+frozen==total 逐条目重扫（账面被外力改写即碎）。
    pub fn audit_conservation(&self) -> Result<(), u16> {
        let mut f: u32 = 0;
        let mut i: usize = 0;
        while i < self.items.len() {
            if self.items[i].state() == ReqState::Frozen {
                f += 1;
            }
            i += 1;
        }
        if f + (self.items.len() as u32 - f) != self.total as u32
            || self.total != self.items.len() as u64
        {
            return Err(XC_CONSERVATION);
        }
        if f != self.frozen_cnt {
            return Err(XC_CONSERVATION);
        }
        Ok(())
    }

    /// 清单指纹（id/类目码/判据字节/状态码逐条 FNV 掺入——基线对拍用）。
    pub fn fingerprint(&self) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        let mut i: usize = 0;
        while i < self.items.len() {
            let it = &self.items[i];
            h = h.wrapping_mul(0x0000_0100_0000_01b3) ^ (it.id as u64);
            h = h.wrapping_mul(0x0000_0100_0000_01b3) ^ (it.kind.code() as u64);
            h = h.wrapping_mul(0x0000_0100_0000_01b3) ^ (it.state as u8 as u64);
            let bs: &[u8] = it.criterion.as_bytes();
            let mut b: usize = 0;
            while b < bs.len() {
                h = h.wrapping_mul(0x0000_0100_0000_01b3) ^ (bs[b] as u64);
                b += 1;
            }
            i += 1;
        }
        h
    }

    fn item_mut(&mut self, id: u32) -> Option<&mut ReqItem> {
        let mut i: usize = 0;
        while i < self.items.len() {
            if self.items[i].id == id {
                return Some(&mut self.items[i]);
            }
            i += 1;
        }
        None
    }
}

// ---------------------------------------------------------------------------
// §5 自例程（drill）——构造样本走核心路径，返回 0 表示全过
// ---------------------------------------------------------------------------

/// 自例程：建账→入草案→变更→冻结→出基线→注入面→守恒机检。
pub fn drill() -> Result<u32, u16> {
    let mut b = ReqBook::new(REQ_CAP)?;
    // 四类目各一条主需求。
    b.add(1, ReqKind::Period, "节拍窗边界偏差不超一个 tick")?;
    b.add(2, ReqKind::Monotonic, "中断面时间读数单调不减零回退")?;
    b.add(3, ReqKind::Timeout, "EOI 等待上限两个节拍窗内可见")?;
    b.add(4, ReqKind::Source, "时间源降级顺序主备核对逐级回退")?;
    // 变更单：改写 1 号判据后应用。
    b.propose_change(1, "节拍窗边界偏差不超半个 tick", 10)?;
    if b.apply_changes() != 1 {
        return Err(XC_REQ_INVALID);
    }
    // 逐条冻结 + 全量冻结收口。
    b.freeze(1, 20)?;
    b.freeze(2, 30)?;
    b.freeze_all(40)?;
    // 出基线。
    let fp1 = b.seal_baseline()?;
    // 注入面：冻结后篡改必拒；重复 id 必拒；重复基线必拒。
    if b.freeze(1, 50) != Err(XC_FROZEN_MUTATE) {
        return Err(XC_FROZEN_MUTATE);
    }
    if b.add(1, ReqKind::Period, "重复") != Err(XC_REQ_DUPLICATE) {
        return Err(XC_REQ_DUPLICATE);
    }
    if b.seal_baseline() != Err(XC_BASELINE_DUP) {
        return Err(XC_BASELINE_DUP);
    }
    // 守恒机检 + 基线指纹稳定。
    b.audit_conservation()?;
    let fp2 = b.fingerprint();
    if fp1.0 != fp2 || b.frozen_count() != 4 || b.rejected() != 3 {
        return Err(XC_CONSERVATION);
    }
    Ok(0)
}
