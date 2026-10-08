//! VE-F5602 域自检（VE-AB 域 · 音频设备与输出管理判据层）
//!
//! 判据侧**独立写死**期望（预算 120/淡出 24、三档补偿 100/50/0、读屏 3 行、
//! 码段 0x56 五码），不复用实现侧常量。聚合防自调：族内判据只调另一族
//! standalone + 进行中 set 自身 tally，守恒断言归 CI 探针层。

use crate::checks::CheckSet;

use super::veab02_devices as dev;
use super::veab02_devices::*;

use alloc::string::String;

// ---------------------------------------------------------------------------
// 判据侧独立写死的期望
// ---------------------------------------------------------------------------

const EXPECT_BUDGET_MS: u32 = 120;
const EXPECT_FADE_MS: u32 = 24;
const EXPECT_CODES: [u16; 5] = [0x5600, 0x5601, 0x5602, 0x5603, 0x5604];
const EXPECT_SCREEN_ROWS: usize = 3;

/// 建表：耳机（锁定用）/扬声器/HDMI，档位各一。
fn table() -> DeviceTable {
    let mut t = DeviceTable::new();
    t.upsert(Device {
        id: 1,
        name: String::from("有线耳机"),
        tier: LatencyTier::Low,
        user_locked: false,
        present: true,
    });
    t.upsert(Device {
        id: 2,
        name: String::from("扬声器"),
        tier: LatencyTier::Medium,
        user_locked: false,
        present: true,
    });
    t.upsert(Device {
        id: 3,
        name: String::from("HDMI"),
        tier: LatencyTier::High,
        user_locked: false,
        present: true,
    });
    t
}

// ---------------------------------------------------------------------------
// 一、规格（热切换 / 淡出保护 / 跟随系统）
// ---------------------------------------------------------------------------

fn chk_spec_hot_switch(set: &mut CheckSet) {
    // 规格-01：热切换走满状态机——当前设备翻面、playhead 保持、无缺陷。
    let t = table();
    let mut m = SwitchMachine::new();
    m.playhead = 48000;
    let r = m.hot_switch(&t, 1, 0, 90);
    let ok = match r {
        Ok(SwitchOutcome::Completed { elapsed_ms, playhead, over_budget }) => {
            elapsed_ms == 90 && playhead == 48000 && !over_budget
        }
        _ => false,
    } && m.current == Some(1)
        && m.phase == SwitchPhase::Done
        && m.over_budget_count == 0;
    if ok {
        set.ok("EAB2-规格-01-热切换无缝续播");
    } else {
        set.fail("EAB2-规格-01-热切换无缝续播", "状态机未走满或位置丢失");
    }
    // 规格-02：切换预算恰边界——120 不超、121 超且缺陷计数翻 1。
    let t = table();
    let mut m = SwitchMachine::new();
    let at_budget = m.hot_switch(&t, 1, 0, EXPECT_BUDGET_MS);
    let at_ok = matches!(
        at_budget,
        Ok(SwitchOutcome::Completed { over_budget: false, .. })
    ) && m.over_budget_count == 0;
    let over = m.hot_switch(&t, 2, 0, EXPECT_BUDGET_MS + 1);
    let over_ok = matches!(
        over,
        Ok(SwitchOutcome::Completed { over_budget: true, .. })
    ) && m.over_budget_count == 1;
    if at_ok && over_ok {
        set.ok("EAB2-规格-02-切换预算恰边界");
    } else {
        set.fail("EAB2-规格-02-切换预算恰边界", "超预算判定未在恰边界翻面");
    }
}

fn chk_spec_fade_protection(set: &mut CheckSet) {
    // 规格-03：淡出保护——淡出时长恒小于预算（防爆音结构约束），
    // 紧急切换耗时恰为淡出档（不是预算档）。
    let t = table();
    let mut m = SwitchMachine::new();
    let gone = m.device_gone(&t, 99);
    let _ = gone;
    // 设备 99 不在表中且不是当前输出——状态机不动（保护面）。
    let untouched = m.emergency_switches == 0 && m.current.is_none();
    // 真消失：当前设备被拔 → 紧急切换到替补，耗时 = 淡出档。
    m.current = Some(1);
    m.playhead = 777;
    let r = m.device_gone(&t, 1);
    let ok = untouched
        && match r {
            SwitchOutcome::Completed { elapsed_ms, playhead, over_budget } => {
                elapsed_ms == EXPECT_FADE_MS && playhead == 777 && !over_budget
            }
            _ => false,
        }
        && m.emergency_switches == 1
        && m.current == Some(2);
    if ok {
        set.ok("EAB2-规格-03-淡出保护与紧急切换");
    } else {
        set.fail("EAB2-规格-03-淡出保护与紧急切换", "紧急切换未走淡出档或位置丢失");
    }
}

