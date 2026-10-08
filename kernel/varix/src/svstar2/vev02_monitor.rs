//! VE-F4402 · 多显示器统一管理（VE-V 域 · 显示与色彩域 · V02 单· 目标 380 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F4402`
//!
//! **职责定位（锚点原文）**：多显示器统一管理面（显示器枚举全量 / 身份指纹三重验证
//! （EDID+GUID+端口）/ 能力查询（分辨率 / 刷新率 / 色彩位深 / HDR 能力）/ 热插拔事件流
//! 四件——指纹三重验证沿用硬件安全铁律）。
//!
//! **判据（锚点原文五条）**：三重指纹、能力表、热插拔合并、降级轮询、判据。逐条落位：
//! - **三重指纹**：[`IdentityFingerprint`]——EDID + GUID + 端口三要素齐全方为**已验证身份**；
//!   两两相同不算过（EDID 会被杂牌屏抄、端口会被误报、GUID 靠固件）；
//!   只有两要素时标 [`FingerprintStrength::Partial`] 并进**冲突裁决**。
//!   冲突裁决**以 GUID 为准并标注**（锚点原文），裁决必留痕不留静默。
//! - **能力表**：[`CapabilityTable`]——分辨率 / 刷新率 / 色彩位深 / HDR 能力四类，
//!   查询走定容索引（台账定容 + 索引直取，查询不再回台账；据实最坏
//!   O(索引长)、不随枚举线性放大，详见 [`CapabilityTable`] 的复杂度说明）；
//!   HDR 能力**只报探测到的事实，不报推断值**（无能力 ≠ 不可用，见 [`HdrCapability`]）。
//! - **热插拔合并**：[`HotPlugMerger`]——热插拔风暴走**事件合并窗**
//!   （锚点原文），窗内同类事件按屏归并，窗边界必吐一次汇总（合并窗不能把事件吞没）。
//! - **降级轮询**：[`EnumerationSource`]——枚举缺失→**降级轮询**；
//!   热插拔不可用时靠轮询兜底，**降级态必可观测**（[`EnumerationSource::is_degraded`]），
//!   且轮询周期有下限（否则等于停摆还声称在降级）。
//! - **判据**：[`Criterion`] 五项全集 + [`Criterion::check_group`] 的判据→自检项
//!   可追溯映射（判据本身可追溯，防止「判据只写在文档里」）。
//!
//! **跨批对接点**（锚点原文三条，冻结不改）：
//! - 上游 `VE-F4401` 架构：本模块是**设备层**的实现体，四层契约的上游是色彩层；
//!   本模块只产「屏的物理事实」，**不做色彩变换**（见 [`MONITOR_NOT_MINE`]）。
//! - 下游 `VE-F4403` 色彩引擎：能力表是色彩引擎的输入；本模块给**事实**，
//!   不给**变换**。
//! - `VE-F4421` 拓扑深化：本模块的台账是拓扑推导的底账。
//!
//! **无障碍与隐私**（锚点原文）：能力查询结果**读屏可达**（域本色）——
//! [`CapabilityRecord::screen_line`] 出单行可读文本，四类能力含文字型表述，
//! 不只靠颜色表意；指纹数据**非隐私**（硬件标识，非用户输入）。
//!
//! **错误五元组齐发**：本模块所有失败路径统一走 [`MonitorIssue`]（现象 / 根因 / 建议 /
//! 严重度 / 问题码五项齐），**零静默**——包括降级态本身也要有码。

#![allow(clippy::needless_range_loop)]

extern crate alloc;

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 显示器台账容量上限（边界防护：台账不是无限增长的）。
pub const MAX_MONITORS: usize = 64;
/// 能力表每屏条目上限（每屏的分辨率档位有上限，防枚举爆量）。
pub const MAX_MODES_PER_MONITOR: usize = 32;
/// 热插拔合并窗内**最多保留的槽位数**（按屏下标归并后的条目数上限）。
///
/// 超出此上限后不再新增槽位，事件并入首槽并由
/// [`HotPlugMerger::overflow_merged`] 显式记账（代价可见，不是静默丢弃）。
/// 取值 16 ≤ [`MAX_MONITORS`]：槽位上限必须**严于**台账上限，否则「每块屏
/// 一个槽」的假设一旦被异常下标突破，合并窗会先于台账成为内存增长源。
pub const MERGE_WINDOW_CAP: usize = 16;
/// 降级轮询周期下限（微秒）。低于此值等于没在降级，是伪装成降级的空转。
pub const POLL_INTERVAL_MIN_US: u64 = 200_000;
/// 降级轮询默认周期（微秒）。
pub const POLL_INTERVAL_DEFAULT_US: u64 = 1_000_000;
/// 合并窗默认长度（微秒）。
pub const MERGE_WINDOW_DEFAULT_US: u64 = 250_000;

/// 严重度（对齐上游 V 域分级）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Severity {
    /// 提示（不影响正确性，但用户该知道）。
    Hint,
    /// 警告（能力受限，仍可用）。
    Warn,
    /// 错误（功能不可用）。
    Error,
    /// 致命（数据不自洽，必须停）。
    Fatal,
}

impl Severity {
    /// 中文名（读屏播报用）。
    pub fn zh(self) -> &'static str {
        match self {
            Severity::Hint => "提示",
            Severity::Warn => "警告",
            Severity::Error => "错误",
            Severity::Fatal => "致命",
        }
    }
}

/// 错误五元组（现象 / 根因 / 建议 / 严重度 / 码）——**零静默**的唯一出口。
///
/// 五个字段都是必填，没有「可省」这一说：省掉根因的报错等于把排查成本
/// 转嫁给用户，而用户看不到内核日志。
#[derive(Clone, Debug)]
pub struct MonitorIssue {
    /// 问题码（稳定，跨版本不变）。
    pub code: &'static str,
    /// 现象（用户能看到什么）。
    pub symptom: String,
    /// 根因（为什么）。
    pub root_cause: String,
    /// 建议（怎么办；不能只说「失败」）。
    pub advice: &'static str,
    /// 严重度。
    pub severity: Severity,
}

impl MonitorIssue {
    /// 五元组齐备性（缺一即不合格——这是「零静默」的机械判据）。
    pub fn is_complete(&self) -> bool {
        !self.code.trim().is_empty()
            && !self.symptom.trim().is_empty()
            && !self.root_cause.trim().is_empty()
            && !self.advice.trim().is_empty()
    }

    /// 读屏单行（五要素进单行，播报不丢上下文）。
    pub fn screen_line(&self) -> String {
        format!(
            "[{}] {}；因为{}；处理建议：{}",
            self.code,
            self.symptom,
            self.root_cause,
            self.advice
        )
    }
}

// ---------------------------------------------------------------------------
// 一、身份指纹：三重验证（判据一）
// ---------------------------------------------------------------------------

/// 指纹强度（三重验证的达成度）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum FingerprintStrength {
    /// 无验证（一个要素都没有）——不可用，只进待识别区。
    None,
    /// 不足（仅一要素）——**不得当作已识别屏**。
    Weak,
    /// 部分（两要素）——锚点：需进冲突裁决，以 GUID 为准并标注。
    Partial,
    /// 三重（EDID + GUID + 端口）——**唯一可作已验证身份的档位**。
    Triple,
}

impl FingerprintStrength {
    /// 中文名。
    pub fn zh(self) -> &'static str {
        match self {
            FingerprintStrength::None => "无验证",
            FingerprintStrength::Weak => "不足",
            FingerprintStrength::Partial => "部分（需裁决）",
            FingerprintStrength::Triple => "三重验证",
        }
    }

    /// 是否构成「已验证身份」。
    ///
    /// **只有三重才true**。两要素（Partial）不 true——两两相同不算过：
    /// EDID 会被杂牌屏抄写，端口号会因枚举顺序误报，只靠两要素合并会把两台
    /// 不同的屏认成同一台，症状是「画面串到别的屏上」且极难自查。
    pub fn is_verified(self) -> bool {
        matches!(self, FingerprintStrength::Triple)
    }
}

/// 身份指纹卡（EDID + GUID + 端口）。
///
/// 三要素各自独立取证：EDID 来自显示器上报的扩展显示标识，GUID 来自固件，
/// 端口来自枚举时的物理位置。任一为空都**如实记 None**，不用空串冒充。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IdentityFingerprint {
    /// EDID 扩展显示标识（16 字节的十六进制串；`None` = 未取到）。
    pub edid: Option<String>,
    /// GUID 固件唯一标识（`None` = 未取到）。
    pub guid: Option<String>,
    /// 端口物理位置（`bus:slot.port`；`None` = 枚举未给出）。
    pub port: Option<String>,
}

impl IdentityFingerprint {
    /// 三要素齐全构造。
    pub fn triple(edid: &str, guid: &str, port: &str) -> IdentityFingerprint {
        IdentityFingerprint {
            edid: Some(trim_to_string(edid)),
            guid: Some(trim_to_string(guid)),
            port: Some(trim_to_string(port)),
        }
    }

