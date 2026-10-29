//! unx4148_b2b5pact — UNX-F4148 B2–B5 消费方契约书（冻结件交付清单）（诊断段 0x9440）。
//!
//! 判据原文：
//!   J1 契约书四件（表定义/最小集/缺省矩阵/变更流程）齐备可导出。
//!   J2 B2–B5 四域消费确认锚位登记，四个确认位占位可查。
//!   J3 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计纪律：
//!   1. 契约书四件以嵌入式只读清单交付，导出 API 逐件出账，缺一件判据失守。
//!   2. 确认锚位状态机 Empty → Registered → Confirmed，一域一位，重复登记拒收。
//!   3. 写回账强校验：状态字必须为已完成、结果摘要长度必须非零，空摘要拒收。
//!   4. 守恒对账：锚位账 + 写回账 + 清单账三账互核，缺项即对账失败。
//!   5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。

use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0x9440;

// ── 诊断码闭集 ───────────────────────────────────────────────
const E_DUP_ANCHOR: u16 = SEG_BASE + 1; // 同域锚位重复登记
const E_NO_ANCHOR: u16 = SEG_BASE + 2;  // 未登记先确认
const E_BAD_STATE: u16 = SEG_BASE + 3;  // 状态机非法迁移
const E_EMPTY_SUM: u16 = SEG_BASE + 4;  // 写回结果摘要为空
const E_BAD_STATUS: u16 = SEG_BASE + 5; // 写回状态字非已完成
const E_PACT_MISS: u16 = SEG_BASE + 6;  // 契约书四件缺件
const E_ANCHOR_MISS: u16 = SEG_BASE + 7;// 确认锚位缺位
const E_BAD_DOMAIN: u16 = SEG_BASE + 8; // 域号越界

/// 契约书四件清单编号（固定序，不得增删）。
pub const ARTIFACT_TABLE_DEF: usize = 0;     // 表定义
pub const ARTIFACT_MIN_SET: usize = 1;       // 最小集
pub const ARTIFACT_DEF_MATRIX: usize = 2;    // 缺省矩阵
pub const ARTIFACT_CHANGE_FLOW: usize = 3;   // 变更流程
pub const ARTIFACT_COUNT: usize = 4;

/// 消费方四域（B2–B5），域号即锚位下标。
pub const DOMAIN_B2: u8 = 0;
pub const DOMAIN_B3: u8 = 1;
pub const DOMAIN_B4: u8 = 2;
pub const DOMAIN_B5: u8 = 3;
pub const DOMAIN_COUNT: usize = 4;

/// 锚位状态机：Empty 未占位 / Registered 已登记 / Confirmed 已确认。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AnchorState {
    Empty,
    Registered,
    Confirmed,
}

/// 单域消费确认锚位。
pub struct ConsumerAnchor {
    pub domain: u8,          // 域号 0..3
    pub state: AnchorState,  // 锚位状态
    pub confirm_epoch: u32,  // 确认代次（确认一次加一）
}

/// 一次 API/软件写回记录。
pub struct WritebackRecord {
    pub status_done: bool,   // 状态=已完成
    pub summary_len: u32,    // 结果摘要长度（非零强校验）
}

/// 契约书枢纽：四件清单 + 四域锚位 + 写回账。
pub struct PactBook {
    artifacts: [bool; ARTIFACT_COUNT], // 四件齐备位
    pub anchors: [ConsumerAnchor; DOMAIN_COUNT],
    pub writebacks: Vec<WritebackRecord>,
    pub registered_total: u32, // 登记成功计数
    pub confirmed_total: u32,  // 确认成功计数
}

impl PactBook {
    pub fn new() -> PactBook {
        PactBook {
            artifacts: [false; ARTIFACT_COUNT],
            anchors: [
                ConsumerAnchor { domain: DOMAIN_B2, state: AnchorState::Empty, confirm_epoch: 0 },
                ConsumerAnchor { domain: DOMAIN_B3, state: AnchorState::Empty, confirm_epoch: 0 },
                ConsumerAnchor { domain: DOMAIN_B4, state: AnchorState::Empty, confirm_epoch: 0 },
                ConsumerAnchor { domain: DOMAIN_B5, state: AnchorState::Empty, confirm_epoch: 0 },
            ],
            writebacks: Vec::new(),
            registered_total: 0,
            confirmed_total: 0,
        }
    }

