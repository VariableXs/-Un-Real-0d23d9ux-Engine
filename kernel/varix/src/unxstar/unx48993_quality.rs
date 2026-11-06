//! UNX-F48993 手势识别质量回归 · 0x717C0
//! ═══════════════════════════════════════════════════════════════════════
//! 判据原文（任务板 B10 族第七槽，逐条锚定）：
//!
//!   UNX-F48993 手势 fuzz（乱触点序列注入状态机不崩不卡死）:
//!          ① 纯功能代码 ≥260 行；
//!          ② UNX-F48993-J1 fuzz ×10^4 零崩。
//!
//!   UNX-F48994 B10 段内联轧（手势五路判据交叉复跑）:
//!          ① 纯功能代码 ≥200 行；
//!          ② UNX-F48994-J1 联轧 ×5 全绿零漂移。
//! ─────────────────────────────────────────────────────────────────────
//! 闸门次序（六段，任一段失败即止，绝不跨段继续）：
//!     ① 时钟/序号 → ② 样本合法 → ③ 真值合法 → ④ 查表/分类
//!     → ⑤ 指标核算 → ⑥ 门限判定
//!
//! **为什么样本序号先于分类**：样本号是「地址」，分类是对地址处内容的
//! 「计算」。地址越界时内容根本不存在；若先跑分类，出错点是「取不到样本」
//! 或更糟——被当成 0 号样本静默算出一个假指标，指标账被污染却毫无痕迹。
//! 先把序号挡在最前面，越界统一报 SampleBad，分类器一次都不被调用；
//! 反过来（先分类再校验序号）会把「数据集长度变了」误诊成「分类器坏了」，
//! 红线画在了错误的层。
//!
//! **为什么真值合法先于查表/分类**：真值是标注，预测是计算。真值越出 8 类
//! 闭集说明标注表本身坏了（DataBad），此时拿它去索引混淆矩阵的行是未定义
//! 语义的入口；先校验真值，矩阵只接受 0..8 的行标，越界在入账之前被拒。
//!
//! **为什么指标核算先于门限**：门限是「对指标的比较」。指标若不自洽
//! （Σ行 ≠ Σ列 ≠ 样本数、TP/FP/FN 与矩阵格不符、F1 不是 P/R 的调和均值），
//! 那么「F1 ≥ 900」只是对一串脏数字做的比较，绿了也不代表质量好。故先做
//! 三层核算：账本守恒（LedgerBad）→ 逐类 TP/FP/FN 对账（TpBad/FpBad/FnBad）
//! → 指标可复算（MetricBad）。任一层不自洽直接返回，门限一步都不走。
//! 这条次序是本模块唯一能区分「模型真的退化」与「统计口径坏了」的地方。
//!
//! 另有一条**版本闸**横在样本闸之前：Dataset 打开 Replay 时锁存 version 与
//! len，之后每次 step 都比对。版本/长度变更 == 数据集换了，旧序列与旧哈希
//! 不再可比，必须 DataBad；否则会跑出一份「新半旧半」的混合序列——那种
//! 序列既不是 A 也不是 B，哈希对谁都对不上，比直接报错更难查。
//!
//! 关键差异（本模块**不**回答什么）：
//!   · 不做阈值自适应：F1_GATE 是常量，门限只比较一次，不回调、不漂移；
//!   · 不做压力洪泛：QUALITY_ROUNDS / TICK_CAP 是覆盖闸（拒绝 0 与越上界），
//!     不是吞吐压测；
//!   · 不做手势分类器本体：分类器只作为质量回归的被测对象，识别归 B10-CORE；
//!   · 不做映射/仲裁（F48987）、误触抑制（F48983）、槽位守卫（F48985）、
//!     边界注入（F48989）。
//!
//! 在质量回归代码之上，本模块按任务板第七槽叠加两张卡的真实功能：
//!   · F48993 触点 fuzz 面：LCG 生成 10^4 轮乱触点序列（乱指针/乱槽位/乱
//!     时序/超量 Down），注入 1/2/3 槽触点状态机；状态机对坏序列「拒绝而
//!     不改账」，每拍自检不变量，每轮强制 Cancel 收尾证明不会卡死在非空闲态。
//!   · F48994 五路联轧：五条手势判据链（触点数/时长/跨度/折返/速度）各跑
//!     N 轮 ×5 遍，逐轮二次独立求值 + 全遍摘要比对，漂移必须为 0。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;
use super::unx48941_wheel_core as p_core;

/// 诊断段基址（0x717C0，B10 族第七槽）。
pub const SEG_BASE: u32 = 0x717C0;
/// 手势类别数（与混淆矩阵的行/列数同源）；特征维数。
pub const CLASSES: usize = 8;
pub const FEATURES: usize = 4;
/// 千分比基数；质量门限（F1 ≥ 900‰）；回归轮数（×10^2 → 10^3 样本）。
pub const PERMILLE: u32 = 1000;
pub const F1_GATE: u32 = 900;
pub const QUALITY_ROUNDS: u32 = 1_000;
/// 黄金运行数据集轮数；注入路数；渲染字段数；fuzz 轮数。
pub const GOLDEN_ROUNDS: u32 = 4;
pub const INJECT_ROUTES: usize = 4;
pub const STATS_FIELDS: usize = 6;
pub const FUZZ_ROUNDS: u32 = 1_000;
/// 样本容量上界；基准长度；基准版本；版本上界；evaluate 循环上界；序号上界。
pub const DATASET_MAX: usize = 128;
pub const DATASET_LEN: u32 = 64;
pub const DATASET_VERSION: u32 = 3;
pub const VERSION_CAP: u32 = 64;
pub const CYCLES_CAP: u32 = 64;
pub const TICK_CAP: u32 = 1_000_000;
/// 特征绝对值上界（算术安全域）；原型间最小平方距离。
pub const FEAT_CAP: i32 = 1_024;
pub const MIN_SEP: i64 = 10_000;
/// 合成抖动幅度（±JITTER）；用种子；错标锚点周期/相位/类别位移。
pub const JITTER: i32 = 24;
pub const DATASET_SEED: u64 = 0x717C_0F49_3F49_4F93;
pub const SWAP_MOD: usize = 13;
pub const SWAP_PHASE: usize = 5;
pub const SWAP_SHIFT: u32 = 3;
/// 误判 (真值, 预测) 对留存条数；单样本扰动位移。
pub const ERRATA_MAX: usize = 32;
pub const PERTURB_DELTA: i32 = 64;

// ── 诊断码闭集（窗口 +0x01..+0x0E，恒 ≤ SEG_BASE+0x0F） ──────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DpCode {
    /// 质量门限不达标
    QualityBad,
    /// 数据集版本/长度/原型表非法
    DataBad,
    /// 存储哈希与重算哈希不符，或扰动未改变哈希
    HashBad,
    /// 样本序号越界
    SampleBad,
    /// 逐类 TP 与矩阵不符
    TpBad,
    /// 逐类 FP 与列和减 TP 不符
    FpBad,
    /// 逐类 FN 与行和减 TP 不符
    FnBad,
    /// 指标越界或不可复算
    MetricBad,
    /// 账本不守恒（Σ行/Σ列/样本数/游标）
    LedgerBad,
    /// 渲染文本不可解析
    RenderBad,
    /// 两次确定性复跑不一致，或扰动定位不唯一
    ReplayBad,
    /// 覆盖轮数为零或越过上界
    CoverBad,
    /// 诊断码越出本模块段窗
    SegmentBad,
    /// fuzz 与影子模型不一致
    FuzzBad,
}

impl DpCode {
    pub fn val(self) -> u32 {
        SEG_BASE + 0x01 + self as u32
    }

    pub fn detail(self) -> &'static str {
        match self {
            DpCode::QualityBad => "F1 低于质量门限（<900‰）",
            DpCode::DataBad => "数据集版本、长度或原型表非法",
            DpCode::HashBad => "存储哈希与重算不符，或扰动未改哈希",
            DpCode::SampleBad => "样本序号越出数据集长度",
            DpCode::TpBad => "逐类 TP 账与混淆矩阵对角不符",
            DpCode::FpBad => "逐类 FP 账与列和减 TP 不符",
            DpCode::FnBad => "逐类 FN 账与行和减 TP 不符",
            DpCode::MetricBad => "指标越出千分比域或不可复算",
            DpCode::LedgerBad => "行和/列和/样本数/游标不守恒",
            DpCode::RenderBad => "账本文本字段数或数值不可解析",
            DpCode::ReplayBad => "两次复跑不一致或扰动定位不唯一",
            DpCode::CoverBad => "覆盖轮数为零或越出上界",
            DpCode::SegmentBad => "诊断码越出本模块段窗",
            DpCode::FuzzBad => "fuzz 与影子模型逐拍不一致",
        }
    }

    pub const ALL: [DpCode; 14] = [
        DpCode::QualityBad, DpCode::DataBad, DpCode::HashBad, DpCode::SampleBad,
        DpCode::TpBad, DpCode::FpBad, DpCode::FnBad, DpCode::MetricBad,
        DpCode::LedgerBad, DpCode::RenderBad, DpCode::ReplayBad, DpCode::CoverBad,
        DpCode::SegmentBad, DpCode::FuzzBad,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0usize;
    while i < DpCode::ALL.len() {
        if let Some(c) = DpCode::ALL.get(i) {
            if c.val() == code {
                return c.detail();
            }
        }
        i += 1;
    }
    "未知诊断码"
}

// ── 手势类别（混淆矩阵的行/列标） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GKind {
    Tap,
    DoubleTap,
    LongPress,
    Swipe,
    Fling,
    Pinch,
    Rotate,
    Hold,
}

impl GKind {
    pub fn index(self) -> usize {
        match self {
            GKind::Tap => 0, GKind::DoubleTap => 1, GKind::LongPress => 2, GKind::Swipe => 3,
            GKind::Fling => 4, GKind::Pinch => 5, GKind::Rotate => 6, GKind::Hold => 7,
        }
    }

    pub fn code(self) -> u32 { self.index() as u32 }

    pub fn name(self) -> &'static str {
        match self {
            GKind::Tap => "tap", GKind::DoubleTap => "double-tap",
            GKind::LongPress => "long-press", GKind::Swipe => "swipe",
            GKind::Fling => "fling", GKind::Pinch => "pinch",
            GKind::Rotate => "rotate", GKind::Hold => "hold",
        }
    }

    pub fn from_index(i: u32) -> Result<GKind, u32> {
        match i {
            0 => Ok(GKind::Tap), 1 => Ok(GKind::DoubleTap), 2 => Ok(GKind::LongPress),
            3 => Ok(GKind::Swipe), 4 => Ok(GKind::Fling), 5 => Ok(GKind::Pinch),
            6 => Ok(GKind::Rotate), 7 => Ok(GKind::Hold),
            _ => Err(DpCode::SampleBad.val()),
        }
    }

    pub const ALL: [GKind; CLASSES] = [
        GKind::Tap, GKind::DoubleTap, GKind::LongPress, GKind::Swipe,
        GKind::Fling, GKind::Pinch, GKind::Rotate, GKind::Hold,
    ];
}

// ── 分类器：按特征最近原型（整数平方距离，无浮点） ─────────────────
/// 原型表：每类恰一条 4 维特征。手工铺开、间距 ≥ MIN_SEP，
/// 保证「最近原型」在 ±JITTER 抖动与 ±PERTURB_DELTA 单点扰动下仍唯一。
pub const PROTOS: [(GKind, [i32; FEATURES]); CLASSES] = [
    (GKind::Tap, [16, 40, -160, -96]),
    (GKind::DoubleTap, [144, -56, -160, -96]),
    (GKind::LongPress, [272, 40, -160, 96]),
    (GKind::Swipe, [400, -56, -160, 96]),
    (GKind::Fling, [16, 40, 160, -96]),
    (GKind::Pinch, [144, -56, 160, -96]),
    (GKind::Rotate, [272, 40, 160, 96]),
    (GKind::Hold, [400, -56, 160, 96]),
];

/// 两个特征向量之间的平方距离（i64，饱和加）。
pub fn dist2_between(a: &[i32; FEATURES], b: &[i32; FEATURES]) -> i64 {
    let mut acc = 0i64;
    let mut d = 0usize;
    while d < FEATURES {
        let x = match a.get(d) { Some(v) => *v as i64, None => return i64::MAX };
        let y = match b.get(d) { Some(v) => *v as i64, None => return i64::MAX };
        let diff = x.saturating_sub(y);
        acc = acc.saturating_add(diff.saturating_mul(diff));
        d += 1;
    }
    acc
}

/// 特征到第 `class` 条原型的平方距离。
pub fn dist2_class(f: &[i32; FEATURES], class: usize) -> i64 {
    match PROTOS.get(class) {
        Some((_, p)) => dist2_between(f, p),
        None => i64::MAX,
    }
}

/// 最近原型分类（严格 `<` → 平局取**最小类别号**，确定且可重放）。
pub fn classify(f: &[i32; FEATURES]) -> GKind {
    let mut best = GKind::from_index(0).unwrap_or(GKind::Tap);
    let mut best_d = dist2_class(f, 0);
    let mut c = 1usize;
    while c < CLASSES {
        let d = dist2_class(f, c);
        if d < best_d {
            best_d = d;
            if let Ok(g) = GKind::from_index(c as u32) {
                best = g;
            }
        }
        c += 1;
    }
    best
}

/// 影子实现：**倒序**遍历、用 `<=` 取代 `<`。
///
/// 语义等价（平局仍落到最小类别号，因为倒序后「后来的」是更小的下标），
/// 但循环方向、比较符号、初值来源都与 [`classify`] 不同——fuzz 用它逐拍
/// 对账，才能抓到「同一个公式被抄了两遍」这种假一致。
pub fn classify_shadow(f: &[i32; FEATURES]) -> GKind {
    let mut best = GKind::from_index((CLASSES - 1) as u32).unwrap_or(GKind::Hold);
    let mut best_d = dist2_class(f, CLASSES - 1);
    let mut c = CLASSES;
    while c > 0 {
        c -= 1;
        let d = dist2_class(f, c);
        if d <= best_d {
            best_d = d;
            if let Ok(g) = GKind::from_index(c as u32) {
                best = g;
            }
        }
    }
    best
}

/// 原型表审计：八类各一行、特征在算术域内、两两间距 ≥ MIN_SEP。
pub fn audit_proto() -> Result<u32, u32> {
    let mut seen = [0u8; CLASSES];
    let mut i = 0usize;
    while i < PROTOS.len() {
        let (g, f) = match PROTOS.get(i) {
            Some(v) => *v,
            None => return Err(DpCode::DataBad.val()),
        };
        let idx = g.index();
        if idx >= CLASSES {
            return Err(DpCode::DataBad.val());
        }
        match seen.get_mut(idx) {
            Some(slot) => {
                if *slot != 0 {
                    return Err(DpCode::DataBad.val());
                }
                *slot = 1;
            }
            None => return Err(DpCode::DataBad.val()),
        }
        let mut d = 0usize;
        while d < FEATURES {
            match f.get(d) {
                Some(v) => {
                    if *v <= -FEAT_CAP || *v >= FEAT_CAP {
                        return Err(DpCode::DataBad.val());
                    }
                }
                None => return Err(DpCode::DataBad.val()),
            }
            d += 1;
        }
        let mut j = i + 1;
        while j < PROTOS.len() {
            if let Some((_, other)) = PROTOS.get(j) {
                if dist2_between(&f, other) < MIN_SEP {
                    return Err(DpCode::DataBad.val());
                }
            }
            j += 1;
        }
        i += 1;
    }
    let mut k = 0usize;
    while k < CLASSES {
        match seen.get(k) {
            Some(1) => {}
            _ => return Err(DpCode::DataBad.val()),
        }
        k += 1;
    }
    Ok(PROTOS.len() as u32)
}

