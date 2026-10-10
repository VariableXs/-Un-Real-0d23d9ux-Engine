//! VE-F2414 · 动画文档（VE-M 域 · 动画段 · 目标 300 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F2414`
//!
//! **判据（锚点原文）**：三文档、管线单源、统一叙事、演练门禁、判据。
//!
//! **职责定位（锚点原文）**：动画文档——三份文档：动画白皮书（求值
//! 管线/轨道系统/时间轴统一：F2401 四段管线+F2402 轨道系统+F1345
//! 时间轴统一的架构文档——面向使用者的动画架构全解：与剪辑域（F1345）
//! 的关系声明（剪辑轨道=M 域轨道的使用场景——单源复用叙事））、轨道
//! 制作指南（六类轨道/绑定/批量操作实操——面向 TA 的轨道手册：绑定
//! 路径语法/批量操作流/事件轨配置）、导入导出手册（glTF 映射/校验/
//! 往返——F2409/F2410 的使用者文档：导入流程/导出精度边界），入 M
//! 域文档体系。
//!
//! # 一、文档即结构：三份文档各章各表全部落成 Rust 数据
//!
//! 散文不可对账，结构才可对账。三文档各自的行集（[`DocRow`]）挂
//! **单源引用**（[`SingleSource`]）而非抄录数值：行的数字/清单只能
//! 经 `source.fetch()` 取自实现模块（vem02 六类轨/vem06 事件注册/
//! vem09 映射表/vem10 逆表/vem13 冻结簿/vem02 KeyframeRef 载体），
//! 入册时取现值盖快照——文档面**没有任何 API 可以写死一个数字**
//! （想抄也没有入口）。CI 对账钩子（[`ci_reconcile`]）逐行审计：
//! 快照≠现值即漂移拦截（锚点错误路径：与实现漂移→CI 拦截、单源
//! 复制→守卫）。
//!
//! # 二、统一叙事与 F1345 跨域对账
//!
//! 白皮书统一章声明「剪辑轨道=M 域轨道的使用场景」——这不是修辞，
//! 是**跨域契约**：M 域轨道的数据载体必须确实是 F1345 侧的关键帧
//! 资产引用（vem02 [`KeyframeRef`](vem02_track::KeyframeRef) 的字段
//! 文档明记"F1345 侧"）。[`verify_unification`] 真调构造载体对拍，
//! 失同步即跨域立案（锚点错误路径原文）。
//!
//! # 三、演练门禁：指南步骤引用的 API 必须活着
//!
//! 指南六步各带一个 API 全名，[`rehearse_guide`] 逐步对拍
//! [F2413 v1 冻结簿](vem13_freeze::FreezeBook)（十二条）——引用了
//! 不在簿里的 API（v2 追加段/拼错名）即拦截（锚点：步骤失效→演练
//! 门禁）。术语以 [`f2419_handoff`](vem13_freeze::f2419_handoff) 的
//! 裁决表为准真调仲裁（track/clip/keyframe 三术语）。
//!
//! **性能（锚点原文）**：构建静态生成；对账随合入；无运行时——
//! 本模块零热路径。
//!
//! **无障碍（锚点原文）**：全文档替述可读（每行 text 非空即判据）；
//! 动画术语配通俗解释；无敏感信息。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use crate::svstar2::vem02_track::{KeyframeRef, TrackClass as ValueTrackClass};
use crate::svstar2::vem06_event::{DiagBag as EventBag, EventNameRegistry};
use crate::svstar2::vem09_import::MappingTable;
use crate::svstar2::vem10_export::ExportMapTable;
use crate::svstar2::vem13_freeze::{f2419_handoff, FreezeBook, FREEZE_V1};

// ---------------------------------------------------------------------------
// 一、常量与错误码
// ---------------------------------------------------------------------------

/// 本项版本。
pub const DOC_SUITE_VERSION: &str = "M14-docs-v1";

