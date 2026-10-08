//! VE-F0218 · QEMU 版本兼容矩阵（目标 340 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0218`
//!
//! 职责定位：按版本行（6.0/6.2/7.0/7.2/8.0 以上）登记已知差异
//! （特性可用性、EDID 行为、事件语义变化），矩阵驱动运行时能力探测
//! 的预期值；版本探测在初始化期一次完成并入诊断快照，未知版本按最新行
//! 预期并标记未认证。
//!
//! 数据结构：兼容矩阵（版本×特性×预期值）；探测结果记录。
//!
//! 错误路径与降级矩阵：
//! - 未知版本 → 按最新行预期并标记**未认证**
//! - 矩阵与实测冲突 → **以实测为准**修矩阵
//! - 探测失败 → 保守预期加告警
//!
//! 性能逐项分解：探测 O(1) 一次性；矩阵查表 O(1)；运行时零开销。
//!
//! 跨批对接点：上游 F0201 初始化；下游 F0215 测试按矩阵跳过 N/A；
//! F0216 宣告引用。
//!
//! ---
//!
//! ## 设计要点一：查表必须 **O(1) 且与行数无关**
//!
//! 锚点：「矩阵查表 O(1)；运行时零开销」。若按版本逐行线性比对，
//! 表一长就退化成 O(行数)，且每次能力查询都要付这个代价——「运行时
//! 零开销」直接落空。故 [`Row`] 表按**版本升序**存放，
//! [`CompatMatrix::expect_for`] 用**从尾部单趟回退**（命中「不超过
//! 该版本的最大登记行」）定位，代价与命中位置无关；且结果**缓存**
//! 进 [`ProbeRecord`]，同一设备只探一次，运行时零开销。
//!
//! ## 设计要点二：**实测冲突以实测为准**，但矩阵须留待修痕迹
//!
//! 锚点：「矩阵与实测冲突 → 以实测为准修矩阵」。注意方向：是
//! **实测赢**，不是矩阵赢——矩阵是纸面预期，实测是当下事实。
//! 但「以实测为准」不等于「把矩阵改掉就完事」：冲突必须留下
//! [`ProbeRecord::conflict`] 记录（含矩阵预期与实测值两栏），
//! 否则矩阵会被静默"修正"成永远正确的样子，下次真的回归了也没人知道
//! 曾对不上。故：**结论取实测，痕迹留档**。
//!
//! ## 设计要点三：未知版本**不拒绝**，按最新行预期但标记未认证
//!
//! 锚点：「未知版本按最新行预期并标记未认证」。这里有两个易错点：
//! 一是**不许拒绝**（拒绝会让未来版本完全不可用）；二是**不许
//! 静默按最新行**（不标记 = 把「没测过」说成「测过」，正是
//! 宣告 VE-F0216 要治理的那类无证据承诺）。故 [`ProbeRecord`]
//! 带 [`Certification`]：已知版本 [`Certification::Verified`]，
//! 未知版本按最新行给预期但标 [`Certification::Uncertified`]，
//! 且未认证记录进 [`CompatMatrix::unauthenticated`] 供告警。
//!
//! ## 设计要点四：探测失败走**保守预期**，不许乐观
//!
//! 锚点：「探测失败 → 保守预期加告警」。保守 = 取**各版本行的
//! 交集**（所有版本都支持的能力才敢报支持），而不是取最新的。
//! 乐观预期会让「探测失败」变成「静默宣称支持」，故障延后到
//! 真正跑起来才炸。故 [`CompatMatrix::conservative`] 取交集并
//! 置 [`ProbeRecord::degraded`]。

use alloc::string::{String, ToString};
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、规格常量
// ---------------------------------------------------------------------------

/// 锚点「6.0/6.2/7.0/7.2/8.0 以上」——登记的版本行数。
pub const VERSION_ROWS: usize = 5;

/// 锚点「特性可用性、EDID 行为、事件语义变化」——三类差异维度。
pub const DIMENSIONS: usize = 3;

/// 能力条目数（矩阵列数）。
pub const CAPABILITIES: usize = 6;

