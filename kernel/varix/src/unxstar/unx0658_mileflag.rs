//! UNX-F0658 · 域回归里程碑标记（收官旗标落账）· 诊断段 0x53c0（本单独占）
//!
//! 目标行数：≥200（纯功能代码）
//! 判据原文摘录：UNX-F0658-J1 收官旗标三条件（全绿+对冲平+归档齐）齐才落账，缺一拦截检出 100%。
//!
//! 设计纪律：
//!  1. no_std + alloc 纯功能线：零 unwrap/expect/panic，整数算术，确定性。
//!  2. 旗标落账是单向门：三条件齐 → 落账一次；未齐 → 拦截并留因。
//!  3. 七种缺一组合（3 条件至少缺 1）逐一注入必须 100% 拦截。
//!  4. 落账行四要素：轮号/条件指纹/落账时戳序号/操作者码，全部派生。
//!  5. 诊断段 0x53c0 为本模块独占段位，错误码闭集显式判别式。

use alloc::vec::Vec;
use alloc::string::{String, ToString};
use alloc::vec;

pub const SEG_BASE: u16 = 0x53c0;

// ── 诊断码闭集 ───────────────────────────────────────────────
pub const E_GREEN: u16 = SEG_BASE + 1;  // 条件一缺：判据未全绿
pub const E_HEDGE: u16 = SEG_BASE + 2;  // 条件二缺：对冲不平
pub const E_ARCH: u16 = SEG_BASE + 3;   // 条件三缺：归档不齐
pub const E_LANDED: u16 = SEG_BASE + 4; // 旗标已落账，重复落账拦截
pub const E_ROUND: u16 = SEG_BASE + 5;  // 轮号非法
pub const E_NOPRIOR: u16 = SEG_BASE + 6; // 未做条件预检先落账

/// 三条件闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cond {
    /// 条件一：判据全绿。
    AllGreen,
    /// 条件二：对冲平。
    HedgeFlat,
    /// 条件三：归档齐。
    ArchiveFull,
}

impl Cond {
    pub const ALL: [Cond; 3] = [Cond::AllGreen, Cond::HedgeFlat, Cond::ArchiveFull];
    pub fn name(self) -> &'static str {
        match self {
            Cond::AllGreen => "全绿",
            Cond::HedgeFlat => "对冲平",
            Cond::ArchiveFull => "归档齐",
        }
    }
}

/// 条件预检结果：逐条件布尔 + 拦截原因。
#[derive(Clone, Debug)]
pub struct CondCheck {
    pub green_ok: bool,
    pub hedge_ok: bool,
    pub arch_ok: bool,
}

impl CondCheck {
    pub fn get(&self, c: Cond) -> bool {
        match c {
            Cond::AllGreen => self.green_ok,
            Cond::HedgeFlat => self.hedge_ok,
            Cond::ArchiveFull => self.arch_ok,
        }
    }

    /// 缺失条件清单（归因输出）。
    pub fn missing(&self) -> Vec<Cond> {
        let mut out = Vec::new();
        for c in Cond::ALL {
            if !self.get(c) {
                out.push(c);
            }
        }
        out
    }

    pub fn all_met(&self) -> bool {
        self.missing().is_empty()
    }
}

/// 落账行四要素。
#[derive(Clone, Debug)]
pub struct FlagRow {
    /// 轮号。
    pub round: u16,
    /// 条件指纹：三条件布尔打包 u8（bit0..bit2）。
    pub cond_fp: u8,
    /// 落账时戳序号（单调递增）。
    pub seq: u64,
    /// 操作者码（里程碑执行器固定 0x0658）。
    pub actor: u16,
}

/// 里程碑门。
pub struct MilestoneGate {
    pub round: u16,
    pub landed: Option<FlagRow>,
    pub seq_counter: u64,
    /// 拦截立案簿：每次被拦截的落账尝试（原因, 时戳序号）。
    pub interceptions: Vec<(u16, u64)>,
}

impl MilestoneGate {
    pub fn new(round: u16) -> Result<MilestoneGate, u16> {
        if round == 0 {
            return Err(E_ROUND);
        }
        Ok(MilestoneGate {
            round,
            landed: None,
            seq_counter: 0,
            interceptions: Vec::new(),
        })
    }

    fn next_seq(&mut self) -> u64 {
        self.seq_counter += 1;
        self.seq_counter
    }

