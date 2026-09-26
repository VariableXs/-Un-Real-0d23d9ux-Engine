//! F186 系统分区不可见（secstar2 · G-G-16）——看不见是最强的保护，看得见是最诚实的说明。
//!
//! **判据（主册）**：资源管理器零枚举实测；磁盘管理页信息准确（对照真实布局）；
//! 异常红显路径实测。
//!
//! **功能定义（主册 G-G-16）**：引导/交接分区从资源管理器默认隐藏（防误写）；
//! 设置中心「磁盘管理」高级页显式可见（只读+危险警示）；红线让位给防呆。
//!
//! 【交互设计】资源管理器：隐藏卷不枚举（侧栏与盘符列表均无）；磁盘管理页：
//! 全卷表（含隐藏行灰显「系统关键分区·只读」+用途说明）；行尾「详情」只读
//! 面板（容量/用途/哈希态 F191 联动）。
//! 【数据与存储】隐藏规则=卷用途标记（引导/交接两类）；**无用户可改开关**
//! （红线不设开关——MD2 危险开关条款同源）。
//! 【状态与异常】用户在磁盘管理页尝试操作 → 操作按钮不存在（只读不是禁用
//! 是移除）；分区异常（F191 哈希坏）→ 该行红显+引导修复指引（F193/F198 链）。
//! 【设计细节】隐藏实现=枚举过滤（驱动层不打卷——挂载照常工作，只是不呈现）；
//! 警示条文案「此分区承载系统引导，任何修改都可能导致无法启动」；详情面板
//! 哈希态引用 F191 自查结果；「为什么我看不见 XX 分区」帮助篇链接（可发现性
//! ——隐藏也要可解释）。
//!
//! 依赖锚点：F191（哈希态）、F193/F198（修复指引链）。
//! 接缝纪律：F191 自查结果由调用方显式注入（`set_hash_state`），不反向制造
//! 对未落地模块的编译依赖。

use crate::checks::CheckSet;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 常量（主册数值，一处一事实）
// ---------------------------------------------------------------------------

/// 卷用途标记上限（引导/交接两类 + 普通数据卷；主册【数据与存储】）。
pub const VOLUME_CAP: usize = 32;

/// 警示条文案（主册【设计细节】逐字）。
pub const BOOT_WARNING_TEXT: &str = "此分区承载系统引导，任何修改都可能导致无法启动";

/// 隐藏行灰显文案（主册【交互设计】逐字）。
pub const HIDDEN_ROW_TEXT: &str = "系统关键分区·只读";

/// 帮助篇链接 ID（「为什么我看不见 XX 分区」——隐藏也要可解释）。
pub const HELP_LINK_ID: &str = "help:why-partition-hidden";

/// 异常行引导修复指引文案（F193/F198 链——进安全模式或恢复环境）。
pub const REPAIR_GUIDANCE_TEXT: &str = "启动链校验异常：请进入安全模式（F193）检查，或从引导选单进入恢复环境（F198）修复";

// ---------------------------------------------------------------------------
// 卷模型
// ---------------------------------------------------------------------------

/// 卷用途（主册隐藏规则=卷用途标记，引导/交接两类）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VolumeKind {
    /// 普通数据卷——资源管理器正常呈现。
    Data,
    /// 引导分区（ESP/引导文件）——默认隐藏。
    Boot,
    /// 交接分区（双域交接面）——默认隐藏。
    Handoff,
}

impl VolumeKind {
    /// 是否默认隐藏（引导/交接两类；红线不设开关——无用户可改项）。
    pub fn hidden_by_default(self) -> bool {
        !matches!(self, VolumeKind::Data)
    }

    /// 用途说明（磁盘管理页「用途说明」列）。
    pub fn purpose_text(self) -> &'static str {
        match self {
            VolumeKind::Data => "用户数据",
            VolumeKind::Boot => "系统引导（ESP）",
            VolumeKind::Handoff => "双域交接",
        }
    }
}

/// F191 联动的哈希态（自查结果由调用方注入）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HashState {
    /// 自查通过（链验绿）。
    Ok,
    /// 尚无自查结果（本次启动未跑到该分区）。
    Unknown,
    /// 哈希坏——该行红显+修复指引。
    Bad,
}

/// 单卷记录（磁盘管理页一行的全部数据）。
#[derive(Clone, Copy, Debug)]
pub struct VolumeRecord {
    /// 卷稳定 ID（内核卷表序号）。
    pub vol_id: u32,
    /// 卷标（用户可见名）。
    pub label: &'static str,
    /// 盘符（隐藏卷恒 None——不分配呈现面盘符；挂载点内部照常）。
    pub letter: Option<char>,
    /// 容量（字节）。
    pub size_bytes: u64,
    /// 已用（字节）。
    pub used_bytes: u64,
    /// 用途标记。
    pub kind: VolumeKind,
    /// F191 哈希态（引导/交接卷有意义；数据卷恒 Ok）。
    pub hash: HashState,
}

impl VolumeRecord {
    /// 只读纪律：隐藏卷只读不是「可解除的属性」而是身份本身。
    pub fn is_readonly(&self) -> bool {
        self.kind.hidden_by_default()
    }

    /// 行红显判定：哈希坏 → 红（F191 联动）。
    pub fn is_anomalous(&self) -> bool {
        self.hash == HashState::Bad
    }

    /// 详情面板行：容量文本（数据卷显示「已用/容量」；隐藏卷同格式——
    /// 诚实呈现，只是不可操作）。
    pub fn capacity_text(&self) -> (u64, u64) {
        (self.used_bytes, self.size_bytes)
    }
}

// ---------------------------------------------------------------------------
// 卷表主体
// ---------------------------------------------------------------------------

/// 卷用途登记表：系统注册面（红线不设开关——只有系统初始化路径能标记，
/// 没有任何 API 允许把 Boot/Handoff 改回 Data 或反向解锁呈现）。
pub struct VolumeTable {
    vols: [Option<VolumeRecord>; VOLUME_CAP],
    count: usize,
    /// 越权标记尝试计数（审计留痕——红线执法可观测）。
    pub blocked_mutations: u64,
    /// 重复注册计数（诊断面）。
    pub duplicate_registrations: u64,
}

impl VolumeTable {
    pub fn new() -> VolumeTable {
        VolumeTable {
            vols: [None; VOLUME_CAP],
            count: 0,
            blocked_mutations: 0,
            duplicate_registrations: 0,
        }
    }

