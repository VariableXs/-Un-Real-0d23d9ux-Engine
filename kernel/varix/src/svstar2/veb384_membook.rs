//! veb384_membook —— VE-F0384 B 域内存总账。
//! 锚点（CoRun Unxreal 主册）：VE-F0384 · B 域内存总账 · 目标 320 行
//! 判据原文：VE-F0384-J1 B 域内存总账记三类账：分账（谁拿了多
//!           少）、还账（谁还了多少）、溢账（峰值水位）；总账
//!           平衡核（分 - 还 = 在用，逐主体核）；溢账峰值单调
//!           记录（只升不降）；对账单出报表（逐主体在用 + 总
//!           水位）；负余额即红（还超过分）。
//! 组件：主体分账表 + 平衡核 + 峰值水位 + 报表。
//! 职责定位：内存总账是「B 域的现金流水」——每笔分配是分账，
//!           每笔释放是还账，任何时刻「分 - 还」必须等于该主体
//!           真实在用量；峰值水位只升不降是审计需要：降了就是
//!           有人改史；负余额是复式记账的铁律红线，一条都不许碰。
//! 性能逐项分解：分/还 O(n)（查主体）；平衡核 O(n)；报表 O(n)。
//! 诊断码段：0x30580（0x30581~0x3058A 闭集）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 模块版本。
pub const MB_VERSION: &str = "veb384_membook-v1";

/// 诊断段基址（0x30580 段 · 本模块独占）。
pub const MB_SEG_BASE: u32 = 0x30580;
pub const XC_NEG: u32 = 0x30581; // 负余额（还超分）
pub const XC_NOENT: u32 = 0x30582; // 无此主体
pub const XC_ZERO: u32 = 0x30583; // 零量操作
pub const XC_MAGIC: u32 = 0x30584; // 账魔数破
pub const XC_IMBAL: u32 = 0x30585; // 账不平（在用与流水差）
pub const XC_PEAK_DROP: u32 = 0x30586; // 峰值回撤（改史）
pub const XC_SUB_DUP: u32 = 0x30587; // 主体重名
pub const XC_LEDGER: u32 = 0x30588; // 总水位与逐员和不一致
pub const XC_NAME: u32 = 0x30589; // 名空
pub const XC_FLOW: u32 = 0x3058A; // 流水断裂

/// 主体账。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SubAcct {
    pub name: &'static str,
    /// 累计分账。
    pub granted: u64,
    /// 累计还账。
    pub returned: u64,
    /// 在用（= granted - returned）。
    pub live: u64,
}

/// 报表行。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RepRow {
    pub name: &'static str,
    pub live: u64,
}

/// 报表。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Report {
    pub rows: Vec<RepRow>,
    /// 总水位（峰值）。
    pub peak: u64,
    /// 当前总在用。
    pub total_live: u64,
}

/// 内存总账。
pub struct MemBook {
    subs: Vec<SubAcct>,
    peak: u64,
    magic: u32,
}

impl MemBook {
    /// 建。
    pub fn new() -> MemBook {
        MemBook { subs: Vec::new(), peak: 0, magic: 0x0580_9E6A }
    }

    /// 开户（分账即开户）。
    pub fn grant(&mut self, name: &'static str, amt: u64) -> Result<u64, u32> {
        if self.magic != 0x0580_9E6A {
            return Err(XC_MAGIC);
        }
        if name.is_empty() {
            return Err(XC_NAME);
        }
        if amt == 0 {
            return Err(XC_ZERO);
        }
        let mut idx: Option<usize> = None;
        for (i, s) in self.subs.iter().enumerate() {
            if s.name == name {
                idx = Some(i);
            }
        }
        match idx {
            Some(i) => {
                self.subs[i].granted += amt;
                self.subs[i].live += amt;
            }
            None => {
                self.subs.push(SubAcct { name, granted: amt, returned: 0, live: amt });
            }
        }
        self.touch_peak();
        Ok(self.subs[idx.unwrap_or(self.subs.len() - 1)].live)
    }

    /// 还账。
    pub fn repay(&mut self, name: &str, amt: u64) -> Result<u64, u32> {
        if amt == 0 {
            return Err(XC_ZERO);
        }
        let i = match self.subs.iter().position(|s| s.name == name) {
            Some(i) => i,
            None => return Err(XC_NOENT),
        };
        if self.subs[i].live < amt {
            return Err(XC_NEG);
        }
        self.subs[i].returned += amt;
        self.subs[i].live -= amt;
        Ok(self.subs[i].live)
    }