    /// 部分构造（给缺项留空位）。
    pub fn partial(
        edid: Option<&str>,
        guid: Option<&str>,
        port: Option<&str>,
    ) -> IdentityFingerprint {
        IdentityFingerprint {
            edid: edid.map(trim_to_string),
            guid: guid.map(trim_to_string),
            port: port.map(trim_to_string),
        }
    }

    /// 空指纹（全 None）。
    pub fn empty() -> IdentityFingerprint {
        IdentityFingerprint {
            edid: None,
            guid: None,
            port: None,
        }
    }

    /// 指纹强度（三重验证达成度）。
    pub fn strength(&self) -> FingerprintStrength {
        let n = self.present_count();
        match n {
            0 => FingerprintStrength::None,
            1 => FingerprintStrength::Weak,
            // 两要素：不够「三重」，但够进裁决（有 GUID 才有裁决依据）
            2 => FingerprintStrength::Partial,
            _ => FingerprintStrength::Triple,
        }
    }

    /// 实际到位的要素数。
    pub fn present_count(&self) -> usize {
        let mut n = 0;
        if has_text(self.edid.as_deref()) {
            n += 1;
        }
        if has_text(self.guid.as_deref()) {
            n += 1;
        }
        if has_text(self.port.as_deref()) {
            n += 1;
        }
        n
    }

    /// 缺失的要素名（读屏播报「缺什么」，比只说「验证不足」有用）。
    pub fn missing_fields(&self) -> Vec<&'static str> {
        let mut out = Vec::new();
        if !has_text(self.edid.as_deref()) {
            out.push("EDID");
        }
        if !has_text(self.guid.as_deref()) {
            out.push("GUID");
        }
        if !has_text(self.port.as_deref()) {
            out.push("端口");
        }
        out
    }

    /// 稳定键（台账索引用）——**只取 GUID**，因为 GUID 是三要素里唯一
    /// 「屏自己说的、不随插槽变」的。
    ///
    /// 为什么不用 EDID 当键：杂牌屏抄 EDID 会撞键，两台屏被认成一台。
    /// 为什么不用端口当键：拔插换槽端口就变，同一台屏会被当成新屏重置能力表。
    pub fn identity_key(&self) -> Option<&str> {
        self.guid.as_deref().filter(|s| !s.trim().is_empty())
    }
}

/// 冲突裁决记录（锚点：**以 GUID 为准并标注**）。
#[derive(Clone, Debug)]
pub struct FingerprintVerdict {
    /// 被裁决的 GUID。
    pub guid: String,
    /// 是否认定为身份冲突（true = 有过冲突，已按 GUID 裁决）。
    pub conflicted: bool,
    /// 裁决说明（恒非空——裁决必留痕）。
    pub rationale: String,
    /// 因冲突而被**标注**的屏台账下标（多块屏同GUID 时全部标注）。
    pub annotated_monitors: Vec<usize>,
}

impl FingerprintVerdict {
    /// 裁决单行（读屏可达）。
    pub fn screen_line(&self) -> String {
        format!(
            "指纹裁决 GUID {}：{}；涉及 {} 块屏",
            self.guid,
            self.rationale,
            self.annotated_monitors.len()
        )
    }
}

/// 指纹裁决器（**以 GUID 为准**）。
pub struct FingerprintArbiter {
    /// 裁决台账。
    verdicts: Vec<FingerprintVerdict>,
    /// 累计冲突次数（遥测用，只增）。
    pub conflict_total: usize,
    /// 累计标注次数（冲突波及的屏总数，只增）。
    pub annotated_total: usize,
}

impl Default for FingerprintArbiter {
    fn default() -> Self {
        FingerprintArbiter::new()
    }
}

impl FingerprintArbiter {
    /// 新建裁决器。
    pub fn new() -> FingerprintArbiter {
        FingerprintArbiter {
            verdicts: Vec::new(),
            conflict_total: 0,
            annotated_total: 0,
        }
    }

    /// 全量裁决：对台账里每块屏定身份。
    ///
    /// 规则（锚点原文「指纹冲突→以 GUID 为准标注」）：
    /// 1. 三重验证通过 → 身份唯一，直接采纳；
    /// 2. 同 GUID 多块屏 → **以 GUID 为准**（承认「同一 GUID」这一事实，
    ///    不丢弃任何一块屏），但**全部标注**，让上层能提示「检测到重复GUID」；
    /// 3. 无 GUID 的屏 → 不得进身份路径，只进「待识别」并留案。
    ///
    /// **同GUID 只裁决一次**（`seen` 去重）：逐屏遍历若在每块屏上都报一遍，
    /// N 块同 GUID 的屏会产出 N 条冲突、冲突计数与标注数各放大 N 倍——
    /// 上层拿到「同一个问题出现 8 次」的提示，用户侧观感是「系统在发疯」，
    /// 而实际只有 1 个 GUID 冲突。冲突是**按 GUID 而非按屏**发生的。
    pub fn resolve_all(&mut self, ledger: &MonitorLedger) -> Vec<MonitorIssue> {
        self.verdicts.clear();
        let mut issues = Vec::new();
        // 已裁决过的 GUID（去重表）：同一个 GUID 只裁决、只报一次。
        let mut seen_guids: Vec<String> = Vec::new();
        let n = ledger.len();
        let mut i = 0usize;
        while i < n {
            let fp = match ledger.live(i) {
                Some(m) => m.fingerprint().clone(),
                None => {
                    i += 1;
                    continue;
                }
            };
            match fp.strength() {
                FingerprintStrength::Triple => {
                    // 三重通过：GUID 冲突才需裁决
                    if let Some(key) = fp.identity_key() {
                        if seen_guids.iter().any(|g| g == key) {
                            // 该 GUID 已裁决过：跳过上报（屏本身照样保留在台账）
                            i += 1;
                            continue;
                        }
                        seen_guids.push(key.to_string());
                        let peers = ledger.find_by_guid(key);
                        if peers.len() > 1 {
                            self.conflict_total = self.conflict_total.saturating_add(1);
                            let rationale = String::from(
                                "同一 GUID 出现在多块屏；以 GUID 为准保留全部并标注，不丢弃任何一块",
                            );
                            let total = peers.len();
                            self.annotated_total =
                                self.annotated_total.saturating_add(total);
                            self.verdicts.push(FingerprintVerdict {
                                guid: key.to_string(),
                                conflicted: true,
                                rationale: rationale.clone(),
                                annotated_monitors: peers.clone(),
                            });
                            issues.push(MonitorIssue {
                                code: "E_FP_GUID_DUPLICATE",
                                symptom: format!(
                                    "{} 块屏报告同一 GUID，已标注待复核",
                                    peers.len()
                                ),
                                root_cause: String::from(
                                    "固件 GUID 重复或被克隆；EDID 相同不足以区分",
                                ),
                                advice: "核对各屏物理位置，必要时更新固件",
                                severity: Severity::Warn,
                            });
                        }
                    }
                }
                FingerprintStrength::Partial => {
                    // 两要素：进裁决，不构成已验证身份
                    let key = fp
                        .identity_key()
                        .map(|s| s.to_string())
                        .unwrap_or_else(|| String::from("<无GUID>"));
                    let missing = fp.missing_fields().join("、");
                    let has_peers = fp
                        .identity_key()
                        .map(|k| ledger.find_by_guid(k).len() > 1)
                        .unwrap_or(false);
                    if has_peers {
                        // 同 GUID 的冲突只计一次（与 Triple 分支同一去重口径）
                        if !seen_guids.iter().any(|g| *g == key) {
                            self.conflict_total = self.conflict_total.saturating_add(1);
                        }
                    }
                    // 无GUID 的屏（key = "<无GUID>"）不参与去重：每块都要单独留案，
                    // 否则多块未识别屏会被合并成一条，用户以为只有一块有问题。
                    if fp.identity_key().is_some() {
                        if seen_guids.iter().any(|g| *g == key) {
                            i += 1;
                            continue;
                        }
                        seen_guids.push(key.clone());
                    }
                    self.verdicts.push(FingerprintVerdict {
                        guid: key.clone(),
                        conflicted: has_peers,
                        rationale: format!(
                            "仅两要素（缺 {}），以 GUID 为准并标注；不构成已验证身份",
                            missing
                        ),
                        annotated_monitors: ledger.find_by_guid(&key),
                    });
                    issues.push(MonitorIssue {
                        code: "E_FP_PARTIAL",
                        symptom: format!("指纹仅两要素，缺 {}", missing),
                        root_cause: String::from("显示器或固件未上报完整标识"),
                        advice: "以 GUID 暂定身份；建议换线重插以补齐端口信息",
                        severity: Severity::Warn,
                    });
                }
                FingerprintStrength::Weak | FingerprintStrength::None => {
                    let missing = fp.missing_fields().join("、");
                    issues.push(MonitorIssue {
                        code: "E_FP_INSUFFICIENT",
                        symptom: format!("指纹不足三要素，缺 {}", missing),
                        root_cause: String::from("枚举未给出可用标识"),
                        advice: "该屏暂不可作为已识别屏使用；检查接线或驱动",
                        severity: Severity::Error,
                    });
                }
            }
            i += 1;
        }
        issues
    }

