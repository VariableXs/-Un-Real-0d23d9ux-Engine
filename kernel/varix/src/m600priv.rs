//! m600priv — VARIX-M600 AI-19 隐私与信任域 (F451~F475)
//!
//! 权限仪表盘/数据流地图/摄像头门卫/麦克风指示灯/位置最小化/剪贴板卫兵/
//! 截图防护/隐私清理向导/匿名化遥测/零知识遥测开关/应用行为公证/
//! 敏感操作二次确认/防偷窥套装/访客模式/儿童安全区/密码学中台/
//! 密钥保管库/生物识别桥/安全启动链全检/漏洞响应流程/隐私降级模式/
//! 数据导出权/数据删除权/隐私回归走廊/信任年报。
//!
//! 硬约束：no_std / 无 alloc / 无浮点（全部 permille/定点）/ 纯逻辑。

use crate::checks::CheckSet;

// ===========================================================================
// F451 — 权限仪表盘：全局授权一览
// ===========================================================================

pub const CAP_CAMERA: u8 = 0;
pub const CAP_MICROPHONE: u8 = 1;
pub const CAP_LOCATION: u8 = 2;
pub const CAP_CONTACTS: u8 = 3;
pub const CAP_CLIPBOARD: u8 = 4;

#[derive(Clone, Copy)]
pub struct PermissionGrant {
    pub app_id: u32,
    pub cap: u8,
    pub granted: bool,
    pub by_user: bool, // 授权必须出自用户之手，不允许静默授权
}

impl PermissionGrant {
    pub fn honest(&self) -> bool {
        self.cap <= CAP_CLIPBOARD && (!self.granted || self.by_user)
    }
}

pub fn perm_dashboard_ok(grants: &[PermissionGrant]) -> bool {
    grants.iter().all(|g| g.honest())
}

pub fn granted_count(grants: &[PermissionGrant], cap: u8) -> usize {
    grants.iter().filter(|g| g.granted && g.cap == cap).count()
}

// ===========================================================================
// F452 — 数据流地图：数据去向可视化与泄漏拦截
// ===========================================================================

pub const FLOW_LOCAL: u8 = 0;
pub const FLOW_CROSS_APP: u8 = 1;
pub const FLOW_NETWORK: u8 = 2;

#[derive(Clone, Copy)]
pub struct DataFlowEdge {
    pub from_app: u32,
    pub to_app: u32,
    pub kind: u8,
}

/// 沙盒应用清单内的应用不得产生网络外流边。
pub fn dataflow_sane(edges: &[DataFlowEdge], sandboxed: &[u32]) -> bool {
    edges.iter().all(|e| {
        e.kind <= FLOW_NETWORK
            && !(e.kind == FLOW_NETWORK && sandboxed.contains(&e.from_app))
    })
}

// ===========================================================================
// F453 — 摄像头门卫：授权 + 指示灯 + 物理快门三重门
// ===========================================================================

#[derive(Clone, Copy)]
pub struct CameraGuard {
    pub app_granted: bool,
    pub indicator_led: bool,
    pub shutter_closed: bool,
}

pub fn camera_frames_allowed(g: &CameraGuard) -> bool {
    g.app_granted && g.indicator_led && !g.shutter_closed
}

// ===========================================================================
// F454 — 麦克风指示灯：状态严格同步
// ===========================================================================

pub const MIC_INDICATOR_MAX_LATENCY_MS: u32 = 50;

pub fn mic_indicator_ok(mic_in_use: bool, led_on: bool, latency_ms: u32) -> bool {
    if mic_in_use != led_on {
        return false;
    }
    !mic_in_use || latency_ms <= MIC_INDICATOR_MAX_LATENCY_MS
}

// ===========================================================================
// F455 — 位置最小化：粗化网格与精度损失
// ===========================================================================

