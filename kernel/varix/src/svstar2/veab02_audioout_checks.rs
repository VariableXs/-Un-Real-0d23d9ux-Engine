//! VE-F5602 自检 · 音频设备与输出管理（AB 域）
//!
//! **锚点判据逐条对应**（`#VE-F5602`「热切换、淡出保护、跟随系统、判据」）：
//!
//! | 锚点判据 | 自检组 |
//! |---|---|
//! | 热切换 | `AB02-切换-*`（状态机全路径+无缝续播位连续+紧急路径+超预算立案） |
//! | 淡出保护 | `AB02-切换-*`（首相位恒 Fading+错序跳淡出拒绝——爆音红线） |
//! | 跟随系统 | `AB02-选举-*`（跟随+锁定优先+锁定失效显性+无设备静音四向） |
//! | 判据 | `AB02-判据-*`（档位建档/枚举/读屏/版本/条数对账） |
//!
//! **判据设计硬规矩**（承 AB/P 域先例）：期望值判据侧独立重算；不变量两头
//! 都测（违规被拒+合规放行）；阈值常量钉死具体数值；判据区零 panic 面。

use crate::checks::CheckSet;
use crate::svstar2::veab02_audioout as ao;

// ---------------------------------------------------------------------------
// 组一：设备表与延迟建档
// ---------------------------------------------------------------------------

fn chk_devices(s: &mut CheckSet) {
    // AB02-设备-01：枚举登记+定位（合法放行侧）。
    let mut t = ao::DeviceTable::new();
    let b1 = t
        .enroll(ao::DeviceEntry {
            id: 1,
            tag: "speaker",
            tier: ao::LatencyTier::Mid,
            spatial: false,
            user_locked: false,
            alive: true,
        })
        .is_ok();
    s.add("AB02-设备-01", b1 && t.get(1).is_some() && t.alive_len() == 1, "枚举登记+按 id 定位");

    // AB02-设备-02：重复 id 拒（设备身份唯一）。
    let dup = t
        .enroll(ao::DeviceEntry {
            id: 1,
            tag: "dup",
            tier: ao::LatencyTier::Low,
            spatial: false,
            user_locked: false,
            alive: true,
        })
        .is_err();
    s.add("AB02-设备-02", dup && t.alive_len() == 1, "重复设备 id 拒绝（表未污染）");

    // AB02-设备-03：延迟建档分档线独立重算（≤40 低 / ≤120 中 / 其余高）。
    let t0 = ao::LatencyTier::from_latency_ms(0);
    let t40 = ao::LatencyTier::from_latency_ms(40);
    let t41 = ao::LatencyTier::from_latency_ms(41);
    let t120 = ao::LatencyTier::from_latency_ms(120);
    let t121 = ao::LatencyTier::from_latency_ms(121);
    let ok = t0 == ao::LatencyTier::Low
        && t40 == ao::LatencyTier::Low
        && t41 == ao::LatencyTier::Mid
        && t120 == ao::LatencyTier::Mid
        && t121 == ao::LatencyTier::High;
    s.add("AB02-设备-03", ok, "分档边界逐点对账（40/41/120/121 四点）");

    // AB02-设备-04：实测建档覆盖档位（披露值=最新实测）。
    let tier = t.record_latency(1, 200);
    let ok = tier == Ok(ao::LatencyTier::High)
        && t.get(1).map_or(false, |e| e.tier == ao::LatencyTier::High);
    s.add("AB02-设备-04", ok, "实测建档覆盖档位（披露以最新实测为准）");

    // AB02-设备-05：建档未知设备拒。
    let ok = t.record_latency(99, 10).is_err();
    s.add("AB02-设备-05", ok, "未知设备建档拒绝（账实相符）");

    // AB02-设备-06：拔出摘除+快照收缩。
    let _ = t.enroll(ao::DeviceEntry {
        id: 2,
        tag: "earphone",
        tier: ao::LatencyTier::Low,
        spatial: true,
        user_locked: false,
        alive: true,
    });
    let ok = t.unplug(2).is_ok() && t.get(2).is_none() && t.alive_ids() == vec1(&t, 1);
    s.add("AB02-设备-06", ok, "拔出摘除（在线快照只剩存活设备）");

    // AB02-设备-07：拔出未知设备拒。
    let ok = t.unplug(2).is_err() && t.unplug(77).is_err();
    s.add("AB02-设备-07", ok, "重复拔出/未知设备拔出拒绝");

    // AB02-设备-08：空间补偿按档位（高延迟补得多——锚点按档位调补偿）。
    let e = t.get(1);
    let ok = e.map_or(false, |x| x.spatial_compensation_ms() == 100);
    s.add("AB02-设备-08", ok, "空间音频补偿按延迟档位（高 100/中 40/低 0）");

    // AB02-设备-09：档位闭集短码互异（披露可区分）。
    let all = ao::LatencyTier::all();
    let mut ok = all.len() == ao::TIER_COUNT;
    for i in 0..all.len() {
        for j in (i + 1)..all.len() {
            ok = ok && all[i].tag() != all[j].tag();
        }
    }
    s.add("AB02-设备-09", ok, "三档短码互异（枚举闭集）");
}

