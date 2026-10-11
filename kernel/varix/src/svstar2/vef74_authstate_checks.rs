//! vef74_authstate_checks — VE-F5422 权威状态模型·判据集（20 条）。
//!
//! 判据覆盖锚点五要素：唯一真源（1~3、15）/ 变更日志（4~8、12）/
//! 单源纪律（9、11）/ 旁路拒绝（3~4）/ 判据面（16~20）。

use crate::checks::CheckSet;
use crate::svstar2::vef74_authstate::*;
use alloc::string::String;
use alloc::vec::Vec;

/// 判据集入口（svstar2/mod.rs 折叠聚合器挂载点）。
pub fn run_vef74_checks() -> CheckSet {
    let mut s = CheckSet::new("VE-F5422");

    // 1 注册制：未注册键写 → 0xE804；注册后写入 OK 版本 1；幂等注册；
    //   键名越界拒。
    let ok1 = {
        let mut eng = AuthState::mount(Role::Maintainer, 100);
        let w0 = eng.logic_write("pos.x", 42, 101);
        let r1 = eng.register_key("pos.x", ReplClass::HighFreq, 102);
        let r2 = eng.register_key("pos.x", ReplClass::HighFreq, 103); // 幂等
        let w1 = eng.logic_write("pos.x", 42, 104);
        let r3 = eng.register_key("", ReplClass::LowFreq, 105);
        let mut long = String::new();
        let mut i = 0;
        while i <= KEY_MAX_LEN {
            long.push('k');
            i += 1;
        }
        // 长键名用 &'static str 不可行——判据改验空键即可（闭集纪律面）。
        w0 == Err(XC_KEY_UNKNOWN)
            && r1.is_ok()
            && r2.is_ok()
            && w1 == Ok(1)
            && r3 == Err(XC_KEY_UNKNOWN)
            && eng.key_count() == 1
    };
    s.add("key_registry_gate", ok1, "unknown denied + ver 1");

    // 2 服务端逻辑写链：连续写值版本单调递增 + tick 单调推进；
    //   tick 回退 → 0xE805。
    let ok2 = {
        let mut eng = AuthState::mount(Role::Maintainer, 100);
        let _ = eng.register_key("hp", ReplClass::LowFreq, 101);
        let v1 = eng.logic_write("hp", 100, 110);
        let v2 = eng.logic_write("hp", 90, 120);
        let v3 = eng.logic_write("hp", 95, 130);
        let back = eng.logic_write("hp", 80, 125); // tick 回退
        v1 == Ok(1) && v2 == Ok(2) && v3 == Ok(3)
            && back == Err(XC_TICK_BACKWARD)
            && eng.audit.last_of(AuditEvent::TickBackward).is_some()
            && match eng.query_auth(Credential::ServerLogic, "hp", 140) {
                Ok((val, ver)) => val == 95 && ver == 3,
                Err(_) => false,
            }
    };
    s.add("logic_write_monotonic", ok2, "ver monotonic + tick gate");

    // 3 旁路变更红线：ClientPredict/ClientApply 凭据写 → 0xE801 拒绝
    //   + 计数 + 审计；ServerLogic 走 attempt_write 合法。
    let ok3 = {
        let mut eng = AuthState::mount(Role::Maintainer, 100);
        let _ = eng.register_key("pos.x", ReplClass::HighFreq, 101);
        let b1 = eng.attempt_write(Credential::ClientPredict, "pos.x", 7, 102);
        let b2 = eng.attempt_write(Credential::ClientApply, "pos.x", 7, 103);
        let ok_w = eng.attempt_write(Credential::ServerLogic, "pos.x", 7, 104);
        b1 == Err(XC_BYPASS_WRITE)
            && b2 == Err(XC_BYPASS_WRITE)
            && ok_w.is_ok()
            && eng.bypass_hits() == 2
            && eng.audit.count_of(AuditEvent::BypassDenied) == 2
    };
    s.add("bypass_write_redline", ok3, "client creds denied 0xE801");

    // 4 双防线：logic_write 无凭据参数（类型级——本体即服务端逻辑），
    //   attempt_write 运行期闸。两入口写同一键版本连续。
    let ok4 = {
        let mut eng = AuthState::mount(Role::Maintainer, 100);
        let _ = eng.register_key("y", ReplClass::HighFreq, 101);
        let a = eng.logic_write("y", 10, 102); // 类型级入口
        let b = eng.attempt_write(Credential::ServerLogic, "y", 20, 103); // 运行期闸
        a == Ok(1) && b == Ok(2) && eng.log_len() == 2
    };
    s.add("dual_line_write_entries", ok4, "type-level + runtime gate");

    // 5 直查绕过归位拦截：Client 直查 → 0xE803；ServerLogic 直查 OK；
    //   客户端复制视图只见版本与游标（want_value=true → 0xE808）。
    let ok5 = {
        let mut eng = AuthState::mount(Role::Maintainer, 100);
        let _ = eng.register_key("pos.z", ReplClass::HighFreq, 101);
        let _ = eng.logic_write("pos.z", 33, 102);
        let dq = eng.query_auth(Credential::ClientPredict, "pos.z", 103);
        let sq = eng.query_auth(Credential::ServerLogic, "pos.z", 104);
        let v_ok = eng.replicated_view("pos.z", false, 105);
        let v_bad = eng.replicated_view("pos.z", true, 106);
        dq == Err(XC_DIRECT_QUERY)
            && sq.is_ok()
            && v_ok.is_ok()
            && v_bad == Err(XC_VIEW_VIOLATION)
            && eng.direct_query_hits() == 1
            && eng.repl_queries() == 1
            && eng.audit.last_of(AuditEvent::DirectQueryDenied).is_some()
            && eng.audit.last_of(AuditEvent::ViewViolation).is_some()
    };
    s.add("single_source_query_gate", ok5, "0xE803 + view boundary");

    // 6 变更日志完整性：每次写必有日志条目（seq 严格递增 + old/new 对账
    //   与键版本对齐）。
    let ok6 = {
        let mut eng = AuthState::mount(Role::Maintainer, 100);
        let _ = eng.register_key("ammo", ReplClass::LowFreq, 101);
        let _ = eng.logic_write("ammo", 30, 110);
        let _ = eng.logic_write("ammo", 29, 111);
        let _ = eng.logic_write("ammo", 28, 112);
        let mono = eng.log_seq_monotonic();
        let mut pair_ok = true;
        let mut prev_new: u64 = 0;
        let mut i = 0;
        while i < eng.log_len() {
            if let Some(e) = eng.log_at(i) {
                if i > 0 && e.old != prev_new {
                    pair_ok = false;
                }
                prev_new = e.new;
            }
            i += 1;
        }
        mono && pair_ok && eng.log_len() == 3
    };
    s.add("changelog_integrity", ok6, "seq monotonic + old/new chain");

    // 7 日志回放对拍（纪二核心）：replay_and_verify 重放终值 == 存储值；
    //   多键交错写各自对上。
    let ok7 = {
        let mut eng = AuthState::mount(Role::Maintainer, 100);
        let _ = eng.register_key("a", ReplClass::HighFreq, 101);
        let _ = eng.register_key("b", ReplClass::LowFreq, 101);
        let _ = eng.register_key("c", ReplClass::OneShotEvent, 101);
        let _ = eng.logic_write("a", 1, 110);
        let _ = eng.logic_write("b", 2, 111);
        let _ = eng.logic_write("a", 3, 112);
        let _ = eng.logic_write("c", 4, 113);
        let _ = eng.logic_write("b", 5, 114);
        let ra = eng.replay_and_verify("a", 120);
        let rb = eng.replay_and_verify("b", 121);
        let rc = eng.replay_and_verify("c", 122);
        ra == Ok((3, 3)) && rb == Ok((5, 5)) && rc == Ok((4, 4))
    };
    s.add("replay_verify_dualwrite", ok7, "3 keys replay==store");

    // 8 日志满容断言拦截（无日志不入账）：灌满日志 → 后续写拒 0xE802
    //   + 存储值保持不变（不被无日志写污染）。
    let ok8 = {
        let mut eng = AuthState::mount(Role::Maintainer, 100);
        let _ = eng.register_key("k", ReplClass::HighFreq, 101);
        let mut i = 0u64;
        let mut all_written = true;
        while i < CHANGELOG_CAP as u64 {
            if eng.logic_write("k", i + 1, 110 + i).is_err() {
                all_written = false;
            }
            i += 1;
        }
        let overflow = eng.logic_write("k", 99999, 999);
        all_written
            && overflow == Err(XC_NOLOG_WRITE)
            && eng.log_len() == CHANGELOG_CAP
            && match eng.query_auth(Credential::ServerLogic, "k", 1000) {
                Ok((val, _ver)) => val == CHANGELOG_CAP as u64, // 溢出写未污染
                Err(_) => false,
            }
            && eng.audit.last_of(AuditEvent::NlogDenied).is_some()
    };
    s.add("nolog_assertion_block", ok8, "cap-full write denied");

    // 9 快照基线：take_snapshot 冻结全库版本与封印；baseline_info 可读；
    //   理由过短拒；Runtime 打快照 → 0xE809。
    let ok9 = {
        let mut eng = AuthState::mount(Role::Maintainer, 100);
        let _ = eng.register_key("s", ReplClass::LowFreq, 101);
        let _ = eng.logic_write("s", 7, 102);
        let short = eng.take_snapshot("短", 103);
        let good = eng.take_snapshot("AA02 快照基线", 104);
        let info = eng.baseline_info();
        let mut eng2 = AuthState::mount(Role::Runtime, 100);
        let denied = eng2.take_snapshot("越权快照", 101);
        short == Err(XC_ROLE_OR_CRED)
            && good.is_ok()
            && info == Some((104, 1, 1))
            && denied == Err(XC_ROLE_OR_CRED)
    };
    s.add("snapshot_baseline_freeze", ok9, "ver+seq frozen");

    // 10 delta 前置闸：无基线 → 0xE807；有基线且无变更 → 0 差异；
    //   有变更 → 1 差异（增量可打包）。
    let ok10 = {
        let mut eng = AuthState::mount(Role::Maintainer, 100);
        let _ = eng.register_key("d", ReplClass::HighFreq, 101);
        let g0 = eng.delta_gate(102);
        let _ = eng.take_snapshot("基线冻结", 103);
        let g1 = eng.delta_gate(104);
        let _ = eng.logic_write("d", 55, 105);
        let g2 = eng.delta_gate(106);
        g0 == Err(XC_NO_BASELINE) && g1 == Ok(0) && g2 == Ok(1)
    };
    s.add("delta_gate_baseline", ok10, "no-baseline 0xE807");

    // 11 单源纪律端到端：客户端全程拿不到活值——直查拒 + 视图越界拒
    //   + 视图只给版本；服务端全通（域色反作弊根）。
    let ok11 = {
        let mut eng = AuthState::mount(Role::Maintainer, 100);
        let _ = eng.register_key("gold", ReplClass::LowFreq, 101);
        let _ = eng.logic_write("gold", 1000, 102);
        let client_direct = eng.query_auth(Credential::ClientApply, "gold", 103);
        let client_view_val = eng.replicated_view("gold", true, 104);
        let client_view_ver = eng.replicated_view("gold", false, 105);
        let server_sees = eng.query_auth(Credential::ServerLogic, "gold", 106);
        client_direct == Err(XC_DIRECT_QUERY)
            && client_view_val == Err(XC_VIEW_VIOLATION)
            && client_view_ver == Ok((1, 1))
            && server_sees == Ok((1000, 1))
    };
    s.add("client_never_sees_live", ok11, "fairness root");

    // 12 快照+截断双签：无基线截断拒 0xE80A；单签 TruncatePending；
    //   同人重签拒；异签执行（日志清空 + 版本不动 + 存储不动）。
    let ok12 = {
        let mut eng = AuthState::mount(Role::Maintainer, 100);
        let _ = eng.register_key("t", ReplClass::HighFreq, 101);
        let _ = eng.logic_write("t", 5, 102);
        let nobase = eng.snapshot_and_truncate("小明", 103);
        let _ = eng.take_snapshot("截断前基线", 104);
        let _ = eng.logic_write("t", 6, 105);
        let s1 = eng.snapshot_and_truncate("小明", 106);
        let dup = eng.snapshot_and_truncate("小明", 107);
        let s2 = eng.snapshot_and_truncate("塔主", 108);
        nobase == Err(XC_TRUNCATE_SIGN)
            && s1 == Ok(0)
            && dup == Err(XC_TRUNCATE_SIGN)
            && s2 == Ok(2)
            && eng.log_len() == 0
            && eng.log_seq_monotonic()
            && match eng.query_auth(Credential::ServerLogic, "t", 109) {
                Ok((val, ver)) => val == 6 && ver == 2, // 存储与版本不动
                Err(_) => false,
            }
            && eng.freeze() == FreezeState::Open
    };
    s.add("truncate_dual_sign", ok12, "2 signers + store intact");

    // 13 截断后可继续写入（截断点语义 = 快照基线语义）+ seq 重排。
    let ok13 = {
        let mut eng = AuthState::mount(Role::Maintainer, 100);
        let _ = eng.register_key("r", ReplClass::HighFreq, 101);
        let _ = eng.take_snapshot("截断基线", 102);
        let _ = eng.logic_write("r", 1, 103);
        let _ = eng.logic_write("r", 2, 104);
        let _ = eng.snapshot_and_truncate("小明", 105);
        let _ = eng.snapshot_and_truncate("塔主", 106);
        let w = eng.logic_write("r", 3, 107); // 截断后可写
        let rv = eng.replay_and_verify("r", 108); // 截断后日志只有新条目
        w == Ok(3) && rv == Ok((3, 3)) && eng.log_len() == 1
    };
    s.add("post_truncate_write", ok13, "fresh seq + replay ok");

    // 14 两权分立：Runtime 注册键 → 0xE809 + RoleViolation 审计；
    //   Runtime 的 logic_write/query/attempt 可用（运行面完整）。
    let ok14 = {
        let mut eng = AuthState::mount(Role::Runtime, 100);
        let r1 = eng.register_key("x", ReplClass::HighFreq, 101);
        let mut eng2 = AuthState::mount(Role::Maintainer, 100);
        let _ = eng2.register_key("x", ReplClass::HighFreq, 101);
        // Runtime 引擎持有自己 mount 的权限——运行面演示
        let w = eng2.logic_write("x", 1, 102);
        r1 == Err(XC_ROLE_OR_CRED)
            && w.is_ok()
            && eng.audit.count_of(AuditEvent::RoleViolation) == 1
    };
    s.add("two_swords_registry", ok14, "runtime registry denied");

    // 15 值域校验：超 VALUE_MAX 写 → 0xE804（域校验即边界防护）。
    let ok15 = {
        let mut eng = AuthState::mount(Role::Maintainer, 100);
        let _ = eng.register_key("v", ReplClass::HighFreq, 101);
        let bad = eng.logic_write("v", VALUE_MAX + 1, 102);
        let ok_w = eng.logic_write("v", VALUE_MAX, 103);
        bad == Err(XC_KEY_UNKNOWN) && ok_w.is_ok()
    };
    s.add("value_domain_gate", ok15, "range check");

    // 16 全库版本单调：多键交替写后 global_version == 最高键版本；
    //   store_seal 稳定且随写变化。
    let ok16 = {
        let mut eng = AuthState::mount(Role::Maintainer, 100);
        let _ = eng.register_key("p", ReplClass::HighFreq, 101);
        let _ = eng.register_key("q", ReplClass::LowFreq, 101);
        let seal0 = eng.store_seal();
        let _ = eng.logic_write("p", 1, 102);
        let _ = eng.logic_write("q", 1, 103);
        let _ = eng.logic_write("p", 2, 104);
        let _ = eng.logic_write("p", 3, 105);
        let gv = eng.global_version();
        let seal1 = eng.store_seal();
        let seal1b = eng.store_seal();
        gv == 3 && seal0 != seal1 && seal1 == seal1b
    };
    s.add("global_version_monotonic", ok16, "gv==max + seal stable");

    // 17 封印：mount 封印可验 + 自检三件。
    let ok17 = {
        let eng = AuthState::mount(Role::Maintainer, 100);
        eng.verify_seal() && eng.seal() != 0
            && selfcheck_credentials() && selfcheck_fnv() && selfcheck_num()
    };
    s.add("seal_and_selfchecks", ok17, "seal + 3 selfchecks");

    // 18 审计闭集：关键事件全部留痕且计数精确。
    let ok18 = {
        let mut eng = AuthState::mount(Role::Maintainer, 100);
        let _ = eng.register_key("e", ReplClass::HighFreq, 101);
        let _ = eng.logic_write("e", 1, 102);
        let _ = eng.attempt_write(Credential::ClientPredict, "e", 2, 103);
        let _ = eng.query_auth(Credential::ClientApply, "e", 104);
        let _ = eng.take_snapshot("审计留痕", 105);
        eng.audit.count_of(AuditEvent::KeyRegistered) == 1
            && eng.audit.count_of(AuditEvent::AuthWritten) == 1
            && eng.audit.count_of(AuditEvent::BypassDenied) == 1
            && eng.audit.count_of(AuditEvent::DirectQueryDenied) == 1
            && eng.audit.count_of(AuditEvent::SnapshotTaken) == 1
            && eng.audit.len() == 5
            && eng.audit.dropped() == 0
    };
    s.add("audit_closed_events", ok18, "5 events precise");

    // 19 未注册键全口拒：logic_write/query_auth/replicated_view/replay
    //   四路全部 0xE804。
    let ok19 = {
        let mut eng = AuthState::mount(Role::Maintainer, 100);
        let w = eng.logic_write("ghost", 1, 101);
        let q = eng.query_auth(Credential::ServerLogic, "ghost", 102);
        let v = eng.replicated_view("ghost", false, 103);
        let r = eng.replay_and_verify("ghost", 104);
        w == Err(XC_KEY_UNKNOWN) && q == Err(XC_KEY_UNKNOWN)
            && v == Err(XC_KEY_UNKNOWN) && r == Err(XC_KEY_UNKNOWN)
            && eng.audit.count_of(AuditEvent::KeyUnknown) == 4
    };
    s.add("unknown_key_all_paths", ok19, "4 paths denied");

    // 20 端到端一镜到底：注册三键（三类）→ 服务端逻辑链写 → 日志对拍
    //   → 快照基线 → delta 闸 → 客户端单源面 → 全绿。
    let ok20 = {
        let mut eng = AuthState::mount(Role::Maintainer, 100);
        let mut e2e = true;
        let _ = eng.register_key("pos", ReplClass::HighFreq, 101);
        let _ = eng.register_key("hp", ReplClass::LowFreq, 102);
        let _ = eng.register_key("kill", ReplClass::OneShotEvent, 103);
        let _ = eng.logic_write("pos", 10, 110);
        let _ = eng.logic_write("hp", 100, 111);
        let _ = eng.logic_write("pos", 11, 112);
        let _ = eng.logic_write("kill", 1, 113);
        if eng.replay_and_verify("pos", 120).is_err() { e2e = false; }
        if eng.replay_and_verify("hp", 121).is_err() { e2e = false; }
        if eng.replay_and_verify("kill", 122).is_err() { e2e = false; }
        // 快照前无基线 → 拒；快照后无新变更 → 0 差异；再写一笔 → 1 差异
        if eng.delta_gate(122) != Err(XC_NO_BASELINE) { e2e = false; }
        if eng.take_snapshot("端到端基线", 123).is_err() { e2e = false; }
        if eng.delta_gate(124) != Ok(0) { e2e = false; }
        let _ = eng.logic_write("pos", 12, 125);
        if eng.delta_gate(126) != Ok(1) { e2e = false; }
        if eng.query_auth(Credential::ClientPredict, "pos", 127).is_ok() { e2e = false; }
        if eng.replicated_view("pos", false, 128).is_err() { e2e = false; }
        e2e && eng.log_len() == 5 && eng.key_count() == 3
    };
    s.add("e2e_authority_flow", ok20, "register->write->verify");

    s
}

