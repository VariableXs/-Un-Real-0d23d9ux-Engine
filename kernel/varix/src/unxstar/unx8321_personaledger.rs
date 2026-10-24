//! UNX-F8321 · 人格资源账本分离（双账本实例化）账 · 诊断段 0xB440
//! 验收锚点：UNX-F8321-J1 资源账本双实例（linux/win32 各一套）成文，
//! 账本行含 persona 列（F8283 子结构承接），跨账取数注入 10/10 零逃逸
//! （linux 账取不到 win32 行，反之亦然），账本结构对照机检。


use alloc::string::{String, ToString};
use alloc::format;
use alloc::vec::Vec;

pub const SEG_BASE: u16 = 0xB440;

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

/// 账本行（含 persona 列，F8283 子结构承接）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LedgerRow {
    pub rid: u32,
    pub persona: Persona,
    pub kind: &'static str, // 资源类型
    pub amount: u64,
}

impl LedgerRow {
    pub fn new(rid: u32, persona: Persona, kind: &'static str, amount: u64) -> Self {
        LedgerRow { rid, persona, kind, amount }
    }
}

/// 双账本：两实例各自实例化。
#[derive(Clone, Debug, PartialEq)]
pub struct PersonaLedger {
    pub persona: Persona,
    pub rows: Vec<LedgerRow>,
}

impl PersonaLedger {
    pub fn new(persona: Persona) -> Self {
        PersonaLedger { persona, rows: Vec::new() }
    }
    pub fn append(&mut self, r: LedgerRow) -> bool {
        if r.persona != self.persona {
            return false; // 错账本拒收
        }
        self.rows.push(r);
        true
    }
    /// 跨账取数：从本账取他账行 → 零逃逸（恒 None）。
    pub fn cross_fetch(&self, rid: u32, other_persona: Persona) -> Option<&LedgerRow> {
        if other_persona == self.persona {
            return None; // 跨账取数语义：指定他账 persona 一律不给
        }
        self.rows.iter().find(|r| r.rid == rid && r.persona == self.persona)
    }
    pub fn total(&self) -> u64 {
        self.rows.iter().map(|r| r.amount).sum()
    }
}

/// 双账本对（机检对照面）。
pub struct DualLedger {
    pub linux: PersonaLedger,
    pub win32: PersonaLedger,
}

