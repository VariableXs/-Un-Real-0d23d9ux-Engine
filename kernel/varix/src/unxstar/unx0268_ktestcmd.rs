//! unx0268_ktestcmd —— UNX-F0268 ktest 命令带过滤与重跑。
//! 锚点（CoRun Unxreal 主册）：UNX-F0268 · ktest 过滤重跑 · 目标 240 行
//! 判据原文：UNX-F0268-J1 ktest 命令支持前缀过滤（空前缀=全量）
//!           与失败重跑（只重跑上一轮红灯集）；重跑结果与首轮
//!           逐例对账，翻绿立案（flaky 嫌疑）不无声。
//! 组件：用例账（名 + 首轮红绿）+ 过滤核（前缀匹配 + 命中数）+
//!       重跑引擎（红灯集重跑 + 翻绿/仍红分类）+ flaky 立案账。
//! 职责定位：重跑不是「再试一次碰运气」——重跑后翻绿的用例是
//!   flaky 嫌疑犯，必须立案留痕（同一名单两次结果不同 = 判定
//!   不可复现，这本身就是缺陷）。仍红的才进修复队列。
//! 性能逐项分解：过滤 O(账深)；重跑 O(红灯数)；立案 O(红灯数)。
//! 诊断码段：0x20240（0x20241~0x20249 闭集；u32 高段全域独占）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 模块版本。
pub const KC_VERSION: &str = "unx0268_ktestcmd-v1";

/// 诊断段基址（0x20240 段 · 本模块独占）。
pub const KC_SEG_BASE: u32 = 0x20240;
pub const XC_EMPTY_SUITE: u32 = 0x20241; // 空用例账拒
pub const XC_DUP_CASE: u32 = 0x20242; // 用例重名
pub const XC_NO_RED: u32 = 0x20243; // 无红可重跑
pub const XC_RERUN_DRIFT: u32 = 0x20244; // 重跑集与红灯集漂移
pub const XC_NAME_EMPTY: u32 = 0x20245; // 空名拒
pub const XC_FLAKY: u32 = 0x20246; // flaky 立案存在
pub const XC_CASE_OOR: u32 = 0x20247; // 用例号越域
pub const XC_STILL_RED: u32 = 0x20248; // 仍红存在
pub const XC_PREFIX_BAD: u32 = 0x20249; // 前缀含非法符

/// 单用例行。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct KCase {
    pub id: u32,
    pub name: &'static str,
    /// 首轮红绿。
    pub first_red: bool,
}

/// 用例账。
pub struct KSuite {
    cases: Vec<KCase>,
}

impl KSuite {
    pub fn new(cases: &[KCase]) -> Result<KSuite, u32> {
        if cases.is_empty() {
            return Err(XC_EMPTY_SUITE);
        }
        for c in cases.iter() {
            if c.name.is_empty() {
                return Err(XC_NAME_EMPTY);
            }
            if c.id == 0 || c.id > 512 {
                return Err(XC_CASE_OOR);
            }
            if cases.iter().filter(|x| x.id == c.id).count() > 1 {
                return Err(XC_DUP_CASE);
            }
        }
        Ok(KSuite { cases: cases.to_vec() })
    }

    /// 前缀过滤：空前缀 = 全量；命中按账序输出。
    pub fn filter(&self, prefix: &str) -> Vec<&KCase> {
        self.cases.iter().filter(|c| c.name.starts_with(prefix)).collect()
    }

    /// 首轮红灯集。
    pub fn red_set(&self) -> Vec<u32> {
        self.cases.iter().filter(|c| c.first_red).map(|c| c.id).collect()
    }

    /// 用例深。
    pub fn len(&self) -> u32 {
        self.cases.len() as u32
    }

    /// 按号查用例。
    pub fn lookup(&self, id: u32) -> Option<&KCase> {
        self.cases.iter().find(|c| c.id == id)
    }
}

/// 重跑结果分类账。
pub struct RerunReport {
    /// 翻绿（flaky 嫌疑）。
    pub turned_green: Vec<u32>,
    /// 仍红（修复队列）。
    pub still_red: Vec<u32>,
}

