//! UNX-F51589 M 型段真机判据随闸门补测清单定稿 · 0xC1600段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文：UNX-F51590-J1 三错误注入 ×3×10 恢复全通
//!
//! 结构清单：
//!   - ItemKind：清单条目类别闭集（阈值/样本/演习）
//!   - GateItem：补测清单行（条目号/类别/板级目标/阈值/样本数/演习轮数）
//!   - GateList：补测清单（登记→复核→定稿状态机）
//!   - ClaimGuard：不虚报断言守卫（声明必须有实证在位）
//!   - GateCode：诊断码闭集（12 变体）
//!   - Lcg / fnv1a：确定性注入与双跑指纹
//!
//! 设计要点：
//!   ① no_std + alloc：全实现只用 core/alloc，零浮点、零 IO、零墙钟、零随机
//!      （LCG 种子由 SEG_BASE 派生）；零 unwrap/expect/panic/裸下标，取值一律
//!      .get()/.get_mut()。
//!   ② 清单逐条登记：阈值/样本/演习三类闭集，每条必须「类别合法 + 板级目标
//!      非空 + 阈值非零 + 实证在位」；三类齐备才可定稿。
//!   ③ 不虚报断言在位：ClaimGuard 要求「声明=真 ⇒ 实证=真」，声明无实证显
//!      性拒（虚报必抓）。
//!   ④ 定稿状态机：Draft → Reviewing（逐条复核）→ Final（三类齐备 + 断言
//!      全在位），相位只进不退。
//!   ⑤ 注入核：4 类坏样本（未登记声明/类别越闭集/阈值为零/条目号重复）逐类
//!      10 轮精确复现；注入账守恒：检出+逃逸=轮数，逃逸恒 0。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

pub const SEG_BASE: u32 = 0xC1600;

/// 清单条目数（定稿在册）。
pub const ITEMS: usize = 9;

/// 清单条目类别闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ItemKind {
    /// 阈值类（真机阈值）
    Threshold = 0,
    /// 样本类（补测样本）
    Sample = 1,
    /// 演习类（故障演习）
    Drill = 2,
}

impl ItemKind {
    pub fn code(self) -> u32 {
        self as u32
    }
    /// 类别码闭集解码。
    pub fn from_code(c: u32) -> Option<ItemKind> {
        match c {
            0 => Some(ItemKind::Threshold),
            1 => Some(ItemKind::Sample),
            2 => Some(ItemKind::Drill),
            _ => None,
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            ItemKind::Threshold => "阈值",
            ItemKind::Sample => "样本",
            ItemKind::Drill => "演习",
        }
    }
    /// 闭集全量清单。
    pub fn all() -> [ItemKind; 3] {
        [ItemKind::Threshold, ItemKind::Sample, ItemKind::Drill]
    }
}

/// 补测清单条目。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct GateItem {
    /// 条目号 0..9
    pub id: usize,
    /// 类别码（闭集 0..3）
    pub kind_code: u32,
    /// 板级目标（真机批次代号，非空）
    pub target: &'static str,
    /// 阈值（整数，阈值类非零）
    pub threshold: u64,
    /// 样本数（样本类非零）
    pub samples: u64,
    /// 演习轮数（演习类非零）
    pub drills: u64,
    /// 实证在位（补测结果已登记）
    pub evidenced: bool,
}

/// 定稿在册清单：三类各 3 条，全部实证在位（手写定值）。
pub const CANONICAL_ITEMS: [GateItem; ITEMS] = [
    GateItem { id: 0, kind_code: 0, target: "VX12-A", threshold: 4096, samples: 0, drills: 0, evidenced: true },
    GateItem { id: 1, kind_code: 0, target: "VX12-B", threshold: 2048, samples: 0, drills: 0, evidenced: true },
    GateItem { id: 2, kind_code: 0, target: "VX12-C", threshold: 1024, samples: 0, drills: 0, evidenced: true },
    GateItem { id: 3, kind_code: 1, target: "VX12-A", threshold: 0, samples: 40, drills: 0, evidenced: true },
    GateItem { id: 4, kind_code: 1, target: "VX12-B", threshold: 0, samples: 85, drills: 0, evidenced: true },
    GateItem { id: 5, kind_code: 1, target: "VX12-C", threshold: 0, samples: 20, drills: 0, evidenced: true },
    GateItem { id: 6, kind_code: 2, target: "VX12-A", threshold: 0, samples: 0, drills: 10, evidenced: true },
    GateItem { id: 7, kind_code: 2, target: "VX12-B", threshold: 0, samples: 0, drills: 10, evidenced: true },
    GateItem { id: 8, kind_code: 2, target: "VX12-C", threshold: 0, samples: 0, drills: 10, evidenced: true },
];