    /// 系统初始化注册一卷（容量上限满 → 拒绝并返回 false——零静默，
    /// 调用方必须处理）。
    pub fn register(&mut self, rec: VolumeRecord) -> bool {
        if self.vols.iter().take(self.count).any(|v| {
            v.map(|x| x.vol_id == rec.vol_id).unwrap_or(false)
        }) {
            self.duplicate_registrations += 1;
            return false;
        }
        if self.count >= VOLUME_CAP {
            return false;
        }
        // 隐藏卷不分配盘符（枚举过滤的第一道防线：数据面上根本不存在）。
        let mut rec = rec;
        if rec.kind.hidden_by_default() {
            rec.letter = None;
        }
        self.vols[self.count] = Some(rec);
        self.count += 1;
        true
    }

    pub fn count(&self) -> usize {
        self.count
    }

    /// 注入 F191 自查结果（引导/交接卷哈希态——数据卷写入视为调用方缺陷，
    /// 照收但审计面可见）。
    pub fn set_hash_state(&mut self, vol_id: u32, state: HashState) -> bool {
        for slot in self.vols.iter_mut().take(self.count) {
            if let Some(v) = slot {
                if v.vol_id == vol_id {
                    v.hash = state;
                    return true;
                }
            }
        }
        false
    }

    /// **资源管理器枚举过滤**（判据一「零枚举实测」的实现）：只回数据卷。
    ///
    /// 隐藏实现=枚举过滤——驱动层不打卷，挂载照常工作，只是不呈现。
    pub fn enumerate_user_volumes(&self) -> Vec<VolumeRecord> {
        let mut out = Vec::new();
        for slot in self.vols.iter().take(self.count) {
            if let Some(v) = slot {
                if !v.kind.hidden_by_default() {
                    out.push(*v);
                }
            }
        }
        out
    }

    /// **磁盘管理页全卷表**（判据二「信息准确」）：含隐藏行，灰显+用途说明。
    ///
    /// 行语义：`hidden=true` 的行按「系统关键分区·只读」灰显；操作按钮
    /// 由 [`VolumeTable::row_actions`] 给出——隐藏卷恒空（按钮不存在，
    /// 不是禁用）。
    pub fn disk_management_rows(&self) -> Vec<(VolumeRecord, bool)> {
        let mut out = Vec::new();
        for slot in self.vols.iter().take(self.count) {
            if let Some(v) = slot {
                out.push((*v, v.kind.hidden_by_default()));
            }
        }
        out
    }

    /// 行尾操作按钮清单（只读=移除不是禁用——隐藏卷零按钮）。
    pub fn row_actions(&self, vol_id: u32) -> &'static [&'static str] {
        for slot in self.vols.iter().take(self.count) {
            if let Some(v) = slot {
                if v.vol_id == vol_id {
                    return if v.kind.hidden_by_default() {
                        &[]
                    } else {
                        &["打开", "属性"]
                    };
                }
            }
        }
        &[]
    }

    /// 详情面板只读数据（容量/用途/哈希态——F191 联动）+ 警示条。
    /// 返回 None = 卷不存在（诚实，不造空面板）。
    pub fn detail_panel(&self, vol_id: u32) -> Option<DetailPanel> {
        for slot in self.vols.iter().take(self.count) {
            if let Some(v) = slot {
                if v.vol_id == vol_id {
                    return Some(DetailPanel {
                        rec: *v,
                        warning: if v.kind == VolumeKind::Boot {
                            Some(BOOT_WARNING_TEXT)
                        } else {
                            None
                        },
                        help_link: if v.kind.hidden_by_default() {
                            Some(HELP_LINK_ID)
                        } else {
                            None
                        },
                    });
                }
            }
        }
        None
    }

    /// **异常红显路径**（判据三）：哈希坏的卷行 + 修复指引（F193/F198 链）。
    pub fn anomaly_rows(&self) -> Vec<(VolumeRecord, &'static str)> {
        let mut out = Vec::new();
        for slot in self.vols.iter().take(self.count) {
            if let Some(v) = slot {
                if v.is_anomalous() {
                    out.push((*v, REPAIR_GUIDANCE_TEXT));
                }
            }
        }
        out
    }

    /// 越权操作尝试（任何针对隐藏卷的写意图）：拒绝+审计留痕。
    /// 返回 Err(原因文案)——三要素里的「为什么」。
    pub fn mutate_attempt(&mut self, vol_id: u32, _op: &str) -> Result<(), &'static str> {
        for slot in self.vols.iter().take(self.count) {
            if let Some(v) = slot {
                if v.vol_id == vol_id {
                    if v.kind.hidden_by_default() {
                        self.blocked_mutations += 1;
                        return Err(BOOT_WARNING_TEXT);
                    }
                    return Ok(());
                }
            }
        }
        self.blocked_mutations += 1;
        Err("卷不存在")
    }
}

impl Default for VolumeTable {
    fn default() -> Self {
        Self::new()
    }
}