/// 重跑引擎：对红灯集逐例二次判定。
/// rerun 里必须恰好是首轮红灯集（多跑/漏跑 = 集合漂移显式拒）。
pub fn rerun(suite: &KSuite, rerun_verdicts: &[(u32, bool)]) -> Result<RerunReport, u32> {
    let red = suite.red_set();
    if red.is_empty() {
        return Err(XC_NO_RED);
    }
    // 集合一致性：rerun 的号集合 == 首轮红灯集合。
    let mut r_ids: Vec<u32> = rerun_verdicts.iter().map(|(i, _)| *i).collect();
    r_ids.sort_unstable();
    r_ids.dedup();
    let mut expect = red.clone();
    expect.sort_unstable();
    if r_ids != expect {
        return Err(XC_RERUN_DRIFT);
    }
    let mut turned_green = Vec::new();
    let mut still_red = Vec::new();
    for (id, green_now) in rerun_verdicts.iter() {
        if *green_now {
            turned_green.push(*id);
        } else {
            still_red.push(*id);
        }
    }
    Ok(RerunReport { turned_green, still_red })
}

/// flaky 立案核：翻绿非空即立案（可复现红线）。
pub fn flaky_ticket(rep: &RerunReport) -> Result<(), u32> {
    if rep.turned_green.is_empty() {
        return Ok(());
    }
    Err(XC_FLAKY)
}

/// 修复队列核：仍红非空即入队。
pub fn fix_queue(rep: &RerunReport) -> Vec<u32> {
    rep.still_red.clone()
}

pub fn run_unx0268_checks() -> CheckSet {
    let mut cs = CheckSet::new("unx0268_ktestcmd");
    // 族一：用例账构造。
    let cases = vec![
        KCase { id: 1, name: "boot_a20", first_red: false },
        KCase { id: 2, name: "boot_mmap", first_red: true },
        KCase { id: 3, name: "ktest_heap", first_red: false },
        KCase { id: 4, name: "ktest_time", first_red: true },
    ];
    let suite = match KSuite::new(&cases) {
        Ok(s) => s,
        Err(_) => {
            cs.add("用例账构造", false, "被拒");
            return cs;
        }
    };
    // 族二：前缀过滤。
    cs.add("全量过滤", suite.filter("").len() == 4, "空前缀=全量");
    cs.add("boot 前缀", suite.filter("boot").len() == 2, "1+2 号");
    cs.add("ktest 前缀", suite.filter("ktest").len() == 2, "3+4 号");
    cs.add("无命中", suite.filter("zzz").is_empty(), "空集合法");
    // 族三：红灯集与重跑。
    cs.add("红灯集", suite.red_set().as_slice() == &[2u32, 4], "账序");
    let rep = match rerun(&suite, &[(2, false), (4, true)]) {
        Ok(r) => r,
        Err(_) => {
            cs.add("重跑", false, "被拒");
            return cs;
        }
    };
    cs.add("仍红 2 号", rep.still_red.as_slice() == &[2u32], "修复队列");
    cs.add("翻绿 4 号", rep.turned_green.as_slice() == &[4u32], "flaky 嫌疑");
    cs.add("flaky 立案", flaky_ticket(&rep) == Err(XC_FLAKY), "翻绿必立案");
    let fixq = fix_queue(&rep);
    cs.add("修复队列", fixq.as_slice() == &[2u32], "仍红入队");
    // 族四：全绿重跑。
    let rep2 = match rerun(&suite, &[(2, true), (4, true)]) {
        Ok(r) => r,
        Err(_) => {
            cs.add("全翻绿重跑", false, "被拒");
            return cs;
        }
    };
    cs.add("全翻绿立案", flaky_ticket(&rep2) == Err(XC_FLAKY), "整批 flaky");
    cs.add("修复队列空", fix_queue(&rep2).is_empty(), "无仍红");
    // 族五：重跑闸。
    cs.add("无红拒重跑", {
        let all_green = vec![
            KCase { id: 1, name: "a", first_red: false },
            KCase { id: 2, name: "b", first_red: false },
        ];
        match KSuite::new(&all_green) {
            Ok(s) => rerun(&s, &[]).is_err(),
            Err(_) => false,
        }
    }, "NO_RED");
    cs.add("重跑集漂移拒", rerun(&suite, &[(2, false)]).is_err(), "漏 4 号");
    cs.add("重跑多跑拒", rerun(&suite, &[(1, false), (2, false), (4, true)]).is_err(), "多 1 号");
    // 族六：构造拒。
    cs.add("空账拒", KSuite::new(&[]).is_err(), "EMPTY");
    cs.add("重名拒", KSuite::new(&[
        KCase { id: 1, name: "a", first_red: false },
        KCase { id: 2, name: "a", first_red: false },
    ]).is_err(), "DUP_CASE");
    cs.add("空名拒", KSuite::new(&[KCase { id: 1, name: "", first_red: false }]).is_err(), "NAME");
    cs.add("号越域拒", KSuite::new(&[KCase { id: 513, name: "x", first_red: false }]).is_err(), "CASE_OOR");
    cs
}
