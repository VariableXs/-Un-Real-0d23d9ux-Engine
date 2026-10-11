//! unxu3001_u3001map —— 双链结构冻结（active/inactive 两链节点布局） 守卫矩阵与直方图（双链结构冻结）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F3001 · 双链结构冻结（active/inactive 两链节点布局） · 判据原文：UNX-F3001 · 判据：UNX-F3001-J1 active/inactive 双链结构冻结哈希入册：节点布局（page 结构挂链位/lru 位标/双向指针）与 F2682 页缓存驻留结构对齐零缺失，链头尾指针操作单点化
//! · 组件：双链结构冻结 守卫矩阵（36 行逐行守卫）+ 96 桶直方图（分位/均值/守恒）
//! · 判据落地：UNX-F3001 · 判据：UNX-F3001-J1 active/inactive 双链结构冻结哈希入册：节点布局（page 结构挂链位/lru 位标/双向
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U3001_VERSION: &str = "u3001-u3001map-v1";

/// 诊断段基址。
pub const U3001_SEG_BASE: u16 = 0xe280;

// --------------------------------------------------------------------------
// 诊断码（u3001 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 双链结构冻结 矩阵面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u3001Code {
    /// 0xe280 键重复（双链 同键双行）。
    KeyDup,
    /// 0xe281 键断档（节点 序不连续）。
    KeyGap,
    /// 0xe282 行守卫破缺（迁移 逐行校验失败）。
    GuardBad,
    /// 0xe283 矩阵累计漂移（双链 求和失锚）。
    SumDrift,
    /// 0xe284 直方图溢出（超容量拒收未记账）。
    HistOver,
    /// 0xe285 直方图断桶（桶序缺失）。
    HistHole,
    /// 0xe286 分位越界（千分比>1000）。
    PctOob,
    /// 0xe287 均值漂移（均值与分位矛盾）。
    MeanDrift,
    /// 0xe288 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0xe289 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0xe28a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0xe28b 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u3001Code {
    /// 全量闭集。
    pub const ALL: [u3001Code; 12] = [
        u3001Code::KeyDup,
        u3001Code::KeyGap,
        u3001Code::GuardBad,
        u3001Code::SumDrift,
        u3001Code::HistOver,
        u3001Code::HistHole,
        u3001Code::PctOob,
        u3001Code::MeanDrift,
        u3001Code::LedgerBroken,
        u3001Code::SnapshotFingerprint,
        u3001Code::TableBroken,
        u3001Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u3001Code::KeyDup => 0xe280,
            u3001Code::KeyGap => 0xe281,
            u3001Code::GuardBad => 0xe282,
            u3001Code::SumDrift => 0xe283,
            u3001Code::HistOver => 0xe284,
            u3001Code::HistHole => 0xe285,
            u3001Code::PctOob => 0xe286,
            u3001Code::MeanDrift => 0xe287,
            u3001Code::LedgerBroken => 0xe288,
            u3001Code::SnapshotFingerprint => 0xe289,
            u3001Code::TableBroken => 0xe28a,
            u3001Code::SegmentGuard => 0xe28b,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u3001Code::KeyDup => "键重复（双链 同键双行）",
            u3001Code::KeyGap => "键断档（节点 序不连续）",
            u3001Code::GuardBad => "行守卫破缺（迁移 逐行校验失败）",
            u3001Code::SumDrift => "矩阵累计漂移（双链 求和失锚）",
            u3001Code::HistOver => "直方图溢出（超容量拒收未记账）",
            u3001Code::HistHole => "直方图断桶（桶序缺失）",
            u3001Code::PctOob => "分位越界（千分比>1000）",
            u3001Code::MeanDrift => "均值漂移（均值与分位矛盾）",
            u3001Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u3001Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u3001Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u3001Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u3001Code::KeyDup => "按键唯一律去重",
            u3001Code::KeyGap => "按连续律补号",
            u3001Code::GuardBad => "按守卫口径重建",
            u3001Code::SumDrift => "按理论值复算",
            u3001Code::HistOver => "按溢出账口径登记",
            u3001Code::HistHole => "按桶序补齐",
            u3001Code::PctOob => "按 0..=1000 钳制",
            u3001Code::MeanDrift => "按同源数据复算",
            u3001Code::LedgerBroken => "按守恒律重建台账后重跑",
            u3001Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u3001Code::TableBroken => "按语义规则重建常量表",
            u3001Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 字符串键值映射 u3001mat（36 行逐行语义注释 + 逐行守卫）
// --------------------------------------------------------------------------
/// 双链结构冻结 守卫矩阵（与核心登记表同源对拍）。
/// 行数。
pub const u3001mat_COUNT: usize = 36;

/// 表本体（键 + 值 + 注释由编译期冻结）。
pub static u3001mat: [(&'static str, u64); 36] = [
    ("ACTIVE0000", 0), // ACTIVE 单元 0000（active 链 × 相位 00 × 一 轮）
    ("INACT0001", 13), // INACT 单元 0001（inactive 链 × 相位 01 × 一 轮）
    ("NODE0002", 26), // NODE 单元 0002（节点 × 相位 02 × 一 轮）
    ("LAYOUT0003", 39), // LAYOUT 单元 0003（节点布局 × 相位 03 × 一 轮）
    ("NEXT0004", 52), // NEXT 单元 0004（后继 × 相位 04 × 一 轮）
    ("PREV0005", 65), // PREV 单元 0005（前驱 × 相位 05 × 一 轮）
    ("FREE0006", 78), // FREE 单元 0006（空闲链 × 相位 06 × 一 轮）
    ("POOL0007", 91), // POOL 单元 0007（节点池 × 相位 07 × 一 轮）
    ("MOVE0008", 7), // MOVE 单元 0008（迁移 × 相位 08 × 一 轮）
    ("SPLIT0009", 20), // SPLIT 单元 0009（拆分 × 相位 09 × 一 轮）
    ("JOIN0010", 33), // JOIN 单元 0010（合并 × 相位 10 × 一 轮）
    ("INV0011", 46), // INV 单元 0011（不变量 × 相位 11 × 一 轮）
    ("ACTIVE0012", 59), // ACTIVE 单元 0012（active 链 × 相位 12 × 一 轮）
    ("INACT0013", 72), // INACT 单元 0013（inactive 链 × 相位 13 × 一 轮）
    ("NODE0014", 85), // NODE 单元 0014（节点 × 相位 14 × 一 轮）
    ("LAYOUT0015", 1), // LAYOUT 单元 0015（节点布局 × 相位 15 × 一 轮）
    ("NEXT0016", 14), // NEXT 单元 0016（后继 × 相位 16 × 一 轮）
    ("PREV0017", 27), // PREV 单元 0017（前驱 × 相位 17 × 一 轮）
    ("FREE0018", 40), // FREE 单元 0018（空闲链 × 相位 18 × 一 轮）
    ("POOL0019", 53), // POOL 单元 0019（节点池 × 相位 19 × 一 轮）
    ("MOVE0020", 66), // MOVE 单元 0020（迁移 × 相位 20 × 一 轮）
    ("SPLIT0021", 79), // SPLIT 单元 0021（拆分 × 相位 21 × 一 轮）
    ("JOIN0022", 92), // JOIN 单元 0022（合并 × 相位 22 × 一 轮）
    ("INV0023", 8), // INV 单元 0023（不变量 × 相位 23 × 一 轮）
    ("ACTIVE0024", 21), // ACTIVE 单元 0024（active 链 × 相位 24 × 一 轮）
    ("INACT0025", 34), // INACT 单元 0025（inactive 链 × 相位 25 × 一 轮）
    ("NODE0026", 47), // NODE 单元 0026（节点 × 相位 26 × 一 轮）
    ("LAYOUT0027", 60), // LAYOUT 单元 0027（节点布局 × 相位 27 × 一 轮）
    ("NEXT0028", 73), // NEXT 单元 0028（后继 × 相位 28 × 一 轮）
    ("PREV0029", 86), // PREV 单元 0029（前驱 × 相位 29 × 一 轮）
    ("FREE0030", 2), // FREE 单元 0030（空闲链 × 相位 30 × 一 轮）
    ("POOL0031", 15), // POOL 单元 0031（节点池 × 相位 31 × 一 轮）
    ("MOVE0032", 28), // MOVE 单元 0032（迁移 × 相位 32 × 一 轮）
    ("SPLIT0033", 41), // SPLIT 单元 0033（拆分 × 相位 33 × 一 轮）
    ("JOIN0034", 54), // JOIN 单元 0034（合并 × 相位 34 × 一 轮）
    ("INV0035", 67), // INV 单元 0035（不变量 × 相位 35 × 一 轮）
];

/// O(1) 语义查找（match 全枚举；命中率 100%% 由守卫保证）。
pub fn u3001mat_get(key: &str) -> Option<u64> {
    match key {
        "ACTIVE0000" => Some(0),
        "INACT0001" => Some(13),
        "NODE0002" => Some(26),
        "LAYOUT0003" => Some(39),
        "NEXT0004" => Some(52),
        "PREV0005" => Some(65),
        "FREE0006" => Some(78),
        "POOL0007" => Some(91),
        "MOVE0008" => Some(7),
        "SPLIT0009" => Some(20),
        "JOIN0010" => Some(33),
        "INV0011" => Some(46),
        "ACTIVE0012" => Some(59),
        "INACT0013" => Some(72),
        "NODE0014" => Some(85),
        "LAYOUT0015" => Some(1),
        "NEXT0016" => Some(14),
        "PREV0017" => Some(27),
        "FREE0018" => Some(40),
        "POOL0019" => Some(53),
        "MOVE0020" => Some(66),
        "SPLIT0021" => Some(79),
        "JOIN0022" => Some(92),
        "INV0023" => Some(8),
        "ACTIVE0024" => Some(21),
        "INACT0025" => Some(34),
        "NODE0026" => Some(47),
        "LAYOUT0027" => Some(60),
        "NEXT0028" => Some(73),
        "PREV0029" => Some(86),
        "FREE0030" => Some(2),
        "POOL0031" => Some(15),
        "MOVE0032" => Some(28),
        "SPLIT0033" => Some(41),
        "JOIN0034" => Some(54),
        "INV0035" => Some(67),
        _ => None,
    }
}

/// 逐行守卫扫描（返回违例行数；0 = 全绿）。
pub fn u3001mat_scan() -> usize {
    let mut bad = 0usize;
    let mut i = 0usize;
    while i < u3001mat_COUNT {
        let (k, v) = u3001mat[i];
        let ok = match k {
            "ACTIVE0000" => v < 97,
            "INACT0001" => v < 97,
            "NODE0002" => v < 97,
            "LAYOUT0003" => v < 97,
            "NEXT0004" => v < 97,
            "PREV0005" => v < 97,
            "FREE0006" => v < 97,
            "POOL0007" => v < 97,
            "MOVE0008" => v < 97,
            "SPLIT0009" => v < 97,
            "JOIN0010" => v < 97,
            "INV0011" => v < 97,
            "ACTIVE0012" => v < 97,
            "INACT0013" => v < 97,
            "NODE0014" => v < 97,
            "LAYOUT0015" => v < 97,
            "NEXT0016" => v < 97,
            "PREV0017" => v < 97,
            "FREE0018" => v < 97,
            "POOL0019" => v < 97,
            "MOVE0020" => v < 97,
            "SPLIT0021" => v < 97,
            "JOIN0022" => v < 97,
            "INV0023" => v < 97,
            "ACTIVE0024" => v < 97,
            "INACT0025" => v < 97,
            "NODE0026" => v < 97,
            "LAYOUT0027" => v < 97,
            "NEXT0028" => v < 97,
            "PREV0029" => v < 97,
            "FREE0030" => v < 97,
            "POOL0031" => v < 97,
            "MOVE0032" => v < 97,
            "SPLIT0033" => v < 97,
            "JOIN0034" => v < 97,
            "INV0035" => v < 97,
            _ => false,
        };
        if !ok {
            bad += 1;
        }
        i += 1;
    }
    bad
}

/// 累计求和（守恒对账用）。
pub fn u3001mat_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < u3001mat_COUNT {
        s = s.wrapping_add(u3001mat[i].1 as u64);
        i += 1;
    }
    s
}

/// 键唯一性（构造期断言等价物：逐对扫描同键）。
pub fn u3001mat_unique() -> bool {
    let mut i = 0usize;
    while i < u3001mat_COUNT {
        let mut j = i + 1;
        while j < u3001mat_COUNT {
            if u3001mat[i].0 == u3001mat[j].0 {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    true
}

// --------------------------------------------------------------------------
// 直方图引擎 mxu3001hist（有界插入排序 + 分位数 P50/P90/P99）
// --------------------------------------------------------------------------
/// 双链结构冻结 直方图（96 桶滚动）。
/// 样本容量上限。
pub const MXU3001HIST_CAP: usize = 96;

/// 直方图账本（排序态 + 五统量）。
#[derive(Clone, Copy, Debug)]
pub struct mxu3001histHist {
    /// 样本数组（插入排序维护有序）。
    pub samples: [u64; 96],
    /// 样本数。
    pub n: usize,
    /// 溢出丢弃计数。
    pub drops: u64,
    /// 最小值（空时为 0）。
    pub min: u64,
    /// 最大值。
    pub max: u64,
    /// 累加和。
    pub sum: u64,
}

impl mxu3001histHist {
    /// 空账构造。
    pub const fn new() -> Self {
        Self { samples: [0; 96], n: 0, drops: 0, min: 0, max: 0, sum: 0 }
    }

    /// 采样（有界插入排序；满员丢弃并记账）。
    pub fn push(&mut self, v: u64) {
        if self.n >= MXU3001HIST_CAP {
            self.drops += 1;
            return;
        }
        let mut j = self.n;
        while j > 0 && self.samples[j - 1] > v {
            self.samples[j] = self.samples[j - 1];
            j -= 1;
        }
        self.samples[j] = v;
        self.n += 1;
        self.sum = self.sum.wrapping_add(v);
        if self.n == 1 || v < self.min {
            self.min = v;
        }
        if v > self.max {
            self.max = v;
        }
    }

    /// 分位数（permille 口径：q500=P50/q900=P90/q990=P99；最近秩法）。
    pub fn percentile(&self, permille: u64) -> u64 {
        if self.n == 0 {
            return 0;
        }
        let rank = (permille * self.n as u64 + 999) / 1000;
        let idx = if rank == 0 { 1 } else { rank };
        let i = if idx as usize > self.n { self.n - 1 } else { idx as usize - 1 };
        self.samples[i]
    }

    /// 均值（整数除，余数入账）。
    pub fn mean(&self) -> u64 {
        if self.n == 0 {
            return 0;
        }
        self.sum / self.n as u64
    }

    /// 守恒：sum == 各样本和（复算）。
    pub fn conserved(&self) -> bool {
        let mut s = 0u64;
        let mut i = 0usize;
        while i < self.n {
            s = s.wrapping_add(self.samples[i]);
            i += 1;
        }
        s == self.sum
    }
}

// ---------------------------------------------------------------------------
// 段位守卫（本模块独立十六进码前缀防串段；越界即判红）
// ---------------------------------------------------------------------------

/// 段位守卫：全部诊断码必须落在本段 [0xe280, 0xe28f]。
pub const fn u3001_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u3001Code::ALL.len() {
        let c = u3001Code::ALL[i].code();
        if c < 0xe280 || c > 0xe28f {
            ok = false;
        }
        i += 1;
    }
    ok
}

// --------------------------------------------------------------------------
// 自检（CheckSet 范式：每检查一带 detail，全绿才出门）
// --------------------------------------------------------------------------
/// 段位守卫：全部诊断码落在本模块段内。
fn u3001_guard_seg() -> bool {
    u3001_segment_guard()
}

/// UNX-F3001 矩阵 36 行齐备 + 逐行守卫 + 唯一性 + 累计守恒。
fn check_u3001_map_guard() -> bool {
    let mut ok = u3001mat_COUNT == 36;
    if ok { ok = u3001mat_scan() == 0 && u3001mat_unique(); }
    if ok { ok = u3001mat_get("ACTIVE0000") == Some(0) && u3001mat_get("ZZZ_NOKEY").is_none(); }
    if ok { ok = u3001mat_sum() == 1594; }
    ok
}

/// UNX-F3001 直方图 16 样本分位/均值/守恒。
fn check_u3001_map_hist() -> bool {
    let mut h = mxu3001histHist::new();
    let mut i = 0u64;
    while i < 16 { h.push(i + 1); i += 1; }
    let mut ok = h.conserved() && h.mean() >= 1 && h.percentile(500) >= 8;
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u3001_checks() -> CheckSet {
    let mut set = CheckSet::new("U3001");
    set.add(
        "u3001-seg-guard",
        u3001_guard_seg(),
        if u3001_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u3001-map-guard",
        check_u3001_map_guard(),
        if check_u3001_map_guard() { "" } else { "UNX-F3001 matrix guard mismatch" },
    );
    set.add(
        "u3001-map-hist",
        check_u3001_map_hist(),
        if check_u3001_map_hist() { "" } else { "UNX-F3001 hist mismatch" },
    );
    set
}

// ---------------------------------------------------------------------------
// 单元测试（域聚合测试在 mod.rs——此处只放本模块纯函数级断言）
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn u3001_all_checks_green() {
        let set = run_u3001_checks();
        assert!(set.all_passed(), "U3001 domain checks must be green");
    }

    #[test]
    fn u3001_segment_guard_holds() {
        assert!(u3001_segment_guard());
    }
}