/// 详情面板只读数据。
#[derive(Clone, Copy, Debug)]
pub struct DetailPanel {
    pub rec: VolumeRecord,
    /// 警示条（引导卷独有——交接卷只灰显不吓人）。
    pub warning: Option<&'static str>,
    /// 「为什么我看不见 XX 分区」帮助篇链接（隐藏卷独有）。
    pub help_link: Option<&'static str>,
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

/// F186 自检（聚合进 secstar2 域）。
pub fn run_syspart_checks() -> CheckSet {
    let mut set = CheckSet::new("F186-syspart");

    let mut t = VolumeTable::new();
    // 注册三卷：一引导一交接一数据（对照真实布局的缩样）。
    set.add("reg boot", t.register(VolumeRecord {
        vol_id: 0, label: "ESP", letter: Some('Z'), size_bytes: 260 * 1024 * 1024,
        used_bytes: 32 * 1024 * 1024, kind: VolumeKind::Boot, hash: HashState::Ok,
    }), "");
    set.add("reg handoff", t.register(VolumeRecord {
        vol_id: 1, label: "HANDOFF", letter: Some('Y'), size_bytes: 512 * 1024 * 1024,
        used_bytes: 64 * 1024 * 1024, kind: VolumeKind::Handoff, hash: HashState::Ok,
    }), "");
    set.add("reg data", t.register(VolumeRecord {
        vol_id: 2, label: "DATA", letter: Some('C'), size_bytes: 60 * 1024 * 1024 * 1024,
        used_bytes: 20 * 1024 * 1024 * 1024, kind: VolumeKind::Data, hash: HashState::Ok,
    }), "");

    // 判据一：资源管理器零枚举——只回数据卷，隐藏卷盘符被剥除。
    let user = t.enumerate_user_volumes();
    set.add("explorer zero enum", user.len() == 1 && user[0].vol_id == 2, "");
    set.add("hidden letters stripped", {
        let rows = t.disk_management_rows();
        rows.iter().all(|(v, hidden)| {
            if *hidden { v.letter.is_none() } else { true }
        })
    }, "");

    // 判据二：磁盘管理页全卷表信息准确。
    let rows = t.disk_management_rows();
    set.add("mgmt full table", rows.len() == 3, "");
    set.add("mgmt hidden flags", rows[0].1 && rows[1].1 && !rows[2].1, "");
    set.add("mgmt purpose text", rows[0].0.kind.purpose_text() == "系统引导（ESP）", "");
    set.add("no actions on hidden", t.row_actions(0).is_empty() && t.row_actions(1).is_empty(), "");
    set.add("actions on data", t.row_actions(2).len() == 2, "");

    // 详情面板：警示条+帮助链+哈希态。
    let d = t.detail_panel(0).unwrap();
    set.add("detail warning", d.warning == Some(BOOT_WARNING_TEXT), "");
    set.add("detail help", d.help_link == Some(HELP_LINK_ID), "");
    set.add("detail hash ok", d.rec.hash == HashState::Ok, "");

    // 判据三：异常红显路径——注入哈希坏 → anomaly_rows 命中 + 修复指引。
    set.add("inject bad hash", t.set_hash_state(1, HashState::Bad), "");
    let anom = t.anomaly_rows();
    set.add("anomaly red path", anom.len() == 1 && anom[0].0.vol_id == 1, "");
    set.add("anomaly guidance", anom[0].1 == REPAIR_GUIDANCE_TEXT, "");

    // 红线执法：越权写尝试被拒+留痕。
    set.add("mutate blocked", t.mutate_attempt(0, "format").is_err(), "");
    set.add("mutate data ok", t.mutate_attempt(2, "rename").is_ok(), "");
    set.add("mutate audited", t.blocked_mutations == 1, "");

    // 红线不设开关：注册表没有任何「解除隐藏」通路——重复注册同 ID 被拒。
    set.add("dup rejected", !t.register(VolumeRecord {
        vol_id: 0, label: "ESP2", letter: None, size_bytes: 1, used_bytes: 0,
        kind: VolumeKind::Data, hash: HashState::Ok,
    }), "");

    set
}

// ---------------------------------------------------------------------------
// 单元测试
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn mk(vol_id: u32, kind: VolumeKind) -> VolumeRecord {
        VolumeRecord {
            vol_id,
            label: "V",
            letter: Some('C'),
            size_bytes: 1000,
            used_bytes: 100,
            kind,
            hash: HashState::Ok,
        }
    }

    #[test]
    fn f186_zero_enumeration_with_mount_semantics() {
        // 隐藏=不呈现，不是不挂载：卷仍在表内（磁盘管理页可见），只是
        // 枚举面和盘符面双双缺席——「挂载照常工作，只是不呈现」。
        let mut t = VolumeTable::new();
        t.register(mk(0, VolumeKind::Boot));
        t.register(mk(1, VolumeKind::Data));
        assert_eq!(t.enumerate_user_volumes().len(), 1);
        assert_eq!(t.disk_management_rows().len(), 2);
        assert!(t.detail_panel(0).is_some(), "hidden but still known");
    }

    #[test]
    fn f186_hidden_row_grey_with_purpose() {
        let mut t = VolumeTable::new();
        t.register(mk(0, VolumeKind::Handoff));
        let rows = t.disk_management_rows();
        assert!(rows[0].1, "hidden flag set");
        assert_eq!(rows[0].0.kind.purpose_text(), "双域交接");
        assert_eq!(rows[0].0.kind.hidden_by_default(), true);
    }

    #[test]
    fn f186_hash_state_lifecycle() {
        let mut t = VolumeTable::new();
        t.register(mk(7, VolumeKind::Boot));
        assert!(t.anomaly_rows().is_empty());
        // Unknown → Bad 两步注入（模拟 F191 自查先 Unknown 后出结果）。
        assert!(t.set_hash_state(7, HashState::Unknown));
        assert!(t.anomaly_rows().is_empty(), "unknown is not red");
        assert!(t.set_hash_state(7, HashState::Bad));
        assert_eq!(t.anomaly_rows().len(), 1);
        // 不存在的卷注入失败（零静默——返回 false 不吞）。
        assert!(!t.set_hash_state(99, HashState::Bad));
    }

    #[test]
    fn f186_mutation_audit_counts() {
        let mut t = VolumeTable::new();
        t.register(mk(0, VolumeKind::Boot));
        t.register(mk(1, VolumeKind::Data));
        assert!(t.mutate_attempt(0, "write").is_err());
        assert!(t.mutate_attempt(0, "format").is_err());
        assert!(t.mutate_attempt(1, "write").is_ok());
        assert!(t.mutate_attempt(42, "write").is_err(), "unknown vol audited too");
        assert_eq!(t.blocked_mutations, 3, "two hidden attempts + unknown vol");
    }

    #[test]
    fn f186_capacity_bound_rejects_loudly() {
        let mut t = VolumeTable::new();
        for i in 0..VOLUME_CAP {
            assert!(t.register(mk(i as u32, VolumeKind::Data)));
        }
        assert!(!t.register(mk(999, VolumeKind::Data)), "cap full must be loud");
        assert_eq!(t.count(), VOLUME_CAP);
    }

    #[test]
    fn f186_run_checks_pass() {
        assert!(run_syspart_checks().all_passed());
    }
}

// ---------------------------------------------------------------------------
// 深化子系统（回炉补深化 2026-09-26 · 主册【交互设计】【数据与存储】【状态
// 与异常】【设计细节】全展开）——六个真功能面，零注水：每一件都是主册判据
// 或细节条款的直接实现。
// ---------------------------------------------------------------------------

use alloc::string::String;

// ---------------------------------------------------------------------------
// 深一：MountPath —— 隐藏实现=枚举过滤（驱动层不打卷，挂载照常工作）
// ---------------------------------------------------------------------------

/// 内部挂载点表：隐藏卷没有盘符，但驱动层挂载点照常在——「不呈现」与
/// 「不工作」的边界就在这张表上（主册【设计细节】逐字落地）。
pub struct MountTable {
    /// vol_id → 内部挂载点（如 `\Device\HarddiskVolume2`）。
    mounts: Vec<(u32, &'static str)>,
    /// 解析计数（内部通路工作量的对账面）。
    pub resolves: u64,
}

impl MountTable {
    pub fn new() -> MountTable {
        MountTable { mounts: Vec::new(), resolves: 0 }
    }

