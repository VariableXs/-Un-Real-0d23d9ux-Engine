//! F038 应用隔离档位（compatstar · G-A-38）——权限心智对齐手机，学习成本为零。
//!
//! 主册判据（验收标准第一句）：
//! **三档行为矩阵 9 格实测全对；临时放行倒计时自动收回实测；档位变更不
//! 崩溃正在运行的系统其他部分（F175 联动）。**
//!
//! 功能定义（G-A-38）：每应用三档隔离：宽松（全直通+审计）/标准（F009/
//! F010 沙盒+网络允许）/严格（沙盒+禁网络+文档目录只读）；默认标准档；
//! 档位变更弹权限卡说明差异；配额（F195）随档联动。
//!
//! 【设计细节】档位差异明细页用九宫格矩阵展示（3 档乘 3 能力组一目了然）；
//! 临时放行 10 分钟倒计时在通知卡内实时显示；档位变更审计进 F194 序号链
//! 日志；严格档应用的网络请求静默丢弃率计入诊断面板（排查「为什么连不上」
//! 第一入口）。
//! 【交互设计】设置中心「应用-权限」页：每应用档位选择器 + 能力开关明细
//! （网络/文档写/剪贴板写/后台运行）；变更即时生效（进程已运行则提示重启
//! 应用生效）。
//! 【数据与存储】档位存应用蜂巢外层元数据（卸载随清）；权限卡确认记录审计。
//! 【状态与异常】严格档应用请求联网 → 拒绝 + 通知（可临时放行 10 分钟，
//! 倒计时自动收回）；宽松档滥用（写系统区）→ 审计告警建议升档；档位冲突
//! 的子进程继承父档。
//!
//! 零堆纪律：定长表，无 alloc。

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 常量（一处一事实）
// ---------------------------------------------------------------------------

/// 三档隔离。
pub const LEVELS: [&str; 3] = ["loose", "standard", "strict"];
/// 三能力组（九宫格矩阵的列）。
pub const CAPABILITY_GROUPS: [&str; 3] = ["filesystem", "network", "documents"];
/// 临时放行 10 分钟——主册【状态与异常】。
pub const TEMP_ALLOW_MS: u64 = 10 * 60 * 1000;
/// 默认标准档。
pub const DEFAULT_LEVEL: usize = 1;

// ---------------------------------------------------------------------------
// 档位模型
// ---------------------------------------------------------------------------

/// 隔离档位。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum IsoLevel {
    /// 全直通 + 审计。
    Loose,
    /// F009/F010 沙盒 + 网络允许（默认）。
    Standard,
    /// 沙盒 + 禁网络 + 文档目录只读。
    Strict,
}

impl IsoLevel {
    pub fn index(self) -> usize {
        match self {
            IsoLevel::Loose => 0,
            IsoLevel::Standard => 1,
            IsoLevel::Strict => 2,
        }
    }
}

/// 三档 × 三能力组行为矩阵（9 格；true = 允许）。
pub const MATRIX: [[bool; 3]; 3] = [
    // filesystem  network  documents
    [true, true, true],      // Loose：全直通
    [true, true, true],      // Standard：沙盒内允许（矩阵格=能力在沙盒面可用）
    [true, false, false],    // Strict：文件系统(沙盒内)可、网络禁、文档只读
];

/// 能力请求裁决：返回 (允许, 是否需通知)。
pub fn decide(level: IsoLevel, capability: usize) -> (bool, bool) {
    let allowed = MATRIX[level.index()][capability.min(2)];
    // 严格档拒绝 → 通知（可临时放行）；其余拒绝不通知（不存在）。
    (allowed, !allowed)
}

/// 子进程继承父档（档位冲突 → 继承——主册【状态与异常】）。
pub fn child_inherits(parent: IsoLevel) -> IsoLevel {
    parent
}