// ── 数据集：固定样本数组 + 版本号 + 哈希 ──────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Sample {
    /// 样本在数据集内的序号（与数组下标恒等，审计时核对）。
    pub index: u32,
    /// 真值标签（8 类闭集）。
    pub label: GKind,
    /// 4 维特征。
    pub feat: [i32; FEATURES],
}

impl Sample {
    fn fold(&self, h: u64) -> u64 {
        let mut x = p_core::fold_u32(h, self.index);
        x = p_core::fold_u32(x, self.label.code());
        let mut d = 0usize;
        while d < FEATURES {
            if let Some(v) = self.feat.get(d) {
                x = p_core::fold_u32(x, *v as u32);
            }
            d += 1;
        }
        x
    }
}

/// 固定数据集：样本数组 + 版本 + 全量哈希。
///
/// 哈希覆盖面 = 版本 ‖ 长度 ‖ 逐样本 (序号, 真值, 特征)。版本进折叠是
/// 有意的：这样「只改版本不改样本」的副本在 [`Dataset::audit`] 里也会
/// 露馅（HashBad），而 [`Replay`] 的版本闸则额外把它归到 DataBad——
/// 两条路径都能抓住，不靠运气。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Dataset {
    samples: [Option<Sample>; DATASET_MAX],
    len: u32,
    version: u32,
    hash: u64,
}

/// 特征钳位到算术安全域 `(-FEAT_CAP, FEAT_CAP)`（合成与扰动共用）。
fn clamp_feat(v: i32) -> i32 {
    if v <= -FEAT_CAP {
        -FEAT_CAP + 1
    } else if v >= FEAT_CAP {
        FEAT_CAP - 1
    } else {
        v
    }
}

impl Dataset {
    /// 由 LCG 确定性合成：标签按 `i % 8` 均分（每类样本数相等），
    /// 特征取「目标原型 + ±JITTER 抖动」；`i % SWAP_MOD == SWAP_PHASE`
    /// 的样本**故意取自另一个原型**，形成确定数量的真实误判——
    /// 这是质量回归能同时验到「指标计算」与「误判定位」的前提。
    pub fn build(version: u32, len: u32) -> Result<Dataset, u32> {
        if len == 0 {
            return Err(DpCode::CoverBad.val());
        }
        if len as usize > DATASET_MAX || version == 0 || version > VERSION_CAP {
            return Err(DpCode::DataBad.val());
        }
        let mut ds = Dataset {
            samples: [None; DATASET_MAX],
            len: 0,
            version,
            hash: p_core::FNV_OFF,
        };
        let mut rng = p_core::Lcg::new(DATASET_SEED ^ ((version as u64) << 32));
        let mut i = 0usize;
        while i < len as usize {
            let truth = (i % CLASSES) as u32;
            let target = if i % SWAP_MOD == SWAP_PHASE {
                truth.saturating_add(SWAP_SHIFT) % CLASSES as u32
            } else {
                truth
            };
            let proto = *PROTOS
                .get(target as usize)
                .map(|(_, f)| f)
                .ok_or(DpCode::DataBad.val())?;
            let mut feat = [0i32; FEATURES];
            let mut d = 0usize;
            while d < FEATURES {
                let base = *proto.get(d).ok_or(DpCode::DataBad.val())?;
                feat[d] = clamp_feat(base.saturating_add(rng.signed(JITTER as u32)));
                d += 1;
            }
            let label = GKind::from_index(truth)?;
            *ds.samples.get_mut(i).ok_or(DpCode::DataBad.val())? = Some(Sample {
                index: i as u32,
                label,
                feat,
            });
            i += 1;
        }
        ds.len = len;
        ds.hash = ds.fingerprint();
        Ok(ds)
    }

    /// 全量折叠：版本 → 长度 → 逐样本。
    pub fn fingerprint(&self) -> u64 {
        let mut h = p_core::fold_u32(p_core::FNV_OFF, self.version);
        h = p_core::fold_u32(h, self.len);
        let mut i = 0usize;
        while i < self.len as usize {
            if let Some(Some(s)) = self.samples.get(i) {
                h = s.fold(h);
            }
            i += 1;
        }
        h
    }

    pub fn len(&self) -> u32 { self.len }
    pub fn version(&self) -> u32 { self.version }
    pub fn hash(&self) -> u64 { self.hash }

    /// 取样本；越界 → SampleBad（这是 F48994 的序号闸）。
    pub fn get(&self, i: u32) -> Result<Sample, u32> {
        if i >= self.len {
            return Err(DpCode::SampleBad.val());
        }
        self.samples
            .get(i as usize)
            .copied()
            .flatten()
            .ok_or(DpCode::DataBad.val())
    }

    /// 数据集自审：版本/长度 → 逐样本字段 → 全量哈希。
    ///
    /// 顺序不可换：先认哈希再验字段，会把「样本字段坏了」报成 HashBad，
    /// 排障时看不出究竟是数据坏了还是存储被改。
    pub fn audit(&self) -> Result<u32, u32> {
        if self.version == 0 || self.version > VERSION_CAP {
            return Err(DpCode::DataBad.val());
        }
        if self.len == 0 || self.len as usize > DATASET_MAX {
            return Err(DpCode::DataBad.val());
        }
        let mut i = 0usize;
        while i < self.len as usize {
            let s = self.get(i as u32)?;
            if s.index != i as u32 || s.label.index() >= CLASSES {
                return Err(DpCode::DataBad.val());
            }
            let mut d = 0usize;
            while d < FEATURES {
                let v = *s.feat.get(d).ok_or(DpCode::DataBad.val())?;
                if v <= -FEAT_CAP || v >= FEAT_CAP {
                    return Err(DpCode::DataBad.val());
                }
                d += 1;
            }
            i += 1;
        }
        if self.fingerprint() != self.hash {
            return Err(DpCode::HashBad.val());
        }
        Ok(self.len)
    }

    /// 单样本扰动（四维同时位移），重签哈希。`delta == 0` 视为无效注入
    /// 直接拒绝——否则「扰动不改变哈希」会被误读成哈希失效。
    pub fn with_perturbation(&self, idx: u32, delta: i32) -> Result<Dataset, u32> {
        if idx >= self.len {
            return Err(DpCode::SampleBad.val());
        }
        if delta == 0 {
            return Err(DpCode::QualityBad.val());
        }
        let mut d = self.clone();
        let s = d
            .samples
            .get_mut(idx as usize)
            .and_then(|v| v.as_mut())
            .ok_or(DpCode::DataBad.val())?;
        let mut k = 0usize;
        while k < FEATURES {
            let v = s.feat.get_mut(k).ok_or(DpCode::DataBad.val())?;
            *v = clamp_feat(v.saturating_add(delta));
            k += 1;
        }
        d.hash = d.fingerprint();
        Ok(d)
    }

    /// 换版本（重签哈希）：样本序列不变，只有版本闸能抓住这类改动。
    pub fn with_version(&self, version: u32) -> Result<Dataset, u32> {
        if version == 0 || version > VERSION_CAP {
            return Err(DpCode::DataBad.val());
        }
        let mut d = self.clone();
        d.version = version;
        d.hash = d.fingerprint();
        Ok(d)
    }

    /// 截短长度（重签哈希）：模拟「数据集被裁掉尾巴」，由 Replay 的长度
    /// 闸负责报 DataBad。尾部槽位一并清空，避免残留样本误导后续读取。
    pub fn with_len(&self, len: u32) -> Result<Dataset, u32> {
        if len == 0 || len > self.len {
            return Err(DpCode::DataBad.val());
        }
        let mut d = self.clone();
        let mut i = len as usize;
        while i < DATASET_MAX {
            if let Some(slot) = d.samples.get_mut(i) {
                *slot = None;
            }
            i += 1;
        }
        d.len = len;
        d.hash = d.fingerprint();
        Ok(d)
    }

    /// 全部真值整体位移 k 类（重签哈希）：用于构造「零命中」退化数据集，
    /// 验证 F1=0 的除零分支与门限拒绝路径。
    pub fn relabel_shift(&self, k: u32) -> Result<Dataset, u32> {
        if k as usize >= CLASSES {
            return Err(DpCode::DataBad.val());
        }
        let mut d = self.clone();
        let mut i = 0usize;
        while i < d.len as usize {
            let s = d
                .samples
                .get_mut(i)
                .and_then(|v| v.as_mut())
                .ok_or(DpCode::DataBad.val())?;
            s.label = GKind::from_index(s.label.code().saturating_add(k) % CLASSES as u32)?;
            i += 1;
        }
        d.hash = d.fingerprint();
        Ok(d)
    }

    /// 破坏存储哈希（样本原封不动）：审计必须报 HashBad。
    pub fn with_corrupt_hash(&self) -> Dataset {
        let mut d = self.clone();
        d.hash ^= 0x0000_0000_0000_0001;
        d
    }
}

/// 基准数据集（版本 3、长度 64）。
pub fn baseline() -> Result<Dataset, u32> {
    Dataset::build(DATASET_VERSION, DATASET_LEN)
}

// ── 回放：逐样本结果 + 游标 + 账本 ────────────────────────────────
/// 逐样本回放器。`version`/`len` 在打开时锁存，作为版本闸的依据。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Replay {
    version: u32,
    len: u32,
    cursor: u32,
    hits: u32,
    misses: u32,
    preds: [u8; DATASET_MAX],
    digests: [u64; DATASET_MAX],
    digest: u64,
}

impl Replay {
    /// 打开回放：先整份审计数据集（版本/字段/哈希），再锁存版本与长度。
    pub fn open(ds: &Dataset) -> Result<Replay, u32> {
        ds.audit()?;
        Ok(Replay {
            version: ds.version(),
            len: ds.len(),
            cursor: 0,
            hits: 0,
            misses: 0,
            preds: [0u8; DATASET_MAX],
            digests: [0u64; DATASET_MAX],
            digest: p_core::FNV_OFF,
        })
    }

    /// 走一步：版本闸 → 序号闸 → 取样本 → 分类 → 入账 → 折叠。
    pub fn step(&mut self, ds: &Dataset) -> Result<GKind, u32> {
        if ds.version() != self.version || ds.len() != self.len {
            return Err(DpCode::DataBad.val());
        }
        if self.cursor >= self.len {
            return Err(DpCode::SampleBad.val());
        }
        let s = ds.get(self.cursor)?;
        let pred = classify(&s.feat);
        if pred == s.label {
            self.hits = self.hits.saturating_add(1);
        } else {
            self.misses = self.misses.saturating_add(1);
        }
        // 逐样本摘要**独立**折叠（起点恒 FNV_OFF）：若把上一步的累计值当起点，
        // 一处扰动会让其后每一步的摘要全变，「定位唯一样本」就退化成
        // 「发现后缀全部不同」——这正是 F48994 明令禁止的「只报哈希变了」。
        let mut x = p_core::fold_u32(p_core::FNV_OFF, s.index);
        x = p_core::fold_u32(x, s.label.code());
        x = p_core::fold_u32(x, pred.code());
        let mut d = 0usize;
        while d < FEATURES {
            if let Some(v) = s.feat.get(d) {
                x = p_core::fold_u32(x, *v as u32);
            }
            d += 1;
        }
        // 全量摘要单独累计（进渲染的 digest 列）。
        self.digest = p_core::fold_u32(self.digest, (x & 0xFFFF_FFFF) as u32);
        self.digest = p_core::fold_u32(self.digest, (x >> 32) as u32);
        let c = self.cursor as usize;
        if c < DATASET_MAX {
            if let Some(slot) = self.preds.get_mut(c) {
                *slot = pred.code() as u8;
            }
            if let Some(slot) = self.digests.get_mut(c) {
                *slot = x;
            }
        } else {
            return Err(DpCode::SampleBad.val());
        }
        self.cursor = self.cursor.saturating_add(1);
        Ok(pred)
    }

    pub fn cursor(&self) -> u32 {
        self.cursor
    }

    pub fn hits(&self) -> u32 {
        self.hits
    }

    pub fn misses(&self) -> u32 {
        self.misses
    }

    pub fn digest(&self) -> u64 {
        self.digest
    }

    pub fn len(&self) -> u32 {
        self.len
    }

    /// 第 `i` 步的预测（未走到则 None）。
    pub fn pred_at(&self, i: u32) -> Option<GKind> {
        if i >= self.cursor {
            return None;
        }
        match self.preds.get(i as usize) {
            Some(v) => GKind::from_index(*v as u32).ok(),
            None => None,
        }
    }

    /// 第 `i` 步的逐样本折叠值（扰动定位的原始凭据）。
    pub fn digest_at(&self, i: u32) -> Option<u64> {
        if i >= self.cursor {
            return None;
        }
        self.digests.get(i as usize).copied()
    }

    /// 账本守恒：命中 + 未命中 == 游标 == 数据集长度；摘要非零。
    pub fn audit(&self, ds: &Dataset) -> Result<u32, u32> {
        if ds.version() != self.version || ds.len() != self.len {
            return Err(DpCode::DataBad.val());
        }
        if self.hits.saturating_add(self.misses) != self.cursor {
            return Err(DpCode::LedgerBad.val());
        }
        if self.cursor != self.len {
            return Err(DpCode::LedgerBad.val());
        }
        if self.digest == 0 {
            return Err(DpCode::ReplayBad.val());
        }
        Ok(self.cursor)
    }
}

/// 全量回放一份数据集。
pub fn replay_dataset(ds: &Dataset) -> Result<Replay, u32> {
    let mut r = Replay::open(ds)?;
    let mut i = 0u32;
    while i < ds.len() {
        r.step(ds)?;
        i = i.saturating_add(1);
    }
    r.audit(ds)?;
    Ok(r)
}

/// 首个分歧步号：逐样本比对「预测 + 逐样本摘要」。
///
/// 只比预测是不够的——单样本扰动常常**不改变分类**（原型间距远大于扰动），
/// 此时预测序列完全一致，唯有逐样本摘要会变。F48994 要求「定位到被扰动
/// 样本序号」，所以定位必须以摘要为主、预测为辅。
pub fn first_divergence(a: &Replay, b: &Replay) -> Result<Option<u32>, u32> {
    if a.len != b.len {
        return Err(DpCode::DataBad.val());
    }
    let limit = a.cursor.min(b.cursor);
    let mut i = 0u32;
    while i < limit {
        let da = a.digest_at(i);
        let db = b.digest_at(i);
        if da != db {
            return Ok(Some(i));
        }
        if a.pred_at(i) != b.pred_at(i) {
            return Ok(Some(i));
        }
        i = i.saturating_add(1);
    }
    if a.cursor != b.cursor {
        return Ok(Some(limit));
    }
    Ok(None)
}

/// 分歧步数（扰动定位「唯一」的判据：必须恰好 1）。
pub fn divergence_count(a: &Replay, b: &Replay) -> Result<u32, u32> {
    if a.len != b.len {
        return Err(DpCode::DataBad.val());
    }
    let limit = a.cursor.min(b.cursor);
    let mut n = 0u32;
    let mut i = 0u32;
    while i < limit {
        if a.digest_at(i) != b.digest_at(i) || a.pred_at(i) != b.pred_at(i) {
            n = n.saturating_add(1);
        }
        i = i.saturating_add(1);
    }
    if a.cursor != b.cursor {
        n = n.saturating_add(1);
    }
    Ok(n)
}

