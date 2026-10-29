//! unx48251_spobs — UNX-F48251 方案面观测埋点（方案切换/简繁开关/短语命中四点）（诊断段 0xC040）。
//!
//! 判据原文：
//!   J1 四点 100%；判据锚定 体验十三·补。
//!   J2 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计纪律：
//!   1. 埋点四点闭集（体验十三·补口径）：ObsSchemeSwitch（方案切
//!      换）、ObsS2tToggle（简繁开关）、ObsPhraseHit（短语命中）、
//!      ObsTopPick（首选上屏）。四点之外不设点——观测面最小完备，
//!      账面只有四列。
//!   2. 埋点总线单入账口：fire(point, payload) 一口入账，每笔带
//!      (序号, 点位, 载荷, 时戳)；总线只增不删，账面流水不可改写。
//!      非四点位的 fire 拒收（E_POINT_BAD）且不入流水——脏点不得
//!      污账。
//!   3. 100% 口径（判据 J1）：计划面 PLAN 确定性生成 ×N 笔事件
//!      （四点各 ≥1），总线消费后四点覆盖掩码全 1、四点计数与计
//!      划面逐点相等、流水序号连续无缺——三点有一不齐即 E_COVER。
//!   4. 载荷面各点独立：切点载荷 (旧方案, 新方案)、简繁载荷 (旧
//!      档, 新档)、短语命中载荷 (code, phrase)、首选载荷 (code,
//!      top_id)——载荷出各自闭带即 E_PAYLOAD，入账前先验。
//!   5. 回放守恒：流水可重放（replay 重算四点计数），重放计数与
//!      在账计数逐点相等；重放不等即账被改写（E_REPLAY）。
//!   6. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。

use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0xC040;

// ── 诊断码（SEG_BASE+1 起，≤0x1F 个）──────────────────────
const E_POINT_BAD: u16 = SEG_BASE + 1; // 点位出四点闭集
const E_PAYLOAD: u16 = SEG_BASE + 2; // 载荷出点位闭带
const E_COVER: u16 = SEG_BASE + 3; // 覆盖/计数不齐
const E_REPLAY: u16 = SEG_BASE + 4; // 重放不等（账被改写）
const E_SEQ_HOLE: u16 = SEG_BASE + 5; // 流水序号缺号
const E_EMPTY_SUM: u16 = SEG_BASE + 6; // 写回摘要为空

/// 埋点四点闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ObsPoint {
    SchemeSwitch,
    S2tToggle,
    PhraseHit,
    TopPick,
}

impl ObsPoint {
    pub fn bit(&self) -> u32 {
        match self {
            ObsPoint::SchemeSwitch => 0x1,
            ObsPoint::S2tToggle => 0x2,
            ObsPoint::PhraseHit => 0x4,
            ObsPoint::TopPick => 0x8,
        }
    }
    pub fn idx(&self) -> usize {
        match self {
            ObsPoint::SchemeSwitch => 0,
            ObsPoint::S2tToggle => 1,
            ObsPoint::PhraseHit => 2,
            ObsPoint::TopPick => 3,
        }
    }
}

/// 流水笔目。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ObsEntry {
    pub seq: u32,
    pub point: u8, // 0..=3（四点 idx）
    pub p0: u32,
    pub p1: u32,
    pub stamp: u64,
}

/// 埋点总线（纪律 2）：单入账口、只增不删。
pub struct ObsBus {
    pub log: Vec<ObsEntry>,
    pub counts: [u32; 4],
    pub stamp_now: u64,
}

impl ObsBus {
    pub fn new() -> ObsBus {
        ObsBus { log: Vec::new(), counts: [0; 4], stamp_now: 0 }
    }