/// 坐标毫度（mdeg）对齐到网格，四舍五入。
pub fn coarsen_coord(coord_mdeg: i32, grid_mdeg: u32) -> i32 {
    let g = grid_mdeg as i32;
    if g <= 1 {
        return coord_mdeg;
    }
    let rem = coord_mdeg % g;
    let base = coord_mdeg - rem;
    if rem * 2 >= g {
        base + g
    } else {
        base
    }
}

/// 精度损失 permille = (grid-1)/grid。
pub fn precision_loss_permille(grid_mdeg: u32) -> u32 {
    if grid_mdeg <= 1 {
        0
    } else {
        (grid_mdeg - 1) * 1000 / grid_mdeg
    }
}

pub const LOCATION_MIN_LOSS_PERMILLE: u32 = 800;

pub fn location_minimized(grid_mdeg: u32) -> bool {
    precision_loss_permille(grid_mdeg) >= LOCATION_MIN_LOSS_PERMILLE
}

// ===========================================================================
// F456 — 剪贴板卫兵：敏感内容短时驻留
// ===========================================================================

pub const TAG_TEXT: u8 = 0;
pub const TAG_PASSWORD: u8 = 1;
pub const TAG_URL: u8 = 2;

pub const CLIPBOARD_CLEAR_MS: u32 = 60_000;
pub const CLIPBOARD_SECRET_MS: u32 = 15_000;

pub fn clipboard_hold_ok(tag: u8, age_ms: u32) -> bool {
    if tag == TAG_PASSWORD {
        age_ms < CLIPBOARD_SECRET_MS
    } else {
        age_ms <= CLIPBOARD_CLEAR_MS
    }
}

// ===========================================================================
// F457 — 截图防护：安全窗口拒绝截屏
// ===========================================================================

pub const WIN_FLAG_SECURE: u32 = 1 << 0;

pub fn screenshot_allowed(win_flags: u32) -> bool {
    win_flags & WIN_FLAG_SECURE == 0
}

// ===========================================================================
// F458 — 隐私清理向导：分类清理进度
// ===========================================================================

pub struct CleanupCategory {
    pub name: &'static str,
    pub items: u32,
    pub cleaned: u32,
}

pub fn cleanup_progress_permille(cats: &[CleanupCategory]) -> u16 {
    let (cleaned, total) =
        cats.iter().fold((0u32, 0u32), |(c, t), x| (c + x.cleaned, t + x.items));
    if total == 0 {
        1000
    } else {
        ((cleaned * 1000 / total).min(1000)) as u16
    }
}

// ===========================================================================
// F459 — 匿名化遥测：去标识 + k-匿名桶
// ===========================================================================

pub const TELEMETRY_K_ANONYMITY: u32 = 5;

pub fn telemetry_anonymous(has_uid: bool, bucket_size: u32) -> bool {
    !has_uid && bucket_size >= TELEMETRY_K_ANONYMITY
}

// ===========================================================================
// F460 — 零知识遥测开关：默认关，明示才开
// ===========================================================================

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum TelemetryChoice {
    OptIn,
    OptOut,
    Undecided,
}

pub fn telemetry_may_send(choice: TelemetryChoice) -> bool {
    matches!(choice, TelemetryChoice::OptIn)
}

// ===========================================================================
// F461 — 应用行为公证：行为哈希链（可审计、不可篡改）
// ===========================================================================

pub const BEHAVIOR_MIX: u64 = 0x9E37_79B9_7F4A_7C15;

pub fn behavior_chain_next(prev: u64, event: u64) -> u64 {
    prev.rotate_left(13) ^ event.wrapping_mul(BEHAVIOR_MIX)
}

/// 逐段重放校验行为链；任何一处不等即判篡改。
pub fn behavior_chain_valid(seed: u64, events: &[u64], chain: &[u64]) -> bool {
    if events.len() != chain.len() {
        return false;
    }
    let mut acc = seed;
    for i in 0..events.len() {
        acc = behavior_chain_next(acc, events[i]);
        if acc != chain[i] {
            return false;
        }
    }
    true
}