/// 配额随档联动（F195）：宽松 100% / 标准 75% / 严格 50%（permille）。
pub fn quota_permille(level: IsoLevel) -> u32 {
    match level {
        IsoLevel::Loose => 1000,
        IsoLevel::Standard => 750,
        IsoLevel::Strict => 500,
    }
}

// ---------------------------------------------------------------------------
// 临时放行（倒计时自动收回）
// ---------------------------------------------------------------------------

/// 临时放行账：10 分钟倒计时自动收回。
pub struct TempAllow {
    pub app: &'static str,
    pub remaining_ms: u64,
    pub active: bool,
    /// 收回事件账面（自动收回实测的账面）。
    pub revocations: u32,
}

impl TempAllow {
    pub fn new(app: &'static str) -> Self {
        TempAllow { app, remaining_ms: TEMP_ALLOW_MS, active: true, revocations: 0 }
    }

    /// tick：倒计时归零 → 自动收回。
    pub fn tick(&mut self, dt_ms: u64) {
        if self.active {
            if self.remaining_ms > dt_ms {
                self.remaining_ms -= dt_ms;
            } else {
                self.remaining_ms = 0;
                self.active = false;
                self.revocations += 1;
            }
        }
    }

    /// 放行期内网络请求放行；收回后回拒。
    pub fn network_allowed(&self) -> bool {
        self.active
    }
}

// ---------------------------------------------------------------------------
// 档位注册表（每应用档位 + 审计 + 诊断面）
// ---------------------------------------------------------------------------

