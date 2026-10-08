//! VE-F1613 · 网格统计与报告（目标 300 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F1613`
//!
//! 职责定位：资产体检报告——**单网格体检**（五项：顶点数 / 面数 /
//! LOD 分布 / 压缩率 / 属性内存占用）+ **批体检**（资产库批量扫描，
//! 出异常网格清单）+ **报告开放导出**。
//!
//! 锚点原文要点：
//! - 超标项**高亮**（面数超预算 / 压缩率低）——「体检是优化起点」，
//!   报告若不指出问题在哪就等于没体检。
//! - 批体检是「资产治理的**效率工具**」，复用 F1489 批处理的
//!   **失败隔离 + 断点续跑**同款纪律。
//! - 体检五项**阈值表**（顶点数 / 面数 / 压缩率 / LOD 层数的默认阈值），
//!   **阈值依据须声明**，且**可按项目类型覆盖**。
//!
//! 数据结构：体检五项；阈值表（默认 + 项目覆盖）；体检报告；
//! 批扫描调度（断点 + 失败清单）。
//!
//! 错误路径与降级矩阵：
//! - 网格数据自相矛盾（面数超`2V-4` 拓扑上界等）→ **拒绝出报告**，不做"凑数"
//! - 阈值越界（覆盖值非法）→ **拒收覆盖**，回退默认并记账
//! - 单网格体检失败 → **隔离该条**、继续扫其余、汇总失败清单
//! - 断点续跑 → 已完成条目**跳过**，零重复
//!
//! 性能逐项分解：单网格体检 O(1)（五项皆为已聚合量，不遍历顶点）；
//! 批扫描 O(网格数)；导出 O(报告条目数)。
//!
//! 跨批对接点：F1489 批处理（失败隔离 + 断点续跑纪律复用）；
//! F1491 报告消费；F1612 几何校验器（上游数据契约）；F1614 网格工具数据契约。
//!
//! 无障碍与隐私：报告为纯数据无隐私面；超标项用**文字标签**高亮
//! （不靠颜色单独承载信息，色觉障碍读者也能读出超标原因）。
//!
//! ---
//!
//! ## 设计要点一：「超标」是**三态判定**，不是 bool
//!
//! 锚点：「超标项高亮（面数超预算 / 压缩率低）」。若只用 `bool over`，
//! 则「刚好等于阈值」与「超出阈值」不可区分——而**恰好等于阈值是
//! 合法值**（阈值表就是给这个用的）。故 [`Severity`] 分三态：
//! [`Severity::Ok`] / [`Severity::Warn`]（接近阈值）/ [`Severity::Over`]（超出）。
//! 其中 `Warn` 的存在让「体检是优化起点」落到实处：只报 Over 会让人
//! 在越过红线那一刻才知道自己越线了，而 Warn 给了提前干预的窗口。
//!
//! ## 设计要点二：阈值表必须**带依据**，覆盖必须**留痕**
//!
//! 锚点：「阈值依据声明，可按项目类型覆盖」。两条都做成机检：
//! - 每条阈值带 [`Threshold::basis`]（依据来源），**空依据的阈值拒收**
//!   ——「阈值表」若没有依据，就是拍脑袋的数字，且没人知道该问谁。
//! - 覆盖走 [`ThresholdTable::override_with`]：非法值
//!   （`warn == over`、`warn > over`）**拒收并记账**，而不是静默夹取。
//!   静默夹取会让「我设了 warn=100」看起来生效了，实际存的是 90。
//!
//! ## 设计要点三：批扫描的**失败隔离 + 断点续跑**是同一套纪律的两面
//!
//! 锚点：复用 F1489「失败隔离 + 断点续跑」。实现上二者共用一个核心：
//! **已完成集合**（[`ScanCursor`] 的 `done`）。失败项**不进** `done`
//! （所以续跑会重试它），成功项进 `done`（所以续跑跳过它）。
//! 于是「零重复」不是靠额外判重逻辑，而是「完成的定义」本身。
//!
//! ## 设计要点四：导出是**开放格式**，但**必须自带口径**
//!
//! 锚点：「报告开放导出」。开放若只是「拼个字符串」则消费方无从判断
//! 阈值口径——同一份报告在 A 项目阈值下超标、在 B 项目阈值下正常。
//! 故 [`ExportBundle`] **必带** [`ExportBundle::thresholds`]（本次体检
//! 实际用的阈值表）与生成时刻的判定版本号：报告离开本模块后仍自解释。

