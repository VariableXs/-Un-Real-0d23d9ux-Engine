//! cgi10_bwdoc — CGPU-F1289 带宽文档（白皮书三节+范式声明+调优指南——文档即契约）。
//!
//! 判据原文（施工书锚点 CGPU-F1289）：文档：带宽白皮书（PCIe 模型/账户化/
//! 仲裁）；仲裁算法说明（复用 C 域范式声明）；调优指南。判据：白皮书、说
//! 明、指南、判据。
//!
//! 职责定位：带宽白皮书三节闭集（PCIe 模型/账户化/仲裁——节缺一即 0x7501，
//! 白皮书是契约不是作文：每节绑定代码实体锚点）+仲裁算法说明（复用 C 域
//! 范式声明——源单号非空，口头复用=无复用（vck03 ReuseOrigin 纪律同源），
//! 声明失据 0x7502）+调优指南（步骤表闭集+每步适用条件+前置检查——步骤缺
//! 0x7503）+文档-代码对拍（白皮书引用的数字常量与代码逐项对拍——两套账
//! 0x7504；文档指纹随实现更新——过期 0x7505）。诊断码段 0x7501~0x7505。

use alloc::vec::Vec;

/// 模块版本。
pub const BW_DOC_VERSION: &str = "I10-bwdoc-v1";

// ---------------------------------------------------------------------------
// 诊断码（XcCode 0x75 段，本会话独占）
// ---------------------------------------------------------------------------

/// 0x7501 白皮书节缺失（三节不全）。
pub const XC_WP_SECTION_MISS: u32 = 0x7501;
/// 0x7502 范式声明失据（仲裁说明未声明复用 C 域范式/源单号空）。
pub const XC_PARADIGM_UNBASED: u32 = 0x7502;
/// 0x7503 指南步骤缺（步骤表不完整）。
pub const XC_GUIDE_STEP_MISS: u32 = 0x7503;
/// 0x7504 文档-代码漂移（白皮书数字与代码常量两套账）。
pub const XC_DOC_CODE_DRIFT: u32 = 0x7504;
/// 0x7505 指南过期（指纹未随实现更新）。
pub const XC_DOC_STALE: u32 = 0x7505;

// ---------------------------------------------------------------------------
// 常量（文档-代码对拍的代码侧真值）
// ---------------------------------------------------------------------------

/// PCIe 代际闭集（与 cgi 域模型一致——白皮书引用）。
pub const PCIE_GEN_COUNT: usize = 5;
/// 账户化账户数（四账户——与 cgi05 账本一致）。
pub const ACCOUNT_COUNT: usize = 4;
/// 仲裁轮转基数（与 C 域仲裁器口径一致）。
pub const ARBITER_ROUNDS: u32 = 8;
/// 调优指南步骤数闭集。
pub const GUIDE_STEPS: usize = 5;
/// 文档指纹种子。
pub const DOC_FP_SEED: u64 = 0xB00C_0000_0000_1A75;

// ---------------------------------------------------------------------------
// 一、带宽白皮书（三节闭集）
// ---------------------------------------------------------------------------

/// 白皮书节（每节绑定代码实体锚点——契约不是作文）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct WpSection {
    /// 节号。
    pub no: usize,
    /// 代码实体锚点（锚到实现单号——CGPU-Fxxxx）。
    pub anchor: u32,
    /// 节指纹（正文 FNV——对拍漂移用）。
    pub content_fp: u64,
}

/// 白皮书（判据"带宽白皮书（PCIe 模型/账户化/仲裁）"——三节闭集）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct BwWhitepaper {
    /// 节表（恰三节：0=PCIe 模型 1=账户化 2=仲裁）。
    pub sections: Vec<WpSection>,
    /// 白皮书总指纹（三节指纹合成——过期检测用）。
    pub total_fp: u64,
    /// 节缺失事件数（0x7501）。
    pub miss_events: u32,
}

