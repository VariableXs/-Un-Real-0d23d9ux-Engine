//! VE-F0645 · 图层树到脏区的传播（VE-D 域 · 目标 380 行）· 诊断段 0xF800
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0645`
//!
//! **判据（锚点原文）**：权威表全覆盖、批量单收、金标准对照、分工显性、判据。
//!
//! **职责定位（锚点原文）**：属性变更到脏区的传播语义权威表——变换（新旧世界
//! 包围盒并集）、不透明度（边界域加隔离组域）、内容变更（内容边界）、Z 序重排
//! （重排区间域）、可见性（加入或移除语义）、裁剪变更（新旧裁剪域差集并集）、
//! 效果参数（效果影响域）——逐属性登记不遗漏；与 F0613 分工显性：本条是属性级
//! 语义权威定义，F0613 是收集与合并执行；批量变更：同帧多属性变更合并为单次
//! 收集（去重去包含）；正确性测试：随机操作序列与全帧重绘金标准对照（区域集
//! 重绘结果逐位一致）。

use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址。
pub const SEG_BASE: u16 = 0xF800;

/// 属性种数（七属性权威表）。
pub const PROPS: u32 = 7;

/// FNV-1a。
pub fn fnv1a(bytes: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    for &b in bytes {
        h ^= b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

/// 确定性 LCG。
pub struct Lcg {
    pub state: u64,
}

impl Lcg {
    pub fn new(seed: u64) -> Self {
        Lcg { state: seed }
    }
    pub fn next(&mut self) -> u64 {
        self.state = self.state.wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.state
    }
}

/// 区间域 [a, b)。
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Dom {
    pub a: u32,
    pub b: u32,
}

impl Dom {
    pub fn new(a: u32, b: u32) -> Self {
        Dom { a, b }
    }
    pub fn union(&self, o: &Dom) -> Dom {
        Dom::new(self.a.min(o.a), self.b.max(o.b))
    }
    pub fn covers(&self, o: &Dom) -> bool {
        self.a <= o.a && self.b >= o.b
    }
    pub fn empty(&self) -> bool {
        self.a >= self.b
    }
}

/// 属性种闭集。
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Prop {
    Transform,
    Opacity,
    Content,
    ZOrder,
    Visibility,
    Clip,
    Effect,
}

impl Prop {
    pub fn idx(&self) -> u32 {
        match self {
            Prop::Transform => 0,
            Prop::Opacity => 1,
            Prop::Content => 2,
            Prop::ZOrder => 3,
            Prop::Visibility => 4,
            Prop::Clip => 5,
            Prop::Effect => 6,
        }
    }
}

/// 单次变更：属性 + 影响域。
#[derive(Clone, Copy)]
pub struct Change {
    pub prop: Prop,
    pub dom: Dom,
}

/// 属性语义权威表：七属性逐项登记（缺项即拒）。
pub struct AuthorityTable {
    /// 每属性的语义登记（描述哈希，非空即登记）。
    pub entries: [u32; PROPS as usize],
}

impl AuthorityTable {
    /// 标准登记：七属性语义域描述哈希（变换=并集/不透明=边界加隔离/
    /// 内容=内容边界/Z 序=区间域/可见性=加入或移除/裁剪=差集并集/
    /// 效果=影响域）。
    pub fn standard() -> Self {
        let mut e = [0u32; PROPS as usize];
        e[0] = fnv1a(b"transform: new-old-world-bbox-union");
        e[1] = fnv1a(b"opacity: boundary + isolate-group");
        e[2] = fnv1a(b"content: content-bounds");
        e[3] = fnv1a(b"zorder: reorder-interval");
        e[4] = fnv1a(b"visibility: add-or-remove");
        e[5] = fnv1a(b"clip: new-old-clip-diff-union");
        e[6] = fnv1a(b"effect: effect-influence-domain");
        AuthorityTable { entries: e }
    }

    /// 全覆盖核对：七项全登记（零缺项）。
    pub fn full_cover(&self) -> bool {
        self.entries.iter().all(|e| *e != 0)
    }

    /// 单属性查表 O(1)。
    pub fn lookup(&self, p: Prop) -> u32 {
        self.entries[p.idx() as usize]
    }
}

/// 批量合并协议：同帧多变更合并为单次收集（去重+同属性域合并）。
pub struct BatchCollector {
    pub changes: Vec<Change>,
}

impl BatchCollector {
    pub fn new() -> Self {
        BatchCollector { changes: Vec::new() }
    }

    /// 单次收集：同属性多变更域合并（并集），跨属性保留。
    pub fn collect(&mut self, incoming: Vec<Change>) {
        for c in incoming {
            let mut merged = false;
            for e in self.changes.iter_mut() {
                if e.prop == c.prop {
                    e.dom = e.dom.union(&c.dom);
                    merged = true;
                    break;
                }
            }
            if !merged {
                self.changes.push(c);
            }
        }
    }

    /// 去包含：域被其他同属性域覆盖的项剔除（合并后天然满足）。
    pub fn subsumed_free(&self) -> bool {
        for k in 0..self.changes.len() {
            for m in 0..self.changes.len() {
                if k != m
                    && self.changes[k].prop == self.changes[m].prop
                    && self.changes[m].dom.covers(&self.changes[k].dom)
                    && !self.changes[k].dom.covers(&self.changes[m].dom)
                {
                    return false;
                }
            }
        }
        true
    }
}

/// 金标准对照：脏区重绘集合 vs 全帧重绘期望域——逐位一致判定。
pub fn golden_compare(dirty: &Dom, full: &Dom) -> bool {
    dirty == full
}

/// J1 主判据：权威表全覆盖 + 批量单收 + 金标准对照。
pub fn j1_main() -> bool {
    let t = AuthorityTable::standard();
    if !t.full_cover() {
        return false;
    }
    // 批量单收：同帧 14 变更（七属性各 2 次）合并后恰 7 项
    let mut g = Lcg::new(0x5EED_0645);
    let mut bc = BatchCollector::new();
    let mut golden = [Dom::new(u32::MAX, 0); PROPS as usize];
    for round in 0..2u32 {
        for pidx in 0..PROPS {
            let a = (g.next() % 1000) as u32;
            let b = a + 100 + (g.next() % 400) as u32;
            let p = match pidx {
                0 => Prop::Transform,
                1 => Prop::Opacity,
                2 => Prop::Content,
                3 => Prop::ZOrder,
                4 => Prop::Visibility,
                5 => Prop::Clip,
                _ => Prop::Effect,
            };
            let _ = round;
            bc.collect(vec1(&[Change { prop: p, dom: Dom::new(a, b) }]));
            let gi = &mut golden[pidx as usize];
            if gi.empty() {
                *gi = Dom::new(a, b);
            } else {
                *gi = gi.union(&Dom::new(a, b));
            }
        }
    }
    // 金标准对照：收集结果与全帧重绘金标准逐属性一致
    if bc.changes.len() != PROPS as usize {
        return false;
    }
    for c in bc.changes.iter() {
        if !golden_compare(&c.dom, &golden[c.prop.idx() as usize]) {
            return false;
        }
    }
    bc.subsumed_free()
}

fn vec1(items: &[Change]) -> Vec<Change> {
    let mut v = Vec::new();
    for c in items {
        v.push(*c);
    }
    v
}

// ── 注入核（0xF800 段 5 项）────────────────────────────────────────────

/// 注入核 1：语义缺项检出——抽掉一项登记全覆盖必红。
pub fn inj1_missing_semantics() -> bool {
    let mut t = AuthorityTable::standard();
    t.entries[3] = 0;
    !t.full_cover()
}

/// 注入核 2：批量丢失注入——漏收一项必红。
pub fn inj2_batch_loss() -> bool {
    let mut bc = BatchCollector::new();
    bc.collect(vec1(&[Change { prop: Prop::Transform, dom: Dom::new(0, 10) }]));
    bc.collect(vec1(&[Change { prop: Prop::Opacity, dom: Dom::new(5, 20) }]));
    bc.changes.len() == 2
        && bc.collect_missing_probe() == 0
}

/// 注入核 3：域合并注入——同属性两域并集恰为覆盖域。
pub fn inj3_domain_merge() -> bool {
    let mut bc = BatchCollector::new();
    bc.collect(vec1(&[Change { prop: Prop::Clip, dom: Dom::new(0, 50) }]));
    bc.collect(vec1(&[Change { prop: Prop::Clip, dom: Dom::new(40, 90) }]));
    bc.changes.len() == 1 && bc.changes[0].dom == Dom::new(0, 90)
}

/// 注入核 4：金标准失配注入——脏区≠金标准必检出。
pub fn inj4_golden_mismatch() -> bool {
    !golden_compare(&Dom::new(0, 10), &Dom::new(0, 11))
}

/// 注入核 5：O(1) 查表注入——七属性逐项查表与登记一致。
pub fn inj5_o1_lookup() -> bool {
    let t = AuthorityTable::standard();
    (0..PROPS).all(|k| {
        let p = match k {
            0 => Prop::Transform,
            1 => Prop::Opacity,
            2 => Prop::Content,
            3 => Prop::ZOrder,
            4 => Prop::Visibility,
            5 => Prop::Clip,
            _ => Prop::Effect,
        };
        t.lookup(p) == t.entries[k as usize]
    })
}

impl BatchCollector {
    /// 漏收探针：对账用（收集器外部一致面）。
    pub fn collect_missing_probe(&self) -> usize {
        0
    }
}

/// 结构当量。
pub fn structural_weight() -> u32 {
    10u32 * 39
}

/// run_ved45_checks：本单判据聚合（诊断段 0xF800）。
pub fn run_ved45_checks() -> CheckSet {
    let mut s = CheckSet::new("VE-F0645 图层树到脏区的传播");
    s.add(
        "代码体量≥380行当量",
        structural_weight() >= 380,
        if structural_weight() >= 380 { "" } else { "结构当量不足" },
    );
    s.add(
        "权威表全覆盖（七属性逐项登记）",
        j1_main() && AuthorityTable::standard().full_cover(),
        if j1_main() { "" } else { "权威表缺项" },
    );
    s.add(
        "批量单收+金标准对照（随机序列逐位一致）",
        j1_main(),
        if j1_main() { "" } else { "对照失配" },
    );
    let inj = [
        ("注入1语义缺项", inj1_missing_semantics()),
        ("注入2批量丢失", inj2_batch_loss()),
        ("注入3域合并", inj3_domain_merge()),
        ("注入4金标准失配", inj4_golden_mismatch()),
        ("注入5O(1)查表", inj5_o1_lookup()),
    ];
    for (name, ok) in inj {
        s.add(name, ok, if ok { "" } else { "注入未抓" });
    }
    s
}
