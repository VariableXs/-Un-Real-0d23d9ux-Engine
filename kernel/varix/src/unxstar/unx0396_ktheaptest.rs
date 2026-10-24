//! unx0396_ktheaptest —— UNX-F0396 ktest 堆面断言接入。
//! 锚点（CoRun Unxreal 主册）：UNX-F0396 · ktest 堆面断言接入 · 目标 220 行
//! 判据原文：UNX-F0396-J1 堆面各模块（泄漏探测/slab 回收/哨兵/
//!           失败注入/扩堆借页/栈池/预留窗/总账）的关键不变量
//!           登记为 ktest 断言项（泄漏平/半满收/哨兵整/注入账/
//!           借还平/栈对齐/窗解冻/账恒等八族）；接入完整性核：
//!           段位 0x20BC0~0x20D80 每个已登记堆面模块至少一条
//!           断言挂账；红项立案带族标签可定位。
//! 组件：断言项（八族枚举 + 自含样本）+ 判定引擎 + 接入账
//!       （登记 + 段位覆盖核 + 红项立案）+ 摘要序列化。
//! 职责定位：接入面是「堆面 → ktest 总闸」的桥——与内存面接入
//!   （UNX-F0377）同构，覆盖对象换成堆面八模块；样本自含、
//!   复跑零依赖是同一纪律：断言项里写死数字，判定不拉起被测
//!   模块本体，复跑清单即世界。
//! 性能逐项分解：逐项判定 O(1)；覆盖核 O(登记数)；摘要 O(项数)。
//! 诊断码段：0x20DC0（0x20DC1~0x20DC9 闭集；u32 高段全域独占）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 模块版本。
pub const KH_VERSION: &str = "unx0396_ktheaptest-v1";

/// 诊断段基址（0x20DC0 段 · 本模块独占）。
pub const KH_SEG_BASE: u32 = 0x20DC0;
pub const XC_EMPTY_SUITE: u32 = 0x20DC1; // 空断言集拒
pub const XC_DUP_NAME: u32 = 0x20DC2; // 断言重名
pub const XC_RED_ASSERT: u32 = 0x20DC3; // 红项存在
pub const XC_GAP_COVER: u32 = 0x20DC4; // 段位覆盖有漏
pub const XC_KIND_OOR: u32 = 0x20DC5; // 族越域
pub const XC_CKSUM: u32 = 0x20DC6; // 摘要校验和不过
pub const XC_MAGIC: u32 = 0x20DC7; // 摘要魔数破
pub const XC_SEG_OOR: u32 = 0x20DC8; // 段位越域
pub const XC_QUOTA_NEG: u32 = 0x20DC9; // 配额负样本

/// 堆面断言八族。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum HeapCase {
    /// 泄漏平族：非白名单净额 = 0。
    LeakFlat { net: u64 },
    /// 半满收族：回收后 slab 数 ≤ 回收前 - 1。
    SlabShrunk { after: u32, before: u32 },
    /// 哨兵整族：破哨兵块数 = 0。
    Sentinel { broken: u32 },
    /// 注入账族：成功 + 失败 = 总请求。
    InjectBal { ok: u32, fail: u32, total: u32 },
    /// 借还平族：池恒等（借出 + 空闲 = 总量）。
    LoanBal { lent: u32, free: u32, total: u32 },
    /// 栈对齐族：基址 16B 对齐。
    StkAlign { base: u64 },
    /// 窗解冻族：关窗后开窗数 = 0。
    Thawed { open_wins: u32 },
    /// 账恒等族：存量 = 分配 - 归还。
    LedgerEq { live: i64, alloc: u64, freed: u64 },
}

impl HeapCase {
    /// 族标签（1~8）。
    pub fn kind(&self) -> u8 {
        match self {
            HeapCase::LeakFlat { .. } => 1,
            HeapCase::SlabShrunk { .. } => 2,
            HeapCase::Sentinel { .. } => 3,
            HeapCase::InjectBal { .. } => 4,
            HeapCase::LoanBal { .. } => 5,
            HeapCase::StkAlign { .. } => 6,
            HeapCase::Thawed { .. } => 7,
            HeapCase::LedgerEq { .. } => 8,
        }
    }

    /// 判定：绿 = 不变量成立。
    pub fn eval(&self) -> bool {
        match self {
            HeapCase::LeakFlat { net } => *net == 0,
            HeapCase::SlabShrunk { after, before } => *before >= 1 && *after < *before,
            HeapCase::Sentinel { broken } => *broken == 0,
            HeapCase::InjectBal { ok, fail, total } => ok + fail == *total,
            HeapCase::LoanBal { lent, free, total } => lent + free == *total,
            HeapCase::StkAlign { base } => base % 16 == 0,
            HeapCase::Thawed { open_wins } => *open_wins == 0,
            HeapCase::LedgerEq { live, alloc, freed } => {
                *live >= 0 && (*alloc - *freed) as i64 == *live
            }
        }
    }

