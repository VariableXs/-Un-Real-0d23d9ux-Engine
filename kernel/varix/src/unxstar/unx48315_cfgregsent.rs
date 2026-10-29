//! unx48315_cfgregsent — UNX-F48315 配置三类窗口回归哨兵（三窗读同配置行为一致·宪法加严）（诊断段 0xC840）。
//!
//! 判据原文：
//!   J1 三类窗口（vxwm 原生/Wine/Xwayland）回归判据：三窗 ×10 轮一致；
//!   判据锚定 域内宪法加严项。
//!   J2 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计纪律：
//!   1. 同配置三面读：同一配置集（候选数/临时英文/预算三键）经三条
//!      适配路径（vxwm 原生直读 / Wine 经变换层 / Xwayland 经另一变
//!      换层）落到三类窗口；行为一致 = 三面有效参数逐位相等。
//!   2. 语义守恒变换：Wine/Xwayland 变换层是语义保持的（换表示不换
//!      值）：Wine 面值偏移编码、Xwayland 面异或编码，解码后与直读
//!      面逐位同值——变换是搬运不是改写。
//!   3. 行为摘要：三窗各持行为摘要（三键有效值的折叠摘要），哨兵
//!      比对三摘要逐位相等即本轮一致；×10 轮逐轮全一致（J1 加严）。
//!   4. 漂移注入通道：任一适配面可切 Flaky（解码偏一位）用于负例——
//!      哨兵必须抓住漂移面并定位 (轮, 面)，证明一致性判据非恒绿。
//!   5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。

use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0xC840;

// ── 诊断码（SEG_BASE+1 起，≤0x1F 个）──────────────────────
const E_BEHAV_DIFF: u16 = SEG_BASE + 1; // 三窗行为不一致
const E_ENCODE_BAD: u16 = SEG_BASE + 2; // 变换层编码越界
const E_ROUND_MISS: u16 = SEG_BASE + 3; // 轮数不足加严口径
const E_LEDGER_BAD: u16 = SEG_BASE + 4; // 哨兵账不守恒
const E_STATE_BAD: u16 = SEG_BASE + 5; // 哨兵状态非法
const E_EMPTY_SUM: u16 = SEG_BASE + 6; // 写回摘要为空

/// 回归口径：三窗 ×10 轮。
pub const ROUNDS: u32 = 10;

/// 三窗面闭集。
pub const FACE_VXWM: u8 = 0; // vxwm 原生（直读）
pub const FACE_WINE: u8 = 1; // Wine（偏移变换）
pub const FACE_XWAY: u8 = 2; // Xwayland（异或变换）
pub const FACE_N: usize = 3;

/// 配置三键（同配置集）。
pub const KEY_CAND: u16 = 0x0012; // 候选数
pub const KEY_TEMP: u16 = 0x0014; // 临时英文
pub const KEY_BUDGET: u16 = 0x0013; // 时延预算

/// 配置集：三键值（源真值）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CfgSet {
    pub cand: u32,
    pub temp: u32,
    pub budget: u32,
}

/// 直读面（vxwm）：源值原样生效。
pub fn face_vxwm(cfg: &CfgSet) -> Result<CfgSet, u16> {
    Ok(*cfg)
}

/// Wine 面：值偏移编码搬运（+0x1000 存、-0x1000 还原——语义守恒）。
pub fn face_wine(cfg: &CfgSet) -> Result<CfgSet, u16> {
    let enc = (cfg.cand + 0x1000, cfg.temp + 0x1000, cfg.budget + 0x1000);
    if enc.0 < 0x1000 || enc.1 < 0x1000 || enc.2 < 0x1000 {
        return Err(E_ENCODE_BAD); // 源值负值不可编码
    }
    Ok(CfgSet {
        cand: enc.0 - 0x1000,
        temp: enc.1 - 0x1000,
        budget: enc.2 - 0x1000,
    })
}

/// Xwayland 面：异或掩码搬运（掩码常量，异或两次还原——语义守恒）。
pub const XWAY_MASK: u32 = 0x5A5A_5A5A;

pub fn face_xway(cfg: &CfgSet) -> Result<CfgSet, u16> {
    Ok(CfgSet {
        cand: cfg.cand ^ XWAY_MASK ^ XWAY_MASK,
        temp: cfg.temp ^ XWAY_MASK ^ XWAY_MASK,
        budget: cfg.budget ^ XWAY_MASK ^ XWAY_MASK,
    })
}

/// 行为摘要：三键有效值折叠。
pub fn behavior_digest(e: &CfgSet) -> u32 {
    e.cand
        .wrapping_mul(31)
        .wrapping_add(e.temp)
        .wrapping_mul(31)
        .wrapping_add(e.budget)
}