// ===========================================================================
// F462 — 敏感操作二次确认：按操作分级要求凭据
// ===========================================================================

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum SensitiveOp {
    ChangePassword,
    DeleteVault,
    ExportKeys,
    FactoryReset,
}

pub const CONFIRM_PASSWORD: u8 = 1 << 0;
pub const CONFIRM_BIOMETRIC: u8 = 1 << 1;

/// 返回该操作要求的凭据位掩码。
pub fn op_confirm_policy(op: SensitiveOp) -> u8 {
    match op {
        SensitiveOp::ChangePassword => CONFIRM_PASSWORD,
        SensitiveOp::DeleteVault => CONFIRM_PASSWORD | CONFIRM_BIOMETRIC,
        SensitiveOp::ExportKeys => CONFIRM_PASSWORD | CONFIRM_BIOMETRIC,
        SensitiveOp::FactoryReset => CONFIRM_PASSWORD | CONFIRM_BIOMETRIC,
    }
}

pub fn op_confirm_satisfied(op: SensitiveOp, have: u8) -> bool {
    have & op_confirm_policy(op) == op_confirm_policy(op)
}

// ===========================================================================
// F463 — 防偷窥套装：身后有人即变暗
// ===========================================================================

pub fn anti_peek_action(person_detected: bool, screen_dimmed: bool) -> bool {
    person_detected == screen_dimmed
}

// ===========================================================================
// F464 — 访客模式：禁入主目录、禁持久写入
// ===========================================================================

pub const GUEST_DENY_HOME: u8 = 1 << 0;
pub const GUEST_DENY_PERSIST: u8 = 1 << 1;

pub fn guest_permitted(action_flags: u8) -> bool {
    action_flags & (GUEST_DENY_HOME | GUEST_DENY_PERSIST) == 0
}

// ===========================================================================
// F465 — 儿童安全区：白名单 + 时长预算
// ===========================================================================

pub struct KidZone {
    pub whitelist_ok: bool,
    pub minutes_used: u32,
    pub budget_min: u32,
}

pub fn kid_zone_ok(k: &KidZone) -> bool {
    k.whitelist_ok && k.budget_min > 0 && k.minutes_used <= k.budget_min
}

// ===========================================================================
// F466 — 密码学中台：常数时间比较 + nonce 唯一性
// ===========================================================================

/// 常数时间比较：无论差异在哪，扫描路径一致。
pub fn ct_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for i in 0..a.len() {
        diff |= a[i] ^ b[i];
    }
    diff == 0
}

/// AEAD nonce 永不复用：新 nonce 不得出现在历史记录中。
pub fn nonce_fresh(log: &[u64], n: u64) -> bool {
    !log.contains(&n)
}

// ===========================================================================
// F467 — 密钥保管库：私钥不出库，公钥可导出
// ===========================================================================

pub const KEY_PRIVATE: u8 = 0;
pub const KEY_PUBLIC: u8 = 1;

pub fn vault_export_allowed(kind: u8) -> bool {
    kind == KEY_PUBLIC
}

#[derive(Clone, Copy)]
pub struct VaultEntry {
    pub id: u64,
    pub kind: u8,
    pub locked: bool,
}

pub fn vault_usable(e: &VaultEntry) -> bool {
    !e.locked && (e.kind == KEY_PRIVATE || e.kind == KEY_PUBLIC)
}

// ===========================================================================
// F468 — 生物识别桥：阈值 + 活体检测
// ===========================================================================

pub const BIOMETRIC_THRESHOLD_PERMILLE: u16 = 850;

pub fn biometric_match(score_permille: u16, liveness: bool) -> bool {
    score_permille >= BIOMETRIC_THRESHOLD_PERMILLE && liveness
}

// ===========================================================================
// F469 — 安全启动链全检：ROM→引导→内核→模块逐环验证
// ===========================================================================

pub const BOOT_CHAIN_MIN_LINKS: usize = 4;

