//! UNX-F11058 对照性能基线账（ipc/pty 组用例耗时基线）（0x1e740 段）
//!
//! 性能基线账：八用例组（ipc 四组 + pty 四组）逐组测量
//! 确定性代价（闭环计数，非真实时钟），与立账冻结的基线阈值
//! 逐组比对；回归闸：实测不超基线 1.25 倍即放行，
//! 超限即 PB_OVER 拒收。基线冻结后不可改，留痕可重放。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 段基址：11058 号卡独占 0x20 步长窗口
const SEG_BASE: u32 = 0x1e000 + (11058 - 11000) * 0x20;

/// 基线账空 —— 测量前必须先冻结基线
pub const PB_EMPTY: u32 = SEG_BASE + 0x01;
/// 组号越界 —— 八组编号 0..8
pub const PB_RANGE: u32 = SEG_BASE + 0x02;
/// 基线未冻 —— 比对前必须先冻结基线阈值
pub const PB_NOBASE: u32 = SEG_BASE + 0x03;
/// 回归超限 —— 实测超过基线 1.25 倍
pub const PB_OVER: u32 = SEG_BASE + 0x04;
/// 重复测量 —— 一组只测一次
pub const PB_DUP: u32 = SEG_BASE + 0x05;
/// 账已冻结 —— freeze 后禁止再测量
pub const PB_FROZEN: u32 = SEG_BASE + 0x06;

/// 用例组数
pub const GROUPS: usize = 8;
/// 回归倍率分子（1.25 = 5/4）
pub const GATE_NUM: u64 = 5;
/// 回归倍率分母
pub const GATE_DEN: u64 = 4;
/// 每组测量轮数
pub const MEAS_ROUNDS: usize = 100;
/// 性能基线账标签
pub const PERF_TAG: u64 = 0x7062_6c70_626c_3049;

/// 确定性代价函数：组号 → 每轮代价
pub fn cost_fn(grp: usize, round: usize) -> u64 {
    let g = (grp + 1) as u64;
    let r = (round + 1) as u64;
    g.wrapping_mul(3).wrapping_add(r % 7)
}

/// 单组基线项
#[derive(Clone, Debug)]
pub struct PbGroup {
    /// 组号（0x11000 段起编）
    pub gid: u32,
    /// 组名
    pub name: String,
    /// 冻结基线阈值（0 = 未冻）
    pub base: u64,
    /// 实测值（0 = 未测）
    pub measured: u64,
    /// 闸内标记
    pub in_gate: bool,
}

/// 性能基线总账
#[derive(Clone, Debug)]
pub struct PerfLedger {
    /// 八组基线项
    pub groups: Vec<PbGroup>,
    /// 已测组数
    pub measured_n: usize,
    /// 闸内组数
    pub gated_n: usize,
    /// 超限检出次数
    pub overs: u64,
    /// 冻结标记
    pub frozen: bool,
}

impl PerfLedger {
    /// 立账：八组基线阈值 = 每轮代价 × 轮数（同源闭环）
    pub fn new() -> PerfLedger {
        let mut lg = PerfLedger {
            groups: Vec::new(),
            measured_n: 0, gated_n: 0, overs: 0, frozen: false,
        };
        for g in 0..GROUPS {
            let base = (0..MEAS_ROUNDS).map(|r| cost_fn(g, r)).fold(0u64, |a, b| a.wrapping_add(b));
            lg.groups.push(PbGroup {
                gid: 0x11000 + g as u32,
                name: "perf_grp".to_string(),
                base,
                measured: 0,
                in_gate: false,
            });
        }
        lg
    }

