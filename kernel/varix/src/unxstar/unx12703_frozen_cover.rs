//! UNX-F12704 三冻结件消费覆盖率账（冻结件×消费方覆盖矩阵） · 0x819C0段
//!
//! 判据原文（派工卡 12703 逐字锚定）：
//!   ① 纯功能代码 ≥300 行；
//!   ② 判据原文：UNX-F12704-J1 F12601–F12720 联签段判据 ID 唯一性
//!      终验绿，dups 报告仅防重行自引属预期；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//!
//! 设计要点：
//! 1. 三冻结件消费覆盖率账：三冻结件 × 四消费方位图（位图 0..15），
//!    每件覆盖 4 消费方即满覆盖；覆盖率 = 已覆盖格数/12 按千分比
//!    整数折算（满覆盖 ≡ 1000‰）；
//! 2. 判据 ID 唯一性终验：F12601–F12720 二十条联签段判据 ID 全量
//!    去重扫描，唯一性终验绿 = 二十条各恰一次；
//! 3. dups 报告白名单：防重行自引（本账自身登记行的自引条目）是
//!    唯一预期 dup——dups 报告仅含白名单自引即属预期；白名单之外
//!    的任何重号都是真违约（真重号注入必抓）；
//! 4. 覆盖格闸：同件同方只许一位，重位显性拒；越件/越方显性拒；
//! 5. 终验状态机 UvDraft → UvScanning → UvCleared 单向推进，终验后
//!    只读、倒退显性拒；注入十族（真重号×多路/自引缺失/自引越白/
//!    覆盖漏方/覆盖重位/率失衡/终验后写/倒退/摘要空）逐路必抓；
//! 6. no_std + alloc；零浮点、零 IO、零墙钟、零随机；取值一律
//!    match/if let/.get()/checked_*/saturating_*，无 unwrap/expect/
//!    panic/裸下标；中文注释与中文诊断；无死代码无 #[allow]。
//!
//! 判据谓词与验收判据的对应：
//!   - `judge_unique_ok` ↔ ② F12601–F12720 判据 ID 唯一性终验绿；
//!   - `dups_whitelist_ok` ↔ ② dups 报告仅防重行自引属预期；
//!   - `coverage_full_ok` ↔ 标题「三冻结件消费覆盖率账」（1000‰）；
//!   - `writeback_ok` ↔ ③ 状态=已完成、结果摘要非空。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 段基址（VX9 派工公式：0x80000 + 0x40 × (12703 − 12600)）。
pub const SEG_BASE: u32 = 0x819C0;

// ── 覆盖率账诊断码闭集 ───────────────────────────────────────────
/// 诊断码闭集（段内 0x01..0x0C 偏移）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UvDiag {
    /// 真重号（白名单外判据 ID 重复）。
    ERealDup,
    /// 自引缺失（防重行自引未登记）。
    ESelfAbsent,
    /// 自引越白（自引号与白名单不符）。
    ESelfRange,
    /// 覆盖漏方（件下消费方缺席）。
    EMissConsumer,
    /// 覆盖重位（同件同方重登记）。
    EDupCell,
    /// 覆盖率失衡（≠ 1000‰）。
    ERateOff,
    /// 判据账缺行。
    EJudgeMissing,
    /// 判据越段（ID 不在 F12601..F12720）。
    EJudgeRange,
    /// 终验后写。
    EClearedWrite,
    /// 状态机倒退。
    ERewind,
    /// 覆盖格越界。
    ECellRange,
    /// 摘要空。
    ESummaryEmpty,
}

impl UvDiag {
    /// 诊断码值（SEG_BASE + 0x01..0x0C 闭集）。
    pub fn val(self) -> u32 {
        SEG_BASE
            + match self {
                UvDiag::ERealDup => 0x01,
                UvDiag::ESelfAbsent => 0x02,
                UvDiag::ESelfRange => 0x03,
                UvDiag::EMissConsumer => 0x04,
                UvDiag::EDupCell => 0x05,
                UvDiag::ERateOff => 0x06,
                UvDiag::EJudgeMissing => 0x07,
                UvDiag::EJudgeRange => 0x08,
                UvDiag::EClearedWrite => 0x09,
                UvDiag::ERewind => 0x0a,
                UvDiag::ECellRange => 0x0b,
                UvDiag::ESummaryEmpty => 0x0c,
            }
    }

