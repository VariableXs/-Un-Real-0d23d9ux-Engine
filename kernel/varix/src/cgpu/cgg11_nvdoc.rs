//! cgg11_nvdoc — CGPU-F0971 NVIDIA 适配文档（白皮书三节+已知问题清单+认证指南）。
//!
//! 判据原文（施工书锚点 CGPU-F0971）：文档：NVIDIA 适配白皮书（六代覆盖/
//! 特性/双驱动线）；NVIDIA 已知问题清单；认证指南（NVIDIA 侧）。判据：白
//! 皮书、清单、指南、判据。
//!
//! 职责定位：NVIDIA 适配白皮书三节闭集（六代覆盖表/特性矩阵/双驱动线
//! GameReady-Studio——节缺 0x7901）+已知问题清单（每条问题必须带复现条件
//! 与影响面——缺一 0x7902，"有问题没条件"的清单条目是怨气不是文档）+认
//! 证指南（NVIDIA 侧步骤表：探测→差异→媒体标签→提交，步骤缺 0x7903）+
//! 文档-代码对拍（漂移 0x7904）+新鲜度（过期 0x7905）。诊断码段
//! 0x7901~0x7905。

use alloc::vec::Vec;

/// 模块版本。
pub const NV_DOC_VERSION: &str = "G11-nvdoc-v1";

// ---------------------------------------------------------------------------
// 诊断码（XcCode 0x79 段，本会话独占）
// ---------------------------------------------------------------------------

/// 0x7901 白皮书节缺（三节不全）。
pub const XC_NVWP_SECTION_MISS: u32 = 0x7901;
/// 0x7902 已知问题条目缺（复现条件/影响面缺一）。
pub const XC_ISSUE_ENTRY_MISS: u32 = 0x7902;
/// 0x7903 认证指南缺（步骤表不全）。
pub const XC_CERT_GUIDE_MISS: u32 = 0x7903;
/// 0x7904 文档-代码漂移。
pub const XC_NVDOC_DRIFT: u32 = 0x7904;
/// 0x7905 文档过期。
pub const XC_NVDOC_STALE: u32 = 0x7905;

// ---------------------------------------------------------------------------
// 常量
// ---------------------------------------------------------------------------

/// NVIDIA 六代覆盖闭集（GTX10/RTX20/RTX30/RTX40/RTX50/未定代预留）。
pub const NV_GEN_COUNT: usize = 6;
/// 双驱动线（GameReady/Studio）。
pub const DRIVER_LINE_COUNT: usize = 2;
/// 认证指南步骤数。
pub const CERT_GUIDE_STEPS: usize = 4;
/// 白皮书指纹种子。
pub const NVDOC_FP_SEED: u64 = 0x4E56_4400_0000_1971;

// ---------------------------------------------------------------------------
// 一、NVIDIA 适配白皮书（三节闭集）
// ---------------------------------------------------------------------------

/// 白皮书节。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum NvWpSection {
    /// 六代覆盖表（每代能力锚）。
    GenCoverage,
    /// 特性矩阵（DLSS/Reflex/广播特性按代布尔）。
    FeatureMatrix,
    /// 双驱动线（GameReady/Studio 分线声明）。
    DriverLines,
}

impl NvWpSection {
    /// 三节闭集。
    pub const ALL: [NvWpSection; 3] = [
        NvWpSection::GenCoverage,
        NvWpSection::FeatureMatrix,
        NvWpSection::DriverLines,
    ];

    /// 序号。
    pub const fn idx(self) -> usize {
        match self {
            NvWpSection::GenCoverage => 0,
            NvWpSection::FeatureMatrix => 1,
            NvWpSection::DriverLines => 2,
        }
    }
}

/// 白皮书（判据"NVIDIA 适配白皮书（六代覆盖/特性/双驱动线"）。
#[derive(Debug)]
pub struct NvWhitepaper {
    /// 已登记节位图。
    pub sections: u32,
    /// 每代覆盖锚（六代——0=未覆盖显性）。
    pub gen_anchors: [u32; NV_GEN_COUNT],
    /// 双驱动线在册位图。
    pub driver_lines: u32,
    /// 节缺事件数（0x7901）。
    pub miss_events: u32,
}

impl NvWhitepaper {
    /// 新白皮书（空）。
    pub const fn new() -> NvWhitepaper {
        NvWhitepaper {
            sections: 0,
            gen_anchors: [0; NV_GEN_COUNT],
            driver_lines: 0,
            miss_events: 0,
        }
    }