    /// 载荷闭带校验（纪律 4）。
    fn payload_ok(point: &ObsPoint, p0: u32, p1: u32) -> bool {
        match point {
            // 切换载荷：方案 id 闭带 [1,8]，新旧可同（幂等切）。
            ObsPoint::SchemeSwitch => p0 >= 1 && p0 <= 8 && p1 >= 1 && p1 <= 8,
            // 简繁载荷：档位 [0,2]。
            ObsPoint::S2tToggle => p0 <= 2 && p1 <= 2,
            // 短语命中：code [100,199]、phrase [1,9999]。
            ObsPoint::PhraseHit => p0 >= 100 && p0 <= 199 && p1 >= 1 && p1 <= 9_999,
            // 首选：code [100,199]、top 非零。
            ObsPoint::TopPick => p0 >= 100 && p0 <= 199 && p1 > 0,
        }
    }

    /// 入账主径：点位验 → 载荷验 → 流水 + 计数双落。
    pub fn fire(&mut self, point: ObsPoint, p0: u32, p1: u32) -> Result<u32, u16> {
        if !ObsBus::payload_ok(&point, p0, p1) {
            return Err(E_PAYLOAD);
        }
        self.stamp_now += 1;
        let seq = self.log.len() as u32;
        self.log.push(ObsEntry {
            seq,
            point: point.idx() as u8,
            p0,
            p1,
            stamp: self.stamp_now,
        });
        self.counts[point.idx()] += 1;
        Ok(seq)
    }

    /// 流水序号连续审计（纪律 3）：0 起无缺号。
    pub fn audit_seq(&self) -> Result<(), u16> {
        let mut i = 0u32;
        while (i as usize) < self.log.len() {
            if self.log[i as usize].seq != i {
                return Err(E_SEQ_HOLE);
            }
            i += 1;
        }
        Ok(())
    }

    /// 覆盖审计（纪律 3）：四点掩码全 1。
    pub fn audit_cover(&self) -> Result<(), u16> {
        let mut mask = 0u32;
        let mut i = 0usize;
        while i < 4 {
            if self.counts[i] > 0 {
                mask |= 1u32 << i;
            }
            i += 1;
        }
        if mask != 0xF {
            return Err(E_COVER);
        }
        Ok(())
    }

    /// 重放守恒（纪律 5）：重放重算四点计数，与在账逐点相等。
    pub fn replay(&self) -> Result<[u32; 4], u16> {
        let mut rc = [0u32; 4];
        let mut i = 0usize;
        while i < self.log.len() {
            let e = &self.log[i];
            if e.point > 3 {
                return Err(E_POINT_BAD);
            }
            rc[e.point as usize] += 1;
            i += 1;
        }
        let mut i = 0usize;
        while i < 4 {
            if rc[i] != self.counts[i] {
                return Err(E_REPLAY);
            }
            i += 1;
        }
        Ok(rc)
    }

    /// 与计划面对账（纪律 3）：四点计数逐点相等。
    pub fn audit_plan(&self, plan: &[u32; 4]) -> Result<(), u16> {
        let mut i = 0usize;
        while i < 4 {
            if self.counts[i] != plan[i] {
                return Err(E_COVER);
            }
            i += 1;
        }
        Ok(())
    }
}

/// LCG（确定性）。
pub struct Lcg {
    s: u32,
}

impl Lcg {
    pub fn new(seed: u32) -> Lcg {
        Lcg { s: seed }
    }
    pub fn next(&mut self) -> u32 {
        self.s = self.s.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        self.s
    }
    pub fn below(&mut self, n: u32) -> u32 {
        if n == 0 { 0 } else { self.next() % n }
    }
}

/// 写回判定（判据 J2）。
fn writeback(done: bool, summary_len: u32) -> Result<(), u16> {
    if !done || summary_len == 0 {
        return Err(E_EMPTY_SUM);
    }
    Ok(())
}

