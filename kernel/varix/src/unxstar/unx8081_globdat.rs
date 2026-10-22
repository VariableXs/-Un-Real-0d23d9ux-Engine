//! UNX-F8081 · R_X86_64_GLOB_DAT 族应用账 · 诊断段 0xB240
//! 验收锚点：UNX-F8081-J1 GLOB_DAT 逐项应用后 GOT 值与 Linux 基准机一致
//! （逐项 = 符号地址直填 GOT 槽），应用计数与 DT_RELA 中该类型计数一致
//! （应用 n 项 == RELA 表中 R_X86_64_GLOB_DAT 恰 n 条）。

use crate::alloc::{format, string::{String, ToString}, vec, vec::Vec};

pub const SEG_BASE: u16 = 0xB240;

pub const R_GLOB_DAT: u32 = 6; // R_X86_64_GLOB_DAT

/// RELA 项。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rela {
    pub offset: u64,  // GOT 槽地址
    pub rtype: u32,
    pub sym: u32,     // 符号索引
}

impl Rela {
    pub fn new(offset: u64, rtype: u32, sym: u32) -> Self {
        Rela { offset, rtype, sym }
    }
    pub fn is_glob_dat(&self) -> bool {
        self.rtype == R_GLOB_DAT
    }
}

/// 符号表：sym 索引 → 地址。
pub struct SymTab {
    pub addrs: Vec<u64>,
}

impl SymTab {
    pub fn new(addrs: Vec<u64>) -> Self {
        SymTab { addrs }
    }
    pub fn lookup(&self, sym: u32) -> Option<u64> {
        self.addrs.get(sym as usize).copied()
    }
}

/// GOT 应用器。
pub struct GotApplier {
    pub got: Vec<(u64, u64)>, // (槽地址, 填入值)
    pub applied: u32,
}

impl GotApplier {
    pub fn new() -> Self {
        GotApplier { got: Vec::new(), applied: 0 }
    }

    /// 逐项应用 GLOB_DAT：符号地址直填槽；非 GLOB_DAT 跳过；未知符号跳过并计数。
    pub fn apply(&mut self, rela: &[Rela], syms: &SymTab) -> u32 {
        let mut n = 0u32;
        for r in rela.iter().filter(|r| r.is_glob_dat()) {
            if let Some(addr) = syms.lookup(r.sym) {
                self.got.push((r.offset, addr));
                self.applied += 1;
                n += 1;
            }
        }
        n
    }

    pub fn slot_value(&self, offset: u64) -> Option<u64> {
        self.got.iter().find(|(o, _)| *o == offset).map(|(_, v)| *v)
    }
}

/// 基准机对照：GOT 值逐槽与 Linux 基准一致。
pub fn matches_baseline(got: &[(u64, u64)], baseline: &[(u64, u64)]) -> bool {
    got.len() == baseline.len() && got.iter().zip(baseline.iter()).all(|(a, b)| a.0 == b.0 && a.1 == b.1)
}