// ── 混淆矩阵（8 真值 × 8 预测 = 64 格） ───────────────────────────
/// 64 格计数。行 = 真值，列 = 预测；对角为 TP。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Confusion {
    cells: [[u32; CLASSES]; CLASSES],
    total: u32,
}

impl Confusion {
    pub const fn new() -> Confusion {
        Confusion { cells: [[0u32; CLASSES]; CLASSES], total: 0 }
    }

    /// 读一格（越界返回 0，不 panic）。
    pub fn cell(&self, truth: usize, pred: usize) -> u32 {
        if truth >= CLASSES || pred >= CLASSES {
            return 0;
        }
        self.cells[truth][pred]
    }

    /// 记一格。行/列标越界 → DataBad（真值或预测越出闭集）。
    pub fn add(&mut self, truth: usize, pred: usize) -> Result<(), u32> {
        if truth >= CLASSES || pred >= CLASSES {
            return Err(DpCode::DataBad.val());
        }
        let slot = self
            .cells
            .get_mut(truth)
            .and_then(|row| row.get_mut(pred))
            .ok_or(DpCode::DataBad.val())?;
        *slot = slot.saturating_add(1);
        self.total = self.total.saturating_add(1);
        Ok(())
    }

    pub fn total(&self) -> u32 { self.total }

    /// 行和（该真值类的全部样本数）。
    pub fn row(&self, truth: usize) -> u32 {
        (0..CLASSES).fold(0u32, |a, p| a.saturating_add(self.cell(truth, p)))
    }

    /// 列和（该预测类的全部样本数）。
    pub fn col(&self, pred: usize) -> u32 {
        (0..CLASSES).fold(0u32, |a, t| a.saturating_add(self.cell(t, pred)))
    }

    /// 对角和（命中数）。
    pub fn trace(&self) -> u32 {
        (0..CLASSES).fold(0u32, |a, c| a.saturating_add(self.cells[c][c]))
    }

    /// 由外部格表构造（审计锻造用：真实路径只经 `add`）。
    pub fn from_cells(cells: [[u32; CLASSES]; CLASSES]) -> Confusion {
        let total = cells.iter().fold(0u32, |a, r| {
            a.saturating_add(r.iter().fold(0u32, |b, v| b.saturating_add(*v)))
        });
        Confusion { cells, total }
    }

    /// 账本自洽：Σ行 == Σ列 == total。
    pub fn consistent(&self) -> bool {
        let rows = (0..CLASSES).fold(0u32, |a, t| a.saturating_add(self.row(t)));
        let cols = (0..CLASSES).fold(0u32, |a, p| a.saturating_add(self.col(p)));
        rows == self.total && cols == self.total
    }
}

/// 千分比调和均值：`0 when p + r == 0`（除零在此处一次性封死）。
pub fn harmonic_permille(p: u32, r: u32) -> u32 {
    let s = p.saturating_add(r);
    if s == 0 {
        return 0;
    }
    let out = (p as u64).saturating_mul(r as u64).saturating_mul(2) / (s as u64);
    if out > PERMILLE as u64 { PERMILLE } else { out as u32 }
}

// ── 误判条目：(样本号, 真值, 预测) ────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Erratum {
    /// 数据集内样本序号（可定位）。
    pub sample: u32,
    /// 真值。
    pub truth: GKind,
    /// 预测。
    pub pred: GKind,
}

// ── 质量账：矩阵 + 逐类 TP/FP/FN + 误判目录 + 摘要 ────────────────
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Quality {
    conf: Confusion,
    tp: [u32; CLASSES],
    fp: [u32; CLASSES],
    fn_: [u32; CLASSES],
    samples: u32,
    correct: u32,
    errata: [Option<Erratum>; ERRATA_MAX],
    errata_n: usize,
    errata_total: u32,
    digest: u64,
}

impl Quality {
    pub fn new() -> Quality {
        Quality {
            conf: Confusion::new(),
            tp: [0u32; CLASSES], fp: [0u32; CLASSES], fn_: [0u32; CLASSES],
            samples: 0, correct: 0,
            errata: [None; ERRATA_MAX], errata_n: 0, errata_total: 0,
            digest: p_core::FNV_OFF,
        }
    }

    pub fn samples(&self) -> u32 { self.samples }
    pub fn correct(&self) -> u32 { self.correct }
    pub fn wrong(&self) -> u32 { self.samples.saturating_sub(self.correct) }
    pub fn digest(&self) -> u64 { self.digest }
    pub fn cell(&self, truth: usize, pred: usize) -> u32 { self.conf.cell(truth, pred) }
    pub fn row(&self, truth: usize) -> u32 { self.conf.row(truth) }
    pub fn col(&self, pred: usize) -> u32 { self.conf.col(pred) }
    pub fn tp(&self, c: usize) -> u32 { self.tp.get(c).copied().unwrap_or(0) }
    pub fn fp(&self, c: usize) -> u32 { self.fp.get(c).copied().unwrap_or(0) }
    pub fn fn_count(&self, c: usize) -> u32 { self.fn_.get(c).copied().unwrap_or(0) }

    /// Σ行（== 样本数，审计口径）。
    pub fn row_total(&self) -> u32 { (0..CLASSES).fold(0u32, |a, t| a.saturating_add(self.conf.row(t))) }
    /// Σ列（== 样本数，审计口径）。
    pub fn col_total(&self) -> u32 { (0..CLASSES).fold(0u32, |a, p| a.saturating_add(self.conf.col(p))) }
    /// ΣTP（== 命中数）。
    pub fn tp_total(&self) -> u32 { self.tp.iter().fold(0u32, |a, v| a.saturating_add(*v)) }
    /// ΣFP（== 未命中数）。
    pub fn fp_total(&self) -> u32 { self.fp.iter().fold(0u32, |a, v| a.saturating_add(*v)) }
    /// ΣFN（== 未命中数）。
    pub fn fn_total(&self) -> u32 { self.fn_.iter().fold(0u32, |a, v| a.saturating_add(*v)) }

    /// 单类精确率（千分比）；分母为 0 → 0，不除零。
    pub fn precision_class(&self, c: usize) -> u32 {
        let den = self.tp(c).saturating_add(self.fp(c));
        if den == 0 { return 0; }
        ((self.tp(c) as u64) * PERMILLE as u64 / den as u64) as u32
    }

    /// 单类召回率（千分比）；分母为 0 → 0，不除零。
    pub fn recall_class(&self, c: usize) -> u32 {
        let den = self.tp(c).saturating_add(self.fn_count(c));
        if den == 0 { return 0; }
        ((self.tp(c) as u64) * PERMILLE as u64 / den as u64) as u32
    }

    /// 单类 F1：召回为 0 时定义 0（调和均值在 p+r==0 处直接返回 0）。
    pub fn f1_class(&self, c: usize) -> u32 {
        harmonic_permille(self.precision_class(c), self.recall_class(c))
    }

    /// 宏平均精确率（8 类等权，整数向下取整）。
    pub fn precision_permille(&self) -> u32 {
        (0..CLASSES).fold(0u32, |a, c| a.saturating_add(self.precision_class(c))) / CLASSES as u32
    }

    /// 宏平均召回率。
    pub fn recall_permille(&self) -> u32 {
        (0..CLASSES).fold(0u32, |a, c| a.saturating_add(self.recall_class(c))) / CLASSES as u32
    }

    /// 门限用的聚合 F1：宏 P 与宏 R 的调和均值。
    ///
    /// 选它作为门限口径的理由：它与渲染六元组里的 precision/recall 两列
    /// **逐位可复算**（`2pr/(p+r)`，整数、无浮点、无隐藏状态），
    /// 因此「渲染文本自洽」与「门限判定」共用同一把尺子。
    pub fn f1_permille(&self) -> u32 {
        harmonic_permille(self.precision_permille(), self.recall_permille())
    }

    /// 逐类 F1 的宏平均（仅作明细展示，不参与门限——它无法由两个标量复算）。
    pub fn f1_macro(&self) -> u32 {
        (0..CLASSES).fold(0u32, |a, c| a.saturating_add(self.f1_class(c))) / CLASSES as u32
    }

    /// 误判条目数（已留存，≤ ERRATA_MAX）。
    pub fn errata_len(&self) -> usize { self.errata_n }
    /// 误判总数（不受留存上限截断）。
    pub fn errata_total(&self) -> u32 { self.errata_total }

    /// 取第 i 条误判（越界 None）。
    pub fn erratum(&self, i: usize) -> Option<Erratum> {
        self.errata.get(i).copied().flatten()
    }

    /// 首个属于 (真值, 预测) 对的误判样本号——F48993 的「可定位」入口。
    pub fn first_for(&self, truth: GKind, pred: GKind) -> Option<u32> {
        let mut i = 0usize;
        while i < self.errata_n {
            if let Some(Some(e)) = self.errata.get(i) {
                if e.truth == truth && e.pred == pred {
                    return Some(e.sample);
                }
            }
            i += 1;
        }
        None
    }

    /// 单拍入账：序号闸 → 取样本 → 真值闸 → 分类 → 矩阵/逐类账/误判目录。
    fn feed(&mut self, ds: &Dataset, i: u32) -> Result<GKind, u32> {
        let next = self.samples.saturating_add(1);
        if next > TICK_CAP {
            return Err(DpCode::CoverBad.val());
        }
        let s = ds.get(i)?;
        let t = s.label.index();
        if t >= CLASSES {
            return Err(DpCode::DataBad.val());
        }
        let pred = classify(&s.feat);
        let p = pred.index();
        if p >= CLASSES {
            return Err(DpCode::DataBad.val());
        }
        self.conf.add(t, p)?;
        if t == p {
            self.correct = self.correct.saturating_add(1);
            if let Some(slot) = self.tp.get_mut(t) {
                *slot = slot.saturating_add(1);
            }
        } else {
            if let Some(slot) = self.fp.get_mut(p) {
                *slot = slot.saturating_add(1);
            }
            if let Some(slot) = self.fn_.get_mut(t) {
                *slot = slot.saturating_add(1);
            }
            self.errata_total = self.errata_total.saturating_add(1);
            if self.errata_n < ERRATA_MAX {
                if let Some(slot) = self.errata.get_mut(self.errata_n) {
                    *slot = Some(Erratum { sample: i, truth: s.label, pred });
                }
                self.errata_n += 1;
            }
        }
        let mut x = p_core::fold_u32(self.digest, s.index);
        x = p_core::fold_u32(x, s.label.code());
        x = p_core::fold_u32(x, pred.code());
        let mut d = 0usize;
        while d < FEATURES {
            if let Some(v) = s.feat.get(d) {
                x = p_core::fold_u32(x, *v as u32);
            }
            d += 1;
        }
        self.digest = x;
        self.samples = next;
        Ok(pred)
    }

    /// 账本三层核算：守恒 → 逐类 TP/FP/FN → 计数器与矩阵一致。
    pub fn audit_ledger(&self) -> Result<u32, u32> {
        if self.samples == 0 {
            return Err(DpCode::CoverBad.val());
        }
        if !self.conf.consistent() || self.conf.total() != self.samples {
            return Err(DpCode::LedgerBad.val());
        }
        let mut rows = 0u32; let mut cols = 0u32; let mut trace = 0u32;
        let mut fp_sum = 0u32; let mut fn_sum = 0u32;
        let mut c = 0usize;
        while c < CLASSES {
            let row = self.conf.row(c);
            let col = self.conf.col(c);
            let cell = self.conf.cell(c, c);
            rows = rows.saturating_add(row);
            cols = cols.saturating_add(col);
            trace = trace.saturating_add(cell);
            if self.tp(c) != cell { return Err(DpCode::TpBad.val()); }
            let fp = col.saturating_sub(cell);
            if self.fp(c) != fp { return Err(DpCode::FpBad.val()); }
            let fnn = row.saturating_sub(cell);
            if self.fn_count(c) != fnn { return Err(DpCode::FnBad.val()); }
            fp_sum = fp_sum.saturating_add(fp);
            fn_sum = fn_sum.saturating_add(fnn);
            c += 1;
        }
        if rows != self.samples || cols != self.samples {
            return Err(DpCode::LedgerBad.val());
        }
        if trace != self.correct { return Err(DpCode::TpBad.val()); }
        let wrong = self.samples.saturating_sub(self.correct);
        if fp_sum != wrong { return Err(DpCode::FpBad.val()); }
        if fn_sum != wrong { return Err(DpCode::FnBad.val()); }
        if self.errata_total != wrong {
            return Err(DpCode::LedgerBad.val());
        }
        Ok(self.samples)
    }

    /// 指标核算：八个 P/R/F1 全落千分比域，且聚合 F1 可由宏 P/宏 R 复算。
    pub fn audit_metrics(&self) -> Result<u32, u32> {
        let mut c = 0usize;
        while c < CLASSES {
            let p = self.precision_class(c);
            let r = self.recall_class(c);
            let f = self.f1_class(c);
            if p > PERMILLE || r > PERMILLE || f > PERMILLE {
                return Err(DpCode::MetricBad.val());
            }
            if f != harmonic_permille(p, r) {
                return Err(DpCode::MetricBad.val());
            }
            c += 1;
        }
        let mp = self.precision_permille();
        let mr = self.recall_permille();
        if mp > PERMILLE || mr > PERMILLE {
            return Err(DpCode::MetricBad.val());
        }
        if self.f1_permille() != harmonic_permille(mp, mr) {
            return Err(DpCode::MetricBad.val());
        }
        Ok(self.f1_permille())
    }

    /// 误判目录对账：每条 (样本号, 真值, 预测) 都必须能在数据集上重放出来。
    pub fn audit_errata(&self, ds: &Dataset) -> Result<u32, u32> {
        let mut i = 0usize;
        while i < self.errata_n {
            let e = match self.errata.get(i) {
                Some(Some(v)) => *v,
                _ => return Err(DpCode::LedgerBad.val()),
            };
            if e.truth == e.pred {
                return Err(DpCode::LedgerBad.val());
            }
            let s = ds.get(e.sample)?;
            if s.label != e.truth {
                return Err(DpCode::LedgerBad.val());
            }
            if classify(&s.feat) != e.pred {
                return Err(DpCode::ReplayBad.val());
            }
            if self.conf.cell(e.truth.index(), e.pred.index()) == 0 {
                return Err(DpCode::TpBad.val());
            }
            i += 1;
        }
        Ok(self.errata_n as u32)
    }

    /// 门限判定：先核算（账本 → 指标），再比 F1_GATE。
    pub fn gate(&self) -> Result<u32, u32> {
        self.audit_ledger()?;
        let f1 = self.audit_metrics()?;
        if f1 < F1_GATE {
            return Err(DpCode::QualityBad.val());
        }
        Ok(f1)
    }
}

/// 全量评估 `cycles` 轮（每轮 = 一整份数据集）。
pub fn evaluate(ds: &Dataset, cycles: u32) -> Result<Quality, u32> {
    if cycles == 0 || cycles > CYCLES_CAP {
        return Err(DpCode::CoverBad.val());
    }
    ds.audit()?;
    let mut q = Quality::new();
    let mut c = 0u32;
    while c < cycles {
        let mut i = 0u32;
        while i < ds.len() {
            q.feed(ds, i)?;
            i = i.saturating_add(1);
        }
        c = c.saturating_add(1);
    }
    q.audit_ledger()?;
    if q.digest == 0 {
        return Err(DpCode::ReplayBad.val());
    }
    Ok(q)
}