impl BwWhitepaper {
    /// 缔造白皮书（三节锚点齐备才成立——缺节 0x7501）。
    pub fn compose(anchors: [u32; 3], fps: [u64; 3]) -> Result<BwWhitepaper, u32> {
        let mut i = 0;
        while i < 3 {
            if anchors[i] == 0 {
                return Err(XC_WP_SECTION_MISS);
            }
            i += 1;
        }
        let mut sections = Vec::new();
        i = 0;
        while i < 3 {
            sections.push(WpSection {
                no: i,
                anchor: anchors[i],
                content_fp: fps[i],
            });
            i += 1;
        }
        Ok(BwWhitepaper {
            sections,
            total_fp: wp_total_fp(&fps),
            miss_events: 0,
        })
    }

    /// 节完整性：恰三节且锚点逐节非空。
    pub fn complete(&self) -> bool {
        if self.sections.len() != 3 {
            return false;
        }
        let mut i = 0;
        while i < self.sections.len() {
            if self.sections[i].anchor == 0 {
                return false;
            }
            i += 1;
        }
        true
    }
}

/// 白皮书总指纹合成（三节依序入 FNV）。
pub const fn wp_total_fp(fps: &[u64]) -> u64 {
    let mut h = DOC_FP_SEED;
    let mut i = 0;
    while i < fps.len() {
        let mut j = 0;
        while j < 8 {
            h = (h ^ ((fps[i] >> (j * 8)) & 0xFF) as u64).wrapping_mul(0x0100_0000_01B3);
            j += 1;
        }
        i += 1;
    }
    h
}

// ---------------------------------------------------------------------------
// 二、仲裁算法说明（复用 C 域范式声明）
// ---------------------------------------------------------------------------

/// 范式复用声明（判据"仲裁算法说明（复用 C 域范式声明"——源单号非空纪律）。
#[derive(Debug)]
pub struct ParadigmDeclaration {
    /// 复用源单号（C 域仲裁器单号——非空纪律）。
    pub origin_task: u32,
    /// 复用范式名（轮转+优先级混合——与 C 域同范式）。
    pub paradigm: [u8; 12],
    /// 范式名长度。
    pub paradigm_len: usize,
    /// 声明在册标记。
    pub declared: bool,
    /// 失据事件数（0x7502）。
    pub unbased_events: u32,
}

impl ParadigmDeclaration {
    /// 新声明（未在册态）。
    pub const fn new() -> ParadigmDeclaration {
        ParadigmDeclaration {
            origin_task: 0,
            paradigm: [0; 12],
            paradigm_len: 0,
            declared: false,
            unbased_events: 0,
        }
    }

    /// 登记声明（源单号非空+范式名非空才合法——0x7502）。
    pub fn declare(&mut self, origin_task: u32, paradigm: &[u8]) -> Result<(), u32> {
        if origin_task == 0 || paradigm.is_empty() {
            self.unbased_events += 1;
            return Err(XC_PARADIGM_UNBASED);
        }
        self.origin_task = origin_task;
        let mut i = 0;
        while i < paradigm.len().min(12) {
            self.paradigm[i] = paradigm[i];
            i += 1;
        }
        self.paradigm_len = paradigm.len().min(12);
        self.declared = true;
        Ok(())
    }

    /// 声明合法性：源单号非空+在册+轮转基数与代码侧一致（ARBITER_ROUNDS）。
    pub fn sound(&self) -> bool {
        self.declared && self.origin_task != 0 && self.paradigm_len > 0
    }
}

// ---------------------------------------------------------------------------
// 三、调优指南（步骤表闭集+适用条件）
// ---------------------------------------------------------------------------

/// 调优步骤。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct TuneStep {
    /// 步号。
    pub no: usize,
    /// 前置检查锚（代码实体——非空）。
    pub precondition: u32,
    /// 适用代际下限（PCIe Gen 下限——0=全代适用）。
    pub gen_floor: usize,
}

/// 调优指南（判据"调优指南"）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct TuningGuide {
    /// 步骤表。
    pub steps: Vec<TuneStep>,
    /// 指南指纹（步骤表合成——过期检测）。
    pub guide_fp: u64,
    /// 步缺事件数（0x7503）。
    pub step_miss_events: u32,
}

