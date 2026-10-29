#![allow(dead_code)]
//! cgb768_e08close —— CGPU-F0768 E08 组收口与 E09 移交。
//!
//! ═══════════════════════════════════════════════════════════════════════
//! 锚点：CGPU-F0768 E08 组收口与 E09 移交  <SEG 0x6B40>
//! 目标行数：≥320（判据：纯功能代码 ≥320 行）
//! 判据原文摘录：
//!   ① 纯功能代码 ≥320 行；
//!   ② 规格与判据按册内锚点原文逐条落实：CGPU Varix STAR II · 总纲与施工书 #CGPU-F0768；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//!
//! 设计纪律（内嵌 E08 收口账与 E09 移交面，整数面）：
//!   1. 收口清单闭集八项（写死语义）：文档冻结 / 基准绿灯 /
//!      一致性绿灯 / 安全绿灯 / 遥测绿灯 / fuzz 清白 / API 冻结 /
//!      账本守恒；状态 Pending → Done 单向；
//!   2. 残留风险账：定容 6 槽，风险严重度 1..=3（越界红码），
//!      未确认（open）风险阻塞封印；确认（ack）单向；
//!   3. 封印状态机：Open → Sealed 单向；封印前置条件：八项全
//!      Done 且风险全 ack；封印后禁再动清单与风险；
//!   4. 移交指纹：FNV-1a 滚过清单位型 + 风险确认位型，封印时
//!      定格，复核漂移红码；
//!   5. 守恒恒等式：清单数 = Pending + Done = 8；
//!      风险数 = open + acked；
//!   6. 零 unwrap/expect/panic，整数算术，确定性，no_std + alloc。
//!
//! 诊断段声明：0x6B40 段为本模块独占（SEG_BASE=0x6B40），判别式
//!   SEG_BASE+1..+N；与 0x6B20/0x6B60 邻段不重叠。
//! 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::vec::Vec;

/// 模块版本。
pub const EC_VERSION: &str = "cgb768_e08close-v1";

/// 诊断段基址（0x6B40 · 本模块独占）。
pub const SEG_BASE: u16 = 0x6B40;

// ── XcCode 显式判别式（SEG_BASE+1..+N）────────────────────────────────
/// 0x6B41 清单项号越界（0..=7 之外）。
pub const XC_BAD_ITEM: u16 = SEG_BASE + 1;
/// 0x6B42 清单项重复完成。
pub const XC_DUP_DONE: u16 = SEG_BASE + 2;
/// 0x6B43 收口前置未齐（有 Pending 项）。
pub const XC_GATE_PENDING: u16 = SEG_BASE + 3;
/// 0x6B44 有未确认风险。
pub const XC_RISK_OPEN: u16 = SEG_BASE + 4;
/// 0x6B45 已封印仍改动。
pub const XC_SEALED: u16 = SEG_BASE + 5;
/// 0x6B46 移交指纹漂移。
pub const XC_DIGEST_DRIFT: u16 = SEG_BASE + 6;
/// 0x6B47 账目不守恒。
pub const XC_UNBALANCED: u16 = SEG_BASE + 7;
/// 0x6B48 风险严重度越界（1..=3 之外）。
pub const XC_BAD_SEV: u16 = SEG_BASE + 8;
/// 0x6B49 风险账满（6 槽）。
pub const XC_RISK_FULL: u16 = SEG_BASE + 9;
/// 0x6B4A 风险号越界或未登记。
pub const XC_BAD_RISK: u16 = SEG_BASE + 10;

// ── 写死常量面 ────────────────────────────────────────────────────────
/// 收口清单闭集：0=文档冻结 1=基准绿灯 2=一致性绿灯 3=安全绿灯
/// 4=遥测绿灯 5=fuzz 清白 6=API 冻结 7=账本守恒。
pub const I_DOCS: usize = 0;
pub const I_BENCH: usize = 1;
pub const I_CONSIST: usize = 2;
pub const I_SEC: usize = 3;
pub const I_TELEM: usize = 4;
pub const I_FUZZ: usize = 5;
pub const I_API: usize = 6;
pub const I_LEDGER: usize = 7;
/// 清单项数（写死：8）。
pub const ITEM_CAP: usize = 8;
/// 风险账容量（写死：6）。
pub const RISK_CAP: usize = 6;
/// 风险严重度上限（写死：3）。
pub const SEV_MAX: u8 = 3;

/// 清单项状态机。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ItemState {
    /// 待收口。
    Pending,
    /// 已收口。
    Done,
}

/// 残留风险条目。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RiskRec {
    /// 风险号（>0）。
    pub id: u8,
    /// 严重度（1..=3）。
    pub sev: u8,
    /// 已确认与否。
    pub acked: bool,
}