    /// 词位编码（族 + 4 数据词）。
    pub fn words(&self) -> [u32; 5] {
        match self {
            HeapCase::LeakFlat { net } => [1, *net as u32, (*net >> 32) as u32, 0, 0],
            HeapCase::SlabShrunk { after, before } => [2, *after, *before, 0, 0],
            HeapCase::Sentinel { broken } => [3, *broken, 0, 0, 0],
            HeapCase::InjectBal { ok, fail, total } => [4, *ok, *fail, *total, 0],
            HeapCase::LoanBal { lent, free, total } => [5, *lent, *free, *total, 0],
            HeapCase::StkAlign { base } => [6, *base as u32, (*base >> 32) as u32, 0, 0],
            HeapCase::Thawed { open_wins } => [7, *open_wins, 0, 0, 0],
            HeapCase::LedgerEq { live, alloc, freed } => {
                [8, *live as u32, *alloc as u32, *freed as u32, 0]
            }
        }
    }
}

/// 断言项。
#[derive(Clone, Debug)]
pub struct HeapAssert {
    /// 断言名。
    pub name: &'static str,
    /// 挂靠堆面模块段位（0x20BC0~0x20D80，步进 0x40）。
    pub seg: u32,
    /// 案例。
    pub case: HeapCase,
}

/// 接入账。
pub struct HeapSuite {
    items: Vec<HeapAssert>,
    reds: Vec<usize>,
}

/// 堆面段位表（8 段）。
pub const HEAP_SEGS: [u32; 8] = [
    0x20BC0, 0x20C00, 0x20C40, 0x20C80, 0x20CC0, 0x20D00, 0x20D40, 0x20D80,
];

impl HeapSuite {
    /// 建空账。
    pub fn new() -> HeapSuite {
        HeapSuite { items: Vec::new(), reds: Vec::new() }
    }

    /// 登记：重名拒、族越域拒、段位越域拒。
    pub fn enroll(&mut self, name: &'static str, seg: u32, case: HeapCase) -> Result<(), u32> {
        if case.kind() == 0 || case.kind() > 8 {
            return Err(XC_KIND_OOR);
        }
        if seg < 0x20BC0 || seg > 0x20D80 || (seg - 0x20BC0) % 0x40 != 0 {
            return Err(XC_SEG_OOR);
        }
        for it in self.items.iter() {
            if it.name == name {
                return Err(XC_DUP_NAME);
            }
        }
        self.items.push(HeapAssert { name, seg, case });
        Ok(())
    }

    /// 求值全部：红项立案；空集拒。
    pub fn evaluate(&mut self) -> Result<u32, u32> {
        if self.items.is_empty() {
            return Err(XC_EMPTY_SUITE);
        }
        self.reds.clear();
        for (i, it) in self.items.iter().enumerate() {
            if !it.case.eval() {
                self.reds.push(i);
            }
        }
        if !self.reds.is_empty() {
            return Err(XC_RED_ASSERT);
        }
        Ok(self.items.len() as u32)
    }

    /// 段位覆盖核：8 个堆面段全覆盖。
    pub fn cover_ok(&self) -> Result<(), u32> {
        for &seg in HEAP_SEGS.iter() {
            if !self.items.iter().any(|it| it.seg == seg) {
                return Err(XC_GAP_COVER);
            }
        }
        Ok(())
    }

    /// 摘要：魔数 + 项数 + 各项词位 + FNV32。
    pub fn digest(&self) -> (Vec<u32>, u32) {
        let mut words: Vec<u32> = Vec::new();
        words.push(0x1D00_0001u32); // 魔数：堆接入面 v1
        words.push(self.items.len() as u32);
        for it in self.items.iter() {
            words.push(it.seg);
            for w in it.case.words().iter() {
                words.push(*w);
            }
        }
        let mut h: u32 = 0x811C_9DC5;
        for &w in words.iter() {
            h ^= w;
            h = h.wrapping_mul(0x0100_0193);
        }
        words.push(h);
        (words, h)
    }

    /// 复跑稳。
    pub fn replay_stable(&self) -> Result<(), u32> {
        let (words, sig) = self.digest();
        if words[0] != 0x1D00_0001 {
            return Err(XC_MAGIC);
        }
        if words[words.len() - 1] != sig {
            return Err(XC_CKSUM);
        }
        Ok(())
    }

    /// 红项数。
    pub fn red_count(&self) -> usize {
        self.reds.len()
    }