    /// 登记节。
    pub fn register(&mut self, s: NvWpSection) {
        self.sections |= 1 << s.idx();
    }

    /// 登记驱动线（0=GameReady 1=Studio）。
    pub fn register_line(&mut self, line: usize) -> Result<(), u32> {
        if line >= DRIVER_LINE_COUNT {
            return Err(XC_NVWP_SECTION_MISS);
        }
        self.driver_lines |= 1 << line;
        Ok(())
    }

    /// 完备裁决：三节齐+六代锚全非零（未覆盖代显性登记为 0xFFFFFFFF 而非 0）
    /// +双线齐（0x7901）。
    pub fn complete(&mut self) -> Result<(), u32> {
        let want = 0b111;
        if self.sections != want || self.driver_lines != 0b11 {
            self.miss_events += 1;
            return Err(XC_NVWP_SECTION_MISS);
        }
        let mut i = 0;
        while i < NV_GEN_COUNT {
            if self.gen_anchors[i] == 0 {
                self.miss_events += 1;
                return Err(XC_NVWP_SECTION_MISS);
            }
            i += 1;
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// 二、NVIDIA 已知问题清单
// ---------------------------------------------------------------------------

/// 已知问题条目。
#[derive(Clone, Copy, Debug)]
pub struct KnownIssue {
    /// 问题号。
    pub no: u32,
    /// 复现条件锚（非零——怎么踩到）。
    pub repro_anchor: u32,
    /// 影响面锚（非零——波及什么）。
    pub impact_anchor: u32,
}

/// 已知问题清单（判据"NVIDIA 已知问题清单"——条目双锚纪律）。
#[derive(Debug)]
pub struct KnownIssues {
    /// 条目表。
    pub items: Vec<KnownIssue>,
    /// 条目缺事件数（0x7902）。
    pub miss_events: u32,
}

impl KnownIssues {
    /// 新清单（空）。
    pub const fn new() -> KnownIssues {
        KnownIssues {
            items: Vec::new(),
            miss_events: 0,
        }
    }

    /// 加条目（复现条件/影响面缺一 0x7902）。
    pub fn add(&mut self, it: KnownIssue) -> Result<(), u32> {
        if it.repro_anchor == 0 || it.impact_anchor == 0 {
            self.miss_events += 1;
            return Err(XC_ISSUE_ENTRY_MISS);
        }
        self.items.push(it);
        Ok(())
    }

    /// 清单最低完备：至少一条在册（空清单=假没问题的谎言面）。
    pub fn sane(&self) -> bool {
        !self.items.is_empty()
    }
}

// ---------------------------------------------------------------------------
// 三、认证指南（NVIDIA 侧）
// ---------------------------------------------------------------------------

/// 认证指南步骤（判据"认证指南（NVIDIA 侧"）。
#[derive(Clone, Copy, Debug)]
pub struct CertGuideStep {
    /// 步号。
    pub no: usize,
    /// 步骤锚（NVIDIA 侧实体——非零）。
    pub anchor: u32,
}

/// 认证指南。
#[derive(Debug)]
pub struct CertGuide {
    /// 步骤表。
    pub steps: Vec<CertGuideStep>,
    /// 步缺事件数（0x7903）。
    pub miss_events: u32,
}

impl CertGuide {
    /// 新指南（空）。
    pub const fn new() -> CertGuide {
        CertGuide {
            steps: Vec::new(),
            miss_events: 0,
        }
    }

    /// 加步（锚空 0x7903）。
    pub fn add(&mut self, no: usize, anchor: u32) -> Result<(), u32> {
        if anchor == 0 {
            self.miss_events += 1;
            return Err(XC_CERT_GUIDE_MISS);
        }
        self.steps.push(CertGuideStep { no, anchor });
        Ok(())
    }

    /// 完备裁决：四步齐（探测→差异→媒体标签→提交）。
    pub fn complete(&mut self) -> Result<(), u32> {
        if self.steps.len() < CERT_GUIDE_STEPS {
            self.miss_events += 1;
            return Err(XC_CERT_GUIDE_MISS);
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// 四、对拍+新鲜度
// ---------------------------------------------------------------------------

/// 文档对拍账（漂移 0x7904 + 过期 0x7905）。
#[derive(Debug)]
pub struct NvDocSync {
    /// 白皮书引用的代际数。
    pub doc_gen_count: usize,
    /// 白皮书引用的驱动线数。
    pub doc_line_count: usize,
    /// 最近对拍指纹。
    pub synced_fp: u64,
    /// 漂移事件数（0x7904）。
    pub drift_events: u32,
    /// 过期事件数（0x7905）。
    pub stale_events: u32,
}

impl NvDocSync {
    /// 新账。
    pub const fn new() -> NvDocSync {
        NvDocSync {
            doc_gen_count: NV_GEN_COUNT,
            doc_line_count: DRIVER_LINE_COUNT,
            synced_fp: 0,
            drift_events: 0,
            stale_events: 0,
        }
    }

    /// 对拍。
    pub fn reconcile(&mut self, impl_fp: u64) -> Result<(), u32> {
        if self.doc_gen_count != NV_GEN_COUNT || self.doc_line_count != DRIVER_LINE_COUNT {
            self.drift_events += 1;
            return Err(XC_NVDOC_DRIFT);
        }
        self.synced_fp = impl_fp;
        Ok(())
    }

    /// 新鲜裁决。
    pub fn fresh(&mut self, current: u64) -> Result<(), u32> {
        if self.synced_fp == 0 || self.synced_fp != current {
            self.stale_events += 1;
            return Err(XC_NVDOC_STALE);
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// 五、总引擎 + 自检（白皮书/清单/指南/对拍/判据五组+段对账）
// ---------------------------------------------------------------------------

/// NVIDIA 适配文档总引擎。
#[derive(Debug)]
pub struct NvDocEngine {
    /// 白皮书。
    pub wp: NvWhitepaper,
    /// 已知问题清单。
    pub issues: KnownIssues,
    /// 认证指南。
    pub guide: CertGuide,
    /// 对拍账。
    pub sync: NvDocSync,
}

impl NvDocEngine {
    /// 出厂。
    pub const fn new() -> NvDocEngine {
        NvDocEngine {
            wp: NvWhitepaper::new(),
            issues: KnownIssues::new(),
            guide: CertGuide::new(),
            sync: NvDocSync::new(),
        }
    }

    /// 组一：白皮书——三节齐+六代锚全非零+双线齐；缺一 0x7901。
    pub fn selfcheck_wp(&mut self) -> Result<(), u32> {
        let mut e = NvDocEngine::new();
        // 缺节→0x7901。
        e.wp.register(NvWpSection::GenCoverage);
        if e.wp.complete() != Err(XC_NVWP_SECTION_MISS) {
            return Err(XC_NVWP_SECTION_MISS);
        }
        // 六代锚缺→0x7901。
        let mut s = 0;
        while s < 3 {
            e.wp.register(NvWpSection::ALL[s]);
            s += 1;
        }
        e.wp.register_line(0)?;
        e.wp.register_line(1)?;
        if e.wp.complete() != Err(XC_NVWP_SECTION_MISS) {
            return Err(XC_NVWP_SECTION_MISS);
        }
        // 六代锚全填（未覆盖代登记 0xFFFFFFFF 显性）→过。
        let mut i = 0;
        while i < NV_GEN_COUNT {
            e.wp.gen_anchors[i] = if i < 5 { 0x4E56_1000 + i as u32 } else { 0xFFFF_FFFF };
            i += 1;
        }
        e.wp.complete()?;
        Ok(())
    }

    /// 组二：清单——双锚纪律；空清单不健全；合法条目过。
    pub fn selfcheck_issues(&mut self) -> Result<(), u32> {
        let mut e = NvDocEngine::new();
        // 缺复现条件→0x7902。
        if e.issues.add(KnownIssue { no: 1, repro_anchor: 0, impact_anchor: 9 })
            != Err(XC_ISSUE_ENTRY_MISS)
        {
            return Err(XC_ISSUE_ENTRY_MISS);
        }
        // 缺影响面→0x7902。
        if e.issues.add(KnownIssue { no: 1, repro_anchor: 9, impact_anchor: 0 })
            != Err(XC_ISSUE_ENTRY_MISS)
        {
            return Err(XC_ISSUE_ENTRY_MISS);
        }
        if e.issues.miss_events != 2 {
            return Err(XC_ISSUE_ENTRY_MISS);
        }
        // 合法条目过。
        e.issues.add(KnownIssue { no: 1, repro_anchor: 0x9001, impact_anchor: 0x9002 })?;
        if !e.issues.sane() {
            return Err(XC_ISSUE_ENTRY_MISS);
        }
        Ok(())
    }

    /// 组三：认证指南——四步齐；锚空 0x7903。
    pub fn selfcheck_guide(&mut self) -> Result<(), u32> {
        let mut e = NvDocEngine::new();
        // 锚空→0x7903。
        if e.guide.add(0, 0) != Err(XC_CERT_GUIDE_MISS) {
            return Err(XC_CERT_GUIDE_MISS);
        }
        if e.guide.miss_events != 1 {
            return Err(XC_CERT_GUIDE_MISS);
        }
        // 四步入册（探测→差异→媒体标签→提交）。
        e.guide.add(0, 0x4E56_2001)?;
        e.guide.add(1, 0x4E56_2002)?;
        e.guide.add(2, 0x4E56_2003)?;
        e.guide.add(3, 0x4E56_2004)?;
        e.guide.complete()?;
        Ok(())
    }

    /// 组四：对拍——漂移显性；过期显性。
    pub fn selfcheck_sync(&mut self) -> Result<(), u32> {
        let mut e = NvDocEngine::new();
        // 代际数漂移→0x7904。
        e.sync.doc_gen_count = 5;
        if e.sync.reconcile(0xAA) != Err(XC_NVDOC_DRIFT) {
            return Err(XC_NVDOC_DRIFT);
        }
        if e.sync.drift_events != 1 {
            return Err(XC_NVDOC_DRIFT);
        }
        // 修正→对拍过。
        e.sync.doc_gen_count = NV_GEN_COUNT;
        e.sync.reconcile(0xAA)?;
        // 实现推进→过期 0x7905。
        if e.sync.fresh(0xAB) != Err(XC_NVDOC_STALE) {
            return Err(XC_NVDOC_STALE);
        }
        // 重对拍→新鲜。
        e.sync.reconcile(0xAB)?;
        e.sync.fresh(0xAB)?;
        Ok(())
    }

    /// 组五：判据——端到端：白皮书+清单+指南+对拍全链绿。
    pub fn selfcheck_verdict(&mut self) -> Result<(), u32> {
        let mut e = NvDocEngine::new();
        let mut s = 0;
        while s < 3 {
            e.wp.register(NvWpSection::ALL[s]);
            s += 1;
        }
        e.wp.register_line(0)?;
        e.wp.register_line(1)?;
        let mut i = 0;
        while i < NV_GEN_COUNT {
            e.wp.gen_anchors[i] = 0x4E56_1000 + i as u32;
            i += 1;
        }
        e.wp.complete()?;
        e.issues.add(KnownIssue { no: 1, repro_anchor: 1, impact_anchor: 2 })?;
        e.issues.add(KnownIssue { no: 2, repro_anchor: 3, impact_anchor: 4 })?;
        let mut g = 0;
        while g < CERT_GUIDE_STEPS {
            e.guide.add(g, 0x4E56_2000 + g as u32)?;
            g += 1;
        }
        e.guide.complete()?;
        e.sync.reconcile(0xBEEF)?;
        e.sync.fresh(0xBEEF)?;
        // 全链账面零事件。
        if e.wp.miss_events != 0
            || e.issues.miss_events != 0
            || e.guide.miss_events != 0
            || e.sync.drift_events != 0
            || e.sync.stale_events != 0
        {
            return Err(XC_NVWP_SECTION_MISS);
        }
        Ok(())
    }

    /// 全量自检入口（五组+段对账）。
    pub fn selfcheck_all(&mut self) -> Result<(), u32> {
        self.selfcheck_wp()?;
        self.selfcheck_issues()?;
        self.selfcheck_guide()?;
        self.selfcheck_sync()?;
        self.selfcheck_verdict()?;
        Self::segment_guard()?;
        Ok(())
    }

    /// 段对账：五码全落 0x7900..0x7A00 开区间且互异。
    pub const fn segment_guard() -> Result<(), u32> {
        let codes = [
            XC_NVWP_SECTION_MISS,
            XC_ISSUE_ENTRY_MISS,
            XC_CERT_GUIDE_MISS,
            XC_NVDOC_DRIFT,
            XC_NVDOC_STALE,
        ];
        let mut i = 0;
        while i < codes.len() {
            if codes[i] < 0x7900 || codes[i] >= 0x7A00 {
                return Err(XC_NVWP_SECTION_MISS);
            }
            let mut j = i + 1;
            while j < codes.len() {
                if codes[i] == codes[j] {
                    return Err(XC_NVWP_SECTION_MISS);
                }
                j += 1;
            }
            i += 1;
        }
        Ok(())
    }
}