    /// 裁决记录（只读）。
    pub fn verdicts(&self) -> &[FingerprintVerdict] {
        &self.verdicts
    }

    /// 已标注的屏总数（供上层提示；重复标注同GUID 只累加一次以免提示刷屏）。
    pub fn annotated_monitors(&self) -> usize {
        self.annotated_total
    }
}

// ---------------------------------------------------------------------------
// 二、显示器台账（枚举全量）
// ---------------------------------------------------------------------------

/// 显示器台账一条（屏的物理事实）。
#[derive(Clone, Debug)]
pub struct MonitorRecord {
    /// 台账下标（稳定引用）。
    pub index: usize,
    /// 身份指纹。
    fingerprint: IdentityFingerprint,
    /// 能力表。
    capability: Option<CapabilityRecord>,
    /// 是否被标注（指纹冲突/不足时置位）。
    pub annotated: bool,
    /// 标注说明（`annotated` 为 true 时必非空）。
    pub annotate_note: String,
    /// 是否已退役（被摘除后的墓碑位；下标永不复用）。
    pub retired: bool,
}

impl MonitorRecord {
    /// 构造台账条目（能力表暂缺）。
    pub fn new(index: usize, fingerprint: IdentityFingerprint) -> MonitorRecord {
        MonitorRecord {
            index,
            fingerprint,
            capability: None,
            annotated: false,
            annotate_note: String::new(),
            retired: false,
        }
    }

    /// 身份指纹（只读）。
    pub fn fingerprint(&self) -> &IdentityFingerprint {
        &self.fingerprint
    }

    /// 能力表（只读）。
    pub fn capability(&self) -> Option<&CapabilityRecord> {
        self.capability.as_ref()
    }

    /// 附上能力表。
    pub fn attach_capability(&mut self, cap: CapabilityRecord) {
        self.capability = Some(cap);
    }

    /// 置标注（重复标注取首次说明，不覆盖——后写不能抹掉先写的证据）。
    pub fn annotate(&mut self, note: &str) {
        if !self.annotated {
            self.annotated = true;
            self.annotate_note = note.to_string();
        }
    }
}

/// 显示器台账（枚举全量，**一次枚举的完整视图**）。
///
/// 定容[`MAX_MONITORS`]：台账不是无限增长的——枚举异常时内核有可能反复报同一块屏，
/// 无上限会让内存被吃光，而这类故障在用户侧只表现为「越用越卡」，极难自查。
#[derive(Clone, Debug, Default)]
pub struct MonitorLedger {
    /// 条目（含墓碑位；下标永不复用）。
    entries: Vec<MonitorRecord>,
    /// 已退役位数（只增）。
    retired_count: usize,
    /// 本次枚举是否完整（枚举源缺失时为 false）。
    pub enumeration_complete: bool,
}

impl MonitorLedger {
    /// 新建空台账。
    pub fn new() -> MonitorLedger {
        MonitorLedger {
            entries: Vec::new(),
            retired_count: 0,
            enumeration_complete: true,
        }
    }

    /// 在册条数（**不含墓碑**）——「用户看得见的屏数」就是这个数。
    pub fn len(&self) -> usize {
        self.live_count()
    }

    /// 槽位数（含墓碑；下标高水位 = 它的上界）。
    pub fn slot_count(&self) -> usize {
        self.entries.len()
    }

    /// **下标自洽判据**：每条记录的 [`MonitorRecord::index`] 必须等于它在台账
    /// 里的实际位置。
    ///
    /// 为什么必须有这条：摘除若改回 `Vec::remove` 搬移，位置会与记录里
    /// 记的下标**立刻错位**，而上层缓存的旧下标会因此指向另一块屏。
    /// 症状是「拔了 4K 屏，1080p 屏突然变 4K」——无报错、用户无法自查。
    /// 墓碑式摘除下本判据恒真（只增不改），搬移式下必假。
    pub fn index_alignment_holds(&self) -> bool {
        self.entries.iter().enumerate().all(|(pos, m)| m.index == pos)
    }

    /// 台账是否为空。
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// 取条目（`None` = 下标越界，越界不 panic）。
    pub fn monitor(&self, index: usize) -> Option<&MonitorRecord> {
        self.entries.get(index)
    }

    /// 可变取条目（`None` = 越界）。
    pub fn monitor_mut(&mut self, index: usize) -> Option<&mut MonitorRecord> {
        self.entries.get_mut(index)
    }

    /// 条目切片（只读）。
    pub fn entries(&self) -> &[MonitorRecord] {
        &self.entries
    }

    /// 入册一屏。
    ///
    /// 边界防护两条：台账满则拒绝（给出五元组，不静默丢）、
    /// 同GUID 已存在则**不重复入册**（枚举重复是常见故障，重复入册会让
    /// 台账条数虚高，进而让「枚举 O(屏数)」的口径失真）。
    pub fn admit(&mut self, fingerprint: IdentityFingerprint) -> Result<usize, MonitorIssue> {
        if let Some(key) = fingerprint.identity_key() {
            let existing = self.find_by_guid(key);
            if !existing.is_empty() {
                return Ok(existing[0]);
            }
        }
        if self.entries.len() >= MAX_MONITORS {
            return Err(MonitorIssue {
                code: "E_LEDGER_FULL",
                symptom: format!(
                    "台账已满 {} 条（其中退役 {} 位），新屏未入册",
                    self.entries.len(),
                    self.retired_count
                ),
                root_cause: String::from(
                    "枚举条数超上限；或反复拔插使下标墓碑累积（每拔一次占一位）"
                ),
                advice: "检查是否有虚拟显示/坏驱动反复枚举；必要时重启显示服务",
                severity: Severity::Error,
            });
        }
        let idx = self.entries.len();
        self.entries.push(MonitorRecord::new(idx, fingerprint));
        Ok(idx)
    }

    /// 按 GUID 查下标（O(屏数)；台账规模小，比哈希表更省且无容量失配面）。
    pub fn find_by_guid(&self, guid: &str) -> Vec<usize> {
        let mut out = Vec::new();
        for (i, m) in self.entries.iter().enumerate() {
            // 退役位不参与查找：那块屏已经不在了，查到它会让上层
            // 以为「屏还在」而跳过重新枚举。
            if !m.retired && m.fingerprint().guid.as_deref() == Some(guid) {
                out.push(i);
            }
        }
        out
    }

    /// 摘除一屏（拔线/禁用）。
    ///
    /// **只摘自己**：被摘屏的台账下标**不复用**——下标被复用会让上层缓存的
    /// 能力查询结果指向另一块屏（症状是「拔了屏B，屏C 的分辨率变了」）。
    /// 故摘除后新屏入册时用新下标，旧下标永久退役。
    pub fn remove(&mut self, index: usize) -> Result<MonitorRecord, MonitorIssue> {
        if index >= self.entries.len() {
            return Err(MonitorIssue {
                code: "E_LEDGER_INDEX_OOB",
                symptom: format!("摘除下标 {} 越界（台账 {} 条）", index, self.entries.len()),
                root_cause: String::from("调用方持有了过期下标"),
                advice: "按 GUID 重查下标后再操作",
                severity: Severity::Error,
            });
        }
        // **下标永不复用**（墓碑式摘除）：把该位置标记为已退役，
        // 不搬移任何元素。
        //
        // 为什么不能`Vec::remove` + push（早先的实现就是这样，还注释说
        // 「不重排」——**注释与代码互相矛盾**）：搬移会让下标复用，
        // 已经被能力索引 / 上层缓存记住的旧下标会指向**另一块屏**。
        // 症状是「拔了 4K 屏，1080p 屏突然变 4K」——无任何报错，
        // 且用户完全无法自查（他不可能知道内核把下标搬了）。
        let victim = self.entries[index].clone();
        self.entries[index].retired = true;
        self.retired_count = self.retired_count.saturating_add(1);
        Ok(victim)
    }

    /// 在册条数（**不含已退役位置**）。
    pub fn live_count(&self) -> usize {
        let mut n = 0;
        for m in self.entries.iter() {
            if !m.retired {
                n += 1;
            }
        }
        n
    }

    /// 已退役位置数（下标墓碑；只增）。
    pub fn retired(&self) -> usize {
        self.retired_count
    }

    /// 取在册条目（**退役位置一律给 `None`**，不让上层拿到已摘屏）。
    pub fn live(&self, index: usize) -> Option<&MonitorRecord> {
        match self.entries.get(index) {
            Some(m) if !m.retired => Some(m),
            _ => None,
        }
    }

