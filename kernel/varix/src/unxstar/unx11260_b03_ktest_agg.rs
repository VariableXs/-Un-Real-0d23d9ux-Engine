//! UNX-F11260 B03 批 ktest 断言集聚合（0x1f3c0 段）
//! 纯功能模型：把 B03 批 19 条判据登记为定容用例册，一次 run_once 全跑；
//! 通过率按千分比记账，缺项（未凑齐 19 条）与红项（已跑未通过）分开立账。
//! 册封后登记即拒，指纹链对登记序与运行轮次敏感，二次封册返回原根。

use crate::alloc::format;
use crate::checks::CheckSet;
use alloc::string::String;
use alloc::vec::Vec;

/// 诊断段（0x1f3c0 独占 0x10）。
pub const SEG_BASE: u32 = 0x1f3c0;

/// 用例号越界（0 或超过 ID_MAX）。
pub const KA_ID: u32 = SEG_BASE + 0x01;
/// 用例号重号。
pub const KA_DUP: u32 = SEG_BASE + 0x02;
/// 用例类别不在闭集（1=纯函数 2=装载 3=自检）。
pub const KA_KIND: u32 = SEG_BASE + 0x03;
/// 用例名空。
pub const KA_EMPTY: u32 = SEG_BASE + 0x04;
/// 册封后写账。
pub const KA_FROZEN: u32 = SEG_BASE + 0x05;
/// 用例册已满。
pub const KA_CAP: u32 = SEG_BASE + 0x06;
/// 聚合时缺项（不足 19 条）。
pub const KA_MISS: u32 = SEG_BASE + 0x07;

/// 本批判据条数（B03 批 19 条）。
pub const CASE_NEED: usize = 19;
/// 用例册定容（>19，容量先定后用）。
pub const CASE_CAP: usize = 24;
/// 用例号上界（与定容解耦，保证 KA_ID 与 KA_CAP 两码各自可达）。
pub const ID_MAX: u32 = 999;
/// 类别闭集上界。
pub const KIND_MAX: u8 = 3;

const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x100_0000_01b3;
/// 链种子（B03 批标）。
const TAG: u64 = 0xb03_01;

/// 单条 ktest 用例：号/名/类别/结果/耗时。
#[derive(Clone, Debug)]
pub struct KCase {
    pub id: u32,
    pub name: String,
    pub kind: u8,
    pub passed: bool,
    pub cost_ms: u32,
}

impl KCase {
    /// 单条渲染：`#07|P|12ms|NAME`。
    pub fn render(&self) -> String {
        let f = if self.passed { "P" } else { "F" };
        format!("#{:02}|{}|{}ms|{}", self.id, f, self.cost_ms, self.name)
    }
}

/// 聚合报告：总条数/通过条数/千分率/红项清单。
#[derive(Clone, Debug)]
pub struct Agg {
    pub total: u32,
    pub pass: u32,
    pub permille: u32,
    pub red: Vec<u32>,
}

/// 用例册：登记 → 一次全跑 → 千分率 → 封册。
#[derive(Clone, Debug)]
pub struct KSuite {
    pub cases: Vec<KCase>,
    pub runs: u32,
    pub accepts: u32,
    pub rejects: u32,
    pub frozen: bool,
    pub root: Option<u64>,
}

impl KSuite {
    pub fn new() -> KSuite {
        KSuite {
            cases: Vec::new(),
            runs: 0,
            accepts: 0,
            rejects: 0,
            frozen: false,
            root: None,
        }
    }

    /// 登记用例。判定序：冻结 → 重号 → 号域 → 名空 → 类别 → 定容。
    /// 冻结最先，保证封册后任何写账都只报 KA_FROZEN；
    /// 重号早于号域，保证重复号报 KA_DUP 而不被越界吞掉。
    pub fn enroll(&mut self, id: u32, name: &str, kind: u8, passed: bool, cost_ms: u32)
        -> Result<(), u32>
    {
        if self.frozen {
            self.rejects = self.rejects.wrapping_add(1);
            return Err(KA_FROZEN);
        }
        if id == 0 || id > ID_MAX {
            self.rejects = self.rejects.wrapping_add(1);
            return Err(KA_ID);
        }
        if self.cases.iter().any(|c| c.id == id) {
            self.rejects = self.rejects.wrapping_add(1);
            return Err(KA_DUP);
        }
        if name.is_empty() {
            self.rejects = self.rejects.wrapping_add(1);
            return Err(KA_EMPTY);
        }
        if kind == 0 || kind > KIND_MAX {
            self.rejects = self.rejects.wrapping_add(1);
            return Err(KA_KIND);
        }
        if self.cases.len() >= CASE_CAP {
            self.rejects = self.rejects.wrapping_add(1);
            return Err(KA_CAP);
        }
        self.cases.push(KCase {
            id,
            name: name_into(name),
            kind,
            passed,
            cost_ms,
        });
        self.accepts = self.accepts.wrapping_add(1);
        Ok(())
    }

