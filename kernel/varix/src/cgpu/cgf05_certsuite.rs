//! cgf05_certsuite — CGPU-F0805 认证基准套件（认证标准考卷体系）。
//!
//! 判据原文（施工书锚点 CGPU-F0805）：基准套件：认证标准考卷（D08 基准执行
//! 器+F0830 网页场景+补充分项场景（2D 合成/视频/3D）——全链认证考卷）；考卷
//! 分级（金/银/铜认证对应不同考卷深度）；考卷冻结纪律（认证考卷一经认证冻
//! 结——变更 ADR）；套件分发。判据：全链考卷、分级、冻结、分发、判据。
//!
//! 职责定位：考卷定义+分级深度矩阵+冻结账+ADR 变更申请账+分发打包。D08 基准
//! 执行器与 F0830 网页场景是考卷的消费执行方（本模块只定义卷面不执行）。
//! 与 cgf03_probedomain（F0803 能力探测）分工——彼探测硬件能力、此认证整机
//! 体验达标。冻结指纹 FNV-1a 独立实现（常数写死可对拍）。诊断码段 0x3Exx。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

/// 套件版本（冻结账与分发包携带，对账用）。
pub const CERT_SUITE_VERSION: &str = "F05-certsuite-v1";

/// 全链考卷场景闭集（判据"全链认证考卷"：D08 链 + 网页 + 三分项）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SuiteScene {
    /// D08 基准执行器全链（考卷的执行底座场景）。
    D08Executor,
    /// F0830 网页场景（合成主负载）。
    WebFull,
    /// 2D 合成分项。
    Synth2D,
    /// 视频分项。
    Video,
    /// 3D 渲染分项。
    Render3D,
}

impl SuiteScene {
    pub const ALL: [SuiteScene; 5] = [
        SuiteScene::D08Executor,
        SuiteScene::WebFull,
        SuiteScene::Synth2D,
        SuiteScene::Video,
        SuiteScene::Render3D,
    ];

    pub const fn zh(self) -> &'static str {
        match self {
            SuiteScene::D08Executor => "D08基准执行器全链",
            SuiteScene::WebFull => "F0830网页场景",
            SuiteScene::Synth2D => "2D合成分项",
            SuiteScene::Video => "视频分项",
            SuiteScene::Render3D => "3D渲染分项",
        }
    }

    /// 核心场景（铜级覆盖下限：合成主负载 + D08 底座 + 最大分项 3D）。
    pub const fn is_core(self) -> bool {
        matches!(self, SuiteScene::D08Executor | SuiteScene::WebFull | SuiteScene::Render3D)
    }
}

/// 认证级闭集（判据"金/银/铜认证对应不同考卷深度"）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CertGrade {
    Gold,
    Silver,
    Bronze,
}

impl CertGrade {
    pub const fn zh(self) -> &'static str {
        match self {
            CertGrade::Gold => "金认证",
            CertGrade::Silver => "银认证",
            CertGrade::Bronze => "铜认证",
        }
    }

    /// 深度矩阵：该级考卷的帧率阈值（ppm 上浮 = 允许劣化幅度，0=不许劣化）。
    /// 金级 0ppm（全项+严判）；银级 30_000ppm（全项+标准）；铜级 80_000ppm
    /// （核心项+宽松）。阈值放宽显性留档——不是悄悄放水。
    pub const fn slack_ppm(self) -> u64 {
        match self {
            CertGrade::Gold => 0,
            CertGrade::Silver => 30_000,
            CertGrade::Bronze => 80_000,
        }
    }

    /// 深度矩阵：该级是否覆盖全场景（金/银全链，铜仅核心项）。
    pub const fn full_coverage(self) -> bool {
        !matches!(self, CertGrade::Bronze)
    }

    /// 深度矩阵：该级每场景最小用例数（深度=用例密度）。
    pub const fn min_cases(self) -> u32 {
        match self {
            CertGrade::Gold => 12,
            CertGrade::Silver => 8,
            CertGrade::Bronze => 4,
        }
    }
}