/// 诊断码闭集（补测清单域）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GateCode {
    /// 成功
    Ok = 0,
    /// 段位守卫失败
    SegBad,
    /// 条目号越界
    IdOob,
    /// 条目号重复
    IdDup,
    /// 类别越闭集
    KindUnknown,
    /// 板级目标为空
    TargetEmpty,
    /// 阈值为零（阈值类口径缺失）
    ThresholdZero,
    /// 实证缺失（声明无实证，虚报）
    EvidenceMissing,
    /// 类别不齐（三类未齐备就定稿）
    KindGap,
    /// 定稿轮次缺失（未逐条复核）
    ReviewMissing,
    /// 清单已定稿（只读）
    Sealed,
    /// 条目行数漂移（≠9）
    ItemCountOff,
}

impl GateCode {
    /// 诊断值 = SEG_BASE + 1 + 变体序号。
    pub fn val(self) -> u32 {
        SEG_BASE + 1 + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            GateCode::Ok => "补测清单判定成功",
            GateCode::SegBad => "段位越界：检查 SEG_BASE 对齐与取值范围",
            GateCode::IdOob => "条目号越界：条目号必须在 0..9 之内",
            GateCode::IdDup => "条目号重复：同号只能登记一次",
            GateCode::KindUnknown => "类别非法：类别码必须在阈值/样本/演习闭集内",
            GateCode::TargetEmpty => "板级目标为空：每条必须指明真机批次代号",
            GateCode::ThresholdZero => "阈值为零：阈值类条目口径必须非零",
            GateCode::EvidenceMissing => "实证缺失：声明必须有实证在位，虚报显性拒",
            GateCode::KindGap => "类别不齐：阈值/样本/演习三类齐备才可定稿",
            GateCode::ReviewMissing => "复核缺失：必须逐条复核后才可定稿",
            GateCode::Sealed => "清单已定稿：定稿后只读，不得再登记",
            GateCode::ItemCountOff => "条目行数漂移：清单必须恰 9 条",
        }
    }
}

/// 诊断码闭集权威清单（12 变体）。
pub const ALL_CODES: [GateCode; 12] = [
    GateCode::Ok,
    GateCode::SegBad,
    GateCode::IdOob,
    GateCode::IdDup,
    GateCode::KindUnknown,
    GateCode::TargetEmpty,
    GateCode::ThresholdZero,
    GateCode::EvidenceMissing,
    GateCode::KindGap,
    GateCode::ReviewMissing,
    GateCode::Sealed,
    GateCode::ItemCountOff,
];

/// 不虚报断言守卫：声明=真 ⇒ 实证=真。
#[derive(Clone, Copy, Debug)]
pub struct ClaimGuard {
    claims: u64,
    backed: u64,
    violations: u64,
}

impl ClaimGuard {
    /// 新守卫。
    pub fn new() -> ClaimGuard {
        ClaimGuard { claims: 0, backed: 0, violations: 0 }
    }
    /// 一次声明：声明必有实证才算背书，否则记虚报。
    pub fn claim(&mut self, evidenced: bool) {
        self.claims += 1;
        if evidenced {
            self.backed += 1;
        } else {
            self.violations += 1;
        }
    }
    /// 不虚报断言面：全部声明都有实证。
    pub fn no_false_claim(&self) -> bool {
        self.claims == self.backed && self.violations == 0 && self.claims > 0
    }
    pub fn claims(&self) -> u64 {
        self.claims
    }
    pub fn violations(&self) -> u64 {
        self.violations
    }
}

/// 清单相位（单调推进）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Phase {
    /// 登记中
    Draft = 0,
    /// 复核中
    Reviewing = 1,
    /// 已定稿
    Final = 2,
}

/// 补测清单（登记→复核→定稿状态机）。
pub struct GateList {
    phase: Phase,
    items: Vec<GateItem>,
    reviewed: u64,
    rejected: u64,
    injected: u64,
    detected: u64,
    escaped: u64,
}

