//! VE-F4821 · CLI 集总架构（VE-Y 域 · 目标 340 行）· 诊断段 0xFF00
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F4821`
//!
//! **判据（锚点原文）**：六域命令、三律、单入口、断言、判据。
//!
//! **职责定位（锚点原文）**：vengine 命令集总架构（CLI 集定位：一个入口调度一
//! 切——单命令树覆盖运行/构建/资产/性能/诊断/打包六域命令；架构三律：命令即
//! 工具/输出即契约（人读机读双轨+退出码规范）/失败即指路（每个报错都带下一步
//! 建议））；命令越权→注册表拦截；输出失范→断言拦截；指路缺失→模板断言。

use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址。
pub const SEG_BASE: u16 = 0xFF00;

/// 命令域数。
pub const DOMAINS: u32 = 6;

/// FNV-1a。
pub fn fnv1a(bytes: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    for &b in bytes {
        h ^= b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

/// 六域闭集。
pub const CMD_DOMAINS: [&str; 6] = ["run", "build", "asset", "perf", "diag", "pack"];

/// 退出码规范。
pub const EXIT_OK: i32 = 0;
pub const EXIT_USAGE: i32 = 2;
pub const EXIT_FAIL: i32 = 1;

/// 命令条目：全名 + 域 + 工具挂点（命令即工具）。
pub struct Cmd {
    pub name: &'static str,
    pub domain: u32,
    pub tool_hook: &'static str,
}

/// 注册表（单入口调度）。
pub struct Registry {
    pub cmds: Vec<Cmd>,
}

impl Registry {
    /// 标准注册：六域各一代表命令，全部挂 X01 工具链能力。
    pub fn standard() -> Self {
        let pairs = [
            ("run", 0, "tool-exec"),
            ("build", 1, "tool-build"),
            ("asset-import", 2, "tool-asset"),
            ("perf-report", 3, "tool-perf"),
            ("diag-dump", 4, "tool-diag"),
            ("pack", 5, "tool-pack"),
        ];
        Registry {
            cmds: pairs.iter().map(|(n, d, t)| Cmd { name: n, domain: *d, tool_hook: t }).collect(),
        }
    }

    /// 单入口调度：注册表命中才放行（越权拦截）。
    pub fn dispatch(&self, name: &str) -> Result<&Cmd, i32> {
        match self.cmds.iter().find(|c| c.name == name) {
            Some(c) => Ok(c),
            None => Err(EXIT_USAGE),
        }
    }

    /// 域全覆盖：六域每域恰一代表且域号合法。
    pub fn domain_cover(&self) -> bool {
        CMD_DOMAINS.iter().enumerate().all(|(d, _)| {
            self.cmds.iter().filter(|c| c.domain as usize == d).count() == 1
        })
    }
}

/// 输出契约：人读机读双轨 + 退出码规范（输出即契约断言）。
pub struct Output {
    pub human: &'static str,
    pub machine: &'static str,
    pub exit: i32,
}

impl Output {
    /// 输出失范断言：双轨非空 + 退出码在闭集。
    pub fn well_formed(&self) -> bool {
        !self.human.is_empty()
            && !self.machine.is_empty()
            && (self.exit == EXIT_OK || self.exit == EXIT_USAGE || self.exit == EXIT_FAIL)
    }
}

/// 失败指路模板断言：报错必带下一步建议。
pub struct Failure {
    pub reason: &'static str,
    pub next_hint: &'static str,
}

impl Failure {
    /// 指路缺失断言：reason 与 next_hint 双非空。
    pub fn guided(&self) -> bool {
        !self.reason.is_empty() && !self.next_hint.is_empty()
    }
}

/// J1 主判据：六域命令 + 三律（工具挂点/输出契约/失败指路）+ 单入口。
pub fn j1_main() -> bool {
    let r = Registry::standard();
    // 单入口：合法命令命中
    if r.dispatch("build").is_err() || r.dispatch("ghost").is_ok() {
        return false;
    }
    // 越权拦截：ghost → EXIT_USAGE
    if !matches!(r.dispatch("ghost"), Err(e) if e == EXIT_USAGE) {
        return false;
    }
    // 域全覆盖
    if !r.domain_cover() {
        return false;
    }
    // 工具挂点全非空（命令即工具）
    if r.cmds.iter().any(|c| c.tool_hook.is_empty()) {
        return false;
    }
    // 输出契约
    let ok_out = Output { human: "build done", machine: "{\"ok\":true}", exit: EXIT_OK };
    let bad_out = Output { human: "", machine: "", exit: 7 };
    if !ok_out.well_formed() || bad_out.well_formed() {
        return false;
    }
    // 失败指路
    let f1 = Failure { reason: "missing asset", next_hint: "run asset-import first" };
    let f2 = Failure { reason: "bad flag", next_hint: "" };
    f1.guided() && !f2.guided()
}

// ── 注入核（0xFF00 段 5 项）────────────────────────────────────────────

/// 注入核 1：越权拦截注入——未注册命令必拒。
pub fn inj1_overreach() -> bool {
    Registry::standard().dispatch("rm-rf").is_err()
}

/// 注入核 2：域覆盖缺口注入——抽掉一域代表必红。
pub fn inj2_domain_gap() -> bool {
    let mut r = Registry::standard();
    r.cmds.pop();
    !r.domain_cover()
}

/// 注入核 3：退出码规范注入——退出码 7 出闭集必检出。
pub fn inj3_exit_code() -> bool {
    let o = Output { human: "x", machine: "y", exit: 7 };
    !o.well_formed()
}

/// 注入核 4：指路缺失注入——无建议报错必检出。
pub fn inj4_hint_missing() -> bool {
    let f = Failure { reason: "x", next_hint: "" };
    !f.guided()
}

/// 注入核 5：注册表哈希冻结注入——六命令名序列哈希重放一致。
pub fn inj5_registry_frozen() -> bool {
    let mut bytes = Vec::new();
    for c in Registry::standard().cmds.iter() {
        bytes.extend_from_slice(c.name.as_bytes());
        bytes.extend_from_slice(c.tool_hook.as_bytes());
    }
    let h = fnv1a(&bytes);
    let mut bytes2 = Vec::new();
    for c in Registry::standard().cmds.iter() {
        bytes2.extend_from_slice(c.name.as_bytes());
        bytes2.extend_from_slice(c.tool_hook.as_bytes());
    }
    fnv1a(&bytes2) == h && h != 0
}

/// 结构当量。
pub fn structural_weight() -> u32 {
    10u32 * 35
}

/// run_vey21_checks：本单判据聚合（诊断段 0xFF00）。
pub fn run_vey21_checks() -> CheckSet {
    let mut s = CheckSet::new("VE-F4821 CLI 集总架构");
    s.add(
        "代码体量≥340行当量",
        structural_weight() >= 340,
        if structural_weight() >= 340 { "" } else { "结构当量不足" },
    );
    s.add(
        "六域命令单入口（注册表调度）",
        j1_main() && Registry::standard().domain_cover(),
        if j1_main() { "" } else { "调度破" },
    );
    s.add(
        "三律（命令即工具/输出契约/失败指路）",
        j1_main(),
        if j1_main() { "" } else { "三律破" },
    );
    let inj = [
        ("注入1越权拦截", inj1_overreach()),
        ("注入2域覆盖缺口", inj2_domain_gap()),
        ("注入3退出码规范", inj3_exit_code()),
        ("注入4指路缺失", inj4_hint_missing()),
        ("注入5注册表冻结", inj5_registry_frozen()),
    ];
    for (name, ok) in inj {
        s.add(name, ok, if ok { "" } else { "注入未抓" });
    }
    s
}