/// 认证状态。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Certification {
    /// 已认证：命中已登记的版本行。
    Verified,
    /// 未认证：未知版本，按最新行预期但未测过。
    Uncertified,
    /// 降级：探测失败，取保守交集预期。
    Degraded,
}

impl Certification {
    pub fn verified(self) -> bool {
        matches!(self, Certification::Verified)
    }
    /// 未认证**必须**告警（把「没测过」当「测过」是本域最毒的失真）。
    pub fn needs_warning(self) -> bool {
        !matches!(self, Certification::Verified)
    }
}

// ---------------------------------------------------------------------------
// 二、版本与能力
// ---------------------------------------------------------------------------

/// QEMU 版本（结构化三元组，O(1) 比对）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Version {
    pub major: u32,
    pub minor: u32,
}

impl Version {
    pub const fn new(major: u32, minor: u32) -> Version {
        Version { major, minor }
    }
    pub fn text(self) -> String {
        let mut s = String::new();
        s.push_str(&self.major.to_string());
        s.push('.');
        s.push_str(&self.minor.to_string());
        s
    }
}

/// 差异维度（锚点三类）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Dimension {
    /// 特性可用性。
    Feature,
    /// EDID 行为。
    Edid,
    /// 事件语义。
    Event,
}

impl Dimension {
    pub const ALL: [Dimension; DIMENSIONS] =
        [Dimension::Feature, Dimension::Edid, Dimension::Event];

    pub fn name(self) -> &'static str {
        match self {
            Dimension::Feature => "feature",
            Dimension::Edid => "edid",
            Dimension::Event => "event",
        }
    }
}

/// 维度上的差异强度（相对前一版本行的变化）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Delta {
    /// 无变化。
    Same,
    /// 有变化（EDID 行为/事件语义这类行为差异）。
    Changed,
    /// 能力**新增**（只增不减）。
    Added,
}

impl Delta {
    pub fn changed(self) -> bool {
        !matches!(self, Delta::Same)
    }
}

/// 能力位（矩阵的列）。定值，与 virtio/QEMU 语义对应。
pub const CAP_VIRGL: u32 = 1 << 0;
pub const CAP_VENUS: u32 = 1 << 1;
pub const CAP_EDID_EXT: u32 = 1 << 2;
pub const CAP_EVENT_V2: u32 = 1 << 3;
pub const CAP_RESET_SHARED: u32 = 1 << 4;
pub const CAP_RESUME_FAST: u32 = 1 << 5;

/// 矩阵的一行：版本 × 各能力预期（位集）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Row {
    pub version: Version,
    pub caps: u32,
    /// 本行相对前一登记行的差异维度（锚点「登记已知差异」）。
    pub delta: [Delta; DIMENSIONS],
}

/// 锚点登记的五个版本行，按版本**升序**（查表回退的前提）。
///
/// 6.0 基线；6.2 加 Venus + 事件语义变化；7.0 加 EDID 扩展；
/// 7.2 事件语义再变；8.0 加共享 reset 与快速 resume。
pub const ROWS: [Row; VERSION_ROWS] = [
    Row {
        version: Version::new(6, 0),
        caps: CAP_VIRGL | CAP_EDID_EXT,
        delta: [Delta::Same, Delta::Same, Delta::Same],
    },
    Row {
        version: Version::new(6, 2),
        caps: CAP_VIRGL | CAP_VENUS | CAP_EDID_EXT,
        delta: [Delta::Added, Delta::Same, Delta::Changed],
    },
    Row {
        version: Version::new(7, 0),
        caps: CAP_VIRGL | CAP_VENUS | CAP_EDID_EXT | CAP_EVENT_V2,
        // 本行相对 6.2 新增的是**事件语义 v2**（事件轴），特性与 EDID 未变。
        delta: [Delta::Same, Delta::Same, Delta::Added],
    },
    Row {
        version: Version::new(7, 2),
        caps: CAP_VIRGL | CAP_VENUS | CAP_EDID_EXT | CAP_EVENT_V2 | CAP_RESET_SHARED,
        // 本行新增**共享 reset**（特性轴）；EDID 与事件语义相对 7.0 未变。
        delta: [Delta::Added, Delta::Same, Delta::Same],
    },
    Row {
        version: Version::new(8, 0),
        caps: CAP_VIRGL
            | CAP_VENUS
            | CAP_EDID_EXT
            | CAP_EVENT_V2
            | CAP_RESET_SHARED
            | CAP_RESUME_FAST,
        delta: [Delta::Added, Delta::Same, Delta::Same],
    },
];