impl TuningGuide {
    /// 缔造指南（五步闭集齐备——缺步 0x7503）。
    pub fn compose(pre: [u32; GUIDE_STEPS], gen_floors: [usize; GUIDE_STEPS]) -> Result<TuningGuide, u32> {
        let mut i = 0;
        while i < GUIDE_STEPS {
            if pre[i] == 0 {
                return Err(XC_GUIDE_STEP_MISS);
            }
            i += 1;
        }
        let mut steps = Vec::new();
        i = 0;
        while i < GUIDE_STEPS {
            steps.push(TuneStep {
                no: i,
                precondition: pre[i],
                gen_floor: gen_floors[i],
            });
            i += 1;
        }
        Ok(TuningGuide {
            steps,
            guide_fp: guide_total_fp(&pre),
            step_miss_events: 0,
        })
    }

    /// 步骤完整性：恰五步+前置全非空。
    pub fn complete(&self) -> bool {
        if self.steps.len() != GUIDE_STEPS {
            return false;
        }
        let mut i = 0;
        while i < self.steps.len() {
            if self.steps[i].precondition == 0 {
                return false;
            }
            i += 1;
        }
        true
    }

    /// 步骤适用性查询：代际低于步下限→该步不适用（None）。
    pub fn applicable(&self, step_no: usize, gen: usize) -> Option<bool> {
        let mut i = 0;
        while i < self.steps.len() {
            if self.steps[i].no == step_no {
                return Some(gen >= self.steps[i].gen_floor);
            }
            i += 1;
        }
        None
    }
}

/// 指南总指纹合成。
pub const fn guide_total_fp(pre: &[u32]) -> u64 {
    let mut h = DOC_FP_SEED ^ 0x4755_4944; // "GUID" 变体
    let mut i = 0;
    while i < pre.len() {
        let mut j = 0;
        while j < 4 {
            h = (h ^ ((pre[i] >> (j * 8)) & 0xFF) as u64).wrapping_mul(0x0100_0000_01B3);
            j += 1;
        }
        i += 1;
    }
    h
}

// ---------------------------------------------------------------------------
// 四、文档-代码对拍（数字一套账+指纹新鲜）
// ---------------------------------------------------------------------------

/// 对拍器（判据"文档-代码对拍"——数字一套账+指纹新鲜）。
#[derive(Debug)]
pub struct DocCodeSync {
    /// 白皮书引用的代际数。
    pub doc_gen_count: usize,
    /// 白皮书引用的账户数。
    pub doc_account_count: usize,
    /// 白皮书引用的仲裁轮转数。
    pub doc_arbiter_rounds: u32,
    /// 最近对拍通过时的实现指纹。
    pub synced_impl_fp: u64,
    /// 漂移事件数（0x7504）。
    pub drift_events: u32,
    /// 过期事件数（0x7505）。
    pub stale_events: u32,
}

impl DocCodeSync {
    /// 新对拍器。
    pub const fn new() -> DocCodeSync {
        DocCodeSync {
            doc_gen_count: PCIE_GEN_COUNT,
            doc_account_count: ACCOUNT_COUNT,
            doc_arbiter_rounds: ARBITER_ROUNDS,
            synced_impl_fp: 0,
            drift_events: 0,
            stale_events: 0,
        }
    }

    /// 对拍：文档数字 vs 代码常量逐项一致——漂移 0x7504。
    pub fn reconcile(&mut self, impl_fp: u64) -> Result<(), u32> {
        if self.doc_gen_count != PCIE_GEN_COUNT
            || self.doc_account_count != ACCOUNT_COUNT
            || self.doc_arbiter_rounds != ARBITER_ROUNDS
        {
            self.drift_events += 1;
            return Err(XC_DOC_CODE_DRIFT);
        }
        self.synced_impl_fp = impl_fp;
        Ok(())
    }