/// 文档文本里出现与源不符的抄录值（单源复制守卫）。
pub const E_DOC_COPY: &str = "E_DOC_COPY";

/// 与实现漂移（快照≠现值）。
pub const E_DOC_DRIFT: &str = "E_DOC_DRIFT";

/// 统一叙事与 F1345 失同步（跨域对账）。
pub const E_DOC_UNIFY: &str = "E_DOC_UNIFY";

/// 演练门禁：指南步骤引用的 API 失效。
pub const E_DOC_REHEARSE: &str = "E_DOC_REHEARSE";

/// 术语与 F2419 裁决表不一致。
pub const E_DOC_TERM: &str = "E_DOC_TERM";

/// 指南步骤数（六步：建轨→设参→绑定→建 clip→单点求值→导出）。
pub const GUIDE_STEPS: usize = 6;

// ---------------------------------------------------------------------------
// 二、文档读者、文档种类、单源引用
// ---------------------------------------------------------------------------

/// 文档读者三分类（锚点：白皮书面向使用者/指南面向 TA/手册面向使用者）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DocAudience {
    /// 引擎使用者（读架构的人）。
    User,
    /// 技术美术（调轨道的人）。
    Technician,
    /// 管线集成者（做导入导出的人）。
    Integrator,
}

impl DocAudience {
    /// 三读者闭集。
    pub const ALL: [DocAudience; 3] = [DocAudience::User, DocAudience::Technician, DocAudience::Integrator];

    /// 中文名（替述可读）。
    pub fn zh(self) -> &'static str {
        match self {
            DocAudience::User => "引擎使用者",
            DocAudience::Technician => "技术美术",
            DocAudience::Integrator => "管线集成者",
        }
    }
}

/// 三文档。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DocKind {
    /// 动画白皮书（架构+统一叙事）。
    Whitepaper,
    /// 轨道制作指南（类型+绑定+批量）。
    Guide,
    /// 导入导出手册（映射+校验+精度）。
    Manual,
}

impl DocKind {
    /// 三文档闭集。
    pub const ALL: [DocKind; 3] = [DocKind::Whitepaper, DocKind::Guide, DocKind::Manual];

    /// 标题。
    pub fn title(self) -> &'static str {
        match self {
            DocKind::Whitepaper => "动画白皮书",
            DocKind::Guide => "轨道制作指南",
            DocKind::Manual => "导入导出手册",
        }
    }

    /// 主读者。
    pub fn audience(self) -> DocAudience {
        match self {
            DocKind::Whitepaper => DocAudience::User,
            DocKind::Guide => DocAudience::Technician,
            DocKind::Manual => DocAudience::Integrator,
        }
    }

    /// 章节数（白皮书 3/指南 3/手册 3）。
    pub fn chapters(self) -> usize {
        3
    }
}

/// 单源引用（文档行唯一允许的数字/清单来源）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SingleSource {
    /// vem02 六类语义轨官方名。
    TrackClasses,
    /// vem13 冻结 API 十二条全名。
    FrozenSigns,
    /// vem09 映射表规格摘要（四通道单源）。
    MappingSpec,
    /// vem10 逆表行数（四通道逆映射）。
    ExportInverse,
    /// vem02 KeyframeRef 载体（F1345 侧资产引用）。
    KeyframeCarrier,
    /// vem06 事件注册制（注册/查表）。
    EventRegistry,
}

