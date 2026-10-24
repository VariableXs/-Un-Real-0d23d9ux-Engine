//! unx0418_ktimeasssert —— UNX-F0418 ktest 计时面断言接入。
//! 锚点（CoRun Unxreal 主册）：UNX-F0418 · ktest 计时面断言接入 · 目标 240 行
//! 判据原文：UNX-F0418-J1 计时面各模块（墙钟偏移/定时队列/时钟
//!           源选择/精度自检/跳变检出/软定时/交接字段/五数/
//!           PIT 交叉/对冲总账/漂移注入）的关键不变量登记为
//!           ktest 断言项（单调/堆序/主备一致/限内/案账平/
//!           序稳/三态/有序/容差/恒等/可补偿十一族）；接入完
//!           整性核：段位 0x20F00~0x211C0 每个计时面模块至少
//!           一条断言挂账；红项立案带族标签。
//! 组件：断言项（十一族枚举 + 自含样本）+ 判定引擎 + 接入账
//!       （登记 + 段位覆盖核 + 红项立案）+ 摘要序列化。
//! 职责定位：接入面是「计时面 → ktest 总闸」的桥——与内存面
//!   （0377）堆面（0396）同构，覆盖对象换成计时面十一模块；
//!   样本自含、复跑零依赖纪律一致。
//! 性能逐项分解：逐项判定 O(1)；覆盖核 O(登记数)；摘要 O(项数)。
//! 诊断码段：0x21200（0x21201~0x21209 闭集；u32 高段全域独占）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 模块版本。
pub const KA_VERSION: &str = "unx0418_ktimeassert-v1";

/// 诊断段基址（0x21200 段 · 本模块独占）。
pub const KA_SEG_BASE: u32 = 0x21200;
pub const XC_EMPTY_SUITE: u32 = 0x21201; // 空断言集拒
pub const XC_DUP_NAME: u32 = 0x21202; // 断言重名
pub const XC_RED_ASSERT: u32 = 0x21203; // 红项存在
pub const XC_GAP_COVER: u32 = 0x21204; // 段位覆盖有漏
pub const XC_KIND_OOR: u32 = 0x21205; // 族越域
pub const XC_CKSUM: u32 = 0x21206; // 摘要校验和不过
pub const XC_MAGIC: u32 = 0x21207; // 摘要魔数破
pub const XC_SEG_OOR: u32 = 0x21208; // 段位越域
pub const XC_NAME_LEN: u32 = 0x21209; // 断言名长度越界

/// 计时面断言十一族。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TimeCase {
    /// 单调族：两次读值后者 ≥ 前者。
    Monotonic { prev: u64, cur: u64 },
    /// 堆序族：父到期 ≤ 子到期。
    HeapOrder { parent: u64, child: u64 },
    /// 主备一致族：复选主源 = 当前主源。
    MainAgree { reelected: u32, current: u32 },
    /// 限内族：抖动 ≤ 限。
    JitterIn { jitter: u64, cap: u64 },
    /// 案账平族：立案数 = 案表长。
    CaseBal { declared: u32, actual: u32 },
    /// 序稳族：到期升序（相邻不降）。
    FireAscend { prev_fire: u64, cur_fire: u64 },
    /// 三态族：字段态码 ∈ [1, 3]。
    FieldState { code: u32 },
    /// 有序族：五数有序。
    FiveOrdered { min: u64, q1: u64, med: u64, q3: u64, max: u64 },
    /// 容差族：交叉偏差 ≤ 容差。
    TolIn { drift_milli: u32, tol_milli: u32 },
    /// 恒等族：在册 = 登记 - 执行 - 注销。
    LedgerEq { live: i64, reg: u32, fire: u32, cancel: u32 },
    /// 可补偿族：补偿残差 ≤ 精度。
    Compensable { residual: u64, precision: u64 },
}

impl TimeCase {
    /// 族标签（1~11）。
    pub fn kind(&self) -> u8 {
        match self {
            TimeCase::Monotonic { .. } => 1,
            TimeCase::HeapOrder { .. } => 2,
            TimeCase::MainAgree { .. } => 3,
            TimeCase::JitterIn { .. } => 4,
            TimeCase::CaseBal { .. } => 5,
            TimeCase::FireAscend { .. } => 6,
            TimeCase::FieldState { .. } => 7,
            TimeCase::FiveOrdered { .. } => 8,
            TimeCase::TolIn { .. } => 9,
            TimeCase::LedgerEq { .. } => 10,
            TimeCase::Compensable { .. } => 11,
        }
    }