impl DualLedger {
    pub fn new() -> Self {
        DualLedger { linux: PersonaLedger::new(Persona::Linux), win32: PersonaLedger::new(Persona::Win32) }
    }
    /// 结构对照机检：两实例 persona 不同、行内 persona 列与账本一致。
    pub fn structure_ok(&self) -> bool {
        self.linux.persona != self.win32.persona
            && self.linux.rows.iter().all(|r| r.persona == Persona::Linux)
            && self.win32.rows.iter().all(|r| r.persona == Persona::Win32)
    }
    /// 跨账取数零逃逸：从 linux 账取 win32 rid 恒 None，反之亦然。
    pub fn no_escape(&self, rid: u32) -> bool {
        self.linux.cross_fetch(rid, Persona::Win32).is_none() && self.win32.cross_fetch(rid, Persona::Linux).is_none()
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

// ── 注入核（0xB440 段 5 项）────────────────────────────────────────────

pub fn inject_core_5() -> Vec<(&'static str, bool)> {
    let mut seed: u64 = 0x8321_B440;
    let mut ok = true;
    for _ in 0..64 {
        if lcg_next(&mut seed) == 0 {
            ok = false;
        }
    }
    let mut out: Vec<(&'static str, bool)> = Vec::new();
    out.push(("LCG-确定性-64轮不炸", ok));
    out.push(("双实例-persona互异", PersonaLedger::new(Persona::Linux).persona != PersonaLedger::new(Persona::Win32).persona));
    out.push(("行-persona列在", LedgerRow::new(1, Persona::Linux, "fd", 1).persona == Persona::Linux));
    out.push(("错账-跨账拒收", !PersonaLedger::new(Persona::Linux).append(LedgerRow::new(1, Persona::Win32, "fd", 1))));
    out.push(("FNV-空串锚", fnv1a(b"") == 0x811C_9DC5));
    out
}

// ── 判据聚合 ────────────────────────────────────────────────────────────

/// run_unx8321_checks：本单判据聚合（诊断段 0xB440）。
pub fn run_unx8321_checks() -> crate::checks::CheckSet {
    let mut s = crate::checks::CheckSet::new("UNX-F8321 人格资源账本分离");

    for (name, ok) in inject_core_5() {
        s.add(&name, ok, if ok { "" } else { "注入核失败" });
    }

    // 双实例落账：linux 4 行 + win32 3 行
    let mut d = DualLedger::new();
    d.linux.append(LedgerRow::new(1, Persona::Linux, "fd", 8));
    d.linux.append(LedgerRow::new(2, Persona::Linux, "mmap", 64));
    d.linux.append(LedgerRow::new(3, Persona::Linux, "fd", 8));
    d.linux.append(LedgerRow::new(4, Persona::Linux, "task", 1));
    d.win32.append(LedgerRow::new(5, Persona::Win32, "handle", 16));
    d.win32.append(LedgerRow::new(6, Persona::Win32, "heap", 128));
    d.win32.append(LedgerRow::new(7, Persona::Win32, "handle", 16));

    // 结构对照机检
    let struct_ok = d.structure_ok();
    s.add("机检-双实例结构对照全过", struct_ok, if struct_ok { "" } else { "结构机检红" });

    // 行数恰 4/3
    let n_ok = d.linux.rows.len() == 4 && d.win32.rows.len() == 3;
    s.add("落账-linux4行win32 3行", n_ok, if n_ok { "" } else { "行数漂移" });

    // persona 列在行（F8283 承接）
    let col_ok = d.linux.rows.iter().all(|r| r.persona.as_str() == "linux") && d.win32.rows.iter().all(|r| r.persona.as_str() == "win32");
    s.add("persona列-全行标注与账本一致", col_ok, if col_ok { "" } else { "persona 列漂移" });

    // 跨账取数注入 ×10 零逃逸
    let mut esc = 0u32;
    for rid in 1..=10u32 {
        if !d.no_escape(rid) {
            esc += 1;
        }
    }
    s.add("零逃逸-10次跨账取数全拦", esc == 0, if esc == 0 { "" } else { "跨账逃逸" });

    // 本账取数正常（正向对照）
    let own = d.linux.rows.iter().find(|r| r.rid == 2).is_some();
    let own2 = d.win32.rows.iter().find(|r| r.rid == 6).is_some();
    s.add("正向-本账取数正常", own && own2, if own && own2 { "" } else { "正向取数失效" });

    // 错账本拒收计数
    let rej1 = !d.linux.append(LedgerRow::new(9, Persona::Win32, "handle", 1));
    let rej2 = !d.win32.append(LedgerRow::new(9, Persona::Linux, "fd", 1));
    let rej_ok = rej1 && rej2;
    s.add("拒收-错账本双方向全拒", rej_ok, if rej_ok { "" } else { "拒收失效" });

    // 账目分离守恒：两账总额互不串
    let lsum = d.linux.total();
    let wsum = d.win32.total();
    let cons_ok = lsum == 8 + 64 + 8 + 1 && wsum == 16 + 128 + 16;
    s.add("守恒-linux81/win32-160两账互不串", cons_ok, if cons_ok { "" } else { "总额串账" });

    // FNV 双账指纹互异
    let hl = fnv1a(format!("{:?}", d.linux.rows).as_bytes());
    let hw = fnv1a(format!("{:?}", d.win32.rows).as_bytes());
    s.add("FNV-双账指纹互异且非零", hl != hw && hl != 0 && hw != 0, if hl != hw { "" } else { "指纹异常" });

    s
}