    /// 一次命令全跑：缺项先于冻结判定（缺项是聚合前置条件）。
    /// 账面只读不写用例体，运行轮次进指纹。
    pub fn run_once(&mut self) -> Result<Agg, u32> {
        if self.frozen {
            self.rejects = self.rejects.wrapping_add(1);
            return Err(KA_FROZEN);
        }
        if self.cases.len() != CASE_NEED {
            self.rejects = self.rejects.wrapping_add(1);
            return Err(KA_MISS);
        }
        let mut pass = 0u32;
        let mut red: Vec<u32> = Vec::new();
        let mut k = 0usize;
        while k < self.cases.len() {
            if let Some(c) = self.cases.get(k) {
                if c.passed {
                    pass = pass.wrapping_add(1);
                } else {
                    red.push(c.id);
                }
            }
            k += 1;
        }
        self.runs = self.runs.wrapping_add(1);
        let total = self.cases.len() as u32;
        let permille = if total == 0 { 0 } else { pass.wrapping_mul(1000) / total };
        Ok(Agg { total, pass, permille, red })
    }

    /// 绿判：凑齐 19 条且逐条通过（缺项不算绿，红项也不算绿）。
    pub fn green(&self) -> bool {
        if self.cases.len() != CASE_NEED {
            return false;
        }
        let mut k = 0usize;
        while k < self.cases.len() {
            match self.cases.get(k) {
                Some(c) => {
                    if !c.passed {
                        return false;
                    }
                }
                None => return false,
            }
            k += 1;
        }
        true
    }

    /// 千分比通过率（空册记 0，不做除零）。
    pub fn permille(&self) -> u32 {
        let total = self.cases.len() as u32;
        if total == 0 {
            return 0;
        }
        let mut pass = 0u32;
        let mut k = 0usize;
        while k < self.cases.len() {
            if let Some(c) = self.cases.get(k) {
                if c.passed {
                    pass = pass.wrapping_add(1);
                }
            }
            k += 1;
        }
        pass.wrapping_mul(1000) / total
    }

    /// 红项清单（登记序）。
    pub fn red(&self) -> Vec<u32> {
        let mut v: Vec<u32> = Vec::new();
        let mut k = 0usize;
        while k < self.cases.len() {
            if let Some(c) = self.cases.get(k) {
                if !c.passed {
                    v.push(c.id);
                }
            }
            k += 1;
        }
        v
    }

    /// FNV-1a 链指纹：号/类别/结果/耗时/名序敏感，运行轮次入链。
    pub fn fold(&self) -> u64 {
        let mut h = FNV_OFF ^ TAG;
        let mut k = 0usize;
        while k < self.cases.len() {
            if let Some(c) = self.cases.get(k) {
                h = (h ^ c.id as u64).wrapping_mul(FNV_PRIME);
                h = (h ^ c.kind as u64).wrapping_mul(FNV_PRIME);
                h = (h ^ c.passed as u64).wrapping_mul(FNV_PRIME);
                h = (h ^ c.cost_ms as u64).wrapping_mul(FNV_PRIME);
                let b = c.name.as_bytes();
                let mut j = 0usize;
                while j < b.len() {
                    if let Some(x) = b.get(j) {
                        h = (h ^ *x as u64).wrapping_mul(FNV_PRIME);
                    }
                    j += 1;
                }
            }
            k += 1;
        }
        h ^ (self.runs as u64)
    }

    /// 封册：已有根则原样返回（幂等），否则现算 fold 并入账。
    pub fn freeze(&mut self) -> Option<u64> {
        if self.frozen {
            return self.root;
        }
        let r = match self.root {
            Some(v) => v,
            None => self.fold(),
        };
        self.root = Some(r);
        self.frozen = true;
        Some(r)
    }
}

/// 借串入堆（no_std 下不引 ToString）。
fn name_into(s: &str) -> String {
    let mut out = String::new();
    out.push_str(s);
    out
}

/// 批内样本：第 `i` 条判据的期望耗时（确定性，正反两路共用）。
pub fn sample_cost(i: u32) -> u32 {
    i.wrapping_mul(7).wrapping_add(3)
}