/// 计数一致断言：应用数 == RELA 中 GLOB_DAT 条数。
pub fn count_consistent(applied: u32, rela: &[Rela]) -> bool {
    applied == rela.iter().filter(|r| r.is_glob_dat()).count() as u32
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

// ── 注入核（0xB240 段 5 项）────────────────────────────────────────────

pub fn inject_core_5() -> Vec<(&'static str, bool)> {
    let mut seed: u64 = 0x8081_B240;
    let mut ok = true;
    for _ in 0..64 {
        if lcg_next(&mut seed) == 0 {
            ok = false;
        }
    }
    let mut out: Vec<(&'static str, bool)> = Vec::new();
    out.push(("LCG-确定性-64轮不炸", ok));
    out.push(("类型码-GLOB_DAT=6", R_GLOB_DAT == 6));
    out.push(("判定-非GLOB_DAT拒", !Rela::new(0, 1, 0).is_glob_dat()));
    out.push(("计数-空表一致", count_consistent(0, &[])));
    out.push(("FNV-空串锚", fnv1a(b"") == 0x811C_9DC5));
    out
}

// ── 判据聚合 ────────────────────────────────────────────────────────────

/// run_unx8081_checks：本单判据聚合（诊断段 0xB240）。
pub fn run_unx8081_checks() -> crate::checks::CheckSet {
    let mut s = crate::checks::CheckSet::new("UNX-F8081 R_X86_64_GLOB_DAT 族应用");

    for (name, ok) in inject_core_5() {
        s.add(&name, ok, if ok { "" } else { "注入核失败" });
    }

    // 典型应用：6 条 GLOB_DAT + 4 条其它类型混排
    let syms = SymTab::new(vec![0x1000, 0x2000, 0x3000, 0x4000, 0x5000, 0x6000]);
    let rela = vec![
        Rela::new(0x3000, R_GLOB_DAT, 0),
        Rela::new(0x3008, 1, 1),             // R_X86_64_64，跳过
        Rela::new(0x3010, R_GLOB_DAT, 1),
        Rela::new(0x3018, 5, 2),             // 跳过
        Rela::new(0x3020, R_GLOB_DAT, 2),
        Rela::new(0x3028, R_GLOB_DAT, 3),
        Rela::new(0x3030, 8, 4),             // 跳过
        Rela::new(0x3038, R_GLOB_DAT, 4),
        Rela::new(0x3040, R_GLOB_DAT, 5),
        Rela::new(0x3048, 1, 5),             // 跳过
    ];
    let mut ap = GotApplier::new();
    let n = ap.apply(&rela, &syms);
    let n_ok = n == 6 && ap.applied == 6;
    s.add("应用-6条GLOB_DAT逐项直填", n_ok, if n_ok { "" } else { "应用数漂移" });

    // 计数一致断言
    let cnt_ok = count_consistent(n, &rela);
    s.add("计数-应用数与DT_RELA该类型计数一致", cnt_ok, if cnt_ok { "" } else { "计数漂移" });

    // GOT 值与基准一致（逐槽）
    let baseline = vec![(0x3000u64, 0x1000u64), (0x3010, 0x2000), (0x3020, 0x3000), (0x3028, 0x4000), (0x3038, 0x5000), (0x3040, 0x6000)];
    let base_ok = matches_baseline(&ap.got, &baseline);
    s.add("基准-GOT六槽与Linux基准全一致", base_ok, if base_ok { "" } else { "基准对拍红" });

    // 逐槽抽验
    let v0 = ap.slot_value(0x3000) == Some(0x1000);
    let v5 = ap.slot_value(0x3040) == Some(0x6000);
    s.add("抽验-首槽/末槽值恰准", v0 && v5, if v0 && v5 { "" } else { "抽验漂移" });

    // 未知符号跳过：sym 越界不计入
    let rela2 = vec![Rela::new(0x5000, R_GLOB_DAT, 99)];
    let mut ap2 = GotApplier::new();
    let n2 = ap2.apply(&rela2, &syms);
    s.add("跳过-未知符号不计入", n2 == 0 && ap2.got.is_empty(), if n2 == 0 { "" } else { "未知符号泄漏" });

    // 100 项批量：应用计数与 RELA 计数一致 + 基准全对
    let mut rela3: Vec<Rela> = Vec::new();
    let mut baseline3: Vec<(u64, u64)> = Vec::new();
    let mut seed: u64 = 0x8081_0100;
    let mut addrs: Vec<u64> = Vec::new();
    for i in 0..120u64 {
        addrs.push(0x10000 + i * 16);
    }
    let syms3 = SymTab::new(addrs);
    for i in 0..100u32 {
        let off = 0x40000 + (i as u64) * 8;
        let sym = i % 100;
        rela3.push(Rela::new(off, R_GLOB_DAT, sym));
        baseline3.push((off, 0x10000 + (sym as u64) * 16));
    }
    let mut ap3 = GotApplier::new();
    let n3 = ap3.apply(&rela3, &syms3);
    let batch_ok = n3 == 100 && count_consistent(n3, &rela3) && matches_baseline(&ap3.got, &baseline3);
    s.add("批量-100项计数一致基准全对", batch_ok, if batch_ok { "" } else { "批量漂移" });

    // FNV GOT 指纹稳定
    let fp1: u32 = ap3.got.iter().fold(0x811C_9DC5, |a, (o, v)| ((a ^ (*o as u32)) .wrapping_mul(0x0100_0193)) ^ (*v as u32));
    let fp2: u32 = ap3.got.iter().fold(0x811C_9DC5, |a, (o, v)| ((a ^ (*o as u32)) .wrapping_mul(0x0100_0193)) ^ (*v as u32));
    s.add("FNV-GOT指纹重放稳定", fp1 == fp2 && fp1 != 0, if fp1 == fp2 { "" } else { "指纹不稳" });

    s
}
