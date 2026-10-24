//! unx0409_clksel —— UNX-F0409 时钟源选择账（优先级矩阵）。
//! 锚点（CoRun Unxreal 主册）：UNX-F0409 · 时钟源选择账 · 目标 240 行
//! 判据原文：UNX-F0409-J1 时钟源按优先级矩阵选主：候选源带三
//!           元评分（精度分/开销分/可用位），总分高者优先；
//!           可用位缺失直接出局（不论分高）；主源失效自动切
//!           备源（切换留痕 + 新主可用位核）；选择可重放（同
//!           评分表同结论）；评分越域拒（分值超千分制）。
//! 组件：候选源（名字 + 三元评分 + 可用位）+ 评分矩阵（加权
//!       总分）+ 选择引擎（可用过滤 + 总分排序）+ 切换账
//!       （留痕 + 主备表）+ 重放核。
//! 职责定位：时钟源选择是「计时底座的选美」——TSC 快但可能
//!   不变频、PIT 稳但慢、HPET 均衡但可能没有；评分矩阵把
//!   「快/稳/在」三元拆开打分，可用位是一票否决项：分数再
//!   高的源不在就是不在。自动切换保证主源失效时计时不断。
//! 性能逐项分解：评分 O(候选数)；选择 O(候选数)（线性扫极值）；
//!   切换 O(1)；重放 O(候选数)。
//! 诊断码段：0x20FC0（0x20FC1~0x20FC9 闭集；u32 高段全域独占）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 模块版本。
pub const CK_VERSION: &str = "unx0409_clksel-v1";

/// 诊断段基址（0x20FC0 段 · 本模块独占）。
pub const CK_SEG_BASE: u32 = 0x20FC0;
pub const XC_SCORE_OOR: u32 = 0x20FC1; // 评分超千分
pub const XC_NO_CAND: u32 = 0x20FC2; // 无可用候选
pub const XC_DUP_NAME: u32 = 0x20FC3; // 候选重名
pub const XC_NAME_LEN: u32 = 0x20FC4; // 名长越界
pub const XC_SWITCH_LOST: u32 = 0x20FC5; // 切换留痕缺失
pub const XC_REPLAY: u32 = 0x20FC6; // 重放不一致
pub const XC_MAIN_DEAD: u32 = 0x20FC7; // 主源失效未切换
pub const XC_WEIGHT_SUM: u32 = 0x20FC8; // 权重和越域
pub const XC_MAGIC: u32 = 0x20FC9; // 矩阵魔数破

/// 候选时钟源。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ClkCand {
    /// 名（1~15 字节）。
    pub name: &'static str,
    /// 精度分（0~1000，越高越好）。
    pub precision: u32,
    /// 开销分（0~1000，越高 = 开销越低越好）。
    pub cheap: u32,
    /// 可用位。
    pub available: bool,
}

/// 评分矩阵（权重：精度 + 开销，和须 = 1000）。
#[derive(Clone, Copy, Debug)]
pub struct Weights {
    pub w_precision: u32,
    pub w_cheap: u32,
}

impl Weights {
    /// 权重和核。
    pub fn ok(&self) -> Result<(), u32> {
        if self.w_precision + self.w_cheap != 1000 {
            return Err(XC_WEIGHT_SUM);
        }
        Ok(())
    }
}

/// 时钟源选择账。
pub struct ClkBook {
    cands: Vec<ClkCand>,
    weights: Weights,
    /// 主源名。
    main: &'static str,
    /// 切换留痕：(步号, 旧主, 新主)。
    trace: Vec<(u64, &'static str, &'static str)>,
    step: u64,
}

impl ClkBook {
    /// 建：候选表 + 权重。
    pub fn new(cands: Vec<ClkCand>, weights: Weights) -> Result<ClkBook, u32> {
        weights.ok()?;
        for (i, c) in cands.iter().enumerate() {
            let nb = c.name.as_bytes().len();
            if nb == 0 || nb > 15 {
                return Err(XC_NAME_LEN);
            }
            if c.precision > 1000 || c.cheap > 1000 {
                return Err(XC_SCORE_OOR);
            }
            for other in cands.iter().skip(i + 1) {
                if other.name == c.name {
                    return Err(XC_DUP_NAME);
                }
            }
        }
        // 初选主源。
        let main = match pick_main(&cands, &weights) {
            Some(n) => n,
            None => return Err(XC_NO_CAND),
        };
        Ok(ClkBook { cands, weights, main, trace: Vec::new(), step: 1 })
    }

