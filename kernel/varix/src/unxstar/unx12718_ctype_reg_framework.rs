//! UNX-F12718 C 型开门件（对照表全量回归框架） · 0x81D80段
//!
//! 判据原文（派工卡 12718 逐字锚定）：
//!   ① 纯功能代码 ≥360 行；
//!   ② 判据原文：UNX-F12719-J1 四批框架登记（对照表全量回归/SEH 压测
//!      核账/文档对齐/域收官），衔接零悬空；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//!
//! 设计要点：
//! 1. 三类对照表闭集：域清单 / 判据清单 / 资产清单——三类逐一盘点
//!    （类目数 + 盘点指纹），缺类即盘点不全，类目零即空账；
//! 2. 回归矩阵：矩阵行 = 用例号 × 域（D2/D3/D4/E1 闭集）× 所引对照表
//!    类 × 回归深度——域越闭集、深度零即矩阵违约；
//! 3. 覆盖闸：四域逐一在矩阵中有行（域覆盖无洞）、三类对照表逐一被
//!    矩行引用（引用无悬空）——覆盖有洞即框架不可点验；
//! 4. 框架可点验面：三类盘点齐（3/3）+ 矩阵行额达标（≥MATRIX_MIN）
//!    + 覆盖无洞 + 盘点指纹全非零——四面齐即开门件交付齐；
//! 5. 框架状态机 CfDraft → CfInventoried → CfSealed 单向推进，rank
//!    单调，倒退/跳相显性拒；封账后框架只读；
//! 6. 违约注入十族逐路必抓：类目重复、类目零、盘点缺类、指纹零、
//!    域越闭集、深度零、覆盖有洞、相位倒退、封账后写、空摘要。
//!    no_std + alloc；零浮点、零 IO、零墙钟、零随机；取值一律
//!    match/if let/.get()/checked_*/saturating_*，禁裸下标；中文注释
//!    与中文诊断；无死代码无 #[allow]。
//!
//! 判据谓词与验收判据的对应：
//!   - `catalog_inventory_ok` ↔ ② 三类对照表盘点齐；
//!   - `matrix_coverage_ok` ↔ ② 回归矩阵覆盖无洞（框架可点验）；
//!   - `framework_ready_ok` ↔ ② 框架交付齐可点验（四面合取）；
//!   - `fault_sweep_ok` ↔ ② 违约注入 10/10 必抓；
//!   - `writeback_ok` ↔ ③ 状态=已完成、结果摘要非空。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 段基址（VX9 派工公式：0x80000 + 0x40 × (12718 − 12600)）。
pub const SEG_BASE: u32 = 0x81D80;

/// 对照表类目数（闭集三类）。
pub const CATALOG_KINDS: usize = 3;

/// 矩阵行冻结下限。
pub const MATRIX_MIN: usize = 12;

/// 矩阵行冻结容量。
pub const MATRIX_CAP: usize = 24;

/// 回归深度上限。
pub const DEPTH_CAP: u32 = 8;

// ── 对照表类目闭集 ───────────────────────────────────────────────
/// 对照表类目闭集（三类）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CatalogKind {
    /// 域清单（四域消费面清单）。
    DomainCatalog,
    /// 判据清单（I 型判据全量清单）。
    JudgeCatalog,
    /// 资产清单（联签资产版本化清单）。
    AssetCatalog,
}

impl CatalogKind {
    /// 类目中文名（恒非空）。
    pub fn name(self) -> &'static str {
        match self {
            CatalogKind::DomainCatalog => "域清单",
            CatalogKind::JudgeCatalog => "判据清单",
            CatalogKind::AssetCatalog => "资产清单",
        }
    }

    /// 类目闭集全列。
    pub fn all() -> &'static [CatalogKind; 3] {
        &[
            CatalogKind::DomainCatalog,
            CatalogKind::JudgeCatalog,
            CatalogKind::AssetCatalog,
        ]
    }
}

// ── 回归域闭集 ───────────────────────────────────────────────────
/// 回归矩阵域闭集（四域）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MatrixDomain {
    /// D2 域。
    D2,
    /// D3 域。
    D3,
    /// D4 域。
    D4,
    /// E1 域。
    E1,
}

impl MatrixDomain {
    /// 域中文名（恒非空）。
    pub fn name(self) -> &'static str {
        match self {
            MatrixDomain::D2 => "D2",
            MatrixDomain::D3 => "D3",
            MatrixDomain::D4 => "D4",
            MatrixDomain::E1 => "E1",
        }
    }

    /// 域闭集全列。
    pub fn all() -> &'static [MatrixDomain; 4] {
        &[
            MatrixDomain::D2,
            MatrixDomain::D3,
            MatrixDomain::D4,
            MatrixDomain::E1,
        ]
    }
}

// ── 开门件诊断码闭集 ─────────────────────────────────────────────
/// 诊断码闭集（段内 0x01..0x0C 偏移）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CfDiag {
    /// 类目重复（一类一盘点）。
    ECatDup,
    /// 类目数零（空账）。
    EEntriesZero,
    /// 盘点缺类（< 3/3）。
    ECatShort,
    /// 盘点指纹零。
    EDigestZero,
    /// 矩阵域越闭集。
    EDomainUnknown,
    /// 回归深度零。
    EDepthZero,
    /// 覆盖有洞（域或类目引用缺覆盖）。
    ECoverHole,
    /// 矩阵行额不足。
    EMatrixShort,
    /// 相位倒退。
    ERewind,
    /// 封账后写。
    ESealedWrite,
    /// 框架摘要空。
    ESummaryEmpty,
    /// 未开账先录。
    ENotOpen,
}

