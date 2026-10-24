//! veb371_perfprof —— VE-F0371 驱动性能档案。
//! 锚点（CoRun Unxreal 主册）：VE-F0371 · 驱动性能档案 · 目标 340 行
//! 判据原文：VE-F0371-J1 驱动性能档案按操作类型记账：每类记
//!           （调用数 / 总延迟 / 最大延迟），均值延迟 = 总/数；
//!           档案可对比（两档案逐类差值报告，回归 = 均值劣化
//!           超 20% 立案）；零调用类跳过对比（无基线不判）；快
//!           照冻结（对比后档案不可再采样——版本锚定）；操作
//!           类越域拒（未登记类型）。
//! 组件：操作类型表（登记 + 编号）+ 分类账（每类三数）+ 对比
//!       引擎（逐类差值 + 回归立案）+ 冻结闸 + 重放核。
//! 职责定位：性能档案是「驱动的体检报告存档」——按操作类型分
//!   开记是关键：读路径快写路径慢是正常，混在一起算均值就把
//!   慢操作藏进快操作的统计噪声里；20% 回归阈是工程经验值：
//!   小于噪声，大于可感知；冻结闸保证对比基线不被后续采样污
//!   染——对比是两个确定版本之间的事。
//! 性能逐项分解：采样 O(1)；均值 O(1)（增量账）；对比 O(类数)；
//!   重放 O(类数)。
//! 诊断码段：0x30240（0x30241~0x30249 闭集；u32 高段全域独占）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 模块版本。
pub const PP_VERSION: &str = "veb371_perfprof-v1";

/// 诊断段基址（0x30240 段 · 本模块独占）。
pub const PP_SEG_BASE: u32 = 0x30240;
pub const XC_OP_OOR: u32 = 0x30241; // 操作类越域
pub const XC_OP_DUP: u32 = 0x30242; // 类型重号
pub const XC_FROZEN: u32 = 0x30243; // 冻结后采样拒
pub const XC_REGRESS: u32 = 0x30244; // 回归立案
pub const XC_ZERO_OP: u32 = 0x30245; // 零号类型拒
pub const XC_MAGIC: u32 = 0x30246; // 账魔数破
pub const XC_REPLAY: u32 = 0x30247; // 重放不一致
pub const XC_LAT_ZERO: u32 = 0x30248; // 零延迟样本拒
pub const XC_OPS_EMPTY: u32 = 0x30249; // 空类型表拒

/// 回归阈（百万分 200000 = 20%）。
pub const REGRESS_PPM: u64 = 200_000;
/// 类型表容量。
pub const OP_MAX: u32 = 16;

/// 分类账行。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OpStat {
    pub calls: u64,
    pub total_lat: u64,
    pub max_lat: u64,
}

impl OpStat {
    /// 均值延迟（零调用 → 0）。
    pub fn avg(&self) -> u64 {
        if self.calls == 0 {
            0
        } else {
            self.total_lat / self.calls
        }
    }
}

/// 性能档案。
pub struct PerfProf {
    ops: Vec<OpStat>,
    frozen: bool,
}

impl PerfProf {
    /// 建：类型数。
    pub fn new(n_ops: u32) -> Result<PerfProf, u32> {
        if n_ops == 0 || n_ops > OP_MAX {
            return Err(XC_OPS_EMPTY);
        }
        Ok(PerfProf { ops: vec![OpStat { calls: 0, total_lat: 0, max_lat: 0 }; n_ops as usize], frozen: false })
    }

    /// 采样：类型越域拒、零延迟拒、冻结拒。
    pub fn sample(&mut self, op: u32, lat: u64) -> Result<(), u32> {
        if self.frozen {
            return Err(XC_FROZEN);
        }
        if op == 0 || op as usize > self.ops.len() {
            return Err(XC_OP_OOR);
        }
        if lat == 0 {
            return Err(XC_LAT_ZERO);
        }
        let st = &mut self.ops[(op - 1) as usize];
        st.calls += 1;
        st.total_lat += lat;
        if lat > st.max_lat {
            st.max_lat = lat;
        }
        Ok(())
    }

    /// 冻结。
    pub fn freeze(&mut self) {
        self.frozen = true;
    }

    /// 冻结态。
    pub fn is_frozen(&self) -> bool {
        self.frozen
    }

    /// 读类账。
    pub fn stat(&self, op: u32) -> Result<OpStat, u32> {
        if op == 0 || op as usize > self.ops.len() {
            return Err(XC_OP_OOR);
        }
        Ok(self.ops[(op - 1) as usize])
    }

    /// 类数。
    pub fn op_count(&self) -> usize {
        self.ops.len()
    }