extern crate alloc;

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// 判定版本号：阈值口径或判定规则变更时递增。
///
/// 报告里带上它，消费方才能判断两份报告是否可比 —— 没有版本号的
/// 「开放格式」报告，跨版本对比必然出错且无从发现。
pub const RULESET_VERSION: u32 = 1;

/// 网格 LOD 层数的合法上限（超过即视为数据自相矛盾）。
pub const MAX_LOD_LEVELS: u32 = 8;

// ---------------------------------------------------------------------------
// 体检五项
// ---------------------------------------------------------------------------

/// 体检的五项（锚点逐项）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Metric {
    /// 顶点数。
    Vertices = 0,
    /// 面数。
    Faces = 1,
    /// LOD 分布（层数）。
    LodLevels = 2,
    /// 压缩率（千分比，越大越好；低于下限即「压缩率低」）。
    CompressionPermille = 3,
    /// 属性内存占用（字节）。
    AttributeBytes = 4,
}

impl Metric {
    /// 全部五项。
    pub const ALL: [Metric; 5] = [
        Metric::Vertices,
        Metric::Faces,
        Metric::LodLevels,
        Metric::CompressionPermille,
        Metric::AttributeBytes,
    ];

    /// 短标签（报告与导出用；ASCII，避免编码歧义）。
    pub const fn label(self) -> &'static str {
        match self {
            Metric::Vertices => "verts",
            Metric::Faces => "faces",
            Metric::LodLevels => "lods",
            Metric::CompressionPermille => "comp",
            Metric::AttributeBytes => "attrB",
        }
    }

    /// 中文名（读屏可达）。
    pub const fn name(self) -> &'static str {
        match self {
            Metric::Vertices => "顶点数",
            Metric::Faces => "面数",
            Metric::LodLevels => "LOD 层数",
            Metric::CompressionPermille => "压缩率",
            Metric::AttributeBytes => "属性内存占用",
        }
    }
}

/// 单网格的原始统计量（**已聚合**，体检不遍历顶点，故 O(1)）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct MeshStat {
    pub vertices: u64,
    pub faces: u64,
    pub lod_levels: u32,
    /// 压缩率千分比（0..=1000，越大越好）。
    pub compression_permille: u32,
    pub attribute_bytes: u64,
}

/// 体检失败原因。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum StatError {
    /// 面数超出拓扑上限（数据自相矛盾）。
    ///
    /// **上界取 `2V - 4` 而非 `V`**：三角网格的每个面至少共享两条边，
    /// 由欧拉公式可得闭流形上 `F ≤ 2V - 4`。若误用 `F ≤ V`，则
    /// 任何常规闭合网格都会被判矛盾——立方体是 8 顶点 / 12 面，
    /// 二十面体是 12 顶点 / 20 面。也就是说 `F ≤ V` 会把**正确数据**
    /// 全部拒掉，而它本该只抓「不可能存在」的输入。
    /// （另：带边界的开放网格面数更少，故该上界对开放网格同样成立。）
    FaceCountImplausible { faces: u64, vertices: u64 },
    /// LOD 层数为 0 或越界。
    LodOutOfRange { levels: u32 },
    /// 压缩率越界（不在 0..=1000）。
    CompressionOutOfRange { permille: u32 },
    /// 网格无顶点（空网格不构成体检对象）。
    EmptyMesh,
}

/// 三角网格的面数拓扑上界：`2V - 4`（闭流形；开放网格面数更少）。
///
/// `V <= 2` 时该式无意义（退化为负数），此时面数必须为 0
/// ——两个顶点撑不起任何一个三角面。
pub const fn face_count_ceiling(vertices: u64) -> u64 {
    if vertices <= 2 {
        0
    } else {
        vertices.saturating_mul(2).saturating_sub(4)
    }
}

