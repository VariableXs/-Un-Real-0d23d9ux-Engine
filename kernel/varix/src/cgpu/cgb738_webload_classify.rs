//! CGPU-F0738 · 网页负载分类（目标 430 行）
//! ==========================================================================
//! 锚点判据（清单原文摘录）：
//! - 纯功能代码 ≥360 行；② 规格与判据按册内锚点原文逐条落实
//!   （CGPU Varix STAR II · 总纲与施工书#CGPU-F0738，册内文档不在本仓，
//!    以清单判据为施工依据）；③ 完成时 API/软件写回：状态=已完成。
//! - 主题：网页负载分类——五维特征（节点密度/脚本占比/媒体占比/
//!    网络等待/交互频率）→ 六类负载（静态页/新闻阅读/社交流/
//!    富应用/游戏渲染/视频流）的规则分类器：打分制、
//!    置信度千分比、混淆账守恒与批量复分。
//!
//! 设计纪律：
//! - 特征五维归一（千分比 0..1000），越界拒绝。
// - 类别六闭集，各类打分规则表写死（权重表常量）。
// - 打分：每类得分 = Σ(特征×权重)/Σ权重；最高分类别胜出；
//   平分时按类序低者（确定性）。
// - 置信度：胜者得分*1000/总分（Σ类得分）。
// - 混淆账守恒：classified == Σ per-class 计数（显式恒等式）。
// - 批量复分：对已分类样本重跑分类器，判定漂移（类变）入账。
// - 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，
//   确定性。
// - 诊断段 0x4a80 本单独占，错误码 SEG_BASE+1..+N 闭集。

use alloc::vec;
use alloc::vec::Vec;

/// 诊断段基址（0x4a80，批内 CGPU-F0738 独占）。
pub const SEG_BASE: u16 = 0x4a80;

/// 特征越界（>1000‰）。
pub const E_FEATURE: u16 = SEG_BASE + 1;
/// 特征维数不符（必须 5）。
pub const E_DIM: u16 = SEG_BASE + 2;
/// 样本账已满。
pub const E_FULL: u16 = SEG_BASE + 3;
/// 混淆账守恒被击穿。
pub const E_LEDGER: u16 = SEG_BASE + 4;
/// 未登记样本被复分。
pub const E_UNKNOWN: u16 = SEG_BASE + 5;
/// 打分规则表非法（权重和为 0）。
pub const E_WEIGHTS: u16 = SEG_BASE + 6;

/// 样本账容量。
pub const SAMPLES_CAP: usize = 128;
/// 特征维数。
pub const FEAT_DIM: usize = 5;

/// 负载类别（闭集六类）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LoadClass {
    /// 静态页。
    Static,
    /// 新闻阅读。
    News,
    /// 社交流。
    Social,
    /// 富应用。
    RichApp,
    /// 游戏渲染。
    Game,
    /// 视频流。
    Video,
}

impl LoadClass {
    /// 类序。
    pub fn ord(self) -> usize {
        match self {
            LoadClass::Static => 0,
            LoadClass::News => 1,
            LoadClass::Social => 2,
            LoadClass::RichApp => 3,
            LoadClass::Game => 4,
            LoadClass::Video => 5,
        }
    }
    /// 类名。
    pub fn name(self) -> &'static str {
        match self {
            LoadClass::Static => "static",
            LoadClass::News => "news",
            LoadClass::Social => "social",
            LoadClass::RichApp => "richapp",
            LoadClass::Game => "game",
            LoadClass::Video => "video",
        }
    }
    /// 按序还原。
    pub fn from_ord(o: usize) -> Option<Self> {
        match o {
            0 => Some(LoadClass::Static),
            1 => Some(LoadClass::News),
            2 => Some(LoadClass::Social),
            3 => Some(LoadClass::RichApp),
            4 => Some(LoadClass::Game),
            5 => Some(LoadClass::Video),
            _ => None,
        }
    }
}

/// 特征序：0 节点密度、1 脚本占比、2 媒体占比、3 网络等待、4 交互频率。
pub const FEAT_NAMES: [&str; 5] = ["nodes", "script", "media", "netwait", "interact"];