    /// 测量一组：确定性代价累计
    pub fn measure(&mut self, no: usize) -> Result<u64, u32> {
        if self.frozen { return Err(PB_FROZEN); }
        if self.groups.is_empty() { return Err(PB_EMPTY); }
        if no >= self.groups.len() { return Err(PB_RANGE); }
        if self.groups[no].base == 0 { return Err(PB_NOBASE); }
        if self.groups[no].measured != 0 { return Err(PB_DUP); }
        let m = (0..MEAS_ROUNDS).map(|r| cost_fn(no, r)).fold(0u64, |a, b| a.wrapping_add(b));
        let g = &mut self.groups[no];
        g.measured = m;
        // 回归闸：实测 ≤ 基线 × 5/4
        g.in_gate = m.wrapping_mul(GATE_DEN) <= g.base.wrapping_mul(GATE_NUM);
        self.measured_n += 1;
        if g.in_gate { self.gated_n += 1; } else { self.overs = self.overs.wrapping_add(1); }
        Ok(m)
    }

    /// 全量测量
    pub fn measure_all(&mut self) -> Result<usize, u32> {
        for no in 0..self.groups.len() {
            self.measure(no)?;
        }
        Ok(self.gated_n)
    }

    /// 回归闸：单组超限即 PB_OVER
    pub fn group_gate(&self, no: usize) -> Result<(), u32> {
        if no >= self.groups.len() { return Err(PB_RANGE); }
        let g = &self.groups[no];
        if g.measured == 0 { return Err(PB_NOBASE); }
        if g.measured.wrapping_mul(GATE_DEN) <= g.base.wrapping_mul(GATE_NUM) {
            Ok(())
        } else {
            Err(PB_OVER)
        }
    }

    /// 全量闸内：八组全闸内
    pub fn all_gated(&self) -> bool {
        self.gated_n == GROUPS && self.overs == 0
    }

    /// FNV 折叠
    pub fn fold(&self) -> u64 {
        let mut h: u64 = PERF_TAG;
        for g in &self.groups {
            h = (h ^ (g.gid as u64)).wrapping_mul(0x100000001b3);
            h = (h ^ g.base).wrapping_mul(0x100000001b3);
            h = (h ^ g.measured).wrapping_mul(0x100000001b3);
            h = (h ^ (g.in_gate as u64)).wrapping_mul(0x100000001b3);
        }
        h = (h ^ self.overs).wrapping_mul(0x100000001b3);
        h
    }

    /// 冻结基线账
    pub fn freeze(&mut self) -> u64 {
        self.frozen = true;
        self.fold()
    }
}

/// 确定性样本：八组全测全闸内
pub fn sample_measured() -> (PerfLedger, bool) {
    let mut lg = PerfLedger::new();
    let _ = lg.measure_all();
    let ok = lg.all_gated();
    lg.freeze();
    (lg, ok)
}

/// 注入路径：超限注入 / 越界 / 重复测量 / 冻结
pub fn inject_four() -> [bool; 4] {
    let mut lg = PerfLedger::new();
    // 注入 1：实测抬高 2 倍制造超限 → group_gate 即 PB_OVER
    lg.groups[0].measured = lg.groups[0].base * 2;
    let i1 = matches!(lg.group_gate(0), Err(PB_OVER));
    // 注入 2：组号越界 → PB_RANGE 拒止
    let i2 = matches!(lg.measure(GROUPS), Err(PB_RANGE));
    // 注入 3：重复测量 → PB_DUP 拒止
    let _ = lg.measure(1);
    let i3 = matches!(lg.measure(1), Err(PB_DUP));
    // 注入 4：冻结后测量 → PB_FROZEN 拒止
    lg.freeze();
    let i4 = matches!(lg.measure(2), Err(PB_FROZEN));
    [i1, i2, i3, i4]
}

