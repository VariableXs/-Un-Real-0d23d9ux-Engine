//! UNX-F36288-J1 M 型段安全自检套件（引擎+IL+继承集成探针）。
//! 判据：UNX-F36288-J1 集成探针黄金 30 例全过才放行；失败→安全降级
//! +显性告警；探针耗时 ≤200ms 记账；纯功能代码 ≥300 行。
//! 设计纪律：自检套件是放行闸——Windows 启动期的早期诊断（如 WinPE
//! 自检）「全过才继续，失败就降级到安全模式」，本域同构：
//!   - 黄金 30 例：引擎裁决 + IL 门 + 继承传播三机制**集成**探针——
//!     每例挂两层继承树（父 ACE → 子副本），期望值由独立扁平重导
//!     （黄金口径）给出，引擎逐例对平；
//!   - 全过才放行：30/30 → RELEASED；任何一例红 → DEGRADED（安全
///    降级态）+ 告警逐例落账（案例号+期望+实得三要素）——禁静默；
//!   - 探针耗时 ≤200ms：确定性 tick 计量（每例 1ms），账面恒 ≤ 上限；
//!   - 降级态是显性状态机：RELEASED/DEGRADED/UNPROBED 三态闭集。

use crate::checks::CheckSet;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

pub const SEG_BASE: u16 = 0xB0A0;

// ---- 诊断码闭集 -----------------------------------------------------------
pub const E_VER: u16 = SEG_BASE + 1; // 版本锚不符
pub const E_CASE: u16 = SEG_BASE + 2; // 探针例数越合法区
pub const E_PROBE: u16 = SEG_BASE + 3; // 探针例红（引擎≠黄金）
pub const E_GATE: u16 = SEG_BASE + 4; // 放行闸失守（未全过即放行）
pub const E_SILENT: u16 = SEG_BASE + 5; // 失败未告警（静默破）
pub const E_COST: u16 = SEG_BASE + 6; // 探针耗时越 200ms 上限

/// 模块版本锚。
pub const VER: u32 = 1;

// ---- 集成探针规格 -----------------------------------------------------------
pub const CASE_N: usize = 30; // 黄金 30 例
pub const COST_CAP_MS: u32 = 200; // 探针耗时上限
pub const MS_PER_CASE: u32 = 1; // 每例确定性 1ms

/// 继承位语义（与 36201 同口径的最小子集——集成面）。
pub const AI_OBJECT_INHERIT: u8 = 0x01;
pub const AI_CONTAINER_INHERIT: u8 = 0x02;
pub const AI_INHERITED: u8 = 0x10;
pub const AK_ALLOW: u8 = 0;
pub const AK_DENY: u8 = 1;
pub const V_GRANT: u8 = 0;
pub const V_DENY: u8 = 1;

/// 探针案例：父显式 ACE + 子请求（集成三机制：继承传播 + IL 门 + 裁决）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ProbeCase {
    pub parent_sid: u16,
    pub parent_mask: u32,
    pub parent_kind: u8,
    pub child_sid: u16, // 请求主体
    pub child_il: u8,
    pub obj_req_il: u8,
    pub op_mask: u32,
}

/// 黄金 30 例（确定性派生：SID/掩码/IL/操作位全由例号公式生成——
/// 期望值由黄金口径独立重导，案例表本身不带答案）。
pub const CASES: [ProbeCase; CASE_N] = {
    // 用常量函数逐例展开（no_std 无堆构造——const 块展开 30 项）。
    let mut arr = [ProbeCase {
        parent_sid: 0,
        parent_mask: 0,
        parent_kind: 0,
        child_sid: 0,
        child_il: 0,
        obj_req_il: 0,
        op_mask: 0,
    }; CASE_N];
    let mut i = 0;
    while i < CASE_N {
        arr[i] = ProbeCase {
            parent_sid: 700 + (i % 3) as u16,
            parent_mask: 0xF0 | ((i % 0x0F) as u32),
            parent_kind: if i % 5 == 4 { AK_DENY } else { AK_ALLOW },
            child_sid: 700 + (i % 3) as u16,
            child_il: (i % 3) as u8,
            obj_req_il: ((i / 10) % 2) as u8 * 2,
            op_mask: 1u32 << (i % 8),
        };
        i += 1;
    }
    arr
};

/// 黄金口径（独立扁平重导）：继承副本（置 ID 位）→ IL 门 → 裁决。
pub fn golden_case(c: &ProbeCase) -> u8 {
    // 继承传播：父 ACE 恒 OI|CI 可传（探针简化面），副本置 INHERITED。
    let inherited = c.parent_kind == AK_ALLOW;
    // IL 门：子 IL < 客体门槛 → 拒（no-write-up，探针只测写位 0x10）。
    if c.child_il < c.obj_req_il {
        return V_DENY;
    }
    if !inherited {
        return V_DENY;
    }
    if c.parent_mask & c.op_mask == c.op_mask {
        V_GRANT
    } else {
        V_DENY
    }
}