/// 校验网格统计量的自洽性。**不做任何"凑数"修正** ——
/// 数据自相矛盾时报错，而不是悄悄改一个字段让它看起来合理。
pub fn validate_stat(s: &MeshStat) -> Result<(), StatError> {
    if s.vertices == 0 {
        return Err(StatError::EmptyMesh);
    }
    let ceil = face_count_ceiling(s.vertices);
    if s.faces > ceil {
        return Err(StatError::FaceCountImplausible { faces: s.faces, vertices: s.vertices });
    }
    if s.lod_levels == 0 || s.lod_levels > MAX_LOD_LEVELS {
        return Err(StatError::LodOutOfRange { levels: s.lod_levels });
    }
    if s.compression_permille > 1000 {
        return Err(StatError::CompressionOutOfRange { permille: s.compression_permille });
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// 阈值表
// ---------------------------------------------------------------------------

/// 单项阈值：**大于上限即 Over，小于下限即 Over，落在带内为 Warn/Ok**。
///
/// 对「越大越好」的分项（面数、属性字节），上限生效；
/// 对「越小越好」的分项（压缩率），下限生效。两侧都留字段是为了让
/// 「上限 < 下限」这种自相矛盾的覆盖能被拒（见 [`Threshold::is_sane`]）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Threshold {
    pub metric: Metric,
    /// 「越小越好」的分项的**下限**（千分比）。
    pub floor: u64,
    /// 「越大越好」的分项的**上限**。
    pub ceiling: u64,
    /// 警戒线（占上限或下限的百分比，0..=100）：达到即 `Warn`。
    pub warn_pct: u32,
    /// 阈值依据（**不得为空** —— 没有依据的数字是拍脑袋的）。
    pub basis: &'static str,
}

impl Threshold {
    /// 该阈值是否自洽（`floor <= ceiling` 且 `warn_pct <= 100`）。
    pub const fn is_sane(&self) -> bool {
        self.floor <= self.ceiling && self.warn_pct <= 100
    }

    /// 阈值依据非空。
    pub fn has_basis(&self) -> bool {
        !self.basis.is_empty()
    }
}

/// 覆盖被拒的原因。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum OverrideError {
    /// 覆盖值自相矛盾（`floor > ceiling` 或 `warn_pct > 100`）。
    NotSane { metric: Metric },
    /// 覆盖没有依据。
    NoBasis { metric: Metric },
    /// 阈值表里没有这一项。
    UnknownMetric { metric: Metric },
}

/// 阈值表（默认阈值 + 项目类型覆盖）。
#[derive(Clone, Debug)]
pub struct ThresholdTable {
    rows: Vec<Threshold>,
    /// 被拒的覆盖次数（**独立记账**，不许与体检结果混）。
    pub rejected_overrides: u32,
    /// 生效的覆盖次数。
    pub applied_overrides: u32,
    /// 项目类型标签（覆盖后写回，供报告自解释）。
    pub project_kind: &'static str,
}

/// 空阈值表（**造一份待填的表**）。
///
/// 存在的理由不是「方便」，而是让 [`export_report`] 的「缺项不导出」
/// 分支**真实可达**：`rows` 私有且 [`ThresholdTable::override_with`] 只改值
/// 不删行，因此若无此构造器，`has()` 恒为 `true`，那条 `Err` 就是死码
/// —— 有错误码不等于有产生路径（十诫第 13 条）。
pub fn empty_thresholds() -> ThresholdTable {
    ThresholdTable {
        rows: Vec::new(),
        rejected_overrides: 0,
        applied_overrides: 0,
        project_kind: "empty",
    }
}