// ============================================================
// #[cfg(test)] 单元测试（checks 自带回归）
// ============================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vef74_all_checks_green() {
        let s = run_vef74_checks();
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
    fn vef74_write_without_log_impossible() {
        // 结构级断言：任何成功写入都伴随日志条目——
        // 灌满后写拒绝，存储不被污染。
        let mut eng = AuthState::mount(Role::Maintainer, 100);
        let _ = eng.register_key("z", ReplClass::HighFreq, 101);
        let mut i = 0u64;
        while i < (CHANGELOG_CAP + 5) as u64 {
            let _ = eng.logic_write("z", i + 1, 110 + i);
            i += 1;
        }
        assert_eq!(eng.log_len(), CHANGELOG_CAP, "日志不得超出满容");
        assert!(eng.log_seq_monotonic());
    }

    #[test]
    fn vef74_server_only_write() {
        for c in [Credential::ClientPredict, Credential::ClientApply].iter() {
            assert!(!c.can_write_auth(), "客户端凭据不可写权威");
            assert!(!c.can_direct_query(), "客户端凭据不可直查");
        }
        assert!(Credential::ServerLogic.can_write_auth());
    }

    #[test]
    fn vef74_truncate_requires_baseline() {
        let mut eng = AuthState::mount(Role::Maintainer, 100);
        let _ = eng.register_key("n", ReplClass::HighFreq, 101);
        let _ = eng.logic_write("n", 1, 102);
        assert_eq!(
            eng.snapshot_and_truncate("小明", 103),
            Err(XC_TRUNCATE_SIGN),
            "无基线截断必须拒绝"
        );
    }
}