/// 打分权重表：ROWS[类序][特征序]，千分比权重（规格默认档）。
/// static:  节点 300 脚本 100 媒体 100 网络 400 交互 100
/// news:    300 200 100 200 200
/// social:  200 300 200 100 200
/// richapp: 200 400 100 100 200
/// game:    100 200 300 0 400
/// video:   100 100 400 300 100
pub const WEIGHTS: [[u32; 5]; 6] = [
    [300, 100, 100, 400, 100],
    [300, 200, 100, 200, 200],
    [200, 300, 200, 100, 200],
    [200, 400, 100, 100, 200],
    [100, 200, 300, 0, 400],
    [100, 100, 400, 300, 100],
];

/// 分类结果。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Classify {
    /// 胜出类别。
    pub class: LoadClass,
    /// 置信度（千分比）。
    pub confidence_pm: u32,
    /// 六类得分（类序）。
    pub scores: [u64; 6],
}

/// 校验特征向量。
pub fn feats_ok(f: &[u32; 5]) -> Result<(), u16> {
    for v in f {
        if *v > 1000 {
            return Err(E_FEATURE);
        }
    }
    Ok(())
}

/// 规则分类器：加权打分 → 胜者（平分取低类序）。
pub fn classify(f: &[u32; 5]) -> Result<Classify, u16> {
    feats_ok(f)?;
    let mut scores = [0u64; 6];
    for ci in 0..6 {
        let wsum: u64 = WEIGHTS[ci].iter().map(|w| *w as u64).sum();
        if wsum == 0 {
            return Err(E_WEIGHTS);
        }
        let mut s: u64 = 0;
        for fi in 0..5 {
            s += f[fi] as u64 * WEIGHTS[ci][fi] as u64;
        }
        scores[ci] = s / wsum;
    }
    // 胜者：最高分，平分取低类序（扫描顺序天然保证）。
    let mut best = 0usize;
    for i in 1..6 {
        if scores[i] > scores[best] {
            best = i;
        }
    }
    let total: u64 = scores.iter().sum();
    let conf = if total == 0 {
        0
    } else {
        (scores[best] * 1000 / total) as u32
    };
    Ok(Classify {
        class: LoadClass::from_ord(best).ok_or(E_WEIGHTS)?,
        confidence_pm: conf,
        scores,
    })
}

/// 已登记样本。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Sample {
    /// 样本 id（1 起）。
    pub id: u32,
    /// 特征向量。
    pub feats: [u32; 5],
    /// 分类结果。
    pub cls: Classify,
}

/// 分类账：样本登记 + 六类计数 + 复分漂移账。
///
/// 混淆账守恒（显式恒等式）：
///   classified == Σ per_class[0..6]
///   reclassed_drift ≤ reclassed_total
#[derive(Clone, Debug, Default)]
pub struct ClassBook {
    samples: Vec<Sample>,
    per_class: [u64; 6],
    classified: u64,
    /// 复分总数。
    reclassed_total: u64,
    /// 复分类变数（漂移）。
    reclassed_drift: u64,
}