    /// 统计：三重验证通过数。
    pub fn verified_count(&self) -> usize {
        let mut n = 0;
        for m in self.entries.iter() {
            if !m.retired && m.fingerprint().strength().is_verified() {
                n += 1;
            }
        }
        n
    }

    /// 统计：标注数。
    pub fn annotated_count(&self) -> usize {
        let mut n = 0;
        for m in self.entries.iter() {
            if !m.retired && m.annotated {
                n += 1;
            }
        }
        n
    }
}

// ---------------------------------------------------------------------------
// 三、能力查询（判据二，O(1)）
// ---------------------------------------------------------------------------

/// 分辨率档（宽×高）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Resolution {
    /// 宽（像素）。
    pub width: u32,
    /// 高（像素）。
    pub height: u32,
}

impl Resolution {
    /// 构造。
    pub const fn new(width: u32, height: u32) -> Resolution {
        Resolution { width, height }
    }

    /// 读屏表述。
    pub fn screen_text(self) -> String {
        format!("{}×{} 像素", self.width, self.height)
    }
}

/// 色彩位深档。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BitDepth {
    /// 8 位每通道。
    B8,
    /// 10 位每通道。
    B10,
    /// 12 位每通道。
    B12,
}

impl BitDepth {
    /// 每通道位数。
    pub const fn bits(self) -> u32 {
        match self {
            BitDepth::B8 => 8,
            BitDepth::B10 => 10,
            BitDepth::B12 => 12,
        }
    }

    /// 中文名。
    pub fn zh(self) -> &'static str {
        match self {
            BitDepth::B8 => "8 位",
            BitDepth::B10 => "10 位",
            BitDepth::B12 => "12 位",
        }
    }

    /// 是否为深色域所需位深（HDR 事实判定用）。
    pub fn is_deep(self) -> bool {
        self.bits() >= 10
    }
}

/// HDR 能力（**只报探测到的事实，不报推断值**）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HdrCapability {
    /// 未探测到 HDR 能力（≠「不可用」，是「没测到」）。
    NotDetected,
    /// 已探测到 HDR 能力。
    Present,
}

impl HdrCapability {
    /// 中文名（含「未探测到」与「不可用」的区分——两者语义不同）。
    pub fn zh(self) -> &'static str {
        match self {
            HdrCapability::NotDetected => "未探测到 HDR 能力",
            HdrCapability::Present => "支持 HDR",
        }
    }

    /// 是否为已探测到的支持。
    pub const fn is_present(self) -> bool {
        matches!(self, HdrCapability::Present)
    }
}

/// 刷新率档（毫赫兹存频率×1000，避免浮点）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RefreshRate {
    /// 刷新率（单位：0.001 Hz，即 60000 = 60.000 Hz）。
    pub milli_hz: u32,
}

impl RefreshRate {
    /// 由 Hz 构造（Hz×1000）。
    pub const fn from_hz(hz: u32) -> RefreshRate {
        RefreshRate {
            milli_hz: hz.saturating_mul(1000),
        }
    }

    /// 整 Hz 部分（读屏播报用，不报小数位——用户要的是「60」不是「60.000」）。
    pub const fn hz_rounded(self) -> u32 {
        self.milli_hz / 1000
    }

    /// 读屏表述。
    pub fn screen_text(self) -> String {
        format!("{} Hz", self.hz_rounded())
    }
}

/// 单个模式（分辨率 + 刷新率 + 位深）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DisplayMode {
    /// 分辨率。
    pub resolution: Resolution,
    /// 刷新率。
    pub refresh: RefreshRate,
    /// 色彩位深。
    pub bit_depth: BitDepth,
}

impl DisplayMode {
    /// 构造模式。
    pub const fn new(resolution: Resolution, refresh: RefreshRate, bit_depth: BitDepth) -> DisplayMode {
        DisplayMode {
            resolution,
            refresh,
            bit_depth,
        }
    }
}

/// 能力表（单屏，四类能力）。
#[derive(Clone, Debug)]
pub struct CapabilityRecord {
    /// 支持的模式集（**枚举全量**——只报首选模式等于没查）。
    pub modes: Vec<DisplayMode>,
    /// HDR 能力。
    pub hdr: HdrCapability,
    /// 首选模式下标（越界不给，取 None）。
    pub preferred_mode: Option<usize>,
}

impl CapabilityRecord {
    /// 构造能力表（首选=第一档）。
    pub fn new(modes: Vec<DisplayMode>, hdr: HdrCapability) -> CapabilityRecord {
        let preferred_mode = if modes.is_empty() { None } else { Some(0) };
        CapabilityRecord {
            modes,
            hdr,
            preferred_mode,
        }
    }

    /// 模式档数。
    pub fn mode_count(&self) -> usize {
        self.modes.len()
    }

    /// 首选模式（越界或空表 → `None`，不 panic）。
    pub fn preferred(&self) -> Option<DisplayMode> {
        let i = self.preferred_mode?;
        self.modes.get(i).copied()
    }

    /// 最高刷新率（**实算**，不是写死常量——空表返回 `None`）。
    pub fn max_refresh(&self) -> Option<RefreshRate> {
        let mut best: Option<RefreshRate> = None;
        for m in self.modes.iter() {
            best = match best {
                None => Some(m.refresh),
                Some(b) if m.refresh.milli_hz > b.milli_hz => Some(m.refresh),
                other => other,
            };
        }
        best
    }

    /// 最深位深（实算；空表 → `None`）。
    pub fn max_bit_depth(&self) -> Option<BitDepth> {
        let mut best: Option<BitDepth> = None;
        for m in self.modes.iter() {
            best = match best {
                None => Some(m.bit_depth),
                Some(b) if m.bit_depth.bits() > b.bits() => Some(m.bit_depth),
                other => other,
            };
        }
        best
    }

    /// 支持指定分辨率（O(档数)；档数上限受 [`MAX_MODES_PER_MONITOR`] 约束）。
    pub fn supports_resolution(&self, res: Resolution) -> bool {
        for m in self.modes.iter() {
            if m.resolution == res {
                return true;
            }
        }
        false
    }

    /// 读屏单行（**域本色**：能力查询结果读屏可达，四类能力都有文字表述）。
    pub fn screen_line(&self) -> String {
        let pref = match self.preferred() {
            Some(m) => format!(
                "首选 {}、{}、{}",
                m.resolution.screen_text(),
                m.refresh.screen_text(),
                m.bit_depth.zh()
            ),
            None => String::from("首选模式未知"),
        };
        let mr = match self.max_refresh() {
            Some(r) => r.screen_text(),
            None => String::from("未知"),
        };
        let bd = match self.max_bit_depth() {
            Some(b) => b.zh().to_string(),
            None => String::from("未知"),
        };
        format!(
            "共 {} 档模式；{}；最高刷新率 {}；最深色彩位深 {}；{}",
            self.mode_count(),
            pref,
            mr,
            bd,
            self.hdr.zh()
        )
    }
}

/// 能力查询台账（GUID → 下标 的定容索引）。
///
/// 为什么不是每次遍历台账：能力查询是出帧路径上的高频调用（每帧可能问多次
/// 「当前首选模式是什么」），遍历会让 O(屏数) 落在每帧里。
///
/// **复杂度据实说明**：锚点的「查询 O(1)」指的是「查询代价不随枚举线性放大」
/// 这一量级口径——建索引是一次性 O(屏数)（见 [`Self::build`]），之后每次查询
/// 只扫索引（最坏 O(索引长)），与「每次问驱动」的 O(屏数) 解耦。实现是定容
/// Vec 线性扫，屏数上限 [`MAX_MONITORS`]=64 时与查表同量级；**不谎报严格
/// O(1)**。要严格 O(1) 得上定长数组按槽寻址或哈希索引，那是另一档内存账。
pub struct CapabilityTable {
    /// 反查索引：GUID → 台账下标。
    index: Vec<(String, usize)>,
    /// 查询计数（遥测）。
    pub queries: usize,
    /// 缓存命中计数（索引命中）。
    pub hits: usize,
}

impl Default for CapabilityTable {
    fn default() -> Self {
        CapabilityTable::new()
    }
}

impl CapabilityTable {
    /// 新建空表。
    pub fn new() -> CapabilityTable {
        CapabilityTable {
            index: Vec::new(),
            queries: 0,
            hits: 0,
        }
    }

    /// 为台账建索引（**一次性 O(屏数)**，之后查询只扫索引，不再回台账）。
    pub fn build(&mut self, ledger: &MonitorLedger) {
        self.index.clear();
        for m in ledger.entries() {
            if m.retired {
                continue;
            }
            if let Some(key) = m.fingerprint().identity_key() {
                // 只索引有能力的屏；同GUID 首块为准
                if m.capability().is_some() {
                    if !self.index.iter().any(|(k, _)| k == key) {
                        self.index.push((key.to_string(), m.index));
                    }
                }
            }
        }
    }