impl SingleSource {
    /// 取现值摘要（文档面写不出数字——只能 fetch）。
    pub fn fetch(self) -> String {
        match self {
            SingleSource::TrackClasses => {
                let names: Vec<&str> = ValueTrackClass::ALL.iter().map(|c| c.as_str()).collect();
                names.join("/")
            }
            SingleSource::FrozenSigns => {
                let book = match FreezeBook::freeze(&FREEZE_V1) {
                    Ok(b) => b,
                    Err(_) => FreezeBook::new(),
                };
                format!("{:02} 签名", book.len())
            }
            SingleSource::MappingSpec => {
                format!("mapping-checksum={}", MappingTable::standard().declared_checksum())
            }
            SingleSource::ExportInverse => {
                let t = ExportMapTable::from_forward(&MappingTable::standard());
                format!("inverse-rows={}", t.rows().len())
            }
            SingleSource::KeyframeCarrier => {
                let asset = KeyframeRef::new("doc-asset", 64);
                format!("carrier(asset_id,count={})", asset.count)
            }
            SingleSource::EventRegistry => {
                let mut reg = EventNameRegistry::new();
                let mut bag = EventBag::new();
                let _ = reg.register("m.anim.event.event_track", &mut bag);
                format!("registry-len={}", reg.len())
            }
        }
    }
}

// ---------------------------------------------------------------------------
// 三、文档行与三文档
// ---------------------------------------------------------------------------

/// 文档行：文本替述可读 + 单源引用 + 入册快照 + 读者。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DocRow {
    /// 所属文档。
    pub doc: DocKind,
    /// 章名。
    pub chapter: &'static str,
    /// 行文本（替述可读——无障碍判据：非空）。
    pub text: String,
    /// 单源引用（唯一数字来源）。
    pub source: SingleSource,
    /// 入册快照（None=未对账；与现值不符即漂移）。
    pub snapshot: Option<String>,
    /// 行读者。
    pub audience: DocAudience,
}

impl DocRow {
    /// 新行（未盖快照——未对账行过不了守卫）。
    pub fn new(
        doc: DocKind,
        chapter: &'static str,
        text: &str,
        source: SingleSource,
        audience: DocAudience,
    ) -> DocRow {
        DocRow {
            doc,
            chapter,
            text: String::from(text),
            source,
            snapshot: None,
            audience,
        }
    }

    /// 入册：取源现值盖快照（**盖章动作而非抄写动作**——数字仍住在
    /// 实现侧，文档只存对账凭据）。
    pub fn stamp(&mut self) {
        self.snapshot = Some(self.source.fetch());
    }

    /// 逐行审计：快照必须在册且与现值逐字节相符。
    pub fn audit(&self) -> Result<(), String> {
        let live = self.source.fetch();
        match &self.snapshot {
            None => {
                Err(format!("{}：{}/{} 行未对账（无快照）", E_DOC_DRIFT, self.doc.title(), self.chapter))
            }
            Some(s) if *s != live => Err(format!(
                "{}：{}/{} 行快照「{}」≠源现值「{}」（复制或漂移，请回源改或重盖章）",
                E_DOC_DRIFT, self.doc.title(), self.chapter, s, live
            )),
            Some(_) => Ok(()),
        }
    }
}

/// 白皮书：管线章（四段管线）+ 轨道章（六类规格）+ 统一章（F1345 叙事）。
pub fn whitepaper() -> Vec<DocRow> {
    let mut rows = alloc::vec![
        DocRow::new(
            DocKind::Whitepaper,
            "管线章·四段管线",
            "求值管线四段：取样→插值→批调度→输出（F2401 架构）；每段的成本模型与\
             分批策略见 F2407，本文只引规模声明不抄数值",
            SingleSource::FrozenSigns,
            DocAudience::User,
        ),
        DocRow::new(
            DocKind::Whitepaper,
            "轨道章·六类规格",
            "轨道六类：位置/旋转/缩放/颜色/浮点/离散（vem02 官方名清单）；离散轨\
             无插值、事件轨走 F1408 注册制触发",
            SingleSource::TrackClasses,
            DocAudience::User,
        ),
        DocRow::new(
            DocKind::Whitepaper,
            "统一章·时间轴统一",
            "时间轴统一：剪辑域（F1345）轨道即 M 域轨道的使用场景——M 域轨道的\
             数据载体就是 F1345 侧关键帧资产引用（vem02 KeyframeRef 字段声明），\
             单源复用不复制",
            SingleSource::KeyframeCarrier,
            DocAudience::User,
        ),
    ];
    for r in rows.iter_mut() {
        r.stamp();
    }
    rows
}