    /// 交付契约书单件（kind 越界拒收）。
    pub fn deliver_artifact(&mut self, kind: usize) -> Result<(), u16> {
        if kind >= ARTIFACT_COUNT {
            return Err(E_PACT_MISS);
        }
        self.artifacts[kind] = true;
        Ok(())
    }

    /// 四件一次齐交（判据 J1 齐备动作）。
    pub fn deliver_all(&mut self) -> Result<(), u16> {
        for k in 0..ARTIFACT_COUNT {
            self.artifacts[k] = true;
        }
        Ok(())
    }

    /// 导出四件清单：齐备时逐件返回编号，缺件返回错误（判据 J1 可导出）。
    pub fn export_artifacts(&self) -> Result<Vec<usize>, u16> {
        let mut out = Vec::new();
        for k in 0..ARTIFACT_COUNT {
            if !self.artifacts[k] {
                return Err(E_PACT_MISS);
            }
            out.push(k);
        }
        Ok(out)
    }

    fn anchor_idx(&self, domain: u8) -> Result<usize, u16> {
        if (domain as usize) >= DOMAIN_COUNT {
            return Err(E_BAD_DOMAIN);
        }
        Ok(domain as usize)
    }

    /// 登记锚位：Empty → Registered，重复登记拒收（判据 J2）。
    pub fn register_anchor(&mut self, domain: u8) -> Result<(), u16> {
        let i = self.anchor_idx(domain)?;
        match self.anchors[i].state {
            AnchorState::Empty => {
                self.anchors[i].state = AnchorState::Registered;
                self.registered_total += 1;
                Ok(())
            }
            AnchorState::Registered | AnchorState::Confirmed => Err(E_DUP_ANCHOR),
        }
    }

    /// 确认锚位：Registered → Confirmed，未登记先确认拒收。
    pub fn confirm_anchor(&mut self, domain: u8) -> Result<(), u16> {
        let i = self.anchor_idx(domain)?;
        match self.anchors[i].state {
            AnchorState::Empty => Err(E_NO_ANCHOR),
            AnchorState::Registered => {
                self.anchors[i].state = AnchorState::Confirmed;
                self.anchors[i].confirm_epoch += 1;
                self.confirmed_total += 1;
                Ok(())
            }
            AnchorState::Confirmed => Err(E_BAD_STATE),
        }
    }

    /// 查询锚位占位可查接口（判据 J2「占位可查」）。
    pub fn anchor_state(&self, domain: u8) -> Result<AnchorState, u16> {
        let i = self.anchor_idx(domain)?;
        Ok(self.anchors[i].state)
    }

    /// API/软件写回：状态=已完成且摘要非空（判据 J3）。
    pub fn writeback(&mut self, status_done: bool, summary_len: u32) -> Result<(), u16> {
        if !status_done {
            return Err(E_BAD_STATUS);
        }
        if summary_len == 0 {
            return Err(E_EMPTY_SUM);
        }
        self.writebacks.push(WritebackRecord { status_done, summary_len });
        Ok(())
    }

    /// 守恒对账：四件齐 + 四锚位 Confirmed + 写回账非空且全合格。
    pub fn audit(&self) -> Result<(), u16> {
        for k in 0..ARTIFACT_COUNT {
            if !self.artifacts[k] {
                return Err(E_PACT_MISS);
            }
        }
        for i in 0..DOMAIN_COUNT {
            if self.anchors[i].state != AnchorState::Confirmed {
                return Err(E_ANCHOR_MISS);
            }
        }
        if self.writebacks.is_empty() {
            return Err(E_EMPTY_SUM);
        }
        for w in &self.writebacks {
            if !w.status_done || w.summary_len == 0 {
                return Err(E_BAD_STATUS);
            }
        }
        // 守恒：登记数 ≥ 确认数（确认前必有登记）。
        if self.confirmed_total > self.registered_total {
            return Err(E_NO_ANCHOR);
        }
        Ok(())
    }
}

