//! veb381_bagg —— VE-F0381 VE-B 域聚合自检。
//! 锚点（CoRun Unxreal 主册）：VE-F0381 · VE-B 域聚合自检 · 目标 380 行
//! 判据原文：VE-F0381-J1 B 域聚合自检把本域各子判据集汇总成
//!           一张总卡：总卡全绿要求子卡全绿；子卡缺失（未注册）
//!           视为红项并出清单；汇总含分项计数（绿/红/缺）；
//!           汇总幂等（跑两遍结果一致）；红项清单格式化输出
//!           （「标签(明细)」逐行）。
//! 组件：子卡注册表（名 → 判据闭包）+ 汇总器（跑批 + 计数 +
//!       红项清单）+ 幂等核。
//! 职责定位：聚合自检是「域总闸」——单卡绿不等于域绿，漏跑的
//!           卡比红卡更危险（静默缺口）；缺失视为红是零信任
//!           原则：没证明你绿，就当你是红；红项清单让运维一
//!           眼看到修哪，不用再二分重跑。
//! 性能逐项分解：汇总 O(子卡数)；清单 O(红项数)。
//! 诊断码段：0x304C0（0x304C1~0x304C9 闭集）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::string::ToString;
use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 模块版本。
pub const AG_VERSION: &str = "veb381_bagg-v1";

/// 诊断段基址（0x304C0 段 · 本模块独占）。
pub const AG_SEG_BASE: u32 = 0x304C0;
pub const XC_NO_SUB: u32 = 0x304C1; // 无子卡可聚
pub const XC_DUP_SUB: u32 = 0x304C2; // 子卡重名
pub const XC_MAGIC: u32 = 0x304C3; // 账魔数破
pub const XC_MISSING: u32 = 0x304C4; // 子卡缺失视为红
pub const XC_NOT_IDEM: u32 = 0x304C5; // 幂等破
pub const XC_EMPTY_LINE: u32 = 0x304C6; // 清单空行异常
pub const XC_REG_OOR: u32 = 0x304C7; // 注册表越域

/// 汇总统计。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AggStats {
    pub green: u32,
    pub red: u32,
    pub missing: u32,
}

impl AggStats {
    /// 总卡全绿：红 0 且缺 0。
    pub fn all_green(&self) -> bool {
        self.red == 0 && self.missing == 0
    }
    /// 子卡总数。
    pub fn total(&self) -> u32 {
        self.green + self.red + self.missing
    }
}

/// 子卡注册表条目。
struct SubSlot {
    name: &'static str,
    /// None = 缺失占位（未实现/未注册）。
    body: Option<fn() -> CheckSet>,
}

/// 汇总器。
pub struct Aggregator {
    subs: Vec<SubSlot>,
    magic: u32,
}

impl Aggregator {
    /// 建。
    pub fn new() -> Aggregator {
        Aggregator { subs: Vec::new(), magic: 0x04C0_A66E }
    }

    /// 注册子卡（实体）。
    pub fn put(&mut self, name: &'static str, f: fn() -> CheckSet) -> Result<(), u32> {
        if self.magic != 0x04C0_A66E {
            return Err(XC_MAGIC);
        }
        for sl in self.subs.iter() {
            if sl.name == name {
                return Err(XC_DUP_SUB);
            }
        }
        self.subs.push(SubSlot { name, body: Some(f) });
        Ok(())
    }

    /// 注册缺失占位（知道该有但没实现）。
    pub fn put_missing(&mut self, name: &'static str) -> Result<(), u32> {
        for sl in self.subs.iter() {
            if sl.name == name {
                return Err(XC_DUP_SUB);
            }
        }
        self.subs.push(SubSlot { name, body: None });
        Ok(())
    }

    /// 汇总：逐卡跑，缺失记 missing。
    pub fn aggregate(&self) -> Result<(AggStats, Vec<&'static str>), u32> {
        if self.magic != 0x04C0_A66E {
            return Err(XC_MAGIC);
        }
        if self.subs.is_empty() {
            return Err(XC_NO_SUB);
        }
        let mut st = AggStats { green: 0, red: 0, missing: 0 };
        let mut reds: Vec<&'static str> = Vec::new();
        for sl in self.subs.iter() {
            match sl.body {
                Some(f) => {
                    let cs = f();
                    if cs.all_passed() {
                        st.green += 1;
                    } else {
                        st.red += 1;
                        reds.push(sl.name);
                    }
                }
                None => {
                    st.missing += 1;
                    reds.push(sl.name);
                }
            }
        }
        Ok((st, reds))
    }

    /// 幂等核：两遍结果一致。
    pub fn idempotent(&self) -> Result<bool, u32> {
        let a = self.aggregate()?;
        let b = self.aggregate()?;
        if a.0 != b.0 || a.1 != b.1 {
            return Err(XC_NOT_IDEM);
        }
        Ok(true)
    }