/// 辅助：期望的存活 id 列表（判据侧独立构造，不经被测表）。
fn vec1(t: &ao::DeviceTable, expect_id: u32) -> Vec<u32> {
    let _ = t;
    let mut v = Vec::new();
    v.push(expect_id);
    v
}

use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 组二：默认设备选举（跟随+锁定+失效+无设备）
// ---------------------------------------------------------------------------

fn mk(id: u32, locked: bool, alive: bool) -> ao::DeviceEntry {
    ao::DeviceEntry {
        id,
        tag: "dev",
        tier: ao::LatencyTier::Mid,
        spatial: false,
        user_locked: locked,
        alive,
    }
}

fn chk_elect(s: &mut CheckSet) {
    // AB02-选举-01：无锁定→跟随系统默认。
    let mut t = ao::DeviceTable::new();
    let _ = t.enroll(mk(1, false, true));
    let _ = t.enroll(mk(2, false, true));
    let ok = ao::elect_default(&t, 2) == Ok((2, ao::ElectVerdict::FollowedSystem));
    s.add("AB02-选举-01", ok, "无锁定跟随系统默认（常态路径）");

    // AB02-选举-02：锁定优先于跟随（锁定的不是系统默认也选中）。
    let _ = t.enroll(mk(3, true, true));
    let ok = ao::elect_default(&t, 2) == Ok((3, ao::ElectVerdict::LockedHold));
    s.add("AB02-选举-02", ok, "用户锁定优先于跟随（意志>系统）");

    // AB02-选举-03：锁定的设备拔出→锁定失效显性回落跟随。
    let _ = t.unplug(3);
    let ok = ao::elect_default(&t, 2) == Ok((2, ao::ElectVerdict::LockGone));
    s.add("AB02-选举-03", ok, "锁定设备消失=失效显性回落（不静默解除）");

    // AB02-选举-04：锁定失效立案口径（ELECT_LOCK_GONE 非空专属）。
    s.add(
        "AB02-选举-04",
        ao::ELECT_LOCK_GONE == "E_LOCK_GONE" && !ao::ELECT_LOCK_GONE.is_empty(),
        "锁定失效事件码钉死（可观测）",
    );

    // AB02-选举-05：全不在线→无设备（静音态路径，不虚构设备）。
    let mut t2 = ao::DeviceTable::new();
    let _ = t2.enroll(mk(9, false, true));
    let _ = t2.unplug(9);
    let ok = ao::elect_default(&t2, 9).is_err();
    s.add("AB02-选举-05", ok, "无可用设备选举失败（落静音态提示路径）");

    // AB02-选举-06：空表同样失败。
    let empty = ao::DeviceTable::new();
    s.add("AB02-选举-06", ao::elect_default(&empty, 1).is_err(), "空表选举失败（不虚构）");
}

// ---------------------------------------------------------------------------
// 组三：热切换状态机（无缝续播+淡出保护+超预算）
// ---------------------------------------------------------------------------