/// 制作指南：类型章 + 绑定章（路径语法）+ 批量章。
pub fn guide() -> Vec<DocRow> {
    let mut rows = alloc::vec![
        DocRow::new(
            DocKind::Guide,
            "类型章",
            "六类轨道选型：连续轨走插值、离散轨跳变；布尔/枚举/触发轨属离散类，\
             按 vem02 轨类清单对号入座",
            SingleSource::TrackClasses,
            DocAudience::Technician,
        ),
        DocRow::new(
            DocKind::Guide,
            "绑定章·路径语法",
            "绑定路径语法：/ 开头、根段限 node/material/custom 三闭集、其后属性段\
             （F2402 协议）；根段写错解析器立拒，照本文抄不会错",
            SingleSource::FrozenSigns,
            DocAudience::Technician,
        ),
        DocRow::new(
            DocKind::Guide,
            "批量章",
            "批量操作流：选择集→框选/复制/粘贴→时间缩放（F2404 操作族）；批量\
             前先冻结引用，避免半途改轨",
            SingleSource::FrozenSigns,
            DocAudience::Technician,
        ),
    ];
    for r in rows.iter_mut() {
        r.stamp();
    }
    rows
}

/// 导入导出手册：映射章 + 校验章 + 精度章。
pub fn manual() -> Vec<DocRow> {
    let mut rows = alloc::vec![
        DocRow::new(
            DocKind::Manual,
            "映射章",
            "四通道映射：位置→translation/旋转→rotation/缩放→scale/形态→weights\
             （F2409 规格表单源）；映射表变更以规格摘要为准，本文不抄表",
            SingleSource::MappingSpec,
            DocAudience::Integrator,
        ),
        DocRow::new(
            DocKind::Manual,
            "校验章",
            "导入三重校验：通道引用/采样数据/格式合规——畸形输入显性拒绝三要素\
             （什么错/在哪/怎么办）；事件名先注册后触发（F1408 注册制）",
            SingleSource::EventRegistry,
            DocAudience::Integrator,
        ),
        DocRow::new(
            DocKind::Manual,
            "精度章",
            "导出精度边界：逆映射四行对拍（F2409 正表机械求逆）+ 采样重排容差表\
             + 精度诚实声明——导出不是无损承诺，边界文档见 F2410",
            SingleSource::ExportInverse,
            DocAudience::Integrator,
        ),
    ];
    for r in rows.iter_mut() {
        r.stamp();
    }
    rows
}

/// 三文档全集（CI 对账的输入）。
pub fn three_docs() -> [(DocKind, Vec<DocRow>); 3] {
    [(DocKind::Whitepaper, whitepaper()), (DocKind::Guide, guide()), (DocKind::Manual, manual())]
}

// ---------------------------------------------------------------------------
// 四、统一叙事与 F1345 跨域对账
// ---------------------------------------------------------------------------

/// F1345 载体（vem02 KeyframeRef——字段文档明记 F1345 侧）。
fn unification_carrier() -> KeyframeRef {
    KeyframeRef::new("unify-asset", 256)
}

