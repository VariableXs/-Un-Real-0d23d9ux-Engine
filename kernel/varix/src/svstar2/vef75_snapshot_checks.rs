//! vef75_snapshot_checks — VE-F5423 状态快照机制·判据集（20 条）。
//!
//! 判据覆盖锚点五要素：周期快照（1~3、10）/ 差量压缩（4~6、8）/
//! 锚点（7、9）/ 哈希校验（6、7）/ 判据面（11~20）。

use crate::checks::CheckSet;
use crate::svstar2::vef75_snapshot::*;
use alloc::string::String;
use alloc::vec::Vec;

/// 判据集入口（svstar2/mod.rs 折叠聚合器挂载点）。
pub fn run_vef75_checks() -> CheckSet {
    let mut s = CheckSet::new("VE-F5423");

    // 1 周期快照器：首拍 OK；半周期内节流拒 0xED02；满半周期后放行；
    //   快照在册可查。
    let ok1 = {
        let mut eng = SnapEngine::mount(Role::Maintainer, 0);
        let a = eng.take_snapshot(0xAAAA, 1, 64);
        let b = eng.take_snapshot(0xBBBB, 2, 80); // 距 64 仅 16 < 32
        let c = eng.take_snapshot(0xCCCC, 3, 100); // 距 64 为 36 ≥ 32
        a.is_ok()
            && b == Err(XC_PERIOD_LATE)
            && c.is_ok()
            && eng.snap_count() == 2
            && eng.snap_by_id(a.unwrap_or(0)).is_some()
            && eng.snap_by_id(c.unwrap_or(0)).is_some()
    };
    s.add("period_snapshot_throttle", ok1, "half-period throttle");

    // 2 保留窗口滚动淘汰：拍满 SNAPSHOT_KEEP+2 → 最旧淘汰 + 淘汰审计；
    //   最新快照始终可取。
    let ok2 = {
        let mut eng = SnapEngine::mount(Role::Maintainer, 0);
        let mut first_id = 0u32;
        let mut i = 0u64;
        while i < (SNAPSHOT_KEEP + 2) as u64 {
            let id = eng.take_snapshot(0x100 + i, i + 1, i * SNAPSHOT_PERIOD + 1);
            if i == 0 {
                first_id = id.unwrap_or(0);
            }
            i += 1;
        }
        eng.snap_count() == SNAPSHOT_KEEP
            && eng.snap_by_id(first_id).is_none() // 最旧已淘汰
            && eng.latest_snapshot().map(|x| x.global_ver).unwrap_or(0) == (SNAPSHOT_KEEP + 2) as u64
            && eng.audit.count_of(AuditEvent::SnapshotEvicted) == 2
    };
    s.add("keep_window_eviction", ok2, "oldest evicted");

    // 3 周期纪律检查：period_check 超期 → 0xED02 + 计数；按期 OK。
    let ok3 = {
        let mut eng = SnapEngine::mount(Role::Maintainer, 0);
        let _ = eng.take_snapshot(0xAA, 1, 10);
        let in_time = eng.period_check(70); // 距 10 = 60 ≤ 64
        let late = eng.period_check(80); // 距 10 = 70 > 64
        in_time.is_ok() && late == Err(XC_PERIOD_LATE) && eng.period_lates() == 1
    };
    s.add("period_late_alarm", ok3, "over 64 tick alarm");

    // 4 差量压缩链式衔接：基线封印衔接（首包接快照封印、后续接上一包
    //   post）；断链（prev 错）→ 0xED03。
    let ok4 = {
        let mut eng = SnapEngine::mount(Role::Maintainer, 0);
        let sid = eng.take_snapshot(0xEA00, 1, 10).unwrap_or(0);
        let d1 = eng.compress(sid, 0xEA00, 0xEA10, 40, 100, 20);
        let d2 = eng.compress(sid, 0xEA10, 0xEA20, 40, 100, 30);
        let bad = eng.compress(sid, 0xDEAD, 0xEA30, 40, 100, 40); // prev 断链
        d1.is_ok() && d2.is_ok() && bad == Err(XC_CHAIN_BREAK)
            && eng.delta_count() == 2
            && eng.audit.last_of(AuditEvent::ChainBreak).is_some()
    };
    s.add("delta_chain_seal_link", ok4, "prev==last.post enforced");

    // 5 压缩率底线拦截：dirty/total 低于 floor → 0xED08（原样传输拦截）；
    //   达标放行；压缩率数学自检。
    let ok5 = {
        let mut eng = SnapEngine::mount(Role::Maintainer, 0);
        let sid = eng.take_snapshot(0xA, 1, 10).unwrap_or(0);
        let low = eng.compress(sid, 0xA, 0xB, 10, 100, 20); // 100‰ < 300‰
        let ok_r = eng.compress(sid, 0xA, 0xB, 40, 100, 30); // 400‰ ≥ 300‰
        low == Err(XC_BELOW_FLOOR) && ok_r.is_ok() && selfcheck_rate_math()
            && eng.audit.last_of(AuditEvent::BelowFloor).is_some()
    };
    s.add("compression_floor_gate", ok5, "floor 300 permille");

    // 6 哈希校验（压缩失真防线）：应用封印 == 声称 post → OK；
    //   失真 → 0xED01 + 断链 + 全量重发标记。
    let ok6 = {
        let mut eng = SnapEngine::mount(Role::Maintainer, 0);
        let sid = eng.take_snapshot(0xA, 1, 10).unwrap_or(0);
        let d = eng.compress(sid, 0xA, 0xB, 40, 100, 20).unwrap_or(0);
        let good = eng.verify_hash(d, 0xB, 30);
        let d2 = eng.compress(sid, 0xB, 0xC, 40, 100, 40).unwrap_or(0);
        let bad = eng.verify_hash(d2, 0xFF, 50); // 失真
        good.is_ok()
            && bad == Err(XC_HASH_MISMATCH)
            && eng.delta_count() == 0 // 断链
            && eng.full_resends() == 1
            && eng.audit.last_of(AuditEvent::HashMismatch).is_some()
            && eng.audit.last_of(AuditEvent::FullResent).is_some()
    };
    s.add("hash_verify_distortion", ok6, "0xED01 then full resend");

    // 7 锚点机制：打锚 → verify_anchor 封印匹配 OK；封印漂移 → 0xED04
    //   丢失 → 清锚 + 全量重发。
    let ok7 = {
        let mut eng = SnapEngine::mount(Role::Maintainer, 0);
        let a0 = eng.verify_anchor(0x1, 10); // 无锚不判丢
        let _ = eng.take_anchor(0xABCD, 20);
        let ok_v = eng.verify_anchor(0xABCD, 30);
        let lost = eng.verify_anchor(0xFFFF, 40); // 漂移
        a0.is_ok() && ok_v.is_ok() && lost == Err(XC_ANCHOR_LOST)
            && eng.anchor_info().is_none()
            && eng.full_resends() == 1
            && eng.audit.last_of(AuditEvent::AnchorLost).is_some()
    };
    s.add("anchor_lost_full_resend", ok7, "0xED04 seal drift");

    // 8 delta 链长上限：链内 DELTA_CHAIN_MAX 段全部成功；第 MAX+1 段
    //   push 触发自动断链 + 全量重发；此后旧 prev 断链（链式完整性）。
    let ok8 = {
        let mut eng = SnapEngine::mount(Role::Maintainer, 0);
        let sid = eng.take_snapshot(0x0, 1, 10).unwrap_or(0);
        let mut prev: u64 = 0x0;
        let mut i = 0u64;
        let mut in_chain_ok = true;
        // 先压满 DELTA_CHAIN_MAX 段（第 9 段 push 后 len=9>8 触发清链）
        while i < DELTA_CHAIN_MAX as u64 {
            let next = prev.wrapping_add(0x11);
            if eng.compress(sid, prev, next, 50, 100, 20 + i).is_err() {
                in_chain_ok = false;
            }
            prev = next;
            i += 1;
        }
        // 第 MAX+1 段：push 后即触发 cap 清链 + 全量重发
        let trigger = eng.compress(sid, prev, prev + 0x11, 50, 100, 99);
        // 清链后旧 prev 断链
        let after = eng.compress(sid, prev + 0x11, prev + 0x22, 50, 100, 100);
        in_chain_ok && trigger.is_ok()
            && after == Err(XC_CHAIN_BREAK)
            && eng.delta_count() == 0 && eng.full_resends() >= 1
            && eng.delta_chain_len() == 0
    };
    s.add("delta_chain_cap", ok8, "cap forces full");

    // 9 锚点仅 Maintainer：Runtime 打锚/打快照 → 0xED09。
    let ok9 = {
        let mut eng = SnapEngine::mount(Role::Runtime, 0);
        let a = eng.take_snapshot(0x1, 1, 10);
        let b = eng.take_anchor(0x1, 11);
        a == Err(XC_ROLE) && b == Err(XC_ROLE)
            && eng.audit.count_of(AuditEvent::RoleViolation) == 2
    };
    s.add("maintainer_only_snap_anchor", ok9, "runtime denied");

    // 10 新连接补发：无快照 → 0xED05（等首快照）；有快照 → 拿最新 id
    //   + 恢复服务计数。
    let ok10 = {
        let mut eng = SnapEngine::mount(Role::Maintainer, 0);
        let e0 = eng.new_connection(10);
        let sid = eng.take_snapshot(0x77, 1, 20).unwrap_or(0);
        let e1 = eng.new_connection(30);
        e0 == Err(XC_SNAP_UNKNOWN) && e1 == Ok(sid)
            && eng.resume_served() == 1
            && eng.latest_snapshot().map(|x| x.store_seal).unwrap_or(0) == 0x77
    };
    s.add("new_connection_bootstrap", ok10, "latest snap served");

    // 11 断线恢复三路：最新快照 → ServedFromSnapshot；旧快照 →
    //   FullResend；无效 id → 0xED07 + FullResend。
    let ok11 = {
        let mut eng = SnapEngine::mount(Role::Maintainer, 0);
        let s1 = eng.take_snapshot(0xA, 1, 10).unwrap_or(0);
        let s2 = eng.take_snapshot(0xB, 2, 100).unwrap_or(0);
        let r1 = eng.request_resume(s2, 110);
        let r2 = eng.request_resume(s1, 111); // 旧快照
        let r3 = eng.request_resume(999, 112); // 无效
        r1 == Ok(ResumeOutcome::ServedFromSnapshot)
            && r2 == Ok(ResumeOutcome::FullResend)
            && r3 == Err(XC_RESUME_BAD_ID)
            && eng.audit.last_of(AuditEvent::ResumeBadId).is_some()
            && eng.audit.count_of(AuditEvent::FullResent) == 2
    };
    s.add("resume_three_paths", ok11, "latest/old/bad-id");

    // 12 未注册快照压缩拒：base_snap_id 不在册 → 0xED05。
    let ok12 = {
        let mut eng = SnapEngine::mount(Role::Maintainer, 0);
        let _ = eng.take_snapshot(0xA, 1, 10);
        let bad = eng.compress(999, 0xA, 0xB, 50, 100, 20);
        bad == Err(XC_SNAP_UNKNOWN)
            && eng.audit.last_of(AuditEvent::ResumeBadId).is_some()
    };
    s.add("compress_unknown_snap", ok12, "0xED05 base");

    // 13 非最新基线压缩拒：从旧快照长 delta → 0xED03（链式完整性）。
    let ok13 = {
        let mut eng = SnapEngine::mount(Role::Maintainer, 0);
        let s1 = eng.take_snapshot(0xA, 1, 10).unwrap_or(0);
        let _ = eng.take_snapshot(0xB, 2, 100).unwrap_or(0);
        let old_base = eng.compress(s1, 0xA, 0xC, 50, 100, 110);
        old_base == Err(XC_CHAIN_BREAK) && eng.delta_count() == 0
    };
    s.add("stale_base_chain_break", ok13, "only latest grows");

    // 14 压缩底线双签调参：单签 Pending；同人重签拒 0xED0A；异签生效
    //   （新底线立即约束压缩）。
    let ok14 = {
        let mut eng = SnapEngine::mount(Role::Maintainer, 0);
        let _ = eng.take_snapshot(0xA, 1, 10).unwrap_or(0);
        let s1 = eng.set_floor(500, "小明", 20);
        let dup = eng.set_floor(500, "小明", 21);
        let s2 = eng.set_floor(500, "塔主", 22);
        s1 == Ok(COMPRESSION_FLOOR_DEFAULT) // 首签未生效返回旧值
            && dup == Err(XC_FLOOR_SIGN)
            && s2 == Ok(500)
            && eng.floor_permille == 500
            && {
                // 新底线立即生效：250‰ < 500‰ → 拦截
                eng.compress(eng.latest_snapshot().map(|x| x.snap_id).unwrap_or(0),
                    0xA, 0xB, 25, 100, 30) == Err(XC_BELOW_FLOOR)
            }
    };
    s.add("floor_dual_sign_tuning", ok14, "2 signers then active");

    // 15 底线参数域校验：0 或 >1000 → 0xED0A。
    let ok15 = {
        let mut eng = SnapEngine::mount(Role::Maintainer, 0);
        let a = eng.set_floor(0, "小明", 10);
        let b = eng.set_floor(1001, "小明", 11);
        let c = eng.set_floor(1, "小明", 12); // 首签 OK（Pending）
        a == Err(XC_FLOOR_SIGN) && b == Err(XC_FLOOR_SIGN) && c == Ok(300)
    };
    s.add("floor_domain_check", ok15, "0/1001 rejected");

    // 16 积压治理对拍：窗口淘汰与快照数守恒（snap_count ≤ KEEP 恒成立）。
    let ok16 = {
        let mut eng = SnapEngine::mount(Role::Maintainer, 0);
        let mut i = 0u64;
        let mut always_ok = true;
        while i < (SNAPSHOT_KEEP * 3 + 1) as u64 {
            if eng.take_snapshot(0x10 + i, i + 1, i * SNAPSHOT_PERIOD + 1).is_err() {
                always_ok = false;
            }
            if eng.snap_count() > SNAPSHOT_KEEP {
                always_ok = false;
            }
            i += 1;
        }
        always_ok
            && eng.audit.count_of(AuditEvent::SnapshotEvicted) == SNAPSHOT_KEEP * 2 + 1
    };
    s.add("backlog_conservation", ok16, "count<=KEEP invariant");

    // 17 封印与自检：verify_seal + 自检三件。
    let ok17 = {
        let eng = SnapEngine::mount(Role::Maintainer, 0);
        eng.verify_seal() && eng.seal() != 0
            && selfcheck_rate_math() && selfcheck_fnv() && selfcheck_num()
    };
    s.add("seal_and_selfchecks", ok17, "seal + 3 selfchecks");

    // 18 审计闭集：关键事件精确计数。
    let ok18 = {
        let mut eng = SnapEngine::mount(Role::Maintainer, 0);
        let _ = eng.take_snapshot(0xA, 1, 10);
        let sid = eng.latest_snapshot().map(|x| x.snap_id).unwrap_or(0);
        let _ = eng.compress(sid, 0xA, 0xB, 40, 100, 20);
        let _ = eng.take_anchor(0xB, 30);
        eng.audit.count_of(AuditEvent::SnapshotTaken) == 1
            && eng.audit.count_of(AuditEvent::DeltaCompressed) == 1
            && eng.audit.count_of(AuditEvent::AnchorTaken) == 1
            && eng.audit.len() == 3
            && eng.audit.dropped() == 0
    };
    s.add("audit_closed_events", ok18, "3 events precise");

    // 19 恢复与全量重发会计：resume/resent 计数与审计一致。
    let ok19 = {
        let mut eng = SnapEngine::mount(Role::Maintainer, 0);
        let _ = eng.take_snapshot(0xA, 1, 10);
        let sid = eng.latest_snapshot().map(|x| x.snap_id).unwrap_or(0);
        let _ = eng.request_resume(sid, 20);
        let _ = eng.request_resume(424242, 30);
        eng.resume_served() == 1 && eng.full_resends() == 1
            && eng.audit.count_of(AuditEvent::ResumeServed) == 1
            && eng.audit.count_of(AuditEvent::ResumeBadId) == 1
            && eng.audit.count_of(AuditEvent::FullResent) == 1
    };
    s.add("resume_resent_ledger", ok19, "counters==audit");

    // 20 端到端一镜到底：快照 → 三段 delta 链（封印衔接）→ 哈希校验
    //   → 锚点重置 → 恢复请求 → 全绿。
    let ok20 = {
        let mut eng = SnapEngine::mount(Role::Maintainer, 0);
        let mut e2e = true;
        let sid = match eng.take_snapshot(0x1000, 1, 10) {
            Ok(id) => id,
            Err(_) => 0,
        };
        if sid == 0 {
            e2e = false;
        } else {
            // 三段 delta：0x1000→0x1010→0x1020→0x1030
            let mut prev: u64 = 0x1000;
            let mut i = 0u32;
            while i < 3 {
                let next = prev + 0x10;
                match eng.compress(sid, prev, next, 50, 100, 20 + i as u64) {
                    Ok(pid) => {
                        if eng.verify_hash(pid, next, 30 + i as u64).is_err() {
                            e2e = false;
                        }
                    }
                    Err(_) => e2e = false,
                }
                prev = next;
                i += 1;
            }
            // 锚点重置（全量兜底）
            if eng.take_anchor(0x1030, 60).is_err() { e2e = false; }
            if eng.verify_anchor(0x1030, 61).is_err() { e2e = false; }
            // 恢复
            match eng.request_resume(sid, 70) {
                Ok(ResumeOutcome::ServedFromSnapshot) => {}
                _ => e2e = false,
            }
        }
        e2e && eng.full_resends() == 0 && eng.resume_served() == 1
    };
    s.add("e2e_snap_delta_anchor", ok20, "full chain zero resend");

    s
}

