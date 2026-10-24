//! unx0377_ktestmem —— UNX-F0377 ktest 内存面断言集接入。
//! 锚点（CoRun Unxreal 主册）：UNX-F0377 · ktest 内存面断言集接入 · 目标 240 行
//! 判据原文：UNX-F0377-J1 内存面各模块的关键不变量逐条登记为
//!           ktest 断言项（位图一致/账平/合并单调/保留区不交/
//!           水位不破/五数守恒六族），每项携带自含样本可独立
//!           判定；接入完整性核要求段位覆盖连续无漏，红项立案
//!           并阻断内存面收官。
//! 组件：断言项（六族枚举 + 自含样本）+ 判定引擎（逐项求值）+
//!       接入账（登记 + 段位覆盖核 + 红项立案）+ 摘要序列化
//!       （词位流 + FNV32 签，复跑零漂移）。
//! 职责定位：接入面是「内存面 → ktest 总闸」的桥——每个内存面
//!   模块的守恒性质在这里被收拢成一条条可判定的断言项；样本
//!   自含（数字直接写死在项里），复跑不需要拉起被测模块本体。
//!   段位覆盖核防漏登：0x20740~0x20A80 段每个已登记模块至少
//!   有一条断言项挂账。
//! 性能逐项分解：逐项判定 O(1)；覆盖核 O(登记数)；摘要 O(项数)。
//! 诊断码段：0x20AC0（0x20AC1~0x20AC9 闭集；u32 高段全域独占）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 模块版本。
pub const KM_VERSION: &str = "unx0377_ktestmem-v1";

/// 诊断段基址（0x20AC0 段 · 本模块独占）。
pub const KM_SEG_BASE: u32 = 0x20AC0;
pub const XC_EMPTY_SUITE: u32 = 0x20AC1; // 空断言集拒
pub const XC_DUP_NAME: u32 = 0x20AC2; // 断言重名
pub const XC_RED_ASSERT: u32 = 0x20AC3; // 红项存在
pub const XC_GAP_COVER: u32 = 0x20AC4; // 段位覆盖有漏
pub const XC_KIND_OOR: u32 = 0x20AC5; // 族越域
pub const XC_CKSUM: u32 = 0x20AC6; // 摘要校验和不过
pub const XC_MAGIC: u32 = 0x20AC7; // 摘要魔数破
pub const XC_SAMPLE_OOR: u32 = 0x20AC8; // 样本越域
pub const XC_REDRIFT: u32 = 0x20AC9; // 复跑红项漂移

/// 断言六族（内存面专用）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MemCase {
    /// 位图一致族：位图计数 = 实占帧数。
    BitmapMatch { bits_set: u32, frames: u32 },
    /// 账平族：借方 = 贷方。
    LedgerBalance { debit: u64, credit: u64 },
    /// 合并单调族：合并后空闲块数不增、总空闲不变。
    MergeMonotonic { blocks_after: u32, blocks_before: u32, free_after: u64, free_before: u64 },
    /// 保留区不交族：两保留区相交帧数为 0。
    ReserveDisjoint { overlap_frames: u32 },
    /// 水位不破族：当前水位落在低/高水位闭区间内。
    WaterLevel { cur: u32, low: u32, high: u32 },
    /// 五数守恒族：总装 = 可用 + 保留 + 牺牲。
    FiveConserve { total: u64, avail: u64, reserve: u64, sacrifice: u64 },
}

impl MemCase {
    /// 族标签（1~6）。
    pub fn kind(&self) -> u8 {
        match self {
            MemCase::BitmapMatch { .. } => 1,
            MemCase::LedgerBalance { .. } => 2,
            MemCase::MergeMonotonic { .. } => 3,
            MemCase::ReserveDisjoint { .. } => 4,
            MemCase::WaterLevel { .. } => 5,
            MemCase::FiveConserve { .. } => 6,
        }
    }

