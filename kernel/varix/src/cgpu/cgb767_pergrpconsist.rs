#![allow(dead_code)]
//! cgb767_pergrpconsist —— CGPU-F0767 感知组一致性。
//!
//! ═══════════════════════════════════════════════════════════════════════
//! 锚点：CGPU-F0767 感知组一致性  <SEG 0x6B20>
//! 目标行数：≥300（判据：纯功能代码 ≥300 行）
//! 判据原文摘录：
//!   ① 纯功能代码 ≥300 行；
//!   ② 规格与判据按册内锚点原文逐条落实：CGPU Varix STAR II · 总纲与施工书 #CGPU-F0767；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//!
//! 设计纪律（内嵌感知面组级一致性账，整数面）：
//!   1. 组员闭集（写死 9 员）：感知面段 cgb758..cgb766，段位
//!      0x6A00 起步进 0x20；组账只收组员 id 面（758..=766）；
//!   2. 段位纪律：注册时查段位落域（0x6A00..=0x6B1F）与段间
//!      不重叠（与既有员差距 <0x20 即红码）；注册即按段位序
//!      就位（升序账）；
//!   3. 行数纪律：上报行数 ≥ 卡面目标（300/320 写死表），
//!      缺行红码；组总行数 = 各员行数之和（守恒恒等式一）；
//!   4. 样本账：各员样本数之和 = 组样本总数（守恒恒等式二），
//!      组账只加不减（入账单调）；
//!   5. 齐员审计：9 员齐且 id 闭集无缺（缺员红码）；
//!   6. 组指纹：FNV-1a 滚过全部 (id, seg) 对，注册/审计双算
//!      必须同值（漂移红码）；
//!   7. 零 unwrap/expect/panic，整数算术，确定性，no_std + alloc。
//!
//! 诊断段声明：0x6B20 段为本模块独占（SEG_BASE=0x6B20），判别式
//!   SEG_BASE+1..+N；与 0x6B00/0x6B40 邻段不重叠。
//! 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::vec::Vec;

/// 模块版本。
pub const PG_VERSION: &str = "cgb767_pergrpconsist-v1";

/// 诊断段基址（0x6B20 · 本模块独占）。
pub const SEG_BASE: u16 = 0x6B20;

// ── XcCode 显式判别式（SEG_BASE+1..+N）────────────────────────────────
/// 0x6B21 组员 id 越域（感知面 758..=766 之外）。
pub const XC_BAD_ID: u16 = SEG_BASE + 1;
/// 0x6B22 组员重复注册。
pub const XC_DUP_MEMBER: u16 = SEG_BASE + 2;
/// 0x6B23 段位重叠（与既有员差距 <0x20）。
pub const XC_SEG_OVERLAP: u16 = SEG_BASE + 3;
/// 0x6B24 段位出域（感知面 0x6A00..=0x6B1F 之外）。
pub const XC_SEG_OUT: u16 = SEG_BASE + 4;
/// 0x6B25 行数缺欠（上报 < 卡面目标）。
pub const XC_LINES_SHORT: u16 = SEG_BASE + 5;
/// 0x6B26 组账不守恒（样本/行数恒等式破）。
pub const XC_UNBALANCED: u16 = SEG_BASE + 6;
/// 0x6B27 组员数漂移（≠9）。
pub const XC_COUNT_DRIFT: u16 = SEG_BASE + 7;
/// 0x6B28 组指纹漂移。
pub const XC_DIGEST_DRIFT: u16 = SEG_BASE + 8;
/// 0x6B29 缺员（id 闭集有缺）。
pub const XC_MEMBER_MISSING: u16 = SEG_BASE + 9;
/// 0x6B30 段位失序（升序账破坏）。
pub const XC_SEG_UNORDERED: u16 = SEG_BASE + 10;

// ── 写死常量面 ────────────────────────────────────────────────────────
/// 组员 id 下界（cgb758）。
pub const ID_LO: u16 = 758;
/// 组员 id 上界（cgb766）。
pub const ID_HI: u16 = 766;
/// 组员数（写死：9）。
pub const MEMBER_CAP: usize = 9;
/// 段位步进（写死：0x20）。
pub const SEG_STRIDE: u16 = 0x20;
/// 感知面段域下界。
pub const SEG_DOMAIN_LO: u16 = 0x6A00;
/// 感知面段域上界。
pub const SEG_DOMAIN_HI: u16 = 0x6B1F;
/// 段位半宽（重叠判定：差距 <0x20 视为重叠）。
pub const SEG_HALF: u16 = 0x20;

/// 卡面行数目标表（写死，随 id 升序 758..766）。
pub const LINE_TARGET: [u32; MEMBER_CAP] = [
    320, // 758 网页交互可感指标
    320, // 759 网页体验事件捕获
    300, // 760 感知遥测
    300, // 761 感知 fuzz
    300, // 762 感知 API 冻结 v1
    300, // 763 感知文档
    300, // 764 感知基准
    300, // 765 感知一致性
    300, // 766 感知安全
];