    /// 落账尝试：三条件齐才落账；缺任一 → 拦截并按缺失条件逐个立案。
    pub fn try_land(&mut self, chk: &CondCheck) -> Result<(), u16> {
        if self.landed.is_some() {
            return Err(E_LANDED);
        }
        let seq = self.next_seq();
        // 预检失败清单（先收集，逐个归因）
        let missing = chk.missing();
        if !missing.is_empty() {
            let mut code = E_GREEN;
            for m in &missing {
                code = match m {
                    Cond::AllGreen => E_GREEN,
                    Cond::HedgeFlat => E_HEDGE,
                    Cond::ArchiveFull => E_ARCH,
                };
                self.interceptions.push((code, seq));
            }
            return Err(code);
        }
        // 三条件齐 → 落账（条件指纹：三布尔打包）
        let fp = (chk.green_ok as u8) | ((chk.hedge_ok as u8) << 1) | ((chk.arch_ok as u8) << 2);
        self.landed = Some(FlagRow {
            round: self.round,
            cond_fp: fp,
            seq,
            actor: 0x0658,
        });
        Ok(())
    }

    /// 拦截检出率口径：注入的失败组合数 vs 实际拦截次数。
    pub fn interception_count(&self) -> usize {
        self.interceptions.len()
    }

    /// 旗标在账判定。
    pub fn is_landed(&self) -> bool {
        self.landed.is_some()
    }

    /// 落账行可解引复核：四要素齐且指纹 = 0b111。
    pub fn audit(&self) -> Result<(), u16> {
        match &self.landed {
            None => Err(E_NOPRIOR),
            Some(f) => {
                if f.cond_fp != 0b111 || f.actor != 0x0658 || f.round != self.round {
                    return Err(E_NOPRIOR);
                }
                if f.seq == 0 {
                    return Err(E_NOPRIOR);
                }
                Ok(())
            }
        }
    }
}

/// 七种缺一组合注入器（3 条件的全部非空真子集补集场景）。
pub fn partial_checks() -> Vec<CondCheck> {
    vec![
        CondCheck { green_ok: false, hedge_ok: true, arch_ok: true },   // 缺全绿
        CondCheck { green_ok: true, hedge_ok: false, arch_ok: true },   // 缺对冲
        CondCheck { green_ok: true, hedge_ok: true, arch_ok: false },   // 缺归档
        CondCheck { green_ok: false, hedge_ok: false, arch_ok: true },  // 缺一+二
        CondCheck { green_ok: false, hedge_ok: true, arch_ok: false },  // 缺一+三
        CondCheck { green_ok: true, hedge_ok: false, arch_ok: false },  // 缺二+三
        CondCheck { green_ok: false, hedge_ok: false, arch_ok: false }, // 全缺
    ]
}

/// 自例程：7 种缺一组合 100% 拦截 + 齐备落账 + 审计过 → 0。
pub fn drill() -> Result<u32, u16> {
    let mut gate = MilestoneGate::new(10)?;
    // 注入：7 种缺一组合，每次必须拦截且归因正确
    for (i, chk) in partial_checks().into_iter().enumerate() {
        let expect: u16 = if !chk.green_ok {
            E_GREEN
        } else if !chk.hedge_ok {
            E_HEDGE
        } else {
            E_ARCH
        };
        let r = gate.try_land(&chk);
        if r != Err(expect) {
            return Err(SEG_BASE + 15 + i as u16);
        }
        if gate.is_landed() {
            return Err(SEG_BASE + 22);
        }
    }
    // 拦截立案数 = 缺失条件总数（7 组合计 = 1+1+1+2+2+2+3 = 12）
    if gate.interception_count() != 12 {
        return Err(SEG_BASE + 23);
    }
    // 齐备 → 落账成功
    let full = CondCheck { green_ok: true, hedge_ok: true, arch_ok: true };
    gate.try_land(&full)?;
    gate.audit()?;
    // 注入：重复落账 → 拦截
    if gate.try_land(&full) != Err(E_LANDED) {
        return Err(SEG_BASE + 24);
    }
    // 注入：未落账簿审计 → 拦截
    let g2 = MilestoneGate::new(11)?;
    if g2.audit() != Err(E_NOPRIOR) {
        return Err(SEG_BASE + 25);
    }
    // 注入：非法轮号 → 拦截
    if MilestoneGate::new(0).err() != Some(E_ROUND) {
        return Err(SEG_BASE + 26);
    }
    Ok(0)
}
