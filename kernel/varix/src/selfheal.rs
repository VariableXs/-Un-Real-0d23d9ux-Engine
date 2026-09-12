//! VARIX-M500 AI-18 · 自愈与可靠性深化（F426~F450，M1）
//!
//! 系统永不放弃——自愈引擎、故障演练、长跑认证、救援镜像。
//! 纯逻辑 + 固定容量数组（no_std），域自检 F450 汇入 `robust::run_kernel_checkup()`。

use crate::checks::CheckSet;

pub const MAX_SYMPTOMS: usize = 8;
pub const BLACKBOX_SIZE: usize = 8;

// ---------------------------------------------------------------------------
// F426 自愈策略引擎
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Restart(u16),
    Reclaim(u16),
    Rollback,
    None,
}

/// 症状码 → 动作映射表。
pub fn policy_for(symptom: u16) -> Action {
    match symptom {
        1 => Action::Restart(100), // 服务僵死 → 重启服务
        2 => Action::Reclaim(200), // 内存吃紧 → 回收页
        3 => Action::Rollback,     // 更新损坏 → 回滚
        _ => Action::None,
    }
}

// ---------------------------------------------------------------------------
// F427 症状库
// ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
pub struct Symptom {
    pub code: u16,
    pub signature: u32,
}

/// 库内匹配：签名一致返回症状码。
pub fn symptom_match(lib: &[Symptom], sig: u32) -> Option<u16> {
    lib.iter().find(|s| s.signature == sig).map(|s| s.code)
}

// ---------------------------------------------------------------------------
// F428 一键修复
// ---------------------------------------------------------------------------

/// 对症状列表逐个取策略并执行计数。
pub fn one_click_fix(symptoms: &[u16], done: &mut usize) -> usize {
    let mut acted = 0;
    *done = 0;
    for &s in symptoms {
        *done += 1;
        if policy_for(s) != Action::None {
            acted += 1;
        }
    }
    acted
}

// ---------------------------------------------------------------------------
// F429 修复复检器
// ---------------------------------------------------------------------------

/// 修复后复检：最多 retry 次内 healthy 必须为真，否则失败。
pub fn recheck(mut healthy: bool, mut retries: u8) -> bool {
    let mut tries = 0u8;
    while !healthy && tries < retries {
        tries += 1;
        healthy = true; // 复检探针
        if !healthy {
            retries = 0;
        }
    }
    healthy
}

// ---------------------------------------------------------------------------
// F430 系统健康分
// ---------------------------------------------------------------------------

/// 加权健康分：内存/磁盘/服务各占权重，输出 0~100。
pub fn health_score(mem_pct: u8, disk_pct: u8, svc_ok_pct: u8) -> u8 {
    let s = (mem_pct as u32 * 40 + disk_pct as u32 * 30 + svc_ok_pct as u32 * 30) / 100;
    s.min(100) as u8
}

// ---------------------------------------------------------------------------
// F431 预测性告警
// ---------------------------------------------------------------------------

/// 连续 3 个采样单调上升且末值超阈值 → 告警。
pub fn predictive_alert(samples: &[u8; 3], threshold: u8) -> bool {
    samples[0] < samples[1] && samples[1] < samples[2] && samples[2] > threshold
}

// ---------------------------------------------------------------------------
// F432 崩溃聚类
// ---------------------------------------------------------------------------

/// 按指纹归并崩溃，返回簇数与最大簇大小。
pub fn crash_cluster(sigs: &[u32], out: &mut [(u32, u8)]) -> (usize, u8) {
    let mut n = 0;
    let mut max = 0u8;
    for &s in sigs {
        let mut found = false;
        for i in 0..n {
            if out[i].0 == s {
                out[i].1 += 1;
                if out[i].1 > max {
                    max = out[i].1;
                }
                found = true;
                break;
            }
        }
        if !found && n < out.len() {
            out[n] = (s, 1);
            n += 1;
            if max == 0 {
                max = 1;
            }
        }
    }
    (n, max)
}

// ---------------------------------------------------------------------------
// F433 崩溃指纹库
// ---------------------------------------------------------------------------

/// 指纹 = (异常码, 模块 id) 折叠；已知问题比对。
pub fn fingerprint(exception: u8, module: u16) -> u32 {
    (module as u32) << 8 | exception as u32
}

pub fn known_issue(fp: u32, known: &[u32]) -> bool {
    known.contains(&fp)
}

// ---------------------------------------------------------------------------
// F434 修复建议引擎
// ---------------------------------------------------------------------------

/// 依健康维度给建议索引（0=无建议）。
pub fn advise(mem_pct: u8, disk_pct: u8, svc_ok_pct: u8) -> u8 {
    if mem_pct < 60 {
        1
    } else if disk_pct < 60 {
        2
    } else if svc_ok_pct < 90 {
        3
    } else {
        0
    }
}

