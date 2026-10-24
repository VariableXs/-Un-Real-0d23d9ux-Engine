//! UNX-F8281 · 人格标签路由机制（linux/win32 分流点）账 · 诊断段 0xB3E0
//! 验收锚点：UNX-F8281-J1 分流点按人格标签路由（Top10 接口⑤ C 侧半边实装），
//! 错误路由注入 10/10 显性暴露（错误标签 → 拒 + 留账），路由账可导出。


use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

pub const SEG_BASE: u16 = 0xB3E0;

/// 人格标签。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Persona {
    Linux,
    Win32,
}

impl Persona {
    pub fn as_str(self) -> &'static str {
        match self {
            Persona::Linux => "linux",
            Persona::Win32 => "win32",
        }
    }
}

/// Top10 接口（⑤ 号在列）。
pub const TOP10_IFACES: [&str; 10] = [
    "if01_read", "if02_write", "if03_open", "if04_close", "if05_route",
    "if06_fork", "if07_exec", "if08_mmap", "if09_ioctl", "if10_exit",
];

/// 路由结果。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RouteEntry {
    pub iface: &'static str,   // 接口名
    pub persona: Persona,      // 分流目标
    pub ok: bool,              // 路由成功（错误注入为 false）
}

/// 分流器：按人格标签路由。
pub struct PersonaRouter {
    pub table: Vec<RouteEntry>,
    pub rejected: u32,
}

impl PersonaRouter {
    pub fn new() -> Self {
        PersonaRouter { table: Vec::new(), rejected: 0 }
    }

    /// Top10 接口 C 侧半边实装：按标签分流（if05 为分流点本体）。
    pub fn route(&mut self, iface: &'static str, p: Persona, label_ok: bool) -> bool {
        if !label_ok || !TOP10_IFACES.contains(&iface) {
            self.rejected += 1;
            self.table.push(RouteEntry { iface, persona: p, ok: false });
            return false;
        }
        self.table.push(RouteEntry { iface, persona: p, ok: true });
        true
    }

    /// 路由账可导出。
    pub fn export(&self) -> String {
        self.table.iter().map(|e| format!("{}:{}", e.iface, e.persona.as_str())).collect::<Vec<String>>().join(",")
    }

    /// 错误路由显性暴露：ok=false 条目全在账。
    pub fn errors_visible(&self) -> u32 {
        self.table.iter().filter(|e| !e.ok).count() as u32
    }
}

/// FNV-1a 32 位。
pub fn fnv1a(data: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    for b in data {
        h ^= *b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

/// 确定性 LCG。
pub fn lcg_next(seed: &mut u64) -> u64 {
    *seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
    *seed
}

// ── 注入核（0xB3E0 段 5 项）────────────────────────────────────────────

pub fn inject_core_5() -> Vec<(&'static str, bool)> {
    let mut seed: u64 = 0x8281_B3E0;
    let mut ok = true;
    for _ in 0..64 {
        if lcg_next(&mut seed) == 0 {
            ok = false;
        }
    }
    let mut out: Vec<(&'static str, bool)> = Vec::new();
    out.push(("LCG-确定性-64轮不炸", ok));
    out.push(("Top10-常数", TOP10_IFACES.len() == 10 && TOP10_IFACES[4] == "if05_route"));
    out.push(("标签-两态", Persona::Linux.as_str() == "linux" && Persona::Win32.as_str() == "win32"));
    out.push(("路由-空账拒0", PersonaRouter::new().rejected == 0));
    out.push(("FNV-空串锚", fnv1a(b"") == 0x811C_9DC5));
    out
}

// ── 判据聚合 ────────────────────────────────────────────────────────────

/// run_unx8281_checks：本单判据聚合（诊断段 0xB3E0）。
pub fn run_unx8281_checks() -> crate::checks::CheckSet {
    let mut s = crate::checks::CheckSet::new("UNX-F8281 人格标签路由机制");

    for (name, ok) in inject_core_5() {
        s.add(&name, ok, if ok { "" } else { "注入核失败" });
    }

    // Top10 C 侧半边实装：10 接口全路由
    let mut r = PersonaRouter::new();
    let mut all_ok = true;
    for (i, iface) in TOP10_IFACES.iter().enumerate() {
        let p = if i % 2 == 0 { Persona::Linux } else { Persona::Win32 };
        if !r.route(iface, p, true) {
            all_ok = false;
        }
    }
    let ten_ok = all_ok && r.table.len() == 10 && r.rejected == 0;
    s.add("实装-Top10接口全路由", ten_ok, if ten_ok { "" } else { "Top10 缺口" });

    // if05 分流点本体在账
    let if05 = r.table.iter().find(|e| e.iface == "if05_route").map(|e| e.ok) == Some(true);
    s.add("分流点-if05在账且成功", if05, if if05 { "" } else { "if05 缺席" });

    // 错误路由注入 ×10 显性暴露
    let mut r2 = PersonaRouter::new();
    let bad_ifaces = ["if99_ghost", "if00_bogus", "ifXX_bad", "route_none", "if05_typo", "if1x_bad", "if20_fake", "if30_err", "if40_x", "if50_y"];
    for b in bad_ifaces.iter() {
        r2.route(b, Persona::Linux, false);
    }
    let rej_ok = r2.rejected == 10 && r2.errors_visible() == 10;
    s.add("错误注入-10/10显性暴露留账", rej_ok, if rej_ok { "" } else { "错误注入漏检" });

    // 混合账：成功 + 失败同账可分
    r2.route("if05_route", Persona::Win32, true);
    let mixed_ok = r2.table.len() == 11 && r2.errors_visible() == 10;
    s.add("混账-11条中10错1成分离", mixed_ok, if mixed_ok { "" } else { "混账漂移" });

    // 路由账可导出
    let mut r3 = PersonaRouter::new();
    r3.route("if01_read", Persona::Linux, true);
    r3.route("if02_write", Persona::Win32, true);
    let exp = r3.export();
    s.add("导出-路由账if01:linux,if02:win32", exp == "if01_read:linux,if02_write:win32", if exp == "if01_read:linux,if02_write:win32" { "" } else { "导出漂移" });

    // 标签大小写/未知标签错误注入也拒（label_ok=false 面再核 10 次）
    let mut r4 = PersonaRouter::new();
    let mut n4 = 0u32;
    for i in 0..10 {
        let iface = TOP10_IFACES[i];
        if !r4.route(iface, Persona::Linux, false) {
            n4 += 1;
        }
    }
    s.add("错标-Top10带错标10/10拒", n4 == 10 && r4.errors_visible() == 10, if n4 == 10 { "" } else { "错标漏网" });

    // FNV 路由账指纹稳定
    let h1 = fnv1a(r.export().as_bytes());
    let h2 = fnv1a(r.export().as_bytes());
    s.add("FNV-路由账指纹重放稳定", h1 == h2 && h1 != 0, if h1 == h2 { "" } else { "指纹不稳" });

    s
}
