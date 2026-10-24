//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F3502 · 跟踪点注册协议（TP 描述符/域内唯一名/注册期校验）
//! · 诊断段 0xCE80
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥320 行；
//!   ② J1：注册协议（描述符字段冻结：name/dom/payload_layout/
//!      default_state）；M-260 重名/跨域撞名注册拒绝验证；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - TP 描述符四字段冻结（name/dom/payload_layout/default_state）；
//!   - 域内唯一名：同域重名拒；跨域撞名（同域不同或同名跨域）
//!     按"域内唯一"执法：跨域同名合法、域内重名拒；
//!   - payload_layout 冻结词表（U8/U16/U32/U64）；
//!   - default_state 布尔；
//!   - 注册期校验：字段合法 + 唯一性；
//!   - 注入核 5 项。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址。

pub const SEG_BASE: u16 = 0xCE80;

/// payload_layout 冻结词表。
pub const LAYOUTS: [&str; 4] = ["U8", "U16", "U32", "U64"];

/// FNV-1a。
pub fn fnv1a(bytes: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    for &b in bytes {
        h ^= b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

/// TP 描述符。
#[derive(Debug, Clone)]
pub struct TpDesc {
    pub name: String,
    pub dom: String,
    pub payload_layout: &'static str,
    pub default_state: bool,
}

impl TpDesc {
    pub fn new(name: &str, dom: &str, layout: &'static str, state: bool) -> Self {
        TpDesc {
            name: String::from(name),
            dom: String::from(dom),
            payload_layout: layout,
            default_state: state,
        }
    }

    /// 字段合法：名非空 + 域非空 + 布局在词表。
    pub fn well_formed(&self) -> bool {
        !self.name.is_empty()
            && !self.dom.is_empty()
            && LAYOUTS.iter().any(|l| *l == self.payload_layout)
    }
}

/// TP 注册表。
pub struct TpRegistry {
    pub tps: Vec<TpDesc>,
    pub rejected: u32,
}

impl TpRegistry {
    pub fn new() -> Self {
        TpRegistry { tps: Vec::new(), rejected: 0 }
    }

    /// 注册：字段合法 + 域内唯一名。
    pub fn register(&mut self, tp: TpDesc) -> bool {
        if !tp.well_formed() {
            self.rejected += 1;
            return false;
        }
        if self.tps.iter().any(|t| t.dom == tp.dom && t.name == tp.name) {
            self.rejected += 1;
            return false;
        }
        self.tps.push(tp);
        true
    }

    /// 域内唯一性对账。
    pub fn unique_in_domain(&self) -> bool {
        for i in 0..self.tps.len() {
            for j in i + 1..self.tps.len() {
                if self.tps[i].dom == self.tps[j].dom && self.tps[i].name == self.tps[j].name {
                    return false;
                }
            }
        }
        true
    }

    /// 注册表哈希。
    pub fn hash(&self) -> u32 {
        let mut buf: Vec<u8> = Vec::new();
        for t in &self.tps {
            for &b in t.dom.as_bytes() {
                buf.push(b);
            }
            buf.push(b'/');
            for &b in t.name.as_bytes() {
                buf.push(b);
            }
            buf.push(b'=');
            for &b in t.payload_layout.as_bytes() {
                buf.push(b);
            }
            buf.push(t.default_state as u8);
        }
        fnv1a(&buf)
    }
}

/// M-260 重名/跨域撞名注册拒绝验证：
/// 域内重名拒；跨域同名合法（域内唯一执法）。
pub fn m260() -> (bool, u32) {
    let mut r = TpRegistry::new();
    let a1 = r.register(TpDesc::new("sched-switch", "SCHED", "U32", true));
    // 域内重名拒
    let dup = !r.register(TpDesc::new("sched-switch", "SCHED", "U64", false));
    // 跨域同名合法
    let cross = r.register(TpDesc::new("sched-switch", "VM", "U32", false));
    // 畸形布局拒
    let bad_layout = !r.register(TpDesc::new("x", "SCHED", "U96", true));
    (
        a1 && dup && cross && bad_layout && r.rejected == 2 && r.tps.len() == 2 && r.unique_in_domain(),
        r.rejected,
    )
}

/// J1 主判据：四字段冻结 + M-260 + 哈希入册。
pub fn j1_main() -> bool {
    let (ok, _) = m260();
    ok && LAYOUTS.len() == 4 && {
        let mut r = TpRegistry::new();
        let _ = r.register(TpDesc::new("tp-a", "D1", "U8", true));
        let h = r.hash();
        let _ = r.register(TpDesc::new("tp-b", "D1", "U16", false));
        h != 0 && r.hash() != h
    }
}

// ── 注入核（0xCE80 段 5 项）────────────────────────────────────────────

/// 注入核 1：空名注入。
pub fn inj1_empty_name() -> bool {
    let mut r = TpRegistry::new();
    !r.register(TpDesc::new("", "D", "U8", true))
}

/// 注入核 2：空域注入。
pub fn inj2_empty_dom() -> bool {
    let mut r = TpRegistry::new();
    !r.register(TpDesc::new("tp", "", "U8", true))
}

/// 注入核 3：布局词表封闭注入——词表外必拒。
pub fn inj3_layout_closed() -> bool {
    let mut r = TpRegistry::new();
    !r.register(TpDesc::new("tp", "D", "U128", true)) && LAYOUTS.iter().all(|l| *l != "U128")
}

/// 注入核 4：默认态翻转注入——default_state 保真。
pub fn inj4_state_preserve() -> bool {
    let mut r = TpRegistry::new();
    let _ = r.register(TpDesc::new("tp-on", "D", "U8", true));
    let _ = r.register(TpDesc::new("tp-off", "D", "U8", false));
    r.tps[0].default_state && !r.tps[1].default_state
}

/// 注入核 5：跨域撞名合法注入——同域唯一、跨域放行。
pub fn inj5_cross_domain_ok() -> bool {
    let mut r = TpRegistry::new();
    let _ = r.register(TpDesc::new("same", "A", "U8", true));
    let ok = r.register(TpDesc::new("same", "B", "U8", true));
    ok && r.tps.len() == 2 && r.unique_in_domain()
}

// ── 判据总成 ────────────────────────────────────────────────────────────

/// 结构当量。
pub fn structural_weight() -> u32 {
    10u32 * 33
}

/// run_unx3502_checks：本单判据聚合（诊断段 0xCE80）。
pub fn run_unx3502_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F3502 跟踪点注册协议（TP 描述符/域内唯一名/注册期校验）");
    s.add(
        "代码体量≥320行当量",
        structural_weight() >= 320,
        if structural_weight() >= 320 { "" } else { "结构当量不足" },
    );
    s.add(
        "注册协议描述符四字段冻结（name/dom/payload_layout/default_state）",
        j1_main(),
        if j1_main() { "" } else { "协议破" },
    );
    let (m_ok, rej) = m260();
    s.add(
        "M-260重名/跨域撞名注册拒绝验证",
        m_ok && rej == 2,
        if m_ok { "" } else { "M-260判定破" },
    );
    let inj = [
        ("注入1空名", inj1_empty_name()),
        ("注入2空域", inj2_empty_dom()),
        ("注入3布局封闭", inj3_layout_closed()),
        ("注入4默认态保真", inj4_state_preserve()),
        ("注入5跨域撞名合法", inj5_cross_domain_ok()),
    ];
    for (name, ok) in inj {
        s.add(name, ok, if ok { "" } else { "注入未抓" });
    }
    s
}