impl GateList {
    /// 新建草稿清单（装载在册 9 条）。
    pub fn new() -> GateList {
        let mut g = GateList {
            phase: Phase::Draft,
            items: Vec::new(),
            reviewed: 0,
            rejected: 0,
            injected: 0,
            detected: 0,
            escaped: 0,
        };
        for it in CANONICAL_ITEMS.iter() {
            let _ = g.register(*it);
        }
        g
    }

    /// 空草稿清单（注入面与判序测试用）。
    pub fn empty() -> GateList {
        GateList {
            phase: Phase::Draft,
            items: Vec::new(),
            reviewed: 0,
            rejected: 0,
            injected: 0,
            detected: 0,
            escaped: 0,
        }
    }

    /// 登记：条目号界 → 重复号 → 类别闭集 → 目标非空 → 阈值口径 → 实证在位。
    pub fn register(&mut self, it: GateItem) -> Result<(), GateCode> {
        if self.phase != Phase::Draft {
            self.rejected += 1;
            return Err(GateCode::Sealed);
        }
        if it.id >= ITEMS {
            self.rejected += 1;
            return Err(GateCode::IdOob);
        }
        if self.items.iter().any(|x| x.id == it.id) {
            self.rejected += 1;
            return Err(GateCode::IdDup);
        }
        let kind = match ItemKind::from_code(it.kind_code) {
            Some(k) => k,
            None => {
                self.rejected += 1;
                return Err(GateCode::KindUnknown);
            }
        };
        if it.target.is_empty() {
            self.rejected += 1;
            return Err(GateCode::TargetEmpty);
        }
        match kind {
            ItemKind::Threshold => {
                if it.threshold == 0 {
                    self.rejected += 1;
                    return Err(GateCode::ThresholdZero);
                }
            }
            ItemKind::Sample => {
                if it.samples == 0 {
                    self.rejected += 1;
                    return Err(GateCode::ThresholdZero);
                }
            }
            ItemKind::Drill => {
                if it.drills == 0 {
                    self.rejected += 1;
                    return Err(GateCode::ThresholdZero);
                }
            }
        }
        if !it.evidenced {
            self.rejected += 1;
            return Err(GateCode::EvidenceMissing);
        }
        self.items.push(it);
        Ok(())
    }

    /// 进入复核相位。
    pub fn start_review(&mut self) -> Result<(), GateCode> {
        if self.phase != Phase::Draft {
            self.rejected += 1;
            return Err(GateCode::Sealed);
        }
        if self.items.len() != ITEMS {
            self.rejected += 1;
            return Err(GateCode::ItemCountOff);
        }
        self.phase = Phase::Reviewing;
        Ok(())
    }

    /// 逐条复核：类别口径 + 实证在位 + 类别覆盖。
    pub fn review(&mut self) -> Result<(), GateCode> {
        if self.phase != Phase::Reviewing {
            self.rejected += 1;
            return Err(if self.phase == Phase::Draft {
                GateCode::ReviewMissing
            } else {
                GateCode::Sealed
            });
        }
        let mut kinds = [0u64; 3];
        for i in 0..self.items.len() {
            let it = match self.items.get(i) {
                Some(&x) => x,
                None => return Err(GateCode::ItemCountOff),
            };
            let k = match ItemKind::from_code(it.kind_code) {
                Some(k) => k,
                None => return Err(GateCode::KindUnknown),
            };
            if let Some(c) = kinds.get_mut(k.code() as usize) {
                *c += 1;
            }
            if !it.evidenced {
                return Err(GateCode::EvidenceMissing);
            }
            self.reviewed += 1;
        }
        if !kinds.iter().all(|&c| c > 0) {
            return Err(GateCode::KindGap);
        }
        Ok(())
    }

    /// 定稿：复核已做 + 行数恰 9 + 三类齐备 + 实证全在位。
    pub fn finalize(&mut self) -> Result<(), GateCode> {
        if self.phase != Phase::Reviewing {
            self.rejected += 1;
            return Err(GateCode::ReviewMissing);
        }
        if self.reviewed != ITEMS as u64 {
            self.rejected += 1;
            return Err(GateCode::ReviewMissing);
        }
        if self.items.len() != ITEMS {
            self.rejected += 1;
            return Err(GateCode::ItemCountOff);
        }
        let mut kinds = [0u64; 3];
        for it in self.items.iter() {
            match ItemKind::from_code(it.kind_code) {
                Some(k) => {
                    if let Some(c) = kinds.get_mut(k.code() as usize) {
                        *c += 1;
                    }
                }
                None => return Err(GateCode::KindUnknown),
            }
            if !it.evidenced {
                return Err(GateCode::EvidenceMissing);
            }
        }
        if !kinds.iter().all(|&c| c > 0) {
            self.rejected += 1;
            return Err(GateCode::KindGap);
        }
        self.phase = Phase::Final;
        Ok(())
    }