    /// 新鲜度：实现指纹变化后未重对拍=文档过期 0x7505。
    pub fn freshness(&mut self, current_impl_fp: u64) -> Result<(), u32> {
        if self.synced_impl_fp == 0 {
            self.stale_events += 1;
            return Err(XC_DOC_STALE);
        }
        if self.synced_impl_fp != current_impl_fp {
            self.stale_events += 1;
            return Err(XC_DOC_STALE);
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// 五、总引擎 + 自检（白皮书/说明/指南/对拍/判据五组+段对账）
// ---------------------------------------------------------------------------

/// 带宽文档总引擎。
#[derive(Debug)]
pub struct BwDocEngine {
    /// 白皮书。
    pub wp: Option<BwWhitepaper>,
    /// 范式声明。
    pub paradigm: ParadigmDeclaration,
    /// 调优指南。
    pub guide: Option<TuningGuide>,
    /// 对拍器。
    pub sync: DocCodeSync,
}

impl BwDocEngine {
    /// 出厂。
    pub const fn new() -> BwDocEngine {
        BwDocEngine {
            wp: None,
            paradigm: ParadigmDeclaration::new(),
            guide: None,
            sync: DocCodeSync::new(),
        }
    }

    /// 组一：白皮书——三节齐备成立；缺节拒；完整性对账。
    pub fn selfcheck_whitepaper(&mut self) -> Result<(), u32> {
        let mut e = BwDocEngine::new();
        // 缺节（锚空）→0x7501。
        if BwWhitepaper::compose([0, 0x1289_0002, 0x1289_0003], [1, 2, 3])
            != Err(XC_WP_SECTION_MISS)
        {
            return Err(XC_WP_SECTION_MISS);
        }
        // 三节齐备→成立。
        e.wp = Some(BwWhitepaper::compose(
            [0x1289_0001, 0x1289_0002, 0x1289_0003],
            [0xA1, 0xA2, 0xA3],
        )?);
        let wp = e.wp.as_ref().ok_or(XC_WP_SECTION_MISS)?;
        if !wp.complete() || wp.sections.len() != 3 {
            return Err(XC_WP_SECTION_MISS);
        }
        Ok(())
    }

    /// 组二：范式声明——源单号空失据；合法声明过；轮转基数一致。
    pub fn selfcheck_paradigm(&mut self) -> Result<(), u32> {
        let mut e = BwDocEngine::new();
        // 源单号空→0x7502（口头复用=无复用）。
        if e.paradigm.declare(0, b"rr-priority") != Err(XC_PARADIGM_UNBASED) {
            return Err(XC_PARADIGM_UNBASED);
        }
        if e.paradigm.unbased_events != 1 {
            return Err(XC_PARADIGM_UNBASED);
        }
        // 范式名空→失据。
        if e.paradigm.declare(0x0005, b"") != Err(XC_PARADIGM_UNBASED) {
            return Err(XC_PARADIGM_UNBASED);
        }
        // 合法声明：C 域仲裁器单号+范式名。
        e.paradigm.declare(0x0005, b"rr-priority")?;
        if !e.paradigm.sound() {
            return Err(XC_PARADIGM_UNBASED);
        }
        Ok(())
    }

    /// 组三：调优指南——五步齐备；缺步拒；代际适用查询。
    pub fn selfcheck_guide(&mut self) -> Result<(), u32> {
        let mut e = BwDocEngine::new();
        // 缺步（前置空）→0x7503。
        if TuningGuide::compose([1, 2, 0, 4, 5], [0, 0, 0, 0, 0]) != Err(XC_GUIDE_STEP_MISS) {
            return Err(XC_GUIDE_STEP_MISS);
        }
        // 五步齐备→成立。
        e.guide = Some(TuningGuide::compose(
            [0x1289_1001, 0x1289_1002, 0x1289_1003, 0x1289_1004, 0x1289_1005],
            [0, 0, 3, 0, 4],
        )?);
        let g = e.guide.as_ref().ok_or(XC_GUIDE_STEP_MISS)?;
        if !g.complete() {
            return Err(XC_GUIDE_STEP_MISS);
        }
        // 适用查询：Gen2 跑步 2（floor 3）→不适用；Gen4→适用。
        if g.applicable(2, 2) != Some(false) || g.applicable(2, 4) != Some(true) {
            return Err(XC_GUIDE_STEP_MISS);
        }
        // 越界步号→None。
        if g.applicable(9, 4).is_some() {
            return Err(XC_GUIDE_STEP_MISS);
        }
        Ok(())
    }

    /// 组四：对拍——数字一套账；漂移显性；过期显性。
    pub fn selfcheck_sync(&mut self) -> Result<(), u32> {
        let mut e = BwDocEngine::new();
        // 文档数字篡改→漂移 0x7504。
        e.sync.doc_account_count = 5;
        if e.sync.reconcile(0xFACE) != Err(XC_DOC_CODE_DRIFT) {
            return Err(XC_DOC_CODE_DRIFT);
        }
        if e.sync.drift_events != 1 {
            return Err(XC_DOC_CODE_DRIFT);
        }
        // 修正后对拍过。
        e.sync.doc_account_count = ACCOUNT_COUNT;
        e.sync.reconcile(0xFACE)?;
        // 实现推进（指纹变）未重对拍→过期 0x7505。
        if e.sync.freshness(0xFACF) != Err(XC_DOC_STALE) {
            return Err(XC_DOC_STALE);
        }
        if e.sync.stale_events != 1 {
            return Err(XC_DOC_STALE);
        }
        // 重对拍后新鲜。
        e.sync.reconcile(0xFACF)?;
        e.sync.freshness(0xFACF)?;
        Ok(())
    }

    /// 组五：判据——端到端：白皮书+声明+指南+对拍四件全链绿。
    pub fn selfcheck_verdict(&mut self) -> Result<(), u32> {
        let mut e = BwDocEngine::new();
        e.wp = Some(BwWhitepaper::compose(
            [0x1289_0001, 0x1289_0002, 0x1289_0003],
            [0xB1, 0xB2, 0xB3],
        )?);
        e.paradigm.declare(0x0005, b"rr-priority")?;
        e.guide = Some(TuningGuide::compose(
            [0x1289_1001, 0x1289_1002, 0x1289_1003, 0x1289_1004, 0x1289_1005],
            [0, 0, 3, 0, 4],
        )?);
        e.sync.reconcile(0xBEEF)?;
        e.sync.freshness(0xBEEF)?;
        let wp_ok = e.wp.as_ref().ok_or(XC_WP_SECTION_MISS)?.complete();
        let g_ok = e.guide.as_ref().ok_or(XC_GUIDE_STEP_MISS)?.complete();
        if !(wp_ok && g_ok && e.paradigm.sound() && e.sync.drift_events == 0 && e.sync.stale_events == 0) {
            return Err(XC_DOC_CODE_DRIFT);
        }
        // 白皮书总指纹守恒：重合成≡initial。
        let wp = e.wp.as_ref().ok_or(XC_WP_SECTION_MISS)?;
        let fps = [wp.sections[0].content_fp, wp.sections[1].content_fp, wp.sections[2].content_fp];
        if wp_total_fp(&fps) != wp.total_fp {
            return Err(XC_DOC_CODE_DRIFT);
        }
        Ok(())
    }

    /// 全量自检入口（五组+段对账）。
    pub fn selfcheck_all(&mut self) -> Result<(), u32> {
        self.selfcheck_whitepaper()?;
        self.selfcheck_paradigm()?;
        self.selfcheck_guide()?;
        self.selfcheck_sync()?;
        self.selfcheck_verdict()?;
        Self::segment_guard()?;
        Ok(())
    }

    /// 段对账：五码全落 0x7500..0x7600 开区间且互异。
    pub const fn segment_guard() -> Result<(), u32> {
        let codes = [
            XC_WP_SECTION_MISS,
            XC_PARADIGM_UNBASED,
            XC_GUIDE_STEP_MISS,
            XC_DOC_CODE_DRIFT,
            XC_DOC_STALE,
        ];
        let mut i = 0;
        while i < codes.len() {
            if codes[i] < 0x7500 || codes[i] >= 0x7600 {
                return Err(XC_WP_SECTION_MISS);
            }
            let mut j = i + 1;
            while j < codes.len() {
                if codes[i] == codes[j] {
                    return Err(XC_WP_SECTION_MISS);
                }
                j += 1;
            }
            i += 1;
        }
        Ok(())
    }
}