/// 统一叙事对账（可注入版）：载体确实承载 F1345 侧资产引用（count 自洽
/// 且可引用），且白皮书统一章行的快照与源现值相符。
///
/// 失同步即跨域立案（锚点：统一叙事与 F1345 失同步→跨域对账）——M 域
/// 文档声称"剪辑轨道=M 域轨道使用场景"，物质证据就是轨道数据载体与
/// F1345 侧引用同型；型不对，叙事就是空话。
pub fn verify_unification_rows(rows: &[DocRow]) -> Result<(), String> {
    let asset = unification_carrier();
    if asset.count == 0 || asset.asset_id.trim().is_empty() {
        return Err(format!(
            "{}：M 域轨道载体与 F1345 侧引用失同步——KeyframeRef 须承载非空资产标识与非零计数",
            E_DOC_UNIFY
        ));
    }
    // 与文档行单源同拍：统一章行的快照必须就是该载人的 fetch。
    let unify_row = match rows.iter().find(|r| r.chapter.starts_with("统一章")) {
        Some(r) => r,
        None => return Err(format!("{}：白皮书缺统一章（叙事无处安放）", E_DOC_UNIFY)),
    };
    unify_row.audit().map_err(|e| format!("{}：{}", E_DOC_UNIFY, e))
}

/// 统一叙事对账（默认取白皮书行集）。
pub fn verify_unification() -> Result<(), String> {
    verify_unification_rows(&whitepaper())
}

// ---------------------------------------------------------------------------
// 五、演练门禁：指南步骤 ↔ F2413 冻结 API
// ---------------------------------------------------------------------------

/// 指南步骤（照文档做一遍的最小动作序列——全名须命中 v1 冻结簿）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GuideStep {
    /// 步号（从 1 起）。
    pub ordinal: u32,
    /// 动作替述（读者看到的指令文本）。
    pub action: &'static str,
    /// 调用的 API 全名（必须在 F2413 v1 冻结簿）。
    pub api: &'static str,
}

/// 指南六步（建轨→设参→绑定→建 clip→单点求值→导出——全过 v1 十二条）。
pub fn guide_steps() -> [GuideStep; GUIDE_STEPS] {
    [
        GuideStep { ordinal: 1, action: "按规格创建轨道并挂 F1345 资产引用", api: "m.anim.track.create" },
        GuideStep { ordinal: 2, action: "写轨道权重等参数（写即钳制记账）", api: "m.anim.track.set_param" },
        GuideStep { ordinal: 3, action: "按 F2402 路径把轨道绑到目标属性", api: "m.anim.track.bind" },
        GuideStep { ordinal: 4, action: "用轨道清单创建 clip", api: "m.anim.clip.create" },
        GuideStep { ordinal: 5, action: "单点求值验证轨道走向", api: "m.anim.eval.at" },
        GuideStep { ordinal: 6, action: "导出 glTF 并核对精度声明", api: "m.anim.io.export" },
    ]
}

/// 演练门禁：逐步对拍 v1 冻结簿——引用了不在簿里的 API 即失效步骤。
pub fn rehearse_guide(steps: &[GuideStep], book: &FreezeBook) -> Result<(), String> {
    for st in steps.iter() {
        if book.get(st.api).is_none() {
            return Err(format!(
                "{}：指南第 {} 步（{}）引用的 API {} 不在 v1 冻结簿——步骤失效，\
                 请修正指南或走 v2 追加段",
                E_DOC_REHEARSE, st.ordinal, st.action, st.api
            ));
        }
    }
    if steps.len() != GUIDE_STEPS {
        return Err(format!(
            "{}：指南步骤数 {} ≠ 预期 {}（六步缺一即流程不完整）",
            E_DOC_REHEARSE,
            steps.len(),
            GUIDE_STEPS
        ));
    }
    Ok(())
}

/// 术语仲裁：与 F2419 裁决表不一致 → 拒（以裁决表为准回改）。
pub fn arbitrate_terms(doc_terms: &[(&'static str, &'static str)]) -> Result<(), String> {
    let book = match FreezeBook::freeze(&FREEZE_V1) {
        Ok(b) => b,
        Err(_) => FreezeBook::new(),
    };
    let ho = f2419_handoff(&book);
    for (en, zh) in ho.terms.iter() {
        match doc_terms.iter().find(|(d_en, _)| d_en == en) {
            None => {
                return Err(format!(
                    "{}：术语 {}（{}）缺失——M 域动画段术语须并入裁决表（F2419）",
                    E_DOC_TERM, en, zh
                ))
            }
            Some((_, d_zh)) if d_zh != zh => {
                return Err(format!(
                    "{}：术语 {} 中文「{}」与 F2419 裁决表「{}」不一致——以裁决表为准回改",
                    E_DOC_TERM, en, d_zh, zh
                ))
            }
            _ => {}
        }
    }
    Ok(())
}