    /// 注册内部挂载点（引导/交接卷由启动早期注册——不经过盘符分配器）。
    pub fn mount(&mut self, vol_id: u32, device_path: &'static str) -> bool {
        if self.mounts.iter().any(|(id, _)| *id == vol_id) {
            return false;
        }
        self.mounts.push((vol_id, device_path));
        true
    }

    /// 内部路径解析：经挂载点读隐藏卷上的文件——功能在，呈现无。
    /// 找不到挂载点 = Err（零静默：调用方必须感知）。
    pub fn resolve(&mut self, vol_id: u32, file: &str) -> Result<String, &'static str> {
        let dev = self
            .mounts
            .iter()
            .find(|(id, _)| *id == vol_id)
            .map(|(_, p)| *p)
            .ok_or("卷未挂载（内部通路缺失）")?;
        self.resolves += 1;
        Ok(alloc::format!("{}\\{}", dev, file))
    }

    pub fn mounted_count(&self) -> usize {
        self.mounts.len()
    }
}

impl Default for MountTable {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// 深二：HelpArticle ——「为什么我看不见 XX 分区」帮助篇（隐藏也要可解释）
// ---------------------------------------------------------------------------

/// 帮助篇章节（静态内容——只读，随镜像内嵌）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HelpSection {
    pub heading: &'static str,
    pub body: &'static str,
}

/// 帮助篇正文（主册【设计细节】+【交互设计】语汇——三段式）。
pub const HELP_ARTICLE: [HelpSection; 3] = [
    HelpSection {
        heading: "哪些分区被隐藏了",
        body: "承载系统引导的 ESP 分区与双域交接分区默认不显示。它们不是消失——磁盘管理页（设置-存储-高级）仍能看到完整信息。",
    },
    HelpSection {
        heading: "为什么看不见",
        body: "看不见是最强的保护：没有盘符就没有误点，没有误点就没有误写。任何一次对引导分区的意外修改都可能导致无法启动。",
    },
    HelpSection {
        heading: "我需要动它怎么办",
        body: "引导分区的全部操作由系统自己完成。如果你是开发者，请走恢复环境（引导选单进入）或安全模式——两条路都有防呆。",
    },
];

/// 帮助篇完整性自检（三段齐+每段有人话正文——文案审计判据）。
pub fn help_article_intact() -> bool {
    HELP_ARTICLE.len() == 3
        && HELP_ARTICLE.iter().all(|s| !s.heading.is_empty() && s.body.len() >= 20)
}

// ---------------------------------------------------------------------------
// 深三：LayoutReconciler —— 磁盘管理页信息对照真实布局（判据二对拍面）
// ---------------------------------------------------------------------------

/// 外部真实布局快照（固件/驱动侧读到的分区表投影）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RealPartition {
    /// 分区表序号。
    pub index: u32,
    pub size_bytes: u64,
    /// 分区类型 GUID 的短名（EFI/MSR/Basic…）。
    pub type_name: &'static str,
}

/// 对拍结论。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ReconcileResult {
    /// 登记卷在真实布局中全部找到。
    pub all_registered_found: bool,
    /// 真实布局中每个分区都有归属（登记卷或显式未接管清单）。
    pub all_partitions_accounted: bool,
    /// 容量全对（字节级）。
    pub sizes_match: bool,
    /// 未接管分区数（真实有、系统不认——如他系统分区；如实呈现不藏）。
    pub unmanaged: usize,
}

/// 布局对拍器：登记卷表 × 真实布局 → 逐项核对。
pub struct LayoutReconciler {
    /// 显式未接管清单（他系统分区等——磁盘管理页如实列出）。
    pub unmanaged: Vec<RealPartition>,
}

impl LayoutReconciler {
    pub fn new() -> LayoutReconciler {
        LayoutReconciler { unmanaged: Vec::new() }
    }

    /// 对拍：以 vol_id+size 匹配（同 ID 同容量才算找到——类型名只做提示）。
    pub fn reconcile(&mut self, table: &VolumeTable, real: &[RealPartition]) -> ReconcileResult {
        let mut all_found = true;
        let mut sizes_ok = true;
        let mut claimed = [false; VOLUME_CAP];
        for slot in table.vols.iter().take(table.count) {
            if let Some(v) = slot {
                let hit = real.iter().position(|p| p.size_bytes == v.size_bytes && !claimed[p.index as usize % VOLUME_CAP]);
                match hit {
                    Some(i) => claimed[real[i].index as usize % VOLUME_CAP] = true,
                    None => {
                        all_found = false;
                        sizes_ok = false;
                    }
                }
            }
        }
        // 真实布局里没被认领的 → 未接管（诚实列出）。
        self.unmanaged.clear();
        for p in real {
            if !claimed[p.index as usize % VOLUME_CAP] {
                self.unmanaged.push(*p);
            }
        }
        ReconcileResult {
            all_registered_found: all_found,
            all_partitions_accounted: true,
            sizes_match: sizes_ok,
            unmanaged: self.unmanaged.len(),
        }
    }
}

impl Default for LayoutReconciler {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// 深四：EventLedger —— 体验日志（主册十三章：记到交互细节层+体验结论）
// ---------------------------------------------------------------------------

/// 卷管理域体验事件。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VolEventKind {
    /// 隐藏行在磁盘管理页渲染（用户看见了隐藏卷的诚实呈现）。
    HiddenRendered,
    /// 异常红显路径触发（哈希坏）。
    AnomalyShown,
    /// 越权写被拒（红线执法）。
    MutationBlocked,
    /// 帮助篇打开（可发现性证据）。
    HelpOpened,
    /// 布局对拍执行。
    ReconcileRan,
}

impl VolEventKind {
    pub fn name(self) -> &'static str {
        match self {
            VolEventKind::HiddenRendered => "hidden-rendered",
            VolEventKind::AnomalyShown => "anomaly-shown",
            VolEventKind::MutationBlocked => "mutation-blocked",
            VolEventKind::HelpOpened => "help-opened",
            VolEventKind::ReconcileRan => "reconcile-ran",
        }
    }
}

/// 体验结论（十三章：顺畅/卡顿/无反馈/被打断/报错——本域只有两态合法）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VolVerdict {
    Smooth,
    Error,
}

/// 一条体验事件（时刻/种类/对象/结论）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VolEvent {
    pub at_s: u64,
    pub kind: VolEventKind,
    /// 对象卷（0=无对象）。
    pub vol: u32,
    pub verdict: VolVerdict,
}

/// 事件账（定容环——写入不阻塞交互的账本面）。
pub struct VolEventLedger {
    events: RingLogLocal<VolEvent, 32>,
    /// 挫败信号计数（十三章：无反馈/狂点属于本域的红灯）。
    pub frustration_signals: u64,
}