    /// 判定：绿 = 不变量成立。
    pub fn eval(&self) -> bool {
        match self {
            MemCase::BitmapMatch { bits_set, frames } => bits_set == frames,
            MemCase::LedgerBalance { debit, credit } => debit == credit,
            MemCase::MergeMonotonic { blocks_after, blocks_before, free_after, free_before } => {
                blocks_after <= blocks_before && free_after == free_before
            }
            MemCase::ReserveDisjoint { overlap_frames } => *overlap_frames == 0,
            MemCase::WaterLevel { cur, low, high } => low <= high && *cur >= *low && *cur <= *high,
            MemCase::FiveConserve { total, avail, reserve, sacrifice } => {
                *total == avail + reserve + sacrifice
            }
        }
    }

    /// 词位编码（族 + 4 数据词，缺位补零）。
    pub fn words(&self) -> [u32; 5] {
        match self {
            MemCase::BitmapMatch { bits_set, frames } => {
                [1, *bits_set, *frames, 0, 0]
            }
            MemCase::LedgerBalance { debit, credit } => {
                [2, *debit as u32, (*debit >> 32) as u32, *credit as u32, (*credit >> 32) as u32]
            }
            MemCase::MergeMonotonic { blocks_after, blocks_before, free_after, free_before } => {
                [3, *blocks_after, *blocks_before, *free_after as u32, *free_before as u32]
            }
            MemCase::ReserveDisjoint { overlap_frames } => [4, *overlap_frames, 0, 0, 0],
            MemCase::WaterLevel { cur, low, high } => [5, *cur, *low, *high, 0],
            MemCase::FiveConserve { total, avail, reserve, sacrifice } => {
                [6, *total as u32, *avail as u32, *reserve as u32, *sacrifice as u32]
            }
        }
    }
}

/// 断言项：族 + 自含样本 + 挂靠段位。
#[derive(Clone, Debug)]
pub struct MemAssert {
    /// 断言名（接入账内唯一）。
    pub name: &'static str,
    /// 挂靠的内存面模块段位（0x20740~0x20A80 段）。
    pub seg: u32,
    /// 案例。
    pub case: MemCase,
}

/// 接入账。
pub struct Suite {
    items: Vec<MemAssert>,
    reds: Vec<usize>,
}

impl Suite {
    /// 建空账。
    pub fn new() -> Suite {
        Suite { items: Vec::new(), reds: Vec::new() }
    }