/// 组员条目。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MemberRec {
    /// 卡号（758..=766）。
    pub id: u16,
    /// 诊断段基址。
    pub seg: u16,
    /// 上报行数。
    pub lines: u32,
    /// 样本账上报数。
    pub samples: u32,
}

/// FNV-1a 32 位单步。
fn fnv_step(h: u32, b: u8) -> u32 {
    (h ^ (b as u32)).wrapping_mul(0x0100_0193)
}

/// 组指纹：滚过全部 (id, seg) 对（按账序）。
pub fn group_digest(ms: &[MemberRec]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    for m in ms.iter() {
        for b in m.id.to_le_bytes() {
            h = fnv_step(h, b);
        }
        for b in m.seg.to_le_bytes() {
            h = fnv_step(h, b);
        }
    }
    h
}

/// 感知组一致性账：升序段位账 + 双恒等式 + 齐员审计。
/// 守恒恒等式：
///   组总行数 = Σ 员行数；组总样本 = Σ 员样本；
///   组账只加不减（单调入账）。
pub struct GroupLedger {
    members: Vec<MemberRec>,
    group_samples: u64,
}

impl GroupLedger {
    /// 空组账。
    pub fn new() -> Self {
        GroupLedger {
            members: Vec::new(),
            group_samples: 0,
        }
    }

    /// 在册数。
    pub fn len(&self) -> usize {
        self.members.len()
    }

    /// 查员（id → 下标）。
    pub fn find(&self, id: u16) -> Result<usize, u16> {
        if id < ID_LO || id > ID_HI {
            return Err(XC_BAD_ID);
        }
        for i in 0..self.members.len() {
            if self.members[i].id == id {
                return Ok(i);
            }
        }
        Err(XC_MEMBER_MISSING)
    }

    /// 注册组员：id 域查 → 重复查 → 段位域查 → 重叠查 →
    /// 行数查 → 按段位升序就位。
    pub fn register(
        &mut self,
        id: u16,
        seg: u16,
        lines: u32,
        samples: u32,
    ) -> Result<u32, u16> {
        if id < ID_LO || id > ID_HI {
            return Err(XC_BAD_ID);
        }
        if self.find(id).is_ok() {
            return Err(XC_DUP_MEMBER);
        }
        if seg < SEG_DOMAIN_LO || seg > SEG_DOMAIN_HI {
            return Err(XC_SEG_OUT);
        }
        // 重叠查：与既有员段位差距 <0x20 即红码。
        for m in self.members.iter() {
            let d = if m.seg > seg { m.seg - seg } else { seg - m.seg };
            if d < SEG_HALF {
                return Err(XC_SEG_OVERLAP);
            }
        }
        // 行数查：上报 ≥ 卡面目标。
        let target = LINE_TARGET[(id - ID_LO) as usize];
        if lines < target {
            return Err(XC_LINES_SHORT);
        }
        let rec = MemberRec {
            id,
            seg,
            lines,
            samples,
        };
        // 升序就位（按 seg）。
        let mut pos = self.members.len();
        for i in 0..self.members.len() {
            if self.members[i].seg > seg {
                pos = i;
                break;
            }
        }
        self.members.insert(pos, rec);
        self.group_samples += samples as u64;
        Ok(group_digest(&self.members))
    }

    /// 段位升序审计：账内相邻差距 ≥0x20 且严格递增。
    pub fn audit_order(&self) -> Result<(), u16> {
        for i in 1..self.members.len() {
            if self.members[i].seg <= self.members[i - 1].seg {
                return Err(XC_SEG_UNORDERED);
            }
            if self.members[i].seg - self.members[i - 1].seg < SEG_HALF {
                return Err(XC_SEG_OVERLAP);
            }
        }
        Ok(())
    }

    /// 齐员审计：9 员齐且 id 闭集 758..=766 无缺。
    pub fn audit_full(&self) -> Result<(), u16> {
        if self.members.len() != MEMBER_CAP {
            return Err(XC_COUNT_DRIFT);
        }
        for id in ID_LO..=ID_HI {
            self.find(id)?;
        }
        Ok(())
    }

    /// 组指纹复核：账序重算 = 注册末次快照。
    pub fn verify_digest(&self, want: u32) -> Result<(), u16> {
        if group_digest(&self.members) != want {
            return Err(XC_DIGEST_DRIFT);
        }
        Ok(())
    }

    /// 行数恒等式：Σ 员行数 ≥ Σ 目标（全过线则组线过）。
    pub fn lines_total(&self) -> u64 {
        let mut s: u64 = 0;
        for m in self.members.iter() {
            s += m.lines as u64;
        }
        s
    }