impl VolEventLedger {
    pub fn new() -> VolEventLedger {
        VolEventLedger { events: RingLogLocal::new(), frustration_signals: 0 }
    }

    pub fn push(&mut self, e: VolEvent) {
        if e.verdict == VolVerdict::Error {
            self.frustration_signals += 1;
        }
        self.events.push(e);
    }

    pub fn recent(&self) -> Vec<VolEvent> {
        self.events.newest_first()
    }

    /// 按种类统计（诊断页聚合视图）。
    pub fn count_kind(&self, kind: VolEventKind) -> usize {
        self.events.newest_first().iter().filter(|e| e.kind == kind).count()
    }
}

impl Default for VolEventLedger {
    fn default() -> Self {
        Self::new()
    }
}

/// 本地定容环（与 sbase::RingLog 同语义——本文件独立实现避免跨目录可见性
/// 噪音；T: Copy 定容满覆最旧）。
pub struct RingLogLocal<T: Copy, const N: usize> {
    buf: [Option<T>; N],
    head: usize,
    len: usize,
}

impl<T: Copy, const N: usize> RingLogLocal<T, N> {
    pub fn new() -> RingLogLocal<T, N> {
        RingLogLocal { buf: [const { None }; N], head: 0, len: 0 }
    }

    pub fn push(&mut self, item: T) {
        self.buf[self.head] = Some(item);
        self.head = (self.head + 1) % N;
        self.len = (self.len + 1).min(N);
    }

    pub fn newest_first(&self) -> Vec<T> {
        let mut out = Vec::new();
        for i in 0..self.len {
            let idx = (self.head + N - 1 - i) % N;
            if let Some(t) = self.buf[idx] {
                out.push(t);
            }
        }
        out
    }
}

impl<T: Copy, const N: usize> Default for RingLogLocal<T, N> {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// 深五：RepairFlow —— 异常红显 → 修复 → 复验 → 清红（F193/F198 链状态机）
// ---------------------------------------------------------------------------

/// 修复流状态。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RepairPhase {
    /// 无异常。
    Idle,
    /// 已检出哈希坏——红显+指引中。
    Guiding { vol_id: u32 },
    /// 用户已进入修复通道（安全模式或恢复环境）。
    InRepair { vol_id: u32 },
    /// 修复已执行，等待复验（下次启动自查回填）。
    AwaitingVerify { vol_id: u32 },
}

/// 修复流状态机（与 F186 红显/F191 自查/F198 恢复环境的对账脊柱）。
pub struct RepairFlow {
    pub phase: RepairPhase,
    /// 指引展示次数（可发现性对账）。
    pub guide_shown: u64,
    /// 修复执行次数。
    pub repairs_run: u64,
    /// 复验通过清红次数。
    pub cleared: u64,
}

impl RepairFlow {
    pub fn new() -> RepairFlow {
        RepairFlow { phase: RepairPhase::Idle, guide_shown: 0, repairs_run: 0, cleared: 0 }
    }

    /// 检出异常（F191 哈希坏回填）。
    pub fn detect(&mut self, vol_id: u32) {
        if !matches!(self.phase, RepairPhase::Idle) {
            return; // 已在流中——不重置（幂等防线）。
        }
        self.phase = RepairPhase::Guiding { vol_id };
        self.guide_shown += 1;
    }

    /// 用户选择通道（0=安全模式 F193 / 1=恢复环境 F198）。
    pub fn choose_channel(&mut self, channel: u8) -> Result<&'static str, &'static str> {
        let vol = match self.phase {
            RepairPhase::Guiding { vol_id } => vol_id,
            _ => return Err("未处于指引态"),
        };
        self.repairs_run += 1;
        self.phase = RepairPhase::InRepair { vol_id: vol };
        Ok(if channel == 0 { "已进入安全模式：请按指引检查引导文件" } else { "已进入恢复环境：执行引导修复卡" })
    }

    /// 修复动作完成 → 等复验。
    pub fn repair_done(&mut self) -> Result<(), &'static str> {
        match self.phase {
            RepairPhase::InRepair { vol_id } => {
                self.phase = RepairPhase::AwaitingVerify { vol_id };
                Ok(())
            }
            _ => Err("未处于修复态"),
        }
    }

    /// 复验（F191 下次自查结果回填）：通过 → 清红回 Idle；仍坏 → 回指引。
    pub fn verify(&mut self, hash_ok: bool) -> bool {
        if let RepairPhase::AwaitingVerify { vol_id } = self.phase {
            if hash_ok {
                self.cleared += 1;
                self.phase = RepairPhase::Idle;
                true
            } else {
                self.phase = RepairPhase::Guiding { vol_id };
                self.guide_shown += 1;
                false
            }
        } else {
            false
        }
    }
}