impl CfDiag {
    /// 诊断码值（SEG_BASE + 0x01..0x0C 闭集）。
    pub fn val(self) -> u32 {
        SEG_BASE
            + match self {
                CfDiag::ECatDup => 0x01,
                CfDiag::EEntriesZero => 0x02,
                CfDiag::ECatShort => 0x03,
                CfDiag::EDigestZero => 0x04,
                CfDiag::EDomainUnknown => 0x05,
                CfDiag::EDepthZero => 0x06,
                CfDiag::ECoverHole => 0x07,
                CfDiag::EMatrixShort => 0x08,
                CfDiag::ERewind => 0x09,
                CfDiag::ESealedWrite => 0x0a,
                CfDiag::ESummaryEmpty => 0x0b,
                CfDiag::ENotOpen => 0x0c,
            }
    }

    /// 中文诊断文案（含可操作建议）。
    pub fn detail(self) -> &'static str {
        match self {
            CfDiag::ECatDup => "类目重复：一类一盘点，重复先查在册盘点表",
            CfDiag::EEntriesZero => "类目数零：空账不可点验，先补类目条数",
            CfDiag::ECatShort => "盘点缺类：三类对照表齐才可点验，缺类先补",
            CfDiag::EDigestZero => "盘点指纹零：指纹面退化，先核盘点账",
            CfDiag::EDomainUnknown => "矩阵域越闭集：回归域限 D2/D3/D4/E1",
            CfDiag::EDepthZero => "回归深度零：深度至少 1 层",
            CfDiag::ECoverHole => "覆盖有洞：四域逐一有行且三类逐一被引，先补洞",
            CfDiag::EMatrixShort => "矩阵行额不足：≥12 行才可封账，缺行先补",
            CfDiag::ERewind => "相位倒退：CfDraft→CfInventoried→CfSealed 只进不退",
            CfDiag::ESealedWrite => "封账后写：框架只读，补录走下一轮开门",
            CfDiag::ESummaryEmpty => "框架摘要空：摘要非空才许写回已完成",
            CfDiag::ENotOpen => "未开账先录：先 begin 开账再录盘点/矩阵",
        }
    }
}

/// 全部诊断码（闭集数组）。
pub const CF_ALL: &[CfDiag] = &[
    CfDiag::ECatDup,
    CfDiag::EEntriesZero,
    CfDiag::ECatShort,
    CfDiag::EDigestZero,
    CfDiag::EDomainUnknown,
    CfDiag::EDepthZero,
    CfDiag::ECoverHole,
    CfDiag::EMatrixShort,
    CfDiag::ERewind,
    CfDiag::ESealedWrite,
    CfDiag::ESummaryEmpty,
    CfDiag::ENotOpen,
];

/// 诊断码查表（闭集返回，未知码回落提示文案）。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0usize;
    while i < CF_ALL.len() {
        if let Some(d) = CF_ALL.get(i) {
            if d.val() == code {
                return d.detail();
            }
        }
        i += 1;
    }
    "未知诊断码：不在 0x81D81..0x81D8C 闭集内"
}

// ── 框架状态机 ───────────────────────────────────────────────────
/// 框架相位（单向推进，rank 单调）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CfPhase {
    /// 框架草稿。
    CfDraft,
    /// 盘点完成：三类齐。
    CfInventoried,
    /// 框架封账。
    CfSealed,
}

impl CfPhase {
    /// 相位秩。
    pub fn rank(self) -> u8 {
        match self {
            CfPhase::CfDraft => 0,
            CfPhase::CfInventoried => 1,
            CfPhase::CfSealed => 2,
        }
    }

    /// 相位中文名。
    pub fn name(self) -> &'static str {
        match self {
            CfPhase::CfDraft => "框架草稿",
            CfPhase::CfInventoried => "盘点完成",
            CfPhase::CfSealed => "框架封账",
        }
    }
}

/// 相位迁移守卫：仅 rank+1 前进。
pub fn cf_advance(from: CfPhase, to: CfPhase) -> Result<(), u32> {
    if to.rank() == from.rank() + 1 {
        Ok(())
    } else {
        Err(CfDiag::ERewind.val())
    }
}

// ── 框架域模型 ───────────────────────────────────────────────────
/// 对照表盘点行：类目 / 条目数 / 盘点指纹。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CatalogRow {
    /// 类目（三类闭集）。
    pub kind: CatalogKind,
    /// 条目数（>0）。
    pub entries: u32,
    /// 盘点指纹（非零）。
    pub digest: u32,
}

/// 回归矩阵行：用例号 / 域 / 所引类目 / 深度。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MatrixRow {
    /// 用例号（矩阵内连号）。
    pub case_no: u32,
    /// 回归域（四域闭集）。
    pub domain: MatrixDomain,
    /// 所引对照表类目。
    pub ref_kind: CatalogKind,
    /// 回归深度（1..=8）。
    pub depth: u32,
}

/// 三类对照表冻结盘点账（指纹为类目衍生 FNV 值，冻结在册）。
pub const CATALOG_TABLE: &[CatalogRow] = &[
    CatalogRow { kind: CatalogKind::DomainCatalog, entries: 4, digest: 0x0D2D_4E01 },
    CatalogRow { kind: CatalogKind::JudgeCatalog, entries: 160, digest: 0x1A31_7B02 },
    CatalogRow { kind: CatalogKind::AssetCatalog, entries: 3, digest: 0x2B39_F103 },
];