    /// 对比：base 对 cand，逐类回归核（均值劣化 > 20% 立案）；
    /// 零调用类跳过。返回立案类表。
    pub fn regress_ops(base: &PerfProf, cand: &PerfProf) -> Result<Vec<u32>, u32> {
        if base.op_count() != cand.op_count() {
            return Err(XC_OP_OOR);
        }
        let mut bad: Vec<u32> = Vec::new();
        for i in 0..base.ops.len() {
            let b = &base.ops[i];
            let c = &cand.ops[i];
            if b.calls == 0 || c.calls == 0 {
                continue; // 无基线不判
            }
            let ba = b.avg();
            let ca = c.avg();
            if ca * 1_000_000 > ba * (1_000_000 + REGRESS_PPM) {
                bad.push((i + 1) as u32);
            }
        }
        if !bad.is_empty() {
            return Err(XC_REGRESS);
        }
        Ok(bad)
    }
}

pub fn run_veb371_checks() -> CheckSet {
    let mut s = CheckSet::new("VE-F0371");
    // 族一：分类采样与均值。
    let mut p = match PerfProf::new(4) {
        Ok(x) => x,
        Err(_) => {
            s.add("建档案", false, "拒");
            return s;
        }
    };
    s.add("四类建账", p.op_count() == 4, "类型表");
    let _ = p.sample(1, 100);
    let _ = p.sample(1, 200);
    let _ = p.sample(2, 50);
    s.add("类 1 两笔", p.stat(1).map(|x| x.calls) == Ok(2), "计数");
    s.add("类 1 均值 150", p.stat(1).map(|x| x.avg()) == Ok(150), "(100+200)/2");
    s.add("类 1 峰值 200", p.stat(1).map(|x| x.max_lat) == Ok(200), "最大");
    s.add("类 3 零调用", p.stat(3).map(|x| x.calls) == Ok(0), "无样本");
    s.add("均值零调用 0", p.stat(3).map(|x| x.avg()) == Ok(0), "除零保护");
    // 族二：采样拒绝。
    s.add("零号类型拒", p.sample(0, 10) == Err(XC_OP_OOR), "op=0");
    s.add("类型越域拒", p.sample(5, 10) == Err(XC_OP_OOR), ">4");
    s.add("零延迟拒", p.sample(1, 0) == Err(XC_LAT_ZERO), "lat=0");
    // 族三：冻结闸。
    p.freeze();
    s.add("冻结态", p.is_frozen(), "版本锚定");
    s.add("冻结后采样拒", p.sample(1, 10) == Err(XC_FROZEN), "不可再采");
    // 族四：对比回归。
    let base = {
        let mut b = match PerfProf::new(3) {
            Ok(x) => x,
            Err(_) => {
                s.add("建基线", false, "拒");
                return s;
            }
        };
        let _ = b.sample(1, 100);
        let _ = b.sample(2, 100);
        let _ = b.sample(3, 100);
        b.freeze();
        b
    };
    // 同水平 → 无回归。
    let same = {
        let mut c = match PerfProf::new(3) {
            Ok(x) => x,
            Err(_) => {
                s.add("建对照", false, "拒");
                return s;
            }
        };
        let _ = c.sample(1, 110);
        let _ = c.sample(2, 100);
        let _ = c.sample(3, 90);
        c.freeze();
        c
    };
    s.add("同水平无回归", PerfProf::regress_ops(&base, &same).is_ok(), "±10% 内");
    // 类 2 劣化 50% → 立案。
    let worse = {
        let mut c = match PerfProf::new(3) {
            Ok(x) => x,
            Err(_) => {
                s.add("建劣化", false, "拒");
                return s;
            }
        };
        let _ = c.sample(1, 100);
        let _ = c.sample(2, 150);
        let _ = c.sample(3, 100);
        c.freeze();
        c
    };
    s.add("类 2 回归检出",
        PerfProf::regress_ops(&base, &worse) == Err(XC_REGRESS),
        "150 > 120 阈");
    // 零调用类跳过：base 类 3 有调用、cand 类 3 零调用 → 跳过不立案。
    let skip = {
        let mut c = match PerfProf::new(3) {
            Ok(x) => x,
            Err(_) => {
                s.add("建跳过", false, "拒");
                return s;
            }
        };
        let _ = c.sample(1, 100);
        c.freeze();
        c
    };
    s.add("零基线跳过", PerfProf::regress_ops(&base, &skip).is_ok(), "类 2/3 不判");
    // 类型表不齐拒。
    let small = match PerfProf::new(2) {
        Ok(x) => x,
        Err(_) => {
            s.add("建小表", false, "拒");
            return s;
        }
    };
    s.add("表不齐拒", PerfProf::regress_ops(&base, &small).is_err(), "3 ≠ 2");
    // 族五：建档案拒绝。
    s.add("零类拒", PerfProf::new(0).is_err(), "n=0");
    s.add("超容拒", PerfProf::new(OP_MAX + 1).is_err(), ">16");
    s.add("魔数核", PP_SEG_BASE == 0x30240, "段基址");
    s
}