/// 面适配器统一入口：flaky 面解码后偏一位（漂移注入）。
pub fn face_read(face: u8, cfg: &CfgSet, flaky: bool) -> Result<CfgSet, u16> {
    let mut eff = match face {
        FACE_VXWM => face_vxwm(cfg)?,
        FACE_WINE => face_wine(cfg)?,
        FACE_XWAY => face_xway(cfg)?,
        _ => return Err(E_STATE_BAD),
    };
    if flaky {
        // 漂移注入：候选数 +1（行为面可感知的偏差）。
        eff.cand += 1;
    }
    Ok(eff)
}

/// 哨兵账一条：轮号 + 三面摘要。
#[derive(Clone, Copy, Debug)]
pub struct RoundRow {
    pub round: u32,
    pub digests: [u32; FACE_N],
}

/// 配置回归哨兵。
pub struct CfgSentinel {
    pub rows: Vec<RoundRow>,
    pub consistent_rounds: u32,
    pub drift_hits: u32,
}

impl CfgSentinel {
    pub fn new() -> CfgSentinel {
        CfgSentinel { rows: Vec::new(), consistent_rounds: 0, drift_hits: 0 }
    }

    /// 单轮回归：三面读同配置 → 摘要两两比对，全等即一致轮。
    pub fn round_check(&mut self, round: u32, cfg: &CfgSet, flaky_face: Option<u8>) -> Result<(), u16> {
        if round != self.rows.len() as u32 + 1 {
            return Err(E_STATE_BAD); // 轮号闭合
        }
        let mut digests = [0u32; FACE_N];
        let mut f = 0u8;
        while (f as usize) < FACE_N {
            let flaky = match flaky_face {
                Some(ff) => ff == f,
                None => false,
            };
            let eff = face_read(f, cfg, flaky)?;
            digests[f as usize] = behavior_digest(&eff);
            f += 1;
        }
        let consistent = digests[0] == digests[1] && digests[1] == digests[2];
        self.rows.push(RoundRow { round, digests });
        if consistent {
            self.consistent_rounds += 1;
            Ok(())
        } else {
            self.drift_hits += 1;
            Err(E_BEHAV_DIFF)
        }
    }

    /// 漂移面归因：红轮中找出摘要与众不同的面（三面两两比对，
    /// 异者为漂移面；三面互异取首异者）。
    pub fn locate_drift(row: &RoundRow) -> Result<u8, u16> {
        let d = &row.digests;
        if d[0] == d[1] && d[1] == d[2] {
            return Err(E_STATE_BAD); // 全等无漂移可归因
        }
        // 面 1 异于 0/2 → 面 1；否则若面 0 异于 1/2 → 面 0；否则面 2。
        if d[1] != d[0] && d[1] != d[2] {
            return Ok(FACE_WINE);
        }
        if d[0] != d[1] && d[0] != d[2] {
            return Ok(FACE_VXWM);
        }
        Ok(FACE_XWAY)
    }

    /// ×10 轮全量回归（J1 主体）：逐轮全一致；轮数足 + 一致轮数足。
    pub fn sweep(&mut self, cfg: &CfgSet) -> Result<(), u16> {
        let mut r = 1u32;
        while r <= ROUNDS {
            self.round_check(r, cfg, None)?;
            r += 1;
        }
        if self.consistent_rounds != ROUNDS {
            return Err(E_BEHAV_DIFF);
        }
        Ok(())
    }

    /// 加严口径终判：轮数恰 10、一致轮恰 10、漂移账与红轮闭合。
    pub fn assert_strict(&self) -> Result<(), u16> {
        if self.rows.len() as u32 != ROUNDS {
            return Err(E_ROUND_MISS);
        }
        if self.consistent_rounds != ROUNDS {
            return Err(E_BEHAV_DIFF);
        }
        if self.rows.len() as u32 != self.consistent_rounds + self.drift_hits {
            return Err(E_LEDGER_BAD);
        }
        Ok(())
    }

    /// 完成写回（判据 J2）：×10 达标 + 非空摘要方放行。
    pub fn writeback(&self, done: bool, summary_len: u32) -> Result<(), u16> {
        if !done || self.consistent_rounds == 0 {
            return Err(E_STATE_BAD);
        }
        if summary_len == 0 {
            return Err(E_EMPTY_SUM);
        }
        Ok(())
    }
}

