//! vef73_reparch_checks — VE-F5421 复制总架构·判据集（20 条）。
//!
//! 判据覆盖锚点五要素：四段流水（1、11~15）/ 三类三策略（2、5~10）/
//! 宪法执行（3~4、18）/ 契约（7、11~15、17）/ 判据面（16、19~20）。

use crate::checks::CheckSet;
use crate::svstar2::vef73_reparch::*;
use alloc::string::String;
use alloc::vec::Vec;

/// 判据集入口（svstar2/mod.rs 折叠聚合器挂载点）。
pub fn run_vef73_checks() -> CheckSet {
    let mut s = CheckSet::new("VE-F5421");

    // 1 四段流水闭集：四段索引可逆 + 职责非空 + 三接口封印稳定。
    let ok1 = {
        let mut rev_ok = true;
        for i in 0..PIPELINE_DEPTH {
            match PipelineStage::from_index(i) {
                Some(st) => {
                    if st.index() != i || st.duty().len() == 0 {
                        rev_ok = false;
                    }
                }
                None => rev_ok = false,
            }
        }
        rev_ok
            && PipelineStage::from_index(4).is_none()
            && canonical_payload_seal(0) == canonical_payload_seal(0)
            && canonical_payload_seal(0) != canonical_payload_seal(1)
            && canonical_payload_seal(2).is_some()
            && canonical_payload_seal(3).is_none()
    };
    s.add("stages_closed_set", ok1, "4 stages + 3 iface seals");

    // 2 三类三策略双写：宪法矩阵 ↔ 纯函数三格全等（对拍判据面）。
    let ok2 = {
        selfcheck_matrix_dual()
            && policy_for(StateClass::HighFreq) == CopyPolicy::UnreliableLatest
            && policy_for(StateClass::LowFreq) == CopyPolicy::ReliableOrdered
            && policy_for(StateClass::OneShotEvent) == CopyPolicy::ReliableOnce
            && CLASS_POLICY_MATRIX[0] == CopyPolicy::UnreliableLatest
            && CLASS_POLICY_MATRIX[1] == CopyPolicy::ReliableOrdered
            && CLASS_POLICY_MATRIX[2] == CopyPolicy::ReliableOnce
            && StateClass::HighFreq.freq_budget_permille() == 1000
            && selfcheck_classes()
    };
    s.add("class_policy_dualwrite", ok2, "matrix==fn, 3 cells");

    // 3 宪法红线：客户端写权威 → 0xEB02 拒绝 + 计数 + 审计留痕；
    //   服务端写权威合法。
    let ok3 = {
        let mut eng = ReplicArch::mount(Role::Runtime, 100);
        let r1 = eng.write_auth(Peer::Client, 101);
        let r2 = eng.write_auth(Peer::Server, 102);
        r1 == Err(XC_CONSTITUTION_RED)
            && r2.is_ok()
            && eng.constitution_reds() == 1
            && eng.audit.last_of(AuditEvent::ConstitutionRed).is_some()
    };
    s.add("constitution_client_red", ok3, "client write denied 0xEB02");

    // 4 客户端合法通道：predict 与 client_apply（只读应用）畅通——
    //   客户端只预测不决定。
    let ok4 = {
        let mut eng = ReplicArch::mount(Role::Runtime, 100);
        let p = eng.predict(101);
        let a = eng.client_apply(102);
        p.is_ok() && a.is_ok() && eng.audit.count_of(AuditEvent::StageAdvanced) == 2
    };
    s.add("client_legal_paths", ok4, "predict+apply open");

    // 5 注册制：未注册类 publish → 0xEB04；注册后放行；重复注册幂等；
    //   Runtime 注册 → 0xEB09 越权。
    let ok5 = {
        let mut eng = ReplicArch::mount(Role::Maintainer, 100);
        let r0 = eng.publish(StateClass::HighFreq, 0, 101);
        let r1 = eng.register_class(StateClass::HighFreq, 102);
        let r2 = eng.register_class(StateClass::HighFreq, 103); // 幂等
        let r3 = eng.publish(StateClass::HighFreq, 0, 104);
        let mut eng2 = ReplicArch::mount(Role::Runtime, 100);
        let r4 = eng2.register_class(StateClass::LowFreq, 101);
        r0 == Err(XC_CLASS_UNKNOWN)
            && r1.is_ok()
            && r2.is_ok()
            && r3.is_ok()
            && r4 == Err(XC_SESSION_OR_ROLE)
            && eng.class_count() == 1
            && eng.audit.count_of(AuditEvent::RoleViolation) == 0
            && eng2.audit.count_of(AuditEvent::RoleViolation) == 1
    };
    s.add("class_registry_gate", ok5, "unknown denied + idempotent");

    // 6 宪法执行的结构级形态：publish 返回策略与宪法一致——
    //   API 无策略参数可选，选错在签名上不可达。
    let ok6 = {
        let mut eng = ReplicArch::mount(Role::Maintainer, 100);
        let mut all_ok = true;
        for c in StateClass::all().iter() {
            if eng.register_class(*c, 101).is_err() {
                all_ok = false;
            }
        }
        let p1 = eng.publish(StateClass::HighFreq, 0, 102);
        let p2 = eng.publish(StateClass::LowFreq, 0, 200);
        let p3 = eng.publish(StateClass::OneShotEvent, 1, 201);
        all_ok
            && p1 == Ok(CopyPolicy::UnreliableLatest)
            && p2 == Ok(CopyPolicy::ReliableOrdered)
            && p3 == Ok(CopyPolicy::ReliableOnce)
    };
    s.add("policy_structurally_bound", ok6, "policy from matrix only");

    // 7 契约冻结与版本拦截：冻结后契约在案（三接口 ver+seal）；
    //   冻结理由过短拒；Runtime 冻结拒 0xEB09。
    let ok7 = {
        let mut eng = ReplicArch::mount(Role::Maintainer, 100);
        let r0 = eng.freeze_contracts("短", 101);
        let r1 = eng.freeze_contracts("AA02 四段契约冻结", 102);
        let c0 = eng.contract(0);
        let c2 = eng.contract(2);
        let mut eng2 = ReplicArch::mount(Role::Runtime, 100);
        let r2 = eng2.freeze_contracts("越权冻结尝试", 101);
        r0 == Err(XC_SESSION_OR_ROLE)
            && r1.is_ok()
            && eng.freeze == FreezeState::Frozen
            && c0.map(|c| c.schema_ver == REPL_SCHEMA_VER && c.iface == 0).unwrap_or(false)
            && c2.map(|c| c.iface == 2 && c.payload_seal == canonical_payload_seal(2).unwrap_or(0)).unwrap_or(false)
            && r2 == Err(XC_SESSION_OR_ROLE)
    };
    s.add("contract_freeze_gate", ok7, "frozen + ver/seal in case");

    // 8 低频频次纪律：窗口内重发 0xEB08；窗口外放行；高频不受窗口限。
    let ok8 = {
        let mut eng = ReplicArch::mount(Role::Maintainer, 100);
        let _ = eng.register_class(StateClass::LowFreq, 101);
        let _ = eng.register_class(StateClass::HighFreq, 101);
        let a = eng.publish(StateClass::LowFreq, 0, 200);
        let b = eng.publish(StateClass::LowFreq, 0, 205); // 窗口内（<12）
        let c = eng.publish(StateClass::LowFreq, 0, 215); // 间隔 15 > 12
        // 高频连发 3 tick 全过
        let mut hz_ok = true;
        for t in 300..303 {
            if eng.publish(StateClass::HighFreq, 0, t).is_err() {
                hz_ok = false;
            }
        }
        a.is_ok() && b == Err(XC_FREQ_EXCEEDED) && c.is_ok()
            && hz_ok && eng.freq_violations() == 1
            && eng.audit.last_of(AuditEvent::FreqExceeded).is_some()
    };
    s.add("lowfreq_window_discipline", ok8, "window 12 tick gate");

    // 9 事件幂等（恰好一次）：同 seq 重放静默去重——不投递不报错，
    //   去重计数递增。
    let ok9 = {
        let mut eng = ReplicArch::mount(Role::Maintainer, 100);
        let _ = eng.register_class(StateClass::OneShotEvent, 101);
        let r1 = eng.publish(StateClass::OneShotEvent, 1, 102);
        let r2 = eng.publish(StateClass::OneShotEvent, 1, 103); // 重放
        let r3 = eng.publish(StateClass::OneShotEvent, 1, 104); // 再重放
        r1 == Ok(CopyPolicy::ReliableOnce)
            && r2 == Ok(CopyPolicy::ReliableOnce)
            && r3 == Ok(CopyPolicy::ReliableOnce)
            && eng.event_dedups() == 2
            && eng.audit.count_of(AuditEvent::EventDeduped) == 2
    };
    s.add("event_exactly_once_dedup", ok9, "dup seq silent dedup");

    // 10 事件有序（可靠恰好一次的顺序面）：seq 严格递增；
    //   跳号 0xEB05 拒绝；补号放行。
    let ok10 = {
        let mut eng = ReplicArch::mount(Role::Maintainer, 100);
        let _ = eng.register_class(StateClass::OneShotEvent, 101);
        let a = eng.publish(StateClass::OneShotEvent, 1, 102);
        let b = eng.publish(StateClass::OneShotEvent, 3, 103); // 跳号
        let c = eng.publish(StateClass::OneShotEvent, 2, 104); // 补号
        a.is_ok() && b == Err(XC_POLICY_VIOLATION) && c.is_ok()
            && eng.audit.last_of(AuditEvent::PolicyViolation).is_some()
    };
    s.add("event_strict_order", ok10, "gap rejected then filled");

    // 11 四段流水端到端：冻结 → 建会话 → S1→S2→S3→S4 顺序推进全过
    //   → completed() 为真 + advances==4 + verdicts 全 Pass。
    let ok11 = {
        let mut eng = ReplicArch::mount(Role::Maintainer, 100);
        let fr = eng.freeze_contracts("AA02 冻结", 101);
        let sid = eng.create_session(102);
        let mut flow_ok = true;
        for i in 0..PIPELINE_DEPTH {
            let stage = match PipelineStage::from_index(i) {
                Some(st) => st,
                None => return_false(),
            };
            let seal = if i == 0 {
                0 // S1 无进入接口
            } else {
                canonical_payload_seal(i - 1).unwrap_or(0)
            };
            let r = eng.advance(sid.unwrap_or(0), stage, REPL_SCHEMA_VER, seal, 110 + i as u64);
            if r != Ok(StageVerdict::Pass) {
                flow_ok = false;
            }
        }
        fr.is_ok() && sid.is_ok() && flow_ok
            && eng.session(sid.unwrap_or(0)).map(|x| x.completed()).unwrap_or(false)
            && eng.session(sid.unwrap_or(0)).map(|x| x.advances == 4).unwrap_or(false)
    };
    s.add("pipeline_e2e_pass", ok11, "S1..S4 all green");

    // 12 段序破坏：跳段（S1 后直推 S3）→ 0xEB06 + 该段 Blocked。
    let ok12 = {
        let mut eng = ReplicArch::mount(Role::Maintainer, 100);
        let _ = eng.freeze_contracts("AA02 冻结", 101);
        let sid = eng.create_session(102).unwrap_or(0);
        let r1 = eng.advance(sid, PipelineStage::Authoritative, REPL_SCHEMA_VER, 0, 110);
        let r2 = eng.advance(sid, PipelineStage::Transport, REPL_SCHEMA_VER,
            canonical_payload_seal(1).unwrap_or(0), 111);
        r1.is_ok() && r2 == Err(XC_STAGE_ORDER)
            && eng.session(sid).map(|x| x.verdicts[2] == StageVerdict::Blocked).unwrap_or(false)
            && eng.session(sid).map(|x| x.cursor == 0).unwrap_or(false)
    };
    s.add("stage_skip_rejected", ok12, "S1->S3 jump 0xEB06");

    // 13 契约版本拦截：旧 ver 入段 → 0xEB03 + Mismatch 留 verdict。
    let ok13 = {
        let mut eng = ReplicArch::mount(Role::Maintainer, 100);
        let _ = eng.freeze_contracts("AA02 冻结", 101);
        let sid = eng.create_session(102).unwrap_or(0);
        let _ = eng.advance(sid, PipelineStage::Authoritative, REPL_SCHEMA_VER, 0, 110);
        let r = eng.advance(sid, PipelineStage::SnapshotDelta, 0, // 旧版本
            canonical_payload_seal(0).unwrap_or(0), 111);
        r == Err(XC_CONTRACT_VER)
            && eng.session(sid).map(|x| x.verdicts[1] == StageVerdict::Mismatch).unwrap_or(false)
            && eng.audit.last_of(AuditEvent::ContractVerRejected).is_some()
    };
    s.add("contract_ver_intercept", ok13, "old ver 0xEB03");

    // 14 段间封印对拍：seal 用错接口形态 → 0xEB01（段间失配→对拍）。
    let ok14 = {
        let mut eng = ReplicArch::mount(Role::Maintainer, 100);
        let _ = eng.freeze_contracts("AA02 冻结", 101);
        let sid = eng.create_session(102).unwrap_or(0);
        let _ = eng.advance(sid, PipelineStage::Authoritative, REPL_SCHEMA_VER, 0, 110);
        // S2 应配 iface0 形态，传 iface1 形态 → 失配
        let wrong = canonical_payload_seal(1).unwrap_or(0);
        let r = eng.advance(sid, PipelineStage::SnapshotDelta, REPL_SCHEMA_VER, wrong, 111);
        r == Err(XC_CONTRACT_SEAL)
            && eng.audit.last_of(AuditEvent::ContractSealMismatch).is_some()
            // 正确形态放行
            && eng.advance(sid, PipelineStage::SnapshotDelta, REPL_SCHEMA_VER,
                canonical_payload_seal(0).unwrap_or(0), 112).is_ok()
    };
    s.add("contract_seal_mismatch", ok14, "wrong shape 0xEB01");

    // 15 解冻态推进拦截：Open 态 advance → 0xEB01（运行期必须冻结态）。
    let ok15 = {
        let mut eng = ReplicArch::mount(Role::Maintainer, 100);
        let sid = eng.create_session(101).unwrap_or(0);
        let r = eng.advance(sid, PipelineStage::Authoritative, REPL_SCHEMA_VER, 0, 102);
        r == Err(XC_CONTRACT_SEAL) && eng.freeze == FreezeState::Open
    };
    s.add("advance_requires_frozen", ok15, "open state 0xEB01");

    // 16 锚点簿：无锚点 delta → 0xEB07；打锚后放行；超期 stale 0xEB07。
    let ok16 = {
        let mut eng = ReplicArch::mount(Role::Maintainer, 100);
        let a = eng.delta_gate(100);
        eng.take_anchor(100);
        let b = eng.delta_gate(110);
        let c = eng.delta_gate(200); // 100 tick > 32 周期
        a == Err(XC_ANCHOR_ABSENT) && b.is_ok() && c == Err(XC_ANCHOR_ABSENT)
            && eng.anchor_age(110) == Some(10)
            && eng.audit.count_of(AuditEvent::AnchorTaken) == 1
            && eng.audit.last_of(AuditEvent::AnchorMissing).is_some()
            && eng.audit.last_of(AuditEvent::AnchorStale).is_some()
    };
    s.add("anchor_gate_period", ok16, "absent+stale 0xEB07");

    // 17 解冻双签：单签 ThawPending；同人重签拒 0xEB0A；异签解除回
    //   Open + 契约清空（0xEB0A 全链）。
    let ok17 = {
        let mut eng = ReplicArch::mount(Role::Maintainer, 100);
        let _ = eng.freeze_contracts("AA02 冻结", 101);
        let s1 = eng.unfreeze("小明", 102);
        let dup = eng.unfreeze("小明", 103);
        let s2 = eng.unfreeze("塔主", 104);
        s1 == Ok(FreezeState::ThawPending)
            && dup == Err(XC_DUAL_SIGN)
            && s2 == Ok(FreezeState::Open)
            && eng.freeze == FreezeState::Open
            && eng.contract(0).is_none()
            && eng.contract(1).is_none()
            && eng.contract(2).is_none()
    };
    s.add("thaw_dual_sign", ok17, "2 signers then open");

    // 18 两权分立：Runtime 冻结/注册/解冻三连拒 0xEB09 + RoleViolation
    //   审计三次；Runtime 的会话与 publish 可用（只读面完整）。
    let ok18 = {
        let mut eng = ReplicArch::mount(Role::Runtime, 100);
        let r1 = eng.freeze_contracts("越权冻结", 101);
        let r2 = eng.register_class(StateClass::HighFreq, 102);
        let r3 = eng.unfreeze("小明", 103);
        let sid = eng.create_session(104);
        r1 == Err(XC_SESSION_OR_ROLE) && r2 == Err(XC_SESSION_OR_ROLE)
            && r3 == Err(XC_SESSION_OR_ROLE)
            && sid.is_ok()
            && eng.audit.count_of(AuditEvent::RoleViolation) == 3
    };
    s.add("two_swords_runtime_denied", ok18, "3 denials + audit");

    // 19 会话未注册：不存在会话 advance → 0xEB09 + 审计。
    let ok19 = {
        let mut eng = ReplicArch::mount(Role::Maintainer, 100);
        let _ = eng.freeze_contracts("AA02 冻结", 101);
        let r = eng.advance(999, PipelineStage::Authoritative, REPL_SCHEMA_VER, 0, 102);
        r == Err(XC_SESSION_OR_ROLE)
            && eng.audit.last_of(AuditEvent::SessionUnknown).is_some()
            && eng.session_count() == 0
    };
    s.add("session_unknown_rejected", ok19, "ghost session 0xEB09");

    // 20 封印与端到端综合：verify_seal + selfcheck + 全宪法场景一镜到底
    //   （注册三类 → 冻结 → 会话四段 → 三类按策略 publish → 锚点闸）。
    let ok20 = {
        let eng = ReplicArch::mount(Role::Maintainer, 100);
        let seal_ok = eng.verify_seal() && eng.seal() != 0;
        let sc = selfcheck_stages() && selfcheck_classes() && selfcheck_num();
        // 综合场景
        let mut eng2 = ReplicArch::mount(Role::Maintainer, 100);
        let mut e2e = true;
        for c in StateClass::all().iter() {
            if eng2.register_class(*c, 101).is_err() {
                e2e = false;
            }
        }
        if eng2.freeze_contracts("端到端冻结", 102).is_err() {
            e2e = false;
        }
        let sid = match eng2.create_session(103) {
            Ok(id) => id,
            Err(_) => 0,
        };
        if sid == 0 {
            e2e = false;
        } else {
            for i in 0..PIPELINE_DEPTH {
                let stage = match PipelineStage::from_index(i) {
                    Some(st) => st,
                    None => return_false(),
                };
                let seal = if i == 0 { 0 } else { canonical_payload_seal(i - 1).unwrap_or(0) };
                if eng2.advance(sid, stage, REPL_SCHEMA_VER, seal, 110 + i as u64).is_err() {
                    e2e = false;
                }
            }
            // 三类发布（各自策略路径）
            if eng2.publish(StateClass::HighFreq, 0, 120).is_err() { e2e = false; }
            if eng2.publish(StateClass::LowFreq, 0, 120).is_err() { e2e = false; }
            if eng2.publish(StateClass::OneShotEvent, 1, 120).is_err() { e2e = false; }
            // 锚点闸
            eng2.take_anchor(121);
            if eng2.delta_gate(122).is_err() { e2e = false; }
        }
        seal_ok && sc && e2e
    };
    s.add("seal_selfcheck_e2e", ok20, "seal+3 checks+scenario");

    s
}