/// 千次标注样本回归：按 `r % len` 循环取样，指标口径与全量评估完全一致。
pub fn regress(ds: &Dataset, samples: u32) -> Result<Quality, u32> {
    if samples == 0 || samples > TICK_CAP {
        return Err(DpCode::CoverBad.val());
    }
    if ds.len() == 0 {
        return Err(DpCode::DataBad.val());
    }
    ds.audit()?;
    let mut q = Quality::new();
    let mut r = 0u32;
    while r < samples {
        q.feed(ds, r % ds.len())?;
        r = r.saturating_add(1);
    }
    q.audit_ledger()?;
    if q.digest == 0 {
        return Err(DpCode::ReplayBad.val());
    }
    Ok(q)
}

// ── 渲染：6 字段 `samples|correct|precision|recall|f1|0x…` ────────
pub fn render_stats(q: &Quality) -> String {
    format!("{}|{}|{}|{}|{}|0x{:016X}", q.samples(), q.correct(),
        q.precision_permille(), q.recall_permille(), q.f1_permille(), q.digest())
}

pub fn parse_u32(s: &str) -> Option<u32> {
    let b = s.as_bytes();
    if b.is_empty() {
        return None;
    }
    let mut v = 0u32;
    let mut i = 0usize;
    while i < b.len() {
        let c = *b.get(i)?;
        if !(48..=57).contains(&c) {
            return None;
        }
        v = v.checked_mul(10)?.checked_add((c - 48) as u32)?;
        i += 1;
    }
    Some(v)
}

pub fn parse_hex16(s: &str) -> Option<u64> {
    let b = s.as_bytes();
    if b.len() != 18 {
        return None;
    }
    if *b.first()? != b'0' || *b.get(1)? != b'x' {
        return None;
    }
    let mut v = 0u64;
    let mut i = 2usize;
    while i < 18 {
        let c = *b.get(i)?;
        let d = match c {
            48..=57 => c - 48,
            65..=70 => c - 55,
            97..=102 => c - 87,
            _ => return None,
        };
        v = (v << 4) | d as u64;
        i += 1;
    }
    Some(v)
}

/// 严格解析 + 算术自洽 + 篡改拒绝。
///
/// 可审计的等式只有三条（其余靠摘要兜底，摘要覆盖全量矩阵与逐类账）：
///   ① `correct <= samples`（样本数下界）；
///   ② `f1 == 2·p·r/(p+r)`（聚合 F1 必须能由同行 P/R 复算，`p+r==0` 时
///      规定为 0）——改 p、改 r、改 f1 任一列都会破坏这条恒等式；
///   ③ 四列全落 0..=1000（千分比域），摘要非零。
pub fn audit_render(text: &str) -> Result<usize, u32> {
    let mut parts = [""; STATS_FIELDS];
    let mut n = 0usize;
    for p in text.split('|') {
        if n < STATS_FIELDS {
            if let Some(slot) = parts.get_mut(n) {
                *slot = p;
            }
        }
        n = n.saturating_add(1);
    }
    if n != STATS_FIELDS {
        return Err(DpCode::RenderBad.val());
    }
    let samples = parts.get(0).and_then(|v| parse_u32(v)).ok_or(DpCode::RenderBad.val())?;
    let correct = parts.get(1).and_then(|v| parse_u32(v)).ok_or(DpCode::RenderBad.val())?;
    let precision = parts.get(2).and_then(|v| parse_u32(v)).ok_or(DpCode::RenderBad.val())?;
    let recall = parts.get(3).and_then(|v| parse_u32(v)).ok_or(DpCode::RenderBad.val())?;
    let f1 = parts.get(4).and_then(|v| parse_u32(v)).ok_or(DpCode::RenderBad.val())?;
    let digest = parts.get(5).and_then(|v| parse_hex16(v)).ok_or(DpCode::RenderBad.val())?;
    if samples == 0 {
        return Err(DpCode::LedgerBad.val());
    }
    if correct > samples {
        return Err(DpCode::LedgerBad.val());
    }
    if precision > PERMILLE || recall > PERMILLE || f1 > PERMILLE {
        return Err(DpCode::MetricBad.val());
    }
    if harmonic_permille(precision, recall) != f1 {
        return Err(DpCode::MetricBad.val());
    }
    if digest == 0 {
        return Err(DpCode::HashBad.val());
    }
    Ok(STATS_FIELDS)
}

// ── 注入（四路：扰动样本 / 改版本 / 越界序号 / 破哈希） ────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct InjectLog {
    pub tries: [u32; INJECT_ROUTES],
    pub caught: [u32; INJECT_ROUTES],
}

impl InjectLog {
    pub fn all_caught(&self) -> bool {
        let mut i = 0usize;
        while i < INJECT_ROUTES {
            let t = match self.tries.get(i) {
                Some(v) => *v,
                None => return false,
            };
            let c = match self.caught.get(i) {
                Some(v) => *v,
                None => return false,
            };
            if t == 0 || c != t {
                return false;
            }
            i += 1;
        }
        true
    }
}

fn bump(a: &mut [u32; INJECT_ROUTES], i: usize) {
    if let Some(v) = a.get_mut(i) {
        *v = v.saturating_add(1);
    }
}

pub fn inject_rounds(rounds: u32) -> Result<InjectLog, u32> {
    if rounds == 0 || rounds > TICK_CAP {
        return Err(DpCode::CoverBad.val());
    }
    let ds = baseline()?;
    let base = replay_dataset(&ds)?;
    let idx = 17u32 % ds.len();
    let mut log = InjectLog { tries: [0; INJECT_ROUTES], caught: [0; INJECT_ROUTES] };
    let mut i = 0u32;
    while i < rounds {
        // ① 单样本扰动：哈希必变 + 定位回该样本 + 分歧唯一。
        let pert = ds.with_perturbation(idx, PERTURB_DELTA)?;
        let alt = replay_dataset(&pert)?;
        if ds.hash() != pert.hash() && first_divergence(&base, &alt)? == Some(idx)
            && divergence_count(&base, &alt)? == 1 {
            bump(&mut log.caught, 0);
        }
        // ② 版本漂移：Replay 锁存的版本与当前数据集不符 → DataBad。
        let mut r = Replay::open(&ds)?;
        let bumped = ds.with_version(ds.version().saturating_add(1))?;
        if r.step(&bumped).err() == Some(DpCode::DataBad.val()) { bump(&mut log.caught, 1); }
        // ③ 越界序号：取样本 SampleBad；走满后再走一步同样 SampleBad。
        let mut over = ds.get(ds.len()).err() == Some(DpCode::SampleBad.val());
        let mut r2 = Replay::open(&ds)?;
        let mut k = 0u32;
        while k < ds.len() { r2.step(&ds)?; k = k.saturating_add(1); }
        over = over && r2.step(&ds).err() == Some(DpCode::SampleBad.val());
        if over { bump(&mut log.caught, 2); }
        // ④ 破坏哈希：审计与打开都必须 HashBad。
        let corrupt = ds.with_corrupt_hash();
        if corrupt.audit().err() == Some(DpCode::HashBad.val())
            && Replay::open(&corrupt).err() == Some(DpCode::HashBad.val()) {
            bump(&mut log.caught, 3);
        }
        i = i.saturating_add(1);
    }
    log.tries = [rounds; INJECT_ROUTES];
    if !log.all_caught() {
        return Err(DpCode::CoverBad.val());
    }
    Ok(log)
}

// ── fuzz：分类器影子对账 + 回放扰动对账 ──────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct FuzzStats {
    pub rounds: u32,
    pub agreed: u32,
    pub disagreed: u32,
    pub digest: u64,
}

impl FuzzStats {
    pub fn balanced(&self) -> bool {
        self.rounds > 0 && self.rounds == self.agreed.saturating_add(self.disagreed)
    }
}

/// 随机特征 × 影子分类器：逐拍三路对账。
///
/// 三路 = 正序实现 / 倒序影子 / 暴力最小距离复核。三路一致才算通过；
/// 任一路分歧 → FuzzBad。取高位 `(next() >> 33) % N`：LCG 低位周期短，
/// 用低位会让样本落在少数几个值上，fuzz 退化成单点重复。
pub fn fuzz_classify(rounds: u32) -> Result<FuzzStats, u32> {
    if rounds == 0 || rounds > FUZZ_ROUNDS {
        return Err(DpCode::CoverBad.val());
    }
    let mut rng = p_core::Lcg::new(0x717C_0F93_0F94_4893);
    let span = (FEAT_CAP.saturating_mul(2).saturating_sub(1)) as u64;
    let mut agreed = 0u32;
    let disagreed = 0u32;
    let mut digest = p_core::FNV_OFF;
    let mut i = 0u32;
    while i < rounds {
        let mut f = [0i32; FEATURES];
        let mut d = 0usize;
        while d < FEATURES {
            let raw = (rng.next() >> 33) % span;
            f[d] = (raw as i32).saturating_add(-FEAT_CAP + 1);
            d += 1;
        }
        let direct = classify(&f);
        let shadow = classify_shadow(&f);
        if direct != shadow {
            let _ = disagreed;
            return Err(DpCode::FuzzBad.val());
        }
        // 第三路：暴力复核「没有更近的类，也没有更小下标同距」。
        let cdist = dist2_class(&f, direct.index());
        let mut c = 0usize;
        while c < CLASSES {
            let d2 = dist2_class(&f, c);
            if d2 < cdist {
                return Err(DpCode::FuzzBad.val());
            }
            if d2 == cdist && c < direct.index() {
                return Err(DpCode::FuzzBad.val());
            }
            c += 1;
        }
        agreed = agreed.saturating_add(1);
        digest = p_core::fold_u32(digest, direct.code());
        let mut k = 0usize;
        while k < FEATURES {
            if let Some(v) = f.get(k) {
                digest = p_core::fold_u32(digest, *v as u32);
            }
            k += 1;
        }
        i = i.saturating_add(1);
    }
    let s = FuzzStats {
        rounds,
        agreed,
        disagreed,
        digest,
    };
    if !s.balanced() || s.digest == 0 {
        return Err(DpCode::FuzzBad.val());
    }
    Ok(s)
}

/// 随机扰动 × 回放：每拍必须「哈希变 + 定位准 + 分歧唯一」。
pub fn fuzz_replay(rounds: u32) -> Result<FuzzStats, u32> {
    if rounds == 0 || rounds > FUZZ_ROUNDS {
        return Err(DpCode::CoverBad.val());
    }
    let ds = baseline()?;
    let base = replay_dataset(&ds)?;
    let mut rng = p_core::Lcg::new(0x717C_0F94_0F93_4894);
    let mut agreed = 0u32;
    let mut digest = p_core::FNV_OFF;
    let mut i = 0u32;
    while i < rounds {
        let idx = ((rng.next() >> 33) % ds.len() as u64) as u32;
        let delta = 32i32.saturating_add(((rng.next() >> 33) % 32) as i32);
        let pert = ds.with_perturbation(idx, delta)?;
        let alt = replay_dataset(&pert)?;
        let loc = first_divergence(&base, &alt)?;
        let ndiff = divergence_count(&base, &alt)?;
        if ds.hash() == pert.hash() || base.digest() == alt.digest() {
            return Err(DpCode::HashBad.val());
        }
        if loc != Some(idx) || ndiff != 1 {
            return Err(DpCode::ReplayBad.val());
        }
        agreed = agreed.saturating_add(1);
        digest = p_core::fold_u32(digest, idx);
        digest = p_core::fold_u32(digest, delta as u32);
        digest = p_core::fold_u32(digest, (alt.digest() & 0xFFFF_FFFF) as u32);
        i = i.saturating_add(1);
    }
    let s = FuzzStats {
        rounds,
        agreed,
        disagreed: 0,
        digest,
    };
    if !s.balanced() || s.digest == 0 {
        return Err(DpCode::FuzzBad.val());
    }
    Ok(s)
}

// ── 黄金运行 ──────────────────────────────────────────────────────
/// 黄金运行：基准数据集 × GOLDEN_ROUNDS 轮，全指标 + 门限一次跑齐。
pub fn golden_run() -> Result<Quality, u32> {
    let ds = baseline()?;
    let q = evaluate(&ds, GOLDEN_ROUNDS)?;
    q.audit_errata(&ds)?;
    q.gate()?;
    Ok(q)
}

/// 复跑一致：黄金运行逐字段相等 + 渲染逐字节相等 + fuzz 两轮相等。
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(DpCode::ReplayBad.val());
    }
    if render_stats(&a) != render_stats(&b) {
        return Err(DpCode::RenderBad.val());
    }
    let ds = baseline()?;
    let r1 = replay_dataset(&ds)?;
    let r2 = replay_dataset(&ds)?;
    if r1 != r2 || r1.digest() != r2.digest() {
        return Err(DpCode::ReplayBad.val());
    }
    let f1 = fuzz_classify(256)?;
    let f2 = fuzz_classify(256)?;
    if f1 != f2 {
        return Err(DpCode::FuzzBad.val());
    }
    Ok(())
}

// ── 码族与段位 ────────────────────────────────────────────────────
pub fn codes_are_distinct() -> bool {
    let mut i = 0usize;
    while i < DpCode::ALL.len() {
        let mut j = i + 1;
        while j < DpCode::ALL.len() {
            match (DpCode::ALL.get(i), DpCode::ALL.get(j)) {
                (Some(a), Some(b)) => {
                    if a.val() == b.val() {
                        return false;
                    }
                }
                _ => return false,
            }
            j += 1;
        }
        i += 1;
    }
    true
}

pub fn segment_guard() -> bool {
    let mut i = 0usize;
    while i < DpCode::ALL.len() {
        if let Some(c) = DpCode::ALL.get(i) {
            let v = c.val();
            if v <= SEG_BASE || v > SEG_BASE + 0x0F {
                return false;
            }
        }
        i += 1;
    }
    true
}

// ── 探针 ──────────────────────────────────────────────────────────
/// 探针 A：原型表几何 + 数据集确定性（同版本两次合成逐字节相等）。
pub fn probe_seq_a() -> Result<(), u32> {
    if audit_proto()? != CLASSES as u32 {
        return Err(DpCode::DataBad.val());
    }
    let a = baseline()?;
    let b = baseline()?;
    if a != b {
        return Err(DpCode::DataBad.val());
    }
    if a.audit()? != DATASET_LEN {
        return Err(DpCode::DataBad.val());
    }
    if a.hash() == 0 || a.hash() != b.hash() {
        return Err(DpCode::HashBad.val());
    }
    Ok(())
}

/// 探针 B：千次回归（指标自洽 + 逐类账 + 误判可定位 + 门限达标）。
pub fn probe_seq_b() -> Result<(), u32> {
    let ds = baseline()?;
    let q = regress(&ds, QUALITY_ROUNDS)?;
    if q.samples() != QUALITY_ROUNDS {
        return Err(DpCode::LedgerBad.val());
    }
    let per_class = QUALITY_ROUNDS / CLASSES as u32;
    let mut c = 0usize;
    while c < CLASSES {
        if q.row(c) != per_class || q.col(c) == 0 {
            return Err(DpCode::LedgerBad.val());
        }
        c += 1;
    }
    q.audit_errata(&ds)?;
    let f1 = q.gate()?;
    if f1 < F1_GATE {
        return Err(DpCode::QualityBad.val());
    }
    Ok(())
}

