//! CGPU-F2246 判据层：降级执行器（锚点判据逐条映射）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/CGPU Varix STAR II · 总纲与施工书.md#CGPU-F2246`
//!
//! **锚点判据（接口冻结/语义复用/失败复用/三组/判据）→ 判据族**：
//! IF 4 / SEM 4 / FAIL 3 / META 3 = 14 项。
//!
//! # 本层核心纪律：判据侧独立重算，不向被测问答案
//!
//! 接口版本与冻结声明**判据侧独立写死逐字对拍**；语义档处置矩阵判据
//! 侧手算（建议任意次失败→留痕跳过；强制 1→重试 2→立案）；处置三态
//! 标签判据侧写死；回执账对账判据侧独立构造正反语料；码段判据 `!=`
//! 防自判死（0x50..0x5B 全排除）。

use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

use super::vco06_executor::*;

// ---------------------------------------------------------------------------
// 判据侧独立参照
// ---------------------------------------------------------------------------

/// 判据侧独立写死的四域标签（官方序逐字）。
const EXP_DOMAINS: [&str; 4] = ["调度域", "效果域", "呈现域", "资源域"];

/// 判据侧独立写死的处置三态标签（逐字）。
const EXP_DISPOSITIONS: [&str; 3] = ["留痕跳过", "重试恰一次", "升级立案"];

/// 判据侧独立写死的声明句（逐字）。
const EXP_FREEZE: &str =
    "执行器统一接口冻结 v1——ExecAction 五元组+execute 签名只增不改，下游各域按统一接口消费降级策略";
const EXP_SEMANTICS: &str =
    "执行语义复用——建议=J02 B 域建议语义模式，强制=合同强制模式，语义全域唯一不另立";
const EXP_FAILURE: &str =
    "执行失败处理复用——处置规则显性：建议失败留痕跳过不阻塞链，强制失败重试恰一次再失败升级立案";
const EXP_SCOPE: &str =
    "域受理边界——调度域收帧率动作、效果域收画质动作、呈现域收延迟动作、资源域收功耗动作，与 F2244 冲突维度四维对齐";

/// 全部诊断码（判据侧点名）。
const ALL_CODES: [XcCode; 5] = [
    XcCode::DOMAIN_NOT_READY,
    XcCode::DOMAIN_REJECT,
    XcCode::MANDATORY_ESCALATED,
    XcCode::API_VERSION_MISMATCH,
    XcCode::RECEIPT_BROKEN,
];

/// 预期判据条数（写前先数实挂 s.add 个数）。
const EXPECTED_CHECKS: usize = 14;

// ---------------------------------------------------------------------------
// 主判据
// ---------------------------------------------------------------------------