/// FNV-1a 32 位单步。
fn fnv_step(h: u32, b: u8) -> u32 {
    (h ^ (b as u32)).wrapping_mul(0x0100_0193)
}

/// E08 收口账：清单 + 风险 + 封印 + 移交指纹。
/// 守恒恒等式：清单八项位型恒全；风险数 = open + acked。
pub struct CloseLedger {
    items: [ItemState; ITEM_CAP],
    done_cnt: u32,
    risks: Vec<RiskRec>,
    ack_cnt: u32,
    sealed: bool,
    digest: u32,
}

impl CloseLedger {
    /// 空账（全 Pending，无风险，未封印）。
    pub fn new() -> Self {
        CloseLedger {
            items: [ItemState::Pending; ITEM_CAP],
            done_cnt: 0,
            risks: Vec::new(),
            ack_cnt: 0,
            sealed: false,
            digest: 0,
        }
    }

    /// 清单项收口：Pending → Done；封印后拒。
    pub fn done_item(&mut self, id: usize) -> Result<(), u16> {
        if self.sealed {
            return Err(XC_SEALED);
        }
        if id >= ITEM_CAP {
            return Err(XC_BAD_ITEM);
        }
        if self.items[id] == ItemState::Done {
            return Err(XC_DUP_DONE);
        }
        self.items[id] = ItemState::Done;
        self.done_cnt += 1;
        Ok(())
    }

    /// 清单项位型（bit i = 1 表示 Done）。
    pub fn item_bits(&self) -> u8 {
        let mut b = 0u8;
        for i in 0..ITEM_CAP {
            if self.items[i] == ItemState::Done {
                b |= 1 << i;
            }
        }
        b
    }

    /// 全 Done 判定。
    pub fn all_done(&self) -> bool {
        self.done_cnt == ITEM_CAP as u32
    }

    /// 风险登记：id 去重、严重度 1..=3、定容 6。
    pub fn add_risk(&mut self, id: u8, sev: u8) -> Result<(), u16> {
        if self.sealed {
            return Err(XC_SEALED);
        }
        if id == 0 {
            return Err(XC_BAD_RISK);
        }
        if sev == 0 || sev > SEV_MAX {
            return Err(XC_BAD_SEV);
        }
        for r in self.risks.iter() {
            if r.id == id {
                return Err(XC_BAD_RISK);
            }
        }
        if self.risks.len() >= RISK_CAP {
            return Err(XC_RISK_FULL);
        }
        self.risks.push(RiskRec {
            id,
            sev,
            acked: false,
        });
        Ok(())
    }

    /// 风险确认：open → acked（单向）。
    pub fn ack_risk(&mut self, id: u8) -> Result<(), u16> {
        if self.sealed {
            return Err(XC_SEALED);
        }
        for r in self.risks.iter_mut() {
            if r.id == id {
                if r.acked {
                    return Err(XC_BAD_RISK);
                }
                r.acked = true;
                self.ack_cnt += 1;
                return Ok(());
            }
        }
        Err(XC_BAD_RISK)
    }

    /// 有未确认风险判定。
    pub fn any_open(&self) -> bool {
        self.ack_cnt != self.risks.len() as u32
    }

    /// 移交指纹：清单位型 + 风险确认位型滚 FNV。
    pub fn compute_digest(&self) -> u32 {
        let mut h: u32 = 0x811C_9DC5;
        h = fnv_step(h, self.item_bits());
        let mut rb = 0u8;
        for (i, r) in self.risks.iter().enumerate() {
            if r.acked {
                rb |= 1 << i;
            }
        }
        h = fnv_step(h, rb);
        h = fnv_step(h, self.risks.len() as u8);
        h
    }

    /// 封印：前置（全 Done 且风险全 ack）不齐红码；定格指纹。
    pub fn seal(&mut self) -> Result<u32, u16> {
        if self.sealed {
            return Err(XC_SEALED);
        }
        if !self.all_done() {
            return Err(XC_GATE_PENDING);
        }
        if self.any_open() {
            return Err(XC_RISK_OPEN);
        }
        self.sealed = true;
        self.digest = self.compute_digest();
        Ok(self.digest)
    }

    /// 指纹复核。
    pub fn verify_digest(&self, want: u32) -> Result<(), u16> {
        if !self.sealed {
            return Err(XC_SEALED);
        }
        if self.digest != want || self.digest != self.compute_digest() {
            return Err(XC_DIGEST_DRIFT);
        }
        Ok(())
    }