/// 考卷项（单场景卷面：用例数 + 达标线）。
#[derive(Clone, Copy, Debug)]
pub struct SuiteItem {
    /// 场景。
    pub scene: SuiteScene,
    /// 用例数。
    pub cases: u32,
    /// 帧率达标线（ppm 达标帧率比，900_000 = 90% 目标帧率）。
    pub pass_ppm: u64,
    /// 考卷时长（帧）。
    pub frames: u32,
}

impl SuiteItem {
    /// 卷面项合法性：用例/时长非零，达标线在 500_000~1_000_000 合理带。
    pub const fn valid(&self) -> bool {
        self.cases > 0 && self.frames > 0 && self.pass_ppm >= 500_000 && self.pass_ppm <= 1_000_000
    }
}

/// 认证级裁定（考卷执行结果回填后的结论）。
#[derive(Clone, Copy, Debug)]
pub struct GradeResult {
    /// 场景。
    pub scene: SuiteScene,
    /// 实测达标线（ppm）。
    pub measured_ppm: u64,
    /// 卷面达标线 + 级别放宽后的生效线。
    pub effective_ppm: u64,
    /// 该项是否通过。
    pub passed: bool,
}

impl GradeResult {
    /// 生效线 = 卷面线 × (1_000_000 - 放宽ppm) / 1_000_000（放宽显性）。
    pub fn effective(item_pass_ppm: u64, grade: CertGrade) -> u64 {
        item_pass_ppm * (1_000_000 - grade.slack_ppm()) / 1_000_000
    }
}

/// ADR 变更申请账行（冻结后改卷必须走 ADR——本模块记账不审批）。
#[derive(Clone, Debug)]
pub struct AdrEntry {
    /// 申请序号。
    pub seq: u32,
    /// 试图变更的项。
    pub item: SuiteItem,
    /// 申请理由（冻结账留痕，审批在 ADR 流程）。
    pub reason: &'static str,
    /// 审批状态（本模块只记 submitted——批准由 ADR 流程另行落账）。
    pub approved: bool,
}

/// 诊断码（0x3Exx 段独占）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum XcCode {
    /// 0x3E01 考卷项非法（用例/时长为零或达标线出带）。
    CeItemInvalid,
    /// 0x3E02 冻结后改卷被拒（变更必须 ADR——铁律级）。
    CeFrozenImmutable,
    /// 0x3E03 未知认证级（闭集外查询）。
    CeGradeUnknown,
    /// 0x3E04 未认证先分发被拒（冻结是分发前置）。
    CeNotCertified,
    /// 0x3E05 变更申请未过 ADR（已记账、未批准——不可改卷）。
    CeAdrRequired,
    /// 0x3E06 分发包指纹对不上（卷面被篡改——分发前必须复验）。
    CeFingerprintMismatch,
    /// 0x3E07 深度矩阵不满足（该级用例密度低于下限）。
    CeDepthShort,
}

impl XcCode {
    /// 诊断详情（闭集逐码中文说明）。
    pub const fn detail(self) -> &'static str {
        match self {
            XcCode::CeItemInvalid => "考卷项非法：用例数/时长必须非零，达标线须在 50%~100% 带内",
            XcCode::CeFrozenImmutable => "考卷已冻结：任何卷面变更必须先走 ADR，直接改卷被拒",
            XcCode::CeGradeUnknown => "未知认证级：金/银/铜三级闭集之外",
            XcCode::CeNotCertified => "考卷未认证冻结：不允许分发未冻结套件",
            XcCode::CeAdrRequired => "变更申请已记账但 ADR 未批准：卷面维持冻结版",
            XcCode::CeFingerprintMismatch => "分发包指纹不符：卷面被篡改，分发拦截",
            XcCode::CeDepthShort => "考卷深度不足：该认证级要求更高的用例密度",
        }
    }
}

/// FNV-1a 64 位（卷面指纹；常数写死独立实现可对拍）。
pub fn fnv1a(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for &b in bytes {
        h = (h ^ b as u64).wrapping_mul(0x100000001b3);
    }
    h
}