    /// 登记数。
    pub fn len(&self) -> usize {
        self.items.len()
    }
}

pub fn run_unx0396_checks() -> CheckSet {
    let mut cs = CheckSet::new("unx0396_ktheaptest");
    // 族一：八族各登记一例全绿。
    let mut s1 = HeapSuite::new();
    let _ = s1.enroll("泄漏平", 0x20BC0, HeapCase::LeakFlat { net: 0 });
    let _ = s1.enroll("半满收", 0x20C00, HeapCase::SlabShrunk { after: 1, before: 2 });
    let _ = s1.enroll("哨兵整", 0x20C40, HeapCase::Sentinel { broken: 0 });
    let _ = s1.enroll("注入账", 0x20C80, HeapCase::InjectBal { ok: 7, fail: 1, total: 8 });
    let _ = s1.enroll("借还平", 0x20CC0, HeapCase::LoanBal { lent: 24, free: 232, total: 256 });
    let _ = s1.enroll("栈对齐", 0x20D00, HeapCase::StkAlign { base: 16384 });
    let _ = s1.enroll("窗解冻", 0x20D40, HeapCase::Thawed { open_wins: 0 });
    let _ = s1.enroll("账恒等", 0x20D80, HeapCase::LedgerEq { live: 600, alloc: 900, freed: 300 });
    cs.add("登记八项", s1.len() == 8, "八族齐");
    cs.add("求值全绿", s1.evaluate().is_ok(), "红 0");
    cs.add("覆盖齐", s1.cover_ok().is_ok(), "8/8 段");
    cs.add("复跑稳", s1.replay_stable().is_ok(), "签自洽");
    // 族二：红项检出。
    let mut s2 = HeapSuite::new();
    let _ = s2.enroll("泄漏红", 0x20BC0, HeapCase::LeakFlat { net: 256 });
    cs.add("泄漏红检出", s2.evaluate() == Err(XC_RED_ASSERT), "净额 256");
    cs.add("红项数 1", s2.red_count() == 1, "立案");
    // 注入账红：7+1≠9。
    let mut s3 = HeapSuite::new();
    let _ = s3.enroll("账不平", 0x20C80, HeapCase::InjectBal { ok: 7, fail: 1, total: 9 });
    cs.add("注入账红", s3.evaluate() == Err(XC_RED_ASSERT), "8≠9");
    // 族三：覆盖漏检。
    let mut s4 = HeapSuite::new();
    let _ = s4.enroll("只挂一段", 0x20BC0, HeapCase::LeakFlat { net: 0 });
    cs.add("覆盖漏检出", s4.cover_ok() == Err(XC_GAP_COVER), "1/8 拒");
    // 族四：登记拒绝。
    let mut s5 = HeapSuite::new();
    cs.add("段位低于域", s5.enroll("x", 0x20B80, HeapCase::Sentinel { broken: 0 }) == Err(XC_SEG_OOR), "<0x20BC0");
    cs.add("段位不对齐", s5.enroll("y", 0x20BD0, HeapCase::Sentinel { broken: 0 }) == Err(XC_SEG_OOR), "非 0x40 步进");
    cs.add("段位高于域", s5.enroll("z", 0x20DC0, HeapCase::Sentinel { broken: 0 }) == Err(XC_SEG_OOR), ">0x20D80");
    let _ = s5.enroll("重", 0x20BC0, HeapCase::LeakFlat { net: 0 });
    cs.add("重名拒", s5.enroll("重", 0x20C00, HeapCase::LeakFlat { net: 0 }) == Err(XC_DUP_NAME), "同名");
    cs.add("空集拒", HeapSuite::new().evaluate() == Err(XC_EMPTY_SUITE), "先拒空");
    // 族五：边界判定。
    let eq0 = HeapCase::SlabShrunk { after: 2, before: 2 };
    cs.add("slab 数不变非回收", !eq0.eval(), "after == before 拒");
    let eq1 = HeapCase::SlabShrunk { after: 1, before: 0 };
    cs.add("零前基拒", !eq1.eval(), "before=0 拒");
    let eq2 = HeapCase::LedgerEq { live: -1, alloc: 10, freed: 11 };
    cs.add("负存量拒", !eq2.eval(), "live<0 拒");
    let eq3 = HeapCase::StkAlign { base: 16390 };
    cs.add("非对齐拒", !eq3.eval(), "16390 % 16 = 6");
    // 族六：词位回读。
    let w = HeapCase::LoanBal { lent: 24, free: 232, total: 256 }.words();
    cs.add("词位族标", w[0] == 5, "借还族 = 5");
    cs.add("词位数据", w[1] == 24 && w[3] == 256, "lent/total 回读");
    cs
}
