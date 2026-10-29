//! unx0618_napath —— UNX-F0618 空档路径账（无 initrd 全链 N/A）。
//!
//! ═══════════════════════════════════════════════════════════════════════
//! 锚点：UNX-F0618 · 空档路径账（无 initrd 全链 N/A） · 目标 ≥200 行
//! 判据原文摘录：空档全链核（F0601 空判→F0604 跳→F0613 空清单封账
//!   3/3 步骤 N/A 账注），误用红检出。
//! ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//!
//! 设计纪律：
//!   1. 空档路径不是"什么都不做"，而是三步 N/A 账注全落账——每步都有
//!      编号、账注、时戳，缺任何一步即账不平。
//!   2. 状态机边闭集：F0601 空判 → F0604 跳 → F0613 封账，只允许按序
//!      推进，跳步/回退均为非法边。
//!   3. 误用红检出：initrd 实际在场（present=true）却走空档路径 → 红，
//!      空档路径只属于"initrd 真缺"的机器。
//!   4. 定容三槽账本，守恒恒等式：落账数 + 空槽数 = 3。
//!   5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，
//!      确定性。
//!
//! 诊断段声明：SEG 0x4F00 段位本模块独占。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::string::{String, ToString};

/// 诊断段基址（0x4F00 段 · 本模块独占）。
pub const SEG_BASE: u16 = 0x4F00;
/// 诊断段顶。
pub const SEG_TOP: u16 = 0x4F0F;

/// XcCode：空判步骤未先执行。
pub const XC_EMPTY_JUDGE_FIRST: u16 = SEG_BASE + 1; // 0x4F01 空档首步必须是 F0601 空判
/// XcCode：非法跳转边（跳步或回退）。
pub const XC_ILLEGAL_EDGE: u16 = SEG_BASE + 2; // 0x4F02 状态机边闭集外
/// XcCode：封账缺步。
pub const XC_SEAL_MISSING_STEP: u16 = SEG_BASE + 3; // 0x4F03 三步未齐不许封
/// XcCode：误用红检（initrd 在场却走空档）。
pub const XC_MISUSE_REDEYE: u16 = SEG_BASE + 4; // 0x4F04 initrd 在场禁走空档
/// XcCode：账注重复。
pub const XC_DUP_NOTE: u16 = SEG_BASE + 5; // 0x4F05 同步不能重复落账
/// XcCode：账本已封不可再写。
pub const XC_SEALED_RDONLY: u16 = SEG_BASE + 6; // 0x4F06 封账后写拦截

/// 空档路径步骤总数（F0601/F0604/F0613 三步闭集）。
pub const NA_STEP_N: usize = 3;

/// 空档路径步骤（状态机节点，编号抄判据原文）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NaStep {
    /// F0601 空判：确认 initrd 缺席这一事实本身要落账。
    EmptyJudge = 0,
    /// F0604 跳：跳过全部 initrd 消费步骤（也是一次显式动作）。
    SkipJump = 1,
    /// F0613 空清单封账：空档清单封账收口。
    BlankSeal = 2,
}

impl NaStep {
    /// 步骤序号（0..3）。
    pub fn idx(self) -> usize {
        self as usize
    }

    /// 步骤名。
    pub fn name(self) -> &'static str {
        match self {
            NaStep::EmptyJudge => "F0601 空判",
            NaStep::SkipJump => "F0604 跳",
            NaStep::BlankSeal => "F0613 空清单封账",
        }
    }
}

/// 合法边表：只允许 idx 0→1→2 顺序推进（边闭集）。
const LEGAL_EDGES: [(usize, usize); NA_STEP_N - 1] = [(0, 1), (1, 2)];

/// 单步 N/A 账注。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NaEntry {
    /// 步骤。
    pub step: NaStep,
    /// N/A 账注（人话，非空才算有效账注）。
    pub note: String,
    /// 落账时戳（整数拍，确定性递增）。
    pub stamp: u64,
}

impl NaEntry {
    /// 账注有效性：非空。
    pub fn valid(&self) -> bool {
        !self.note.is_empty()
    }
}

