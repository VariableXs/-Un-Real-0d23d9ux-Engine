
// ---------------------------------------------------------------------------
// F019 · 深化批次三：引用计数泄漏检测（进程退出告警）+ 四族方法覆盖登记
// （A2 数据驱动扩面台账）
//
// 主册依据（G-A-19【状态与异常】）：「引用计数泄漏检测 → 进程退出时告警日志」；
// 【设计细节】「四族对象接口方法覆盖以『50 件清单实际调用面』为准（A2 数据
// 驱动扩面）」。既有面：错误码/类工厂/会话回收/蜂巢路径不重复。
// ---------------------------------------------------------------------------

/// 引用计数泄漏审计（进程退出检——告警日志的数据源）。
#[derive(Clone, Copy, Debug)]
pub struct RefLeakAudit {
    /// 退出时仍存活的对象数（泄漏量）。
    pub leaked_objects: u32,
    /// 本退出检是否触发告警（leak > 0 → 告警）。
    pub warned: bool,
}

impl RefLeakAudit {
    pub const fn new() -> RefLeakAudit {
        RefLeakAudit { leaked_objects: 0, warned: false }
    }

    /// 进程退出检：记录存活对象数并裁决是否告警（返回告警与否——日志面消费）。
    pub fn session_exit(&mut self, live_objects: u32) -> bool {
        self.leaked_objects = live_objects;
        self.warned = live_objects > 0;
        self.warned
    }
}

/// 四族对象方法覆盖登记（A2 数据驱动扩面台账——50 件采样口径的当前覆盖，
/// 扩面 = 改表不改逻辑）。
pub const FAMILY_METHOD_COVERAGE: [(&str, u32); 4] = [
    ("shell-link", 12),
    ("drop-source", 6),
    ("clipboard", 9),
    ("app-activation", 5),
];

/// 覆盖总数（诊断页显示口径）。
pub const fn family_coverage_total() -> u32 {
    let mut total = 0u32;
    let mut i = 0;
    while i < FAMILY_METHOD_COVERAGE.len() {
        total += FAMILY_METHOD_COVERAGE[i].1;
        i += 1;
    }
    total
}

/// F019 深化批次三自检。
pub fn run_comloc_deep2_checks() -> CheckSet {
    let mut cs = CheckSet::new("F019-comloc-deep2");
    // 1) 退出检双向：存活 0 → 无告警（干净退出）；存活 3 → 告警 + 泄漏量登记。
    let mut a = RefLeakAudit::new();
    let clean = a.session_exit(0);
    let mut b = RefLeakAudit::new();
    let dirty = b.session_exit(3);
    cs.add(
        "ref_leak_audit_warn_on_exit",
        RefLeakAudit::new().session_exit(0) == false
            && !clean
            && dirty
            && b.leaked_objects == 3
            && a.leaked_objects == 0,
        "",
    );
    // 2) 覆盖登记：四族齐、总数 32（12+6+9+5）、族名非空（台账可读）。
    let mut names_ok = true;
    for (name, _) in FAMILY_METHOD_COVERAGE {
        names_ok &= !name.is_empty();
    }
    cs.add(
        "family_method_coverage_ledger",
        FAMILY_METHOD_COVERAGE.len() == 4 && family_coverage_total() == 32 && names_ok,
        "",
    );
    // 3) 错误码钉值锚（深化不破坏既有判据——REGDB/NOAGGREGATION 原值不变）。
    cs.add(
        "error_codes_pinned",
        REGDB_E_CLASSNOTREG == 0x8004_0154 && CLASS_E_NOAGGREGATION == 0x8004_0110,
        "",
    );
    cs
}