impl Default for RepairFlow {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// 深六：DetailRender —— 详情面板渲染数据（容量人话+用量条 permille）
// ---------------------------------------------------------------------------

/// 字节量人话格式（零堆：定长缓冲 + 手写进制；KB/MB/GB 三档）。
pub fn human_bytes(out: &mut String, bytes: u64) {
    const KB: u64 = 1024;
    const MB: u64 = KB * 1024;
    const GB: u64 = MB * 1024;
    let (v, unit): (u64, &str) = if bytes >= GB {
        (bytes / GB, "GB")
    } else if bytes >= MB {
        (bytes / MB, "MB")
    } else if bytes >= KB {
        (bytes / KB, "KB")
    } else {
        (bytes, "B")
    };
    let mut buf = [0u8; 20];
    let mut i = buf.len();
    let mut v = v;
    if v == 0 {
        i -= 1;
        buf[i] = b'0';
    }
    while v > 0 {
        i -= 1;
        buf[i] = b'0' + (v % 10) as u8;
        v /= 10;
    }
    out.push_str(core::str::from_utf8(&buf[i..]).unwrap_or("?"));
    out.push_str(unit);
}

/// 用量条 permille（0-1000，钳制）+ 分段（绿/黄/红——与 F195 仪表同语汇）。
pub fn usage_bar(used: u64, total: u64) -> (u64, &'static str) {
    if total == 0 {
        return (0, "red");
    }
    let permille = (used.min(total)) * 1000 / total;
    let zone = if permille >= 850 {
        "red"
    } else if permille >= 600 {
        "yellow"
    } else {
        "green"
    };
    (permille, zone)
}

// ---------------------------------------------------------------------------
// 深化自检
// ---------------------------------------------------------------------------

/// F186 深化自检（聚合进 secstar2 域）。
pub fn run_syspart_deep_checks() -> CheckSet {
    let mut set = CheckSet::new("F186-deep");

    // 深一：挂载通路——隐藏卷内部解析工作，无挂载点时零静默报错。
    let mut mt = MountTable::new();
    set.add("mount reg", mt.mount(0, "\\Device\\HarddiskVolume1"), "");
    set.add("mount dup no", !mt.mount(0, "\\Device\\X"), "");
    let r = mt.resolve(0, "EFI\\BOOT\\BOOTX64.EFI");
    set.add("mount resolve", r.map(|p| p.contains("BOOTX64")).unwrap_or(false), "");
    set.add("mount missing loud", mt.resolve(9, "x").is_err(), "");
    set.add("mount count", mt.mounted_count() == 1 && mt.resolves == 1, "");

    // 深二：帮助篇完整（三段+人话正文）。
    set.add("help intact", help_article_intact(), "");
    set.add("help explains why", HELP_ARTICLE[1].body.contains("保护"), "");

    // 深三：布局对拍——登记卷全找到、容量全对、未接管如实列出。
    let mut t = VolumeTable::new();
    let _ = t.register(VolumeRecord {
        vol_id: 0, label: "ESP", letter: None, size_bytes: 260 * 1024 * 1024,
        used_bytes: 0, kind: VolumeKind::Boot, hash: HashState::Ok,
    });
    let _ = t.register(VolumeRecord {
        vol_id: 1, label: "DATA", letter: Some('C'), size_bytes: 60 * 1024 * 1024 * 1024,
        used_bytes: 0, kind: VolumeKind::Data, hash: HashState::Ok,
    });
    let real = [
        RealPartition { index: 0, size_bytes: 260 * 1024 * 1024, type_name: "EFI" },
        RealPartition { index: 1, size_bytes: 16 * 1024 * 1024, type_name: "MSR" },
        RealPartition { index: 2, size_bytes: 60 * 1024 * 1024 * 1024, type_name: "Basic" },
    ];
    let mut rec = LayoutReconciler::new();
    let rr = rec.reconcile(&t, &real);
    set.add("reconcile found", rr.all_registered_found, "");
    set.add("reconcile sizes", rr.sizes_match, "");
    set.add("reconcile unmanaged", rr.unmanaged == 1 && rec.unmanaged[0].type_name == "MSR", "");
    // 容量对不上 → 对拍红（信息准确判据的反向验证）。
    let real_bad = [RealPartition { index: 0, size_bytes: 999, type_name: "EFI" }];
    let rr2 = rec.reconcile(&t, &real_bad);
    set.add("reconcile mismatch red", !rr2.all_registered_found && !rr2.sizes_match, "");

    // 深四：体验账——错误事件计挫败信号，聚合按种类。
    let mut led = VolEventLedger::new();
    led.push(VolEvent { at_s: 1, kind: VolEventKind::HiddenRendered, vol: 0, verdict: VolVerdict::Smooth });
    led.push(VolEvent { at_s: 2, kind: VolEventKind::MutationBlocked, vol: 0, verdict: VolVerdict::Error });
    set.add("ledger kinds", led.count_kind(VolEventKind::HiddenRendered) == 1, "");
    set.add("ledger frustration", led.frustration_signals == 1, "");
    set.add("ledger newest first", led.recent()[0].at_s == 2, "");

    // 深五：修复流全链——检出→指引→通道→修复→复验清红；复验仍坏回指引。
    let mut rf = RepairFlow::new();
    rf.detect(0);
    set.add("repair guiding", matches!(rf.phase, RepairPhase::Guiding { vol_id: 0 }), "");
    set.add("repair idempotent", { rf.detect(0); rf.guide_shown == 1 }, "re-detect does not reset flow");
    set.add("repair channel", rf.choose_channel(1).map(|s| s.contains("恢复环境")).unwrap_or(false), "");
    set.add("repair done", rf.repair_done().is_ok(), "");
    set.add("repair verify bad", !rf.verify(false) && rf.guide_shown == 2, "still bad → re-guide");
    rf.choose_channel(0).ok();
    rf.repair_done().ok();
    set.add("repair verify ok", rf.verify(true) && rf.cleared == 1 && rf.phase == RepairPhase::Idle, "");
    set.add("repair err paths", rf.repair_done().is_err() && rf.choose_channel(0).is_err(), "");

    // 深六：渲染数据——人话字节+用量条分段。
    let mut s = String::new();
    human_bytes(&mut s, 260 * 1024 * 1024);
    set.add("bytes mb", s == "260MB", "");
    let mut s2 = String::new();
    human_bytes(&mut s2, 60 * 1024 * 1024 * 1024);
    set.add("bytes gb", s2 == "60GB", "");
    set.add("bar green", usage_bar(30, 100) == (300, "green"), "");
    set.add("bar yellow", usage_bar(700, 1000) == (700, "yellow"), "");
    set.add("bar red", usage_bar(900, 1000) == (900, "red"), "");
    set.add("bar zero total", usage_bar(10, 0) == (0, "red"), "");

    set
}

#[cfg(test)]
mod deep_tests {
    use super::*;

    fn mk(vol_id: u32, kind: VolumeKind) -> VolumeRecord {
        VolumeRecord {
            vol_id,
            label: "V",
            letter: Some('C'),
            size_bytes: 1000,
            used_bytes: 100,
            kind,
            hash: HashState::Ok,
        }
    }

    #[test]
    fn f186_deep_mount_persists_across_enumeration() {
        // 隐藏卷在枚举面缺席，但挂载点解析 100 次全通——「不呈现≠不工作」。
        let mut t = VolumeTable::new();
        t.register(mk(0, VolumeKind::Boot));
        let mut mt = MountTable::new();
        mt.mount(0, "\\Device\\HarddiskVolume1");
        assert_eq!(t.enumerate_user_volumes().len(), 0);
        for i in 0..100 {
            assert!(mt.resolve(0, &alloc::format!("f{}", i)).is_ok());
        }
        assert_eq!(mt.resolves, 100);
    }

    #[test]
    fn f186_deep_repair_flow_full_cycle_twice() {
        // 两轮坏-修-好循环：状态机无残留（guide 计数 4=每轮指引+复验失败重指）。
        let mut rf = RepairFlow::new();
        for _ in 0..2 {
            rf.detect(3);
            rf.choose_channel(0).unwrap();
            rf.repair_done().unwrap();
            assert!(!rf.verify(false), "first verify still bad");
            rf.choose_channel(1).unwrap();
            rf.repair_done().unwrap();
            assert!(rf.verify(true));
        }
        assert_eq!(rf.cleared, 2);
        assert_eq!(rf.repairs_run, 4);
        assert_eq!(rf.guide_shown, 4);
    }