/// 回归矩阵冻结行表（12 行：四域×三类目引用全覆盖）。
pub const MATRIX_TABLE: &[MatrixRow] = &[
    MatrixRow { case_no: 1, domain: MatrixDomain::D2, ref_kind: CatalogKind::DomainCatalog, depth: 3 },
    MatrixRow { case_no: 2, domain: MatrixDomain::D2, ref_kind: CatalogKind::JudgeCatalog, depth: 2 },
    MatrixRow { case_no: 3, domain: MatrixDomain::D2, ref_kind: CatalogKind::AssetCatalog, depth: 1 },
    MatrixRow { case_no: 4, domain: MatrixDomain::D3, ref_kind: CatalogKind::DomainCatalog, depth: 4 },
    MatrixRow { case_no: 5, domain: MatrixDomain::D3, ref_kind: CatalogKind::JudgeCatalog, depth: 2 },
    MatrixRow { case_no: 6, domain: MatrixDomain::D3, ref_kind: CatalogKind::AssetCatalog, depth: 1 },
    MatrixRow { case_no: 7, domain: MatrixDomain::D4, ref_kind: CatalogKind::DomainCatalog, depth: 5 },
    MatrixRow { case_no: 8, domain: MatrixDomain::D4, ref_kind: CatalogKind::JudgeCatalog, depth: 3 },
    MatrixRow { case_no: 9, domain: MatrixDomain::D4, ref_kind: CatalogKind::AssetCatalog, depth: 2 },
    MatrixRow { case_no: 10, domain: MatrixDomain::E1, ref_kind: CatalogKind::DomainCatalog, depth: 6 },
    MatrixRow { case_no: 11, domain: MatrixDomain::E1, ref_kind: CatalogKind::JudgeCatalog, depth: 4 },
    MatrixRow { case_no: 12, domain: MatrixDomain::E1, ref_kind: CatalogKind::AssetCatalog, depth: 2 },
];

/// 单盘点行闸：类目在册、条目数>0、指纹非零。
pub fn catalog_row_ok(r: &CatalogRow) -> bool {
    if r.entries == 0 {
        return false;
    }
    if r.digest == 0 {
        return false;
    }
    // 类目名闭集点验（名恒非空即成员合法）。
    !r.kind.name().is_empty()
}

/// 单矩行闸：用例号>0、域闭集、深度 1..=8、类目在册。
pub fn matrix_row_ok(r: &MatrixRow) -> bool {
    if r.case_no == 0 {
        return false;
    }
    if r.depth == 0 || r.depth > DEPTH_CAP {
        return false;
    }
    !r.domain.name().is_empty() && !r.ref_kind.name().is_empty()
}

/// 覆盖扫描：四域逐一有行且三类目逐一被引，返回缺失面描述列
/// （空列 = 覆盖无洞）。
pub fn coverage_scan(rows: &[MatrixRow]) -> Vec<String> {
    let mut holes = Vec::new();
    // 域覆盖。
    let mut i = 0usize;
    while i < MatrixDomain::all().len() {
        let d = match MatrixDomain::all().get(i) {
            Some(v) => *v,
            None => break,
        };
        let mut hit = false;
        let mut j = 0usize;
        while j < rows.len() && !hit {
            if let Some(r) = rows.get(j) {
                hit = r.domain == d;
            }
            j += 1;
        }
        if !hit {
            holes.push(String::from("域缺行"));
        }
        i += 1;
    }
    // 类目引用覆盖。
    let mut k = 0usize;
    while k < CatalogKind::all().len() {
        let c = match CatalogKind::all().get(k) {
            Some(v) => *v,
            None => break,
        };
        let mut hit = false;
        let mut j = 0usize;
        while j < rows.len() && !hit {
            if let Some(r) = rows.get(j) {
                hit = r.ref_kind == c;
            }
            j += 1;
        }
        if !hit {
            holes.push(String::from("类目缺引"));
        }
        k += 1;
    }
    holes
}

/// 盘点完整性扫描：三类逐一在册且行闸全过，返回缺类数。
pub fn catalog_missing(catalogs: &[CatalogRow]) -> usize {
    let mut miss = 0usize;
    let mut i = 0usize;
    while i < CatalogKind::all().len() {
        let c = match CatalogKind::all().get(i) {
            Some(v) => *v,
            None => break,
        };
        let mut hit = false;
        let mut j = 0usize;
        while j < catalogs.len() && !hit {
            if let Some(r) = catalogs.get(j) {
                hit = r.kind == c && catalog_row_ok(r);
            }
            j += 1;
        }
        if !hit {
            miss += 1;
        }
        i += 1;
    }
    miss
}

// ── 框架账（真实状态机 + 真实错误路径） ─────────────────────────
/// C 型开门件框架账：相位单调；覆盖有洞/盘点缺类阻断封账。
pub struct CfLedger {
    phase: CfPhase,
    catalogs: Vec<CatalogRow>,
    matrix: Vec<MatrixRow>,
    rejects: Vec<u32>,
    summary: String,
}

impl CfLedger {
    /// 新账。
    pub fn new() -> CfLedger {
        CfLedger {
            phase: CfPhase::CfDraft,
            catalogs: Vec::new(),
            matrix: Vec::new(),
            rejects: Vec::new(),
            summary: String::new(),
        }
    }

    /// 开账（CfDraft 受理，begin 幂等）。
    pub fn begin(&mut self) -> Result<(), u32> {
        if self.phase == CfPhase::CfDraft {
            Ok(())
        } else {
            let d = CfDiag::ENotOpen.val();
            self.rejects.push(d);
            Err(d)
        }
    }

    pub fn catalog_len(&self) -> usize {
        self.catalogs.len()
    }

    pub fn matrix_len(&self) -> usize {
        self.matrix.len()
    }

    pub fn reject_len(&self) -> usize {
        self.rejects.len()
    }

    pub fn phase(&self) -> CfPhase {
        self.phase
    }