// ============================================================
// #[cfg(test)] 单元测试（checks 自带回归）
// ============================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vef75_all_checks_green() {
        let s = run_vef75_checks();
        let (passed, failed) = s.tally();
        let mut di = 0;
        while di < s.len() {
            if let Some(it) = s.get(di) {
                if !it.passed {
                    println!("RED #{}: {} / {}", di, it.name, it.detail);
                }
            }
            di += 1;
        }
        assert_eq!(failed, 0, "failed={} passed={}", failed, passed);
        assert!(!s.truncated(), "checkset truncated");
        assert_eq!(passed, 20, "expect 20 checks, got {}", passed);
    }

    #[test]
    fn vef75_hash_mismatch_never_passes() {
        // 失真封印过不了哈希校验；失真即断链（后续同 id 查询转 0xED05
        // ——包已不在册，防线语义一致）。
        let mut eng = SnapEngine::mount(Role::Maintainer, 0);
        let sid = eng.take_snapshot(0xA, 1, 10).unwrap_or(0);
        let d = eng.compress(sid, 0xA, 0xB, 40, 100, 20).unwrap_or(0);
        assert!(eng.verify_hash(d, 0xB, 30).is_ok());
        let d2 = eng.compress(sid, 0xB, 0xC, 40, 100, 40).unwrap_or(0);
        assert_eq!(eng.verify_hash(d2, 0xC ^ 1, 50), Err(XC_HASH_MISMATCH));
        assert_eq!(eng.delta_count(), 0, "失真即断链");
        assert!(eng.full_resends() >= 1);
    }

    #[test]
    fn vef75_eviction_never_loses_latest() {
        let mut eng = SnapEngine::mount(Role::Maintainer, 0);
        let mut last_id = 0u32;
        for i in 0..10u64 {
            last_id = eng.take_snapshot(i, i + 1, i * SNAPSHOT_PERIOD + 1).unwrap_or(0);
        }
        assert_eq!(eng.latest_snapshot().map(|x| x.snap_id).unwrap_or(0), last_id);
        assert!(eng.snap_count() <= SNAPSHOT_KEEP);
    }

    #[test]
    fn vef75_floor_zero_always_rejected() {
        let mut eng = SnapEngine::mount(Role::Maintainer, 0);
        assert_eq!(eng.set_floor(0, "小明", 10), Err(XC_FLOOR_SIGN));
        assert_eq!(eng.set_floor(u32::MAX, "小明", 11), Err(XC_FLOOR_SIGN));
    }
}