/// 自例程：R1 正常链 → R2 边界 → R3 注入拦截 → R4 守恒对账。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;

    // R1 正常路径：四件齐交 → 四域登记并确认 → 写回一条 → 对账过。
    let mut b = PactBook::new();
    b.deliver_all()?;
    let exported = b.export_artifacts()?;
    if exported.len() != ARTIFACT_COUNT {
        return Err(E_PACT_MISS);
    }
    for d in 0..DOMAIN_COUNT as u8 {
        b.register_anchor(d)?;
    }
    for d in 0..DOMAIN_COUNT as u8 {
        if b.anchor_state(d)? != AnchorState::Registered {
            return Err(E_NO_ANCHOR);
        }
        b.confirm_anchor(d)?;
    }
    b.writeback(true, 0x40)?;
    b.audit()?;
    hits += 1;

    // R2 边界：域号越界拒收；摘要长度恰为 1 通过。
    let mut b = PactBook::new();
    if b.register_anchor(DOMAIN_COUNT as u8).is_ok() {
        return Err(E_BAD_DOMAIN);
    }
    b.writeback(true, 1)?;
    hits += 1;

    // R3 注入拦截：重复登记 / 未登记先确认 / 空摘要写回 / 非法状态 / 缺件导出。
    let mut b = PactBook::new();
    b.register_anchor(DOMAIN_B2)?;
    if b.register_anchor(DOMAIN_B2).is_ok() {
        return Err(E_DUP_ANCHOR);
    }
    if b.confirm_anchor(DOMAIN_B3).is_ok() {
        return Err(E_NO_ANCHOR);
    }
    if b.writeback(true, 0).is_ok() {
        return Err(E_EMPTY_SUM);
    }
    if b.writeback(false, 8).is_ok() {
        return Err(E_BAD_STATUS);
    }
    if b.export_artifacts().is_ok() {
        return Err(E_PACT_MISS);
    }
    hits += 1;

    // R4 守恒对账：全链审计通过；缺一件时审计必失败。
    let mut b = PactBook::new();
    b.deliver_all()?;
    for d in 0..DOMAIN_COUNT as u8 {
        b.register_anchor(d)?;
        b.confirm_anchor(d)?;
    }
    b.writeback(true, 0x20)?;
    b.audit()?;
    let mut b2 = PactBook::new();
    b2.deliver_all()?;
    for d in 0..DOMAIN_COUNT as u8 {
        b2.register_anchor(d)?;
        b2.confirm_anchor(d)?;
    }
    b2.writeback(true, 0x20)?;
    b2.artifacts[ARTIFACT_CHANGE_FLOW] = false; // 人为缺件注入
    if b2.audit().is_ok() {
        return Err(E_PACT_MISS);
    }
    hits += 1;

    // R5 追加回归：重复导出幂等 + 多条写回账 + 确认代次可查。
    let mut b = PactBook::new();
    b.deliver_all()?;
    let e1 = b.export_artifacts()?;
    let e2 = b.export_artifacts()?; // 二次导出幂等
    if e1.len() != e2.len() {
        return Err(E_PACT_MISS);
    }
    for d in 0..DOMAIN_COUNT as u8 {
        b.register_anchor(d)?;
        b.confirm_anchor(d)?;
    }
    b.writeback(true, 1)?;
    b.writeback(true, 2)?;
    b.writeback(true, 3)?;
    if b.writebacks.len() != 3 {
        return Err(E_EMPTY_SUM);
    }
    if b.anchors[DOMAIN_B4 as usize].confirm_epoch != 1 {
        return Err(E_BAD_STATE);
    }
    if b.registered_total != DOMAIN_COUNT as u32 {
        return Err(E_NO_ANCHOR);
    }
    if b.confirmed_total != DOMAIN_COUNT as u32 {
        return Err(E_ANCHOR_MISS);
    }
    b.audit()?;
    hits += 1;

    Ok(hits)
}