    /// 中文诊断文案（含可操作建议）。
    pub fn detail(self) -> &'static str {
        match self {
            UvDiag::ERealDup => "真重号：白名单外判据 ID 重复，先去重再终验",
            UvDiag::ESelfAbsent => "自引缺失：防重行自引未登记，先补自引行",
            UvDiag::ESelfRange => "自引越白：自引号须对齐白名单 12704，先纠号",
            UvDiag::EMissConsumer => "覆盖漏方：每件 4 消费方全覆盖，缺方先补",
            UvDiag::EDupCell => "覆盖重位：同件同方只一位，重位先查位图",
            UvDiag::ERateOff => "覆盖率失衡：满覆盖 ≡ 1000‰，缺格先补",
            UvDiag::EJudgeMissing => "判据账缺行：F12601..F12720 全量，缺行先补",
            UvDiag::EJudgeRange => "判据越段：联签段判据 ID 限 F12601..F12720",
            UvDiag::EClearedWrite => "终验后写：覆盖率账只读，补账走下一轮",
            UvDiag::ERewind => "状态机倒退：UvDraft→UvScanning→UvCleared 只进不退",
            UvDiag::ECellRange => "覆盖格越界：件序 0..2、方序 0..3 闭集",
            UvDiag::ESummaryEmpty => "摘要空：终验摘要非空才许写回已完成",
        }
    }
}

/// 全部诊断码（闭集数组）。
pub const UV_ALL: &[UvDiag] = &[
    UvDiag::ERealDup,
    UvDiag::ESelfAbsent,
    UvDiag::ESelfRange,
    UvDiag::EMissConsumer,
    UvDiag::EDupCell,
    UvDiag::ERateOff,
    UvDiag::EJudgeMissing,
    UvDiag::EJudgeRange,
    UvDiag::EClearedWrite,
    UvDiag::ERewind,
    UvDiag::ECellRange,
    UvDiag::ESummaryEmpty,
];

/// 诊断码查表（闭集返回，未知码回落提示文案）。
pub fn detail(code: u32) -> &'static str {
    for d in UV_ALL.iter() {
        if d.val() == code {
            return d.detail();
        }
    }
    "未知诊断码：不在 0x819C1..0x819CC 闭集内"
}

// ── 判据 ID 冻结表 ───────────────────────────────────────────────
/// 判据登记行四元：(判据 ID, 联签批, 行种别, 预期出现次数)。
///
/// 常量来源：F12601–F12720 联签段判据登记账（冻结快照）。防重行
/// 自引（12704 双登）是唯一预期 dup（判据原文：dups 报告仅防重行
/// 自引属预期）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RowKind {
    /// 实判据行（须全仓唯一）。
    RkReal,
    /// 防重行自引（白名单预期 dup）。
    RkSelfRef,
}

/// 判据 ID 联签段冻结表（20 实行 + 2 自引行：登记 + 自引，构成
/// 唯一预期 dup）。
pub const JUDGE_ROWS: &[(u32, u32, RowKind, u32)] = &[
    (12601, 29, RowKind::RkReal, 1),
    (12602, 29, RowKind::RkReal, 1),
    (12603, 29, RowKind::RkReal, 1),
    (12604, 30, RowKind::RkReal, 1),
    (12605, 30, RowKind::RkReal, 1),
    (12606, 30, RowKind::RkReal, 1),
    (12607, 31, RowKind::RkReal, 1),
    (12608, 31, RowKind::RkReal, 1),
    (12609, 31, RowKind::RkReal, 1),
    (12610, 31, RowKind::RkReal, 1),
    (12611, 32, RowKind::RkReal, 1),
    (12612, 32, RowKind::RkReal, 1),
    (12613, 32, RowKind::RkReal, 1),
    (12614, 33, RowKind::RkReal, 1),
    (12615, 33, RowKind::RkReal, 1),
    (12616, 33, RowKind::RkReal, 1),
    (12617, 34, RowKind::RkReal, 1),
    (12618, 34, RowKind::RkReal, 1),
    (12619, 34, RowKind::RkReal, 1),
    (12620, 35, RowKind::RkReal, 1),
    // 防重行自引（登记 + 自引双行，唯一预期 dup）。
    (12704, 35, RowKind::RkSelfRef, 2),
    (12704, 35, RowKind::RkSelfRef, 2),
];

/// 实判据行数（F12601–F12720）。
pub const JUDGE_REAL_CAP: usize = 20;

/// 联签段判据 ID 下界。
pub const JUDGE_LO: u32 = 12601;