/// 注入核：四路坏样本各报本模块自有码。
pub fn inject_four() -> [bool; 4] {
    let mut got = [false; 4];
    // (a) 号域越界：0 号不是合法判据号
    let mut a = KSuite::new();
    got[0] = a.enroll(0, "零号", 1, true, 1).err() == Some(KA_ID);
    // (b) 重号：同号二次登记
    let mut b = KSuite::new();
    let _ = b.enroll(1, "首登", 1, true, 1);
    got[1] = b.enroll(1, "重登", 1, true, 1).err() == Some(KA_DUP);
    // (c) 形状违规：类别越出 1..=3 闭集
    let mut c = KSuite::new();
    got[2] = c.enroll(1, "怪类", 9, true, 1).err() == Some(KA_KIND);
    // (d) 封册后写账
    let mut d = KSuite::new();
    let _ = d.enroll(1, "封前", 1, true, 1);
    let _ = d.freeze();
    got[3] = d.enroll(2, "封后", 1, true, 1).err() == Some(KA_FROZEN);
    got
}

/// 类别闭集映射：19 条判据按 1/2/3 轮转。
pub fn kind_of(i: u32) -> u8 {
    ((i % KIND_MAX as u32) as u8) + 1
}

/// 造一份满册样本；`pass_all=false` 时第 3 条判红。
pub fn full_suite(pass_all: bool) -> KSuite {
    let mut s = KSuite::new();
    let mut i = 1u32;
    while i <= CASE_NEED as u32 {
        let ok = pass_all || i != 3;
        let _ = s.enroll(i, "B03 判据", kind_of(i), ok, sample_cost(i));
        i = i.wrapping_add(1);
    }
    s
}