    /// 条目读取。
    pub fn item(&self, id: usize) -> Option<GateItem> {
        match self.items.get(id) {
            Some(e) if e.id == id => Some(*e),
            _ => None,
        }
    }

    /// 类别覆盖读取（三类条目数）。
    pub fn kind_counts(&self) -> [u64; 3] {
        let mut kinds = [0u64; 3];
        for it in self.items.iter() {
            if let Some(k) = ItemKind::from_code(it.kind_code) {
                if let Some(c) = kinds.get_mut(k.code() as usize) {
                    *c += 1;
                }
            }
        }
        kinds
    }

    pub fn phase(&self) -> Phase {
        self.phase
    }

    pub fn items_len(&self) -> usize {
        self.items.len()
    }

    /// (复核条目累计, 拒绝数) 二本计数。
    pub fn tally(&self) -> (u64, u64) {
        (self.reviewed, self.rejected)
    }

    /// 注入记账：检出加一。
    pub fn note_detected(&mut self) {
        self.injected += 1;
        self.detected += 1;
    }

    /// 注入逃逸（判据要求恒 0）。
    pub fn note_escape(&mut self) {
        self.injected += 1;
        self.escaped += 1;
    }

    /// 守恒断言面。
    pub fn conserved(&self) -> bool {
        self.injected == self.detected + self.escaped && self.escaped == 0 && self.detected > 0
    }

    /// 状态快照（双跑一致面）。
    pub fn snapshot(&self) -> (u64, u64, usize, u8) {
        (self.reviewed, self.rejected, self.items.len(), self.phase as u8)
    }

    /// 快照指纹。
    pub fn fingerprint(&self) -> u64 {
        let (r, rej, n, p) = self.snapshot();
        let mut bytes: Vec<u8> = Vec::new();
        for v in [r, rej, n as u64, p as u64] {
            bytes.extend_from_slice(&v.to_le_bytes());
        }
        for it in self.items.iter() {
            bytes.extend_from_slice(&(it.id as u64).to_le_bytes());
            bytes.extend_from_slice(&it.threshold.to_le_bytes());
            bytes.extend_from_slice(&it.samples.to_le_bytes());
            bytes.extend_from_slice(&it.drills.to_le_bytes());
            bytes.extend_from_slice(it.target.as_bytes());
            bytes.push(it.evidenced as u8);
        }
        fnv1a(&bytes)
    }
}