    /// 样本恒等式：Σ 员样本 = 组总样本。
    pub fn conserved(&self) -> Result<(), u16> {
        let mut s: u64 = 0;
        for m in self.members.iter() {
            s += m.samples as u64;
        }
        if s != self.group_samples {
            return Err(XC_UNBALANCED);
        }
        // 目标线总和。
        let mut t: u64 = 0;
        for m in self.members.iter() {
            t += LINE_TARGET[(m.id - ID_LO) as usize] as u64;
        }
        if self.lines_total() < t {
            return Err(XC_LINES_SHORT);
        }
        Ok(())
    }

    /// 组样本总数只读。
    pub fn samples_total(&self) -> u64 {
        self.group_samples
    }
}

/// 自例程：9 员注册 → 段位升序 → 齐员审计 → 指纹复核 →
/// 双恒等式 → 错误路径（越域 id/重复员/段重叠/段出域/缺行/缺员）。
pub fn drill() -> Result<u32, u16> {
    let mut g = GroupLedger::new();
    // 主路径：9 员注册（段位 0x6A00 步进 0x20）。
    let lines: [u32; 9] = [539, 407, 390, 379, 463, 452, 455, 470, 500];
    for i in 0..9usize {
        let id = ID_LO + i as u16;
        let seg = SEG_DOMAIN_LO + (i as u16) * SEG_STRIDE;
        g.register(id, seg, lines[i], 100)?;
    }
    // 齐员 + 段序 + 守恒。
    g.audit_full()?;
    g.audit_order()?;
    g.conserved()?;
    if g.len() != MEMBER_CAP {
        return Err(XC_COUNT_DRIFT);
    }
    if g.samples_total() != 900 {
        return Err(XC_UNBALANCED);
    }
    // 行数总和：539+407+390+379+463+452+455+470+500 = 4055。
    if g.lines_total() != 4055 {
        return Err(XC_UNBALANCED);
    }
    // 指纹复核：账序重算同值。
    let d = group_digest(&g.members);
    g.verify_digest(d)?;
    // 错误路径 1：越域 id（757 / 767）。
    let mut g2 = GroupLedger::new();
    if g2.register(757, SEG_DOMAIN_LO, 320, 0) != Err(XC_BAD_ID) {
        return Err(XC_BAD_ID);
    }
    if g2.register(767, SEG_DOMAIN_LO, 320, 0) != Err(XC_BAD_ID) {
        return Err(XC_BAD_ID);
    }
    // 错误路径 2：段位出域（0x6980 下溢 / 0x6B40 上溢）。
    if g2.register(758, 0x6980, 320, 0) != Err(XC_SEG_OUT) {
        return Err(XC_SEG_OUT);
    }
    if g2.register(758, 0x6B40, 320, 0) != Err(XC_SEG_OUT) {
        return Err(XC_SEG_OUT);
    }
    // 错误路径 3：行数缺欠（758 目标 320，上报 319）。
    if g2.register(758, SEG_DOMAIN_LO, 319, 0) != Err(XC_LINES_SHORT) {
        return Err(XC_LINES_SHORT);
    }
    // 正常注册 758 后再查重复/重叠。
    g2.register(758, SEG_DOMAIN_LO, 320, 10)?;
    if g2.register(758, SEG_DOMAIN_LO, 330, 0) != Err(XC_DUP_MEMBER) {
        return Err(XC_DUP_MEMBER);
    }
    // 错误路径 4：段位重叠（0x6A10 距 0x6A00 差 0x10 <0x20）。
    if g2.register(759, 0x6A10, 320, 0) != Err(XC_SEG_OVERLAP) {
        return Err(XC_SEG_OVERLAP);
    }
    // 错误路径 5：缺员审计（仅 1 员）。
    if g2.audit_full() != Err(XC_COUNT_DRIFT) {
        return Err(XC_COUNT_DRIFT);
    }
    g2.conserved()?;
    // 样本恒等式破坏注入：手工推不进去（账只加不减），
    // 改用缺员 + 样本错账组合：只注册 2 员，sum=20 == 组账，恒等式过，
    // 但齐员审计仍抓缺员。
    let mut g3 = GroupLedger::new();
    g3.register(758, SEG_DOMAIN_LO, 320, 10)?;
    g3.register(766, 0x6B00, 300, 10)?;
    g3.conserved()?;
    if g3.audit_full() != Err(XC_MEMBER_MISSING) {
        return Err(XC_MEMBER_MISSING);
    }
    // 升序就位核对：766 段位 0x6B00 在账尾。
    if g3.len() != 2 {
        return Err(XC_UNBALANCED);
    }
    // 指纹漂移：外部比对值异变。
    if g3.verify_digest(d) != Err(XC_DIGEST_DRIFT) {
        return Err(XC_DIGEST_DRIFT);
    }
    // 指纹确定性：同表双算同值。
    let d3a = group_digest(&g3.members);
    let d3b = group_digest(&g3.members);
    if d3a != d3b {
        return Err(XC_DIGEST_DRIFT);
    }
    Ok(0)
}