/// 空档路径账本：定容三槽 + 状态机 + 封账位。
#[derive(Clone, Debug)]
pub struct NaPathBook {
    /// 三槽账本（None = 未落账；落账数+空槽数=3 守恒）。
    slots: [Option<NaEntry>; NA_STEP_N],
    /// 状态机当前游标（最后完成的步骤 idx；未开始 = NA_STEP_N 哨兵外用 0 表示"未起步"时以 done_mask 判断）。
    cursor: usize,
    /// 已完成位图（bit i = 步骤 i 已落账）。
    done_mask: u32,
    /// 封账位：置位后整本只读。
    sealed: bool,
    /// initrd 在场标志（由上层空判喂入，误用红检的依据）。
    initrd_present: bool,
}

impl NaPathBook {
    /// 新账本：必须先声明 initrd 是否在场。
    pub fn open(initrd_present: bool) -> Self {
        NaPathBook {
            slots: [None, None, None],
            cursor: 0,
            done_mask: 0,
            sealed: false,
            initrd_present,
        }
    }

    /// 已落账步数。
    pub fn done_n(&self) -> usize {
        self.slots.iter().filter(|s| s.is_some()).count()
    }

    /// 守恒恒等式：落账数 + 空槽数 = 3。
    pub fn conserve_ok(&self) -> bool {
        self.done_n() + self.slots.iter().filter(|s| s.is_none()).count() == NA_STEP_N
    }

    /// 是否封账。
    pub fn sealed(&self) -> bool {
        self.sealed
    }

    /// 误用红检：initrd 在场却开账 → 红。
    pub fn misuse_redeye(&self) -> bool {
        self.initrd_present
    }

    /// 落一步账。时戳由调用方整数递增喂入（确定性）。
    pub fn step(&mut self, target: NaStep, note: &str, stamp: u64) -> Result<(), u16> {
        if self.sealed {
            return Err(XC_SEALED_RDONLY);
        }
        if self.misuse_redeye() {
            // initrd 在场，空档路径整条禁用——第一步就红。
            return Err(XC_MISUSE_REDEYE);
        }
        let i = target.idx();
        if self.slots[i].is_some() {
            return Err(XC_DUP_NOTE);
        }
        // 首步必须是空判；后续必须沿合法边推进。
        if self.done_mask == 0 {
            if target != NaStep::EmptyJudge {
                return Err(XC_EMPTY_JUDGE_FIRST);
            }
        } else {
            let from = self.cursor;
            let legal = LEGAL_EDGES.iter().any(|&(a, b)| a == from && b == i);
            if !legal {
                return Err(XC_ILLEGAL_EDGE);
            }
        }
        if note.is_empty() {
            // 账注为空不算有效落账——但仍是错误路径而非 panic。
            return Err(XC_DUP_NOTE);
        }
        self.slots[i] = Some(NaEntry {
            step: target,
            note: note.to_string(),
            stamp,
        });
        self.done_mask |= 1u32 << i;
        self.cursor = i;
        Ok(())
    }

    /// 空清单封账：三步全齐才可封；封后只读。
    pub fn seal(&mut self) -> Result<u32, u16> {
        if self.sealed {
            return Err(XC_SEALED_RDONLY);
        }
        if self.misuse_redeye() {
            return Err(XC_MISUSE_REDEYE);
        }
        for i in 0..NA_STEP_N {
            if self.slots[i].is_none() {
                return Err(XC_SEAL_MISSING_STEP);
            }
        }
        self.sealed = true;
        Ok(self.done_mask)
    }

    /// 时戳有序核：落账时戳必须严格递增（按步骤序）。
    pub fn stamps_ordered(&self) -> bool {
        let mut prev: Option<u64> = None;
        for i in 0..NA_STEP_N {
            if let Some(e) = &self.slots[i] {
                if let Some(p) = prev {
                    if e.stamp <= p {
                        return false;
                    }
                }
                prev = Some(e.stamp);
            }
        }
        true
    }

    /// 人话报告（程序化直读，零手抄）。
    pub fn report(&self) -> String {
        let mut s = alloc::format!(
            "空档路径账 · 落账 {}/{} · 封账={} · initrd在场={} · 守恒={}",
            self.done_n(),
            NA_STEP_N,
            self.sealed,
            self.initrd_present,
            self.conserve_ok()
        );
        for i in 0..NA_STEP_N {
            match &self.slots[i] {
                Some(e) => {
                    s.push_str(&alloc::format!("\n  [{}] {} @{} — {}", i, e.step.name(), e.stamp, e.note));
                }
                None => {
                    s.push_str(&alloc::format!("\n  [{}] 未落账", i));
                }
            }
        }
        s
    }