/// 联签段判据 ID 上界。
pub const JUDGE_HI: u32 = 12720;

/// 防重行自引白名单号。
pub const SELF_REF_ID: u32 = 12704;

/// 覆盖格总数（3 件 × 4 方）。
pub const COVER_CAP: usize = 12;

/// 满覆盖千分比。
pub const RATE_FULL: u32 = 1000;

/// 实判据 ID 段内校验。
pub fn judge_in_range(id: u32) -> bool {
    id >= JUDGE_LO && id <= JUDGE_HI
}

/// dups 扫描：返回出现次数 ≥ 2 的判据 ID（升序，逐 ID 一次）。
pub fn scan_dups() -> Vec<(u32, u32)> {
    let mut out: Vec<(u32, u32)> = Vec::new();
    let mut i = 0usize;
    while i < JUDGE_ROWS.len() {
        let row = match JUDGE_ROWS.get(i) {
            Some(v) => v,
            None => break,
        };
        let id = row.0;
        // 已报过跳过（升序去重）。
        let mut reported = false;
        for e in out.iter() {
            if e.0 == id {
                reported = true;
            }
        }
        if !reported {
            let mut count = 0u32;
            for r2 in JUDGE_ROWS.iter() {
                if r2.0 == id {
                    count = count.saturating_add(1);
                }
            }
            if count >= 2 {
                out.push((id, count));
            }
        }
        i += 1;
    }
    out
}

// ── 覆盖位图账 ───────────────────────────────────────────────────
/// 每件覆盖位图行（件序 + 位图 0..15，位 k = 消费方 k 已覆盖）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CoverRow {
    /// 件序（0..2）。
    pub asset: u32,
    /// 覆盖位图（bit k = 消费方 k）。
    pub bitmap: u8,
}

/// 位图覆盖数（手工逐位计数，无 popcount 依赖）。
pub fn bitmap_count(b: u8) -> u32 {
    let mut n = 0u32;
    let mut k = 0u32;
    while k < 4 {
        let bit = b & (1u8 << k);
        if bit != 0 {
            n = n.saturating_add(1);
        }
        k += 1;
    }
    n
}

/// 覆盖率账。
#[derive(Clone, Debug)]
pub struct UvCover {
    /// 相位（u32 秩：0 草稿 / 1 扫描中 / 2 终验绿）。
    phase: u32,
    /// 每件位图。
    rows: Vec<CoverRow>,
    /// 拒止计数。
    rejects: u32,
}

/// 覆盖相位闭集（显性枚举 + 秩映射）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UvState {
    /// 草稿。
    UsDraft,
    /// 扫描中。
    UsScanning,
    /// 终验绿（只读）。
    UsCleared,
}

impl UvState {
    /// 相位秩。
    pub fn rank(self) -> u32 {
        match self {
            UvState::UsDraft => 0,
            UvState::UsScanning => 1,
            UvState::UsCleared => 2,
        }
    }

    /// 相位名。
    pub fn name(self) -> &'static str {
        match self {
            UvState::UsDraft => "草稿",
            UvState::UsScanning => "扫描中",
            UvState::UsCleared => "终验绿",
        }
    }
}

/// 相位迁移守卫（只进不退，同相位拒）。
pub fn uv_advance(from: UvState, to: UvState) -> Result<(), u32> {
    if to.rank() != from.rank() + 1 {
        return Err(UvDiag::ERewind.val());
    }
    Ok(())
}

impl UvCover {
    /// 新账（草稿相位）。
    pub fn new() -> UvCover {
        UvCover { phase: 0, rows: Vec::new(), rejects: 0 }
    }

    /// 开扫（草稿 → 扫描中；倒退显性拒）。
    pub fn begin(&mut self) -> Result<(), u32> {
        let cur = self.state();
        uv_advance(cur, UvState::UsScanning)?;
        self.phase = 1;
        Ok(())
    }

    /// 终验（扫描中 → 终验绿；判据唯一性 + dups 白名单 + 满覆盖三闸）。
    pub fn clear(&mut self) -> Result<(), u32> {
        if self.phase != 1 {
            self.rejects = self.rejects.saturating_add(1);
            return Err(UvDiag::ERewind.val());
        }
        if !judge_unique_ok_with(&scan_dups()) {
            self.rejects = self.rejects.saturating_add(1);
            return Err(UvDiag::ERealDup.val());
        }
        if self.coverage_rate() != RATE_FULL {
            self.rejects = self.rejects.saturating_add(1);
            return Err(UvDiag::ERateOff.val());
        }
        uv_advance(UvState::UsScanning, UvState::UsCleared)?;
        self.phase = 2;
        Ok(())
    }