/// 认证基准套件本体（考卷账 + 冻结账 + ADR 账 + 分发）。
pub struct CertSuite {
    /// 卷面（按场景一项）。
    items: Vec<SuiteItem>,
    /// 冻结状态（false=起草中可改，true=已认证冻结）。
    frozen: bool,
    /// 冻结时指纹（分发复验基准）。
    fingerprint: u64,
    /// ADR 变更申请账。
    pub adr: Vec<AdrEntry>,
    /// ADR 序号发生器。
    adr_seq: u32,
    /// 立案计数。
    pub cases: u32,
}

impl CertSuite {
    /// 起草一套空考卷（逐项 add_item 填卷）。
    pub fn draft() -> Self {
        CertSuite {
            items: Vec::new(),
            frozen: false,
            fingerprint: 0,
            adr: Vec::new(),
            adr_seq: 0,
            cases: 0,
        }
    }

    /// 加卷面项（冻结后拒——CeFrozenImmutable；项非法拒——CeItemInvalid）。
    pub fn add_item(&mut self, item: SuiteItem) -> Result<(), XcCode> {
        if self.frozen {
            self.cases = self.cases.saturating_add(1);
            return Err(XcCode::CeFrozenImmutable);
        }
        if !item.valid() {
            self.cases = self.cases.saturating_add(1);
            return Err(XcCode::CeItemInvalid);
        }
        // 同场景重复项拒绝（卷面一场景一项——重复即卷面含糊）
        if self.items.iter().any(|x| x.scene == item.scene) {
            self.cases = self.cases.saturating_add(1);
            return Err(XcCode::CeItemInvalid);
        }
        self.items.push(item);
        Ok(())
    }

    /// 卷面项数。
    pub fn item_count(&self) -> usize {
        self.items.len()
    }

    /// 深度校验（判据"分级对应不同考卷深度"的机械化）：
    /// 该级要求全场景覆盖（金/银）或核心项覆盖（铜），且每项用例数 ≥ 下限。
    /// 立案写账（&mut self）。
    pub fn depth_check(&mut self, grade: CertGrade) -> Result<(), XcCode> {
        for s in SuiteScene::ALL {
            let need = grade.full_coverage() || s.is_core();
            match self.items.iter().find(|x| x.scene == s) {
                Some(item) => {
                    if item.cases < grade.min_cases() {
                        self.cases = self.cases.saturating_add(1);
                        return Err(XcCode::CeDepthShort);
                    }
                }
                None => {
                    if need {
                        self.cases = self.cases.saturating_add(1);
                        return Err(XcCode::CeDepthShort);
                    }
                }
            }
        }
        Ok(())
    }

    /// 认证冻结（分发前置；冻结即指纹落账——判据"一经认证冻结"）。
    pub fn certify(&mut self, grade: CertGrade) -> Result<u64, XcCode> {
        self.depth_check(grade)?;
        if self.items.is_empty() {
            self.cases = self.cases.saturating_add(1);
            return Err(XcCode::CeItemInvalid);
        }
        self.frozen = true;
        self.fingerprint = self.compute_fingerprint();
        Ok(self.fingerprint)
    }

    /// 冻结状态。
    pub const fn frozen(&self) -> bool {
        self.frozen
    }

    /// 卷面指纹（场景+用例+达标线+时长逐字节序列化后 FNV）。
    pub fn compute_fingerprint(&self) -> u64 {
        let mut buf: Vec<u8> = Vec::new();
        for it in &self.items {
            buf.push(it.scene as u8);
            buf.extend_from_slice(&it.cases.to_le_bytes());
            buf.extend_from_slice(&it.pass_ppm.to_le_bytes());
            buf.extend_from_slice(&it.frames.to_le_bytes());
        }
        fnv1a(&buf)
    }

    /// 冻结后改卷请求 → 记 ADR 账并拒（判据"变更 ADR"纪律的落账形态）。
    pub fn request_change(&mut self, item: SuiteItem, reason: &'static str) -> Result<u32, XcCode> {
        if !self.frozen {
            // 未冻结直接改（起草期）——不走 ADR
            return if item.valid() {
                self.replace_item(item);
                Ok(0)
            } else {
                self.cases = self.cases.saturating_add(1);
                Err(XcCode::CeItemInvalid)
            };
        }
        self.adr_seq = self.adr_seq.saturating_add(1);
        self.adr.push(AdrEntry {
            seq: self.adr_seq,
            item,
            reason,
            approved: false, // 审批在 ADR 流程；本模块只记 submitted
        });
        self.cases = self.cases.saturating_add(1);
        Err(XcCode::CeAdrRequired)
    }