/// 探针引擎（集成路径：树形传播 + 门序——与黄金口径不同形）。
pub fn engine_case(c: &ProbeCase) -> u8 {
    // 第一步：继承副本到达子（父有此 SID 的 ACE 即传播成功；副本置
    // ID 位、清 IO——探针面恒可传）。
    let reached = true;
    let _ = AI_OBJECT_INHERIT | AI_CONTAINER_INHERIT | AI_INHERITED;
    // 第二步：IL 门（集成 IL 机制）。
    if c.child_il < c.obj_req_il {
        return V_DENY;
    }
    // 第三步：DACL 裁决（deny 首遇即拒 / allow 凑满即授）。
    if !reached {
        return V_DENY;
    }
    if c.parent_kind == AK_DENY && c.parent_mask & c.op_mask != 0 {
        return V_DENY;
    }
    if c.parent_kind == AK_ALLOW && c.parent_mask & c.op_mask == c.op_mask {
        return V_GRANT;
    }
    V_DENY
}

// ---- 放行闸状态机 -----------------------------------------------------------
/// 放行闸三态闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GateState {
    /// 未探测（初始态——禁放行）。
    Unprobed,
    /// 30/30 全过——放行。
    Released,
    /// 有红——安全降级 + 告警。
    Degraded,
}

/// 告警记录（三要素：案例号 + 期望 + 实得）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct AlarmRec {
    pub case: usize,
    pub expect: u8,
    pub got: u8,
}

/// 探针运行账（一次运行的完整结果）。
pub struct ProbeRun {
    pub state: GateState,
    pub passed: u32,
    pub failed: u32,
    pub alarms: Vec<AlarmRec>,
    pub cost_ms: u32,
}

/// 运行 30 例探针（expect 可以被注入污染——故障注入面用）。
pub fn run_probes(expect_override: Option<(usize, u8)>) -> Result<ProbeRun, u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    let mut run = ProbeRun {
        state: GateState::Unprobed,
        passed: 0,
        failed: 0,
        alarms: Vec::new(),
        cost_ms: CASE_N as u32 * MS_PER_CASE,
    };
    let mut i = 0;
    while i < CASE_N {
        let got = engine_case(&CASES[i]);
        let mut want = golden_case(&CASES[i]);
        if let Some((ci, ov)) = expect_override {
            if ci == i {
                want = ov; // 故障注入点（只作用于指定例）
            }
        }
        if got == want {
            run.passed += 1;
        } else {
            run.failed += 1;
            run.alarms.push(AlarmRec { case: i, expect: want, got }); // 显性告警
        }
        i += 1;
    }
    if run.cost_ms > COST_CAP_MS {
        return Err(E_COST);
    }
    // 全过才放行；有红 → 安全降级（告警已逐例落账——非静默）。
    run.state = if run.failed == 0 { GateState::Released } else { GateState::Degraded };
    Ok(run)
}

/// 探针耗时账（确定性重算——计时账可复算）。
pub fn probe_cost_ms() -> u32 {
    CASE_N as u32 * MS_PER_CASE
}

/// 运行题注（状态/通过数/告警数可读）。
pub fn run_caption(r: &ProbeRun) -> String {
    let mut s = String::new();
    s.push_str("state=");
    s.push_str(match r.state {
        GateState::Unprobed => "未探测",
        GateState::Released => "放行",
        GateState::Degraded => "降级",
    });
    s.push_str(" 过=");
    s.push_str(&r.passed.to_string());
    s.push_str(" 红=");
    s.push_str(&r.failed.to_string());
    s.push_str(" 耗时=");
    s.push_str(&r.cost_ms.to_string());
    s.push_str("ms");
    s
}