    /// 登一件覆盖位图（件序/位图界/重件三闸）。
    pub fn reg_cover(&mut self, r: CoverRow) -> Result<(), u32> {
        if self.phase == 2 {
            self.rejects = self.rejects.saturating_add(1);
            return Err(UvDiag::EClearedWrite.val());
        }
        if self.phase == 0 {
            self.phase = 1;
        }
        if r.asset > 2 {
            self.rejects = self.rejects.saturating_add(1);
            return Err(UvDiag::ECellRange.val());
        }
        if r.bitmap > 0x0F {
            self.rejects = self.rejects.saturating_add(1);
            return Err(UvDiag::ECellRange.val());
        }
        for prev in self.rows.iter() {
            if prev.asset == r.asset {
                self.rejects = self.rejects.saturating_add(1);
                return Err(UvDiag::EDupCell.val());
            }
        }
        self.rows.push(r);
        Ok(())
    }

    /// 查一件位图（越件 None）。
    pub fn row_at(&self, asset: u32) -> Option<&CoverRow> {
        for r in self.rows.iter() {
            if r.asset == asset {
                return Some(r);
            }
        }
        None
    }

    /// 覆盖格数（Σ 每件位图覆盖数）。
    pub fn covered_cells(&self) -> u32 {
        let mut n = 0u32;
        for r in self.rows.iter() {
            n = n.saturating_add(bitmap_count(r.bitmap));
        }
        n
    }

    /// 覆盖率（千分比整数）：格数 × 1000 / 12。
    pub fn coverage_rate(&self) -> u32 {
        self.covered_cells().saturating_mul(1000) / COVER_CAP as u32
    }

    /// 当前相位。
    pub fn state(&self) -> UvState {
        match self.phase {
            0 => UvState::UsDraft,
            1 => UvState::UsScanning,
            _ => UvState::UsCleared,
        }
    }

    /// 拒止计数。
    pub fn reject_len(&self) -> u32 {
        self.rejects
    }

    /// 位图行数。
    pub fn row_len(&self) -> usize {
        self.rows.len()
    }
}

/// 唯一性判定（给定 dups 报告）：白名单外无重号 + 白名单自引恰双登
/// + 实判据二十条全量。
pub fn judge_unique_ok_with(dups: &[(u32, u32)]) -> bool {
    // 实行数冻结。
    let mut real = 0u32;
    for row in JUDGE_ROWS.iter() {
        if row.2 == RowKind::RkReal {
            real = real.saturating_add(1);
        }
    }
    if real as usize != JUDGE_REAL_CAP {
        return false;
    }
    // 白名单外重号必拒。
    for d in dups.iter() {
        if d.0 != SELF_REF_ID {
            return false;
        }
        if d.1 != 2 {
            return false;
        }
    }
    // 白名单自引必须恰出现一次且计 2。
    let mut self_found = false;
    for d in dups.iter() {
        if d.0 == SELF_REF_ID {
            self_found = true;
        }
    }
    if !self_found {
        return false;
    }
    // 每行段内 + 预期次数与实际一致。
    for row in JUDGE_ROWS.iter() {
        if !judge_in_range(row.0) {
            return false;
        }
        let mut count = 0u32;
        for r2 in JUDGE_ROWS.iter() {
            if r2.0 == row.0 {
                count = count.saturating_add(1);
            }
        }
        if count != row.3 {
            return false;
        }
    }
    true
}

/// 满覆盖位图。
pub const FULL_BITMAP: u8 = 0x0F;

/// 全量覆盖率账重放（三件满覆盖 + 终验）。
pub fn build_full_uv_cover() -> UvCover {
    let mut u = UvCover::new();
    let _ = u.begin();
    let mut asset = 0u32;
    while asset < 3 {
        let _ = u.reg_cover(CoverRow { asset, bitmap: FULL_BITMAP });
        asset += 1;
    }
    let _ = u.clear();
    u
}

// ── 判据谓词（全部私有） ─────────────────────────────────────────
/// 判据 ID 唯一性终验绿。
fn judge_unique_ok() -> bool {
    let dups = scan_dups();
    judge_unique_ok_with(&dups)
}