/// vco06 域自检入口。
pub fn run_vco06_checks() -> CheckSet {
    let mut s = CheckSet::new("cgpu-executor");

    // ================= 一、接口冻结（IF） =================

    // IF-1：接口版本字面量 + 四域闭集标签逐字对拍（判据侧写死）。
    let mut if1 = EXEC_API_V1 == "v1" && ExecDomain::ALL.len() == DOMAIN_COUNT;
    for i in 0..4 {
        if ExecDomain::ALL[i].label() != EXP_DOMAINS[i] {
            if1 = false;
        }
    }
    s.add("O06-IF-版本与四域标签对拍", if1, "");

    // IF-2：接口冻结声明逐字对拍（判据侧写死）。
    s.add("O06-IF-冻结声明逐字对拍", EXEC_API_FREEZE_NOTE == EXP_FREEZE, "");

    // IF-3：统一接口五元组端到端——构造动作→域受理→回执成功（接口可用）。
    let a = ExecAction {
        domain: ExecDomain::Scheduling,
        action_id: 3,
        semantics: Semantics::Mandatory,
        payload_digest: 0x1F2E,
        sequence: 7,
    };
    let if3 = execute(&a, DomainReply::Accepted) == Ok(Disposition::SkipLogged)
        && accepted_disposition() == Disposition::SkipLogged;
    s.add("O06-IF-五元组端到端受理", if3, "");

    // IF-4：域受理边界声明逐字对拍（判据侧写死）+ 四域落点与 F2244 对齐。
    s.add("O06-IF-域受理边界声明", DOMAIN_SCOPE_NOTE == EXP_SCOPE, "");

    // ================= 二、语义（SEM） =================

    // SEM-1：语义二闭集标签逐字对拍（判据侧写死）。
    s.add(
        "O06-SEM-语义二闭集标签",
        Semantics::Advisory.label() == "建议" && Semantics::Mandatory.label() == "强制",
        "",
    );

    // SEM-2：语义复用声明逐字对拍（判据侧写死）。
    s.add("O06-SEM-语义复用声明对拍", SEMANTICS_REUSE_NOTE == EXP_SEMANTICS, "");

    // SEM-3：语义档处置矩阵判据侧手算——建议任意次失败=留痕跳过；
    // 强制 1=重试恰一次、2=升级立案（恰边界 attempt=1/2 双查）。
    let adv = ExecAction {
        domain: ExecDomain::Effects,
        action_id: 1,
        semantics: Semantics::Advisory,
        payload_digest: 9,
        sequence: 1,
    };
    let man = ExecAction {
        domain: ExecDomain::Scheduling,
        action_id: 2,
        semantics: Semantics::Mandatory,
        payload_digest: 8,
        sequence: 2,
    };
    let sem3 = handle_failure(&adv, 1) == Disposition::SkipLogged
        && handle_failure(&adv, 5) == Disposition::SkipLogged
        && handle_failure(&man, 1) == Disposition::RetryOnce
        && handle_failure(&man, 2) == Disposition::Escalate
        && handle_failure(&man, 9) == Disposition::Escalate;
    s.add("O06-SEM-处置矩阵手算", sem3, "");

    // SEM-4：失败码按回执映射——受理/未就绪/拒绝三向。
    let sem4 = execute(&man, DomainReply::NotReady) == Err(XcCode::DOMAIN_NOT_READY)
        && execute(&man, DomainReply::Rejected) == Err(XcCode::DOMAIN_REJECT)
        && execute(&adv, DomainReply::Accepted) == Ok(Disposition::SkipLogged);
    s.add("O06-SEM-回执三向映射", sem4, "");

    // ================= 三、失败复用（FAIL） =================

    // FAIL-1：失败复用声明逐字对拍（判据侧写死）。
    s.add("O06-FAIL-失败复用声明对拍", FAILURE_REUSE_NOTE == EXP_FAILURE, "");

    // FAIL-2：处置三态标签逐字对拍（判据侧写死）。
    let fail2 = Disposition::SkipLogged.label() == EXP_DISPOSITIONS[0]
        && Disposition::RetryOnce.label() == EXP_DISPOSITIONS[1]
        && Disposition::Escalate.label() == EXP_DISPOSITIONS[2];
    s.add("O06-FAIL-处置三态标签对拍", fail2, "");

    // FAIL-3：回执账对账——正语料（严格递增+标记自洽）过；
    // 反语料双查（序号回退=账断裂；升级标记错位=账实不符）逐码拒。
    let good = [
        ExecReceipt { sequence: 1, disposition: Disposition::SkipLogged, escalated: false },
        ExecReceipt { sequence: 2, disposition: Disposition::RetryOnce, escalated: false },
        ExecReceipt { sequence: 5, disposition: Disposition::Escalate, escalated: true },
    ];
    let bad_order = [
        ExecReceipt { sequence: 2, disposition: Disposition::SkipLogged, escalated: false },
        ExecReceipt { sequence: 2, disposition: Disposition::SkipLogged, escalated: false },
    ];
    let bad_flag = [
        ExecReceipt { sequence: 1, disposition: Disposition::RetryOnce, escalated: true },
    ];
    let fail3 = audit_receipts(&good) == Ok(())
        && audit_receipts(&bad_order) == Err(XcCode::RECEIPT_BROKEN)
        && audit_receipts(&bad_flag) == Err(XcCode::RECEIPT_BROKEN);
    s.add("O06-FAIL-回执账正反语料对账", fail3, "");

    // ================= 四、判据自检（META） =================

    // META-3：码段独占——全部 0x5Cxx，且 != 0x50..0x5B（防自判死）。
    let section_ok = ALL_CODES.iter().all(|c| (c.code() >> 8) == 0x5C)
        && ALL_CODES.iter().all(|c| {
            let hi = c.code() >> 8;
            hi != 0x50 && hi != 0x51 && hi != 0x52 && hi != 0x53
                && hi != 0x54 && hi != 0x55 && hi != 0x56 && hi != 0x57
                && hi != 0x58 && hi != 0x59 && hi != 0x5A && hi != 0x5B
        });
    s.add("O06-META-诊断码段独占", section_ok, "");

    // META-2：码两两互异 + 人话原因非空 + 判据容量无截断。
    let mut code_ok = true;
    for i in 0..ALL_CODES.len() {
        for j in 0..ALL_CODES.len() {
            if i != j && ALL_CODES[i].code() == ALL_CODES[j].code() {
                code_ok = false;
            }
        }
    }
    for c in ALL_CODES {
        if c.reason().is_empty() {
            code_ok = false;
        }
    }
    s.add("O06-META-码互异与无截断", code_ok && !s.truncated(), "");

    // META-1：判据条数对账（放末位：此时 len 应为 13，加自身恰 14）。
    s.add("O06-META-判据条数对账", s.len() + 1 == EXPECTED_CHECKS, "");

    s
}