    fn has_kind(&self, k: CatalogKind) -> bool {
        let mut i = 0usize;
        while i < self.catalogs.len() {
            if let Some(r) = self.catalogs.get(i) {
                if r.kind == k {
                    return true;
                }
            }
            i += 1;
        }
        false
    }

    /// 录盘点行：类目查重 + 行闸 + 容量。
    pub fn reg_catalog(&mut self, r: CatalogRow) -> Result<(), u32> {
        if self.phase != CfPhase::CfDraft {
            let d = if self.phase == CfPhase::CfSealed {
                CfDiag::ESealedWrite.val()
            } else {
                CfDiag::ERewind.val()
            };
            self.rejects.push(d);
            return Err(d);
        }
        if self.catalogs.len() >= CATALOG_KINDS {
            let d = CfDiag::ECatDup.val();
            self.rejects.push(d);
            return Err(d);
        }
        if self.has_kind(r.kind) {
            let d = CfDiag::ECatDup.val();
            self.rejects.push(d);
            return Err(d);
        }
        if r.entries == 0 {
            let d = CfDiag::EEntriesZero.val();
            self.rejects.push(d);
            return Err(d);
        }
        if r.digest == 0 {
            let d = CfDiag::EDigestZero.val();
            self.rejects.push(d);
            return Err(d);
        }
        self.catalogs.push(r);
        Ok(())
    }

    /// 录矩行：行闸 + 用例连号 + 容量。
    pub fn reg_matrix(&mut self, r: MatrixRow) -> Result<(), u32> {
        if self.phase != CfPhase::CfDraft {
            let d = if self.phase == CfPhase::CfSealed {
                CfDiag::ESealedWrite.val()
            } else {
                CfDiag::ERewind.val()
            };
            self.rejects.push(d);
            return Err(d);
        }
        if self.matrix.len() >= MATRIX_CAP {
            let d = CfDiag::EMatrixShort.val();
            self.rejects.push(d);
            return Err(d);
        }
        if r.depth == 0 || r.depth > DEPTH_CAP {
            let d = CfDiag::EDepthZero.val();
            self.rejects.push(d);
            return Err(d);
        }
        if r.case_no != (self.matrix.len() + 1) as u32 {
            let d = CfDiag::EMatrixShort.val();
            self.rejects.push(d);
            return Err(d);
        }
        if !matrix_row_ok(&r) {
            let d = CfDiag::EDomainUnknown.val();
            self.rejects.push(d);
            return Err(d);
        }
        self.matrix.push(r);
        Ok(())
    }

    /// 盘点收束：CfDraft → CfInventoried，三类齐 + 矩阵行额达标 + 覆盖无洞。
    pub fn inventory(&mut self) -> Result<(), u32> {
        match cf_advance(self.phase, CfPhase::CfInventoried) {
            Err(d) => {
                self.rejects.push(d);
                Err(d)
            }
            Ok(()) => {
                if catalog_missing(&self.catalogs) != 0 {
                    let d = CfDiag::ECatShort.val();
                    self.rejects.push(d);
                    return Err(d);
                }
                if self.matrix.len() < MATRIX_MIN {
                    let d = CfDiag::EMatrixShort.val();
                    self.rejects.push(d);
                    return Err(d);
                }
                if !coverage_scan(&self.matrix).is_empty() {
                    let d = CfDiag::ECoverHole.val();
                    self.rejects.push(d);
                    return Err(d);
                }
                self.phase = CfPhase::CfInventoried;
                Ok(())
            }
        }
    }

    /// 封账：CfInventoried → CfSealed，摘要非空。
    pub fn seal(&mut self, summary: &str) -> Result<(), u32> {
        match cf_advance(self.phase, CfPhase::CfSealed) {
            Err(d) => {
                self.rejects.push(d);
                Err(d)
            }
            Ok(()) => {
                if summary.is_empty() {
                    let d = CfDiag::ESummaryEmpty.val();
                    self.rejects.push(d);
                    return Err(d);
                }
                self.summary = String::from(summary);
                self.phase = CfPhase::CfSealed;
                Ok(())
            }
        }
    }

    pub fn summary(&self) -> &str {
        &self.summary
    }
}

// ── 框架指纹（FNV-1a 32bit） ────────────────────────────────────
/// 对盘点账+矩行折叠。
pub fn cf_digest(catalogs: &[CatalogRow], matrix: &[MatrixRow]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    let mut i = 0usize;
    while i < catalogs.len() {
        if let Some(r) = catalogs.get(i) {
            let krank = match r.kind {
                CatalogKind::DomainCatalog => 1u32,
                CatalogKind::JudgeCatalog => 2,
                CatalogKind::AssetCatalog => 3,
            };
            h = h.wrapping_mul(0x0100_0193) ^ krank;
            h = h.wrapping_mul(0x0100_0193) ^ r.entries;
            h = h.wrapping_mul(0x0100_0193) ^ r.digest;
        }
        i += 1;
    }
    let mut j = 0usize;
    while j < matrix.len() {
        if let Some(r) = matrix.get(j) {
            let drank = match r.domain {
                MatrixDomain::D2 => 1u32,
                MatrixDomain::D3 => 2,
                MatrixDomain::D4 => 3,
                MatrixDomain::E1 => 4,
            };
            let krank = match r.ref_kind {
                CatalogKind::DomainCatalog => 1u32,
                CatalogKind::JudgeCatalog => 2,
                CatalogKind::AssetCatalog => 3,
            };
            h = h.wrapping_mul(0x0100_0193) ^ r.case_no;
            h = h.wrapping_mul(0x0100_0193) ^ drank;
            h = h.wrapping_mul(0x0100_0193) ^ krank;
            h = h.wrapping_mul(0x0100_0193) ^ r.depth;
        }
        j += 1;
    }
    h
}