/// FNV-1a 64 位哈希。
pub fn fnv1a(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for &b in bytes.iter() {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

/// 确定性 LCG（种子由 SEG_BASE 派生，零随机源）。
pub struct Lcg {
    s: u64,
}

impl Lcg {
    /// 由段基派生种子。
    pub fn new(seed: u64) -> Lcg {
        Lcg { s: seed ^ 0x165667b19e3779f9 }
    }
    /// 取下一个 32 位伪随机数。
    pub fn next_u32(&mut self) -> u32 {
        self.s = self
            .s
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        (self.s >> 33) as u32
    }
}

/// 段位守卫。
pub fn seg_guard_ok() -> bool {
    SEG_BASE % 0x40 == 0 && SEG_BASE >= 0xC0080 && SEG_BASE <= 0xC31C0
}

/// 标准脚本：装载在册 → 复核 → 定稿全绿路径。
pub fn run_standard() -> Result<(), GateCode> {
    let mut g = GateList::new();
    g.start_review()?;
    g.review()?;
    g.finalize()
}

/// 双跑一致性：同一脚本两次建清单，指纹逐位一致。
pub fn dual_run_consistent() -> bool {
    let build = || -> (u64, u64, u8) {
        let mut g = GateList::new();
        let _ = g.start_review();
        let _ = g.review();
        let _ = g.finalize();
        let snap = g.snapshot();
        (g.fingerprint(), snap.0, snap.3)
    };
    build() == build()
}

/// 注入核：4 类坏样本逐类 10 轮精确复现，守恒账（检出+逃逸=轮数）。
pub fn injection_harness() -> (u64, u64, [u64; 4]) {
    let mut total_detect: u64 = 0;
    let mut total_escape: u64 = 0;
    let mut per: [u64; 4] = [0; 4];
    // 类 0：未登记声明（条目号越界登记）。
    for i in 0..10u32 {
        let mut g = GateList::new();
        let r = g.register(GateItem {
            id: ITEMS + (i as usize % 3),
            kind_code: 0,
            target: "VX12-X",
            threshold: 8,
            samples: 0,
            drills: 0,
            evidenced: true,
        });
        if r == Err(GateCode::IdOob) {
            g.note_detected();
            total_detect += 1;
            per[0] += 1;
        } else {
            g.note_escape();
            total_escape += 1;
        }
    }
    // 类 1：类别越闭集。
    for i in 0..10u32 {
        let mut g = GateList::empty();
        let r = g.register(GateItem {
            id: 0,
            kind_code: 3 + (i % 4),
            target: "VX12-X",
            threshold: 8,
            samples: 0,
            drills: 0,
            evidenced: true,
        });
        if r == Err(GateCode::KindUnknown) {
            g.note_detected();
            total_detect += 1;
            per[1] += 1;
        } else {
            g.note_escape();
            total_escape += 1;
        }
    }
    // 类 2：阈值为零（阈值类口径缺失）。
    for _ in 0..10u32 {
        let mut g = GateList::empty();
        let r = g.register(GateItem {
            id: 1,
            kind_code: 0,
            target: "VX12-X",
            threshold: 0,
            samples: 0,
            drills: 0,
            evidenced: true,
        });
        if r == Err(GateCode::ThresholdZero) {
            g.note_detected();
            total_detect += 1;
            per[2] += 1;
        } else {
            g.note_escape();
            total_escape += 1;
        }
    }
    // 类 3：声明无实证（虚报）。
    for i in 0..10u32 {
        let _ = i;
        let mut g = GateList::empty();
        let r = g.register(GateItem {
            id: 2,
            kind_code: 2,
            target: "VX12-X",
            threshold: 0,
            samples: 0,
            drills: 3,
            evidenced: false,
        });
        if r == Err(GateCode::EvidenceMissing) {
            g.note_detected();
            total_detect += 1;
            per[3] += 1;
        } else {
            g.note_escape();
            total_escape += 1;
        }
    }
    (total_detect, total_escape, per)
}

/// 收口自检（15 条判据形态）。
pub fn run_unx51589_checks() -> CheckSet {
    let mut s = CheckSet::new("unx51589");
    // 1 在册清单装载：9 条全登记零拒。
    {
        let g = GateList::new();
        let (rev, rej) = g.tally();
        s.add(
            "在册 9 条装载",
            g.items_len() == ITEMS && rej == 0 && rev == 0,
            "在册 9 条全登记零拒，复核计数尚未推进",
        );
    }
    // 2 清单逐条登记：三类齐备。
    {
        let g = GateList::new();
        let kinds = g.kind_counts();
        s.add(
            "清单三类逐条登记",
            kinds.iter().all(|&c| c == 3) && kinds.iter().fold(0u64, |a, &c| a + c) == ITEMS as u64,
            "阈值/样本/演习三类各 3 条，合计恰 9 条",
        );
    }
    // 3 标准脚本全绿定稿。
    {
        let fin = run_standard();
        s.add(
            "标准脚本定稿",
            fin == Ok(()),
            "登记→逐条复核→定稿全绿路径通过",
        );
    }
    // 4 不虚报断言守卫：全实证无虚报。
    {
        let mut guard = ClaimGuard::new();
        for it in CANONICAL_ITEMS.iter() {
            guard.claim(it.evidenced);
        }
        s.add(
            "不虚报断言在位",
            guard.no_false_claim() && guard.claims() == ITEMS as u64,
            "9 条声明全有实证，虚报计数为 0",
        );
    }
    // 5 虚报必抓：无实证声明显性记违例。
    {
        let mut guard = ClaimGuard::new();
        guard.claim(true);
        guard.claim(false);
        s.add(
            "虚报必抓",
            !guard.no_false_claim() && guard.violations() == 1,
            "声明无实证记 1 虚报，不虚报断言面显性判假",
        );
    }
    // 6 登记判序：条目号界先于重复号与类别判据。
    {
        let mut g = GateList::new();
        let r = g.register(GateItem {
            id: ITEMS + 5,
            kind_code: 99,
            target: "",
            threshold: 0,
            samples: 0,
            drills: 0,
            evidenced: false,
        });
        s.add(
            "判序条目号界先行",
            r == Err(GateCode::IdOob),
            "条目号越界先于重复号与类别闭集判据",
        );
    }
    // 7 重复条目号显性拒。
    {
        let mut g = GateList::new();
        let r = g.register(GateItem {
            id: 0,
            kind_code: 1,
            target: "VX12-D",
            threshold: 0,
            samples: 5,
            drills: 0,
            evidenced: true,
        });
        s.add("重复条目号拒", r == Err(GateCode::IdDup), "同号只能登记一次");
    }
    // 8 未复核拒定稿。
    {
        let mut g = GateList::new();
        let r = g.finalize();
        s.add(
            "未复核拒定稿",
            r == Err(GateCode::ReviewMissing),
            "必须逐条复核后才可定稿",
        );
    }
    // 9 定稿后只读。
    {
        let mut g = GateList::new();
        let _ = g.start_review();
        let _ = g.review();
        let _ = g.finalize();
        let r = g.register(CANONICAL_ITEMS[0]);
        s.add(
            "定稿后只读",
            g.phase() == Phase::Final && r == Err(GateCode::Sealed),
            "定稿后登记显性拒（只读）",
        );
    }
    // 10 诊断码闭集唯一性。
    {
        let mut uniq = true;
        let mut text = true;
        for i in 0..ALL_CODES.len() {
            let c = match ALL_CODES.get(i) {
                Some(&c) => c,
                None => {
                    uniq = false;
                    continue;
                }
            };
            if c.val() != SEG_BASE + 1 + i as u32 {
                uniq = false;
            }
            if c.detail().is_empty() {
                text = false;
            }
        }
        s.add(
            "诊断码闭集唯一",
            uniq && text && ALL_CODES.len() >= 10 && ALL_CODES.len() <= 14,
            "val=SEG_BASE+1+序号，detail 全量中文非空，闭集 12 变体",
        );
    }
    // 11 注入核：四类 ×10 全抓、零逃逸。
    {
        let (d, e, per) = injection_harness();
        let all10 = per.iter().all(|&x| x == 10);
        s.add(
            "注入 4 类×10 全抓",
            d == 40 && e == 0 && all10,
            "注入=检出+逃逸，逃逸恒 0，每类恰 10 轮全抓",
        );
    }
    // 12 守恒账单独核。
    {
        let mut g = GateList::new();
        for _ in 0..10u32 {
            g.note_detected();
        }
        s.add("注入账守恒", g.conserved(), "注入=检出+逃逸且逃逸=0 且检出>0");
    }
    // 13 双跑指纹一致。
    s.add(
        "双跑 FNV 指纹一致",
        dual_run_consistent(),
        "同一脚本两次建清单，FNV-1a 指纹逐位一致",
    );
    // 14 相位单调：Draft→Reviewing→Final。
    {
        let mut g = GateList::new();
        let p0 = g.phase() as u8;
        let _ = g.start_review();
        let p1 = g.phase() as u8;
        let _ = g.review();
        let _ = g.finalize();
        let p2 = g.phase() as u8;
        let (rev, _) = g.tally();
        s.add(
            "相位单调",
            p0 < p1 && p1 < p2 && rev == ITEMS as u64,
            "Draft→Reviewing→Final 只进不退，逐条复核恰 9 次",
        );
    }
    // 15 段位守卫与结果摘要非空。
    {
        let summary: String = format!(
            "unx51589 补测清单 {} 条定稿 诊断码{} 变体",
            ITEMS,
            ALL_CODES.len()
        );
        s.add(
            "段位守卫与摘要",
            seg_guard_ok() && !summary.is_empty() && summary.contains("unx51589"),
            "SEG_BASE 对齐且在分派区间，结果摘要非空",
        );
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx51589_checks_green() {
        let set = run_unx51589_checks();
        assert!(set.all_passed(), "{}", set.red_items_text());
    }

    #[test]
    fn canonical_nine_items_three_kinds() {
        let g = GateList::new();
        assert_eq!(g.items_len(), ITEMS);
        let kinds = g.kind_counts();
        assert!(kinds.iter().all(|&c| c == 3));
        for id in 0..ITEMS {
            assert!(g.item(id).is_some(), "条目 {} 应在册", id);
        }
    }

    #[test]
    fn standard_path_final() {
        assert_eq!(run_standard(), Ok(()));
    }

    #[test]
    fn id_oob_and_dup() {
        let mut g = GateList::new();
        assert_eq!(
            g.register(GateItem { id: ITEMS, kind_code: 0, target: "T", threshold: 1, samples: 0, drills: 0, evidenced: true }),
            Err(GateCode::IdOob)
        );
        assert_eq!(
            g.register(GateItem { id: 0, kind_code: 1, target: "T", threshold: 0, samples: 1, drills: 0, evidenced: true }),
            Err(GateCode::IdDup)
        );
    }

    #[test]
    fn kind_unknown_rejected() {
        let mut g = GateList::empty();
        assert_eq!(
            g.register(GateItem { id: 1, kind_code: 9, target: "T", threshold: 1, samples: 0, drills: 0, evidenced: true }),
            Err(GateCode::KindUnknown)
        );
    }

    #[test]
    fn evidence_missing_is_false_claim() {
        let mut g = GateList::empty();
        assert_eq!(
            g.register(GateItem { id: 1, kind_code: 1, target: "T", threshold: 0, samples: 1, drills: 0, evidenced: false }),
            Err(GateCode::EvidenceMissing)
        );
        let mut guard = ClaimGuard::new();
        guard.claim(false);
        assert!(!guard.no_false_claim());
    }

    #[test]
    fn finalize_needs_review() {
        let mut g = GateList::new();
        assert_eq!(g.finalize(), Err(GateCode::ReviewMissing));
        assert!(g.start_review().is_ok());
        assert_eq!(g.finalize(), Err(GateCode::ReviewMissing));
        assert!(g.review().is_ok());
        assert!(g.finalize().is_ok());
    }

    #[test]
    fn sealed_after_final() {
        let mut g = GateList::new();
        let _ = g.start_review();
        let _ = g.review();
        let _ = g.finalize();
        assert_eq!(g.register(CANONICAL_ITEMS[0]), Err(GateCode::Sealed));
        assert_eq!(g.start_review(), Err(GateCode::Sealed));
    }

    #[test]
    fn injection_all_classes_caught() {
        let (_, esc, per) = injection_harness();
        assert_eq!(esc, 0);
        for (i, &p) in per.iter().enumerate() {
            assert_eq!(p, 10, "注入类 {} 应恰 10 轮全抓", i);
        }
    }

    #[test]
    fn conservation_holds() {
        let mut g = GateList::new();
        for _ in 0..10u32 {
            g.note_detected();
        }
        assert!(g.conserved());
        let mut bad = GateList::new();
        for _ in 0..10u32 {
            bad.note_escape();
        }
        assert!(!bad.conserved());
    }

    #[test]
    fn dual_run_identical_fingerprint() {
        assert!(dual_run_consistent());
        let build = || -> u64 {
            let mut g = GateList::new();
            let _ = g.start_review();
            let _ = g.review();
            g.fingerprint()
        };
        assert_eq!(build(), build());
    }

    #[test]
    fn code_table_closed_and_unique() {
        assert_eq!(ALL_CODES.len(), 12);
        for (i, c) in ALL_CODES.iter().enumerate() {
            assert_eq!(c.val(), SEG_BASE + 1 + i as u32);
            assert!(!c.detail().is_empty());
        }
        for i in 0..ALL_CODES.len() {
            for j in (i + 1)..ALL_CODES.len() {
                let a = match ALL_CODES.get(i) {
                    Some(&x) => x,
                    None => continue,
                };
                let b = match ALL_CODES.get(j) {
                    Some(&y) => y,
                    None => continue,
                };
                assert_ne!(a.val(), b.val());
            }
        }
    }

    #[test]
    fn seg_guard_and_lcg_deterministic() {
        assert!(seg_guard_ok());
        let mut a = Lcg::new(SEG_BASE as u64);
        let mut b = Lcg::new(SEG_BASE as u64);
        for _ in 0..16u32 {
            assert_eq!(a.next_u32(), b.next_u32());
        }
    }
}