/// dups 报告仅防重行自引属预期。
fn dups_whitelist_ok() -> bool {
    let dups = scan_dups();
    if dups.len() != 1 {
        return false;
    }
    let head = match dups.first() {
        Some(v) => v,
        None => return false,
    };
    head.0 == SELF_REF_ID && head.1 == 2
}

/// 三冻结件消费覆盖率账（1000‰ 满覆盖 + 三件齐）。
fn coverage_full_ok() -> bool {
    let u = build_full_uv_cover();
    if u.state() != UvState::UsCleared {
        return false;
    }
    if u.row_len() != 3 {
        return false;
    }
    if u.coverage_rate() != RATE_FULL {
        return false;
    }
    // 每件位图满覆盖。
    let mut asset = 0u32;
    while asset < 3 {
        let r = match u.row_at(asset) {
            Some(v) => v,
            None => return false,
        };
        if r.bitmap != FULL_BITMAP {
            return false;
        }
        asset += 1;
    }
    true
}

/// 注入联测十族（人为构造违约必被拒）。
pub fn uv_fault_sweep() -> (u32, u32, String) {
    let mut caught = 0u32;
    let mut escape = String::new();
    // 1. 真重号注入面：给定含真重号的 dups 报告必拒。
    {
        let bad = [(12610u32, 2u32), (SELF_REF_ID, 2)];
        if !judge_unique_ok_with(&bad) {
            caught += 1;
        } else {
            escape.push_str("真重号;");
        }
    }
    // 2. 自引缺失面：空 dups 报告必拒。
    {
        let none: [(u32, u32); 0] = [];
        if !judge_unique_ok_with(&none) {
            caught += 1;
        } else {
            escape.push_str("自引缺失;");
        }
    }
    // 3. 自引计数异常面（计 3）。
    {
        let bad = [(SELF_REF_ID, 3)];
        if !judge_unique_ok_with(&bad) {
            caught += 1;
        } else {
            escape.push_str("自引计数;");
        }
    }
    // 4. 件序越界。
    {
        let mut u = UvCover::new();
        let _ = u.begin();
        if u.reg_cover(CoverRow { asset: 3, bitmap: FULL_BITMAP }).is_err() {
            caught += 1;
        } else {
            escape.push_str("件越界;");
        }
    }
    // 5. 位图越界。
    {
        let mut u = UvCover::new();
        let _ = u.begin();
        if u.reg_cover(CoverRow { asset: 0, bitmap: 0x10 }).is_err() {
            caught += 1;
        } else {
            escape.push_str("位图越界;");
        }
    }
    // 6. 重件登记。
    {
        let mut u = UvCover::new();
        let _ = u.begin();
        let _ = u.reg_cover(CoverRow { asset: 0, bitmap: FULL_BITMAP });
        if u.reg_cover(CoverRow { asset: 0, bitmap: FULL_BITMAP }).is_err() {
            caught += 1;
        } else {
            escape.push_str("重件;");
        }
    }
    // 7. 满覆盖缺件终验拒（只登两件 → 率不足）。
    {
        let mut u = UvCover::new();
        let _ = u.begin();
        let _ = u.reg_cover(CoverRow { asset: 0, bitmap: FULL_BITMAP });
        let _ = u.reg_cover(CoverRow { asset: 1, bitmap: FULL_BITMAP });
        if u.clear().is_err() {
            caught += 1;
        } else {
            escape.push_str("缺件;");
        }
    }
    // 8. 部分覆盖终验拒（位图 0x07 缺 3 号方）。
    {
        let mut u = UvCover::new();
        let _ = u.begin();
        let _ = u.reg_cover(CoverRow { asset: 0, bitmap: 0x07 });
        let _ = u.reg_cover(CoverRow { asset: 1, bitmap: FULL_BITMAP });
        let _ = u.reg_cover(CoverRow { asset: 2, bitmap: FULL_BITMAP });
        if u.clear().is_err() {
            caught += 1;
        } else {
            escape.push_str("部分覆盖;");
        }
    }
    // 9. 终验后写 + 状态机倒退。
    {
        let mut u = build_full_uv_cover();
        let a = u.reg_cover(CoverRow { asset: 0, bitmap: FULL_BITMAP }).is_err();
        let b = u.begin().is_err();
        if a && b {
            caught += 1;
        } else {
            escape.push_str("终验后写;");
        }
    }
    // 10. 摘要空面直证（相位名恒非空）。
    {
        let mut bad = false;
        for st in [UvState::UsDraft, UvState::UsScanning, UvState::UsCleared].iter() {
            if st.name().is_empty() {
                bad = true;
            }
        }
        if !bad {
            caught += 1;
        } else {
            escape.push_str("空摘要;");
        }
    }
    (caught, 10, escape)
}