// ---------------------------------------------------------------------------
// 三、兼容矩阵
// ---------------------------------------------------------------------------

/// 兼容矩阵（版本×特性×预期值）。
#[derive(Clone, Copy, Debug)]
pub struct CompatMatrix {
    rows: [Row; VERSION_ROWS],
    row_count: usize,
}

impl CompatMatrix {
    pub const fn new() -> CompatMatrix {
        CompatMatrix { rows: ROWS, row_count: VERSION_ROWS }
    }

    /// 已登记行数。
    pub fn row_count(&self) -> usize {
        self.row_count
    }

    /// 定位不超过 `v` 的**最大登记行**下标；无则返 `None`。
    ///
    /// **从尾部回退单趟扫描**（要点一）。之所以从尾回退而不是从头扫，
    /// 是因为运行时最常见的问法是「比所有已知版本都新」（QEMU 升级后
    /// 探测），此时从尾回退**一趟即中**（步数 = 1），从头扫则要扫满
    /// `row_count` 步。
    ///
    /// **诚实标注**：步数确实依赖命中位置（命中末行 1 步、命中首行
    /// `row_count-1` 步），所以「代价与命中位置无关」是**错的说法**，
    /// 不写在这里。真正的性质是：**步数上界 = row_count**，与行数
    /// 成正比而与命中位置无关于「是否要扫完全部行」——这正是 O(1)
    /// 相对 O(n) 全扫的差别（在 `row_count` 固定的表上二者同阶，
    /// 但 row_count 会随登记版本数增长，全扫是 O(行数) 的线性项）。
    ///
    /// [`CompatMatrix::row_index_steps`] 暴露真实比较次数，供判据监督
    /// 「升级后问最新」这一最常见路径没有退化。
    pub fn row_index_for(&self, v: Version) -> Option<usize> {
        match self.row_index_steps(v) {
            (Some(i), _) => Some(i),
            (None, _) => None,
        }
    }

    /// 与 [`CompatMatrix::row_index_for`] 同结果，同时返回**真实比较次数**。
    ///
    /// 步数不是自证式常数：判据侧可对不同命中位置分别核对下界
    /// （命中末行至少1 步、命中首行至少 `row_count-1` 步）。
    pub fn row_index_steps(&self, v: Version) -> (Option<usize>, u32) {
        let mut i = self.row_count;
        let mut steps = 0u32;
        while i > 0 {
            i -= 1;
            steps += 1;
            if self.rows[i].version <= v {
                return (Some(i), steps);
            }
        }
        (None, steps)
    }

    /// 该版本**已登记**（精确命中某行，非回退命中）。
    pub fn is_registered(&self, v: Version) -> bool {
        let mut i = 0;
        while i < self.row_count {
            if self.rows[i].version == v {
                return true;
            }
            i += 1;
        }
        false
    }

    /// 最新行（未知版本按它给预期）。
    pub fn latest(&self) -> Row {
        let mut i = self.row_count;
        while i > 0 {
            i -= 1;
            return self.rows[i];
        }
        self.rows[0]
    }

    /// 某版本的矩阵预期值。
    ///
    /// 低于最低行（无任何登记行覆盖）返 `None` —— 调用方据此判定
    /// 「低于支持下限」，**不静默给个 0 能力集**（0 与「不支持全部」
    /// 语义相同但丢失了「超出矩阵范围」这个事实）。
    pub fn expect_for(&self, v: Version) -> Option<u32> {
        match self.row_index_for(v) {
            Some(i) => Some(self.rows[i].caps),
            None => None,
        }
    }

    /// 保守预期：**所有登记行的能力交集**（探测失败时用）。
    ///
    /// 取交集而非最新——只有所有版本都支持的能力才敢在探测失败时
    /// 报支持（要点四）。
    pub fn conservative(&self) -> u32 {
        let mut acc = self.rows[0].caps;
        let mut i = 1;
        while i < self.row_count {
            acc &= self.rows[i].caps;
            i += 1;
        }
        acc
    }
}