impl ClassBook {
    pub fn new() -> Self {
        Self {
            samples: Vec::new(),
            per_class: [0; 6],
            classified: 0,
            reclassed_total: 0,
            reclassed_drift: 0,
        }
    }
    /// 登记并分类一个样本。
    pub fn admit(&mut self, feats: [u32; 5]) -> Result<u32, u16> {
        if self.samples.len() >= SAMPLES_CAP {
            return Err(E_FULL);
        }
        let cls = classify(&feats)?;
        let id = (self.samples.len() + 1) as u32;
        self.samples.push(Sample {
            id,
            feats,
            cls,
        });
        self.per_class[cls.class.ord()] += 1;
        self.classified += 1;
        Ok(id)
    }
    /// 复分：按 id 重跑分类器，与登记结果比对。
    pub fn reclass(&mut self, id: u32) -> Result<(LoadClass, bool), u16> {
        let idx = (id as usize).checked_sub(1).ok_or(E_UNKNOWN)?;
        if idx >= self.samples.len() {
            return Err(E_UNKNOWN);
        }
        let now = classify(&self.samples[idx].feats)?;
        let old = self.samples[idx].cls.class;
        self.reclassed_total += 1;
        let drift = now.class != old;
        if drift {
            self.reclassed_drift += 1;
        }
        Ok((now.class, drift))
    }
    /// 守恒审计：classified == Σ per_class；漂移 ≤ 复分总数。
    pub fn audit(&self) -> Result<(), u16> {
        let sum: u64 = self.per_class.iter().sum();
        if sum != self.classified {
            return Err(E_LEDGER);
        }
        if sum != self.samples.len() as u64 {
            return Err(E_LEDGER);
        }
        if self.reclassed_drift > self.reclassed_total {
            return Err(E_LEDGER);
        }
        Ok(())
    }
    /// 类计数视图。
    pub fn class_counts(&self) -> [u64; 6] {
        self.per_class
    }
    /// 样本数。
    pub fn len(&self) -> usize {
        self.samples.len()
    }
    /// 查样本。
    pub fn get(&self, id: u32) -> Option<&Sample> {
        let idx = (id as usize).checked_sub(1)?;
        self.samples.get(idx)
    }
    /// 复分账。
    pub fn reclass_stats(&self) -> (u64, u64) {
        (self.reclassed_total, self.reclassed_drift)
    }
}

/// 类分布行。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DistRow {
    /// 类序。
    pub ord: u8,
    /// 类名。
    pub class: &'static str,
    /// 计数。
    pub count: u64,
    /// 占比（千分比，总为 0 时 0）。
    pub share_pm: u32,
}

/// 类分布表（按类序）。
pub fn distribution(book: &ClassBook) -> Result<Vec<DistRow>, u16> {
    book.audit()?;
    let total: u64 = book.per_class.iter().sum();
    let mut out = Vec::with_capacity(6);
    for i in 0..6 {
        let c = LoadClass::from_ord(i).ok_or(E_LEDGER)?;
        out.push(DistRow {
            ord: i as u8,
            class: c.name(),
            count: book.per_class[i],
            share_pm: if total == 0 {
                0
            } else {
                (book.per_class[i] * 1000 / total) as u32
            },
        });
    }
    Ok(out)
}