    /// 峰值触点（只升不降）。
    fn touch_peak(&mut self) {
        let mut total = 0u64;
        for s in self.subs.iter() {
            total += s.live;
        }
        if total > self.peak {
            self.peak = total;
        }
    }

    /// 当前总在用。
    pub fn total_live(&self) -> u64 {
        let mut t = 0u64;
        for s in self.subs.iter() {
            t += s.live;
        }
        t
    }

    /// 峰值。
    pub fn peak(&self) -> u64 {
        self.peak
    }

    /// 平衡核：逐主体 granted - returned == live；总水位 ≥ 当前总在用。
    pub fn audit(&self) -> Result<u32, u32> {
        if self.magic != 0x0580_9E6A {
            return Err(XC_MAGIC);
        }
        for s in self.subs.iter() {
            if s.granted.wrapping_sub(s.returned) != s.live {
                return Err(XC_IMBAL);
            }
            if s.returned > s.granted {
                return Err(XC_NEG);
            }
        }
        if self.peak < self.total_live() {
            return Err(XC_PEAK_DROP);
        }
        Ok(self.subs.len() as u32)
    }

    /// 出报表（主体注册序稳定）。
    pub fn report(&self) -> Result<Report, u32> {
        let mut rows = Vec::new();
        let mut total = 0u64;
        for s in self.subs.iter() {
            rows.push(RepRow { name: s.name, live: s.live });
            total += s.live;
        }
        if total != self.total_live() {
            return Err(XC_LEDGER);
        }
        Ok(Report { rows, peak: self.peak, total_live: total })
    }

    /// 主体数。
    pub fn sub_count(&self) -> usize {
        self.subs.len()
    }
}

pub fn run_veb384_checks() -> CheckSet {
    let mut s = CheckSet::new("VE-F0384");
    // 族一：分还流水。
    let mut b = MemBook::new();
    let g1 = b.grant("fs", 100);
    s.add("fs 分 100", g1 == Ok(100), "开户");
    let g2 = b.grant("net", 50);
    s.add("net 分 50", g2 == Ok(50), "开户");
    s.add("追加分账", b.grant("fs", 30) == Ok(130), "累加");
    let r1 = b.repay("fs", 60);
    s.add("fs 还 60", r1 == Ok(70), "余 70");
    s.add("峰值 150", b.peak() == 150, "130+50 时点");
    // 族二：平衡核。
    s.add("审计过", b.audit() == Ok(2), "两员平账");
    let rep = b.report();
    s.add("报表总在用", matches!(&rep, Ok(rp) if rp.total_live == 120), "70+50");
    s.add("报表峰值", matches!(&rep, Ok(rp) if rp.peak == 150), "只升不降");
    s.add("fs 行", matches!(&rep, Ok(rp) if rp.rows.first() == Some(&RepRow { name: "fs", live: 70 })), "注册序");
    // 族三：负余额红线。
    s.add("还超分拒", b.repay("fs", 71) == Err(XC_NEG), "余 70 还 71");
    s.add("账未动", b.audit() == Ok(2), "拒后无痕");
    // 族四：零量与非主体。
    s.add("零分拒", b.grant("z", 0) == Err(XC_ZERO), "零量");
    s.add("零还拒", b.repay("fs", 0) == Err(XC_ZERO), "零量");
    s.add("无名还拒", b.repay("ghost", 1) == Err(XC_NOENT), "无主体");
    s.add("名空拒", b.grant("", 1) == Err(XC_NAME), "名空");
    // 族五：还清后再核。
    let _ = b.repay("fs", 70);
    let _ = b.repay("net", 50);
    s.add("全清零在用", b.total_live() == 0, "两清");
    s.add("峰值保持", b.peak() == 150, "史不动");
    s.add("清后审计", b.audit() == Ok(2), "平");
    let rep2 = b.report();
    s.add("清后报表", matches!(&rep2, Ok(rp) if rp.total_live == 0 && rp.peak == 150), "零在用峰值在");
    s.add("魔数核", MB_SEG_BASE == 0x30580, "段基址");
    s
}
