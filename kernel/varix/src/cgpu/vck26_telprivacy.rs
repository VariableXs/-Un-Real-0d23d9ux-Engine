//! CGPU-F1707 · 异构遥测隐私
//!
//! 判据锚点（施工书原文摘录）：
//! 隐私：分类（异构数据三级分类复用——分类表（模型名是否敏感（模型清单可算
//! 指纹——指纹评估（模型名本地化——本地化决定；控制（面板/导出/删除复用——
//! 控制复用；匿名（上传匿名复用；测试（分类/指纹/控制/匿名四组）。判据：
//! 分类表、指纹评估、控制、匿名、四组、判据。
//!
//! 诊断码段 0x9201~0x9205（独占）：
//!   0x9201 分类越表（数据项未分类即出流/级别越三级闭集）
//!   0x9202 指纹评估失职（模型名出流而未过指纹评估）
//!   0x9203 控制三闸失守（面板/导出/删除任一失控）
//!   0x9204 匿名化破约（上传流携带可识别指纹/匿名声明与实况不符）
//!   0x9205 段越界（判码落 0x9200..0x9300 区间外）
//!
//! 纯功能：no_std+alloc；零 unwrap/expect；全部自检在 selfcheck_* 组内。

use alloc::vec::Vec;

/// 模块版本。
pub const TEL_PRIVACY_VERSION: &str = "K26-telprivacy-v1";

// ---------------------------------------------------------------------------
// 判码（段内独占 0x9201~0x9205）
// ---------------------------------------------------------------------------

pub const XC_CLASS_OOB: u32 = 0x9201;
pub const XC_FP_ASSESS_SKIPPED: u32 = 0x9202;
pub const XC_CONTROL_BREACH: u32 = 0x9203;
pub const XC_ANON_BROKEN: u32 = 0x9204;
pub const XC_SEG_OOR: u32 = 0x9205;

// ---------------------------------------------------------------------------
// 一、三级分类与分类表
// ---------------------------------------------------------------------------

/// 敏感级三闭集。
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum SensLevel {
    /// 公开：聚合统计（无个体可辨）。
    Public,
    /// 内部：引擎/任务级运行量（不出设备）。
    Internal,
    /// 敏感：模型名/负载画像（出流前必须指纹评估+匿名化）。
    Sensitive,
}

impl SensLevel {
    pub const ALL: [SensLevel; 3] =
        [SensLevel::Public, SensLevel::Internal, SensLevel::Sensitive];
    pub const fn name(self) -> &'static str {
        match self {
            SensLevel::Public => "public",
            SensLevel::Internal => "internal",
            SensLevel::Sensitive => "sensitive",
        }
    }
}