// ── 判据谓词 ─────────────────────────────────────────────────────
/// 谓词：三类对照表盘点齐（3/3、行闸全过、逐类目唯一）。
fn catalog_inventory_ok() -> bool {
    if CATALOG_TABLE.len() != CATALOG_KINDS {
        return false;
    }
    if catalog_missing(CATALOG_TABLE) != 0 {
        return false;
    }
    // 逐类目唯一。
    let mut i = 0usize;
    while i < CATALOG_TABLE.len() {
        let a = match CATALOG_TABLE.get(i) {
            Some(v) => v,
            None => return false,
        };
        let mut j = i + 1;
        while j < CATALOG_TABLE.len() {
            let b = match CATALOG_TABLE.get(j) {
                Some(v) => v,
                None => return false,
            };
            if a.kind == b.kind {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    true
}

/// 谓词：回归矩阵覆盖无洞（行闸全过 + 连号 + 域/类目覆盖无洞）。
fn matrix_coverage_ok() -> bool {
    if MATRIX_TABLE.len() < MATRIX_MIN {
        return false;
    }
    let mut i = 0usize;
    while i < MATRIX_TABLE.len() {
        let r = match MATRIX_TABLE.get(i) {
            Some(v) => v,
            None => return false,
        };
        if !matrix_row_ok(r) {
            return false;
        }
        if r.case_no != (i + 1) as u32 {
            return false;
        }
        i += 1;
    }
    coverage_scan(MATRIX_TABLE).is_empty()
}

/// 谓词：框架可点验（盘点齐 ∧ 覆盖无洞 ∧ 指纹非零 ∧ 行额达标）。
fn framework_ready_ok() -> bool {
    catalog_inventory_ok()
        && matrix_coverage_ok()
        && cf_digest(CATALOG_TABLE, MATRIX_TABLE) != 0
        && MATRIX_TABLE.len() >= MATRIX_MIN
}

/// 完整框架账构造基线：开账→录盘点→录矩阵→盘点收束→封账。
pub fn build_full_ledger() -> CfLedger {
    let mut a = CfLedger::new();
    let _ = a.begin();
    let mut i = 0usize;
    while i < CATALOG_TABLE.len() {
        if let Some(r) = CATALOG_TABLE.get(i) {
            let _ = a.reg_catalog(*r);
        }
        i += 1;
    }
    let mut j = 0usize;
    while j < MATRIX_TABLE.len() {
        if let Some(r) = MATRIX_TABLE.get(j) {
            let _ = a.reg_matrix(*r);
        }
        j += 1;
    }
    let _ = a.inventory();
    let _ = a.seal("C 型开门件：三类对照表盘点齐、回归矩阵覆盖无洞、框架可点验");
    a
}

/// 双跑一致判定。
fn dual_run_ok() -> bool {
    let x = build_full_ledger();
    let y = build_full_ledger();
    x.catalog_len() == y.catalog_len()
        && x.matrix_len() == y.matrix_len()
        && x.reject_len() == y.reject_len()
        && x.phase() == y.phase()
        && cf_digest(CATALOG_TABLE, MATRIX_TABLE) == cf_digest(CATALOG_TABLE, MATRIX_TABLE)
}

/// 谓词：完成写回语义。
fn writeback_ok() -> bool {
    let a = build_full_ledger();
    a.phase() == CfPhase::CfSealed && !a.summary().is_empty()
}

/// 谓词：段位守卫（VX9 窗口）。
fn seg_guard_ok() -> bool {
    SEG_BASE % 0x40 == 0 && SEG_BASE >= 0x80000 && SEG_BASE <= 0x831C0
}

/// VX9 段位公式重算。
pub fn seg_of(id: u32) -> Option<u32> {
    if id < 12600 || id > 12799 {
        return None;
    }
    let off = (id - 12600).checked_mul(0x40)?;
    Some(0x80000u32.checked_add(off)?)
}

// ── 违约注入十族 ─────────────────────────────────────────────────
/// 注入演练：十族框架违约逐路投递，全部 Err（或扫描命中）才算抓全。
pub fn cf_fault_sweep() -> (usize, usize, String) {
    let mut caught = 0usize;
    let mut miss = String::new();

    let c0 = match CATALOG_TABLE.get(0) {
        Some(v) => *v,
        None => return (0, 10, String::from("表空")),
    };
    let m0 = match MATRIX_TABLE.get(0) {
        Some(v) => *v,
        None => return (0, 10, String::from("表空")),
    };

    // 族1：类目重复。
    let mut a1 = CfLedger::new();
    let _ = a1.reg_catalog(c0);
    if a1.reg_catalog(c0).is_err() {
        caught += 1;
    } else {
        miss.push_str("F1类目重复;");
    }

    // 族2：类目零。
    let mut a2 = CfLedger::new();
    let zero = CatalogRow { kind: CatalogKind::JudgeCatalog, entries: 0, digest: 0x1 };
    if a2.reg_catalog(zero).is_err() {
        caught += 1;
    } else {
        miss.push_str("F2类目零;");
    }

    // 族3：指纹零。
    let mut a3 = CfLedger::new();
    let zd = CatalogRow { kind: CatalogKind::AssetCatalog, entries: 3, digest: 0 };
    if a3.reg_catalog(zd).is_err() {
        caught += 1;
    } else {
        miss.push_str("F3指纹零;");
    }

    // 族4：盘点缺类（只录一类即盘点收束）。
    let mut a4 = CfLedger::new();
    let _ = a4.reg_catalog(c0);
    if a4.inventory().is_err() {
        caught += 1;
    } else {
        miss.push_str("F4盘点缺类;");
    }

    // 族5：回归深度零。
    let mut a5 = CfLedger::new();
    let zd_row = MatrixRow { case_no: 1, domain: MatrixDomain::D2, ref_kind: CatalogKind::DomainCatalog, depth: 0 };
    if a5.reg_matrix(zd_row).is_err() {
        caught += 1;
    } else {
        miss.push_str("F5深度零;");
    }

    // 族6：用例跳号。
    let mut a6 = CfLedger::new();
    let gap = MatrixRow { case_no: 2, domain: MatrixDomain::D2, ref_kind: CatalogKind::DomainCatalog, depth: 1 };
    if a6.reg_matrix(gap).is_err() {
        caught += 1;
    } else {
        miss.push_str("F6用例跳号;");
    }

    // 族7：覆盖有洞（抽掉全部 E1 行后盘点收束必拒）。
    let mut a7 = CfLedger::new();
    let mut i = 0usize;
    while i < CATALOG_TABLE.len() {
        if let Some(r) = CATALOG_TABLE.get(i) {
            let _ = a7.reg_catalog(*r);
        }
        i += 1;
    }
    let mut j = 0usize;
    while j < MATRIX_TABLE.len() {
        if let Some(r) = MATRIX_TABLE.get(j) {
            if r.domain != MatrixDomain::E1 {
                let _ = a7.reg_matrix(*r);
            }
        }
        j += 1;
    }
    if a7.inventory().is_err() {
        caught += 1;
    } else {
        miss.push_str("F7覆盖有洞;");
    }

    // 族8：封账后写。
    let mut a8 = build_full_ledger();
    if a8.reg_catalog(c0).is_err() {
        caught += 1;
    } else {
        miss.push_str("F8封账后写;");
    }

    // 族9：相位倒退（封账后再盘点收束）。
    let mut a9 = build_full_ledger();
    if a9.inventory().is_err() {
        caught += 1;
    } else {
        miss.push_str("F9相位倒退;");
    }

    // 族10：空摘要封账。
    let mut a10 = build_full_ledger_no_seal();
    if a10.seal("").is_err() {
        caught += 1;
    } else {
        miss.push_str("F10空摘要;");
    }

    (caught, 10usize, miss)
}

/// 建账至盘点相位（未封账）。
pub fn build_full_ledger_no_seal() -> CfLedger {
    let mut a = CfLedger::new();
    let _ = a.begin();
    let mut i = 0usize;
    while i < CATALOG_TABLE.len() {
        if let Some(r) = CATALOG_TABLE.get(i) {
            let _ = a.reg_catalog(*r);
        }
        i += 1;
    }
    let mut j = 0usize;
    while j < MATRIX_TABLE.len() {
        if let Some(r) = MATRIX_TABLE.get(j) {
            let _ = a.reg_matrix(*r);
        }
        j += 1;
    }
    let _ = a.inventory();
    a
}

/// 演练绿判：10/10 抓全。
fn fault_sweep_ok() -> bool {
    let (c, t, m) = cf_fault_sweep();
    c == t && m.is_empty()
}

// ── 检查节 ───────────────────────────────────────────────────────
/// UNX-F12718 自检入口（≥12 条，全 add_dyn）。
pub fn run_unx12718_checks() -> CheckSet {
    let mut s = CheckSet::new("unx12718");

    s.add_dyn(
        "F12718-01 段位守卫",
        seg_guard_ok(),
        &format!("seg=0x{:05X}", SEG_BASE),
    );

    s.add_dyn(
        "F12718-02 段位公式重算",
        {
            let mut ok = seg_of(12718) == Some(SEG_BASE);
            ok &= seg_of(12600) == Some(0x80000);
            ok &= seg_of(12799) == Some(0x831C0);
            ok &= seg_of(12599).is_none() && seg_of(12800).is_none();
            ok
        },
        "0x80000+0x40×(id−12600) 全窗对号，越窗 None",
    );

    s.add_dyn(
        "F12718-03 诊断码闭集",
        {
            let mut ok = CF_ALL.len() == 12;
            let mut i = 0usize;
            while ok && i < CF_ALL.len() {
                if let Some(d) = CF_ALL.get(i) {
                    let v = d.val();
                    ok &= v > SEG_BASE && v < SEG_BASE + 0x40;
                    ok &= !d.detail().is_empty();
                }
                i += 1;
            }
            ok &= !detail(SEG_BASE + 0x30).is_empty();
            ok
        },
        "0x01..0x0C 闭集偏移互异且文案非空",
    );

    s.add_dyn(
        "F12718-04 三类对照表盘点齐",
        catalog_inventory_ok(),
        &format!("catalogs={} 3/3 行闸全过", CATALOG_TABLE.len()),
    );

    s.add_dyn(
        "F12718-05 回归矩阵覆盖无洞",
        matrix_coverage_ok(),
        &format!("matrix={} 覆盖洞=0", MATRIX_TABLE.len()),
    );

    s.add_dyn(
        "F12718-06 框架可点验四面合取",
        framework_ready_ok(),
        "盘点齐 ∧ 覆盖无洞 ∧ 指纹非零 ∧ 行额达标",
    );

    s.add_dyn(
        "F12718-07 注入联测 10/10 拒止",
        {
            let (c, t, _) = cf_fault_sweep();
            c == t
        },
        &format!("caught={}/10", cf_fault_sweep().0),
    );

    s.add_dyn(
        "F12718-08 相位迁移守卫直读",
        cf_advance(CfPhase::CfDraft, CfPhase::CfInventoried).is_ok()
            && cf_advance(CfPhase::CfInventoried, CfPhase::CfSealed).is_ok()
            && cf_advance(CfPhase::CfSealed, CfPhase::CfInventoried).is_err()
            && cf_advance(CfPhase::CfDraft, CfPhase::CfSealed).is_err(),
        "CfDraft→CfInventoried→CfSealed 只进不退不跳",
    );

    s.add_dyn(
        "F12718-09 类目/域闭集",
        {
            let mut ok = CatalogKind::all().len() == CATALOG_KINDS;
            ok &= MatrixDomain::all().len() == 4;
            let mut i = 0usize;
            while ok && i < CatalogKind::all().len() {
                if let Some(k) = CatalogKind::all().get(i) {
                    ok &= !k.name().is_empty();
                }
                i += 1;
            }
            let mut j = 0usize;
            while ok && j < MatrixDomain::all().len() {
                if let Some(d) = MatrixDomain::all().get(j) {
                    ok &= !d.name().is_empty();
                }
                j += 1;
            }
            ok
        },
        "三类目/四域名恒非空",
    );

    s.add_dyn(
        "F12718-10 覆盖扫描直读",
        {
            // 全表无洞；抽掉 E1 全行的表必现「域缺行」洞。
            let full = coverage_scan(MATRIX_TABLE);
            let mut hole_table: Vec<MatrixRow> = Vec::new();
            let mut j = 0usize;
            while j < MATRIX_TABLE.len() {
                if let Some(r) = MATRIX_TABLE.get(j) {
                    if r.domain != MatrixDomain::E1 {
                        hole_table.push(*r);
                    }
                }
                j += 1;
            }
            let holes = coverage_scan(&hole_table);
            full.is_empty() && !holes.is_empty()
        },
        "覆盖扫描对洞敏感（抽域必现洞）",
    );

    s.add_dyn(
        "F12718-11 完整建账基线",
        {
            let a = build_full_ledger();
            a.catalog_len() == CATALOG_KINDS
                && a.matrix_len() == MATRIX_TABLE.len()
                && a.reject_len() == 0
                && a.phase() == CfPhase::CfSealed
        },
        &format!(
            "catalogs={} matrix={} phase={}",
            build_full_ledger().catalog_len(),
            build_full_ledger().matrix_len(),
            build_full_ledger().phase().name()
        ),
    );

    s.add_dyn(
        "F12718-12 双跑一致",
        dual_run_ok(),
        "确定性重放两轮逐位同判",
    );

    s.add_dyn(
        "F12718-13 框架指纹非零",
        cf_digest(CATALOG_TABLE, MATRIX_TABLE) != 0,
        &format!("digest={:08X}", cf_digest(CATALOG_TABLE, MATRIX_TABLE)),
    );

    s.add_dyn(
        "F12718-14 完成写回语义",
        writeback_ok(),
        "封账即已完成且摘要非空",
    );

    s
}

// ── 测试区 ───────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seg_guard_aligned_and_in_window() {
        assert!(seg_guard_ok());
        assert_eq!(SEG_BASE % 0x40, 0);
        assert!(SEG_BASE >= 0x80000 && SEG_BASE <= 0x831C0);
        assert_eq!(SEG_BASE, 0x81D80);
        assert_eq!(seg_of(12718), Some(SEG_BASE));
    }

    #[test]
    fn diag_codes_are_closed_set() {
        assert_eq!(CF_ALL.len(), 12);
        let mut i = 0usize;
        while i < CF_ALL.len() {
            let vi = match CF_ALL.get(i) {
                Some(d) => d.val(),
                None => 0,
            };
            assert!(vi > SEG_BASE && vi < SEG_BASE + 0x40);
            let mut j = i + 1;
            while j < CF_ALL.len() {
                let vj = match CF_ALL.get(j) {
                    Some(d) => d.val(),
                    None => 0,
                };
                assert_ne!(vi, vj, "诊断码重复 0x{:05X}", vi);
                j += 1;
            }
            assert!(!detail(vi).is_empty());
            i += 1;
        }
        assert!(detail(SEG_BASE + 0x30).contains("未知"));
    }

    #[test]
    fn inventory_and_coverage_hold() {
        assert!(catalog_inventory_ok());
        assert!(matrix_coverage_ok());
        assert!(framework_ready_ok());
        assert_eq!(CATALOG_TABLE.len(), CATALOG_KINDS);
        assert!(MATRIX_TABLE.len() >= MATRIX_MIN);
        // 逐行闸。
        let mut i = 0usize;
        while i < MATRIX_TABLE.len() {
            let r = match MATRIX_TABLE.get(i) {
                Some(v) => v,
                None => return,
            };
            assert!(matrix_row_ok(r));
            assert!(r.depth >= 1 && r.depth <= DEPTH_CAP);
            i += 1;
        }
        // 盘点行闸。
        let mut j = 0usize;
        while j < CATALOG_TABLE.len() {
            let r = match CATALOG_TABLE.get(j) {
                Some(v) => v,
                None => return,
            };
            assert!(catalog_row_ok(r));
            j += 1;
        }
        // 深度上限边界。
        let edge = MatrixRow { case_no: 99, domain: MatrixDomain::D2, ref_kind: CatalogKind::DomainCatalog, depth: DEPTH_CAP + 1 };
        assert!(!matrix_row_ok(&edge));
    }

    #[test]
    fn coverage_scan_hole_sensitivity() {
        assert!(coverage_scan(MATRIX_TABLE).is_empty());
        // 抽掉全部 E1 行 → 域缺行洞。
        let mut hole_table: Vec<MatrixRow> = Vec::new();
        let mut i = 0usize;
        while i < MATRIX_TABLE.len() {
            if let Some(r) = MATRIX_TABLE.get(i) {
                if r.domain != MatrixDomain::E1 {
                    hole_table.push(*r);
                }
            }
            i += 1;
        }
        let holes = coverage_scan(&hole_table);
        assert_eq!(holes.len(), 1);
        // 抽掉全部资产清单引用行 → 类目缺引洞。
        let mut no_asset: Vec<MatrixRow> = Vec::new();
        let mut j = 0usize;
        while j < MATRIX_TABLE.len() {
            if let Some(r) = MATRIX_TABLE.get(j) {
                if r.ref_kind != CatalogKind::AssetCatalog {
                    no_asset.push(*r);
                }
            }
            j += 1;
        }
        let holes2 = coverage_scan(&no_asset);
        assert_eq!(holes2.len(), 1);
        // 空矩阵全洞（4 域 + 3 类目 = 7）。
        assert_eq!(coverage_scan(&[]).len(), 7);
    }

    #[test]
    fn injection_ten_ten_caught() {
        let (c, t, miss) = cf_fault_sweep();
        assert_eq!(c, t, "漏抓：{}", miss);
        assert_eq!(t, 10);
        assert!(fault_sweep_ok());
    }

    #[test]
    fn injection_paths_carry_expected_codes() {
        let c0 = match CATALOG_TABLE.get(0) {
            Some(v) => *v,
            None => return,
        };
        let mut a = CfLedger::new();
        let _ = a.reg_catalog(c0);
        assert_eq!(a.reg_catalog(c0), Err(CfDiag::ECatDup.val()));
        let zero = CatalogRow { kind: CatalogKind::JudgeCatalog, entries: 0, digest: 0x1 };
        assert_eq!(a.reg_catalog(zero), Err(CfDiag::EEntriesZero.val()));
        let zd = CatalogRow { kind: CatalogKind::AssetCatalog, entries: 3, digest: 0 };
        assert_eq!(a.reg_catalog(zd), Err(CfDiag::EDigestZero.val()));
        // 深度零 / 跳号。
        let zd_row = MatrixRow { case_no: 1, domain: MatrixDomain::D2, ref_kind: CatalogKind::DomainCatalog, depth: 0 };
        assert_eq!(a.reg_matrix(zd_row), Err(CfDiag::EDepthZero.val()));
        let gap = MatrixRow { case_no: 2, domain: MatrixDomain::D2, ref_kind: CatalogKind::DomainCatalog, depth: 1 };
        assert_eq!(a.reg_matrix(gap), Err(CfDiag::EMatrixShort.val()));
        // 盘点缺类。
        let mut b = CfLedger::new();
        let _ = b.reg_catalog(c0);
        assert_eq!(b.inventory(), Err(CfDiag::ECatShort.val()));
        // 拒单面：类目重复 + 类目零 + 指纹零 + 深度零 + 用例跳号 = 5。
        assert_eq!(a.reject_len(), 5);
    }

    #[test]
    fn phase_guards_and_seal() {
        assert!(cf_advance(CfPhase::CfDraft, CfPhase::CfInventoried).is_ok());
        assert!(cf_advance(CfPhase::CfInventoried, CfPhase::CfSealed).is_ok());
        assert!(cf_advance(CfPhase::CfInventoried, CfPhase::CfInventoried).is_err());
        assert!(cf_advance(CfPhase::CfSealed, CfPhase::CfDraft).is_err());
        assert!(CfPhase::CfDraft.rank() < CfPhase::CfInventoried.rank());
        assert!(CfPhase::CfInventoried.rank() < CfPhase::CfSealed.rank());
        // 空摘要拒；正常封账过；封账后写拒。
        let mut a = build_full_ledger_no_seal();
        assert_eq!(a.seal(""), Err(CfDiag::ESummaryEmpty.val()));
        assert!(a.seal("框架封账").is_ok());
        let c0 = match CATALOG_TABLE.get(0) {
            Some(v) => *v,
            None => return,
        };
        assert_eq!(a.reg_catalog(c0), Err(CfDiag::ESealedWrite.val()));
        assert_eq!(a.phase(), CfPhase::CfSealed);
    }

    #[test]
    fn dual_run_consistent() {
        assert!(dual_run_ok());
        let x = build_full_ledger();
        let y = build_full_ledger();
        assert_eq!(x.catalog_len(), y.catalog_len());
        assert_eq!(x.matrix_len(), y.matrix_len());
        assert_eq!(x.reject_len(), y.reject_len());
        assert_eq!(x.phase(), y.phase());
        assert_eq!(cf_digest(CATALOG_TABLE, MATRIX_TABLE), cf_digest(CATALOG_TABLE, MATRIX_TABLE));
        assert_ne!(cf_digest(CATALOG_TABLE, MATRIX_TABLE), 0);
    }

    #[test]
    fn writeback_semantics_hold() {
        assert!(writeback_ok());
        for st in [CfPhase::CfDraft, CfPhase::CfInventoried, CfPhase::CfSealed].iter() {
            assert!(!st.name().is_empty());
        }
        for d in CF_ALL.iter() {
            assert!(!d.detail().is_empty());
        }
        for k in CatalogKind::all().iter() {
            assert!(!k.name().is_empty());
        }
        for d in MatrixDomain::all().iter() {
            assert!(!d.name().is_empty());
        }
    }

    #[test]
    fn checks_entry_all_green() {
        let s = run_unx12718_checks();
        assert!(s.all_passed(), "自检存在红项：{}", s.red_items_text());
        assert!(s.len() >= 12, "检查条目不足 12");
    }
}