    /// 红项清单格式化：「名(缺失)」或「名(N红)」逐行。
    pub fn red_text(&self) -> Result<alloc::string::String, u32> {
        let (_, reds) = self.aggregate()?;
        let mut out = alloc::string::String::new();
        for r in reds.iter() {
            let mut is_missing = false;
            for sl in self.subs.iter() {
                if sl.name == *r && sl.body.is_none() {
                    is_missing = true;
                }
            }
            if is_missing {
                out.push_str(r);
                out.push_str("(缺失)\n");
            } else {
                out.push_str(r);
                out.push_str("(红)\n");
            }
        }
        if out.is_empty() {
            return Err(XC_EMPTY_LINE);
        }
        Ok(out)
    }

    /// 子卡数。
    pub fn sub_count(&self) -> usize {
        self.subs.len()
    }
}

/// 测试子卡 A（全绿）。
fn sub_green_a() -> CheckSet {
    let mut s = CheckSet::new("SUB-A");
    s.add("a1", 1 + 1 == 2, "真");
    s.add("a2", true, "真");
    s
}

/// 测试子卡 B（有一红）。
fn sub_red_b() -> CheckSet {
    let mut s = CheckSet::new("SUB-B");
    s.add("b1", true, "真");
    s.add("b2", false, "假");
    s
}

/// 测试子卡 C（全绿）。
fn sub_green_c() -> CheckSet {
    let mut s = CheckSet::new("SUB-C");
    s.add("c1", 3 * 3 == 9, "真");
    s
}

pub fn run_veb381_checks() -> CheckSet {
    let mut s = CheckSet::new("VE-F0381");
    // 族一：正常汇总。
    let mut ag = Aggregator::new();
    let _ = ag.put("sub-a", sub_green_a);
    let _ = ag.put("sub-b", sub_red_b);
    let _ = ag.put("sub-c", sub_green_c);
    s.add("子卡 3", ag.sub_count() == 3, "注册");
    let agg = ag.aggregate();
    s.add("汇总统计", matches!(&agg, Ok((st, _)) if st.green == 2 && st.red == 1 && st.missing == 0), "2绿1红");
    s.add("总卡不全绿", matches!(&agg, Ok((st, _)) if !st.all_green()), "有红");
    s.add("红项含 B", matches!(&agg, Ok((_, rs)) if rs.first() == Some(&"sub-b")), "点名");
    s.add("总数核", matches!(&agg, Ok((st, _)) if st.total() == 3), "守恒");
    // 族二：缺失视为红。
    let mut mg = Aggregator::new();
    let _ = mg.put("sub-a", sub_green_a);
    let _ = mg.put_missing("sub-x");
    let aggm = mg.aggregate();
    s.add("缺失计 1", matches!(&aggm, Ok((st, _)) if st.missing == 1 && st.red == 0), "缺≠红但挡绿");
    s.add("缺卡不全绿", matches!(&aggm, Ok((st, _)) if !st.all_green()), "零信任");
    s.add("缺卡点名", matches!(&aggm, Ok((_, rs)) if rs.first() == Some(&"sub-x")), "x 入列");
    // 族三：清单格式。
    let txt = ag.red_text();
    s.add("清单非空", txt.is_ok(), "有红");
    s.add("清单含名", matches!(&txt, Ok(t) if t.contains("sub-b")), "点名");
    s.add("清单含括号", matches!(&txt, Ok(t) if t.contains("(红)")), "格式");
    let txtm = mg.red_text();
    s.add("缺失清单", matches!(&txtm, Ok(t) if t.contains("sub-x(缺失)")), "缺失标注");
    // 全绿清单为空 → Err。
    let mut gg = Aggregator::new();
    let _ = gg.put("sub-a", sub_green_a);
    s.add("全绿无清单", gg.red_text() == Err(XC_EMPTY_LINE), "无红可列");
    // 族四：幂等。
    s.add("幂等过", ag.idempotent() == Ok(true), "两遍一致");
    s.add("缺失幂等", mg.idempotent() == Ok(true), "缺失也稳");
    // 族五：注册约束。
    let mut d = Aggregator::new();
    let _ = d.put("dup", sub_green_a);
    s.add("重名拒", d.put("dup", sub_green_c) == Err(XC_DUP_SUB), "主键");
    s.add("缺失重名拒", d.put_missing("dup") == Err(XC_DUP_SUB), "同键");
    let empty = Aggregator::new();
    s.add("空聚拒", empty.aggregate() == Err(XC_NO_SUB), "无卡");
    s.add("魔数核", AG_SEG_BASE == 0x304C0, "段基址");
    s
}