fn chk_spec_follow_system(set: &mut CheckSet) {
    // 规格-04：跟随系统——跟随请求把输出切到系统默认设备。
    let t = table();
    let mut m = SwitchMachine::new();
    let r = m.follow_system_default(&t, 2);
    if r == Ok(2) && m.current == Some(2) && m.locked_refusals == 0 {
        set.ok("EAB2-规格-04-跟随系统");
    } else {
        set.fail("EAB2-规格-04-跟随系统", "跟随未切换或误计数");
    }
    // 规格-05：锁定优先于跟随——锁定后跟随请求显式拒绝并计数；
    // 解锁后跟随恢复。
    m.lock(Some(3));
    let refused = m.follow_system_default(&t, 2);
    let lock_ok = refused == Err(E_AB02_LOCKED)
        && m.locked_refusals == 1
        && m.current == Some(2);
    m.lock(None);
    let resumed = m.follow_system_default(&t, 2) == Ok(2);
    if lock_ok && resumed {
        set.ok("EAB2-规格-05-锁定优先于跟随");
    } else {
        set.fail("EAB2-规格-05-锁定优先于跟随", "锁定未拦截跟随或解锁未恢复");
    }
    // 规格-06：锁定优先在默认设备选择上的体现——锁定设备的在位性优先。
    let mut t2 = table();
    t2.upsert(Device {
        id: 9,
        name: String::from("蓝牙音箱"),
        tier: LatencyTier::Medium,
        user_locked: true,
        present: true,
    });
    let picked = t2.pick_default();
    if picked == Some(9) {
        set.ok("EAB2-规格-06-默认选择锁定优先");
    } else {
        set.fail("EAB2-规格-06-默认选择锁定优先", "默认选择未优先锁定设备");
    }
}

// ---------------------------------------------------------------------------
// 二、边界（消失 / 静音 / 拒绝路径 / 档位补偿 / 读屏）
// ---------------------------------------------------------------------------

fn chk_bound_silent_and_reject(set: &mut CheckSet) {
    // 边界-01：全部拔光 → 静音态提示（状态不是故障，计数如实，读屏可见）。
    let mut t = table();
    let mut m = SwitchMachine::new();
    m.current = Some(1);
    t.upsert(Device {
        id: 1,
        name: String::from("有线耳机"),
        tier: LatencyTier::Low,
        user_locked: false,
        present: false,
    });
    t.upsert(Device {
        id: 2,
        name: String::from("扬声器"),
        tier: LatencyTier::Medium,
        user_locked: false,
        present: false,
    });
    t.upsert(Device {
        id: 3,
        name: String::from("HDMI"),
        tier: LatencyTier::High,
        user_locked: false,
        present: false,
    });
    let r = m.device_gone(&t, 1);
    let rows = m.screen_status(&t);
    let ok = r == SwitchOutcome::Silent
        && m.silent_entries == 1
        && m.current.is_none()
        && rows[0].contains("静音");
    if ok {
        set.ok("EAB2-边界-01-无设备静音态");
    } else {
        set.fail("EAB2-边界-01-无设备静音态", "静音态未进入或提示缺失");
    }
    // 边界-02：未知设备与同设备切换的专属拒绝码。
    let t = table();
    let mut m = SwitchMachine::new();
    let unknown = m.hot_switch(&t, 42, 0, 10);
    let _ = m.hot_switch(&t, 1, 0, 10);
    let same = m.hot_switch(&t, 1, 0, 10);
    let ok = unknown == Err(E_AB02_UNKNOWN_DEVICE)
        && same == Err(E_AB02_SAME_DEVICE)
        && m.current == Some(1);
    if ok {
        set.ok("EAB2-边界-02-未知与同设备拒绝");
    } else {
        set.fail("EAB2-边界-02-未知与同设备拒绝", "拒绝码漂移或误切");
    }
    // 边界-03：延迟档位补偿三档互异且单调递减（100/50/0 判据侧写死）。
    let comps = [
        LatencyTier::Low.spatial_compensation(),
        LatencyTier::Medium.spatial_compensation(),
        LatencyTier::High.spatial_compensation(),
    ];
    if comps == [100, 50, 0] {
        set.ok("EAB2-边界-03-延迟档位补偿");
    } else {
        set.fail("EAB2-边界-03-延迟档位补偿", "补偿档漂移");
    }
}