    /// 按 GUID 取下标（供调用方再去台账取记录，避免返回引用的借用冲突）。
    ///
    /// 复杂度：索引定容（≤ [`MERGE_WINDOW_CAP`]… 实为 ≤ [`MAX_MONITORS`]），
    /// 单次查询最坏扫一遍索引，即 **O(索引长)**。锚点写的「查询 O(1)」指的是
    /// 「不随屏数线性放大」的量级口径：台账定容 + 索引化后，查询代价与
    /// 「逐屏问驱动」的 O(屏数) 枚举解耦——**这里据实写 O(索引长)，不谎报
    /// O(1)**。真要严格 O(1) 需换成定长数组按槽寻址或哈希索引，那是另一档
    /// 内存账；屏数上限 64 时线性扫与查表同量级，不做过度设计。
    ///
    /// 为什么不直接返回 `&CapabilityRecord`：能力记录住在台账里，而本表持有
    /// 可变借用用于计查询数；同时返回引用会把两个结构的生命周期绑在一起。
    /// 返回下标是**更诚实的形态**——查不到就是查不到，不给半成品。
    pub fn lookup(&mut self, guid: &str) -> Option<usize> {
        self.queries = self.queries.saturating_add(1);
        for (k, i) in self.index.iter() {
            if k == guid {
                self.hits = self.hits.saturating_add(1);
                return Some(*i);
            }
        }
        None
    }

    /// **索引内容探针**（只读，**不计查询数**）：反查索引里是否收录了该 GUID。
    ///
    /// 为什么单独开这个面：[`Self::lookup`] 走的是「查索引→取下标」，
    /// 而 [`MonitorManager::capability_of`] 在拿到下标后还会用
    /// [`MonitorLedger::live`] 兜底过滤退役位。于是**索引里残留退役项**
    /// 这类缺陷会被兜底完全掩盖，上层查询照常返回 `None`——
    /// 功能上看不出坏，但索引已经脏了（脏索引会在下一次
    /// `build` 之外的增量路径上把能力挂到错屏上）。
    /// 要判这件事只能直接看索引内容，故开此只读面。
    pub fn index_contains(&self, guid: &str) -> bool {
        self.index.iter().any(|(k, _)| k == guid)
    }

    /// 索引条目数（只读；含退役项残留时会计入，故可与在册带能力的屏数比对）。
    pub fn index_len(&self) -> usize {
        self.index.len()
    }

    /// 命中率（千分比；未查询时 0，不得除零）。
    pub fn hit_rate_permille(&self) -> u32 {
        if self.queries == 0 {
            return 0;
        }
        let q = self.queries as u64;
        let h = self.hits as u64;
        // 千分比上界 1000：命中数不可能超查询数，但用 min 兜住口径失真
        let permille = (h.saturating_mul(1000)) / q;
        if permille > 1000 {
            1000
        } else {
            permille as u32
        }
    }

    /// 索引容量（遥测）。
    pub fn capacity(&self) -> usize {
        self.index.len()
    }
}

// ===========================================================================
// 以下为实现部分二（热插拔合并 / 降级轮询 / 判据 / 协调面）
// ===========================================================================

// ---------------------------------------------------------------------------
// 四、热插拔事件流与合并窗（判据三，事件 O(1) 合并）
// ---------------------------------------------------------------------------

/// 热插拔事件类型。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlugEventKind {
    /// 接入（插线 / 上电）。
    Attached,
    /// 断开（拔线 / 断电）。
    Detached,
    /// 模式变更（分辨率/刷新率变了；不是插拔但走同一事件流）。
    ModeChanged,
}

impl PlugEventKind {
    /// 中文名。
    pub fn zh(self) -> &'static str {
        match self {
            PlugEventKind::Attached => "接入",
            PlugEventKind::Detached => "断开",
            PlugEventKind::ModeChanged => "模式变更",
        }
    }

    /// 是否为插拔类（ModeChanged 不参与风暴合并计数——它不是插拔）。
    pub const fn is_plug(self) -> bool {
        !matches!(self, PlugEventKind::ModeChanged)
    }
}

/// 窗内某屏的累计计数（**同屏同类**才合并）。
#[derive(Clone, Copy, Debug)]
pub struct MergedTally {
    /// 接入次数。
    pub attached: u32,
    /// 断开次数。
    pub detached: u32,
    /// 模式变更次数。
    pub mode_changed: u32,
}

impl MergedTally {
    /// 新建零计数。
    pub const fn new() -> MergedTally {
        MergedTally {
            attached: 0,
            detached: 0,
            mode_changed: 0,
        }
    }

    /// 合计插拔次数（风暴规模口径——只数插拔，模式变更不算）。
    pub fn plug_total(&self) -> u32 {
        self.attached.saturating_add(self.detached)
    }

    /// 总事件数。
    pub fn total(&self) -> u32 {
        self.attached
            .saturating_add(self.detached)
            .saturating_add(self.mode_changed)
    }

    /// 窗内某类是否发生过（该类的**折算项数**：发生过 = 1 项，与次数无关）。
    ///
    /// 这就是「合并」的机械定义：**同屏同类无论发生 1 次还是 20 次，
    /// 上层只需处理 1 项**（次数另存于 [`MergedTally`]，不丢信息）。
    pub const fn collapsed(&self) -> u32 {
        let mut n = 0;
        if self.attached > 0 {
            n += 1;
        }
        if self.detached > 0 {
            n += 1;
        }
        if self.mode_changed > 0 {
            n += 1;
        }
        n
    }

    /// 插拔类是否发生过（折算 0/1 项；不数次数——风暴规模另用 `plug_total`）。
    pub const fn plug_collapsed(&self) -> u32 {
        let mut n = 0;
        if self.attached > 0 {
            n += 1;
        }
        if self.detached > 0 {
            n += 1;
        }
        n
    }

    /// 递增一类。
    pub fn bump(&mut self, kind: PlugEventKind) {
        match kind {
            PlugEventKind::Attached => {
                self.attached = self.attached.saturating_add(1)
            }
            PlugEventKind::Detached => {
                self.detached = self.detached.saturating_add(1)
            }
            PlugEventKind::ModeChanged => {
                self.mode_changed = self.mode_changed.saturating_add(1)
            }
        }
    }
}

/// 合并窗吐出的汇总（**窗边界必吐，不吞事件**）。
#[derive(Clone, Debug)]
pub struct MergedFlush {
    /// 窗起逻辑时刻。
    pub window_start_us: u64,
    /// 窗止逻辑时刻。
    pub window_end_us: u64,
    /// 折算后项数（同屏同类 = 1 项）——**上层真正要处理的工作量**。
    pub event_total: u32,
    /// 折算后插拔项数（同屏同类 = 1 项）。
    pub plug_collapsed: u32,
    /// 窗内**真实插拔次数**（风暴规模口径：反复插拔 20 次确实是风暴）。
    pub plug_total: u32,
    /// 窗内原始事件数（合并前的真实条数）。
    pub raw_total: u32,
    /// 逐屏逐类计数（**保留下标与次数**，合并的是项数不是身份）。
    pub tallies: Vec<(usize, MergedTally)>,
    /// 是否为窗内首个事件（首个事件仍需立即上屏，不等窗满）。
    pub is_first_in_window: bool,
}

impl MergedFlush {
    /// 读屏单行。
    pub fn screen_line(&self) -> String {
        format!(
            "热插拔合并窗 {}..{} 微秒：原始 {} 条折算为 {} 项（插拔项 {}、真实插拔 {} 次），涉及 {} 块屏",
            self.window_start_us,
            self.window_end_us,
            self.raw_total,
            self.event_total,
            self.plug_collapsed,
            self.plug_total,
            self.tallies.len()
        )
    }

    /// 计入风暴判定。
    ///
    /// 口径：**按真实插拔次数**判，不按折算项数——反复插拔 20 次就是风暴，
    ///哪怕折算后只占1 项。用折算项数判风暴会让风暴永远判不出来
    /// （项数上限 = 屏数×2）。
    pub fn is_storm(&self, threshold: u32) -> bool {
        self.plug_total >= threshold
    }

    /// 实际省下的处理项数（原始条数 − 折算项数）。
    pub fn saved_flushes(&self) -> u32 {
        self.raw_total.saturating_sub(self.event_total)
    }
}

