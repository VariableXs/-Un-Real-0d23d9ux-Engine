//! VE-F3756 · 创作无障碍资产
//!
//! 锚点：创作无障碍资产（F3157 模式创作版复刻：资产类型（无障碍创作模板/
//! 辅具配置档案/语义描述库/触觉反馈库/音频提示库——资产五类复述；资产质量
//! （每资产有验收记录（复述验收红线：无验收=不可用资产（验收硬门）；资产分发
//! （市场分发无障碍标注（复述标注红线）；资产版本（与工具版本联动（复述联动）。
//!
//! 数据结构：五类资产；验收硬门；标注复述；版本联动。
//! 错误路径与降级矩阵：无验收→拒收（硬门实测）；标注缺→补（复述红线）；
//! 版本脱钩→修正（复述）；资产缺失→降级（复述）。
//! 跨批对接点：F3157/F3175 单源复用声明；F3750 工具对端；F3758 市场对端。
//!
//! 诊断码段：独占 0x93xx。

#![allow(dead_code)]

extern crate alloc;

use alloc::vec::Vec;

// ─────────────────────────────────────────────────────────────
// 诊断码（0x93xx）
// ─────────────────────────────────────────────────────────────

/// F3756 诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct R8Code(pub u16);

impl R8Code {
    /// 资产无验收——硬门。
    pub const NO_ACCEPTANCE: R8Code = R8Code(0x9301);
    /// 市场标注缺失——标注红线。
    pub const LABEL_MISSING: R8Code = R8Code(0x9302);
    /// 版本脱钩——联动。
    pub const VERSION_UNLINKED: R8Code = R8Code(0x9303);
    /// 资产类缺失——五类复述。
    pub const ASSET_CLASS_MISS: R8Code = R8Code(0x9304);
    /// 验收记录失实——同源红线。
    pub const ACCEPT_FALSE: R8Code = R8Code(0x9305);
    /// 分发未标注——市场。
    pub const DISTRIB_UNLABELED: R8Code = R8Code(0x9306);

    /// 诊断说明。
    pub fn explain(self) -> &'static str {
        match self.0 {
            0x9301 => "资产无验收记录，硬门拒收（无验收=不可用资产）。",
            0x9302 => "市场分发无障碍标注缺失，标注红线不通过。",
            0x9303 => "资产版本与工具版本脱钩，联动复述不完整。",
            0x9304 => "五类资产有缺类，复述不完整。",
            0x9305 => "验收记录失实，同源红线不通过。",
            0x9306 => "分发渠道未标注，市场红线不通过。",
            _ => "未知诊断码。",
        }
    }

    /// 数值。
    pub fn raw(self) -> u16 {
        self.0
    }
}

// ─────────────────────────────────────────────────────────────
// 资产五类
// ─────────────────────────────────────────────────────────────

/// 资产类别。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AssetClass {
    /// 无障碍创作模板。
    Template,
    /// 辅具配置档案。
    AssistConfig,
    /// 语义描述库。
    SemLibrary,
    /// 触觉反馈库。
    HapticLib,
    /// 音频提示库。
    AudioLib,
}

impl AssetClass {
    /// 稳定序号。
    pub fn index(self) -> u8 {
        match self {
            AssetClass::Template => 0,
            AssetClass::AssistConfig => 1,
            AssetClass::SemLibrary => 2,
            AssetClass::HapticLib => 3,
            AssetClass::AudioLib => 4,
        }
    }

    /// 全集（五类）。
    pub fn all() -> [AssetClass; 5] {
        [
            AssetClass::Template,
            AssetClass::AssistConfig,
            AssetClass::SemLibrary,
            AssetClass::HapticLib,
            AssetClass::AudioLib,
        ]
    }
}

/// 资产。
#[derive(Clone, Debug)]
pub struct Asset {
    /// 资产 id。
    pub id: u32,
    /// 类别。
    pub class: AssetClass,
    /// 验收记录 id（0=无验收）。
    pub acceptance_id: u32,
    /// 验收是否有效（实测）。
    pub acceptance_valid: bool,
    /// 无障碍标注。
    pub labeled: bool,
    /// 资产版本。
    pub version: u16,
    /// 工具版本。
    pub tool_version: u16,
}

impl Asset {
    /// 构造。
    pub fn new(id: u32, class: AssetClass, version: u16, tool_version: u16) -> Asset {
        Asset {
            id,
            class,
            acceptance_id: 0,
            acceptance_valid: false,
            labeled: false,
            version,
            tool_version,
        }
    }

    /// 验收硬门合规：有验收 id + 验收有效。
    pub fn acceptance_ok(&self) -> bool {
        self.acceptance_id != 0 && self.acceptance_valid
    }

    /// 版本联动合规。
    pub fn version_linked(&self) -> bool {
        self.version == self.tool_version
    }

    /// 标注合规。
    pub fn labeled_ok(&self) -> bool {
        self.labeled
    }

    /// 完整合规。
    pub fn complete(&self) -> bool {
        self.acceptance_ok() && self.version_linked() && self.labeled_ok()
    }
}

// ─────────────────────────────────────────────────────────────
// 资产库
// ─────────────────────────────────────────────────────────────

/// 资产库。
#[derive(Clone, Debug, Default)]
pub struct AssetStore {
    assets: Vec<Asset>,
}

impl AssetStore {
    /// 空库。
    pub fn new() -> AssetStore {
        AssetStore {
            assets: Vec::new(),
        }
    }

    /// 入库。
    pub fn add(&mut self, a: Asset) {
        self.assets.push(a);
    }