pub fn boot_chain_ok(links: &[bool]) -> bool {
    links.len() >= BOOT_CHAIN_MIN_LINKS && links.iter().all(|l| *l)
}

// ===========================================================================
// F470 — 漏洞响应流程：按严重度分级 SLA
// ===========================================================================

pub const SEV_CRITICAL: u8 = 0;
pub const SEV_HIGH: u8 = 1;
pub const SEV_MEDIUM: u8 = 2;
pub const SEV_LOW: u8 = 3;

pub fn vuln_sla_days(sev: u8) -> u16 {
    match sev {
        SEV_CRITICAL => 7,
        SEV_HIGH => 30,
        SEV_MEDIUM => 90,
        _ => 180,
    }
}

pub fn vuln_on_time(sev: u8, days_open: u16) -> bool {
    days_open <= vuln_sla_days(sev)
}

// ===========================================================================
// F471 — 隐私降级模式：出差/应急一键断联
// ===========================================================================

#[derive(Clone, Copy)]
pub struct PrivacyDegrade {
    pub radios_off: bool,
    pub telemetry_off: bool,
    pub location_off: bool,
}

pub fn privacy_degrade_complete(d: &PrivacyDegrade) -> bool {
    d.radios_off && d.telemetry_off && d.location_off
}

// ===========================================================================
// F472 — 数据导出权：开放格式 + 30 天 SLA + 身份确认
// ===========================================================================

pub const EXPORT_SLA_DAYS: u16 = 30;

pub fn export_request_ok(days: u16, open_format: bool, confirmed_owner: bool) -> bool {
    confirmed_owner && open_format && days <= EXPORT_SLA_DAYS
}

// ===========================================================================
// F473 — 数据删除权：正本与备份副本全量清除
// ===========================================================================

#[derive(Clone, Copy)]
pub struct DeleteRequest {
    pub items: usize,
    pub purged: usize,
    pub backup_copies: usize,
    pub backups_purged: usize,
}

pub fn deletion_complete(r: &DeleteRequest) -> bool {
    r.items > 0 && r.purged == r.items && r.backups_purged == r.backup_copies
}

// ===========================================================================
// F474 — 隐私回归走廊：用例集持续守护
// ===========================================================================

pub struct PrivacyCase {
    pub name: &'static str,
    pub passed: bool,
}

pub fn privacy_regression_open(cases: &[PrivacyCase]) -> usize {
    cases.iter().filter(|c| !c.passed).count()
}

// ===========================================================================
// F475 — 信任年报：年度隐私叙事存档
// ===========================================================================

pub const TRUST_REPORT_SECTIONS: [&str; 4] =
    ["incidents", "permissions-stats", "deletions", "learnings"];

// ===========================================================================
// 域自检
// ===========================================================================