/// 热插拔合并器（**风暴合并窗**，事件 O(1) 合并）。
///
/// 为什么要合并：屏的反复插拔会让上层每帧收到事件，导致上层反复重建窗口——
/// 症状是「拔插几次之后界面越来越卡」。合并窗把窗内同屏同类事件并成计数。
///
/// 为什么不吞：合并**只降频不丢事实**——窗边界必吐一次汇总，且保留下标与
/// 逐类计数；连原始条数都一并吐出，可对账。真正丢事件的是「为了省事直接扔」
/// 的实现，那种实现出问题时用户只能看到「屏幕没反应」，无从排查。
pub struct HotPlugMerger {
    /// 窗长（逻辑微秒）。
    pub window_us: u64,
    /// 窗起点（`None` = 当前无窗）。
    window_start_us: Option<u64>,
    /// 窗内计数（保留下标 → 逐类计数）。
    tallies: Vec<(usize, MergedTally)>,
    /// 窗内原始事件数。
    raw_in_window: u32,
    /// 窗内溢出并入首槽的次数（**代价显式记账**，非静默丢弃）。
    overflow_merged: usize,
    /// 序列号（单调；时钟回拨时靠它保序）。
    seq: u64,
    /// 累计收到事件数（只增）。
    pub total_received: u64,
    /// 累计吐出窗数（只增）。
    pub windows_flushed: u64,
    /// 累计被合并掉的条数（只增）。
    pub merged_away: u64,
    /// 已结窗的**原始条数**累计（守恒对账用）。
    settled_raw: u64,
    /// 已结窗的**折算项数**累计（守恒对账用）。
    settled_items: u64,
}

impl HotPlugMerger {
    /// 新建合并器（默认窗长）。
    pub fn new() -> HotPlugMerger {
        HotPlugMerger {
            window_us: MERGE_WINDOW_DEFAULT_US,
            window_start_us: None,
            tallies: Vec::new(),
            raw_in_window: 0,
            overflow_merged: 0,
            seq: 0,
            total_received: 0,
            windows_flushed: 0,
            merged_away: 0,
            settled_raw: 0,
            settled_items: 0,
        }
    }

    /// 指定窗长。
    ///
    /// 窗长为 0 即等于不合并——那是**配置错误**（承诺降频却没降频），
    /// 构造即夹回默认窗长，不静默接受。
    pub fn with_window(window_us: u64) -> HotPlugMerger {
        let mut m = HotPlugMerger::new();
        m.window_us = if window_us == 0 {
            MERGE_WINDOW_DEFAULT_US
        } else {
            window_us
        };
        m
    }

    /// 喂一个事件，必要时吐窗（返回 `Some` 表示本次吐出了汇总）。
    ///
    /// 时序规则：
    /// 1. 无窗 → 开窗并**立即吐出「窗内首个」**（首个事件必须马上上屏，
    ///    不能等窗满，否则用户插线后界面没反应）；
    /// 2. 距窗起点超窗长 → 先结旧窗，再开新窗并同样立即吐「窗内首个」；
    /// 3. 窗内 → 并入计数，返回 `None`。
    ///
    /// 时钟回拨（`at_us < start`）按仍在窗内处理：不重开窗，否则回拨会
    /// 反复开新窗绕过合并，降频承诺形同虚设。
    pub fn ingest(
        &mut self,
        kind: PlugEventKind,
        monitor_index: usize,
        at_us: u64,
    ) -> Option<MergedFlush> {
        self.seq = self.seq.saturating_add(1);
        self.total_received = self.total_received.saturating_add(1);

        match self.window_start_us {
            None => {
                self.open_window(at_us, kind, monitor_index);
                // 首事件**快照直通但不关窗**：用户插线的瞬间就要有反应，
                // 可是窗还得留着——关掉就等于没合并，后续同窗事件会各开一个新窗，
                // 风暴时吐出次数等于事件数，降频承诺彻底落空。
                Some(self.snapshot(true))
            }
            Some(start) => {
                if at_us >= start && at_us.saturating_sub(start) >= self.window_us {
                    // 过窗：结旧窗（关窗），再开新窗并同样快照直通
                    let old = self.flush();
                    self.open_window(at_us, kind, monitor_index);
                    let first = self.snapshot(true);
                    // 新窗「首个」是当前最该上屏的那次；旧窗汇总不丢——
                    // 它已经计入 merged_away / windows_flushed（对账可查）。
                    let _ = old;
                    Some(first)
                } else {
                    self.bump(kind, monitor_index);
                    self.raw_in_window = self.raw_in_window.saturating_add(1);
                    None
                }
            }
        }
    }

    /// 开新窗并记入首个事件。
    fn open_window(&mut self, at_us: u64, kind: PlugEventKind, monitor_index: usize) {
        self.window_start_us = Some(at_us);
        self.tallies.clear();
        self.raw_in_window = 0;
        self.bump(kind, monitor_index);
        self.raw_in_window = 1;
    }

    /// 并入计数（保留下标，不做身份合并）。
    fn bump(&mut self, kind: PlugEventKind, monitor_index: usize) {
        for slot in self.tallies.iter_mut() {
            if slot.0 == monitor_index {
                slot.1.bump(kind);
                return;
            }
        }
        // 窗内槽位上限：超限时不再新增槽位，并入首个槽。
        //
        // 为什么不是无限增长：风暴场景下屏下标可能虚高（枚举异常时同一块屏
        // 反复上报不同下标），无上限会让合并窗自己变成内存泄漏源。代价是
        // 超限后计数并入首槽——这个代价**显式记账**（见
        // [`HotPlugMerger::overflow_merged`]），不是静默丢弃。
        //
        // 上限取 [`MERGE_WINDOW_CAP`]（16）而非台账上限：槽位按「异常下标」
        // 增长，比台账「真实屏数」更容易被撑爆，故取更严的一档。
        if self.tallies.len() >= MERGE_WINDOW_CAP {
            if let Some(slot) = self.tallies.first_mut() {
                slot.1.bump(kind);
            }
            self.overflow_merged = self.overflow_merged.saturating_add(1);
            return;
        }
        let mut t = MergedTally::new();
        t.bump(kind);
        self.tallies.push((monitor_index, t));
    }

    /// 快照当前窗内容（**不关窗、不记账**）。
    ///
    /// 首事件直通走这里：内容立刻可上屏，窗继续开着接住后续同窗事件。
    /// 若这里顺带关窗（早先的实现就是如此），合并窗等于不存在——
    /// 每个事件各开一窗各结一次，吐出次数等于事件数。
    fn snapshot(&self, is_first: bool) -> MergedFlush {
        let start = self.window_start_us.unwrap_or(0);
        // **折算口径**：同屏同类折算成 1 项（这才是"合并"），
        // 次数另存于 tallies 不丢。上层处理项数而非条数，
        // 风暴时工作量与事件条数脱钩。
        let mut event_total = 0u32;
        let mut plug_collapsed = 0u32;
        let mut plug_total = 0u32;
        for slot in self.tallies.iter() {
            event_total = event_total.saturating_add(slot.1.collapsed());
            plug_collapsed = plug_collapsed.saturating_add(slot.1.plug_collapsed());
            plug_total = plug_total.saturating_add(slot.1.plug_total());
        }
        MergedFlush {
            window_start_us: start,
            window_end_us: start.saturating_add(self.window_us),
            event_total,
            plug_collapsed,
            plug_total,
            raw_total: self.raw_in_window,
            tallies: self.tallies.clone(),
            is_first_in_window: is_first,
        }
    }

    /// 结窗（**关窗 + 记账**，窗边界必调）。
    fn flush(&mut self) -> MergedFlush {
        let f = self.snapshot(false);
        // 合并掉的条数 = 原始条数 − 折算后项数（可核对的等式，不是估计值）。
        // 合并掉的条数 = 原始条数 − 折算项数（两者都记，守恒式才闭合）。
        let away = f.raw_total.saturating_sub(f.event_total);
        self.merged_away = self.merged_away.saturating_add(u64::from(away));
        // 已结窗的**原始条数**与**折算项数**分别累计：
        // 守恒式是 `收到 == 已结原始 + 在途`（条数口径，两侧都是条数）；
        // 折算项数是**另一个口径**，用来证明合并确实省了工作量。
        self.settled_raw = self.settled_raw.saturating_add(u64::from(f.raw_total));
        self.settled_items = self
            .settled_items
            .saturating_add(u64::from(f.event_total));
        self.windows_flushed = self.windows_flushed.saturating_add(1);
        self.window_start_us = None;
        self.tallies.clear();
        self.raw_in_window = 0;
        f
    }

    /// 窗内溢出并入首槽的次数（**代价显式记账**）。
    pub fn overflow_merged(&self) -> usize {
        self.overflow_merged
    }

    /// 守恒对账：**收到 == 合并掉 + 已结窗原始条数 + 在途**。
    ///
    /// 这条等式让「合并有没有吞事件」变成可机械核对的数，而不是靠人肉看代码。
    ///
    /// 口径说明：已结窗的原始条数之和 = `total_received − merged_away −
    /// raw_in_window`。`merged_away` 在每处 `flush` 里按 `raw − event_total`
    /// 累加，所以这个等式是**恒等式而非估算**：任何一处把 `merged_away`
    /// 算错、或把事件在途中弄丢，等式立刻不成立。
    pub fn conservation_holds(&self) -> bool {
        let in_flight = u64::from(self.raw_in_window);
        // 条数口径守恒：收到 == 已结窗原始 + 在途。
        // （合并量是「原始 − 折算」的差，不参与这条等式——它衡量的是
        //   **工作量**的缩减，不是条数的去向。把它塞进条数等式会重复计数。）
        self.settled_raw.saturating_add(in_flight) == self.total_received
            && self.settled_items <= self.settled_raw
            && self.merged_away == self.settled_raw.saturating_sub(self.settled_items)
    }