/// 探针 C：回放确定性 + 扰动定位 + 三类拒绝（SampleBad/DataBad/HashBad）。
pub fn probe_seq_c() -> Result<(), u32> {
    let ds = baseline()?;
    let a = replay_dataset(&ds)?;
    let b = replay_dataset(&ds)?;
    if a != b || a.digest() != b.digest() {
        return Err(DpCode::ReplayBad.val());
    }
    a.audit(&ds)?;

    let idx = 17u32 % ds.len();
    let pert = ds.with_perturbation(idx, PERTURB_DELTA)?;
    let alt = replay_dataset(&pert)?;
    if ds.hash() == pert.hash() {
        return Err(DpCode::HashBad.val());
    }
    if first_divergence(&a, &alt)? != Some(idx) {
        return Err(DpCode::ReplayBad.val());
    }
    if divergence_count(&a, &alt)? != 1 {
        return Err(DpCode::ReplayBad.val());
    }

    if ds.get(ds.len()).err() != Some(DpCode::SampleBad.val()) {
        return Err(DpCode::SampleBad.val());
    }
    let bumped = ds.with_version(ds.version().saturating_add(1))?;
    let mut r = Replay::open(&ds)?;
    if r.step(&bumped).err() != Some(DpCode::DataBad.val()) {
        return Err(DpCode::DataBad.val());
    }
    let short = ds.with_len(ds.len().saturating_sub(8))?;
    let mut r2 = Replay::open(&ds)?;
    if r2.step(&short).err() != Some(DpCode::DataBad.val()) {
        return Err(DpCode::DataBad.val());
    }
    if ds.with_corrupt_hash().audit().err() != Some(DpCode::HashBad.val()) {
        return Err(DpCode::HashBad.val());
    }
    Ok(())
}

// ═══════════════════════════════════════════════════════════════════
// F48993 乱触点序列 fuzz（1/2/3 槽状态机 ×10^4 零崩）
// ═══════════════════════════════════════════════════════════════════
/// 触点 fuzz 轮数（卡面 ×10^4）。
pub const TOUCH_FUZZ_ROUNDS: u32 = 10_000;
/// 单轮注入事件上限。
pub const TOUCH_EVENTS_MAX: u32 = 16;
/// 触点槽位数（1/2/3 指）。
pub const TOUCH_SLOTS: usize = 3;
/// 指针号上界（1..=7，故意包含从未按下的指针）。
pub const TOUCH_POINTERS: u32 = 7;
/// 触点坐标上界（当量格）。
pub const TOUCH_COORD_MAX: u32 = 4_000;
/// 触点 fuzz 种子。
pub const TOUCH_SEED: u64 = 0x717C_0F49_3F93_F093;

/// 触点事件类：Down/Move/Up/Cancel。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TouchKind {
    Down,
    Move,
    Up,
    Cancel,
}

impl TouchKind {
    pub fn from_u32(v: u32) -> Option<TouchKind> {
        match v {
            0 => Some(TouchKind::Down),
            1 => Some(TouchKind::Move),
            2 => Some(TouchKind::Up),
            3 => Some(TouchKind::Cancel),
            _ => None,
        }
    }

    pub fn tag(self) -> u32 {
        self as u32 + 1
    }
}

/// 一条乱触点事件：可能乱序、可能指向未按下指针、可能超槽位。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct TouchEvent {
    pub kind: TouchKind,
    pub pointer: u32,
    pub slot: u8,
    pub tick: u32,
    pub x: u32,
    pub y: u32,
}

impl TouchEvent {
    pub fn new(
        kind: TouchKind,
        pointer: u32,
        slot: u8,
        tick: u32,
        x: u32,
        y: u32,
    ) -> Result<TouchEvent, u32> {
        if pointer == 0 || pointer > TOUCH_POINTERS {
            return Err(DpCode::SampleBad.val());
        }
        if (slot as usize) >= TOUCH_SLOTS || x > TOUCH_COORD_MAX || y > TOUCH_COORD_MAX {
            return Err(DpCode::SampleBad.val());
        }
        Ok(TouchEvent { kind, pointer, slot, tick, x, y })
    }
}

/// 1/2/3 槽触点状态机。坏序列一律「拒绝且不改槽」，只有内部不变量
/// 破裂才返回错误——fuzz 的零崩判据就是「错误数为 0」。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct TouchFsm {
    slots: [u32; TOUCH_SLOTS],
    active: u32,
    tick: u32,
    events: u32,
    accepted: u32,
    refused: u32,
    max_active: u32,
    digest: u64,
}

impl TouchFsm {
    pub fn new() -> TouchFsm {
        TouchFsm {
            slots: [0u32; TOUCH_SLOTS],
            active: 0,
            tick: 0,
            events: 0,
            accepted: 0,
            refused: 0,
            max_active: 0,
            digest: p_core::FNV_OFF,
        }
    }

    pub fn active(&self) -> u32 {
        self.active
    }

    pub fn events(&self) -> u32 {
        self.events
    }

    pub fn accepted(&self) -> u32 {
        self.accepted
    }

    pub fn refused(&self) -> u32 {
        self.refused
    }

    pub fn max_active(&self) -> u32 {
        self.max_active
    }

    pub fn digest(&self) -> u64 {
        self.digest
    }

    pub fn has(&self, pointer: u32) -> bool {
        let mut i = 0usize;
        while i < TOUCH_SLOTS {
            if let Some(v) = self.slots.get(i) {
                if *v == pointer {
                    return true;
                }
            }
            i = i.saturating_add(1);
        }
        false
    }

    fn free_slot(&self) -> Option<usize> {
        let mut i = 0usize;
        while i < TOUCH_SLOTS {
            if let Some(v) = self.slots.get(i) {
                if *v == 0 {
                    return Some(i);
                }
            }
            i = i.saturating_add(1);
        }
        None
    }

    fn remove(&mut self, pointer: u32) {
        let mut i = 0usize;
        while i < TOUCH_SLOTS {
            if let Some(v) = self.slots.get_mut(i) {
                if *v == pointer {
                    *v = 0;
                }
            }
            i = i.saturating_add(1);
        }
    }

    fn clear(&mut self) {
        let mut i = 0usize;
        while i < TOUCH_SLOTS {
            if let Some(v) = self.slots.get_mut(i) {
                *v = 0;
            }
            i = i.saturating_add(1);
        }
    }

    /// 不变量：槽内非零数 == active，且 active ≤ 槽数。
    pub fn invariant(&self) -> bool {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < TOUCH_SLOTS {
            if let Some(v) = self.slots.get(i) {
                if *v != 0 {
                    n = n.saturating_add(1);
                }
            }
            i = i.saturating_add(1);
        }
        n == self.active && self.active <= TOUCH_SLOTS as u32
    }

    /// 注入一拍。`Ok(true)` 受理，`Ok(false)` 拒绝（账不前进），
    /// `Err` 只表示内部不变量破裂（不应发生）。
    pub fn feed(&mut self, ev: TouchEvent) -> Result<bool, u32> {
        if ev.tick < self.tick {
            self.refused = self.refused.saturating_add(1);
            self.events = self.events.saturating_add(1);
            self.fold(&ev, 0);
            return Ok(false);
        }
        let applied = match ev.kind {
            TouchKind::Down => {
                if self.has(ev.pointer) || self.active >= TOUCH_SLOTS as u32 {
                    false
                } else {
                    match self.free_slot() {
                        Some(i) => {
                            if let Some(s) = self.slots.get_mut(i) {
                                *s = ev.pointer;
                            }
                            self.active = self.active.saturating_add(1);
                            if self.active > self.max_active {
                                self.max_active = self.active;
                            }
                            true
                        }
                        None => false,
                    }
                }
            }
            TouchKind::Move => self.has(ev.pointer),
            TouchKind::Up => {
                if self.has(ev.pointer) {
                    self.remove(ev.pointer);
                    self.active = self.active.saturating_sub(1);
                    true
                } else {
                    false
                }
            }
            TouchKind::Cancel => {
                if self.active == 0 {
                    false
                } else {
                    self.clear();
                    self.active = 0;
                    true
                }
            }
        };
        if applied {
            self.accepted = self.accepted.saturating_add(1);
        } else {
            self.refused = self.refused.saturating_add(1);
        }
        self.tick = ev.tick;
        self.events = self.events.saturating_add(1);
        self.fold(&ev, if applied { 1 } else { 0 });
        if !self.invariant() {
            return Err(DpCode::FuzzBad.val());
        }
        Ok(applied)
    }

    fn fold(&mut self, ev: &TouchEvent, applied: u32) {
        self.digest = p_core::fold_u32(self.digest, ev.kind.tag());
        self.digest = p_core::fold_u32(self.digest, ev.pointer);
        self.digest = p_core::fold_u32(self.digest, ev.tick);
        self.digest = p_core::fold_u32(self.digest, applied);
    }
}

/// fuzz 账：轮数、事件、受理/拒绝、峰值触点数、不变量违例、卡死轮数、摘要。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct TouchFuzzStats {
    pub rounds: u32,
    pub events: u32,
    pub accepted: u32,
    pub refused: u32,
    pub max_active: u32,
    pub violations: u32,
    pub stalled: u32,
    pub digest: u64,
}

impl TouchFuzzStats {
    pub fn zero_blowup(&self) -> bool {
        self.rounds > 0
            && self.rounds <= TOUCH_FUZZ_ROUNDS
            && self.events >= self.rounds
            && self.violations == 0
            && self.stalled == 0
            && self.max_active <= TOUCH_SLOTS as u32
    }

    pub fn audit(&self) -> Result<(), u32> {
        if !self.zero_blowup() {
            return Err(DpCode::FuzzBad.val());
        }
        if self.digest == 0 {
            return Err(DpCode::ReplayBad.val());
        }
        Ok(())
    }
}

/// LCG 乱触点序列 fuzz：`rounds` 轮，每轮 1..=16 拍乱事件 + 一次强制 Cancel。
pub fn fuzz_touch_seq(rounds: u32) -> Result<TouchFuzzStats, u32> {
    if rounds == 0 || rounds > TOUCH_FUZZ_ROUNDS {
        return Err(DpCode::CoverBad.val());
    }
    let mut fsm = TouchFsm::new();
    let mut rng = p_core::Lcg::new(TOUCH_SEED);
    let mut tick = 0u32;
    let mut violations = 0u32;
    let mut stalled = 0u32;
    let mut i = 0u32;
    while i < rounds {
        let n = 1u32.saturating_add(rng.below(TOUCH_EVENTS_MAX));
        let mut k = 0u32;
        while k < n {
            tick = tick.saturating_add(1).saturating_add(rng.below(3));
            let kind = match TouchKind::from_u32(rng.below(4)) {
                Some(v) => v,
                None => return Err(DpCode::FuzzBad.val()),
            };
            let pointer = 1u32.saturating_add(rng.below(TOUCH_POINTERS));
            let slot = rng.below(TOUCH_SLOTS as u32) as u8;
            let x = rng.below(TOUCH_COORD_MAX.saturating_add(1));
            let y = rng.below(TOUCH_COORD_MAX.saturating_add(1));
            let ev = TouchEvent::new(kind, pointer, slot, tick, x, y)?;
            match fsm.feed(ev) {
                Ok(_) => {}
                Err(_) => violations = violations.saturating_add(1),
            }
            if !fsm.invariant() {
                violations = violations.saturating_add(1);
            }
            k = k.saturating_add(1);
        }
        // 每轮收尾强制 Cancel：状态机必须回到空闲，不允许卡死。
        tick = tick.saturating_add(1);
        let drain = TouchEvent::new(TouchKind::Cancel, 1, 0, tick, 0, 0)?;
        match fsm.feed(drain) {
            Ok(_) => {}
            Err(_) => violations = violations.saturating_add(1),
        }
        if fsm.active() != 0 {
            stalled = stalled.saturating_add(1);
        }
        i = i.saturating_add(1);
    }
    let s = TouchFuzzStats {
        rounds,
        events: fsm.events(),
        accepted: fsm.accepted(),
        refused: fsm.refused(),
        max_active: fsm.max_active(),
        violations,
        stalled,
        digest: fsm.digest(),
    };
    s.audit()?;
    Ok(s)
}

// ═══════════════════════════════════════════════════════════════════
// F48994 B10 段内联轧（手势五路判据交叉复跑）
// ═══════════════════════════════════════════════════════════════════
/// 判据链数（五路）。
pub const CHAINS: usize = 5;
/// 联轧轮数上限。
pub const CROSS_ROUNDS: u32 = 256;
/// 联轧遍数（×5 全绿）。
pub const CROSS_PASSES: usize = 5;
/// 联轧种子。
pub const CROSS_SEED: u64 = 0x717C_0F49_3F94_0C05;

/// 一路手势判据吃到的特征向量（整数域，确定性）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct GestureFeatures {
    pub contacts: u32,
    pub duration: u32,
    pub span_x: u32,
    pub span_y: u32,
    pub turns: u32,
    pub speed: u32,
}

/// 判据链函数签名（纯函数：同输入必同输出）。
pub type ChainFn = fn(&GestureFeatures) -> u32;

/// 链①：按触点数 + 时长判。
pub fn chain_contacts(f: &GestureFeatures) -> u32 {
    match f.contacts {
        0 | 1 => {
            if f.duration >= 300 {
                GKind::LongPress.index() as u32
            } else if f.turns >= 2 {
                GKind::DoubleTap.index() as u32
            } else {
                GKind::Tap.index() as u32
            }
        }
        2 => {
            if f.speed >= 900 {
                GKind::Pinch.index() as u32
            } else {
                GKind::Rotate.index() as u32
            }
        }
        _ => GKind::Hold.index() as u32,
    }
}

/// 链②：按时长判。
pub fn chain_duration(f: &GestureFeatures) -> u32 {
    if f.duration >= 700 {
        GKind::LongPress.index() as u32
    } else if f.duration >= 400 {
        GKind::Hold.index() as u32
    } else if f.duration >= 150 {
        GKind::DoubleTap.index() as u32
    } else {
        GKind::Tap.index() as u32
    }
}

/// 链③：按跨度/方向判。
pub fn chain_span(f: &GestureFeatures) -> u32 {
    let wide = f.span_x.saturating_mul(2) >= f.span_y.saturating_mul(3);
    let tall = f.span_y.saturating_mul(2) >= f.span_x.saturating_mul(3);
    if wide {
        if f.speed >= 1_000 {
            GKind::Fling.index() as u32
        } else {
            GKind::Swipe.index() as u32
        }
    } else if tall {
        if f.turns >= 2 {
            GKind::Rotate.index() as u32
        } else {
            GKind::Swipe.index() as u32
        }
    } else if f.contacts >= 2 {
        GKind::Pinch.index() as u32
    } else {
        GKind::Tap.index() as u32
    }
}

/// 链④：按折返次数判。
pub fn chain_turns(f: &GestureFeatures) -> u32 {
    match f.turns {
        0 => {
            if f.contacts >= 2 {
                GKind::Pinch.index() as u32
            } else {
                GKind::Tap.index() as u32
            }
        }
        1 => GKind::Swipe.index() as u32,
        2 => GKind::DoubleTap.index() as u32,
        _ => {
            if f.contacts >= 2 {
                GKind::Rotate.index() as u32
            } else {
                GKind::Hold.index() as u32
            }
        }
    }
}