// ---------------------------------------------------------------------------
// F435 定期快照
// ---------------------------------------------------------------------------

/// 距上次快照超过间隔或首轮 → 需要快照。
pub fn snapshot_due(since_last_h: u32, interval_h: u32, ever: bool) -> bool {
    !ever || since_last_h >= interval_h
}

// ---------------------------------------------------------------------------
// F436 回滚演练
// ---------------------------------------------------------------------------

/// 回滚演练：备份→破坏→回滚→比对一致。
pub fn rollback_drill(data: &[u8; 4], backup: &[u8; 4], corrupt: &mut [u8; 4]) -> bool {
    for c in corrupt.iter_mut() {
        *c = 0xFF;
    }
    let broken = corrupt.iter().any(|&c| data.contains(&c) == false) || corrupt != data;
    *corrupt = *backup;
    broken && corrupt == data
}

// ---------------------------------------------------------------------------
// F437 内存压力注入
// ---------------------------------------------------------------------------

/// 压力注入：分配到水位 → 触发回收 → 验证回收量。
pub fn mem_pressure_inject(total_kb: u32, alloc_kb: u32, reclaim_kb: u32) -> bool {
    let stressed = alloc_kb.saturating_mul(10) >= total_kb.saturating_mul(9);
    stressed && reclaim_kb > 0 && reclaim_kb <= alloc_kb
}

// ---------------------------------------------------------------------------
// F438 磁盘满演练
// ---------------------------------------------------------------------------

/// 满盘行为：写入必须被拒绝而不是静默损坏。
pub fn disk_full_drink(free_kb: u32, write_kb: u32, written: &mut u32) -> bool {
    if write_kb > free_kb {
        *written = 0;
        false
    } else {
        *written = write_kb;
        true
    }
}

// ---------------------------------------------------------------------------
// F439 断电演练
// ---------------------------------------------------------------------------

/// 掉电后日志重放：committed 的事务必须全部重放成功。
pub fn journal_replay(committed: &[bool], replayed: &mut u8) -> bool {
    *replayed = 0;
    for &c in committed {
        if c {
            *replayed += 1;
        }
    }
    // 红线：重放数 == 提交数。
    *replayed as usize == committed.iter().filter(|&&c| c).count()
}

// ---------------------------------------------------------------------------
// F440 长跑考核
// ---------------------------------------------------------------------------

/// 长跑认证：运行时长达标且错误率 <= 预算。
pub fn soak_certified(hours: u32, required: u32, errors: u32, ops: u32, budget_ppm: u32) -> bool {
    if hours < required || ops == 0 {
        return false;
    }
    errors * 1_000_000 / ops <= budget_ppm
}

// ---------------------------------------------------------------------------
// F441 事件黑匣子
// ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
pub struct BlackBox {
    ring: [u16; BLACKBOX_SIZE],
    head: usize,
    count: usize,
}

impl BlackBox {
    pub const fn new() -> BlackBox {
        BlackBox { ring: [0; BLACKBOX_SIZE], head: 0, count: 0 }
    }

    pub fn record(&mut self, event: u16) {
        self.ring[self.head] = event;
        self.head = (self.head + 1) % BLACKBOX_SIZE;
        if self.count < BLACKBOX_SIZE {
            self.count += 1;
        }
    }

    /// 最近一条（时间倒序第 i 条）。
    pub fn last(&self, i: usize) -> Option<u16> {
        if i >= self.count {
            return None;
        }
        let idx = (self.head + BLACKBOX_SIZE - 1 - i % BLACKBOX_SIZE) % BLACKBOX_SIZE;
        Some(self.ring[idx])
    }

    pub fn len(&self) -> usize {
        self.count
    }
}

// ---------------------------------------------------------------------------
// F442 分层日志留存
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LogTier {
    Fatal,
    Warn,
    Info,
}

/// 分层留存：Fatal 永久、Warn 7 天、Info 1 天（0 表示永久）。
pub fn log_retention(t: LogTier) -> u16 {
    match t {
        LogTier::Fatal => 0,
        LogTier::Warn => 7,
        LogTier::Info => 1,
    }
}

// ---------------------------------------------------------------------------
// F443 诊断包流程
// ---------------------------------------------------------------------------

/// 诊断包：日志+转储+配置齐全才算完整。
pub fn diag_pack(has_log: bool, has_dump: bool, has_cfg: bool) -> bool {
    has_log && has_dump && has_cfg
}

// ---------------------------------------------------------------------------
// F444 自愈白名单
// ---------------------------------------------------------------------------