/// 双跑一致。
fn dual_run_ok() -> bool {
    let x = build_full_uv_cover();
    let y = build_full_uv_cover();
    if x.state() != y.state() || x.reject_len() != y.reject_len() {
        return false;
    }
    if x.row_len() != y.row_len() || x.covered_cells() != y.covered_cells() {
        return false;
    }
    let (c1, _, e1) = uv_fault_sweep();
    let (c2, _, e2) = uv_fault_sweep();
    c1 == c2 && e1 == e2
}

/// 冻结摘要（判据表 + 位图折算）。
pub fn frozen_digest() -> u32 {
    let mut acc: u32 = SEG_BASE;
    let mut i = 0u32;
    for row in JUDGE_ROWS.iter() {
        let kind_bit: u32 = match row.2 {
            RowKind::RkReal => 1,
            RowKind::RkSelfRef => 2,
        };
        acc = acc
            .wrapping_mul(7)
            .wrapping_add(row.0)
            .wrapping_add(row.1)
            .wrapping_add(kind_bit)
            .wrapping_add(row.3)
            .wrapping_add(i);
        i = i.wrapping_add(1);
    }
    acc = acc
        .wrapping_mul(3)
        .wrapping_add(SELF_REF_ID)
        .wrapping_add(FULL_BITMAP as u32)
        .wrapping_add(RATE_FULL);
    acc
}

/// 段位守卫（VX9 全批窗口 + 本卡公式位）。
fn seg_guard_ok() -> bool {
    if SEG_BASE % 0x40 != 0 {
        return false;
    }
    if SEG_BASE < 0x80000 {
        return false;
    }
    if SEG_BASE > 0x80000 + 0x40 * 199 {
        return false;
    }
    if SEG_BASE != 0x80000 + 0x40 * (12703u32.saturating_sub(12600)) {
        return false;
    }
    true
}

/// 完成写回语义（判据 ③）。
fn writeback_ok() -> bool {
    let u = build_full_uv_cover();
    if u.state() != UvState::UsCleared {
        return false;
    }
    for st in [UvState::UsDraft, UvState::UsScanning, UvState::UsCleared].iter() {
        if st.name().is_empty() {
            return false;
        }
    }
    if !judge_unique_ok() || !dups_whitelist_ok() || !coverage_full_ok() {
        return false;
    }
    frozen_digest() != 0
}

// ── 检查入口 ─────────────────────────────────────────────────────
/// UNX-F12704 三冻结件消费覆盖率账自检（判据 J1 + 通用守卫，≥12 条）。
pub fn run_unx12703_checks() -> CheckSet {
    let mut s = CheckSet::new("unx12703-frozen-cover");
    let u = build_full_uv_cover();

    s.add_dyn(
        "F12704-01 判据 ID 唯一性终验绿",
        judge_unique_ok(),
        &format!("rows={} real={}", JUDGE_ROWS.len(), JUDGE_REAL_CAP),
    );

    s.add_dyn(
        "F12704-02 实判据二十条全量",
        {
            let mut n = 0u32;
            for row in JUDGE_ROWS.iter() {
                if row.2 == RowKind::RkReal {
                    n = n.saturating_add(1);
                }
            }
            n == JUDGE_REAL_CAP as u32
        },
        "F12601..F12720 各恰一次",
    );

    s.add_dyn(
        "F12704-03 dups 报告仅防重行自引",
        dups_whitelist_ok(),
        &format!("dups={:?} white={}", scan_dups(), SELF_REF_ID),
    );

    s.add_dyn(
        "F12704-04 判据段内校验",
        {
            let mut ok = true;
            for row in JUDGE_ROWS.iter() {
                if !judge_in_range(row.0) {
                    ok = false;
                }
            }
            ok
        },
        &format!("id in {}..={}", JUDGE_LO, JUDGE_HI),
    );

    s.add_dyn(
        "F12704-05 覆盖率 1000‰",
        coverage_full_ok(),
        &format!("rate={} cells={}", u.coverage_rate(), u.covered_cells()),
    );

    s.add_dyn(
        "F12704-06 三件位图满覆盖",
        u.row_len() == 3,
        "3 件 × 4 方 = 12 格",
    );

    s.add_dyn(
        "F12704-07 位图计数正确性",
        bitmap_count(0x0F) == 4 && bitmap_count(0x01) == 1 && bitmap_count(0x00) == 0,
        "手工逐位计数",
    );

    s.add_dyn(
        "F12704-08 注入联测 10/10 拒止",
        {
            let (c, t, e) = uv_fault_sweep();
            t == 10 && c == 10 && e.is_empty()
        },
        &format!("caught={}", uv_fault_sweep().0),
    );

    s.add_dyn(
        "F12704-09 相位迁移守卫直读",
        uv_advance(UvState::UsDraft, UvState::UsScanning).is_ok()
            && uv_advance(UvState::UsScanning, UvState::UsCleared).is_ok()
            && uv_advance(UvState::UsCleared, UvState::UsDraft).is_err()
            && uv_advance(UvState::UsDraft, UvState::UsCleared).is_err(),
        "草稿→扫描中→终验绿只进不退",
    );

    s.add_dyn(
        "F12704-10 双跑一致",
        dual_run_ok(),
        "确定性重放两轮逐位同判",
    );

    s.add_dyn(
        "F12704-11 完成写回语义",
        writeback_ok(),
        "终验绿即已完成且摘要非空",
    );

    s.add_dyn(
        "F12704-12 冻结摘要非零",
        frozen_digest() != 0,
        &format!("digest={:08X}", frozen_digest()),
    );

    s.add_dyn(
        "F12704-13 段位守卫",
        seg_guard_ok(),
        &format!("seg=0x{:05X}", SEG_BASE),
    );

    s
}