/// 分类表：遥测数据项 → 敏感级（复用三级分类——表外项出流即 0x9201）。
pub struct ClassBook {
    rows: [(&'static str, SensLevel); 6],
}

impl ClassBook {
    pub const fn build() -> Self {
        ClassBook {
            rows: [
                ("fps_per_watt_bp", SensLevel::Public),
                ("engine_utili_permille", SensLevel::Internal),
                ("handoff_overhead_us", SensLevel::Internal),
                ("queue_depth", SensLevel::Internal),
                ("model_name", SensLevel::Sensitive),
                ("workload_profile", SensLevel::Sensitive),
            ],
        }
    }

    /// 查级。
    pub fn level_of(&self, item: &str) -> Option<SensLevel> {
        let mut found = None;
        let mut i = 0;
        while i < self.rows.len() {
            if self.rows[i].0 == item {
                found = Some(self.rows[i].1);
            }
            i += 1;
        }
        found
    }

    /// 出流闸：表外项拒；Sensitive 项必须持放行票（指纹评估+匿名化完成）。
    pub fn emit_gate(&self, item: &str, clearance: u8) -> Result<(), u32> {
        let lv = match self.level_of(item) {
            Some(l) => l,
            None => return Err(XC_CLASS_OOB),
        };
        match lv {
            SensLevel::Public => Ok(()),
            SensLevel::Internal => {
                if clearance >= 1 {
                    Ok(())
                } else {
                    Err(XC_CLASS_OOB)
                }
            }
            SensLevel::Sensitive => {
                if clearance >= 2 {
                    Ok(())
                } else {
                    Err(XC_FP_ASSESS_SKIPPED)
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// 二、模型名指纹评估 → 本地化决定
// ---------------------------------------------------------------------------

/// 指纹评估结论三闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FpVerdict {
    /// 低风险：聚合名（家族级）不可定位具体模型。
    Low,
    /// 可指纹：模型清单交集可定位部署（模型名本地化）。
    Fingerprintable,
}

/// 评估器：模型名本地化决定（判据"模型清单可算指纹——指纹评估（模型名本地化"）。
pub struct ModelFpAssessor {
    /// 已评估模型名账（评估过才有出流资格）。
    assessed: Vec<(&'static str, FpVerdict)>,
    /// 本地化名单（Fingerprintable 项必须本地化=不出流原文名）。
    localized: Vec<&'static str>,
    /// 评估跳过事件数（审计）。
    pub skipped: u32,
}

impl ModelFpAssessor {
    pub fn new() -> Self {
        ModelFpAssessor { assessed: Vec::new(), localized: Vec::new(), skipped: 0 }
    }

    /// 评估：名单内可定位部署判 Fingerprintable（版本号/量化后缀越多越可定位）。
    pub fn assess(&mut self, name: &'static str, version_suffix: bool, quant_suffix: bool) -> FpVerdict {
        let v = if version_suffix && quant_suffix {
            FpVerdict::Fingerprintable
        } else {
            FpVerdict::Low
        };
        self.assessed.push((name, v));
        v
    }

    /// 本地化决定：Fingerprintable 项必须入本地化名单。
    pub fn localize(&mut self, name: &'static str) -> Result<(), u32> {
        let mut ok = false;
        for (n, v) in &self.assessed {
            if *n == name && *v == FpVerdict::Fingerprintable {
                ok = true;
            }
        }
        if !ok {
            self.skipped += 1;
            return Err(XC_FP_ASSESS_SKIPPED);
        }
        let mut dup = false;
        for l in &self.localized {
            if *l == name {
                dup = true;
            }
        }
        if !dup {
            self.localized.push(name);
        }
        Ok(())
    }

    /// 出流资格：模型名必须在本地化名单（=已评估+已本地化）。
    pub fn may_emit(&mut self, name: &str) -> Result<(), u32> {
        for l in &self.localized {
            if *l == name {
                return Ok(());
            }
        }
        self.skipped += 1;
        Err(XC_FP_ASSESS_SKIPPED)
    }

    /// 已评估数。
    pub fn assessed_count(&self) -> usize {
        self.assessed.len()
    }
}

// ---------------------------------------------------------------------------
// 三、控制三闸（面板/导出/删除 复用）
// ---------------------------------------------------------------------------

/// 控制闸三闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Ctrl {
    /// 面板显示控制。
    Panel,
    /// 导出控制。
    Export,
    /// 删除控制。
    Delete,
}

/// 控制账：三闸各有开关+审计计数。
pub struct ControlBook {
    pub panel_on: bool,
    pub export_on: bool,
    pub delete_done: bool,
    pub panel_events: u32,
    pub export_events: u32,
    pub delete_events: u32,
}

impl ControlBook {
    pub fn new() -> Self {
        ControlBook {
            panel_on: true,
            export_on: false, // 导出默认关（复用三闸默认保守）。
            delete_done: false,
            panel_events: 0,
            export_events: 0,
            delete_events: 0,
        }
    }

    /// 面板闸。
    pub fn panel(&mut self) -> Result<(), u32> {
        if !self.panel_on {
            return Err(XC_CONTROL_BREACH);
        }
        self.panel_events += 1;
        Ok(())
    }

    /// 导出闸（默认关，显性开才放行）。
    pub fn export(&mut self) -> Result<(), u32> {
        if !self.export_on {
            return Err(XC_CONTROL_BREACH);
        }
        self.export_events += 1;
        Ok(())
    }

    /// 删除闸：执行删除登记（幂等——重复删不重复计数）。
    pub fn delete(&mut self) -> Result<(), u32> {
        if self.delete_done {
            return Ok(());
        }
        self.delete_done = true;
        self.delete_events += 1;
        Ok(())
    }

    /// 三闸账平：导出从未未授权放行（events>0 必须 export_on 曾开——此处以
    /// 开关在册为准）+ 删除幂等不重复计。
    pub fn audited(&self) -> Result<(), u32> {
        if !self.export_on && self.export_events > 0 {
            return Err(XC_CONTROL_BREACH);
        }
        if self.delete_done && self.delete_events > 1 {
            return Err(XC_CONTROL_BREACH);
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// 四、上传匿名
// ---------------------------------------------------------------------------

/// FNV-1a 64。
pub const fn fnv64(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf_29ce_4842_22325;
    let mut i = 0;
    while i < bytes.len() {
        h = (h ^ bytes[i] as u64).wrapping_mul(0x100_0000_01b3);
        i += 1;
    }
    h
}

/// 上传字段白名单（匿名化=白名单外字段剥离）。
pub const UPLOAD_WHITELIST: [&str; 4] = ["fps_per_watt_bp", "engine_utili_permille", "queue_depth", "handoff_overhead_us"];

/// 匿名化上传闸：白名单外字段（含 model_name 等敏感项）一律剥离。
/// 入参为 'static 字段名闭集（调用面全是字面量），输出同寿命——零 unsafe。
pub fn anon_upload(fields: &[&'static str]) -> Result<Vec<&'static str>, u32> {
    let mut out = Vec::new();
    for f in fields {
        let mut ok = false;
        let mut i = 0;
        while i < UPLOAD_WHITELIST.len() {
            if UPLOAD_WHITELIST[i] == *f {
                ok = true;
            }
            i += 1;
        }
        if ok {
            out.push(*f);
        }
    }
    Ok(out)
}

// ---------------------------------------------------------------------------
// 自检（四组：分类/指纹/控制/匿名）
// ---------------------------------------------------------------------------

/// 隐私引擎。
pub struct PrivacyEngine {
    pub book: ClassBook,
    pub assessor: ModelFpAssessor,
    pub ctrl: ControlBook,
}

impl PrivacyEngine {
    pub fn new() -> Self {
        PrivacyEngine {
            book: ClassBook::build(),
            assessor: ModelFpAssessor::new(),
            ctrl: ControlBook::new(),
        }
    }

    /// 组一：分类——三级闭集 + 表外拒 + 级别闸（Public 直过/Internal 需票/
    /// Sensitive 无票拒）。
    pub fn selfcheck_class(&mut self) -> Result<(), u32> {
        self.book.emit_gate("fps_per_watt_bp", 0)?;
        self.book.emit_gate("engine_utili_permille", 1)?;
        // Sensitive 无票拒（指纹评估跳过）。
        if self.book.emit_gate("model_name", 1).is_ok() {
            return Err(XC_FP_ASSESS_SKIPPED);
        }
        // 表外项拒。
        if self.book.emit_gate("ghost_item", 2).is_ok() {
            return Err(XC_CLASS_OOB);
        }
        Ok(())
    }

    /// 组二：指纹评估——版本+量化后缀判可指纹 → 本地化 → 出流资格 + 跳评估拒。
    pub fn selfcheck_fp(&mut self) -> Result<(), u32> {
        // 低风险：无版本无量化。
        let v1 = self.assessor.assess("family-only", false, false);
        if v1 != FpVerdict::Low {
            return Err(XC_FP_ASSESS_SKIPPED);
        }
        // 可指纹：版本+量化。
        let v2 = self.assessor.assess("bigmodel-v3-int8", true, true);
        if v2 != FpVerdict::Fingerprintable {
            return Err(XC_FP_ASSESS_SKIPPED);
        }
        // 可指纹项必须本地化才有出流资格。
        if self.assessor.may_emit("bigmodel-v3-int8").is_ok() {
            return Err(XC_FP_ASSESS_SKIPPED);
        }
        self.assessor.localize("bigmodel-v3-int8")?;
        self.assessor.may_emit("bigmodel-v3-int8")?;
        // 未评估项出流拒。
        if self.assessor.may_emit("never-assessed").is_ok() {
            return Err(XC_FP_ASSESS_SKIPPED);
        }
        // 本地化未评估项拒。
        if self.assessor.localize("ghost").is_err() {
            // 期望路径：ghost 未评估 → localize 拒（skip 计数增加）。
        } else {
            return Err(XC_FP_ASSESS_SKIPPED);
        }
        if self.assessor.assessed_count() < 2 {
            return Err(XC_FP_ASSESS_SKIPPED);
        }
        Ok(())
    }

    /// 组三：控制——面板默认开/导出默认关/删除幂等 + 账平审计。
    pub fn selfcheck_control(&mut self) -> Result<(), u32> {
        self.ctrl.panel()?;
        // 导出默认关：直接导出拒。
        if self.ctrl.export().is_ok() {
            return Err(XC_CONTROL_BREACH);
        }
        self.ctrl.export_on = true;
        self.ctrl.export()?;
        self.ctrl.export_on = false;
        // 关后再导出拒（events 不增——未授权放行审计拦）。
        if self.ctrl.export().is_ok() {
            return Err(XC_CONTROL_BREACH);
        }
        // 删除幂等。
        self.ctrl.delete()?;
        self.ctrl.delete()?;
        if self.ctrl.delete_events != 1 {
            return Err(XC_CONTROL_BREACH);
        }
        self.ctrl.audited()?;
        Ok(())
    }

    /// 组四：匿名——白名单过闸 + 敏感项剥离 + 空流合法。
    pub fn selfcheck_anon(&mut self) -> Result<(), u32> {
        let fields = ["fps_per_watt_bp", "model_name", "queue_depth"];
        let out = anon_upload(&fields)?;
        if out.len() != 2 {
            return Err(XC_ANON_BROKEN);
        }
        // 剥离后不得含敏感项。
        for f in &out {
            if *f == "model_name" || *f == "workload_profile" {
                return Err(XC_ANON_BROKEN);
            }
        }
        // 全敏感流 → 空流合法（全部剥离=可上传空包）。
        let all_sens = ["model_name", "workload_profile"];
        let out2 = anon_upload(&all_sens)?;
        if !out2.is_empty() {
            return Err(XC_ANON_BROKEN);
        }
        Ok(())
    }

    /// 全量自检入口（四组+段对账）。
    pub fn selfcheck_all(&mut self) -> Result<(), u32> {
        Self::segment_guard()?;
        self.selfcheck_class()?;
        self.selfcheck_fp()?;
        self.selfcheck_control()?;
        self.selfcheck_anon()?;
        Ok(())
    }

    /// 段对账：五码全落 0x9200..0x9300 开区间且互异。
    pub const fn segment_guard() -> Result<(), u32> {
        let codes = [
            XC_CLASS_OOB,
            XC_FP_ASSESS_SKIPPED,
            XC_CONTROL_BREACH,
            XC_ANON_BROKEN,
            XC_SEG_OOR,
        ];
        let mut i = 0;
        while i < codes.len() {
            if codes[i] < 0x9200 || codes[i] >= 0x9300 {
                return Err(XC_CLASS_OOB);
            }
            let mut j = i + 1;
            while j < codes.len() {
                if codes[i] == codes[j] {
                    return Err(XC_CLASS_OOB);
                }
                j += 1;
            }
            i += 1;
        }
        Ok(())
    }
}