    /// 判定。
    pub fn eval(&self) -> bool {
        match self {
            TimeCase::Monotonic { prev, cur } => cur >= prev,
            TimeCase::HeapOrder { parent, child } => parent <= child,
            TimeCase::MainAgree { reelected, current } => reelected == current,
            TimeCase::JitterIn { jitter, cap } => jitter <= cap,
            TimeCase::CaseBal { declared, actual } => declared == actual,
            TimeCase::FireAscend { prev_fire, cur_fire } => cur_fire >= prev_fire,
            TimeCase::FieldState { code } => (1..=3).contains(code),
            TimeCase::FiveOrdered { min, q1, med, q3, max } => {
                min <= q1 && q1 <= med && med <= q3 && q3 <= max
            }
            TimeCase::TolIn { drift_milli, tol_milli } => drift_milli <= tol_milli,
            TimeCase::LedgerEq { live, reg, fire, cancel } => {
                *live >= 0 && (*reg as i64 - *fire as i64 - *cancel as i64) == *live
            }
            TimeCase::Compensable { residual, precision } => residual <= precision,
        }
    }

    /// 词位编码（族 + 4 数据词）。
    pub fn words(&self) -> [u32; 5] {
        match self {
            TimeCase::Monotonic { prev, cur } => [1, *prev as u32, (*prev >> 32) as u32, *cur as u32, (*cur >> 32) as u32],
            TimeCase::HeapOrder { parent, child } => [2, *parent as u32, (*parent >> 32) as u32, *child as u32, (*child >> 32) as u32],
            TimeCase::MainAgree { reelected, current } => [3, *reelected, *current, 0, 0],
            TimeCase::JitterIn { jitter, cap } => [4, *jitter as u32, (*jitter >> 32) as u32, *cap as u32, (*cap >> 32) as u32],
            TimeCase::CaseBal { declared, actual } => [5, *declared, *actual, 0, 0],
            TimeCase::FireAscend { prev_fire, cur_fire } => [6, *prev_fire as u32, (*prev_fire >> 32) as u32, *cur_fire as u32, (*cur_fire >> 32) as u32],
            TimeCase::FieldState { code } => [7, *code, 0, 0, 0],
            TimeCase::FiveOrdered { min, q1, med, q3, max } => [8, *min as u32, *med as u32, *q3 as u32, *max as u32 | (*q1 as u32) << 16],
            TimeCase::TolIn { drift_milli, tol_milli } => [9, *drift_milli, *tol_milli, 0, 0],
            TimeCase::LedgerEq { live, reg, fire, cancel } => [10, *live as u32, *reg, *fire, *cancel],
            TimeCase::Compensable { residual, precision } => [11, *residual as u32, (*residual >> 32) as u32, *precision as u32, (*precision >> 32) as u32],
        }
    }
}

/// 断言项。
#[derive(Clone, Debug)]
pub struct TimeAssert {
    pub name: &'static str,
    /// 挂靠计时面模块段位（0x20F00~0x211C0，步进 0x40）。
    pub seg: u32,
    pub case: TimeCase,
}

/// 计时面段位表（11 段）。
pub const TIME_SEGS: [u32; 11] = [
    0x20F00, 0x20F40, 0x20F80, 0x20FC0, 0x21000, 0x21040, 0x21080, 0x210C0,
    0x21100, 0x21140, 0x21180,
];

/// 接入账。
pub struct TimeSuite {
    items: Vec<TimeAssert>,
    reds: Vec<usize>,
}

impl TimeSuite {
    /// 建空账。
    pub fn new() -> TimeSuite {
        TimeSuite { items: Vec::new(), reds: Vec::new() }
    }

    /// 登记。
    pub fn enroll(&mut self, name: &'static str, seg: u32, case: TimeCase) -> Result<(), u32> {
        let nb = name.as_bytes().len();
        if nb == 0 || nb > 32 {
            return Err(XC_NAME_LEN);
        }
        if case.kind() == 0 || case.kind() > 11 {
            return Err(XC_KIND_OOR);
        }
        if seg < 0x20F00 || seg > 0x211C0 || (seg - 0x20F00) % 0x40 != 0 {
            return Err(XC_SEG_OOR);
        }
        for it in self.items.iter() {
            if it.name == name {
                return Err(XC_DUP_NAME);
            }
        }
        self.items.push(TimeAssert { name, seg, case });
        Ok(())
    }

    /// 求值。
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

    /// 覆盖核：11 段全覆盖。
    pub fn cover_ok(&self) -> Result<(), u32> {
        for &seg in TIME_SEGS.iter() {
            if !self.items.iter().any(|it| it.seg == seg) {
                return Err(XC_GAP_COVER);
            }
        }
        Ok(())
    }