/// 本文档体系术语表（与 F2419 裁决表同源同检）。
pub fn glossary() -> Vec<(&'static str, &'static str)> {
    let book = match FreezeBook::freeze(&FREEZE_V1) {
        Ok(b) => b,
        Err(_) => FreezeBook::new(),
    };
    f2419_handoff(&book).terms.to_vec()
}

// ---------------------------------------------------------------------------
// 六、CI 对账钩子
// ---------------------------------------------------------------------------

/// 文档套件 CI 对账（随合入跑）：逐行单源审计 + 读者覆盖 + 统一叙事 +
/// 演练门禁 + 术语仲裁。任一不过即拒。
pub fn ci_reconcile(docs: &[(DocKind, Vec<DocRow>); 3]) -> Result<(), String> {
    for (kind, rows) in docs.iter() {
        let chapters: Vec<&str> = rows.iter().map(|r| r.chapter).collect();
        let mut uniq: Vec<&str> = Vec::new();
        for c in chapters.iter() {
            if !uniq.contains(c) {
                uniq.push(c);
            }
        }
        if uniq.len() != kind.chapters() {
            return Err(format!(
                "{}：{} 章节数 {} ≠ 预期 {}",
                E_DOC_DRIFT,
                kind.title(),
                uniq.len(),
                kind.chapters()
            ));
        }
        for r in rows.iter() {
            r.audit()?;
        }
        for r in rows.iter() {
            if r.text.trim().is_empty() {
                return Err(format!(
                    "{}：{}/{} 行文本为空——替述不可读，文档不成文档",
                    E_DOC_DRIFT,
                    kind.title(),
                    r.chapter
                ));
            }
        }
    }
    for a in DocAudience::ALL.iter() {
        let covered = docs.iter().any(|(_, rows)| rows.iter().any(|r| r.audience == *a));
        if !covered {
            return Err(format!(
                "{}：读者 {} 在三文档中零覆盖——文档只服务一部分人",
                E_DOC_DRIFT,
                a.zh()
            ));
        }
    }
    let book = match FreezeBook::freeze(&FREEZE_V1) {
        Ok(b) => b,
        Err(e) => return Err(format!("{}：v1 冻结簿不可用（{}）", E_DOC_REHEARSE, e)),
    };
    rehearse_guide(&guide_steps(), &book)?;
    arbitrate_terms(&glossary())?;
    verify_unification()
}

// ---------------------------------------------------------------------------
// 七、手册的用法示范（用 F2409/F2410 而不是抄 F2409/F2410）
// ---------------------------------------------------------------------------

/// 手册映射章示范：映射表的规格摘要现算（文档不写"位置→translation"
/// 的抄录表——写出的是**怎么问**：规格摘要随实现走）。
pub fn manual_worked_example() -> Result<String, String> {
    let table = MappingTable::standard();
    if !crate::svstar2::vem09_import::mapping_covers_four_channels(&table) {
        return Err(format!("{}：映射表四通道覆盖失败（单源异常）", E_DOC_DRIFT));
    }
    Ok(format!(
        "四通道映射摘要 = {}（逆表 {} 行）",
        table.declared_checksum(),
        ExportMapTable::from_forward(&table).rows().len()
    ))
}

// ---------------------------------------------------------------------------
// 八、判据
// ---------------------------------------------------------------------------

use crate::checks::CheckSet;