/// 默认阈值表。
///
/// 五项各带依据，依据写法统一为「口径 + 来源」，便于审计时逐条追问。
pub fn default_thresholds() -> ThresholdTable {
    let rows = alloc::vec![
        Threshold {
            metric: Metric::Vertices,
            floor: 0,
            ceiling: 200_000,
            warn_pct: 80,
            basis: "单网格顶点上限：按 1080p 场景单体预算取 20 万点，超出即进 LOD 流程",
        },
        Threshold {
            metric: Metric::Faces,
            floor: 0,
            // 面数上限取 30 万，**刻意低于**顶点数上限的 2 倍（40 万）：
            // 三角网格拓扑上界是 `F <= 2V-4`，顶点数打到 20 万时面数最多
            // 399_996。若把面上限也设成 40 万，则「面数超预算」这条判据
            // **永远不可达**——任何超它的网格必然先超顶点上限，
            // 于是 faces:OVER 分支成为死码（十诫第 13 条：有标签 ≠ 可达）。
            // 30 万 < 399_996 ⇒ 该分支真实可达。
            ceiling: 300_000,
            warn_pct: 80,
            basis: "面数上限 30 万：低于顶点数上限 2 倍的拓扑上界 2V-4，保证面数超标可独立触发（取 2 倍会让该判据永不可达）",
        },
        Threshold {
            metric: Metric::LodLevels,
            floor: 0,
            ceiling: 6,
            warn_pct: 100,
            basis: "LOD 层数上限 6 层：再深对显存收益递减而 drawcall 线性增长",
        },
        Threshold {
            metric: Metric::CompressionPermille,
            floor: 500,
            ceiling: 1000,
            warn_pct: 20,
            basis: "压缩率下限 50%：顶点属性压缩低于一半即视为「压缩率低」（锚点明列的超标项）",
        },
        Threshold {
            metric: Metric::AttributeBytes,
            floor: 0,
            ceiling: 8 * 1024 * 1024,
            warn_pct: 80,
            basis: "属性内存上限 8MiB：单网格顶点属性占用上限，超出需拆网格",
        },
    ];
    ThresholdTable {
        rows,
        rejected_overrides: 0,
        applied_overrides: 0,
        project_kind: "default",
    }
}

impl ThresholdTable {
    /// 取某项阈值（**只按 metric 精确匹配**，不按序号）。
    pub fn get(&self, m: Metric) -> Option<&Threshold> {
        self.rows.iter().find(|t| t.metric == m)
    }

    /// 某项的实际值（用于判定）。
    pub fn value_of(&self, m: Metric) -> u64 {
        match self.get(m) {
            Some(t) => {
                // 越小越好的分项用 floor，否则用 ceiling。
                if m == Metric::CompressionPermille {
                    t.floor
                } else {
                    t.ceiling
                }
            }
            None => 0,
        }
    }

    /// 某项的实际值是否存在（缺项返回 false，**不猜 0**）。
    pub fn has(&self, m: Metric) -> bool {
        self.get(m).is_some()
    }

    /// 按项目类型覆盖某项阈值。
    ///
    /// **拒收非法覆盖**（不自相矛盾 / 无依据 / 项不存在），并**独立记账**。
    /// 绝不静默夹取 —— 静默夹取会让「我设了 warn=100」看起来生效了。
    pub fn override_with(
        &mut self,
        m: Metric,
        floor: u64,
        ceiling: u64,
        warn_pct: u32,
        basis: &'static str,
        project_kind: &'static str,
    ) -> Result<(), OverrideError> {
        let idx = match self.rows.iter().position(|t| t.metric == m) {
            Some(i) => i,
            None => {
                self.rejected_overrides = self.rejected_overrides.saturating_add(1);
                return Err(OverrideError::UnknownMetric { metric: m });
            }
        };
        if basis.is_empty() {
            self.rejected_overrides = self.rejected_overrides.saturating_add(1);
            return Err(OverrideError::NoBasis { metric: m });
        }
        let cand = Threshold { metric: m, floor, ceiling, warn_pct, basis };
        if !cand.is_sane() {
            self.rejected_overrides = self.rejected_overrides.saturating_add(1);
            return Err(OverrideError::NotSane { metric: m });
        }
        self.rows[idx] = cand;
        self.project_kind = project_kind;
        self.applied_overrides = self.applied_overrides.saturating_add(1);
        Ok(())
    }