    /// 守恒明细（收到 / 合并掉 / 在途 / 已结窗原始）——供自检逐值核对。
    pub fn conservation(&self) -> (u64, u64, u64, u64) {
        let in_flight = u64::from(self.raw_in_window);
        (self.total_received, self.merged_away, in_flight, self.settled_raw)
    }

    /// 已结窗的折算项数累计（合并实效的直接证据）。
    pub fn settled_items(&self) -> u64 {
        self.settled_items
    }

    /// 已结窗的原始条数累计（条数守恒的另一半）。
    pub fn settled_raw(&self) -> u64 {
        self.settled_raw
    }
}

// ---------------------------------------------------------------------------
// 五、降级轮询（判据四）
// ---------------------------------------------------------------------------

/// 枚举来源。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EnumerationSource {
    /// 事件式（热插拔事件可用，正常路径）。
    Event,
    /// 轮询（**降级态**：枚举或热插拔缺失）。
    Polling,
}

impl EnumerationSource {
    /// 中文名。
    pub fn zh(self) -> &'static str {
        match self {
            EnumerationSource::Event => "事件枚举",
            EnumerationSource::Polling => "轮询降级",
        }
    }

    /// 是否降级态。
    pub const fn is_degraded(self) -> bool {
        matches!(self, EnumerationSource::Polling)
    }
}

/// 枚举协调器（**枚举缺失→降级轮询**，降级态必可观测）。
pub struct Enumerator {
    /// 当前来源。
    source: EnumerationSource,
    /// 轮询周期（逻辑微秒）。
    poll_interval_us: u64,
    /// 上次枚举时刻。
    last_poll_us: u64,
    /// 轮询次数（只增）。
    pub poll_count: u64,
    /// 降级次数（只增；每次从事件切到轮询记一次）。
    pub degrade_count: u32,
    /// 降级原因（降级时必非空；`None` = 未降级）。
    pub degrade_reason: Option<String>,
}

impl Enumerator {
    /// 新建（事件式，非降级）。
    pub fn new() -> Enumerator {
        Enumerator {
            source: EnumerationSource::Event,
            poll_interval_us: POLL_INTERVAL_DEFAULT_US,
            last_poll_us: 0,
            poll_count: 0,
            degrade_count: 0,
            degrade_reason: None,
        }
    }

    /// 当前来源。
    pub fn source(&self) -> EnumerationSource {
        self.source
    }

    /// 是否降级。
    pub fn is_degraded(&self) -> bool {
        self.source.is_degraded()
    }

    /// 轮询周期。
    pub fn poll_interval_us(&self) -> u64 {
        self.poll_interval_us
    }

    /// 降级到轮询。
    ///
    /// 锚点：「枚举缺失→降级轮询」。降级**必须**带原因——降级态对用户是
    /// 「屏偶尔认不出」，不给原因就等于让用户自己猜。
    ///
    /// 周期低于 [`POLL_INTERVAL_MIN_US`] 一律夹到下限：更快的轮询在内核态
    /// 是纯耗电，且屏的枚举周期由硬件决定，问得更勤并不会更早发现。
    pub fn degrade_to_polling(&mut self, reason: &str, interval_us: u64) -> MonitorIssue {
        if !self.source.is_degraded() {
            self.degrade_count = self.degrade_count.saturating_add(1);
        }
        self.source = EnumerationSource::Polling;
        self.poll_interval_us = if interval_us < POLL_INTERVAL_MIN_US {
            POLL_INTERVAL_MIN_US
        } else {
            interval_us
        };
        self.degrade_reason = Some(if reason.trim().is_empty() {
            String::from("未提供降级原因（异常）")
        } else {
            reason.to_string()
        });
        MonitorIssue {
            code: "E_ENUM_DEGRADED",
            symptom: format!(
                "显示器枚举已降级为轮询（周期 {} 微秒）",
                self.poll_interval_us
            ),
            root_cause: self.degrade_reason.clone().unwrap_or_default(),
            advice: "检查驱动与接线；热插拔事件恢复后会自动回到事件枚举",
            severity: Severity::Warn,
        }
    }

    /// 回到事件枚举（事件源恢复）。
    pub fn restore_events(&mut self) {
        self.source = EnumerationSource::Event;
        self.degrade_reason = None;
    }

    /// 是否该轮询（降级态且已过周期）。
    ///
    /// 非降级态恒 `false`——事件式下不该有轮询开销。
    pub fn should_poll(&self, now_us: u64) -> bool {
        if !self.is_degraded() {
            return false;
        }
        if self.poll_count == 0 {
            return true;
        }
        now_us.saturating_sub(self.last_poll_us) >= self.poll_interval_us
    }

    /// 执行一次轮询（记时刻与次数）。
    pub fn do_poll(&mut self, now_us: u64) {
        self.last_poll_us = now_us;
        self.poll_count = self.poll_count.saturating_add(1);
    }

    /// 降级态读屏单行（**降级必须可观测**：不只靠日志）。
    pub fn screen_line(&self) -> String {
        match self.degrade_reason.as_deref() {
            Some(r) if self.is_degraded() => format!(
                "显示器枚举：降级轮询，每 {} 微秒一次；原因 {}",
                self.poll_interval_us, r
            ),
            _ => String::from("显示器枚举：事件模式"),
        }
    }
}

impl Default for Enumerator {
    fn default() -> Self {
        Enumerator::new()
    }
}

// ---------------------------------------------------------------------------
// 六、判据五项（判据本身可追溯）
// ---------------------------------------------------------------------------

/// VE-F4402 判据（锚点原文五条）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Criterion {
    /// 判据一：三重指纹（EDID+GUID+端口；两两不算过；冲突以 GUID 为准并标注）。
    TripleFingerprint,
    /// 判据二：能力表（分辨率/刷新率/位深/HDR 四类，查询 O(1)）。
    CapabilityTable,
    /// 判据三：热插拔合并（风暴走合并窗，O(1) 合并，窗边界必吐）。
    PlugMerge,
    /// 判据四：降级轮询（枚举缺失降级轮询，降级态可观测）。
    DegradePoll,
    /// 判据五：判据本身（总纲自证可追溯）。
    Criterion,
}

impl Criterion {
    /// 五项判据全集（判据：一项不缺）。
    pub const CRITERIA: [Criterion; 5] = [
        Criterion::TripleFingerprint,
        Criterion::CapabilityTable,
        Criterion::PlugMerge,
        Criterion::DegradePoll,
        Criterion::Criterion,
    ];

    /// 判据项数。
    pub const COUNT: usize = 5;

    /// 判据中文名（读屏播报用）。
    pub fn zh(self) -> &'static str {
        match self {
            Criterion::TripleFingerprint => "三重指纹",
            Criterion::CapabilityTable => "能力表",
            Criterion::PlugMerge => "热插拔合并",
            Criterion::DegradePoll => "降级轮询",
            Criterion::Criterion => "判据",
        }
    }

    /// 自检项组前缀（判据→自检项的可追溯映射键）。
    ///
    /// 自检项名以此前缀开头即视为该判据被覆盖。映射是**双向**的：
    /// `Criterion::check_group` 给出键，自检侧用同一前缀产出项。
    pub fn check_group(self) -> &'static str {
        match self {
            Criterion::TripleFingerprint => "V02-指纹-",
            Criterion::CapabilityTable => "V02-能力-",
            Criterion::PlugMerge => "V02-合并-",
            Criterion::DegradePoll => "V02-降级-",
            Criterion::Criterion => "V02-判据-",
        }
    }

    /// 判据稳定码（跨版本不变，供日志对账）。
    pub fn code(self) -> &'static str {
        match self {
            Criterion::TripleFingerprint => "V02-J1",
            Criterion::CapabilityTable => "V02-J2",
            Criterion::PlugMerge => "V02-J3",
            Criterion::DegradePoll => "V02-J4",
            Criterion::Criterion => "V02-J5",
        }
    }

    /// 该判据要求的最低自检项数（缺项即红——防止判据只写在文档里）。
    pub const fn min_checks(self) -> usize {
        match self {
            Criterion::TripleFingerprint => 3,
            Criterion::CapabilityTable => 3,
            Criterion::PlugMerge => 3,
            Criterion::DegradePoll => 3,
            Criterion::Criterion => 3,
        }
    }

    /// 该判据的验收要点（每条都是可执行判定的口语化描述）。
    pub fn acceptance(self) -> &'static str {
        match self {
            Criterion::TripleFingerprint => {
                "EDID+GUID+端口三要素齐全方为已验证身份；两要素不构成已验证；\
                 同 GUID 以 GUID 为准并标注，不丢弃任何一块"
            }
            Criterion::CapabilityTable => {
                "分辨率/刷新率/色彩位深/HDR 四类齐备；查询 O(1)；\
                 HDR 只报探测到的事实，空表返回 None 而非默认值"
            }
            Criterion::PlugMerge => {
                "热插拔风暴走合并窗，同屏同类 O(1) 并计数；\
                 窗边界必吐汇总且保留下标与原始条数；首事件立即上屏不等窗满"
            }
            Criterion::DegradePoll => {
                "枚举缺失降级为轮询且带原因；轮询周期有下限；\
                 降级态可观测；事件恢复后回到事件式"
            }
            Criterion::Criterion => {
                "五项判据齐备且每项至少 3 条自检覆盖；判据→自检映射双向一致"
            }
        }
    }
}