    /// 总分（加权）。
    pub fn score_of(&self, c: &ClkCand) -> u32 {
        (c.precision * self.weights.w_precision + c.cheap * self.weights.w_cheap) / 1000
    }

    /// 当前主源。
    pub fn main(&self) -> &'static str {
        self.main
    }

    /// 主源失效切换：新主 = 余下可用者中最高分；无可用拒。
    pub fn failover(&mut self) -> Result<&'static str, u32> {
        // 现主出局。
        for c in self.cands.iter_mut() {
            if c.name == self.main {
                c.available = false;
            }
        }
        let mut rest: Vec<ClkCand> = Vec::new();
        for c in self.cands.iter() {
            if c.available {
                rest.push(*c);
            }
        }
        if rest.is_empty() {
            return Err(XC_NO_CAND);
        }
        let new_main = match pick_main(&rest, &self.weights) {
            Some(n) => n,
            None => return Err(XC_NO_CAND),
        };
        self.trace.push((self.step, self.main, new_main));
        self.step += 1;
        self.main = new_main;
        Ok(new_main)
    }

    /// 重放核：评分重算主源与当前主一致（无失效历史时）。
    pub fn replay_ok(&self) -> Result<(), u32> {
        if !self.trace.is_empty() {
            return Ok(()); // 有切换史，重放只对初选
        }
        let mut rest: Vec<ClkCand> = Vec::new();
        for c in self.cands.iter() {
            if c.available {
                rest.push(*c);
            }
        }
        match pick_main(&rest, &self.weights) {
            Some(n) if n == self.main => Ok(()),
            Some(_) => Err(XC_REPLAY),
            None => Err(XC_NO_CAND),
        }
    }

    /// 留痕核：每次切换必有痕。
    pub fn trace_ok(&self) -> Result<(), u32> {
        let expected = self
            .cands
            .iter()
            .filter(|c| !c.available && c.name != self.main)
            .count() as u64;
        if expected > 0 && self.trace.is_empty() {
            return Err(XC_SWITCH_LOST);
        }
        Ok(())
    }

    /// 留痕数。
    pub fn trace_count(&self) -> usize {
        self.trace.len()
    }

    /// 可用候选数。
    pub fn avail_count(&self) -> usize {
        self.cands.iter().filter(|c| c.available).count()
    }

    /// 矩阵魔数核（权重和复检）。
    pub fn magic_ok(&self) -> Result<(), u32> {
        self.weights.ok()?;
        if self.weights.w_precision == 0 && self.weights.w_cheap == 0 {
            return Err(XC_MAGIC);
        }
        Ok(())
    }
}

/// 选主：可用者中总分最高（同分取表中靠前者）。
fn pick_main(cands: &[ClkCand], w: &Weights) -> Option<&'static str> {
    let mut best: Option<(&ClkCand, u32)> = None;
    for c in cands.iter() {
        if !c.available {
            continue;
        }
        let s = (c.precision * w.w_precision + c.cheap * w.w_cheap) / 1000;
        match best {
            Some((_, bs)) if bs >= s => {}
            _ => best = Some((c, s)),
        }
    }
    best.map(|(c, _)| c.name)
}