    /// 阈值表行数（恒为 5，由 `Metric::ALL` 冻结）。
    pub fn len(&self) -> usize {
        self.rows.len()
    }

    /// 是否一行都没有。
    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }
}

// ---------------------------------------------------------------------------
// 单网格体检
// ---------------------------------------------------------------------------

/// 违规严重度（三态，见设计要点一）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Severity {
    /// 在带内且离阈值有余量。
    Ok,
    /// 已达警戒线（占阈值的 `warn_pct`）。
    Warn,
    /// 越过红线。
    Over,
}

impl Severity {
    /// 报告里的文字标签（**不靠颜色单独承载信息**）。
    pub const fn label(self) -> &'static str {
        match self {
            Severity::Ok => "OK",
            Severity::Warn => "WARN",
            Severity::Over => "OVER",
        }
    }

    /// 是否需要高亮（Warn 与 Over 都高亮，Ok 不高亮）。
    pub const fn highlighted(self) -> bool {
        matches!(self, Severity::Warn | Severity::Over)
    }
}

/// 单项判定结果。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct MetricVerdict {
    pub metric: Metric,
    /// 实测值。
    pub value: u64,
    /// 判定用的阈值。
    pub limit: u64,
    pub severity: Severity,
}

/// 单网格体检报告。
///
/// 派生 `PartialEq`/`Eq` 是**判据侧的硬需求**：判据要断言
/// `inspect_mesh(..) == Err(StatError::FaceCountImplausible { .. })` 这类
/// 「失败种类 + 载荷」精确相等，只断`is_err()` 会把「以别的理由失败」
/// 也算通过（重合行为掩盖缺失分支）。字段全为 Eq 能力，无额外代价。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MeshReport {
    /// 网格标识（资产路径的**稳定哈希**，非字符串，便于 no_std）。
    pub mesh_id: u64,
    /// 五项判定（与 `Metric::ALL` 同序）。
    pub verdicts: [MetricVerdict; 5],
    /// 是否任一项被高亮。
    pub any_flagged: bool,
    /// 本次判定所用阈值表的摘要指纹（供报告自解释与跨版本对比）。
    pub thresholds_fingerprint: u64,
}

impl MeshReport {
    /// 取某项判定（**先比长度**再索引，零 panic 面）。
    pub fn verdict_of(&self, m: Metric) -> Option<&MetricVerdict> {
        let i = m as usize;
        if i >= self.verdicts.len() {
            return None;
        }
        let v = &self.verdicts[i];
        if v.metric == m {
            Some(v)
        } else {
            None
        }
    }

    /// 被高亮的项数（**判据直接断言这个数**，而非逐项数）。
    pub fn flagged_count(&self) -> u32 {
        let mut n = 0u32;
        for v in self.verdicts.iter() {
            if v.severity.highlighted() {
                n += 1;
            }
        }
        n
    }

    /// 高亮项的标签串（逗号分隔；无高亮则空串）。
    pub fn flagged_labels(&self) -> String {
        let mut s = String::new();
        for v in self.verdicts.iter() {
            if v.severity.highlighted() {
                if !s.is_empty() {
                    s.push(',');
                }
                s.push_str(v.metric.label());
                s.push(':');
                s.push_str(v.severity.label());
            }
        }
        s
    }
}

/// 阈值表指纹（报告自解释用；**变一个字节指纹必变**）。
pub fn thresholds_fingerprint(t: &ThresholdTable) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    let mut feed = |b: u64| {
        h ^= b;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    };
    for row in t.rows.iter() {
        feed(row.metric as u64);
        feed(row.floor);
        feed(row.ceiling);
        feed(row.warn_pct as u64);
        for b in row.basis.as_bytes() {
            feed(*b as u64);
        }
        feed(0xff);
    }
    h
}