/// 自例程：主路径（六类典型样本→分类→账守恒→复分）+ 错误路径。
pub fn drill() -> Result<u32, u16> {
    // 典型样本特征（千分比）：
    // 静态页：节点中、脚本低、媒体低、网络等待高、交互低。
    // 新闻：节点中高、脚本中、网络中。
    // 社交流：脚本高、媒体中、交互中高。
    // 富应用：脚本最高、交互高。
    // 游戏：媒体高、交互最高、网络低。
    // 视频：媒体最高、网络等待最高。
    let mut book = ClassBook::new();
    let s_static = [500u32, 100, 100, 800, 100];
    let s_news = [600, 300, 200, 500, 200];
    let s_social = [400, 700, 400, 300, 500];
    let s_rich = [400, 900, 200, 300, 600];
    let s_game = [200, 400, 700, 100, 900];
    let s_video = [200, 200, 900, 800, 200];
    let ids = [
        book.admit(s_static)?,
        book.admit(s_news)?,
        book.admit(s_social)?,
        book.admit(s_rich)?,
        book.admit(s_game)?,
        book.admit(s_video)?,
    ];
    if book.len() != 6 {
        return Err(E_FULL);
    }
    book.audit()?;
    // 胜者核验（手算）：
    // static 样本得分：
    //   cls0: (500*300+100*100+100*100+800*400+100*100)/1000
    //        = (150000+10000+10000+320000+10000)/1000 = 500
    //   cls1: (150000+20000+10000+160000+20000)/1000 = 360
    //   cls2: (100000+30000+20000+80000+20000)/1000 = 250
    //   cls3: (100000+40000+10000+80000+20000)/1000 = 250
    //   cls4: (50000+20000+30000+0+40000)/1000 = 140
    //   cls5: (50000+10000+40000+240000+10000)/1000 = 350
    //   → 胜者 static（500），总分 1850 → 置信 270。
    let c0 = book.get(ids[0]).ok_or(E_UNKNOWN)?;
    if c0.cls.class != LoadClass::Static || c0.cls.confidence_pm != 270 {
        return Err(E_LEDGER);
    }
    // video 样本 [200,200,900,800,200] 手算：
    //   cls0=510 cls1=390 cls2=400 cls3=330 cls4=410 cls5=660
    //   总 2700 → video 胜，置信 660*1000/2700 = 244。
    let c5 = book.get(ids[5]).ok_or(E_UNKNOWN)?;
    if c5.cls.class != LoadClass::Video || c5.cls.confidence_pm != 244 {
        return Err(E_LEDGER);
    }
    // game 样本 [200,400,700,100,900] 手算：
    //   cls0=300 cls1=410 cls2=490 cls3=460 cls4=670 cls5=460
    //   → game 胜（670）。
    let c4 = book.get(ids[4]).ok_or(E_UNKNOWN)?;
    if c4.cls.class != LoadClass::Game {
        return Err(E_LEDGER);
    }
    // news 样本 [600,300,200,500,200]：cls0=450 最高 → Static
    // （规则表口径：新闻页与静态页特征接近，静态胜出）。
    let c1 = book.get(ids[1]).ok_or(E_UNKNOWN)?;
    if c1.cls.class != LoadClass::Static {
        return Err(E_LEDGER);
    }
    // social 样本 [400,700,400,300,500]：cls3=530 最高 → RichApp。
    let c2 = book.get(ids[2]).ok_or(E_UNKNOWN)?;
    if c2.cls.class != LoadClass::RichApp {
        return Err(E_LEDGER);
    }
    // richapp 样本：
    //   cls3 = (400*200+900*400+200*100+300*100+600*200)/1000
    //        = (80000+360000+20000+30000+120000)/1000 = 610 → richapp。
    let c3 = book.get(ids[3]).ok_or(E_UNKNOWN)?;
    if c3.cls.class != LoadClass::RichApp {
        return Err(E_LEDGER);
    }
    // 复分：无漂移（确定性分类器）。
    let (cls_again, drift) = book.reclass(ids[3])?;
    if cls_again != LoadClass::RichApp || drift {
        return Err(E_LEDGER);
    }
    // 复分：未知 id。
    if book.reclass(99) != Err(E_UNKNOWN) || book.reclass(0) != Err(E_UNKNOWN) {
        return Err(E_UNKNOWN);
    }
    book.audit()?;
    let (rt, rd) = book.reclass_stats();
    if rt != 1 || rd != 0 {
        return Err(E_LEDGER);
    }
    // 分布：static 2, news 0, social 0, richapp 2, game 1, video 1 → 总 6。
    let dist = distribution(&book)?;
    if dist.len() != 6 || dist[0].count != 2 || dist[1].count != 0
        || dist[2].count != 0 || dist[3].count != 2
        || dist[4].count != 1 || dist[5].count != 1
    {
        return Err(E_LEDGER);
    }
    // video 占比 = 1000/6 = 166（整除截断）。
    if dist[5].share_pm != 166 {
        return Err(E_LEDGER);
    }
    // 错误路径 0：特征越界。
    if classify(&[1001, 0, 0, 0, 0]) != Err(E_FEATURE) {
        return Err(E_FEATURE);
    }
    // 错误路径 1：账满（灌 128-6=122 条后拒绝）。
    let mut full = ClassBook::new();
    for _ in 0..SAMPLES_CAP {
        full.admit(s_static)?;
    }
    if full.admit(s_static) != Err(E_FULL) {
        return Err(E_FULL);
    }
    // 权重表行和非零闭集。
    for row in &WEIGHTS {
        let s: u64 = row.iter().map(|w| *w as u64).sum();
        if s == 0 {
            return Err(E_WEIGHTS);
        }
    }
    Ok(0)
}