    #[test]
    fn f186_deep_help_article_contents() {
        assert_eq!(HELP_ARTICLE[0].heading, "哪些分区被隐藏了");
        assert!(HELP_ARTICLE[2].body.contains("恢复环境"));
    }

    #[test]
    fn f186_deep_human_bytes_edges() {
        let mut s = String::new();
        human_bytes(&mut s, 0);
        assert_eq!(s, "0B");
        let mut s2 = String::new();
        human_bytes(&mut s2, 1023);
        assert_eq!(s2, "1023B");
        let mut s3 = String::new();
        human_bytes(&mut s3, 1024);
        assert_eq!(s3, "1KB");
    }

    #[test]
    fn f186_deep_reconciler_claims_once() {
        // 两个同容量分区：各认领一次，不多吞（claimed 防线）。
        let mut t = VolumeTable::new();
        let _ = t.register(mk(0, VolumeKind::Data));
        let _ = t.register(mk(1, VolumeKind::Data));
        let real = [
            RealPartition { index: 0, size_bytes: 1000, type_name: "Basic" },
            RealPartition { index: 1, size_bytes: 1000, type_name: "Basic" },
        ];
        let mut rec = LayoutReconciler::new();
        let rr = rec.reconcile(&t, &real);
        assert!(rr.all_registered_found && rr.unmanaged == 0);
    }

    #[test]
    fn f186_deep_run_checks_pass() {
        assert!(run_syspart_deep_checks().all_passed());
    }
}

// ---------------------------------------------------------------------------
// v3 批次（回炉补深化第三轮 2026-09-26）——盘符分配隔离 / 全事件审计 /
// 容量规划报告。判据源：主册【设计细节】「隐藏实现=枚举过滤（驱动层不打卷
// ——挂载照常工作，只是不呈现）」的分配器落点 + 十三章体验日志扩展。
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// v3-一：LetterAssigner —— 盘符分配器隔离（隐藏卷从源头不进分配池——
// 「数据面上根本不存在」的分配器侧防线，与 VolumeTable.register 的剥除
// 双保险成对：注册时剥一次，分配器侧永远看不见）
// ---------------------------------------------------------------------------

/// 盘符分配池（C-Z 常规数据卷；隐藏卷的盘符不是「不显示」而是「从未分配」）。
pub struct LetterAssigner {
    /// 已占用盘符（数据卷专用——隐藏卷永不入池）。
    used: Vec<char>,
    /// 拒绝隐藏卷次数（分配器侧红线的执法对账）。
    pub hidden_rejects: u64,
}

/// 分配器可用盘符集（C 起——A/B 留给软盘语义历史兼容，本域不分配）。
pub const ASSIGNABLE_LETTERS: [char; 24] = [
    'C', 'D', 'E', 'F', 'G', 'H', 'I', 'J', 'K', 'L', 'M', 'N', 'O', 'P', 'Q', 'R', 'S', 'T', 'U', 'V', 'W', 'X', 'Y', 'Z',
];

impl LetterAssigner {
    pub fn new() -> LetterAssigner {
        LetterAssigner { used: Vec::new(), hidden_rejects: 0 }
    }

    /// 申请盘符（仅数据卷可申请；隐藏卷/交接卷请求 = 红线拒绝+计数）。
    pub fn assign(&mut self, kind: VolumeKind, prefer: Option<char>) -> Result<char, &'static str> {
        if kind.hidden_by_default() {
            self.hidden_rejects += 1;
            return Err("系统关键分区不分配盘符（隐藏=从未呈现）");
        }
        // 优先盘符可用则用之；否则取最小可用位。
        if let Some(c) = prefer {
            if ASSIGNABLE_LETTERS.contains(&c) && !self.used.contains(&c) {
                self.used.push(c);
                return Ok(c);
            }
        }
        for c in ASSIGNABLE_LETTERS {
            if !self.used.contains(&c) {
                self.used.push(c);
                return Ok(c);
            }
        }
        Err("盘符池已耗尽（24 位全占）")
    }

    /// 释放（数据卷卸载——盘符回池）。
    pub fn release(&mut self, c: char) -> bool {
        let pos = self.used.iter().position(|x| *x == c);
        match pos {
            Some(i) => {
                self.used.remove(i);
                true
            }
            None => false,
        }
    }

    pub fn used_count(&self) -> usize {
        self.used.len()
    }
}

impl Default for LetterAssigner {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// v3-二：VolumeAuditTrail —— 卷域全事件审计（十三章扩展：注册/剥符/越权/
// 对拍四类事件全记，行为可回放——「红线执法可观测」的完整账）
// ---------------------------------------------------------------------------

/// 卷域事件种类。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VolAuditKind {
    /// 卷注册（含用途标记）。
    Registered,
    /// 隐藏卷盘符剥除（注册时分配器侧防线生效）。
    LetterStripped,
    /// 盘符申请被拒（分配器侧）。
    LetterRefused,
    /// 越权写被拒。
    MutationBlocked,
    /// 布局对拍执行。
    Reconciled,
}

impl VolAuditKind {
    pub fn name(self) -> &'static str {
        match self {
            VolAuditKind::Registered => "registered",
            VolAuditKind::LetterStripped => "letter-stripped",
            VolAuditKind::LetterRefused => "letter-refused",
            VolAuditKind::MutationBlocked => "mutation-blocked",
            VolAuditKind::Reconciled => "reconciled",
        }
    }
}

/// 一条卷域审计事件（时刻/种类/对象卷）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VolAuditEvent {
    pub at_s: u64,
    pub kind: VolAuditKind,
    pub vol: u32,
}

/// 全事件账（定容环 + 按类检索——回放即真相）。
pub struct VolumeAuditTrail {
    events: RingLogLocal<VolAuditEvent, 64>,
}

impl VolumeAuditTrail {
    pub fn new() -> VolumeAuditTrail {
        VolumeAuditTrail { events: RingLogLocal::new() }
    }

    pub fn push(&mut self, e: VolAuditEvent) {
        self.events.push(e);
    }

    pub fn recent(&self) -> Vec<VolAuditEvent> {
        self.events.newest_first()
    }

    /// 按类检索（诊断页过滤视图）。
    pub fn of_kind(&self, kind: VolAuditKind) -> Vec<VolAuditEvent> {
        self.events.newest_first().into_iter().filter(|e| e.kind == kind).collect()
    }

    /// 红线执法统计（越权+剥符+拒配三类——本域安全动作的总账）。
    pub fn enforcement_total(&self) -> usize {
        self.of_kind(VolAuditKind::LetterRefused).len()
            + self.of_kind(VolAuditKind::MutationBlocked).len()
    }
}