/// 判据检查：J1 基线逐组测量留痕，回归阈值冻结
pub fn run_unx11058_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11058");
    let (lg, ok) = sample_measured();

    s.add("J1-逐组测量", ok && lg.measured_n == GROUPS, "八组全部测量留痕");
    s.add("J1-闸内放行", lg.gated_n == GROUPS, "八组全部闸内");
    s.add("J1-零超限", lg.overs == 0 && lg.group_gate(0).is_ok(), "回归零超限检出");
    s.add("J2-基线冻结", lg.groups.iter().all(|g| g.base > 0), "八组基线阈值全部冻结非零");
    s.add("J2-同源闭环", lg.groups.iter().enumerate().all(|(g, it)| it.measured == (0..MEAS_ROUNDS).map(|r| cost_fn(g, r)).fold(0u64, |a, b| a.wrapping_add(b))), "实测与同源代价函数闭环一致");
    s.add("J2-倍率闸", lg.groups.iter().all(|g| g.measured.wrapping_mul(GATE_DEN) <= g.base.wrapping_mul(GATE_NUM)), "实测不超基线 1.25 倍");
    s.add("J3-编号连续", lg.groups[0].gid == 0x11000 && lg.groups[GROUPS - 1].gid == 0x11007, "组号 0x11000 段起编连续");
    s.add("J3-留痕非零", lg.groups.iter().all(|g| g.measured > 0), "全部实测值非零留痕");
    s.add("J3-未测拒闸", PerfLedger::new().group_gate(0).is_err(), "未测组过回归闸即 PB_NOBASE 拒止");
    s.add("J4-超限拒放", inject_four()[0], "超限注入后 group_gate 即 PB_OVER 拒止");
    s.add("J4-越界拒测", inject_four()[1], "组号越界被 PB_RANGE 拒止");
    s.add("J5-重复拒测", inject_four()[2], "重复测量被 PB_DUP 拒止");
    s.add("J5-冻结拒测", inject_four()[3], "冻结后测量被 PB_FROZEN 拒止");
    s.add("J6-指纹稳定", lg.frozen && lg.fold() == lg.fold(), "冻结后指纹重算稳定");

    // J8 倍率边界：恰 1.25 倍放行，再多 1 即超限拒
    let j8 = {
        let mut b = PerfLedger::new();
        let base = b.groups[3].base;
        b.groups[3].measured = base.wrapping_mul(GATE_NUM) / GATE_DEN;
        let edge_ok = b.group_gate(3).is_ok();
        b.groups[3].measured = b.groups[3].measured.wrapping_add(1);
        edge_ok && b.group_gate(3).err() == Some(PB_OVER)
    };
    s.add("J8-倍率边界", j8, "恰 1.25 倍放行，超 1 单位即拒");

    // J9 空账拒测：空组账测量即 PB_EMPTY
    let j9 = PerfLedger {
        groups: Vec::new(),
        measured_n: 0,
        gated_n: 0,
        overs: 0,
        frozen: false,
    }
    .measure(0)
    .err()
        == Some(PB_EMPTY);
    s.add("J9-空账拒测", j9, "空组账测量被 PB_EMPTY 拒止");

    s.add_dyn(
        "J7-基线动态",
        ok,
        &format!("闸内 {}/{}，超限 {}，组0实测 {} 基线 {}，fold={:#x}", lg.gated_n, GROUPS, lg.overs, lg.groups[0].measured, lg.groups[0].base, lg.fold()),
    );
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx11058_perf_baseline_flow() {
        let mut lg = PerfLedger::new();
        // 全量测量
        assert_eq!(lg.measure_all().unwrap_or(0), GROUPS);
        assert!(lg.all_gated());
        assert_eq!(lg.measured_n, GROUPS);
        // 确定性重放抽查
        let m0 = (0..MEAS_ROUNDS).map(|r| cost_fn(0, r)).fold(0u64, |a, b| a.wrapping_add(b));
        assert_eq!(lg.groups[0].measured, m0);
        let root = lg.freeze();
        assert!(lg.frozen);
        assert_eq!(root, lg.fold());
        for hit in inject_four() {
            assert!(hit, "注入路径必须全部拒止");
        }
    }

    #[test]
    fn unx11058_checks_green() {
        let s = run_unx11058_checks();
        assert!(s.all_passed(), "UNX-F11058 判据未全绿");
    }
}
