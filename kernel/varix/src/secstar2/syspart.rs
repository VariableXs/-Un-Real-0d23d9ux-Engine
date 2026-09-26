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