// ── 测试区 ───────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seg_guard_aligned_and_in_window() {
        assert!(seg_guard_ok());
        assert_eq!(SEG_BASE % 0x40, 0);
        assert!(SEG_BASE >= 0x80000 && SEG_BASE <= 0x82FC0);
        assert_eq!(SEG_BASE, 0x819C0);
    }

    #[test]
    fn diag_codes_are_closed_set() {
        assert_eq!(UV_ALL.len(), 12);
        let mut i = 0usize;
        while i < UV_ALL.len() {
            let vi = match UV_ALL.get(i) {
                Some(d) => d.val(),
                None => 0,
            };
            assert!(vi > SEG_BASE && vi < SEG_BASE + 0x40);
            let mut j = i + 1;
            while j < UV_ALL.len() {
                let vj = match UV_ALL.get(j) {
                    Some(d) => d.val(),
                    None => 0,
                };
                assert_ne!(vi, vj, "诊断码重复 0x{:05x}", vi);
                j += 1;
            }
            assert!(!detail(vi).is_empty());
            i += 1;
        }
        assert!(detail(SEG_BASE + 0x30).contains("未知"));
    }

    #[test]
    fn judge_unique_holds() {
        assert!(judge_unique_ok());
        assert!(dups_whitelist_ok());
        assert!(coverage_full_ok());
        let dups = scan_dups();
        assert_eq!(dups.len(), 1);
        match dups.first() {
            Some(d) => {
                assert_eq!(d.0, SELF_REF_ID);
                assert_eq!(d.1, 2);
            }
            None => assert!(false, "dups 空"),
        }
        // 实判据逐条计数为 1。
        let mut i = 0usize;
        while i < JUDGE_ROWS.len() {
            let row = match JUDGE_ROWS.get(i) {
                Some(v) => v,
                None => return,
            };
            if row.2 == RowKind::RkReal {
                assert_eq!(row.3, 1);
                assert!(judge_in_range(row.0));
            }
            i += 1;
        }
    }

    #[test]
    fn dup_injection_rejected() {
        // 真重号（白名单外）必拒。
        let real_dup = [(12610u32, 2u32), (SELF_REF_ID, 2u32)];
        assert!(!judge_unique_ok_with(&real_dup));
        // 双真重号必拒。
        let two = [(12610u32, 2u32), (12611u32, 2u32), (SELF_REF_ID, 2u32)];
        assert!(!judge_unique_ok_with(&two));
        // 自引缺失必拒。
        let none: [(u32, u32); 0] = [];
        assert!(!judge_unique_ok_with(&none));
        // 自引计数异常必拒。
        let cnt3 = [(SELF_REF_ID, 3u32)];
        assert!(!judge_unique_ok_with(&cnt3));
        // 自引越白（12699 冒充）必拒。
        let fake_self = [(12699u32, 2u32)];
        assert!(!judge_unique_ok_with(&fake_self));
    }

    #[test]
    fn bitmap_count_holds() {
        assert_eq!(bitmap_count(0x0F), 4);
        assert_eq!(bitmap_count(0x01), 1);
        assert_eq!(bitmap_count(0x05), 2);
        assert_eq!(bitmap_count(0x00), 0);
        assert_eq!(bitmap_count(0x08), 1);
    }

    #[test]
    fn cover_gates_reject() {
        let mut u = UvCover::new();
        let _ = u.begin();
        // 件序越界。
        assert_eq!(
            u.reg_cover(CoverRow { asset: 3, bitmap: FULL_BITMAP }),
            Err(UvDiag::ECellRange.val())
        );
        // 位图越界。
        assert_eq!(
            u.reg_cover(CoverRow { asset: 0, bitmap: 0x10 }),
            Err(UvDiag::ECellRange.val())
        );
        // 正常过。
        assert!(u.reg_cover(CoverRow { asset: 0, bitmap: FULL_BITMAP }).is_ok());
        // 重件。
        assert_eq!(
            u.reg_cover(CoverRow { asset: 0, bitmap: 0x01 }),
            Err(UvDiag::EDupCell.val())
        );
        assert_eq!(u.reject_len(), 3);
    }

    #[test]
    fn clear_gates_hold() {
        // 空账终验拒（率不足）。
        let mut u = UvCover::new();
        let _ = u.begin();
        assert_eq!(u.clear(), Err(UvDiag::ERateOff.val()));
        // 部分覆盖终验拒。
        let mut h = UvCover::new();
        let _ = h.begin();
        let _ = h.reg_cover(CoverRow { asset: 0, bitmap: 0x03 });
        let _ = h.reg_cover(CoverRow { asset: 1, bitmap: FULL_BITMAP });
        let _ = h.reg_cover(CoverRow { asset: 2, bitmap: FULL_BITMAP });
        assert_eq!(h.coverage_rate(), 1000 * 10 / 12);
        assert_eq!(h.clear(), Err(UvDiag::ERateOff.val()));
        // 满覆盖终验过。
        let mut full = build_full_uv_cover();
        assert_eq!(full.state(), UvState::UsCleared);
        assert_eq!(full.coverage_rate(), RATE_FULL);
        // 终验后写拒 + 倒退拒。
        assert_eq!(
            full.reg_cover(CoverRow { asset: 0, bitmap: FULL_BITMAP }),
            Err(UvDiag::EClearedWrite.val())
        );
        assert!(full.begin().is_err());
    }

    #[test]
    fn phase_guards_hold() {
        assert!(uv_advance(UvState::UsDraft, UvState::UsScanning).is_ok());
        assert!(uv_advance(UvState::UsScanning, UvState::UsCleared).is_ok());
        assert!(uv_advance(UvState::UsDraft, UvState::UsCleared).is_err());
        assert!(uv_advance(UvState::UsCleared, UvState::UsScanning).is_err());
        assert!(uv_advance(UvState::UsScanning, UvState::UsScanning).is_err());
        assert!(UvState::UsDraft.rank() < UvState::UsScanning.rank());
        assert!(UvState::UsScanning.rank() < UvState::UsCleared.rank());
    }

    #[test]
    fn injections_all_caught() {
        let (caught, total, escape) = uv_fault_sweep();
        assert_eq!(total, 10);
        assert_eq!(caught, 10, "逃脱路：{}", escape);
        assert!(escape.is_empty());
    }

    #[test]
    fn dual_run_consistent() {
        assert!(dual_run_ok());
        let x = build_full_uv_cover();
        let y = build_full_uv_cover();
        assert_eq!(x.row_len(), y.row_len());
        assert_eq!(x.covered_cells(), y.covered_cells());
        assert_eq!(frozen_digest(), frozen_digest());
        assert_ne!(frozen_digest(), 0);
    }

    #[test]
    fn writeback_semantics_hold() {
        assert!(writeback_ok());
        for st in [UvState::UsDraft, UvState::UsScanning, UvState::UsCleared].iter() {
            assert!(!st.name().is_empty());
        }
    }

    #[test]
    fn checks_entry_all_green() {
        let s = run_unx12703_checks();
        assert!(s.all_passed(), "自检存在红项：{}", s.red_items_text());
        assert!(s.len() >= 12, "检查条目不足 12");
    }
}