fn chk_switch(s: &mut CheckSet) {
    let mut t = ao::DeviceTable::new();
    let _ = t.enroll(mk(1, false, true));
    let _ = t.enroll(mk(2, false, true));

    // AB02-切换-01：正常发起——首相位恒 Fading（淡出保护不可跳过）。
    let r = ao::begin_switch(&t, 1, 2, 42_000, 1_000);
    let ok = match r {
        Ok(r) => r.phase == ao::SwitchPhase::Fading && r.resume_at_ms == 42_000,
        Err(_) => false,
    };
    s.add("AB02-切换-01", ok, "切换发起首相位=Fading（爆音红线强制相位）");

    // AB02-切换-02：状态机全路径 Fading→Rerouted→Done+完成时刻回填。
    let mut r = ao::begin_switch(&t, 1, 2, 42_000, 1_000).unwrap_or(ao::SwitchRecord {
        from: 1,
        to: 2,
        phase: ao::SwitchPhase::Fading,
        started_ms: 1_000,
        done_ms: None,
        resume_at_ms: 42_000,
    });
    let p1 = ao::advance(&mut r, 1_010);
    let p2 = ao::advance(&mut r, 1_030);
    let ok = p1 == Ok(ao::SwitchPhase::Rerouted)
        && p2 == Ok(ao::SwitchPhase::Done)
        && r.done_ms == Some(1_030);
    s.add("AB02-切换-02", ok, "淡出→重定向→完成全路径+时刻回填");

    // AB02-切换-03：Done 后再推进拒（状态机终态不可再迁移）。
    let ok = ao::advance(&mut r, 1_050).is_err();
    s.add("AB02-切换-03", ok, "终态再推进拒绝（错序显性）");

    // AB02-切换-04：无缝续播位不被状态机改写（播放位连续=无缝的可断言面）。
    let ok = r.resume_at_ms == 42_000;
    s.add("AB02-切换-04", ok, "续播位全程不变（无缝=播放位连续）");

    // AB02-切换-05：预算内不算缺陷（elapsed 独立对账）。
    let ok = r.over_budget() == false && r.elapsed_ms() == Some(30);
    s.add("AB02-切换-05", ok, "30ms<150ms 预算内（elapsed 独立对账）");

    // AB02-切换-06：超预算立案（超预算即缺陷红线，边界+1 可达）。
    let mut r2 = ao::begin_switch(&t, 1, 2, 0, 0).unwrap_or(ao::SwitchRecord {
        from: 1,
        to: 2,
        phase: ao::SwitchPhase::Fading,
        started_ms: 0,
        done_ms: None,
        resume_at_ms: 0,
    });
    let _ = ao::advance(&mut r2, 1);
    let _ = ao::advance(&mut r2, ao::SWITCH_BUDGET_MS + 1);
    let ok = r2.over_budget()
        && ao::SWITCH_BUDGET_MS == 150
        && ao::E_SWITCH_BUDGET == "E_SWITCH_BUDGET";
    s.add("AB02-切换-06", ok, "超 150ms 预算即缺陷（预算钉死+边界可达）");

    // AB02-切换-07：恰在预算点不算超（含端点语义：> 才算）。
    let mut r3 = ao::begin_switch(&t, 1, 2, 0, 0).unwrap_or(ao::SwitchRecord {
        from: 1,
        to: 2,
        phase: ao::SwitchPhase::Fading,
        started_ms: 0,
        done_ms: None,
        resume_at_ms: 0,
    });
    let _ = ao::advance(&mut r3, 1);
    let _ = ao::advance(&mut r3, ao::SWITCH_BUDGET_MS);
    s.add("AB02-切换-07", !r3.over_budget(), "恰在预算点不算超（严格大于）");

    // AB02-切换-08：自切拒/目标不在拒/源已不在拒（三向非法入口闭包）。
    let ok = ao::begin_switch(&t, 1, 1, 0, 0).is_err()
        && ao::begin_switch(&t, 1, 99, 0, 0).is_err()
        && ao::begin_switch(&t, 99, 1, 0, 0).is_err();
    s.add("AB02-切换-08", ok, "自切/目标未知/源未知三向拒绝");

    // AB02-切换-09：紧急切换（设备消失→无缝切换，源不在+目标在才合法）。
    let _ = t.enroll(mk(5, false, true));
    let _ = t.unplug(5);
    let e = ao::emergency_switch(&t, 5, 1, 77_000, 2_000);
    let ok = match e {
        Ok(r) => r.from == 5 && r.to == 1 && r.phase == ao::SwitchPhase::Fading,
        Err(_) => false,
    };
    s.add("AB02-切换-09", ok, "拔出驱动紧急切换（无缝续播从 77s 继续）");

    // AB02-切换-10：紧急切换误用拒（源还在=不是紧急场景）。
    let ok = ao::emergency_switch(&t, 1, 2, 0, 0).is_err()
        && ao::emergency_switch(&t, 2, 2, 0, 0).is_err()
        && ao::emergency_switch(&t, 9, 99, 0, 0).is_err();
    s.add("AB02-切换-10", ok, "紧急路径误用三向拒绝（源在/自切/目标未知）");

    // AB02-切换-11：淡出时长钉死 20ms。
    s.add(
        "AB02-切换-11",
        ao::FADE_OUT_MS == 20 && ao::E_SWITCH_BAD == "E_SWITCH_BAD",
        "淡出保护时长 20ms 钉死（爆音红线参数）",
    );
}