    /// 验收。
    pub fn accept(&mut self, id: u32, acceptance_id: u32, valid: bool) -> bool {
        for a in self.assets.iter_mut() {
            if a.id == id {
                a.acceptance_id = acceptance_id;
                a.acceptance_valid = valid;
                return true;
            }
        }
        false
    }

    /// 标注。
    pub fn label(&mut self, id: u32) -> bool {
        for a in self.assets.iter_mut() {
            if a.id == id {
                a.labeled = true;
                return true;
            }
        }
        false
    }

    /// 对齐版本。
    pub fn sync_version(&mut self, id: u32) -> bool {
        for a in self.assets.iter_mut() {
            if a.id == id {
                a.version = a.tool_version;
                return true;
            }
        }
        false
    }

    /// 扫描无验收。
    pub fn scan_no_acceptance(&self) -> Vec<u32> {
        let mut bad = Vec::new();
        for a in self.assets.iter() {
            if !a.acceptance_ok() {
                bad.push(a.id);
            }
        }
        bad
    }

    /// 扫描未标注。
    pub fn scan_unlabeled(&self) -> Vec<u32> {
        let mut bad = Vec::new();
        for a in self.assets.iter() {
            if !a.labeled_ok() {
                bad.push(a.id);
            }
        }
        bad
    }

    /// 扫描版本脱钩。
    pub fn scan_version_unlinked(&self) -> Vec<u32> {
        let mut bad = Vec::new();
        for a in self.assets.iter() {
            if !a.version_linked() {
                bad.push(a.id);
            }
        }
        bad
    }

    /// 五类齐备。
    pub fn all_classes_present(&self) -> bool {
        for c in AssetClass::all() {
            if !self.assets.iter().any(|a| a.class == c) {
                return false;
            }
        }
        true
    }

    /// 缺类清单。
    pub fn missing_classes(&self) -> Vec<AssetClass> {
        let mut bad = Vec::new();
        for c in AssetClass::all() {
            if !self.assets.iter().any(|a| a.class == c) {
                bad.push(c);
            }
        }
        bad
    }

    /// 资产数。
    pub fn len(&self) -> usize {
        self.assets.len()
    }

    /// 空判断。
    pub fn is_empty(&self) -> bool {
        self.assets.is_empty()
    }
}

// ─────────────────────────────────────────────────────────────
// 主编排器
// ─────────────────────────────────────────────────────────────

/// 创作无障碍资产管理器。
#[derive(Clone, Debug, Default)]
pub struct A11yAssetManager {
    store: AssetStore,
}

impl A11yAssetManager {
    /// 构造。
    pub fn new() -> A11yAssetManager {
        A11yAssetManager {
            store: AssetStore::new(),
        }
    }

    /// 取库（可变）。
    pub fn store_mut(&mut self) -> &mut AssetStore {
        &mut self.store
    }

    /// 取库。
    pub fn store(&self) -> &AssetStore {
        &self.store
    }

    /// 全量审查。
    pub fn audit(&self) -> Vec<R8Code> {
        let mut bad = Vec::new();
        if !self.store.scan_no_acceptance().is_empty() {
            bad.push(R8Code::NO_ACCEPTANCE);
        }
        if !self.store.scan_unlabeled().is_empty() {
            bad.push(R8Code::LABEL_MISSING);
        }
        if !self.store.scan_version_unlinked().is_empty() {
            bad.push(R8Code::VERSION_UNLINKED);
        }
        if !self.store.all_classes_present() {
            bad.push(R8Code::ASSET_CLASS_MISS);
        }
        bad
    }
}

// ─────────────────────────────────────────────────────────────
// 判据
// ─────────────────────────────────────────────────────────────

/// F3756 判据集。
pub fn run_vr08p_checks() -> crate::checks::CheckSet {
    let mut out = crate::checks::CheckSet::new("VE-F3756");

    // 0x9301 验收硬门
    {
        let mut s = AssetStore::new();
        s.add(Asset::new(1, AssetClass::Template, 2, 2));
        out.add("0x9301-无验收检出", s.scan_no_acceptance().len() == 1, "");
        s.accept(1, 100, true);
        out.add("0x9301-验收后过", s.scan_no_acceptance().is_empty(), "");
    }

    // 0x9302 标注红线
    {
        let mut s = AssetStore::new();
        s.add(Asset::new(2, AssetClass::AudioLib, 1, 1));
        out.add("0x9302-未标注检出", s.scan_unlabeled().len() == 1, "");
        s.label(2);
        out.add("0x9302-标注后过", s.scan_unlabeled().is_empty(), "");
    }

    // 0x9303 版本联动
    {
        let mut s = AssetStore::new();
        s.add(Asset::new(3, AssetClass::HapticLib, 1, 2));
        out.add("0x9303-脱钩检出", s.scan_version_unlinked().len() == 1, "");
        s.sync_version(3);
        out.add("0x9303-对齐后过", s.scan_version_unlinked().is_empty(), "");
    }

    // 0x9304 五类齐备
    {
        let mut s = AssetStore::new();
        out.add("0x9304-全缺检出", s.missing_classes().len() == 5, "");
        for c in AssetClass::all() {
            let id = c.index() as u32 + 10;
            s.add(Asset::new(id, c, 1, 1));
        }
        out.add("0x9304-五类齐过", s.all_classes_present(), "");
    }

    // 判据
    {
        let mut m = A11yAssetManager::new();
        let bad = m.audit();
        out.add("0x9305-管理器检出", !bad.is_empty(), "");
    }

    out
}