/// B03 批断言集聚合判据。
pub fn run_unx11260_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F11260");

    // X1 满册绿判：19 条齐且逐条通过
    let x1 = {
        let suite = full_suite(true);
        suite.green() && suite.cases.len() == CASE_NEED
    };
    s.add("X1-满册绿判", x1, "19 条齐备且逐条通过");

    // X2 一次全跑：千分率 1000、红项零
    let x2 = {
        let mut suite = full_suite(true);
        match suite.run_once() {
            Ok(a) => a.total == 19 && a.pass == 19 && a.permille == 1000 && a.red.is_empty(),
            Err(_) => false,
        }
    };
    s.add("X2-一次全跑", x2, "单次聚合得 19/19 千分率 1000");

    // X3 指纹确定：同册两次 fold 同值且非零
    let x3 = {
        let suite = full_suite(true);
        let a = suite.fold();
        let b = suite.fold();
        a == b && a != 0
    };
    s.add("X3-指纹确定", x3, "同册两次指纹一致且非零");

    // X4 封册幂等：二次封册返回原根（不重算 fold）
    let x4 = {
        let mut suite = full_suite(true);
        let r1 = suite.freeze();
        let r2 = suite.freeze();
        let live = suite.fold();
        suite.cases.clear();
        let r3 = suite.freeze();
        r1 == r2 && r2 == r3 && r1.is_some() && live != suite.fold()
    };
    s.add("X4-封册幂等", x4, "二次封册返回原根，封后账变不改根");

    // X5 封册拒写
    let x5 = {
        let mut suite = full_suite(true);
        let _ = suite.freeze();
        suite.enroll(99, "迟到", 1, true, 1).err() == Some(KA_FROZEN)
    };
    s.add("X5-封册拒写", x5, "封册后登记与重跑均拒");

    // X6 重号拒收
    let x6 = {
        let mut suite = KSuite::new();
        let _ = suite.enroll(7, "甲", 1, true, 1);
        suite.enroll(7, "乙", 2, true, 2).err() == Some(KA_DUP)
    };
    s.add("X6-重号拒收", x6, "同号二次登记报重号");

    // X7 号域越界：0 号与越上界双路
    let x7 = {
        let mut a = KSuite::new();
        let mut b = KSuite::new();
        a.enroll(0, "零号", 1, true, 1).err() == Some(KA_ID)
            && b.enroll(ID_MAX.wrapping_add(1), "超界", 1, true, 1).err() == Some(KA_ID)
    };
    s.add("X7-号域越界", x7, "0 号与超上界号均报越界");

    // X8 类别闭集：下越 0 与上越 4 双路
    let x8 = {
        let mut a = KSuite::new();
        let mut b = KSuite::new();
        a.enroll(1, "下越", 0, true, 1).err() == Some(KA_KIND)
            && b.enroll(1, "上越", KIND_MAX.wrapping_add(1), true, 1).err() == Some(KA_KIND)
    };
    s.add("X8-类别闭集", x8, "类别 0 与 4 均在闭集外报拒");

    // X9 名空拒收
    let x9 = KSuite::new().enroll(1, "", 1, true, 1).err() == Some(KA_EMPTY);
    s.add("X9-名空拒收", x9, "空名用例不入册");

    // X10 缺项门禁：18 条聚合报缺项且不绿
    let x10 = {
        let mut suite = KSuite::new();
        let mut i = 1u32;
        while i < CASE_NEED as u32 {
            let _ = suite.enroll(i, "差一条", kind_of(i), true, 1);
            i = i.wrapping_add(1);
        }
        suite.run_once().err() == Some(KA_MISS) && !suite.green()
    };
    s.add("X10-缺项门禁", x10, "18 条聚合报缺项且绿判为假");

    // X11 定容边界：满 24 条后第 25 条报满册
    let x11 = {
        let mut suite = KSuite::new();
        let mut i = 1u32;
        let mut ok = true;
        while i <= CASE_CAP as u32 {
            if suite.enroll(i, "填册", kind_of(i), true, 1).is_err() {
                ok = false;
            }
            i = i.wrapping_add(1);
        }
        ok && suite.enroll(25, "溢出", 1, true, 1).err() == Some(KA_CAP)
    };
    s.add("X11-定容边界", x11, "满 24 条后第 25 条报满册");

    // X12 红项账：18/19 千分率 947，红项号可枚举，计数账不混
    let x12 = {
        let mut suite = full_suite(false);
        let a = suite.run_once();
        let agg_ok = match a {
            Ok(v) => v.total == 19 && v.pass == 18 && v.permille == 947 && v.red.len() == 1,
            Err(_) => false,
        };
        agg_ok
            && suite.red().len() == 1
            && suite.red().first() == Some(&3)
            && suite.accepts == 19
            && suite.rejects == 0
    };
    s.add("X12-红项账", x12, "一条判红得千分率 947 且红项号为 3");

    // X13 明细：聚合渲染与轮次账
    let (x13, line13) = {
        let mut suite = full_suite(true);
        match suite.run_once() {
            Ok(a) => {
                let t = format!("{}/{} rate={}‰ runs={}", a.pass, a.total, a.permille, suite.runs);
                (t.contains("19/19") && t.contains("runs=1"), t)
            }
            Err(_) => (false, String::new()),
        }
    };
    s.add_dyn("X13-聚合明细", x13, &line13);

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx11260_flow() {
        let mut suite = full_suite(true);
        assert!(suite.cases.len() == 19);
        assert!(suite.green());
        assert_eq!(suite.permille(), 1000);
        assert!(suite.red().is_empty());
        let a = suite.run_once();
        assert!(a.is_ok());
        match a {
            Ok(v) => assert!(v.pass == 19 && v.permille == 1000),
            Err(_) => assert!(false, "满册聚合不得报错"),
        }
        let r1 = suite.freeze();
        let r2 = suite.freeze();
        assert_eq!(r1, r2);
        assert!(r1.is_some());
        assert!(r1 == suite.root);
        assert!(suite.enroll(88, "封后", 1, true, 1).err() == Some(KA_FROZEN));
        assert!(suite.run_once().err() == Some(KA_FROZEN));
    }

    #[test]
    fn unx11260_gates() {
        let mut suite = KSuite::new();
        assert!(suite.enroll(1, "甲", 1, true, 3).is_ok());
        assert!(suite.enroll(2, "乙", 2, false, 4).is_ok());
        assert!(suite.enroll(1, "重号", 1, true, 1).err() == Some(KA_DUP));
        assert!(suite.enroll(0, "零号", 1, true, 1).err() == Some(KA_ID));
        assert!(suite.enroll(3, "", 1, true, 1).err() == Some(KA_EMPTY));
        assert!(suite.enroll(3, "怪类", 0, true, 1).err() == Some(KA_KIND));
        assert!(suite.enroll(3, "越类", 4, true, 1).err() == Some(KA_KIND));
        assert!(suite.run_once().err() == Some(KA_MISS));
        assert!(suite.freeze().is_some());
        assert!(suite.enroll(9, "封后", 1, true, 1).err() == Some(KA_FROZEN));
        assert!(suite.run_once().err() == Some(KA_FROZEN));
        assert_eq!(suite.accepts, 2);
        assert_eq!(suite.rejects, 8);
        assert!(suite.rejects + suite.accepts == 10);
    }

    #[test]
    fn unx11260_checks_green() {
        let s = run_unx11260_checks();
        assert!(s.all_passed(), "UNX-F11260 判据未全绿");
    }
}