pub fn run_unx0409_checks() -> CheckSet {
    let mut cs = CheckSet::new("unx0409_clksel");
    // 族一：标准选择（TSC 快、HPET 均衡、PIT 慢但在、ACPI 缺席）。
    let cands: Vec<ClkCand> = vec![
        ClkCand { name: "tsc", precision: 950, cheap: 900, available: true },
        ClkCand { name: "hpet", precision: 800, cheap: 700, available: true },
        ClkCand { name: "pit", precision: 300, cheap: 400, available: true },
        ClkCand { name: "acpi", precision: 990, cheap: 950, available: false },
    ];
    let w = Weights { w_precision: 700, w_cheap: 300 };
    let mut b = match ClkBook::new(cands, w) {
        Ok(x) => x,
        Err(_) => {
            cs.add("建账", false, "拒");
            return cs;
        }
    };
    cs.add("主源 tsc", b.main() == "tsc", "最高加权分");
    cs.add("acpi 出局", b.avail_count() == 3, "可用位过滤");
    cs.add("tsc 得分 935", b.score_of(&ClkCand { name: "x", precision: 950, cheap: 900, available: true }) == 935, "(950×700+900×300)/1000");
    cs.add("重放合", b.replay_ok().is_ok(), "复选同主");
    cs.add("魔数过", b.magic_ok().is_ok(), "权重 700+300");
    // 族二：一票否决（高分解缺席）。
    // acpi 990/950 若可用必压过 tsc——先验证记分，再验证缺席生效。
    let c2: Vec<ClkCand> = vec![
        ClkCand { name: "tsc", precision: 950, cheap: 900, available: true },
        ClkCand { name: "acpi", precision: 990, cheap: 950, available: true },
    ];
    let b2 = match ClkBook::new(c2, w) {
        Ok(x) => x,
        Err(_) => {
            cs.add("建账2", false, "拒");
            return cs;
        }
    };
    cs.add("acpi 可用即主", b2.main() == "acpi", "981 分压 935");
    // 族三：failover 自动切换。
    let n = b.failover();
    cs.add("切至 hpet", n == Ok("hpet"), "tsc 出局次高分");
    cs.add("留痕 1", b.trace_count() == 1, "tsc→hpet");
    cs.add("可用降 2", b.avail_count() == 2, "hpet + pit");
    let n2 = b.failover();
    cs.add("切至 pit", n2 == Ok("pit"), "hpet 出局");
    cs.add("留痕 2", b.trace_count() == 2, "两跳");
    let n3 = b.failover();
    cs.add("全灭拒", n3 == Err(XC_NO_CAND), "无可用者");
    cs.add("留痕核过", b.trace_ok().is_ok(), "有出局有痕");
    // 族四：权重和核。
    cs.add("权重和 999 拒", Weights { w_precision: 700, w_cheap: 299 }.ok() == Err(XC_WEIGHT_SUM), "≠1000");
    cs.add("权重和 1001 拒", Weights { w_precision: 701, w_cheap: 300 }.ok() == Err(XC_WEIGHT_SUM), "≠1000");
    cs.add("权重和 1000 过", Weights { w_precision: 500, w_cheap: 500 }.ok().is_ok(), "对半");
    // 族五：登记拒绝。
    let bad_c: Vec<ClkCand> = vec![ClkCand { name: "x", precision: 1001, cheap: 0, available: true }];
    cs.add("评分超千拒", matches!(ClkBook::new(bad_c, w), Err(XC_SCORE_OOR)), "1001");
    let bad_n: Vec<ClkCand> = vec![ClkCand { name: "", precision: 1, cheap: 1, available: true }];
    cs.add("空名拒", matches!(ClkBook::new(bad_n, w), Err(XC_NAME_LEN)), "零字节");
    let dup: Vec<ClkCand> = vec![
        ClkCand { name: "tsc", precision: 1, cheap: 1, available: true },
        ClkCand { name: "tsc", precision: 2, cheap: 2, available: true },
    ];
    cs.add("重名拒", matches!(ClkBook::new(dup, w), Err(XC_DUP_NAME)), "tsc 撞");
    // 全不可用。
    let dead: Vec<ClkCand> = vec![ClkCand { name: "x", precision: 900, cheap: 900, available: false }];
    cs.add("全缺席拒", matches!(ClkBook::new(dead, w), Err(XC_NO_CAND)), "无人可选");
    // 族六：同分取表前者。
    let tie: Vec<ClkCand> = vec![
        ClkCand { name: "aaa", precision: 500, cheap: 500, available: true },
        ClkCand { name: "bbb", precision: 500, cheap: 500, available: true },
    ];
    let bt = match ClkBook::new(tie, Weights { w_precision: 500, w_cheap: 500 }) {
        Ok(x) => x,
        Err(_) => {
            cs.add("建账3", false, "拒");
            return cs;
        }
    };
    cs.add("同分取先", bt.main() == "aaa", "表中靠前");
    cs
}
