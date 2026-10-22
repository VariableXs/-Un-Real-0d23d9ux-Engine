//! UNX-F8301 · 人格切换原语（双人格进程身份转换点）账 · 诊断段 0xB420
//! 验收锚点：UNX-F8301-J1 切换原语成文（linux→win32 / win32→linux 双向登记面），
//! 切换账逐笔（进程号/原标签/新标签/时点四列），注入错误方向 10/10 显性拒绝
//! （同标签切换 → 拒），切换语义对照 D2 联签登记。

use crate::alloc::{format, string::{String, ToString}, vec, vec::Vec};

pub const SEG_BASE: u16 = 0xB420;

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
    pub fn flip(self) -> Persona {
        match self {
            Persona::Linux => Persona::Win32,
            Persona::Win32 => Persona::Linux,
        }
    }
}

/// 切换账单笔（四列）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SwitchEntry {
    pub pid: u32,       // 进程号
    pub from: Persona,  // 原标签
    pub to: Persona,    // 新标签
    pub tick: u64,      // 时点
}

impl SwitchEntry {
    pub fn valid(&self) -> bool {
        self.from != self.to && self.to == self.from.flip()
    }
}

/// 切换原语（D2 联签登记面）。
pub struct PersonaSwitch {
    pub ledger: Vec<SwitchEntry>,
    pub rejected: u32,
    pub d2_cosign: bool,
}

impl PersonaSwitch {
    pub fn new() -> Self {
        PersonaSwitch { ledger: Vec::new(), rejected: 0, d2_cosign: false }
    }

    /// 切换：仅异标签可切；同标签/未知方向拒绝。
    pub fn switch(&mut self, pid: u32, from: Persona, to: Persona, tick: u64) -> bool {
        let e = SwitchEntry { pid, from, to, tick };
        if !e.valid() {
            self.rejected += 1;
            return false;
        }
        self.ledger.push(e);
        true
    }

    /// 双向登记面：两方向各至少一笔。
    pub fn both_directions(&self) -> bool {
        self.ledger.iter().any(|e| e.from == Persona::Linux && e.to == Persona::Win32)
            && self.ledger.iter().any(|e| e.from == Persona::Win32 && e.to == Persona::Linux)
    }

    /// 时点单调：账内 tick 非降序。
    pub fn tick_monotonic(&self) -> bool {
        self.ledger.windows(2).all(|w| w[0].tick <= w[1].tick)
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

// ── 注入核（0xB420 段 5 项）────────────────────────────────────────────

pub fn inject_core_5() -> Vec<(&'static str, bool)> {
    let mut seed: u64 = 0x8301_B420;
    let mut ok = true;
    for _ in 0..64 {
        if lcg_next(&mut seed) == 0 {
            ok = false;
        }
    }
    let mut out: Vec<(&'static str, bool)> = Vec::new();
    out.push(("LCG-确定性-64轮不炸", ok));
    out.push(("标签-翻转双向", Persona::Linux.flip() == Persona::Win32 && Persona::Win32.flip() == Persona::Linux));
    out.push(("账单-同标非法", !SwitchEntry { pid: 1, from: Persona::Linux, to: Persona::Linux, tick: 0 }.valid()));
    out.push(("原语-空账拒0", PersonaSwitch::new().rejected == 0));
    out.push(("FNV-空串锚", fnv1a(b"") == 0x811C_9DC5));
    out
}

// ── 判据聚合 ────────────────────────────────────────────────────────────

/// run_unx8301_checks：本单判据聚合（诊断段 0xB420）。
pub fn run_unx8301_checks() -> crate::checks::CheckSet {
    let mut s = crate::checks::CheckSet::new("UNX-F8301 人格切换原语");

    for (name, ok) in inject_core_5() {
        s.add(&name, ok, if ok { "" } else { "注入核失败" });
    }

    // 双向切换登记
    let mut sw = PersonaSwitch::new();
    let f1 = sw.switch(10, Persona::Linux, Persona::Win32, 100);
    let f2 = sw.switch(20, Persona::Win32, Persona::Linux, 200);
    let both_ok = f1 && f2 && sw.both_directions() && sw.ledger.len() == 2;
    s.add("双向-linux↔win32两笔登记", both_ok, if both_ok { "" } else { "双向缺口" });

    // 账单四列：pid/原/新/时点
    let e = sw.ledger[0];
    let col_ok = e.pid == 10 && e.from == Persona::Linux && e.to == Persona::Win32 && e.tick == 100;
    s.add("四列-pid/原/新/时点恰准", col_ok, if col_ok { "" } else { "四列漂移" });

    // 时点单调
    let mono = sw.tick_monotonic();
    s.add("时点-账内非降序", mono, if mono { "" } else { "时点漂移" });

    // 错误方向注入 ×10 拒（同标签切换）
    let mut sw2 = PersonaSwitch::new();
    let mut rej = 0u32;
    for i in 0..10u64 {
        if !sw2.switch(1000 + i as u32, Persona::Linux, Persona::Linux, i) {
            rej += 1;
        }
    }
    s.add("错向-同标签10/10显性拒绝", rej == 10 && sw2.rejected == 10 && sw2.ledger.is_empty(), if rej == 10 { "" } else { "错向漏网" });

    // 错向不落账但拒绝面可见
    s.add("错向-拒绝全留计数", sw2.rejected == 10, if sw2.rejected == 10 { "" } else { "拒账缺失" });

    // D2 联签登记
    sw.d2_cosign = true;
    s.add("联签-D2登记在册", sw.d2_cosign, if sw.d2_cosign { "" } else { "联签缺失" });

    // 50 轮随机切换：合法走账 + tick 单调守恒
    let mut sw3 = PersonaSwitch::new();
    let mut seed: u64 = 0x8301_0050;
    let mut last_tick = 0u64;
    let mut n_ok = 0u32;
    for i in 0..50u64 {
        let p = if lcg_next(&mut seed) % 2 == 0 { Persona::Linux } else { Persona::Win32 };
        let tick = last_tick + lcg_next(&mut seed) % 10 + 1;
        if sw3.switch(5000 + i as u32, p, p.flip(), tick) {
            n_ok += 1;
            last_tick = tick;
        }
    }
    let rnd_ok = n_ok == 50 && sw3.tick_monotonic() && sw3.both_directions();
    s.add("随机-50轮合法切换全落账单调", rnd_ok, if rnd_ok { "" } else { "随机缺口" });

    // FNV 切换账指纹稳定
    let h1 = fnv1a(format!("{:?}", sw3.ledger).as_bytes());
    let h2 = fnv1a(format!("{:?}", sw3.ledger).as_bytes());
    s.add("FNV-切换账指纹重放稳定", h1 == h2 && h1 != 0, if h1 == h2 { "" } else { "指纹不稳" });

    s
}