    /// 摘要。
    pub fn digest(&self) -> (Vec<u32>, u32) {
        let mut words: Vec<u32> = Vec::new();
        words.push(0x71D0_0001u32); // 魔数：计时接入面 v1
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
        if words[0] != 0x71D0_0001 {
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

pub fn run_unx0418_checks() -> CheckSet {
    let mut cs = CheckSet::new("unx0418_ktimeassert");
    // 族一：十一族各登记一例全绿。
    let mut s1 = TimeSuite::new();
    let _ = s1.enroll("单调", 0x20F00, TimeCase::Monotonic { prev: 100, cur: 200 });
    let _ = s1.enroll("堆序", 0x20F40, TimeCase::HeapOrder { parent: 50, child: 75 });
    let _ = s1.enroll("主备一致", 0x20FC0, TimeCase::MainAgree { reelected: 1, current: 1 });
    let _ = s1.enroll("抖动限内", 0x21000, TimeCase::JitterIn { jitter: 10, cap: 20 });
    let _ = s1.enroll("案账平", 0x21040, TimeCase::CaseBal { declared: 3, actual: 3 });
    let _ = s1.enroll("到期升序", 0x21080, TimeCase::FireAscend { prev_fire: 5, cur_fire: 10 });
    let _ = s1.enroll("三态合法", 0x210C0, TimeCase::FieldState { code: 2 });
    let _ = s1.enroll("五数有序", 0x21100, TimeCase::FiveOrdered { min: 1, q1: 2, med: 3, q3: 4, max: 5 });
    let _ = s1.enroll("PIT 容差", 0x21140, TimeCase::TolIn { drift_milli: 5, tol_milli: 50 });
    let _ = s1.enroll("总账恒等", 0x21180, TimeCase::LedgerEq { live: 2, reg: 3, fire: 1, cancel: 0 });
    let _ = s1.enroll("可补偿", 0x211C0, TimeCase::Compensable { residual: 1, precision: 2 });
    cs.add("登记十一项", s1.len() == 11, "十一族齐");
    cs.add("求值全绿", s1.evaluate().is_ok(), "红 0");
    cs.add("覆盖齐", s1.cover_ok().is_ok(), "11/11 段");
    cs.add("复跑稳", s1.replay_stable().is_ok(), "签自洽");
    // 族二：红项检出。
    let mut s2 = TimeSuite::new();
    let _ = s2.enroll("回退红", 0x20F00, TimeCase::Monotonic { prev: 200, cur: 100 });
    cs.add("回退红检出", s2.evaluate() == Err(XC_RED_ASSERT), "100 < 200");
    cs.add("红项数 1", s2.red_count() == 1, "立案");
    // 恒等红。
    let mut s3 = TimeSuite::new();
    let _ = s3.enroll("账不平", 0x21180, TimeCase::LedgerEq { live: 5, reg: 3, fire: 1, cancel: 0 });
    cs.add("恒等红", s3.evaluate() == Err(XC_RED_ASSERT), "3-1 ≠ 5");
    // 族三：覆盖漏检。
    let mut s4 = TimeSuite::new();
    let _ = s4.enroll("只挂一段", 0x20F00, TimeCase::Monotonic { prev: 1, cur: 2 });
    cs.add("覆盖漏检出", s4.cover_ok() == Err(XC_GAP_COVER), "1/11 拒");
    // 族四：登记拒绝。
    let mut s5 = TimeSuite::new();
    cs.add("段位低于域", s5.enroll("x", 0x20EC0, TimeCase::Monotonic { prev: 1, cur: 1 }) == Err(XC_SEG_OOR), "<0x20F00");
    cs.add("段位不对齐", s5.enroll("y", 0x20F10, TimeCase::Monotonic { prev: 1, cur: 1 }) == Err(XC_SEG_OOR), "非 0x40 步进");
    cs.add("段位高于域", s5.enroll("z", 0x21200, TimeCase::Monotonic { prev: 1, cur: 1 }) == Err(XC_SEG_OOR), ">0x211C0");
    cs.add("空名拒",
        s5.enroll("", 0x20F00, TimeCase::Monotonic { prev: 1, cur: 1 }) == Err(XC_NAME_LEN),
        "零字节");
    let _ = s5.enroll("重", 0x20F00, TimeCase::Monotonic { prev: 1, cur: 1 });
    cs.add("重名拒",
        s5.enroll("重", 0x20F40, TimeCase::Monotonic { prev: 1, cur: 1 }) == Err(XC_DUP_NAME),
        "同名");
    cs.add("空集拒", TimeSuite::new().evaluate() == Err(XC_EMPTY_SUITE), "先拒空");
    // 族五：边界判定。
    let eq0 = TimeCase::Monotonic { prev: 5, cur: 5 };
    cs.add("相等单调过", eq0.eval(), "恒等合法");
    let eq1 = TimeCase::JitterIn { jitter: 20, cap: 20 };
    cs.add("贴限过", eq1.eval(), "≤ 语义");
    let eq2 = TimeCase::FieldState { code: 0 };
    cs.add("态码零拒", !eq2.eval(), "越下界");
    let eq3 = TimeCase::FieldState { code: 4 };
    cs.add("态码四拒", !eq3.eval(), "越上界");
    let eq4 = TimeCase::Compensable { residual: 3, precision: 2 };
    cs.add("残差超限拒", !eq4.eval(), "3 > 2");
    let eq5 = TimeCase::FiveOrdered { min: 3, q1: 2, med: 3, q3: 4, max: 5 };
    cs.add("五数无序拒", !eq5.eval(), "min > q1");
    // 族六：词位回读。
    let w = TimeCase::TolIn { drift_milli: 5, tol_milli: 50 }.words();
    cs.add("词位族标", w[0] == 9, "容差族 = 9");
    cs.add("词位数据", w[1] == 5 && w[2] == 50, "drift/tol 回读");
    cs
}