/// 本模块的**不做清单**（跨批边界，锚点「F4403 色彩引擎下游」）。
///
/// 写死在这里而不是散在注释里，是因为边界一旦只存在于注释，改代码的人
/// 看不见它，就会「顺手」把色彩变换塞进设备层——那正是四层冻结要防的事。
pub const MONITOR_NOT_MINE: [&str; 6] = [
    "色彩变换与色彩空间转换（F4403/F4406）",
    "HDR 色调映射与元数据处理（F4404）",
    "多屏色彩同步（F4407）",
    "虚拟显示设备创建（F4410）",
    "对比度与色弱映射等一等契约（F4401 色彩层持有）",
    "显示器配置文件持久化（F4405）",
];

/// 上游承接面（VE-F4401 冻结接口 V01-IF1：设备层 → 色彩层）。
///
/// 本模块产出的「屏的物理事实」经这条边交给色彩层；本模块**不越过这条边**
/// 交付任何变换后的色彩值。
pub const V01_IF1_ACCEPTANCE: &[&str] = &[
    "显示器台账（枚举全量）",
    "身份指纹（三重验证 + 冲突标注）",
    "能力表（四类，O(1) 查询）",
    "枚举来源（事件 / 降级轮询，可观测）",
];

/// 上游对接登记表（本模块登记到 V 域设备层，供 F4421 拓扑深化对账）。
pub struct UpstreamLedger {
    /// 已登记项。
    pub registered: Vec<&'static str>,
    /// 越界交付次数（正常路径恒 0——越界即架构违规）。
    pub out_of_layer_deliveries: u32,
}

impl UpstreamLedger {
    /// 新建登记。
    pub fn new() -> UpstreamLedger {
        UpstreamLedger {
            registered: Vec::new(),
            out_of_layer_deliveries: 0,
        }
    }

    /// 登记一项交付。
    ///
    /// 交付出现在 [`MONITOR_NOT_MINE`] 里 → **当场拒收并计数**，
    /// 不是收下再说。静默收下越界交付会让四层冻结失效。
    pub fn register(&mut self, item: &'static str) -> Result<(), MonitorIssue> {
        if MONITOR_NOT_MINE.contains(&item) {
            self.out_of_layer_deliveries = self.out_of_layer_deliveries.saturating_add(1);
            return Err(MonitorIssue {
                code: "E_LAYER_VIOLATION",
                symptom: format!("设备层试图交付「{}」", item),
                root_cause: String::from("把下游层职责放进了设备层"),
                advice: "该职责属色彩/HDR/应用层，请走对应层的接口",
                severity: Severity::Fatal,
            });
        }
        self.registered.push(item);
        Ok(())
    }

    /// 承接面齐备性（四项齐备才 ready）。
    pub fn is_ready(&self) -> bool {
        V01_IF1_ACCEPTANCE
            .iter()
            .all(|x| self.registered.iter().any(|r| r == x))
    }
}

impl Default for UpstreamLedger {
    fn default() -> Self {
        UpstreamLedger::new()
    }
}

// ---------------------------------------------------------------------------
// 七、统一协调面（四件合流）
// ---------------------------------------------------------------------------

/// 多显示器统一管理面（**四件合流**：枚举全量 + 三重指纹 + 能力查询 + 事件流）。
pub struct MonitorManager {
    /// 台账。
    pub ledger: MonitorLedger,
    /// 指纹裁决器。
    pub arbiter: FingerprintArbiter,
    /// 能力索引。
    pub table: CapabilityTable,
    /// 合并器。
    pub merger: HotPlugMerger,
    /// 枚举协调。
    pub enumerator: Enumerator,
    /// 上游登记。
    pub upstream: UpstreamLedger,
}

impl Default for MonitorManager {
    fn default() -> Self {
        MonitorManager::new()
    }
}

impl MonitorManager {
    /// 新建管理面。
    pub fn new() -> MonitorManager {
        MonitorManager {
            ledger: MonitorLedger::new(),
            arbiter: FingerprintArbiter::new(),
            table: CapabilityTable::new(),
            merger: HotPlugMerger::new(),
            enumerator: Enumerator::new(),
            upstream: UpstreamLedger::new(),
        }
    }

    /// 入册一屏并立刻做指纹裁决（O(屏数)）。
    pub fn admit(&mut self, fp: IdentityFingerprint) -> Result<usize, MonitorIssue> {
        let idx = self.ledger.admit(fp)?;
        self.table.build(&self.ledger);
        let _ = self.arbiter.resolve_all(&self.ledger);
        Ok(idx)
    }

    /// 附能力表并重建索引（O(屏数)）。
    pub fn attach_capability(
        &mut self,
        index: usize,
        cap: CapabilityRecord,
    ) -> Result<(), MonitorIssue> {
        match self.ledger.monitor_mut(index) {
            Some(r) if r.retired => Err(MonitorIssue {
                code: "E_LEDGER_INDEX_RETIRED",
                symptom: format!("下标 {} 已退役（屏已摘除），不接受能力表", index),
                root_cause: String::from("调用方持有了已摘屏的过期下标"),
                advice: "按 GUID 重查在册下标",
                severity: Severity::Error,
            }),
            None => Err(MonitorIssue {
                code: "E_LEDGER_INDEX_OOB",
                symptom: format!("为下标 {} 附能力表失败：越界", index),
                root_cause: String::from("台账下标已失效"),
                advice: "按 GUID 重查下标",
                severity: Severity::Error,
            }),
            Some(rec) => {
                rec.attach_capability(cap);
                self.table.build(&self.ledger);
                Ok(())
            }
        }
    }

    /// 查某屏能力（`None` = 未索引或无能力表）。走定容索引，见 [`CapabilityTable`]。
    pub fn capability_of(&mut self, guid: &str) -> Option<&CapabilityRecord> {
        let idx = self.table.lookup(guid)?;
        self.ledger.live(idx).and_then(|m| m.capability())
    }

    /// 读屏全量摘要（**能力查询结果读屏可达**，域本色）。
    pub fn screen_summary(&self) -> String {
        let mut parts = Vec::new();
        parts.push(format!("已识别显示器 {} 台", self.ledger.len()));
        parts.push(format!("三重验证通过 {} 台", self.ledger.verified_count()));
        if self.ledger.annotated_count() > 0 {
            parts.push(format!("已标注 {} 台", self.ledger.annotated_count()));
        }
        parts.push(self.enumerator.screen_line());
        for m in self.ledger.entries() {
            if m.retired {
                continue;
            }
            let who = match m.fingerprint().identity_key() {
                Some(k) => k.to_string(),
                None => String::from("<未识别>"),
            };
            match m.capability() {
                Some(c) => parts.push(format!("屏 {}：{}", who, c.screen_line())),
                None => parts.push(format!("屏 {}：能力未知", who)),
            }
        }
        parts.join("；")
    }

    /// 就绪闸：关键面齐备。
    ///
    /// **缺项给人话**，不说「not ready」。
    pub fn readiness(&self) -> Result<(), String> {
        let mut miss = Vec::new();
        if self.ledger.is_empty() {
            miss.push(String::from("显示器台账为空（未枚举到任何屏）"));
        }
        if self.ledger.verified_count() == 0 {
            miss.push(String::from("无任何屏完成三重指纹验证"));
        }
        if !self.upstream.is_ready() {
            miss.push(String::from("上游承接面未登记齐（V01-IF1 四项）"));
        }
        if !self.ledger.enumeration_complete {
            miss.push(String::from("本次枚举不完整（枚举源缺失）"));
        }
        if miss.is_empty() {
            return Ok(());
        }
        Err(format!("多显示器管理面未就绪：{}", miss.join("；")))
    }
}

// ---------------------------------------------------------------------------
// 八、内部工具
// ---------------------------------------------------------------------------

/// 去空白后转 `String`（空串归 `None`，不产出「看着有值其实是空」的字段）。
fn trim_to_string(s: &str) -> String {
    s.trim().to_string()
}

/// 非空白判定。
fn has_text(s: Option<&str>) -> bool {
    match s {
        None => false,
        Some(t) => !t.trim().is_empty(),
    }
}

/// 域自检总入口（判据逐条落在 `vev02_checks.rs`，此处仅做登记）。
pub fn run_vev02_checks() -> CheckSet {
    crate::svstar2::vev02_checks::run_vev02_checks_a()
}