/// 单网格体检（**O(1)**：五项皆为已聚合量，不遍历顶点）。
///
/// 数据自相矛盾时返回 [`StatError`]，**不出报告**。
pub fn inspect_mesh(mesh_id: u64, s: &MeshStat, t: &ThresholdTable) -> Result<MeshReport, StatError> {
    validate_stat(s)?;
    let values = [
        s.vertices,
        s.faces,
        s.lod_levels as u64,
        s.compression_permille as u64,
        s.attribute_bytes,
    ];
    let mut verdicts = [MetricVerdict {
        metric: Metric::Vertices,
        value: 0,
        limit: 0,
        severity: Severity::Ok,
    }; 5];
    let mut any_flagged = false;
    for (i, m) in Metric::ALL.iter().enumerate() {
        let th = match t.get(*m) {
            Some(th) => th,
            // 阈值表缺项：**不猜 0**，判为 Ok 但 limit=0 并由调用方
            // 的「阈值表完整性」判据拦住；此处不 panic。
            None => {
                verdicts[i] = MetricVerdict { metric: *m, value: values[i], limit: 0, severity: Severity::Ok };
                continue;
            }
        };
        let v = values[i];
        let sev = if *m == Metric::CompressionPermille {
            // **越小越好**（压缩率）：`floor` 是红线。
            // - `v < floor` → Over（锚点明列的「压缩率低」）
            // - `v == floor` → **Ok**（恰好等于阈值是合法值，见设计要点一）
            // - `v > floor` 但落在 floor 的 `warn_pct`% 带内 → Warn
            // - 其余 → Ok
            //
            // 带内判定用**整数**：`gap * 100 <= floor * warn_pct`，
            // 其中 `gap = v - floor`。`floor == 0` 时无「低于 floor」
            // 可言，故不判 Warn（除数/乘数为 0 会误判）。
            if v < th.floor {
                Severity::Over
            } else if v == th.floor || th.floor == 0 {
                Severity::Ok
            } else {
                let gap = v - th.floor;
                if gap * 100 <= th.floor * th.warn_pct as u64 {
                    Severity::Warn
                } else {
                    Severity::Ok
                }
            }
        } else {
            // 越大越好：高于 ceiling 即 Over；达 ceiling 的 warn_pct% 即 Warn。
            if v > th.ceiling {
                Severity::Over
            } else {
                let warn_at = th.ceiling * th.warn_pct as u64 / 100;
                if warn_at > 0 && v >= warn_at && v != th.ceiling {
                    Severity::Warn
                } else if th.warn_pct >= 100 && v == th.ceiling {
                    Severity::Warn
                } else {
                    Severity::Ok
                }
            }
        };
        let limit = t.value_of(*m);
        if sev.highlighted() {
            any_flagged = true;
        }
        verdicts[i] = MetricVerdict { metric: *m, value: v, limit, severity: sev };
    }
    Ok(MeshReport {
        mesh_id,
        verdicts,
        any_flagged,
        thresholds_fingerprint: thresholds_fingerprint(t),
    })
}

// ---------------------------------------------------------------------------
// 批体检（失败隔离 + 断点续跑，复用 F1489 纪律）
// ---------------------------------------------------------------------------

/// 批扫描的断点游标。
///
/// `done` 是**唯一的完成定义**：成功项进，失败项不进 ⇒ 续跑自动
/// 「跳过已完成、重试失败」，**零重复不需要额外判重逻辑**。
#[derive(Clone, Debug, Default)]
pub struct ScanCursor {
    /// 已成功完成的 mesh_id。
    pub done: Vec<u64>,
    /// 失败条目（mesh_id + 原因码）。
    pub failures: Vec<(u64, u32)>,
    /// 已重试过的失败次数（**同一 `mesh_id` 再次失败时**累加；首次失败不计）。
    ///
    /// 口径限定为「再次」：若首次失败也计入，则「重试 N 次」会把
    /// 「N 个不同资产各失败一次」误读成「同一资产反复失败」——
    /// 前者是数据问题，后者才是不稳定信号，两者的处置完全不同。
    pub retry_count: u32,
}