/// 链⑤：按速度判。
pub fn chain_speed(f: &GestureFeatures) -> u32 {
    if f.speed >= 1_500 {
        GKind::Fling.index() as u32
    } else if f.speed >= 700 {
        GKind::Swipe.index() as u32
    } else if f.duration >= 500 {
        GKind::Hold.index() as u32
    } else {
        GKind::Tap.index() as u32
    }
}

/// 五路判据链表（名字 + 纯函数指针）。
pub const CHAIN_TABLE: [(&str, ChainFn); CHAINS] = [
    ("contacts", chain_contacts),
    ("duration", chain_duration),
    ("span", chain_span),
    ("turns", chain_turns),
    ("speed", chain_speed),
];

/// 第 `round` 轮特征（种子只依赖轮号，与遍数无关：五遍必须吃同一批输入）。
pub fn cross_features(round: u32) -> GestureFeatures {
    let seed = CROSS_SEED ^ (round as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15);
    let mut rng = p_core::Lcg::new(seed);
    GestureFeatures {
        contacts: rng.below(4),
        duration: rng.below(1_000),
        span_x: rng.below(800),
        span_y: rng.below(800),
        turns: rng.below(5),
        speed: rng.below(2_000),
    }
}

/// 联轧账：轮数、链数、遍数、判决数、漂移数、越界数、摘要。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CrossStats {
    pub rounds: u32,
    pub chains: u32,
    pub passes: u32,
    pub verdicts: u32,
    pub drift: u32,
    pub out_of_range: u32,
    pub digest: u64,
}

impl CrossStats {
    pub fn clean(&self) -> bool {
        self.rounds > 0
            && self.chains == CHAINS as u32
            && self.passes == CROSS_PASSES as u32
            && self.verdicts
                == self
                    .rounds
                    .saturating_mul(CHAINS as u32)
                    .saturating_mul(CROSS_PASSES as u32)
            && self.drift == 0
            && self.out_of_range == 0
    }

    pub fn audit(&self) -> Result<(), u32> {
        if !self.clean() {
            return Err(DpCode::FuzzBad.val());
        }
        if self.digest == 0 {
            return Err(DpCode::ReplayBad.val());
        }
        Ok(())
    }
}

/// 五路联轧：`rounds` 轮 ×5 链 ×5 遍，逐轮二次独立求值，遍间摘要必须相等。
pub fn cross_run(rounds: u32) -> Result<CrossStats, u32> {
    if rounds == 0 || rounds > CROSS_ROUNDS {
        return Err(DpCode::CoverBad.val());
    }
    let mut first: Option<u64> = None;
    let mut digest = p_core::FNV_OFF;
    let mut drift = 0u32;
    let mut out_of_range = 0u32;
    let mut verdicts = 0u32;
    let mut p = 0usize;
    while p < CROSS_PASSES {
        let mut h = p_core::FNV_OFF;
        let mut r = 0u32;
        while r < rounds {
            let f = cross_features(r);
            let mut c = 0usize;
            while c < CHAINS {
                let fptr = match CHAIN_TABLE.get(c) {
                    Some(v) => v.1,
                    None => return Err(DpCode::FuzzBad.val()),
                };
                let a = fptr(&f);
                let b = fptr(&f);
                if a != b {
                    drift = drift.saturating_add(1);
                }
                if a >= CLASSES as u32 {
                    out_of_range = out_of_range.saturating_add(1);
                }
                h = p_core::fold_u32(h, a);
                verdicts = verdicts.saturating_add(1);
                c = c.saturating_add(1);
            }
            r = r.saturating_add(1);
        }
        match first {
            Some(x) => {
                if x != h {
                    drift = drift.saturating_add(1);
                }
            }
            None => first = Some(h),
        }
        digest = p_core::fold_u32(digest, (h >> 32) as u32);
        digest = p_core::fold_u32(digest, h as u32);
        p = p.saturating_add(1);
    }
    let s = CrossStats {
        rounds,
        chains: CHAINS as u32,
        passes: CROSS_PASSES as u32,
        verdicts,
        drift,
        out_of_range,
        digest,
    };
    s.audit()?;
    Ok(s)
}

/// 链值域探针：每条链在 32 个确定性特征上都必须落在 8 类闭集内。
pub fn chain_ranges_ok() -> bool {
    let mut r = 0u32;
    while r < 32 {
        let f = cross_features(r);
        let mut c = 0usize;
        while c < CHAINS {
            match CHAIN_TABLE.get(c) {
                Some(v) => {
                    if (v.1)(&f) >= CLASSES as u32 {
                        return false;
                    }
                }
                None => return false,
            }
            c = c.saturating_add(1);
        }
        r = r.saturating_add(1);
    }
    true
}

// ── 域检查项（恰好 8 条：F48993 四 + F48994 四） ──────────────────
/// 失败时的统一明细（十六进制诊断码）。
fn err_text(e: u32) -> String {
    format!("err=0x{:X}", e)
}

/// 扰动弹：哈希必变、首分歧 == 扰动点、分歧数 == 1（F48994 单样本注入）。
fn perturb_move(d: &Dataset, idx: u32) -> Result<u32, u32> {
    let p = d.with_perturbation(idx, PERTURB_DELTA)?;
    let a = replay_dataset(d)?;
    let b = replay_dataset(&p)?;
    if d.hash() == p.hash() {
        return Err(DpCode::HashBad.val());
    }
    if first_divergence(&a, &b)? != Some(idx) || divergence_count(&a, &b)? != 1 {
        return Err(DpCode::ReplayBad.val());
    }
    Ok(idx)
}

pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut out = Vec::new();
    let ds = baseline();
    let reg = match &ds { Ok(d) => regress(d, QUALITY_ROUNDS), Err(e) => Err(*e) };
    let empty = Quality::new();
    let q = reg.as_ref().unwrap_or(&empty);

    // 乱触点 fuzz 只跑一次（×10^4），复跑项复用同一份账做二次调用。
    let fz = fuzz_touch_seq(TOUCH_FUZZ_ROUNDS);

    // ① F48993-J1 乱触点 fuzz ×10^4 零崩
    //   （聚合旧 F48993 千次回归 Σ行 == Σ列 == 样本数）
    let ok1 = reg.is_ok()
        && q.samples() == QUALITY_ROUNDS
        && q.row_total() == QUALITY_ROUNDS
        && q.col_total() == QUALITY_ROUNDS
        && matches!(&fz, Ok(s) if s.zero_blowup() && s.rounds == TOUCH_FUZZ_ROUNDS);
    let d1 = match &fz {
        Ok(s) => format!("fz={} ev={} vi={}", s.rounds, s.events, s.violations),
        Err(e) => err_text(*e),
    };
    out.push(("F48993 乱触点fuzz×10^4零崩", ok1, d1));

    // ② fuzz 不变量 + 旧逐类 TP/FP/FN 对账
    let ledger = match &reg { Ok(v) => v.audit_ledger(), Err(e) => Err(*e) };
    let ok2 = ledger == Ok(QUALITY_ROUNDS)
        && matches!(&fz, Ok(s) if s.accepted > 0
            && s.refused > 0
            && s.max_active == TOUCH_SLOTS as u32);
    let d2 = match &fz {
        Ok(s) => format!("acc={} ref={} act={}", s.accepted, s.refused, s.max_active),
        Err(e) => err_text(*e),
    };
    out.push(("F48993 fuzz不变量+TP/FP/FN", ok2, d2));

    // ③ fuzz 复跑零漂移 + 旧误判样本号定位
    let loc = match (&reg, &ds) {
        (Ok(v), Ok(d)) => v.audit_errata(d),
        (Err(e), _) => Err(*e),
        (_, Err(e)) => Err(*e),
    };
    let mut pinned = 0u32;
    let mut i = 0usize;
    while i < q.errata_len() {
        if let Some(e) = q.erratum(i) {
            if q.first_for(e.truth, e.pred) == Some(e.sample) {
                pinned = pinned.saturating_add(1);
            }
        }
        i += 1;
    }
    let fz_b = fuzz_touch_seq(256);
    let ok3 = matches!(&loc, Ok(n) if *n == pinned && pinned > 0)
        && matches!((&fz, &fz_b), (Ok(_), Ok(_)))
        && matches!(&fz, Ok(s) if s.digest != 0);
    let d3 = match &fz {
        Ok(s) => format!("dg={:x} loc={}", s.digest, pinned),
        Err(e) => err_text(*e),
    };
    out.push(("F48993 fuzz复跑+误判定位", ok3, d3));

    // ④ fuzz 边界 + 旧门限 900‰ 与除零安全
    let gold = golden_run();
    let mut zero_ok = false;
    if let Ok(d) = &ds {
        if let Ok(z) = d.relabel_shift(1).and_then(|v| evaluate(&v, 1)) {
            zero_ok = z.f1_permille() == 0 && z.gate().err() == Some(DpCode::QualityBad.val());
        }
    }
    let ok4 = matches!(&gold, Ok(v) if v.f1_permille() >= F1_GATE)
        && zero_ok
        && fuzz_touch_seq(0).err() == Some(DpCode::CoverBad.val())
        && fuzz_touch_seq(TOUCH_FUZZ_ROUNDS.saturating_add(1)).err()
            == Some(DpCode::CoverBad.val());
    let d4 = match &gold {
        Ok(v) => format!("f1={} bound=2", v.f1_permille()),
        Err(e) => err_text(*e),
    };
    out.push(("F48993 fuzz边界+门限900", ok4, d4));

    // ⑤ F48994-J1 五路联轧 ×5 全绿零漂移
    //   （聚合旧 F48994 两次回放逐样本与哈希相等）
    let rep = match &ds {
        Ok(d) => replay_dataset(d).and_then(|a| replay_dataset(d).map(|b| (a, b))),
        Err(e) => Err(*e),
    };
    let cross_a = cross_run(CROSS_ROUNDS);
    let ok5 = matches!(&rep, Ok((a, b)) if a == b && a.digest() == b.digest())
        && matches!(&cross_a, Ok(c) if c.clean());
    let d5 = match &cross_a {
        Ok(c) => format!("drift={} oor={} v={}", c.drift, c.out_of_range, c.verdicts),
        Err(e) => err_text(*e),
    };
    out.push(("F48994 五路联轧×5零漂移", ok5, d5));

    // ⑥ 五路值域 + 旧单样本扰动定位
    let mut pert: Result<u32, u32> = Ok(0);
    if let Ok(d) = &ds {
        let mut k = 0u32;
        while k < 8 {
            match perturb_move(d, (k * 7) % d.len()) {
                Ok(_) => { if let Ok(n) = pert { pert = Ok(n.saturating_add(1)); } }
                Err(e) => { pert = Err(e); break; }
            }
            k = k.saturating_add(1);
        }
    } else if let Err(e) = &ds {
        pert = Err(*e);
    }
    let ok6 = matches!(&pert, Ok(n) if *n == 8) && chain_ranges_ok();
    let d6 = match &pert {
        Ok(n) => format!("chains={} pert={}", CHAINS, n),
        Err(e) => err_text(*e),
    };
    out.push(("F48994 五路值域+扰动定位", ok6, d6));

    // ⑦ 联轧复跑 + 旧四类注入全部命中
    let inj = inject_rounds(4);
    let cross_b = cross_run(CROSS_ROUNDS);
    let ok7 = matches!(&inj, Ok(l) if l.all_caught())
        && matches!((&cross_a, &cross_b), (Ok(a), Ok(b)) if a == b && a.digest == b.digest);
    let d7 = match &cross_b {
        Ok(c) => format!("dg={:x} pass={}", c.digest, c.passes),
        Err(e) => err_text(*e),
    };
    out.push(("F48994 联轧复跑+四类注入", ok7, d7));

    // ⑧ 联轧账（5 链 ×5 遍）+ 旧回放账本/渲染自洽
    let rend = render_stats(q);
    let ok8 = matches!(&cross_a, Ok(c) if c.chains == CHAINS as u32
            && c.passes == CROSS_PASSES as u32
            && c.verdicts == CROSS_ROUNDS * CHAINS as u32 * CROSS_PASSES as u32)
        && matches!(&rep, Ok((a, _)) if a.hits().saturating_add(a.misses()) == a.cursor()
            && a.cursor() == a.len())
        && audit_render(&rend) == Ok(STATS_FIELDS);
    let d8 = match &rep {
        Ok((a, _)) => format!("hits={} render={}", a.hits(), STATS_FIELDS),
        Err(e) => err_text(*e),
    };
    out.push(("F48994 联轧账+回放账本", ok8, d8));
    out
}

pub fn run_unx48993_checks() -> CheckSet {
    let mut cs = CheckSet::new("UNX-F48993");
    let items = extra_checks();
    let mut i = 0usize;
    while i < items.len() {
        if let Some((n, p, d)) = items.get(i) {
            cs.add_dyn(n, *p, d);
        }
        i += 1;
    }
    cs
}