/// 判据辅助：恒返回 Authoritative（match 兜底）
fn return_false() -> PipelineStage {
    PipelineStage::Authoritative
}

// ============================================================
// #[cfg(test)] 单元测试（checks 自带回归）
// ============================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vef73_all_checks_green() {
        let s = run_vef73_checks();
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
    fn vef73_matrix_fn_equivalence() {
        for c in StateClass::all().iter() {
            assert_eq!(
                CLASS_POLICY_MATRIX[c.index()],
                policy_for(*c),
                "宪法矩阵与纯函数必须全等"
            );
        }
    }

    #[test]
    fn vef73_event_dedup_never_replays() {
        let mut eng = ReplicArch::mount(Role::Maintainer, 100);
        let _ = eng.register_class(StateClass::OneShotEvent, 101);
        for seq in 1..=5u64 {
            assert!(eng.publish(StateClass::OneShotEvent, seq, 102 + seq).is_ok());
        }
        // 1..=5 全部重放：全部去重，无一次投递
        for seq in 1..=5u64 {
            assert!(eng.publish(StateClass::OneShotEvent, seq, 200 + seq).is_ok());
        }
        assert_eq!(eng.event_dedups(), 5);
    }

    #[test]
    fn vef73_stage_cursor_monotonic() {
        let mut eng = ReplicArch::mount(Role::Maintainer, 100);
        let _ = eng.freeze_contracts("单调性", 101);
        let sid = eng.create_session(102).unwrap_or(0);
        let _ = eng.advance(sid, PipelineStage::Authoritative, REPL_SCHEMA_VER, 0, 110);
        let _ = eng.advance(sid, PipelineStage::SnapshotDelta, REPL_SCHEMA_VER,
            canonical_payload_seal(0).unwrap_or(0), 111);
        // 回段（重推 S1）→ 0xEB06
        let r = eng.advance(sid, PipelineStage::Authoritative, REPL_SCHEMA_VER, 0, 112);
        assert_eq!(r, Err(XC_STAGE_ORDER), "回段必须拒绝");
    }

    #[test]
    fn vef73_iface_seals_distinct() {
        let a = canonical_payload_seal(0).unwrap_or(0);
        let b = canonical_payload_seal(1).unwrap_or(0);
        let c = canonical_payload_seal(2).unwrap_or(0);
        assert!(a != b && b != c && a != c, "三接口形态封印必须互异");
    }
}