/// 自例程判据：逐条落实 J1（四点 100%），返回命中数。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;
    let mut rng = Lcg::new(0xC040_0100);

    // R1 主判据：计划面 ×100 笔（四点各 ≥25），总线消费后覆盖掩码
    //    全 1、计数与计划逐点相等、流水连续、重放守恒。
    let mut bus = ObsBus::new();
    let mut plan = [0u32; 4];
    let mut i = 0u32;
    while i < 100 {
        let which = i % 4;
        let ok = match which {
            0 => {
                let old = 1 + rng.below(8);
                let new = 1 + rng.below(8);
                bus.fire(ObsPoint::SchemeSwitch, old, new).is_ok()
            }
            1 => {
                let old = rng.below(3);
                let new = rng.below(3);
                bus.fire(ObsPoint::S2tToggle, old, new).is_ok()
            }
            2 => {
                let code = 100 + rng.below(100);
                let phrase = 1 + rng.below(9_999);
                bus.fire(ObsPoint::PhraseHit, code, phrase).is_ok()
            }
            _ => {
                let code = 100 + rng.below(100);
                let top = 1 + rng.below(9_999);
                bus.fire(ObsPoint::TopPick, code, top).is_ok()
            }
        };
        if ok {
            plan[which as usize] += 1;
        }
        i += 1;
    }
    bus.audit_cover()?;
    bus.audit_plan(&plan)?;
    bus.audit_seq()?;
    let rc = bus.replay()?;
    let mut i = 0usize;
    while i < 4 {
        if rc[i] != plan[i] {
            return Err(E_REPLAY);
        }
        i += 1;
    }
    hits += 1;

    // R2 脏点拒收不入账（纪律 2）：载荷出闭带四点各拒一笔，流水
    //    零增长、计数零增长。
    let mut bus = ObsBus::new();
    if bus.fire(ObsPoint::SchemeSwitch, 0, 5).is_ok() {
        return Err(E_PAYLOAD); // 方案 id 0 出带
    }
    if bus.fire(ObsPoint::S2tToggle, 1, 3).is_ok() {
        return Err(E_PAYLOAD); // 档位 3 出带
    }
    if bus.fire(ObsPoint::PhraseHit, 200, 5).is_ok() {
        return Err(E_PAYLOAD); // code 出带
    }
    if bus.fire(ObsPoint::TopPick, 150, 0).is_ok() {
        return Err(E_PAYLOAD); // top 零
    }
    if bus.log.len() != 0 {
        return Err(E_PAYLOAD);
    }
    let mut i = 0usize;
    while i < 4 {
        if bus.counts[i] != 0 {
            return Err(E_PAYLOAD);
        }
        i += 1;
    }
    hits += 1;

    // R3 四点各一笔最小覆盖：四笔入账后掩码即满（100% 的最小账面）。
    let mut bus = ObsBus::new();
    bus.fire(ObsPoint::SchemeSwitch, 1, 2)?;
    bus.fire(ObsPoint::S2tToggle, 0, 1)?;
    bus.fire(ObsPoint::PhraseHit, 100, 1)?;
    bus.fire(ObsPoint::TopPick, 100, 1)?;
    bus.audit_cover()?;
    if bus.log.len() != 4 {
        return Err(E_COVER);
    }
    bus.audit_seq()?;
    hits += 1;

    // R4 时戳单调：流水 stamp 严格递增（单入口径）。
    let mut i = 1usize;
    while i < bus.log.len() {
        if bus.log[i].stamp <= bus.log[i - 1].stamp {
            return Err(E_SEQ_HOLE);
        }
        i += 1;
    }
    hits += 1;

    // R5 重放篡改探针：篡改一笔流水点位 → 重放必炸（探针不响即
    //    重放账是空转）。
    let mut bus = ObsBus::new();
    bus.fire(ObsPoint::SchemeSwitch, 1, 2)?;
    bus.fire(ObsPoint::S2tToggle, 0, 1)?;
    bus.log[0].point = 1; // 篡改点位，重放计数将不等
    if bus.replay().is_ok() {
        return Err(E_REPLAY);
    }
    hits += 1;

    // R6 空总线覆盖必拒：零流水不可能满掩码（100% 是实账不是默认）。
    let bus = ObsBus::new();
    if bus.audit_cover().is_ok() {
        return Err(E_COVER);
    }
    hits += 1;

    // R7 写回（判据 J2）：完成态 + 非空摘要放行；空摘要拒。
    if writeback(true, 0).is_ok() {
        return Err(E_EMPTY_SUM);
    }
    writeback(true, 0x3040)?;
    hits += 1;

    Ok(hits)
}