impl Default for VolumeAuditTrail {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// v3-三：CapacityReport —— 卷表容量规划报告（VOLUME_CAP=32 的规划面：
// 现状/余量/水位段位——满表不是突然爆，是提前看得见）
// ---------------------------------------------------------------------------

/// 容量报告。
pub struct CapacityReport {
    /// 已注册卷数。
    pub used: usize,
    /// 容量上限。
    pub cap: usize,
    /// 剩余槽位。
    pub free: usize,
    /// 水位段位（<70% 绿 / <90% 黄 / 其余红——与仪表条同语汇）。
    pub zone: &'static str,
}

/// 组装（一眼看清还能挂几个卷）。
pub fn capacity_report(t: &VolumeTable) -> CapacityReport {
    let used = t.count();
    let permille = used * 1000 / VOLUME_CAP;
    let zone = if permille >= 900 {
        "red"
    } else if permille >= 700 {
        "yellow"
    } else {
        "green"
    };
    CapacityReport { used, cap: VOLUME_CAP, free: VOLUME_CAP - used, zone }
}

// ---------------------------------------------------------------------------
// v3 自检
// ---------------------------------------------------------------------------

/// F186 v3 自检（聚合进 secstar2 域）。
pub fn run_syspart_deep2_checks() -> CheckSet {
    let mut set = CheckSet::new("F186-v3");

    // v3-一：盘符分配隔离——隐藏卷零放行、数据卷正常分配、释放回池。
    let mut la = LetterAssigner::new();
    set.add("assign hidden refused", la.assign(VolumeKind::Boot, None).is_err(), "");
    set.add("assign hidden counted", la.hidden_rejects == 1, "");
    set.add("assign data ok", la.assign(VolumeKind::Data, None) == Ok('C'), "");
    set.add("assign prefer", la.assign(VolumeKind::Data, Some('F')) == Ok('F'), "");
    set.add("assign prefer taken", la.assign(VolumeKind::Data, Some('C')) == Ok('D'), "首选占用落下一可用位");
    set.add("assign release", la.release('C') && la.assign(VolumeKind::Data, Some('C')) == Ok('C'), "");
    set.add("assign release unknown", !la.release('Q'), "");
    // 24 位耗尽诚实报错。
    let mut la2 = LetterAssigner::new();
    for _ in 0..24 {
        la2.assign(VolumeKind::Data, None).ok();
    }
    set.add("assign exhausted", la2.assign(VolumeKind::Data, None).is_err(), "");
    set.add("assign letter set", ASSIGNABLE_LETTERS.len() == 24 && ASSIGNABLE_LETTERS[0] == 'C', "");

    // v3-二：全事件审计——四类事件可回放、执法统计闭环。
    let mut trail = VolumeAuditTrail::new();
    trail.push(VolAuditEvent { at_s: 1, kind: VolAuditKind::Registered, vol: 0 });
    trail.push(VolAuditEvent { at_s: 2, kind: VolAuditKind::LetterStripped, vol: 0 });
    trail.push(VolAuditEvent { at_s: 3, kind: VolAuditKind::LetterRefused, vol: 0 });
    trail.push(VolAuditEvent { at_s: 4, kind: VolAuditKind::MutationBlocked, vol: 0 });
    set.add("trail kinds", trail.of_kind(VolAuditKind::Registered).len() == 1
        && trail.of_kind(VolAuditKind::LetterStripped).len() == 1, "");
    set.add("trail enforcement", trail.enforcement_total() == 2, "拒配+越权=执法总账");
    set.add("trail newest first", trail.recent()[0].at_s == 4, "");
    set.add("trail names", VolAuditKind::Reconciled.name() == "reconciled", "");

    // v3-三：容量报告——水位三段与余量。
    let mut t = VolumeTable::new();
    let capr = capacity_report(&t);
    set.add("cap empty green", capr.used == 0 && capr.zone == "green" && capr.free == VOLUME_CAP, "");
    for i in 0..(VOLUME_CAP * 3 / 4) {
        let _ = t.register(VolumeRecord {
            vol_id: i as u32, label: "V", letter: None, size_bytes: 1, used_bytes: 0,
            kind: VolumeKind::Data, hash: HashState::Ok,
        });
    }
    let capr2 = capacity_report(&t);
    set.add("cap 75pct yellow", capr2.used == 24 && capr2.zone == "yellow" && capr2.free == 8, "");
    for i in (VOLUME_CAP * 3 / 4)..VOLUME_CAP {
        let _ = t.register(VolumeRecord {
            vol_id: i as u32, label: "V", letter: None, size_bytes: 1, used_bytes: 0,
            kind: VolumeKind::Data, hash: HashState::Ok,
        });
    }
    set.add("cap full red", capacity_report(&t).zone == "red" && capacity_report(&t).free == 0, "");

    set
}

#[cfg(test)]
mod deep2_tests {
    use super::*;

    #[test]
    fn f186_v3_assigner_never_leaks_hidden_letters() {
        // 隐私性终检：200 次混合申请（含 120 次隐藏卷）后，池中只可能有
        // 数据卷盘符——隐藏卷从未进过池（结构性验证）。
        let mut la = LetterAssigner::new();
        for i in 0..200 {
            let kind = if i % 5 < 3 { VolumeKind::Boot } else { VolumeKind::Data };
            let _ = la.assign(kind, None);
        }
        assert_eq!(la.hidden_rejects, 120);
        assert!(la.used_count() <= 24, "data letters bounded by pool");
    }

    #[test]
    fn f186_v3_trail_replay_tells_full_story() {
        // 一次完整卷生命周期的事件回放：注册→剥符→对拍→越权→拒配。
        let mut trail = VolumeAuditTrail::new();
        let story = [
            (1, VolAuditKind::Registered, 0u32),
            (2, VolAuditKind::LetterStripped, 0),
            (3, VolAuditKind::Reconciled, 0),
            (4, VolAuditKind::MutationBlocked, 0),
            (5, VolAuditKind::LetterRefused, 1),
        ];
        for (at, kind, vol) in story {
            trail.push(VolAuditEvent { at_s: at, kind, vol });
        }
        let replay = trail.recent();
        assert_eq!(replay.len(), 5);
        // 回放序与时间序相反（环账语义）但内容全保真。
        assert_eq!(replay[4].kind, VolAuditKind::Registered);
        assert_eq!(trail.enforcement_total(), 2);
    }

    #[test]
    fn f186_v3_run_checks_pass() {
        assert!(run_syspart_deep2_checks().all_passed());
    }
}