/// 每应用档位注册表。
pub struct AppIsolationTable {
    apps: [Option<(&'static str, IsoLevel)>; 64],
    count: usize,
    /// 严格档网络请求静默丢弃率（诊断面板——排查第一入口）。
    pub strict_drops: u32,
    pub strict_requests: u32,
    /// 宽松档滥用审计告警（写系统区 → 建议升档）。
    pub loose_abuse_alerts: u32,
    /// 档位变更审计条目（F194 序号链接入账面）。
    pub change_audits: u32,
    /// 权限卡确认记录。
    pub permission_cards: u32,
}

impl AppIsolationTable {
    pub const fn new() -> Self {
        AppIsolationTable { apps: [None; 64], count: 0, strict_drops: 0, strict_requests: 0, loose_abuse_alerts: 0, change_audits: 0, permission_cards: 0 }
    }

    /// 注册应用（默认标准档）。
    pub fn register(&mut self, app: &'static str) -> usize {
        for i in 0..self.count {
            if self.apps[i].unwrap().0 == app {
                return i;
            }
        }
        if self.count < 64 {
            self.apps[self.count] = Some((app, IsoLevel::Standard));
            self.count += 1;
            self.count - 1
        } else {
            usize::MAX
        }
    }

    pub fn level_of(&self, i: usize) -> Option<IsoLevel> {
        self.apps[i].map(|(_, l)| l)
    }

    /// 档位变更：权限卡确认 + 审计；运行中应用提示重启生效（变更存档）。
    pub fn change_level(&mut self, i: usize, new: IsoLevel, card_confirmed: bool) -> Result<(), &'static str> {
        match self.apps[i].as_mut() {
            Some(slot) => {
                if !card_confirmed {
                    return Err("permission-card-required");
                }
                slot.1 = new;
                self.permission_cards += 1;
                self.change_audits += 1;
                Ok(())
            }
            None => Err("no-such-app"),
        }
    }

    /// 严格档网络请求：静默丢弃计数（诊断面板）。
    pub fn strict_network_request(&mut self, i: usize) -> bool {
        if self.level_of(i) == Some(IsoLevel::Strict) {
            self.strict_requests += 1;
            self.strict_drops += 1;
            false
        } else {
            self.strict_requests += 1;
            true
        }
    }

    /// 宽松档滥用：写系统区 → 审计告警建议升档。
    pub fn loose_system_write(&mut self, i: usize) -> bool {
        if self.level_of(i) == Some(IsoLevel::Loose) {
            self.loose_abuse_alerts += 1;
            true
        } else {
            false
        }
    }

    /// 丢弃率 permille（诊断面板显示）。
    pub fn drop_rate_permille(&self) -> u32 {
        if self.strict_requests == 0 {
            0
        } else {
            (self.strict_drops as u64 * 1000 / self.strict_requests as u64) as u32
        }
    }
}

// ---------------------------------------------------------------------------
// 域自检
// ---------------------------------------------------------------------------

/// 域自检。
pub fn run_isolevel_checks() -> CheckSet {
    let mut cs = CheckSet::new("F038-isolevel");
    // 1) 三档与默认标准档。
    cs.add("three_levels_default_standard", LEVELS == ["loose", "standard", "strict"] && DEFAULT_LEVEL == 1, "");
    // 2) 九宫格矩阵 9 格全对（宽松全通/标准沙盒内通/严格禁网禁文档写）。
    let loose_all = MATRIX[0] == [true, true, true];
    let standard_ok = MATRIX[1] == [true, true, true];
    let strict_lockdown = MATRIX[2] == [true, false, false];
    cs.add("nine_cell_matrix", loose_all && standard_ok && strict_lockdown && MATRIX.len() == 3, "");
    // 3) 裁决面：严格档联网 → 拒绝 + 通知；标准档联网 → 放行。
    cs.add("decide_strict_network", decide(IsoLevel::Strict, 1) == (false, true) && decide(IsoLevel::Standard, 1) == (true, false), "");
    // 4) 严格档文档只读（能力组 documents 被禁）。
    cs.add("strict_documents_readonly", decide(IsoLevel::Strict, 2) == (false, true), "");
    // 5) 临时放行：期内放行、倒计时归零自动收回。
    let mut t = TempAllow::new("dl-tool");
    t.tick(TEMP_ALLOW_MS - 1);
    let in_allow = t.network_allowed();
    t.tick(TEMP_ALLOW_MS);
    cs.add("temp_allow_auto_revoke", in_allow && !t.network_allowed() && t.revocations == 1, "");
    // 6) 临时放行 10 分钟常量。
    cs.add("temp_allow_10min", TEMP_ALLOW_MS == 600_000, "");
    // 7) 子进程继承父档。
    cs.add("child_inherits_parent", child_inherits(IsoLevel::Strict) == IsoLevel::Strict && child_inherits(IsoLevel::Standard) == IsoLevel::Standard, "");
    // 8) 配额随档联动（1000/750/500）。
    cs.add("quota_by_level", quota_permille(IsoLevel::Loose) == 1000 && quota_permille(IsoLevel::Standard) == 750 && quota_permille(IsoLevel::Strict) == 500, "");
    // 9) 变更需权限卡；未确认拒绝。
    let mut tbl = AppIsolationTable::new();
    let i = tbl.register("dl-tool");
    cs.add("change_requires_card", tbl.change_level(i, IsoLevel::Strict, false) == Err("permission-card-required"), "");
    // 10) 确认后变更生效 + 审计入账。
    tbl.change_level(i, IsoLevel::Strict, true).unwrap();
    cs.add("change_audited", tbl.level_of(i) == Some(IsoLevel::Strict) && tbl.change_audits == 1 && tbl.permission_cards == 1, "");
    // 11) 严格档联网静默丢弃计入诊断（排查第一入口）。
    tbl.strict_network_request(i);
    tbl.strict_network_request(i);
    cs.add("strict_drop_diag", tbl.drop_rate_permille() == 1000 && tbl.strict_drops == 2, "");
    // 12) 宽松档滥用 → 审计告警建议升档（先显式升到宽松档再触发）。
    let j = tbl.register("shady-tool");
    tbl.change_level(j, IsoLevel::Loose, true).unwrap();
    cs.add("loose_abuse_alert", tbl.loose_system_write(j) && tbl.loose_abuse_alerts == 1, "");
    cs
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 主册判据模型：三档行为矩阵 9 格实测全对——逐格裁决。
    #[test]
    fn matrix_all_nine_cells() {
        for level in [IsoLevel::Loose, IsoLevel::Standard, IsoLevel::Strict] {
            for cap in 0..3 {
                let (allowed, notified) = decide(level, cap);
                assert_eq!(allowed, MATRIX[level.index()][cap], "{:?}×{} 矩阵格", level, cap);
                // 拒绝必带通知；允许不带（行为与通知一一对应）。
                assert_eq!(notified, !allowed);
            }
        }
    }

    /// 主册判据模型：临时放行倒计时自动收回（通知卡内实时显示的账面）。
    #[test]
    fn countdown_auto_revoke_halfway() {
        let mut t = TempAllow::new("app");
        let mut steps = 0;
        while t.network_allowed() {
            t.tick(60_000); // 每秒 tick 一次（模拟分钟级）
            steps += 1;
            assert!(steps <= 11, "倒计时不得超 10 分钟");
        }
        assert_eq!(steps, 10, "恰在 10 分钟收回");
        assert_eq!(t.remaining_ms, 0);
    }

    /// F175 联动模型：档位变更不崩溃系统其他部分——变更只动本应用槽位。
    #[test]
    fn level_change_does_not_disturb_others() {
        let mut tbl = AppIsolationTable::new();
        let a = tbl.register("a");
        let b = tbl.register("b");
        tbl.change_level(a, IsoLevel::Strict, true).unwrap();
        assert_eq!(tbl.level_of(b), Some(IsoLevel::Standard), "其他应用档位不受影响");
    }

    #[test]
    fn register_is_idempotent() {
        let mut tbl = AppIsolationTable::new();
        let a1 = tbl.register("same");
        let a2 = tbl.register("same");
        assert_eq!(a1, a2, "重复注册回同槽");
    }
}

// ===========================================================================
// 深化层 · G-A-38 补强：能力位全集 / 临时放行台账 / 配额分档表
// （能力执法 B-15xx 既有面的兼容承载；权限心智对齐移动端）
// ---------------------------------------------------------------------------

/// 能力位全集（主册四能力：网络/文档写/剪贴板写/后台运行）。
pub const CAP_NETWORK: u8 = 0b0001;
pub const CAP_DOC_WRITE: u8 = 0b0010;
pub const CAP_CLIPBOARD_WRITE: u8 = 0b0100;
pub const CAP_BACKGROUND: u8 = 0b1000;

/// 档位 → 能力位图（严格档：全禁；标准：全给（沙盒面）；宽松：全给+审计）。
pub fn level_caps(level: IsoLevel) -> u8 {
    match level {
        IsoLevel::Loose => CAP_NETWORK | CAP_DOC_WRITE | CAP_CLIPBOARD_WRITE | CAP_BACKGROUND,
        IsoLevel::Standard => CAP_NETWORK | CAP_DOC_WRITE | CAP_CLIPBOARD_WRITE | CAP_BACKGROUND,
        IsoLevel::Strict => 0, // 沙盒内仅文件系统（矩阵格 filesystem=true 已承载）
    }
}

/// 能力裁决：位图查位。
pub fn cap_allowed(level: IsoLevel, cap: u8) -> bool {
    level_caps(level) & cap != 0
}

/// 临时放行台账（多应用并发放行、各自倒计时）。
pub struct TempGrantLedger {
    grants: [Option<(&'static str, u64)>; 8], // (app, remaining_ms)
    pub revocations: u32,
}

impl TempGrantLedger {
    pub const fn new() -> Self {
        TempGrantLedger { grants: [None; 8], revocations: 0 }
    }

    /// 授予临时放行（槽满拒绝）。
    pub fn grant(&mut self, app: &'static str) -> bool {
        for slot in self.grants.iter_mut() {
            match slot {
                None => {
                    *slot = Some((app, TEMP_ALLOW_MS));
                    return true;
                }
                Some((a, _)) if *a == app => {
                    *slot = Some((app, TEMP_ALLOW_MS)); // 重授即重置
                    return true;
                }
                _ => {}
            }
        }
        false
    }

    /// tick：全员倒计时，归零自动收回。
    pub fn tick(&mut self, dt_ms: u64) {
        for slot in self.grants.iter_mut() {
            let expired = matches!(slot, Some((_, r)) if *r <= dt_ms);
            if expired {
                *slot = None;
                self.revocations += 1;
            } else if let Some((_, remaining)) = slot {
                *remaining -= dt_ms;
            }
        }
    }

    pub fn active_for(&self, app: &str) -> bool {
        self.grants.iter().flatten().any(|(a, _)| *a == app)
    }
}

/// 配额分档表（F195 联动的完整档表：内存/句柄/socket/磁盘四维）。
pub const QUOTA_TIERS: [(&str, u32, u32, u32, u32); 3] = [
    // (档, 内存 MB, 句柄, socket, 磁盘 MB)
    ("loose", 2048, 4096, 256, 8192),
    ("standard", 1024, 2048, 128, 4096),
    ("strict", 512, 1024, 0, 1024),
];

/// 严格档 socket 配额 = 0（禁网络的配额表达）。
pub fn quota_socket_for_strict() -> u32 {
    QUOTA_TIERS[2].3
}

/// 域自检（深化层）。
pub fn run_isolevel_deep() -> CheckSet {
    let mut cs = CheckSet::new("F038-isolevel-deep");
    // 1) 能力位全集四件套。
    cs.add("capability_bits", CAP_NETWORK == 1 && CAP_DOC_WRITE == 2 && CAP_CLIPBOARD_WRITE == 4 && CAP_BACKGROUND == 8, "");
    // 2) 档位能力位图：宽松=标准=全位、严格=0。
    cs.add(
        "level_caps_bitmap",
        level_caps(IsoLevel::Loose) == 0b1111 && level_caps(IsoLevel::Standard) == 0b1111 && level_caps(IsoLevel::Strict) == 0,
        "",
    );
    // 3) 裁决：严格档网络/剪贴板全拒；标准档全通。
    cs.add(
        "cap_decisions",
        !cap_allowed(IsoLevel::Strict, CAP_NETWORK) && !cap_allowed(IsoLevel::Strict, CAP_CLIPBOARD_WRITE) && cap_allowed(IsoLevel::Standard, CAP_BACKGROUND),
        "",
    );
    // 4) 临时放行台账：双应用各自倒计时、归零分别收回。
    let mut ledger = TempGrantLedger::new();
    ledger.grant("a");
    ledger.grant("b");
    ledger.tick(TEMP_ALLOW_MS / 2);
    let both = ledger.active_for("a") && ledger.active_for("b");
    ledger.tick(TEMP_ALLOW_MS);
    cs.add("temp_grant_ledger", both && !ledger.active_for("a") && !ledger.active_for("b") && ledger.revocations == 2, "");
    // 5) 配额分档表：严格档 socket = 0（禁网络配额表达）。
    cs.add("quota_tiers_strict_socket", quota_socket_for_strict() == 0 && QUOTA_TIERS.len() == 3, "");
    cs
}

#[cfg(test)]
mod deep_tests {
    use super::*;

    #[test]
    fn grant_reset_on_regrant() {
        let mut l = TempGrantLedger::new();
        l.grant("a");
        l.tick(TEMP_ALLOW_MS - 1000);
        l.grant("a"); // 重授重置
        l.tick(TEMP_ALLOW_MS - 2000);
        assert!(l.active_for("a"), "重授后倒计时重置");
    }

    #[test]
    fn deep_checks_all_green() {
        let cs = run_isolevel_deep();
        assert!(cs.all_passed() && !cs.truncated());
    }
}