    /// ADR 批准落地（调用方确已走完 ADR 流程后调用：改卷+解冻+重冻周期）。
    /// 批准后卷面回起草态（重新认证才可再分发——冻结纪律全周期闭合）。
    pub fn apply_approved_adr(&mut self, adr_seq: u32, item: SuiteItem) -> Result<(), XcCode> {
        let ok = self
            .adr
            .iter_mut()
            .find(|a| a.seq == adr_seq && !a.approved)
            .map(|a| {
                a.approved = true;
                true
            })
            .unwrap_or(false);
        if !ok {
            self.cases = self.cases.saturating_add(1);
            return Err(XcCode::CeAdrRequired);
        }
        if !item.valid() {
            self.cases = self.cases.saturating_add(1);
            return Err(XcCode::CeItemInvalid);
        }
        self.frozen = false; // 回起草态：重新 certify 才可分发
        self.replace_item(item);
        Ok(())
    }

    fn replace_item(&mut self, item: SuiteItem) {
        match self.items.iter().position(|x| x.scene == item.scene) {
            Some(i) => self.items[i] = item,
            None => self.items.push(item),
        }
    }

    /// 分发打包（判据"套件分发"：未冻结拒；级别未知拒；包内卷面按级裁剪）。
    /// 立案写账（&mut self）。
    pub fn pack_for(&mut self, grade: CertGrade, expected_fp: u64) -> Result<SuitePackage, XcCode> {
        if !self.frozen {
            self.cases = self.cases.saturating_add(1);
            return Err(XcCode::CeNotCertified);
        }
        let fp = self.compute_fingerprint();
        if fp != self.fingerprint || fp != expected_fp {
            self.cases = self.cases.saturating_add(1);
            return Err(XcCode::CeFingerprintMismatch);
        }
        let mut items = Vec::new();
        for &it in &self.items {
            if !grade.full_coverage() && !it.scene.is_core() {
                continue; // 铜级只发核心项（深度矩阵裁剪显性）
            }
            items.push(it);
        }
        Ok(SuitePackage {
            grade,
            items,
            version: CERT_SUITE_VERSION,
            fingerprint: fp,
        })
    }
}

/// 分发包（判据"套件分发"的载体：执行方 D08 拿包跑卷）。
#[derive(Clone, Debug)]
pub struct SuitePackage {
    /// 认证级。
    pub grade: CertGrade,
    /// 包内卷面（按级裁剪后）。
    pub items: Vec<SuiteItem>,
    /// 版本。
    pub version: &'static str,
    /// 卷面指纹（执行侧复验）。
    pub fingerprint: u64,
}

impl SuitePackage {
    /// 执行结果回填判定：逐项出 GradeResult（生效线=卷面线×放宽系数）。
    pub fn grade(&self, measured: &[(SuiteScene, u64)]) -> Vec<GradeResult> {
        let mut out = Vec::new();
        for it in &self.items {
            let eff = GradeResult::effective(it.pass_ppm, self.grade);
            let m = measured
                .iter()
                .find(|(s, _)| *s == it.scene)
                .map(|(_, v)| *v)
                .unwrap_or(0);
            out.push(GradeResult {
                scene: it.scene,
                measured_ppm: m,
                effective_ppm: eff,
                passed: m >= eff,
            });
        }
        out
    }

    /// 包级结论：全部项通过才授级。
    pub fn certify_conclusion(&self, results: &[GradeResult]) -> Option<CertGrade> {
        if results.len() < self.items.len() || results.iter().any(|r| !r.passed) {
            return None;
        }
        Some(self.grade)
    }

    /// 单行摘要。
    pub fn summary(&self) -> alloc::string::String {
        alloc::format!(
            "考卷包 {} {} 项 {} 指纹{:016x}",
            self.version,
            self.grade.zh(),
            self.items.len(),
            self.fingerprint,
        )
    }
}