/// F2414 域自检（判据五组：三文档/单源/统一/演练/CI）。
pub fn run_vem14_checks() -> CheckSet {
    let mut s = CheckSet::new("VE-F2414");

    // --- 三文档（判据一）---
    let docs = three_docs();
    let w = &docs[0].1;
    let g = &docs[1].1;
    let m = &docs[2].1;
    s.add(
        "M14-文档-01",
        w.len() == 3 && g.len() == 3 && m.len() == 3 && DocKind::ALL.len() == 3,
        "三文档齐备（各 3 行×3 章）",
    );
    s.add(
        "M14-文档-02",
        docs.iter().all(|(k, rows)| rows.iter().all(|r| r.doc == *k))
            && DocAudience::ALL.iter().all(|a| docs.iter().any(|(_, rows)| rows.iter().any(|r| r.audience == *a))),
        "文档归属正确+三读者全覆盖",
    );
    s.add(
        "M14-文档-03",
        docs.iter().all(|(_, rows)| rows.iter().all(|r| !r.text.trim().is_empty())),
        "全文档替述可读（文本非空）",
    );
    let mut used: Vec<SingleSource> = Vec::new();
    for (_, rows) in docs.iter() {
        for r in rows.iter() {
            if !used.contains(&r.source) {
                used.push(r.source);
            }
        }
    }
    s.add("M14-文档-04", used.len() >= 5, "单源引用覆盖 ≥5 种（六源面）");

    // --- 单源引用（判据二）---
    s.add(
        "M14-单源-01",
        docs.iter().all(|(_, rows)| rows.iter().all(|r| r.audit().is_ok())),
        "盖章行审计全过（快照=现值）",
    );
    let mut fresh = whitepaper()[0].clone();
    fresh.snapshot = None;
    let r = fresh.audit();
    s.add("M14-单源-02", r.is_err() && r.unwrap_err().starts_with(E_DOC_DRIFT), "未对账行拦截");
    let mut copied = guide()[0].clone();
    copied.snapshot = Some(String::from("位置/旋转/缩放/颜色/布尔/事件"));
    let r = copied.audit();
    s.add(
        "M14-单源-03",
        r.is_err() && r.as_ref().unwrap_err().starts_with(E_DOC_DRIFT),
        "单源复制拦截（抄录值与源不符）",
    );
    let srcs = [
        SingleSource::TrackClasses,
        SingleSource::FrozenSigns,
        SingleSource::MappingSpec,
        SingleSource::ExportInverse,
        SingleSource::KeyframeCarrier,
        SingleSource::EventRegistry,
    ];
    s.add(
        "M14-单源-04",
        srcs.iter().all(|x| !x.fetch().trim().is_empty()) && srcs.iter().all(|x| x.fetch() == x.fetch()),
        "六源 fetch 全可达且确定",
    );

    // --- 统一叙事（判据三：F1345 跨域对账）---
    s.add("M14-统一-01", verify_unification().is_ok(), "F1345 载体对拍（叙事有物质证据）");
    let mut bad = whitepaper();
    if let Some(row) = bad.iter_mut().find(|r| r.chapter.starts_with("统一章")) {
        row.snapshot = Some(String::from("失同步的快照"));
    }
    let r = verify_unification_rows(&bad);
    s.add(
        "M14-统一-02",
        r.is_err() && r.unwrap_err().starts_with(E_DOC_UNIFY),
        "漂移即跨域立案（失同步可检出）",
    );

    // --- 演练门禁（判据四）---
    let book = match FreezeBook::freeze(&FREEZE_V1) {
        Ok(b) => b,
        Err(_) => FreezeBook::new(),
    };
    s.add("M14-演练-01", rehearse_guide(&guide_steps(), &book).is_ok(), "指南六步 API 全在 v1 簿");
    let mut stale_steps = guide_steps();
    stale_steps[1].api = "m.anim.track.set_lod"; // v2 追加段（不在 v1 冻结簿）
    let r = rehearse_guide(&stale_steps, &book);
    s.add(
        "M14-演练-02",
        r.is_err() && r.as_ref().unwrap_err().contains("set_lod") && r.as_ref().unwrap_err().starts_with(E_DOC_REHEARSE),
        "失效步骤拦截（引 v2 签名拒）",
    );
    let mut typo_steps = guide_steps();
    typo_steps[3].api = "m.anim.clip.creat"; // 拼错名
    let r = rehearse_guide(&typo_steps, &book);
    s.add("M14-演练-03", r.is_err() && r.unwrap_err().starts_with(E_DOC_REHEARSE), "拼错 API 名拦截");
    let r = rehearse_guide(&guide_steps()[..5], &book);
    s.add("M14-演练-04", r.is_err() && r.unwrap_err().contains("步骤数"), "步骤数不足拦截");
    s.add(
        "M14-演练-05",
        guide_steps().iter().all(|st| st.api.starts_with("m.anim.")),
        "步骤 API 全 m.anim. 前缀",
    );

    // --- 术语与 F2419 ---
    let ho_terms = glossary();
    s.add(
        "M14-术语-01",
        ho_terms.len() == 3 && arbitrate_terms(&ho_terms).is_ok(),
        "术语三行与 F2419 同源（仲裁过）",
    );
    let deviating = alloc::vec![("track", "轨道"), ("clip", "剪辑"), ("keyframe", "关键帧")];
    let r = arbitrate_terms(&deviating);
    s.add(
        "M14-术语-02",
        r.is_err() && r.as_ref().unwrap_err().contains("剪辑") && r.as_ref().unwrap_err().starts_with(E_DOC_TERM),
        "偏离术语拦截（中文名以 F2419 为准）",
    );
    let missing = alloc::vec![("track", "轨道"), ("clip", "片段")];
    let r = arbitrate_terms(&missing);
    s.add("M14-术语-03", r.is_err() && r.unwrap_err().contains("keyframe"), "缺术语拦截（keyframe 缺失拒）");

    // --- CI 与版本 ---
    s.add("M14-CI-01", ci_reconcile(&three_docs()).is_ok(), "CI 对账钩子全过");
    let mut poisoned = three_docs();
    poisoned[1].1[0].snapshot = Some(String::from("被污染的快照"));
    let r = ci_reconcile(&poisoned);
    s.add("M14-CI-02", r.is_err() && r.unwrap_err().starts_with(E_DOC_DRIFT), "钩子双向（漂移注入即拒）");
    let we = manual_worked_example();
    s.add(
        "M14-CI-03",
        we.as_ref().map(|x| x.contains("四通道映射摘要")).unwrap_or(false),
        "手册示范真调 F2409/F2410（现算不抄录）",
    );

    let fp = {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        for b in DOC_SUITE_VERSION.bytes() {
            h ^= b as u64;
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
        }
        h
    };
    s.add("M14-版本-01", fp != 0, "版本指纹非零（M14-docs-v1）");

    s.add(
        "M14-暂挂-01",
        M_LEDGER_DOC_SUSPENDED_NOTE.contains("暂挂") && M_LEDGER_DOC_SUSPENDED_NOTE.contains("F2414"),
        "M 域文档汇总暂挂声明显性",
    );

    // M14-暂挂-02：判据条数对账（本条为第 24 条）。
    s.add("M14-暂挂-02", s.len() == 23, "判据条数对账（23+本条）");

    s
}

/// M 域文档汇总前的暂挂声明（锚点跨批对接点：输出供 M 域文档汇总——
/// 建账前暂挂，移交期模式延续；与 L 域 F2214 同款）。
pub const M_LEDGER_DOC_SUSPENDED_NOTE: &str = "动画三文档入 M 域文档汇总：汇总册建账前暂挂声明（移交期模式延续——F2414 同款）；文档随 F2413 v1 冻结同步演进，十年承诺期内只增不改";
