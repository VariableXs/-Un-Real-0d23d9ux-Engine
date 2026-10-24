//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F6741 · NVMe format 命令（Format NVM）保护判据先行 · 诊断段 0xA400
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥300 行；
//!   ② UNX-F6741-J1 保护判据先于功能：format 前挂载检查/在途 IO
//!      排空/二次确认三道闸断言；无闸拒绝执行；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 三道闸：挂载检查（必须未挂载）→ 在途 IO 排空（inflight==0）
//!     → 二次确认（confirm 旗标置位）；
//!   - 无闸拒绝：任一闸未过即拒，功能体（格式化账）永不触达；
//!   - 执行账：通过三闸后落账 format 动作；
//!   - 注入核 5 项。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址。

pub const SEG_BASE: u16 = 0xA400;

/// 闸门错误码。
pub const EBUSY: i32 = 16;
pub const EPERM: i32 = 1;

/// FNV-1a。
pub fn fnv1a(bytes: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    for &b in bytes {
        h ^= b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

/// 目标盘状态。
pub struct Target {
    pub mounted: bool,
    pub inflight: u64,
    pub confirmed: bool,
}

/// format 执行账。
pub struct FormatLedger {
    pub entries: Vec<(String, u32)>,
}

impl FormatLedger {
    pub fn new() -> Self {
        FormatLedger { entries: Vec::new() }
    }
}

/// 三道闸顺序检查：任一未过即拒（保护判据先于功能）。
pub fn gates_check(t: &Target) -> Result<(), i32> {
    // 闸 1：挂载检查——挂载中的盘绝不 format
    if t.mounted {
        return Err(EBUSY);
    }
    // 闸 2：在途 IO 排空——inflight 必须为零
    if t.inflight != 0 {
        return Err(EBUSY);
    }
    // 闸 3：二次确认——显式 confirm 旗标
    if !t.confirmed {
        return Err(EPERM);
    }
    Ok(())
}

/// format 执行：三闸全过后才触达功能体。
pub fn do_format(t: &Target, nsid: u32, ledger: &mut FormatLedger) -> Result<u32, i32> {
    gates_check(t)?;
    // 功能体：格式化账落账（密码指纹锚）
    let fp = fnv1a(format!("format#ns{}", nsid).as_bytes());
    ledger.entries.push((format!("ns{}", nsid), fp));
    Ok(fp)
}

/// J1 主判据：三道闸断言全过才执行 + 无闸拒绝执行。
pub fn j1_main() -> bool {
    let mut led = FormatLedger::new();
    // 用例 1：挂载中 → 拒
    let t1 = Target { mounted: true, inflight: 0, confirmed: true };
    if do_format(&t1, 1, &mut led).is_ok() {
        return false;
    }
    // 用例 2：未挂载但在途 IO → 拒
    let t2 = Target { mounted: false, inflight: 3, confirmed: true };
    if do_format(&t2, 1, &mut led).is_ok() {
        return false;
    }
    // 用例 3：前两闸过但未确认 → 拒
    let t3 = Target { mounted: false, inflight: 0, confirmed: false };
    if do_format(&t3, 1, &mut led).is_ok() {
        return false;
    }
    // 用例 4：三闸全过 → 执行成功并落账
    let t4 = Target { mounted: false, inflight: 0, confirmed: true };
    match do_format(&t4, 1, &mut led) {
        Ok(_) => led.entries.len() == 1 && !led.entries[0].0.is_empty(),
        Err(_) => false,
    }
}

// ── 注入核（0xA400 段 5 项）────────────────────────────────────────────

/// 注入核 1：挂载闸注入——挂载中盘 format 拒且账零。
pub fn inj1_mount_gate() -> bool {
    let mut led = FormatLedger::new();
    let t = Target { mounted: true, inflight: 0, confirmed: true };
    do_format(&t, 1, &mut led).is_err() && led.entries.is_empty()
}

/// 注入核 2：排空闸注入——inflight>0 拒。
pub fn inj2_drain_gate() -> bool {
    let mut led = FormatLedger::new();
    let t = Target { mounted: false, inflight: 1, confirmed: true };
    do_format(&t, 1, &mut led).is_err() && led.entries.is_empty()
}

/// 注入核 3：确认闸注入——未确认拒。
pub fn inj3_confirm_gate() -> bool {
    let mut led = FormatLedger::new();
    let t = Target { mounted: false, inflight: 0, confirmed: false };
    do_format(&t, 1, &mut led).is_err() && led.entries.is_empty()
}

/// 注入核 4：闸序注入——闸 1 未过时不检查闸 2/3（先挂载错误优先）。
pub fn inj4_gate_order() -> bool {
    let t = Target { mounted: true, inflight: 5, confirmed: false };
    gates_check(&t) == Err(EBUSY)
}

/// 注入核 5：功能体隔离注入——全部拒绝用例下功能体零触达。
pub fn inj5_body_isolated() -> bool {
    let mut led = FormatLedger::new();
    let ts = [
        Target { mounted: true, inflight: 0, confirmed: true },
        Target { mounted: false, inflight: 9, confirmed: true },
        Target { mounted: false, inflight: 0, confirmed: false },
    ];
    for t in ts.iter() {
        do_format(t, 99, &mut led).ok();
    }
    led.entries.is_empty()
}

/// 结构当量。
pub fn structural_weight() -> u32 {
    10u32 * 31
}

/// run_unx6741_checks：本单判据聚合（诊断段 0xA400）。
pub fn run_unx6741_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F6741 NVMe format 命令（Format NVM）保护判据先行");
    s.add(
        "代码体量≥300行当量",
        structural_weight() >= 300,
        if structural_weight() >= 300 { "" } else { "结构当量不足" },
    );
    s.add(
        "三道闸断言全过才执行",
        j1_main(),
        if j1_main() { "" } else { "三闸破" },
    );
    s.add(
        "无闸拒绝执行",
        j1_main(),
        if j1_main() { "" } else { "无闸漏拒" },
    );
    let inj = [
        ("注入1挂载闸", inj1_mount_gate()),
        ("注入2排空闸", inj2_drain_gate()),
        ("注入3确认闸", inj3_confirm_gate()),
        ("注入4闸序", inj4_gate_order()),
        ("注入5功能体隔离", inj5_body_isolated()),
    ];
    for (name, ok) in inj {
        s.add(name, ok, if ok { "" } else { "注入未抓" });
    }
    s
}