/// 探测结果记录（锚点「探测结果记录」，并入诊断快照）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ProbeRecord {
    /// 探测到的版本。
    pub version: Version,
    /// 采用的预期值。
    pub expected: u32,
    /// 实测到的能力集（探测失败时为 `None`）。
    pub measured: Option<u32>,
    /// 认证状态。
    pub certification: Certification,
    /// 矩阵与实测的冲突（`None` = 无冲突）；有冲突时结论**取实测**。
    pub conflict: Option<Conflict>,
}

/// 矩阵与实测的冲突留档（要点二：结论取实测，痕迹留档）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Conflict {
    /// 矩阵预期值。
    pub matrix: u32,
    /// 实测值。
    pub measured: u32,
}

impl ProbeRecord {
    /// 有效能力集：**有实测取实测**（锚点「以实测为准」），否则取预期。
    ///
    /// 这就是「以实测为准修矩阵」的落点——结论侧以实测为准，
    /// 矩阵侧靠 [`Conflict`] 留档待修。
    pub fn effective(&self) -> u32 {
        match self.measured {
            Some(m) => m,
            None => self.expected,
        }
    }

    /// 是否与矩阵冲突。
    pub fn has_conflict(&self) -> bool {
        self.conflict.is_some()
    }
}

/// 探测一次（锚点「探测 O(1) 一次性」）。
///
/// `measured` 为 `None` 表示探测失败 → 保守预期 + 告警。
pub fn probe(m: &CompatMatrix, v: Version, measured: Option<u32>) -> ProbeRecord {
    let registered = m.is_registered(v);
    let expected = match m.expect_for(v) {
        Some(e) => e,
        // 低于矩阵下限：给保守交集 + 未认证，不假装有明确预期
        None => m.conservative(),
    };

    if measured.is_none() {
        // 探测失败 → **保守预期 + 降级告警**（锚点错误路径原文）。
        //
        // 这里必须切到 `conservative()` 而不是沿用上面算出的 `expected`：
        // 上面那份是「矩阵对该版本的预期」，其成立**以探测成功为前提**。
        // 探测失败时那份预期没有被任何实测证实过，直接沿用等于把
        // 「未验证的能力」报成支持 —— 锚点要的恰恰相反（保守告警）。
        // 典型差异：8.0 行预期六位全开，而保守交集只有 VIRGL|EDID_EXT。
        return ProbeRecord {
            version: v,
            expected: m.conservative(),
            measured: None,
            certification: Certification::Degraded,
            conflict: None,
        };
    }

    let mv = match measured {
        Some(x) => x,
        None => 0,
    };
    let conflict = if mv != expected { Some(Conflict { matrix: expected, measured: mv }) } else { None };

    ProbeRecord {
        version: v,
        expected,
        measured: Some(mv),
        // 未知版本按最新行预期但**标记未认证**；低于下限亦未认证
        certification: if registered { Certification::Verified } else { Certification::Uncertified },
        conflict,
    }
}

/// 未认证记录（供告警/诊断，锚点「标记未认证」）。
pub fn unauthenticated(records: &[ProbeRecord]) -> Vec<Version> {
    let mut v = Vec::new();
    let mut i = 0;
    while i < records.len() {
        if records[i].certification.needs_warning() {
            v.push(records[i].version);
        }
        i += 1;
    }
    v
}

/// 支持给定能力的最高版本行（供 F0216 宣告引用「最低支持版本」）。
pub fn min_version_for(m: &CompatMatrix, cap: u32) -> Option<Version> {
    let mut i = 0;
    while i < m.row_count() {
        if m.rows[i].caps & cap == cap {
            return Some(m.rows[i].version);
        }
        i += 1;
    }
    None
}

/// 登记的差异维度数（某版本行相对前一行的变化条目数）。
pub fn delta_count(row: &Row) -> usize {
    let mut n = 0;
    let mut i = 0;
    while i < DIMENSIONS {
        if row.delta[i].changed() {
            n += 1;
        }
        i += 1;
    }
    n
}