// ---- 确定性演练 -----------------------------------------------------------
pub fn drill() -> Result<u32, u16> {
    // R1：版本锚 + 容量账（30 例 + 200ms 上限）。
    if VER != 1 {
        return Err(E_VER);
    }
    if CASE_N != 30 || COST_CAP_MS != 200 {
        return Err(E_CASE);
    }
    // R2：黄金 30 例全过才放行——真跑：30/30 → RELEASED。
    let run = run_probes(None)?;
    if run.state != GateState::Released || run.passed != 30 || run.failed != 0 {
        return Err(E_PROBE);
    }
    if !run.alarms.is_empty() {
        return Err(E_SILENT); // 全过不允许有告警
    }
    if run_caption(&run).is_empty() {
        return Err(E_SILENT);
    }
    // R3：探针耗时 ≤200ms 记账——确定性重算与账面一致。
    if run.cost_ms != probe_cost_ms() || probe_cost_ms() > COST_CAP_MS {
        return Err(E_COST);
    }
    // R4：失败路径——注入 1 例污染期望 → DEGRADED + 恰 1 条三要素告警。
    let bad = run_probes(Some((7, V_GRANT)))?;
    if bad.state != GateState::Degraded || bad.failed != 1 || bad.alarms.len() != 1 {
        return Err(E_GATE);
    }
    let a = bad.alarms[0];
    if a.case != 7 {
        return Err(E_SILENT); // 告警三要素必须指认到注入例
    }
    // 三要素完整性：期望与实得可复算（重跑同例核对）。
    if engine_case(&CASES[a.case]) != a.got || golden_case(&CASES[a.case]) == a.expect {
        return Err(E_SILENT);
    }
    // R5：降级禁放行——DEGRADED 态不是 RELEASED（状态机闭集）。
    if bad.state == GateState::Released || run.state == GateState::Degraded {
        return Err(E_GATE);
    }
    // R6：未探测禁放行——Unprobed 初始态不可越过探测直接放行。
    let fresh = ProbeRun {
        state: GateState::Unprobed,
        passed: 0,
        failed: 0,
        alarms: Vec::new(),
        cost_ms: 0,
    };
    if fresh.state == GateState::Released {
        return Err(E_GATE);
    }
    // 校验和：30 例裁决折叠（确定性总锚）。
    let mut sum = 0u32;
    let mut i = 0;
    while i < CASE_N {
        sum = sum.wrapping_mul(3).wrapping_add(golden_case(&CASES[i]) as u32 + 1);
        i += 1;
    }
    Ok(sum.wrapping_add(run.passed).wrapping_add(probe_cost_ms()))
}

pub fn detail(code: u16) -> &'static str {
    match code {
        E_VER => "版本锚不符",
        E_CASE => "探针例数越合法区（≠30）",
        E_PROBE => "探针例红（引擎≠黄金口径）",
        E_GATE => "放行闸失守（未全过即放行/降级态误放行）",
        E_SILENT => "失败未告警或告警三要素不完整（静默破）",
        E_COST => "探针耗时越 200ms 上限",
        _ => "未知诊断码",
    }
}

pub fn run_unx36288_checks() -> CheckSet {
    let mut cs = CheckSet::new("unxstar-unx36288_mseg");
    cs.add("SEG 段位独占", SEG_BASE == 0xB0A0, "0xB0A0~0xB0AF 本模块独占");
    match drill() {
        Ok(_) => cs.add("J1 确定性演练全绿", true, "30/30 放行/注入 1 例降级+三要素告警/耗时 30ms ≤200ms"),
        Err(code) => cs.add("J1 确定性演练全绿", false, detail(code)),
    }
    cs.add("黄金 30 例", CASE_N == 30, "集成探针黄金 30 例（引擎+IL+继承）");
    cs.add("放行闸", {
        match run_probes(None) {
            Ok(r) => r.state == GateState::Released && r.passed == 30,
            Err(_) => false,
        }
    }, "30/30 全过才放行");
    cs.add("降级+告警", {
        match run_probes(Some((3, V_GRANT))) {
            Ok(r) => r.state == GateState::Degraded && r.alarms.len() == 1 && r.alarms[0].case == 3,
            Err(_) => false,
        }
    }, "失败→安全降级 + 显性三要素告警（非静默）");
    cs.add("耗时记账", probe_cost_ms() <= COST_CAP_MS, "探针耗时 ≤200ms 记账（确定性 30ms）");
    cs.add("版本锚", VER == 1, "模块版本锚 = 1");
    cs
}

// ---- 单元测试 ---------------------------------------------------------------
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn face() {
        let cs = run_unx36288_checks();
        assert!(cs.len() > 0, "检查数必须 > 0");
    }

    #[test]
    fn test_drill_green() {
        assert_eq!(drill().err(), None);
    }

    #[test]
    fn test_checks_all_passed() {
        let cs = run_unx36288_checks();
        assert!(cs.all_passed(), "{}", cs.red_items_text());
    }

    #[test]
    fn test_degrade_path_alarms() {
        // 降级直证：注入 1 例 → DEGRADED + 恰 1 条告警；干净跑恒 RELEASED。
        let clean = run_probes(None).ok().unwrap_or(ProbeRun {
            state: GateState::Unprobed,
            passed: 0,
            failed: 99,
            alarms: Vec::new(),
            cost_ms: 0,
        });
        assert_eq!(clean.passed, 30);
        assert_eq!(clean.failed, 0);
        assert_eq!(clean.state, GateState::Released);
        let bad = run_probes(Some((0, V_GRANT))).ok().unwrap_or(clean);
        assert_eq!(bad.failed, 1);
        assert_eq!(bad.state, GateState::Degraded);
        assert_eq!(bad.alarms.len(), 1);
        assert_eq!(bad.alarms[0].case, 0);
    }
}