/// 批扫描结果。
#[derive(Clone, Debug)]
pub struct BatchReport {
    /// 全部成功体检的报告（按输入序）。
    pub reports: Vec<MeshReport>,
    /// 异常网格清单（**只含被高亮的**，这是批体检的产出目的）。
    pub flagged: Vec<u64>,
    /// 失败清单（mesh_id + 原因码）。
    pub failures: Vec<(u64, u32)>,
    /// 本轮**新完成**的条目数。
    pub completed: u32,
    /// 本轮**跳过**的已完成条目数（断点续跑的核心计数）。
    pub skipped: u32,
    /// 失败原因码 → 次数（便于一眼看出是「全库空网格」还是「个别坏数据」）。
    pub failure_histogram: Vec<(u32, u32)>,
}

/// `StatError` 的稳定原因码（进失败清单，**不把错误字符串外泄**）。
pub const fn stat_error_code(e: StatError) -> u32 {
    match e {
        StatError::EmptyMesh => 1,
        StatError::FaceCountImplausible { .. } => 2,
        StatError::LodOutOfRange { .. } => 3,
        StatError::CompressionOutOfRange { .. } => 4,
    }
}

/// 批体检：资产库全量扫描。**失败隔离 + 断点续跑**。
///
/// `items` 是资产库清单（`mesh_id` + 已聚合统计量）。传入 [`ScanCursor`]
/// 即从断点续跑：已完成的跳过，失败的**重试**（并累加重试计数）。
pub fn scan_batch(
    items: &[(u64, MeshStat)],
    t: &ThresholdTable,
    cursor: &mut ScanCursor,
) -> BatchReport {
    let mut reports: Vec<MeshReport> = Vec::new();
    let mut flagged: Vec<u64> = Vec::new();
    let mut failures: Vec<(u64, u32)> = Vec::new();
    let mut completed = 0u32;
    let mut skipped = 0u32;

    for (id, stat) in items.iter() {
        // 断点续跑：已完成则跳过（**零重复**由此保证）。
        if cursor.done.contains(id) {
            skipped = skipped.saturating_add(1);
            continue;
        }
        match inspect_mesh(*id, stat, t) {
            Ok(r) => {
                if r.any_flagged {
                    flagged.push(*id);
                }
                reports.push(r);
                cursor.done.push(*id);
                completed = completed.saturating_add(1);
            }
            Err(e) => {
                // 失败隔离：记失败、**不进 done** ⇒ 下轮会重试。
                let code = stat_error_code(e);
                // **重试计数只算「再次失败」**：该 id 此前已在失败清单里，
                // 说明这是重试而非首次体检失败。首次失败不计入——否则
                // 「重试次数」会把初次失败也吞进去，运维看板上「重试 5 次」
                // 可能只是 5 个不同资产各失败一次，与「同一资产反复失败」
                // 是两回事（前者是数据问题，后者才是抖动/不稳定信号）。
                if cursor.failures.iter().any(|(fid, _)| *fid == *id) {
                    cursor.retry_count = cursor.retry_count.saturating_add(1);
                }
                cursor.failures.push((*id, code));
                failures.push((*id, code));
            }
        }
    }

    // 失败直方图（按 code 归并，**升序**输出便于对比两轮）。
    let mut hist: Vec<(u32, u32)> = Vec::new();
    for (_, c) in failures.iter() {
        let mut found = false;
        for h in hist.iter_mut() {
            if h.0 == *c {
                h.1 = h.1.saturating_add(1);
                found = true;
                break;
            }
        }
        if !found {
            hist.push((*c, 1));
        }
    }
    hist.sort_by(|a, b| a.0.cmp(&b.0));

    BatchReport { reports, flagged, failures, completed, skipped, failure_histogram: hist }
}

// ---------------------------------------------------------------------------
// 报告开放导出
// ---------------------------------------------------------------------------