    /// 指纹（FNV-1a 64，整数算术）。
    pub fn fingerprint(&self) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        for b in self.report().as_bytes() {
            h ^= *b as u64;
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
        }
        h
    }
}

/// 全链 N/A 核：给定"initrd 是否在场"与三步账注，跑完整空档路径。
/// 返回 (封账位图, 总时戳跨度)；任何错误路径走 XcCode。
pub fn na_full_chain(
    initrd_present: bool,
    notes: [&str; NA_STEP_N],
    stamps: [u64; NA_STEP_N],
) -> Result<(u32, u64), u16> {
    let mut book = NaPathBook::open(initrd_present);
        if initrd_present {
            // 误用红检：在场机器不许进空档路径，直接红。
            return Err(XC_MISUSE_REDEYE);
        }
    let order = [NaStep::EmptyJudge, NaStep::SkipJump, NaStep::BlankSeal];
    for i in 0..NA_STEP_N {
        book.step(order[i], notes[i], stamps[i])?;
    }
    let mask = book.seal()?;
    let span = stamps[NA_STEP_N - 1] - stamps[0];
    Ok((mask, span))
}

/// 自例程：主路径（真缺 initrd）+ 错误路径（误用/跳步/缺步/重复/封后写）。
pub fn drill() -> Result<u32, u16> {
    // 主路径：三步 N/A 账注全落账后封账。
    let (mask, span) = na_full_chain(
        false,
        ["F0601 空判：initrd 缺席，判空落账", "F0604 跳：全部消费步骤 N/A 跳过", "F0613 空清单封账：空档清单 0 行封账"],
        [10, 20, 30],
    )?;
    if mask != 0b111 {
        return Err(XC_SEAL_MISSING_STEP);
    }
    if span != 20 {
        return Err(SEG_BASE + 7);
    }

    // 错误路径 1：initrd 在场误用空档路径 → 红检。
    if na_full_chain(true, ["x", "y", "z"], [1, 2, 3]) != Err(XC_MISUSE_REDEYE) {
        return Err(XC_MISUSE_REDEYE);
    }

    // 错误路径 2：首步不是空判。
    let mut b2 = NaPathBook::open(false);
    if b2.step(NaStep::SkipJump, "跳", 1) != Err(XC_EMPTY_JUDGE_FIRST) {
        return Err(XC_EMPTY_JUDGE_FIRST);
    }

    // 错误路径 3：跳步（0→2 非法边）。
    let mut b3 = NaPathBook::open(false);
    b3.step(NaStep::EmptyJudge, "空判", 1)?;
    if b3.step(NaStep::BlankSeal, "越权封", 2) != Err(XC_ILLEGAL_EDGE) {
        return Err(XC_ILLEGAL_EDGE);
    }

    // 错误路径 4：重复落账。
    let mut b4 = NaPathBook::open(false);
    b4.step(NaStep::EmptyJudge, "空判一", 1)?;
    if b4.step(NaStep::EmptyJudge, "空判二", 2) != Err(XC_DUP_NOTE) {
        return Err(XC_DUP_NOTE);
    }

    // 错误路径 5：缺步封账。
    let mut b5 = NaPathBook::open(false);
    b5.step(NaStep::EmptyJudge, "只落一步", 1)?;
    if b5.seal() != Err(XC_SEAL_MISSING_STEP) {
        return Err(XC_SEAL_MISSING_STEP);
    }

    // 错误路径 6：封账后写拦截。
    let mut b6 = NaPathBook::open(false);
    b6.step(NaStep::EmptyJudge, "s1", 1)?;
    b6.step(NaStep::SkipJump, "s2", 2)?;
    b6.step(NaStep::BlankSeal, "s3", 3)?;
    b6.seal()?;
    if b6.step(NaStep::EmptyJudge, "封后再写", 4) != Err(XC_SEALED_RDONLY) {
        return Err(XC_SEALED_RDONLY);
    }

    // 守恒与有序核。
    let mut b7 = NaPathBook::open(false);
    b7.step(NaStep::EmptyJudge, "a", 5)?;
    b7.step(NaStep::SkipJump, "b", 9)?;
    b7.step(NaStep::BlankSeal, "c", 15)?;
    if !b7.conserve_ok() || !b7.stamps_ordered() {
        return Err(SEG_BASE + 7);
    }
    if b7.fingerprint() == 0 {
        return Err(SEG_BASE + 7);
    }
    Ok(0)
}