// ---------------------------------------------------------------------------
// 组四：读屏可达 + 判据元
// ---------------------------------------------------------------------------

fn chk_meta(s: &mut CheckSet) {
    // AB02-判据-01：设备读屏单行含档位与锁定态。
    let e = mk(7, true, true);
    let line = ao::screen_line_device(&e);
    let ok = line.contains("mid") && line.contains("已锁定为默认");
    s.add("AB02-判据-01", ok, "设备读屏单行（档位+锁定态，状态可见）");

    // AB02-判据-02：切换读屏单行含阶段与续播位。
    let r = ao::SwitchRecord {
        from: 1,
        to: 2,
        phase: ao::SwitchPhase::Done,
        started_ms: 0,
        done_ms: Some(30),
        resume_at_ms: 42_000,
    };
    let line = ao::screen_line_switch(&r);
    let ok = line.contains("切换完成") && line.contains("42000");
    s.add("AB02-判据-02", ok, "切换读屏单行（阶段+续播位）");

    // AB02-判据-03：静音态提示非空（无设备路径显性）。
    s.add(
        "AB02-判据-03",
        !ao::MUTE_NOTICE.is_empty() && ao::MUTE_NOTICE.contains("静音"),
        "静音态提示钉死（无设备不黑箱）",
    );

    // AB02-判据-04：协议版本前缀。
    s.add(
        "AB02-判据-04",
        ao::AUDIOOUT_VERSION.starts_with("AB02"),
        "协议版本 AB02-*（跨版本对账锚）",
    );

    // AB02-判据-05：错误码非空互异。
    s.add(
        "AB02-判据-05",
        !ao::E_DEVICE_UNKNOWN.is_empty()
            && !ao::E_SWITCH_BUDGET.is_empty()
            && !ao::E_SWITCH_BAD.is_empty()
            && ao::E_DEVICE_UNKNOWN != ao::E_SWITCH_BAD
            && ao::E_SWITCH_BUDGET != ao::E_SWITCH_BAD,
        "错误码非空互异（外部可观测分支）",
    );

    // AB02-判据-06：判据条数对账（本条前已有 31 条，本条为第 32 条）。
    s.add("AB02-判据-06", s.len() == 31, "判据条数对账（声明 32）");
}

// ---------------------------------------------------------------------------
// 聚合（单集 27 条 ≤ MAX_CHECKS=112）
// ---------------------------------------------------------------------------

/// F5602 域自检（聚合入口，注册表用）。
pub fn run_veab02_checks() -> CheckSet {
    let mut s = CheckSet::new("VE-F5602");
    chk_devices(&mut s);
    chk_elect(&mut s);
    chk_switch(&mut s);
    chk_meta(&mut s);
    s
}