pub fn run_m600priv_checks() -> CheckSet {
    let mut set = CheckSet::new("m600priv");

    // F451 权限仪表盘
    let grants = [
        PermissionGrant { app_id: 1, cap: CAP_CAMERA, granted: true, by_user: true },
        PermissionGrant { app_id: 2, cap: CAP_LOCATION, granted: false, by_user: false },
    ];
    set.add("F451 dashboard honest", perm_dashboard_ok(&grants), "user-granted only");
    set.add(
        "F451 silent grant caught",
        !PermissionGrant { app_id: 3, cap: CAP_MICROPHONE, granted: true, by_user: false }.honest(),
        "no silent grant",
    );
    set.add("F451 cap tally", granted_count(&grants, CAP_CAMERA) == 1 && granted_count(&grants, CAP_MICROPHONE) == 0, "count");

    // F452 数据流地图
    let edges = [
        DataFlowEdge { from_app: 10, to_app: 11, kind: FLOW_CROSS_APP },
        DataFlowEdge { from_app: 10, to_app: 0, kind: FLOW_NETWORK },
    ];
    let sandboxed = [20u32, 21];
    set.add("F452 flow map sane", dataflow_sane(&edges, &sandboxed), "edges valid");
    let leak = [DataFlowEdge { from_app: 20, to_app: 0, kind: FLOW_NETWORK }];
    set.add("F452 sandbox leak caught", !dataflow_sane(&leak, &sandboxed), "no net for sandbox");

    // F453 摄像头门卫
    let cam_ok = CameraGuard { app_granted: true, indicator_led: true, shutter_closed: false };
    let cam_shut = CameraGuard { app_granted: true, indicator_led: true, shutter_closed: true };
    set.add(
        "F453 camera guard",
        camera_frames_allowed(&cam_ok) && !camera_frames_allowed(&cam_shut),
        "grant+led+shutter",
    );

    // F454 麦克风指示灯
    set.add(
        "F454 mic indicator",
        mic_indicator_ok(true, true, 30) && mic_indicator_ok(false, false, 0),
        "sync on/off",
    );
    set.add(
        "F454 mic latency bound",
        !mic_indicator_ok(true, true, 80) && !mic_indicator_ok(true, false, 0),
        "latency<=50ms, never dark-while-live",
    );

    // F455 位置最小化
    let coarse = coarsen_coord(1237, 10);
    set.add("F455 coarsen grid", coarse == 1240 && coarsen_coord(1243, 10) == 1240, "round to grid");
    set.add("F455 precision minimized", location_minimized(5) && location_minimized(100) && !location_minimized(2), "loss>=800‰");

    // F456 剪贴板卫兵
    set.add(
        "F456 clipboard guard",
        clipboard_hold_ok(TAG_PASSWORD, 14_000) && !clipboard_hold_ok(TAG_PASSWORD, 15_000),
        "secret 15s",
    );
    set.add(
        "F456 clipboard normal",
        clipboard_hold_ok(TAG_TEXT, CLIPBOARD_CLEAR_MS) && !clipboard_hold_ok(TAG_URL, CLIPBOARD_CLEAR_MS + 1),
        "normal 60s",
    );

    // F457 截图防护
    set.add(
        "F457 screenshot shield",
        screenshot_allowed(0) && !screenshot_allowed(WIN_FLAG_SECURE | 0b1110),
        "secure wins",
    );

    // F458 隐私清理向导
    let cats = [
        CleanupCategory { name: "cache", items: 300, cleaned: 300 },
        CleanupCategory { name: "history", items: 200, cleaned: 100 },
    ];
    let prog = cleanup_progress_permille(&cats);
    set.add("F458 cleanup progress", prog == 800, "800‰ swept");
    set.add("F458 cleanup full", cleanup_progress_permille(&[CleanupCategory { name: "x", items: 5, cleaned: 5 }]) == 1000, "complete");

    // F459 匿名化遥测
    set.add(
        "F459 anonymized telemetry",
        telemetry_anonymous(false, 8) && !telemetry_anonymous(true, 8) && !telemetry_anonymous(false, 3),
        "no uid, k>=5",
    );

    // F460 零知识遥测开关
    set.add(
        "F460 zero-knowledge switch",
        telemetry_may_send(TelemetryChoice::OptIn)
            && !telemetry_may_send(TelemetryChoice::OptOut)
            && !telemetry_may_send(TelemetryChoice::Undecided),
        "explicit opt-in only",
    );

    // F461 应用行为公证
    let seed = 0xC0FF_EEu64;
    let evs = [0x1111u64, 0x2222, 0x3333];
    let l1 = behavior_chain_next(seed, evs[0]);
    let l2 = behavior_chain_next(l1, evs[1]);
    let l3 = behavior_chain_next(l2, evs[2]);
    let chain = [l1, l2, l3];
    set.add("F461 behavior notary", behavior_chain_valid(seed, &evs, &chain), "chain replay ok");
    let tampered = [l1, l2 ^ 1, l3];
    set.add("F461 tamper caught", !behavior_chain_valid(seed, &evs, &tampered), "flip detected");

    // F462 敏感操作二次确认
    set.add(
        "F462 confirm policy",
        op_confirm_satisfied(SensitiveOp::ChangePassword, CONFIRM_PASSWORD)
            && !op_confirm_satisfied(SensitiveOp::ChangePassword, 0),
        "password needed",
    );
    set.add(
        "F462 confirm 2fa",
        op_confirm_satisfied(SensitiveOp::ExportKeys, CONFIRM_PASSWORD | CONFIRM_BIOMETRIC)
            && !op_confirm_satisfied(SensitiveOp::ExportKeys, CONFIRM_PASSWORD),
        "vault export needs both",
    );

    // F463 防偷窥套装
    set.add(
        "F463 anti-peek",
        anti_peek_action(true, true) && anti_peek_action(false, false) && !anti_peek_action(true, false),
        "dim on presence",
    );

    // F464 访客模式
    set.add(
        "F464 guest mode",
        guest_permitted(0) && !guest_permitted(GUEST_DENY_HOME) && !guest_permitted(GUEST_DENY_PERSIST | GUEST_DENY_HOME),
        "home/persist denied",
    );

    // F465 儿童安全区
    let kid = KidZone { whitelist_ok: true, minutes_used: 45, budget_min: 60 };
    set.add(
        "F465 kid zone",
        kid_zone_ok(&kid)
            && !kid_zone_ok(&KidZone { whitelist_ok: false, minutes_used: 0, budget_min: 60 })
            && !kid_zone_ok(&KidZone { whitelist_ok: true, minutes_used: 61, budget_min: 60 }),
        "whitelist+budget",
    );

    // F466 密码学中台
    set.add("F466 ct_eq", ct_eq(&[1u8, 2, 3], &[1, 2, 3]) && !ct_eq(&[1u8, 2, 3], &[1, 2, 4]) && !ct_eq(&[1u8], &[1, 2]), "constant time");
    let nonces = [7u64, 9, 11];
    set.add("F466 nonce fresh", nonce_fresh(&nonces, 13) && !nonce_fresh(&nonces, 9), "never reuse");

    // F467 密钥保管库
    let vk = VaultEntry { id: 1, kind: KEY_PRIVATE, locked: false };
    set.add(
        "F467 vault export rule",
        !vault_export_allowed(KEY_PRIVATE) && vault_export_allowed(KEY_PUBLIC),
        "private stays",
    );
    set.add(
        "F467 vault usable",
        vault_usable(&vk) && !vault_usable(&VaultEntry { id: 2, kind: KEY_PUBLIC, locked: true }),
        "locked refused",
    );

    // F468 生物识别桥
    set.add(
        "F468 biometric bridge",
        biometric_match(900, true) && !biometric_match(900, false) && !biometric_match(849, true),
        "threshold+liveness",
    );

    // F469 安全启动链全检
    let good = [true, true, true, true, true];
    let broken = [true, true, false, true];
    set.add(
        "F469 secure boot chain",
        boot_chain_ok(&good) && !boot_chain_ok(&broken) && !boot_chain_ok(&[true, true, true]),
        "all links, >=4",
    );

    // F470 漏洞响应流程
    set.add(
        "F470 vuln sla",
        vuln_sla_days(SEV_CRITICAL) == 7
            && vuln_sla_days(SEV_HIGH) == 30
            && vuln_on_time(SEV_CRITICAL, 7)
            && !vuln_on_time(SEV_CRITICAL, 8),
        "graded sla",
    );

    // F471 隐私降级模式
    let dg = PrivacyDegrade { radios_off: true, telemetry_off: true, location_off: true };
    set.add(
        "F471 privacy degrade",
        privacy_degrade_complete(&dg)
            && !privacy_degrade_complete(&PrivacyDegrade { radios_off: true, telemetry_off: true, location_off: false }),
        "all off",
    );

    // F472 数据导出权
    set.add(
        "F472 export right",
        export_request_ok(20, true, true)
            && !export_request_ok(20, false, true)
            && !export_request_ok(31, true, true)
            && !export_request_ok(20, true, false),
        "open format, 30d, owner",
    );

    // F473 数据删除权
    let del = DeleteRequest { items: 5, purged: 5, backup_copies: 2, backups_purged: 2 };
    set.add(
        "F473 deletion right",
        deletion_complete(&del)
            && !deletion_complete(&DeleteRequest { items: 5, purged: 4, backup_copies: 2, backups_purged: 2 })
            && !deletion_complete(&DeleteRequest { items: 3, purged: 3, backup_copies: 1, backups_purged: 0 }),
        "full purge incl backups",
    );

    // F474 隐私回归走廊
    let cases = [
        PrivacyCase { name: "clipboard-expiry", passed: true },
        PrivacyCase { name: "camera-led", passed: true },
        PrivacyCase { name: "sandbox-net", passed: false },
    ];
    set.add("F474 privacy regression", privacy_regression_open(&cases) == 1 && privacy_regression_open(&[]) == 0, "open count");

    // F475 信任年报
    set.add("F475 trust report", TRUST_REPORT_SECTIONS.len() == 4, "archived");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f455_coarsen_rounding() {
        assert_eq!(coarsen_coord(1237, 10), 1240);
        assert_eq!(coarsen_coord(1243, 10), 1240);
        assert_eq!(coarsen_coord(-1235, 10), -1230); // 负数截断向零取整
        assert_eq!(coarsen_coord(7, 1), 7);
        assert_eq!(precision_loss_permille(5), 800);
    }

    #[test]
    fn f461_chain_replay() {
        let seed = 42u64;
        let evs = [1u64, 2, 3];
        let c0 = behavior_chain_next(seed, evs[0]);
        let c1 = behavior_chain_next(c0, evs[1]);
        let c2 = behavior_chain_next(c1, evs[2]);
        let chain = [c0, c1, c2];
        assert!(behavior_chain_valid(seed, &evs, &chain));
        // 长度不匹配直接拒绝
        assert!(!behavior_chain_valid(seed, &evs, &chain[..2]));
        // 事件改一位即失效
        let evs2 = [1u64, 2, 4];
        assert!(!behavior_chain_valid(seed, &evs2, &chain));
    }

    #[test]
    fn f462_policy_tiers() {
        assert_eq!(op_confirm_policy(SensitiveOp::ChangePassword), CONFIRM_PASSWORD);
        assert_eq!(
            op_confirm_policy(SensitiveOp::FactoryReset),
            CONFIRM_PASSWORD | CONFIRM_BIOMETRIC
        );
        assert!(op_confirm_satisfied(SensitiveOp::DeleteVault, CONFIRM_BIOMETRIC | CONFIRM_PASSWORD));
        assert!(!op_confirm_satisfied(SensitiveOp::DeleteVault, CONFIRM_PASSWORD));
    }

    #[test]
    fn f466_crypto_helpers() {
        assert!(ct_eq(b"abcd", b"abcd"));
        assert!(!ct_eq(b"abcd", b"abce"));
        assert!(!ct_eq(b"abc", b"abcd"));
        let log = [1u64, 0xFFFF_FFFF_FFFF_FFFF];
        assert!(nonce_fresh(&log, 2));
        assert!(!nonce_fresh(&log, 1));
    }

    #[test]
    fn f470_sla_ladder() {
        assert_eq!(vuln_sla_days(SEV_CRITICAL), 7);
        assert_eq!(vuln_sla_days(SEV_HIGH), 30);
        assert_eq!(vuln_sla_days(SEV_MEDIUM), 90);
        assert_eq!(vuln_sla_days(SEV_LOW), 180);
        assert!(vuln_on_time(SEV_HIGH, 29) && !vuln_on_time(SEV_HIGH, 31));
    }

    #[test]
    fn f475_domain_selfcheck_all_pass() {
        let set = run_m600priv_checks();
        assert!(set.len() >= 25, "got {}", set.len());
        assert!(set.all_passed());
    }
}