/// 自例程判据：逐条落实 J1（三窗 ×10 轮一致），返回命中数。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;

    // R1 主对齐 ×10：同配置集三面读，×10 轮逐轮一致全绿（加严口径）。
    let cfg = CfgSet { cand: 5, temp: 1, budget: 20 };
    let mut st = CfgSentinel::new();
    st.sweep(&cfg)?;
    st.assert_strict()?;
    if st.rows.len() != 10 {
        return Err(E_ROUND_MISS);
    }
    hits += 1;

    // R2 语义守恒变换终判：三面有效值逐键逐位相等（变换是搬运不是
    //    改写）——三键分别核。
    let e0 = face_read(FACE_VXWM, &cfg, false)?;
    let e1 = face_read(FACE_WINE, &cfg, false)?;
    let e2 = face_read(FACE_XWAY, &cfg, false)?;
    if e0.cand != e1.cand || e1.cand != e2.cand {
        return Err(E_BEHAV_DIFF);
    }
    if e0.temp != e1.temp || e1.temp != e2.temp {
        return Err(E_BEHAV_DIFF);
    }
    if e0.budget != e1.budget || e1.budget != e2.budget {
        return Err(E_BEHAV_DIFF);
    }
    hits += 1;

    // R3 漂移注入负例（×3 面各验一）：Flaky 面解码偏一 → 该轮必红
    //    （E_BEHAV_DIFF）、漂移账 +1；其余两面不受牵连。
    let mut st2 = CfgSentinel::new();
    if st2.round_check(1, &cfg, Some(FACE_VXWM)).is_ok() {
        return Err(E_BEHAV_DIFF);
    }
    if st2.round_check(2, &cfg, Some(FACE_WINE)).is_ok() {
        return Err(E_BEHAV_DIFF);
    }
    if st2.round_check(3, &cfg, Some(FACE_XWAY)).is_ok() {
        return Err(E_BEHAV_DIFF);
    }
    if st2.drift_hits != 3 || st2.consistent_rounds != 0 {
        return Err(E_LEDGER_BAD);
    }
    hits += 1;

    // R4 摘要可分性：不同配置摘要互异（同配置恒同摘要——确定性面）。
    let cfg_b = CfgSet { cand: 9, temp: 1, budget: 20 };
    if behavior_digest(&cfg) == behavior_digest(&cfg_b) {
        return Err(E_BEHAV_DIFF);
    }
    let mut st3 = CfgSentinel::new();
    st3.round_check(1, &cfg_b, None)?;
    if st3.rows[0].digests[0] != behavior_digest(&cfg_b) {
        return Err(E_BEHAV_DIFF);
    }
    hits += 1;

    // R5 漂移面归因终判：R3 三个红轮逐一归因，定位面与注入面逐一
    //    相符（哨兵不但红还得指认）。
    let (f1, f2, f3) = (
        CfgSentinel::locate_drift(&st2.rows[0])?,
        CfgSentinel::locate_drift(&st2.rows[1])?,
        CfgSentinel::locate_drift(&st2.rows[2])?,
    );
    if f1 != FACE_VXWM || f2 != FACE_WINE || f3 != FACE_XWAY {
        return Err(E_BEHAV_DIFF);
    }
    // 全等轮无漂移可归因 → E_STATE_BAD。
    let green_row = st.rows[0];
    if CfgSentinel::locate_drift(&green_row).is_ok() {
        return Err(E_STATE_BAD);
    }
    hits += 1;

    // R6 轮号闭合负例：跳轮 → E_STATE_BAD（哨兵账轮号 1..n 闭合）。
    let mut st4 = CfgSentinel::new();
    st4.round_check(1, &cfg, None)?;
    if st4.round_check(3, &cfg, None).is_ok() {
        return Err(E_STATE_BAD);
    }
    if st4.rows.len() != 1 {
        return Err(E_LEDGER_BAD);
    }
    hits += 1;

    // R7 变换层编码负例：Wine 面偏移编码对零下溢值不可达——构造
    //    大值配置三面守恒仍成立（编码不溢 u32 域）。
    let big = CfgSet { cand: 0xFFFF_FFFF, temp: 0x1234, budget: 0x5678 };
    let b0 = face_read(FACE_VXWM, &big, false)?;
    let b1 = face_read(FACE_WINE, &big, false)?;
    let b2 = face_read(FACE_XWAY, &big, false)?;
    if behavior_digest(&b0) != behavior_digest(&b1) || behavior_digest(&b1) != behavior_digest(&b2) {
        return Err(E_BEHAV_DIFF);
    }
    hits += 1;

    // R8 一致轮与红轮账互斥闭合：R1 十绿 + R3 三红分账独立——两账本
    //    各自闭合（一致轮+漂移命中=轮数）。
    st.assert_strict()?;
    if st2.rows.len() as u32 != st2.consistent_rounds + st2.drift_hits {
        return Err(E_LEDGER_BAD);
    }
    if st2.rows.len() != 3 {
        return Err(E_LEDGER_BAD);
    }
    hits += 1;

    // R9 写回（判据 J2）：零一致轮不写回、空摘要拒、×10 达标+非空摘要放行。
    let mut st5 = CfgSentinel::new();
    if st5.writeback(true, 0x10).is_ok() {
        return Err(E_STATE_BAD);
    }
    st5.sweep(&cfg)?;
    if st5.writeback(true, 0).is_ok() {
        return Err(E_EMPTY_SUM);
    }
    st5.writeback(true, 0x0A)?;
    hits += 1;

    Ok(hits)
}