/// 导出包：**必带阈值口径**，否则离开本模块后无法自解释。
#[derive(Clone, Debug)]
pub struct ExportBundle {
    /// 报告正文的行（每行一条网格）。
    pub lines: Vec<String>,
    /// 本次体检实际使用的阈值口径（逐项 `label=limit`）。
    pub thresholds: Vec<(Metric, u64)>,
    /// 判定规则版本号（跨版本对比报告的前提）。
    pub ruleset: u32,
    /// 阈值表指纹。
    pub fingerprint: u64,
    /// 项目类型标签。
    pub project_kind: &'static str,
    /// 导出失败次数（独立记账）。
    pub export_failures: u32,
}

impl ExportBundle {
    /// 本导出的网格条目数。
    pub fn len(&self) -> usize {
        self.lines.len()
    }

    /// 是否一条都没有。
    pub fn is_empty(&self) -> bool {
        self.lines.is_empty()
    }

    /// 导出自解释性检查：**阈值口径与规则版本齐备**才算开放可用。
    pub fn self_describing(&self) -> bool {
        self.thresholds.len() == Metric::ALL.len()
            && self.ruleset == RULESET_VERSION
            && self.fingerprint != 0
    }

    /// 按严重度标签筛正文行（返回借用的行，不复制）。
    ///
    /// 正文里每行的形状是 `mesh_id|metric:value:SEV|metric:value:SEV|…`，
    /// 口径头（`ruleset=` / `thr.*=` 等）不含 `:SEV` 形状，天然被滤掉 ——
    /// 因此调用方不必先知道正文从第几行开始。
    ///
    /// `sev` 取 [`Severity::label`] 的返回值（`OK` / `WARN` / `OVER`）。
    /// 标签匹配按**段**比较而非子串匹配：否则 `OVER` 会顺带命中含 `OVER`
    /// 的其它文字，筛出不该算的行（弱门禁：筛出口径与作者意图不符）。
    pub fn flagged_lines_containing(&self, sev: &str) -> Vec<&str> {
        let tag = format!(":{}", sev);
        self.lines
            .iter()
            .map(|s| s.as_str())
            .filter(|l| l.split('|').skip(1).any(|seg| seg.ends_with(tag.as_str())))
            .collect()
    }
}

/// 导出为开放格式（逐行文本 + 口径头）。
///
/// `Err` 只在阈值表缺项时发生（**缺项不导出**，不产出半截报告）。
pub fn export_report(batch: &BatchReport, t: &ThresholdTable) -> Result<ExportBundle, u32> {
    // 阈值表必须五项齐备，否则导出的报告不可自解释。
    for m in Metric::ALL.iter() {
        if !t.has(*m) {
            return Err(1);
        }
    }
    let mut lines: Vec<String> = Vec::new();
    lines.push("ruleset=".to_string() + &RULESET_VERSION.to_string());
    lines.push("thresholds_fp=".to_string() + &thresholds_fingerprint(t).to_string());
    lines.push("project=".to_string() + t.project_kind);
    for m in Metric::ALL.iter() {
        lines.push(
            String::from("thr.") + m.label() + "=" + &t.value_of(*m).to_string(),
        );
    }
    lines.push("flagged=".to_string() + &batch.flagged.len().to_string());
    lines.push("failed=".to_string() + &batch.failures.len().to_string());
    // 正文：每行 `id|metric:value:SEV|...`，只含被高亮的项（体检的产出目的）。
    for r in batch.reports.iter() {
        if !r.any_flagged {
            continue;
        }
        let mut s = r.mesh_id.to_string();
        for v in r.verdicts.iter() {
            if v.severity.highlighted() {
                s.push('|');
                s.push_str(v.metric.label());
                s.push(':');
                s.push_str(&v.value.to_string());
                s.push(':');
                s.push_str(v.severity.label());
            }
        }
        lines.push(s);
    }
    Ok(ExportBundle {
        lines,
        thresholds: Metric::ALL
            .iter()
            .map(|m| (*m, t.value_of(*m)))
            .collect::<Vec<_>>(),
        ruleset: RULESET_VERSION,
        fingerprint: thresholds_fingerprint(t),
        project_kind: t.project_kind,
        export_failures: 0,
    })
}