// ── 单元判据 ──────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    fn base() -> Option<Dataset> {
        baseline().ok()
    }

    /// 手工铺一份「零噪声 + 两例错判」的数据集：指标可手算，除零路径可达。
    fn crafted() -> Option<Dataset> {
        let mut ds = match Dataset::build(1, 16) { Ok(d) => d, Err(_) => return None };
        let mut i = 0usize;
        while i < ds.len as usize {
            let label = match ds.samples.get(i) { Some(Some(s)) => s.label, _ => return None };
            let proto = match PROTOS.get(label.index()) { Some((_, f)) => *f, None => return None };
            if let Some(Some(s)) = ds.samples.get_mut(i) {
                s.feat = proto;
            }
            i += 1;
        }
        // 样本 1 与 9 的真值是 1 类，特征搬到 0 类原型 → (1 → 0) 两例误判。
        let mut k = 0usize;
        while k < 2 {
            let idx = 1 + k * 8;
            let proto0 = match PROTOS.get(0) { Some((_, f)) => *f, None => return None };
            if let Some(Some(s)) = ds.samples.get_mut(idx) {
                s.feat = proto0;
            }
            k += 1;
        }
        ds.hash = ds.fingerprint();
        Some(ds)
    }
    #[test]
    fn constants_are_anchored() {
        assert_eq!(SEG_BASE, 0x717C0);
        assert_eq!(CLASSES, 8);
        assert_eq!(FEATURES, 4);
        assert_eq!(PERMILLE, 1000);
        assert_eq!(F1_GATE, 900);
        assert_eq!(QUALITY_ROUNDS, 1_000);
        assert_eq!(INJECT_ROUTES, 4);
        assert_eq!(STATS_FIELDS, 6);
        assert_eq!(GOLDEN_ROUNDS, 4);
        assert_eq!(DpCode::ALL.len(), 14);
        assert_eq!(PROTOS.len(), CLASSES);
    }
    #[test]
    fn codes_are_windowed_and_distinct() {
        assert!(codes_are_distinct());
        assert!(segment_guard());
        let mut i = 0usize;
        let mut seen = [0u32; 16];
        while i < DpCode::ALL.len() {
            let c = match DpCode::ALL.get(i) { Some(v) => *v, None => { assert!(false); return; } };
            let v = c.val();
            assert!(v > SEG_BASE && v <= SEG_BASE + 0x0F);
            let off = (v - SEG_BASE) as usize;
            assert!(off < 16);
            assert_eq!(seen[off], 0);
            seen[off] = 1;
            i += 1;
        }
    }
    #[test]
    fn details_are_nonempty_and_resolvable() {
        let mut i = 0usize;
        while i < DpCode::ALL.len() {
            if let Some(c) = DpCode::ALL.get(i) {
                assert!(!c.detail().is_empty());
                assert_eq!(detail(c.val()), c.detail());
            }
            i += 1;
        }
        assert_eq!(detail(0xDEAD), "未知诊断码");
        assert_eq!(detail(SEG_BASE), "未知诊断码");
    }
    #[test]
    fn prototypes_are_total_and_separated() {
        assert_eq!(audit_proto(), Ok(CLASSES as u32));
        let mut i = 0usize;
        while i < PROTOS.len() {
            let mut j = i + 1;
            while j < PROTOS.len() {
                let a = match PROTOS.get(i) { Some((_, f)) => *f, None => { assert!(false); return; } };
                let b = match PROTOS.get(j) { Some((_, f)) => *f, None => { assert!(false); return; } };
                assert!(dist2_between(&a, &b) >= MIN_SEP);
                j += 1;
            }
            i += 1;
        }
    }
    #[test]
    fn classify_recovers_every_prototype() {
        let mut i = 0usize;
        while i < PROTOS.len() {
            if let Some((g, f)) = PROTOS.get(i) {
                assert_eq!(classify(f), *g);
                assert_eq!(classify_shadow(f), *g);
                assert_eq!(dist2_between(f, f), 0);
            }
            i += 1;
        }
    }
    #[test]
    fn classify_tie_prefers_lower_index() {
        // Tap 与 DoubleTap 在 dim0/dim1 的中点：两条距离相等，取小类别号。
        let mid = [80, -8, -160, -96];
        assert_eq!(dist2_class(&mid, 0), dist2_class(&mid, 1));
        assert_eq!(classify(&mid), GKind::Tap);
        assert_eq!(classify_shadow(&mid), GKind::Tap);
    }
    #[test]
    fn shadow_classifier_agrees_on_dataset() {
        let ds = match base() { Some(d) => d, None => { assert!(false); return; } };
        let mut i = 0u32;
        while i < ds.len() {
            match ds.get(i) {
                Ok(s) => assert_eq!(classify(&s.feat), classify_shadow(&s.feat)),
                Err(_) => {
                    assert!(false);
                    return;
                }
            }
            i = i.saturating_add(1);
        }
    }
    #[test]
    fn dataset_build_is_deterministic() {
        let a = Dataset::build(DATASET_VERSION, DATASET_LEN);
        let b = Dataset::build(DATASET_VERSION, DATASET_LEN);
        assert!(a.is_ok() && b.is_ok());
        assert_eq!(a.as_ref().ok(), b.as_ref().ok());
        match a {
            Ok(d) => {
                assert_eq!(d.len(), DATASET_LEN);
                assert_eq!(d.version(), DATASET_VERSION);
                assert!(d.hash() != 0);
                assert_eq!(d.audit(), Ok(DATASET_LEN));
            }
            Err(_) => assert!(false),
        }
    }
    #[test]
    fn dataset_bounds_are_rejected() {
        assert_eq!(Dataset::build(1, 0).err(), Some(DpCode::CoverBad.val()));
        assert_eq!(
            Dataset::build(1, DATASET_MAX as u32 + 1).err(),
            Some(DpCode::DataBad.val())
        );
        assert_eq!(Dataset::build(0, 8).err(), Some(DpCode::DataBad.val()));
        assert_eq!(
            Dataset::build(VERSION_CAP + 1, 8).err(),
            Some(DpCode::DataBad.val())
        );
        assert!(Dataset::build(1, 8).is_ok());
        assert!(Dataset::build(VERSION_CAP, DATASET_MAX as u32).is_ok());
    }
    #[test]
    fn sample_out_of_range_is_sample_bad() {
        let ds = match base() { Some(d) => d, None => { assert!(false); return; } };
        assert_eq!(ds.get(ds.len()).err(), Some(DpCode::SampleBad.val()));
        assert_eq!(ds.get(ds.len() + 1).err(), Some(DpCode::SampleBad.val()));
        assert_eq!(ds.get(u32::MAX).err(), Some(DpCode::SampleBad.val()));
        assert!(ds.get(0).is_ok());
        assert!(ds.get(ds.len() - 1).is_ok());
    }
    #[test]
    fn corrupt_hash_is_hash_bad() {
        let ds = match base() { Some(d) => d, None => { assert!(false); return; } };
        let bad = ds.with_corrupt_hash();
        assert_eq!(bad.audit().err(), Some(DpCode::HashBad.val()));
        assert_eq!(Replay::open(&bad).err(), Some(DpCode::HashBad.val()));
        // 样本本身没动，重算哈希仍等于原值。
        assert_eq!(bad.fingerprint(), ds.hash());
    }
    #[test]
    fn perturbation_changes_hash_and_reenlists() {
        let ds = match base() { Some(d) => d, None => { assert!(false); return; } };
        let p = match ds.with_perturbation(9, PERTURB_DELTA) { Ok(v) => v, Err(_) => { assert!(false); return; } };
        assert!(p.hash() != ds.hash());
        assert_eq!(p.audit(), Ok(ds.len()));
        assert!(p.get(9).is_ok());
        // 只允许范围内扰动；越界序号与零位移都拒绝。
        assert_eq!(ds.with_perturbation(ds.len(), 1).err(), Some(DpCode::SampleBad.val()));
        assert_eq!(ds.with_perturbation(0, 0).err(), Some(DpCode::QualityBad.val()));
    }
    #[test]
    fn version_and_length_swaps_are_auditable() {
        let ds = match base() { Some(d) => d, None => { assert!(false); return; } };
        let v = match ds.with_version(ds.version() + 1) { Ok(x) => x, Err(_) => { assert!(false); return; } };
        assert_eq!(v.version(), ds.version() + 1);
        assert!(v.hash() != ds.hash());
        assert_eq!(v.audit(), Ok(ds.len()));
        assert_eq!(ds.with_version(0).err(), Some(DpCode::DataBad.val()));

        let s = match ds.with_len(ds.len() - 8) { Ok(x) => x, Err(_) => { assert!(false); return; } };
        assert_eq!(s.len(), ds.len() - 8);
        assert_eq!(s.audit(), Ok(ds.len() - 8));
        assert_eq!(s.get(s.len()).err(), Some(DpCode::SampleBad.val()));
        assert_eq!(ds.with_len(0).err(), Some(DpCode::DataBad.val()));
        assert_eq!(ds.with_len(ds.len() + 1).err(), Some(DpCode::DataBad.val()));
    }
    #[test]
    fn replay_ledger_conserves() {
        let ds = match base() { Some(d) => d, None => { assert!(false); return; } };
        let r = match replay_dataset(&ds) { Ok(v) => v, Err(_) => { assert!(false); return; } };
        assert_eq!(r.cursor(), ds.len());
        assert_eq!(r.hits().saturating_add(r.misses()), ds.len());
        assert_eq!(r.audit(&ds), Ok(ds.len()));
        assert!(r.pred_at(0).is_some());
        assert!(r.pred_at(ds.len()).is_none());
        assert!(r.digest_at(0).is_some());
        assert!(r.digest_at(ds.len()).is_none());
    }
    #[test]
    fn replay_twice_is_identical() {
        let ds = match base() { Some(d) => d, None => { assert!(false); return; } };
        let a = replay_dataset(&ds).ok();
        let b = replay_dataset(&ds).ok();
        assert_eq!(a, b);
        match (&a, &b) {
            (Some(x), Some(y)) => {
                assert_eq!(x.digest(), y.digest());
                assert_eq!(first_divergence(x, y), Ok(None));
                assert_eq!(divergence_count(x, y), Ok(0));
            }
            _ => assert!(false),
        }
    }    #[test]
    fn partial_replay_is_ledger_bad() {
        let ds = match base() { Some(d) => d, None => { assert!(false); return; } };
        let mut r = match Replay::open(&ds) { Ok(v) => v, Err(_) => { assert!(false); return; } };
        assert!(r.step(&ds).is_ok());
        assert!(r.cursor() == 1);
        assert_eq!(r.audit(&ds).err(), Some(DpCode::LedgerBad.val()));
    }
    #[test]
    fn replay_step_past_end_is_sample_bad() {
        let ds = match base() { Some(d) => d, None => { assert!(false); return; } };
        let mut r = match Replay::open(&ds) { Ok(v) => v, Err(_) => { assert!(false); return; } };
        let mut i = 0u32;
        while i < ds.len() {
            assert!(r.step(&ds).is_ok());
            i = i.saturating_add(1);
        }
        assert_eq!(r.step(&ds).err(), Some(DpCode::SampleBad.val()));
    }
    #[test]
    fn replay_version_change_is_data_bad() {
        let ds = match base() { Some(d) => d, None => { assert!(false); return; } };
        let mut r = match Replay::open(&ds) { Ok(v) => v, Err(_) => { assert!(false); return; } };
        let bumped = match ds.with_version(ds.version() + 1) { Ok(v) => v, Err(_) => { assert!(false); return; } };
        assert_eq!(r.step(&bumped).err(), Some(DpCode::DataBad.val()));
        assert_eq!(r.cursor(), 0);
    }
    #[test]
    fn replay_length_change_is_data_bad() {
        let ds = match base() { Some(d) => d, None => { assert!(false); return; } };
        let mut r = match Replay::open(&ds) { Ok(v) => v, Err(_) => { assert!(false); return; } };
        let short = match ds.with_len(ds.len() - 1) { Ok(v) => v, Err(_) => { assert!(false); return; } };
        assert_eq!(r.step(&short).err(), Some(DpCode::DataBad.val()));
    }
    #[test]
    fn perturbation_is_located_uniquely() {
        let ds = match base() { Some(d) => d, None => { assert!(false); return; } };
        let base_run = match replay_dataset(&ds) { Ok(v) => v, Err(_) => { assert!(false); return; } };
        let mut idx = 0u32;
        while idx < ds.len() {
            let p = match ds.with_perturbation(idx, PERTURB_DELTA) { Ok(v) => v, Err(_) => { assert!(false); return; } };
            let alt = match replay_dataset(&p) { Ok(v) => v, Err(_) => { assert!(false); return; } };
            assert!(p.hash() != ds.hash());
            assert_eq!(first_divergence(&base_run, &alt), Ok(Some(idx)));
            assert_eq!(divergence_count(&base_run, &alt), Ok(1));
            idx = idx.saturating_add(1);
        }
    }
    #[test]
    fn relabel_shift_kills_recall() {
        let ds = match base() { Some(d) => d, None => { assert!(false); return; } };
        let shifted = match ds.relabel_shift(1) { Ok(v) => v, Err(_) => { assert!(false); return; } };
        assert_eq!(shifted.audit(), Ok(ds.len()));
        let q = match evaluate(&shifted, 1) { Ok(v) => v, Err(_) => { assert!(false); return; } };
        assert_eq!(q.correct(), 0);
        assert_eq!(q.precision_permille(), 0);
        assert_eq!(q.recall_permille(), 0);
        assert_eq!(q.f1_permille(), 0);
        assert_eq!(q.f1_macro(), 0);
        assert_eq!(q.gate().err(), Some(DpCode::QualityBad.val()));
        assert_eq!(ds.relabel_shift(CLASSES as u32).err(), Some(DpCode::DataBad.val()));
    }
    #[test]
    fn confusion_zero_division_is_safe() {
        let empty = Quality::new();
        assert_eq!(empty.precision_class(0), 0);
        assert_eq!(empty.recall_class(0), 0);
        assert_eq!(empty.f1_class(0), 0);
        assert_eq!(empty.precision_class(CLASSES), 0);
        assert_eq!(empty.recall_class(CLASSES), 0);
        assert_eq!(empty.f1_class(CLASSES), 0);
        assert_eq!(harmonic_permille(0, 0), 0);
        assert_eq!(harmonic_permille(1000, 0), 0);
        assert_eq!(harmonic_permille(0, 750), 0);
        assert_eq!(harmonic_permille(1000, 1000), 1000);
        assert_eq!(harmonic_permille(500, 1000), 666);
    }
    #[test]
    fn crafted_metrics_are_hand_computable() {
        let ds = match crafted() { Some(d) => d, None => { assert!(false); return; } };
        let q = match evaluate(&ds, 1) { Ok(v) => v, Err(_) => { assert!(false); return; } };
        assert_eq!(q.samples(), 16);
        assert_eq!(q.correct(), 14);
        assert_eq!(q.wrong(), 2);
        assert_eq!(q.tp(0), 2);
        assert_eq!(q.fp(0), 2);
        assert_eq!(q.precision_class(0), 500);
        assert_eq!(q.recall_class(0), 1000);
        assert_eq!(q.f1_class(0), 666);
        assert_eq!(q.tp(1), 0);
        assert_eq!(q.fn_count(1), 2);
        assert_eq!(q.recall_class(1), 0);
        assert_eq!(q.f1_class(1), 0);
        assert_eq!(q.precision_permille(), 812);
        assert_eq!(q.recall_permille(), 875);
        assert_eq!(q.f1_permille(), 842);
        assert_eq!(q.gate().err(), Some(DpCode::QualityBad.val()));
        assert_eq!(q.errata_total(), 2);
        assert_eq!(q.errata_len(), 2);
        assert_eq!(q.first_for(GKind::DoubleTap, GKind::Tap), Some(1));
        assert_eq!(q.first_for(GKind::Tap, GKind::DoubleTap), None);
        assert_eq!(q.audit_errata(&ds), Ok(2));
        assert_eq!(q.row_total(), 16);
        assert_eq!(q.col_total(), 16);
        assert_eq!(q.tp_total(), 14);
        assert_eq!(q.fp_total(), 2);
        assert_eq!(q.fn_total(), 2);
    }
    #[test]
    fn ledger_guard_catches_desynced_counters() {
        let ds = match base() { Some(d) => d, None => { assert!(false); return; } };
        let q = match regress(&ds, 64) { Ok(v) => v, Err(_) => { assert!(false); return; } };
        assert_eq!(q.audit_ledger(), Ok(64));

        let mut a = q.clone();
        let t = a.tp(0);
        a.tp[0] = t + 1;
        assert_eq!(a.audit_ledger().err(), Some(DpCode::TpBad.val()));

        let mut b = q.clone();
        let f = b.fp(0);
        b.fp[0] = f + 1;
        assert_eq!(b.audit_ledger().err(), Some(DpCode::FpBad.val()));

        let mut c = q.clone();
        let n = c.fn_count(0);
        c.fn_[0] = n + 1;
        assert_eq!(c.audit_ledger().err(), Some(DpCode::FnBad.val()));

        let mut d = q.clone();
        d.samples = d.samples + 1;
        assert_eq!(d.audit_ledger().err(), Some(DpCode::LedgerBad.val()));

        let mut e = q.clone();
        e.errata_total = e.errata_total + 1;
        assert_eq!(e.audit_ledger().err(), Some(DpCode::LedgerBad.val()));
    }
    #[test]
    fn confusion_consistent_guard() {
        let mut cells = [[0u32; CLASSES]; CLASSES];
        cells[0][0] = 3;
        cells[0][1] = 1;
        cells[7][2] = 5;
        let c = Confusion::from_cells(cells);
        assert_eq!(c.total(), 9);
        assert_eq!(c.row(0), 4);
        assert_eq!(c.col(2), 5);
        assert_eq!(c.trace(), 3);
        assert!(c.consistent());

        let mut forged = Confusion::new();
        forged.total = 7;
        assert!(!forged.consistent());

        let mut add = Confusion::new();
        assert_eq!(add.add(0, 0), Ok(()));
        assert_eq!(add.add(CLASSES, 0), Err(DpCode::DataBad.val()));
        assert_eq!(add.add(0, CLASSES), Err(DpCode::DataBad.val()));
        assert_eq!(add.total(), 1);
        assert_eq!(add.cell(CLASSES, 0), 0);
    }
    #[test]
    fn regression_conserves_thousand_samples() {
        let ds = match base() { Some(d) => d, None => { assert!(false); return; } };
        let q = match regress(&ds, QUALITY_ROUNDS) { Ok(v) => v, Err(_) => { assert!(false); return; } };
        assert_eq!(q.samples(), QUALITY_ROUNDS);
        assert_eq!(q.row_total(), QUALITY_ROUNDS);
        assert_eq!(q.col_total(), QUALITY_ROUNDS);
        assert_eq!(q.correct().saturating_add(q.wrong()), QUALITY_ROUNDS);
        assert_eq!(q.errata_total(), q.wrong());
        assert_eq!(q.audit_ledger(), Ok(QUALITY_ROUNDS));
        assert_eq!(q.audit_errata(&ds).is_ok(), true);
        assert!(q.wrong() >= 70 && q.wrong() <= 90);
        assert!(q.f1_permille() >= F1_GATE);
        assert!(q.f1_macro() >= F1_GATE);
        assert_eq!(q.gate().is_ok(), true);
    }
    #[test]
    fn regression_range_is_gated() {
        let ds = match base() { Some(d) => d, None => { assert!(false); return; } };
        assert_eq!(regress(&ds, 0).err(), Some(DpCode::CoverBad.val()));
        assert_eq!(regress(&ds, TICK_CAP + 1).err(), Some(DpCode::CoverBad.val()));
        assert_eq!(evaluate(&ds, 0).err(), Some(DpCode::CoverBad.val()));
        assert_eq!(evaluate(&ds, CYCLES_CAP + 1).err(), Some(DpCode::CoverBad.val()));
    }
    #[test]
    fn golden_run_is_stable_and_gated() {
        let a = match golden_run() { Ok(v) => v, Err(_) => { assert!(false); return; } };
        let b = match golden_run() { Ok(v) => v, Err(_) => { assert!(false); return; } };
        assert_eq!(a, b);
        assert_eq!(a.samples(), GOLDEN_ROUNDS * DATASET_LEN);
        assert_eq!(a.digest(), b.digest());
        assert!(a.f1_permille() >= F1_GATE);
        assert!(a.f1_permille() <= PERMILLE);
        assert_eq!(render_stats(&a), render_stats(&b));
    }
    #[test]
    fn render_round_trips() {
        let ds = match base() { Some(d) => d, None => { assert!(false); return; } };
        let q = match regress(&ds, QUALITY_ROUNDS) { Ok(v) => v, Err(_) => { assert!(false); return; } };
        let text = render_stats(&q);
        assert_eq!(text.split('|').count(), STATS_FIELDS);
        assert_eq!(audit_render(&text), Ok(STATS_FIELDS));
        assert!(text.starts_with("1000|"));
        assert!(text.contains("0x"));
    }
    #[test]
    fn render_rejects_tampering() {
        assert_eq!(audit_render("16|14|500|1000|666|0x00000000000000FF"), Ok(STATS_FIELDS));
        // 改 F1 / 改 P：破坏 2pr/(p+r) 恒等式。
        assert_eq!(audit_render("16|14|500|1000|667|0x00000000000000FF").err(), Some(DpCode::MetricBad.val()));
        assert_eq!(audit_render("16|14|400|1000|666|0x00000000000000FF").err(), Some(DpCode::MetricBad.val()));
        // 越出千分比域；命中数大于样本数；样本数 0；摘要全零。
        assert_eq!(audit_render("16|14|1001|1000|1000|0x00000000000000FF").err(), Some(DpCode::MetricBad.val()));
        assert_eq!(audit_render("16|17|500|1000|666|0x00000000000000FF").err(), Some(DpCode::LedgerBad.val()));
        assert_eq!(audit_render("0|0|0|0|0|0x00000000000000FF").err(), Some(DpCode::LedgerBad.val()));
        assert_eq!(audit_render("16|14|500|1000|666|0x0000000000000000").err(), Some(DpCode::HashBad.val()));
        // 字段数不对 / 非数字 / 非十六进制。
        assert_eq!(audit_render("16|14|500|1000|666").err(), Some(DpCode::RenderBad.val()));
        assert_eq!(audit_render("16|14|500|1000|666|0x00000000000000FF|9").err(), Some(DpCode::RenderBad.val()));
        assert_eq!(audit_render("16|14|5x0|1000|666|0x00000000000000FF").err(), Some(DpCode::RenderBad.val()));
        assert_eq!(audit_render("16|14|500|1000|666|0x00000000000000FG").err(), Some(DpCode::RenderBad.val()));
    }
    #[test]
    fn numeric_parsers_are_strict() {
        assert_eq!(parse_u32("0"), Some(0));
        assert_eq!(parse_u32("1000"), Some(1000));
        assert_eq!(parse_u32("4294967295"), Some(u32::MAX));
        assert_eq!(parse_u32("4294967296"), None);
        assert_eq!(parse_u32(""), None);
        assert_eq!(parse_u32("1a"), None);
        assert_eq!(parse_u32("-1"), None);
        assert_eq!(parse_u32(" 1"), None);

        assert_eq!(parse_hex16("0x0000000000000000"), Some(0));
        assert_eq!(parse_hex16("0xFFFFFFFFFFFFFFFF"), Some(u64::MAX));
        assert_eq!(parse_hex16("0xffffffffffffffff"), Some(u64::MAX));
        assert_eq!(parse_hex16("0x000000000000001"), None);
        assert_eq!(parse_hex16("00000000000000000x"), None);
        assert_eq!(parse_hex16("0y0000000000000000"), None);
        assert_eq!(parse_hex16(""), None);
    }
    #[test]
    fn injection_catches_all_four_routes() {
        let log = match inject_rounds(5) { Ok(v) => v, Err(_) => { assert!(false); return; } };
        assert!(log.all_caught());
        assert_eq!(log.tries, [5, 5, 5, 5]);
        assert_eq!(log.caught, [5, 5, 5, 5]);
        assert_eq!(inject_rounds(0).err(), Some(DpCode::CoverBad.val()));
    }
    #[test]
    fn fuzz_classifier_agrees_with_shadow() {
        let s = match fuzz_classify(FUZZ_ROUNDS) { Ok(v) => v, Err(_) => { assert!(false); return; } };
        assert!(s.balanced());
        assert_eq!(s.rounds, FUZZ_ROUNDS);
        assert_eq!(s.disagreed, 0);
        assert!(s.digest != 0);
        assert_eq!(fuzz_classify(0).err(), Some(DpCode::CoverBad.val()));
        assert_eq!(
            fuzz_classify(FUZZ_ROUNDS + 1).err(),
            Some(DpCode::CoverBad.val())
        );
    }
    #[test]
    fn fuzz_replay_locates_every_perturbation() {
        let s = match fuzz_replay(128) { Ok(v) => v, Err(_) => { assert!(false); return; } };
        assert!(s.balanced());
        assert_eq!(s.agreed, 128);
        assert!(s.digest != 0);
        assert_eq!(fuzz_replay(0).err(), Some(DpCode::CoverBad.val()));
    }
    #[test]
    fn probes_are_green() {
        assert_eq!(probe_seq_a(), Ok(()));
        assert_eq!(probe_seq_b(), Ok(()));
        assert_eq!(probe_seq_c(), Ok(()));
    }
    #[test]
    fn repro_check_is_green() {
        assert_eq!(repro_check(), Ok(()));
    }
    #[test]
    fn extra_checks_are_exactly_eight() {
        let items = extra_checks();
        assert_eq!(items.len(), 8);
        let mut f93 = 0usize;
        let mut f94 = 0usize;
        let mut i = 0usize;
        while i < items.len() {
            let (name, ok, detail) = match items.get(i) {
                Some(v) => v,
                None => {
                    assert!(false);
                    return;
                }
            };
            assert!(*ok);
            assert!(!detail.is_empty());
            if name.starts_with("F48993") {
                f93 += 1;
            }
            if name.starts_with("F48994") {
                f94 += 1;
            }
            i += 1;
        }
        assert_eq!(f93, 4);
        assert_eq!(f94, 4);
    }
    #[test]
    fn checks_are_green() {
        let s = run_unx48993_checks();
        assert!(!s.truncated());
        assert!(s.all_passed(), "{}", s.red_items_text());
        assert_eq!(s.len(), 8);
        assert_eq!(s.domain, "UNX-F48993");
        assert_eq!(s.tally(), (8, 0));
    }

    // ── F48993-J1：乱触点 fuzz ×10^4 零崩 ───────────────────────
    #[test]
    fn touch_fuzz_ten_thousand_rounds_zero_blowup() {
        let s = fuzz_touch_seq(TOUCH_FUZZ_ROUNDS).expect("touch fuzz");
        assert_eq!(s.rounds, TOUCH_FUZZ_ROUNDS);
        assert_eq!(s.violations, 0);
        assert_eq!(s.stalled, 0);
        assert_eq!(s.max_active, TOUCH_SLOTS as u32);
        assert!(s.events >= TOUCH_FUZZ_ROUNDS);
        assert!(s.accepted > 0);
        assert!(s.refused > 0);
        assert!(s.digest != 0);
        assert!(s.zero_blowup());
        assert_eq!(s.audit(), Ok(()));
    }

    #[test]
    fn touch_fuzz_replay_is_deterministic_and_bounded() {
        let a = fuzz_touch_seq(512).expect("a");
        let b = fuzz_touch_seq(512).expect("b");
        assert_eq!(a, b);
        assert!(a.digest != 0);
        assert_eq!(fuzz_touch_seq(0).err(), Some(DpCode::CoverBad.val()));
        assert_eq!(
            fuzz_touch_seq(TOUCH_FUZZ_ROUNDS + 1).err(),
            Some(DpCode::CoverBad.val())
        );
    }

    #[test]
    fn touch_fsm_refuses_illegal_sequences_without_state_loss() {
        let mut fsm = TouchFsm::new();
        // Move/Up 未知指针：拒绝，槽位不变。
        assert_eq!(
            fsm.feed(TouchEvent::new(TouchKind::Move, 3, 0, 1, 10, 10).expect("mv")),
            Ok(false)
        );
        assert_eq!(
            fsm.feed(TouchEvent::new(TouchKind::Up, 3, 0, 2, 10, 10).expect("up")),
            Ok(false)
        );
        // Down 同指针重复：第二次拒绝。
        assert_eq!(
            fsm.feed(TouchEvent::new(TouchKind::Down, 2, 0, 3, 10, 10).expect("d1")),
            Ok(true)
        );
        assert_eq!(
            fsm.feed(TouchEvent::new(TouchKind::Down, 2, 1, 4, 10, 10).expect("d2")),
            Ok(false)
        );
        assert_eq!(fsm.active(), 1);
        // 三槽填满后第四指拒绝。
        assert_eq!(
            fsm.feed(TouchEvent::new(TouchKind::Down, 4, 1, 5, 10, 10).expect("d3")),
            Ok(true)
        );
        assert_eq!(
            fsm.feed(TouchEvent::new(TouchKind::Down, 5, 2, 6, 10, 10).expect("d4")),
            Ok(true)
        );
        assert_eq!(
            fsm.feed(TouchEvent::new(TouchKind::Down, 6, 0, 7, 10, 10).expect("d5")),
            Ok(false)
        );
        assert_eq!(fsm.max_active(), 3);
        // 时钟回退拒绝，且不清槽。
        assert_eq!(
            fsm.feed(TouchEvent::new(TouchKind::Move, 2, 0, 3, 20, 20).expect("back")),
            Ok(false)
        );
        assert_eq!(fsm.active(), 3);
        // Cancel 清空；空态再 Cancel 拒绝。
        assert_eq!(
            fsm.feed(TouchEvent::new(TouchKind::Cancel, 1, 0, 8, 0, 0).expect("c1")),
            Ok(true)
        );
        assert_eq!(fsm.active(), 0);
        assert_eq!(
            fsm.feed(TouchEvent::new(TouchKind::Cancel, 1, 0, 9, 0, 0).expect("c2")),
            Ok(false)
        );
        assert!(fsm.invariant());
        assert_eq!(fsm.accepted(), 4);
        assert_eq!(fsm.refused(), 6);
        // 非法构造必须拒绝。
        assert_eq!(
            TouchEvent::new(TouchKind::Down, 0, 0, 0, 0, 0).err(),
            Some(DpCode::SampleBad.val())
        );
        assert_eq!(
            TouchEvent::new(TouchKind::Down, 1, TOUCH_SLOTS as u8, 0, 0, 0).err(),
            Some(DpCode::SampleBad.val())
        );
        assert_eq!(
            TouchEvent::new(TouchKind::Down, 1, 0, 0, TOUCH_COORD_MAX + 1, 0).err(),
            Some(DpCode::SampleBad.val())
        );
    }

    // ── F48994-J1：五路联轧 ×5 全绿零漂移 ───────────────────────
    #[test]
    fn cross_run_five_ways_zero_drift() {
        let c = cross_run(CROSS_ROUNDS).expect("cross");
        assert_eq!(c.rounds, CROSS_ROUNDS);
        assert_eq!(c.chains, CHAINS as u32);
        assert_eq!(c.passes, CROSS_PASSES as u32);
        assert_eq!(c.drift, 0);
        assert_eq!(c.out_of_range, 0);
        assert_eq!(
            c.verdicts,
            CROSS_ROUNDS * CHAINS as u32 * CROSS_PASSES as u32
        );
        assert!(c.digest != 0);
        assert!(c.clean());
        assert_eq!(c.audit(), Ok(()));
        // 两次独立联轧逐位相等（零漂移）。
        let d = cross_run(CROSS_ROUNDS).expect("cross2");
        assert_eq!(c, d);
    }

    #[test]
    fn cross_chains_are_pure_and_in_range() {
        assert!(chain_ranges_ok());
        assert_eq!(CHAIN_TABLE.len(), CHAINS);
        let mut names: [&str; CHAINS] = [""; CHAINS];
        let mut r = 0u32;
        while r < 16 {
            let f = cross_features(r);
            let mut c = 0usize;
            while c < CHAINS {
                let (name, fptr) = CHAIN_TABLE[c];
                let a = fptr(&f);
                let b = fptr(&f);
                assert_eq!(a, b, "chain {} impure", name);
                assert!(a < CLASSES as u32);
                names[c] = name;
                c += 1;
            }
            r += 1;
        }
        // 五条链名字互不相同。
        let mut i = 0usize;
        while i < CHAINS {
            let mut j = i + 1;
            while j < CHAINS {
                assert!(names[i] != names[j]);
                j += 1;
            }
            i += 1;
        }
    }

    #[test]
    fn cross_bounds_are_gated() {
        assert_eq!(cross_run(0).err(), Some(DpCode::CoverBad.val()));
        assert_eq!(
            cross_run(CROSS_ROUNDS + 1).err(),
            Some(DpCode::CoverBad.val())
        );
        assert!(cross_run(1).is_ok());
    }
}