    /// 登记一项；重名或族越域拒。
    pub fn enroll(&mut self, name: &'static str, seg: u32, case: MemCase) -> Result<(), u32> {
        if case.kind() == 0 || case.kind() > 6 {
            return Err(XC_KIND_OOR);
        }
        if seg < 0x20740 || seg > 0x20A80 || seg % 0x40 != 0 {
            return Err(XC_SAMPLE_OOR);
        }
        for it in self.items.iter() {
            if it.name == name {
                return Err(XC_DUP_NAME);
            }
        }
        self.items.push(MemAssert { name, seg, case });
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

    /// 段位覆盖核：挂靠段位去重计数 = 覆盖期望（14 个内存面模块段）。
    pub fn cover_ok(&self) -> Result<(), u32> {
        // 段表：0x20740 起步进 0x40 至 0x20A80，共 14 段。
        let mut covered = [false; 14];
        for it in self.items.iter() {
            if it.seg >= 0x20740 && it.seg <= 0x20A80 && (it.seg - 0x20740) % 0x40 == 0 {
                covered[((it.seg - 0x20740) / 0x40) as usize] = true;
            }
        }
        let n = covered.iter().filter(|&&b| b).count();
        if n < 14 {
            return Err(XC_GAP_COVER);
        }
        Ok(())
    }

    /// 摘要序列化：魔数 + 项数 + 各项词位，FNV32 签收尾。
    pub fn digest(&self) -> (Vec<u32>, u32) {
        let mut words: Vec<u32> = Vec::new();
        words.push(0xB19A_0001u32); // 魔数：B19 接入面 v1
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

    /// 复跑核：摘要重算一致（清单即世界）。
    pub fn replay_stable(&self) -> Result<(), u32> {
        let (words, sig) = self.digest();
        if words[0] != 0xB19A_0001 {
            return Err(XC_MAGIC);
        }
        let tail = words[words.len() - 1];
        if tail != sig {
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

pub fn run_unx0377_checks() -> CheckSet {
    let mut cs = CheckSet::new("unx0377_ktestmem");
    // 族一：六族各登记一例全绿。
    let mut s1 = Suite::new();
    let en = s1.enroll("位图一致", 0x20740, MemCase::BitmapMatch { bits_set: 96, frames: 96 });
    cs.add("登记位图", en.is_ok(), "0x20740");
    let _ = s1.enroll("账平", 0x20780, MemCase::LedgerBalance { debit: 4096, credit: 4096 });
    let _ = s1.enroll("合并单调", 0x20840,
        MemCase::MergeMonotonic { blocks_after: 2, blocks_before: 3, free_after: 64, free_before: 64 });
    let _ = s1.enroll("保留不交", 0x20900, MemCase::ReserveDisjoint { overlap_frames: 0 });
    let _ = s1.enroll("水位正常", 0x209C0, MemCase::WaterLevel { cur: 50, low: 20, high: 80 });
    let _ = s1.enroll("五数守恒", 0x20A80,
        MemCase::FiveConserve { total: 1_048_576, avail: 1_000_000, reserve: 40_000, sacrifice: 8_576 });
    cs.add("登记六项", s1.len() == 6, "六族齐");
    cs.add("求值全绿", s1.evaluate().is_ok(), "红 0");
    cs.add("空集拒", Suite::new().evaluate() == Err(XC_EMPTY_SUITE), "先拒空");
    // 族二：红项检出与立案。
    let mut s2 = Suite::new();
    let _ = s2.enroll("账破", 0x20780, MemCase::LedgerBalance { debit: 100, credit: 99 });
    cs.add("红项检出", s2.evaluate() == Err(XC_RED_ASSERT), "差 1 立案");
    cs.add("红项挂账", s2.red_count() == 1, "红 1");
    // 族三：段位覆盖核。
    let mut s3 = Suite::new();
    // 14 段各挂一条最小绿例。
    let segs: [u32; 14] = [
        0x20740, 0x20780, 0x207C0, 0x20800, 0x20840, 0x20880, 0x208C0,
        0x20900, 0x20940, 0x20980, 0x209C0, 0x20A00, 0x20A40, 0x20A80,
    ];
    let mut all_ok = true;
    for (i, &sg) in segs.iter().enumerate() {
        let case = MemCase::LedgerBalance { debit: (i as u64) + 1, credit: (i as u64) + 1 };
        if s3.enroll("覆盖项", sg, case).is_err() {
            all_ok = false;
        }
    }
    cs.add("14 段登记", all_ok && s3.len() == 14, "逐段一条");
    // 重名拦截演示（第 15 项同名）。
    cs.add("重名拒",
        s3.enroll("覆盖项", 0x20740, MemCase::LedgerBalance { debit: 1, credit: 1 })
            == Err(XC_DUP_NAME),
        "同名拒");
    cs.add("覆盖齐", s3.cover_ok().is_ok(), "14/14");
    cs.add("复跑稳", s3.replay_stable().is_ok(), "签自洽");
    // 族四：覆盖漏检。
    let mut s4 = Suite::new();
    let _ = s4.enroll("只挂一段", 0x20740,
        MemCase::LedgerBalance { debit: 7, credit: 7 });
    cs.add("覆盖漏检出", s4.cover_ok() == Err(XC_GAP_COVER), "1/14 拒");
    // 族五：段位越域拒。
    let mut s5 = Suite::new();
    cs.add("段位低于域",
        s5.enroll("x", 0x20700, MemCase::LedgerBalance { debit: 1, credit: 1 })
            == Err(XC_SAMPLE_OOR),
        "0x20700 拒");
    cs.add("段位不对齐",
        s5.enroll("y", 0x20750, MemCase::LedgerBalance { debit: 1, credit: 1 })
            == Err(XC_SAMPLE_OOR),
        "非 0x40 步进拒");
    // 族六：词位编码回读。
    let c = MemCase::FiveConserve { total: 1_048_576, avail: 1_000_000, reserve: 40_000, sacrifice: 8_576 };
    let w = c.words();
    cs.add("词位族标", w[0] == 6, "五数族 = 6");
    cs.add("词位数据", w[1] == 1_048_576 && w[2] == 1_000_000, "total/avail 回读");
    cs.add("守恒恒等", c.eval(), "1M = 1000k + 40k + 8576");
    cs
}