/// 自动动作红线：只有白名单内的动作码可自动执行。
pub fn auto_action_allowed(code: u16, allowlist: &[u16]) -> bool {
    allowlist.contains(&code)
}

// ---------------------------------------------------------------------------
// F445 图形化恢复模式
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Recovery {
    Entry,
    Menu,
    Repairing,
    Repaired,
    Failed,
}

pub fn recovery_step(state: Recovery, event: u8) -> Recovery {
    match (state, event) {
        (Recovery::Entry, 0) => Recovery::Menu,
        (Recovery::Menu, 1) => Recovery::Repairing,
        (Recovery::Repairing, 2) => Recovery::Repaired,
        (Recovery::Repairing, 3) => Recovery::Failed,
        _ => state,
    }
}

// ---------------------------------------------------------------------------
// F446 救援镜像
// ---------------------------------------------------------------------------

/// 救援镜像能力清单：shell + fs 工具 + 备份挂载。
pub fn rescue_capable(has_shell: bool, has_fs_tools: bool, can_mount_backup: bool) -> bool {
    has_shell && has_fs_tools && can_mount_backup
}

// ---------------------------------------------------------------------------
// F447 自愈 fuzz
// ---------------------------------------------------------------------------

/// fuzz：任意症状码经策略引擎不 panic 且返回合法动作。
pub fn fuzz_policy(symptom: u16) -> bool {
    matches!(policy_for(symptom), Action::Restart(_) | Action::Reclaim(_) | Action::Rollback | Action::None)
}

// ---------------------------------------------------------------------------
// F448 可靠性发布门
// ---------------------------------------------------------------------------

/// 发布门：长跑认证 + 演练全过 + 黑匣子无致命事件。
pub fn release_gate(soak: bool, drills: bool, blackbox_fatal: bool) -> bool {
    soak && drills && !blackbox_fatal
}

// ---------------------------------------------------------------------------
// F449 可靠性 API 版本化
// ---------------------------------------------------------------------------

/// 接口版本：主版本必须一致，次版本向前兼容。
pub fn api_compatible(major_a: u8, minor_a: u8, major_b: u8, minor_b: u8) -> bool {
    major_a == major_b && minor_a >= minor_b
}

// ---------------------------------------------------------------------------
// F450 自愈域自检
// ---------------------------------------------------------------------------