fn chk_idem_determinism(set: &mut CheckSet) {
    // 幂等-01：切换确定性——同设备表同参数，切换结局与播放位置无关
    //（O(1) 的可测面：切换不扫流、不依赖已播时长）。
    let t = table();
    let mut m1 = SwitchMachine::new();
    m1.playhead = 0;
    let r1 = m1.hot_switch(&t, 1, 0, 90);
    let mut m2 = SwitchMachine::new();
    m2.playhead = u64::MAX / 2;
    let r2 = m2.hot_switch(&t, 1, 0, 90);
    let same = match (r1, r2) {
        (
            Ok(SwitchOutcome::Completed { playhead: p1, over_budget: o1, .. }),
            Ok(SwitchOutcome::Completed { playhead: p2, over_budget: o2, .. }),
        ) => o1 == o2 && p1 == m1.playhead && p2 == m2.playhead,
        _ => false,
    };
    if same {
        set.ok("EAB2-幂等-01-切换与流长无关");
    } else {
        set.fail("EAB2-幂等-01-切换与流长无关", "切换结局依赖播放位置");
    }
    // 幂等-02：读屏行数预算固定 3 行（设备/相位/缺陷）。
    let t = table();
    let m = SwitchMachine::new();
    let rows = m.screen_status(&t);
    if rows.len() == EXPECT_SCREEN_ROWS && rows[2].contains("超预算") {
        set.ok("EAB2-幂等-02-读屏行数预算");
    } else {
        set.fail("EAB2-幂等-02-读屏行数预算", "读屏行数漂移或缺缺陷行");
    }
    // 幂等-03：设备表 upsert 同 id 覆盖不重复建账。
    let mut t = table();
    t.upsert(Device {
        id: 1,
        name: String::from("有线耳机（改）"),
        tier: LatencyTier::Low,
        user_locked: false,
        present: true,
    });
    let dup_free = t.all().iter().filter(|d| d.id == 1).count() == 1
        && t.present_count() == 3;
    if dup_free {
        set.ok("EAB2-幂等-03-设备表去重");
    } else {
        set.fail("EAB2-幂等-03-设备表去重", "同设备重复建账");
    }
}

// ---------------------------------------------------------------------------
// 三、判据承载力（零 panic / 码段独立复核 / 防自调）
// ---------------------------------------------------------------------------

/// 单遍词法剥除（F2807 教训：字符串内 `//` 不得被行注释剥离误伤）。
fn strip_lexical_noise(src: &str) -> String {
    let b = src.as_bytes();
    let mut out = String::new();
    let mut i = 0usize;
    while i < b.len() {
        if i + 1 < b.len() && b[i] == b'/' && b[i + 1] == b'/' {
            while i < b.len() && b[i] != b'\n' {
                i += 1;
            }
            continue;
        }
        if i + 1 < b.len() && b[i] == b'/' && b[i + 1] == b'*' {
            let mut depth = 1usize;
            i += 2;
            while i < b.len() && depth > 0 {
                if i + 1 < b.len() && b[i] == b'/' && b[i + 1] == b'*' {
                    depth += 1;
                    i += 2;
                } else if i + 1 < b.len() && b[i] == b'*' && b[i + 1] == b'/' {
                    depth -= 1;
                    i += 2;
                } else {
                    i += 1;
                }
            }
            continue;
        }
        if b[i] == b'"' {
            i += 1;
            while i < b.len() {
                if b[i] == b'\\' {
                    i += 2;
                    continue;
                }
                let closed = b[i] == b'"';
                i += 1;
                if closed {
                    break;
                }
            }
            continue;
        }
        if b[i] == b'\'' {
            i += 1;
            while i < b.len() {
                if b[i] == b'\\' {
                    i += 2;
                    continue;
                }
                let closed = b[i] == b'\'';
                i += 1;
                if closed {
                    break;
                }
            }
            continue;
        }
        if let Some(c) = src.get(i..i + 1) {
            out.push_str(c);
        }
        i += 1;
    }
    out
}