    /// 守恒核：done 计数与位型一致；风险 ack 计数一致。
    pub fn conserved(&self) -> Result<(), u16> {
        let bits = self.item_bits().count_ones();
        if bits as u32 != self.done_cnt {
            return Err(XC_UNBALANCED);
        }
        let mut acked = 0u32;
        for r in self.risks.iter() {
            if r.acked {
                acked += 1;
            }
        }
        if acked != self.ack_cnt {
            return Err(XC_UNBALANCED);
        }
        Ok(())
    }

    /// 收口摘要位型：完成数(0..4) | 封印位(1<<4) | 风险数(5..8)。
    pub fn summary(&self) -> u32 {
        (self.done_cnt & 0xF)
            | if self.sealed { 1 << 4 } else { 0 }
            | ((self.risks.len() as u32 & 0x7) << 5)
    }
}

/// 自例程：清单逐项收口 → 风险登记与确认 → 封印前置拦截 →
/// 封印定格 → 指纹复核 → 守恒 → 错误路径（封印后改动/坏严重度/
/// 重复完成/缺项封印/开风险封印）。
pub fn drill() -> Result<u32, u16> {
    let mut g = CloseLedger::new();
    // 错误路径 1：缺项封印。
    if g.seal() != Err(XC_GATE_PENDING) {
        return Err(XC_GATE_PENDING);
    }
    // 错误路径 2：坏项号 / 重复完成。
    if g.done_item(8) != Err(XC_BAD_ITEM) {
        return Err(XC_BAD_ITEM);
    }
    g.done_item(I_DOCS)?;
    if g.done_item(I_DOCS) != Err(XC_DUP_DONE) {
        return Err(XC_DUP_DONE);
    }
    // 风险登记与确认。
    g.add_risk(1, 2)?;
    g.add_risk(2, 1)?;
    // 错误路径 3：坏严重度（0 / 4）/ 重复风险号。
    if g.add_risk(3, 0) != Err(XC_BAD_SEV) {
        return Err(XC_BAD_SEV);
    }
    if g.add_risk(3, 4) != Err(XC_BAD_SEV) {
        return Err(XC_BAD_SEV);
    }
    if g.add_risk(1, 3) != Err(XC_BAD_RISK) {
        return Err(XC_BAD_RISK);
    }
    // 错误路径 4：有开风险时补齐清单 → 封印仍拦。
    for i in 1..ITEM_CAP {
        g.done_item(i)?;
    }
    if g.seal() != Err(XC_RISK_OPEN) {
        return Err(XC_RISK_OPEN);
    }
    g.ack_risk(1)?;
    g.ack_risk(2)?;
    // 错误路径 5：重复确认。
    if g.ack_risk(1) != Err(XC_BAD_RISK) {
        return Err(XC_BAD_RISK);
    }
    // 封印 + 指纹。
    let d = g.seal()?;
    g.verify_digest(d)?;
    if g.compute_digest() != d {
        return Err(XC_DIGEST_DRIFT);
    }
    g.conserved()?;
    // 摘要：8 完成 | 封印位 | 2 风险。
    let s = g.summary();
    if s & 0xF != 8 {
        return Err(XC_UNBALANCED);
    }
    if s & (1 << 4) == 0 {
        return Err(XC_UNBALANCED);
    }
    if (s >> 5) & 0x7 != 2 {
        return Err(XC_UNBALANCED);
    }
    // 错误路径 6：封印后改动（清单/风险/确认/再封印）。
    if g.done_item(I_DOCS) != Err(XC_SEALED) {
        return Err(XC_SEALED);
    }
    if g.add_risk(3, 1) != Err(XC_SEALED) {
        return Err(XC_SEALED);
    }
    if g.ack_risk(1) != Err(XC_SEALED) {
        return Err(XC_SEALED);
    }
    if g.seal() != Err(XC_SEALED) {
        return Err(XC_SEALED);
    }
    // 指纹漂移：外部异值比对。
    if g.verify_digest(d ^ 1) != Err(XC_DIGEST_DRIFT) {
        return Err(XC_DIGEST_DRIFT);
    }
    // 风险账满：6 槽。
    let mut g2 = CloseLedger::new();
    for i in 1..=6u8 {
        g2.add_risk(i, 1)?;
    }
    if g2.add_risk(7, 1) != Err(XC_RISK_FULL) {
        return Err(XC_RISK_FULL);
    }
    g2.conserved()?;
    // 全 ack 后封印面（无清单完成仍拦前置）。
    for i in 1..=6u8 {
        g2.ack_risk(i)?;
    }
    if g2.any_open() {
        return Err(XC_UNBALANCED);
    }
    if g2.seal() != Err(XC_GATE_PENDING) {
        return Err(XC_GATE_PENDING);
    }
    Ok(0)
}