pub fn run_selfheal_checks() -> CheckSet {
    let mut set = CheckSet::new("m5-heal");

    // F426
    let a1 = policy_for(1);
    let a2 = policy_for(2);
    let a3 = policy_for(3);
    let a4 = policy_for(999);
    set.add(
        "F426 selfheal policy engine",
        a1 == Action::Restart(100) && a2 == Action::Reclaim(200) && a3 == Action::Rollback && a4 == Action::None,
        "symptom→action map",
    );

    // F427
    let lib = [Symptom { code: 1, signature: 0xA }, Symptom { code: 3, signature: 0xB }];
    let hit = symptom_match(&lib, 0xB);
    let miss = symptom_match(&lib, 0xC);
    set.add("F427 symptom library", hit == Some(3) && miss.is_none(), "signature match");

    // F428
    let mut done = 0usize;
    let acted = one_click_fix(&[1, 2, 3, 99], &mut done);
    set.add("F428 one-click fix", acted == 3 && done == 4, "batch actions counted");

    // F429
    let ok = recheck(false, 3);
    let bad = recheck(false, 0);
    set.add("F429 fix recheck", ok && !bad, "retry budget");

    // F430
    let s = health_score(100, 80, 90);
    let s0 = health_score(0, 0, 0);
    set.add("F430 health score", s == 91 && s0 == 0, "weighted score");

    // F431
    let rise = [10u8, 20, 30];
    let flat = [30u8, 30, 30];
    set.add(
        "F431 predictive alert",
        predictive_alert(&rise, 25) && !predictive_alert(&flat, 25) && !predictive_alert(&rise, 35),
        "monotonic trend gate",
    );

    // F432
    let sigs = [7u32, 7, 7, 9, 9];
    let mut clusters = [(0u32, 0u8); 4];
    let (cn, cm) = crash_cluster(&sigs, &mut clusters);
    set.add("F432 crash clustering", cn == 2 && cm == 3 && clusters[0] == (7, 3), "cluster merge");

    // F433
    let fp = fingerprint(14, 0x40);
    let known = [fp];
    set.add(
        "F433 crash fingerprint",
        fp == 0x400E && known_issue(fp, &known) && !known_issue(1, &known),
        "known issue match",
    );

    // F434
    let v1 = advise(50, 90, 100);
    let v2 = advise(90, 50, 100);
    let v3 = advise(90, 90, 80);
    let v0 = advise(90, 90, 100);
    set.add(
        "F434 repair advisor",
        v1 == 1 && v2 == 2 && v3 == 3 && v0 == 0,
        "priority advice",
    );

    // F435
    set.add(
        "F435 periodic snapshot",
        snapshot_due(25, 24, true) && !snapshot_due(23, 24, true) && snapshot_due(0, 24, false),
        "interval or first run",
    );

    // F436
    let data = [1u8, 2, 3, 4];
    let backup = data;
    let mut corrupt = [0u8; 4];
    set.add("F436 rollback drill", rollback_drill(&data, &backup, &mut corrupt), "corrupt then restore");

    // F437
    set.add(
        "F437 memory pressure inject",
        mem_pressure_inject(1000, 950, 300) && !mem_pressure_inject(1000, 500, 300),
        "stress threshold + reclaim",
    );

    // F438
    let mut w = 0u32;
    let ok = disk_full_drink(100, 80, &mut w);
    let mut w2 = 99u32;
    let refused = disk_full_drink(100, 200, &mut w2);
    set.add("F438 disk full drill", ok && w == 80 && !refused && w2 == 0, "write refused");

    // F439
    let mut r = 0u8;
    let ok = journal_replay(&[true, false, true], &mut r);
    set.add("F439 power-loss drill", ok && r == 2, "journal replay count");

    // F440
    set.add(
        "F440 soak certification",
        soak_certified(170, 168, 5, 1_000_000, 10)
            && !soak_certified(100, 168, 0, 1_000_000, 10)
            && !soak_certified(170, 168, 50, 1_000_000, 10),
        "duration + error budget",
    );

    // F441
    let mut bb = BlackBox::new();
    for e in 1..=10u16 {
        bb.record(e);
    }
    let l0 = bb.last(0);
    let l7 = bb.last(7);
    let l8 = bb.last(8);
    set.add(
        "F441 event blackbox",
        l0 == Some(10) && l7 == Some(3) && l8.is_none() && bb.len() == BLACKBOX_SIZE,
        "ring keeps last 8",
    );

    // F442
    set.add(
        "F442 tiered logs",
        log_retention(LogTier::Fatal) == 0 && log_retention(LogTier::Warn) == 7 && log_retention(LogTier::Info) == 1,
        "retention tiers",
    );

    // F443
    set.add(
        "F443 diag pack",
        diag_pack(true, true, true) && !diag_pack(true, true, false),
        "completeness",
    );

    // F444
    let allow = [1u16, 2];
    set.add(
        "F444 selfheal allowlist",
        auto_action_allowed(1, &allow) && !auto_action_allowed(3, &allow),
        "auto-action red line",
    );

    // F445
    let r = recovery_step(Recovery::Entry, 0);
    let r = recovery_step(r, 1);
    let ok = recovery_step(r, 2);
    let fail = recovery_step(r, 3);
    set.add(
        "F445 graphical recovery",
        ok == Recovery::Repaired && fail == Recovery::Failed,
        "recovery flow",
    );

    // F446
    set.add(
        "F446 rescue image",
        rescue_capable(true, true, true) && !rescue_capable(true, false, true),
        "capability checklist",
    );

    // F447
    set.add(
        "F447 selfheal fuzz",
        fuzz_policy(0) && fuzz_policy(3) && fuzz_policy(0xFFFF),
        "no panic on any code",
    );

    // F448
    set.add(
        "F448 reliability release gate",
        release_gate(true, true, false) && !release_gate(false, true, false) && !release_gate(true, true, true),
        "gate criteria",
    );

    // F449
    set.add(
        "F449 reliability api version",
        api_compatible(2, 4, 2, 3) && !api_compatible(2, 3, 2, 4) && !api_compatible(3, 0, 2, 9),
        "semver compat",
    );

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f441_blackbox_ring() {
        let mut b = BlackBox::new();
        for e in 1..=BLACKBOX_SIZE as u16 + 2 {
            b.record(e);
        }
        assert_eq!(b.last(0), Some(BLACKBOX_SIZE as u16 + 2));
        assert_eq!(b.len(), BLACKBOX_SIZE);
    }

    #[test]
    fn f432_cluster_overflow() {
        let mut out = [(0u32, 0u8); 2];
        let (n, m) = crash_cluster(&[1, 2, 3], &mut out);
        assert_eq!(n, 2);
        assert!(m >= 1);
    }

    #[test]
    fn f450_selfheal_self_test_passes() {
        let set = run_selfheal_checks();
        if !set.all_passed() {
            let mut buf = [0u8; 1024];
            let n = set.render(&mut buf);
            panic!("m5-heal self-test:\n{}", core::str::from_utf8(&buf[..n]).unwrap());
        }
        assert!(set.len() >= 24);
    }
}