fn chk_criterion_zero_panic(set: &mut CheckSet) {
    // 判据-01：判据面零 panic（自扫本文件）。
    let src = include_str!("veab02_devices_checks.rs");
    let clean = strip_lexical_noise(src);
    let mut hits = 0usize;
    for pat in [".unwrap()", ".expect(", "panic!"].iter() {
        if clean.contains(pat) {
            hits += 1;
        }
    }
    if hits == 0 {
        set.ok("EAB2-判据-01-判据面零 panic");
    } else {
        set.fail("EAB2-判据-01-判据面零 panic", "判据面含 panic 面");
    }
    // 规格-07：生产面零 panic（扫实现文件）。
    let src = include_str!("veab02_devices.rs");
    let clean = strip_lexical_noise(src);
    let mut hits = 0usize;
    for pat in [".unwrap()", ".expect(", "panic!", "unwrap_or_else(||"].iter() {
        if clean.contains(pat) {
            hits += 1;
        }
    }
    if hits == 0 {
        set.ok("EAB2-规格-07-生产面零 panic");
    } else {
        set.fail("EAB2-规格-07-生产面零 panic", "生产面含 panic 面");
    }
}

fn chk_criterion_codes_independent(set: &mut CheckSet) {
    // 判据-02：码段独占独立复核——五码皆 0x56 细分段、互异、与写死值逐位等。
    let got = [
        E_AB02_NO_DEVICE,
        E_AB02_OVER_BUDGET,
        E_AB02_SAME_DEVICE,
        E_AB02_UNKNOWN_DEVICE,
        E_AB02_LOCKED,
    ];
    let mut ok = true;
    let mut i = 0usize;
    while i < got.len() {
        if got[i] & 0xFF00 != 0x5600 || got[i] != EXPECT_CODES[i] {
            ok = false;
        }
        let mut j = i + 1;
        while j < got.len() {
            if got[i] == got[j] {
                ok = false;
            }
            j += 1;
        }
        i += 1;
    }
    if ok {
        set.ok("EAB2-判据-02-码段独占独立复核");
    } else {
        set.fail("EAB2-判据-02-码段独占独立复核", "码漂移或撞码");
    }
}

fn chk_criterion_not_truncated(set: &mut CheckSet) {
    // 判据-03：聚合防自调——只调不递归的 A 族 + 自身进行中 tally；
    // 条数期望判据侧写死（A 族 12 条；判据-03 登记前 B 族进行中 3 条）。
    let a = run_veab02_checks_a_standalone();
    let (ap, af) = a.tally();
    let (sp, sf) = set.tally();
    let a_ok = ap + af == 12;
    let self_ok = sp + sf == 3;
    let no_trunc =
        !a.truncated() && !set.truncated() && a.dropped() == 0 && set.dropped() == 0;
    if a_ok && self_ok && no_trunc {
        set.ok("EAB2-判据-03-聚合守恒防自调");
    } else {
        set.fail("EAB2-判据-03-聚合守恒防自调", "族条数漂移或有截断/丢弃");
    }
}

// ---------------------------------------------------------------------------
// 入口（a=规格+边界+幂等 / b=判据承载力；合并入口供聚合器）
// ---------------------------------------------------------------------------

/// 判据族 a：规格 + 边界 + 幂等。
pub fn run_veab02_checks_a_standalone() -> CheckSet {
    let mut s = CheckSet::new("svstar2/veab02/a");
    chk_spec_hot_switch(&mut s);
    chk_spec_fade_protection(&mut s);
    chk_spec_follow_system(&mut s);
    chk_bound_silent_and_reject(&mut s);
    chk_idem_determinism(&mut s);
    s
}

/// 判据族 b：判据承载力。
pub fn run_veab02_checks_b_standalone() -> CheckSet {
    let mut s = CheckSet::new("svstar2/veab02/b");
    chk_criterion_zero_panic(&mut s);
    chk_criterion_codes_independent(&mut s);
    chk_criterion_not_truncated(&mut s);
    s
}

/// 全域判据入口（聚合器调用这个）。
pub fn run_veab02_checks() -> CheckSet {
    CheckSet::merge(run_veab02_checks_a_standalone(), run_veab02_checks_b_standalone())
}
