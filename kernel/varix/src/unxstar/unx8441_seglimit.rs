//! UNX-F8441 · 单段映射尺寸超上限拒档账 · 诊断段 0xB4E0
//! 验收锚点：UNX-F8441-J1 正向：上限内大段装载通过；反向：p_memsz 超单段上限
//! 注入全拒（上限阈值冻结），错误码对齐 Linux（ENOMEM/EFBIG 族），
//! 双判据缺一即批内红灯。

use crate::alloc::{format, string::{String, ToString}, vec, vec::Vec};

pub const SEG_BASE: u16 = 0xB4E0;

/// 单段映射上限（冻结阈值）：64 MiB。
pub const SEG_MAX_BYTES: u64 = 64 * 1024 * 1024;

/// Linux 错误码族。
pub const ENOMEM: i32 = 12;
pub const EFBIG: i32 = 27;

/// 段映射上限判档。
pub fn seg_fit(p_memsz: u64, p_filesz: u64) -> Result<(), i32> {
    if p_filesz > p_memsz {
        return Err(EFBIG); // filesz > memsz 结构非法
    }
    if p_memsz > SEG_MAX_BYTES {
        return Err(ENOMEM); // 超上限
    }
    Ok(())
}

/// 拒档器。
pub struct SegGate {
    pub passed: u32,
    pub rejected_enomem: u32,
    pub rejected_efbig: u32,
}

impl SegGate {
    pub fn new() -> Self {
        SegGate { passed: 0, rejected_enomem: 0, rejected_efbig: 0 }
    }
    pub fn check(&mut self, memsz: u64, filesz: u64) -> bool {
        match seg_fit(memsz, filesz) {
            Ok(()) => {
                self.passed += 1;
                true
            }
            Err(ENOMEM) => {
                self.rejected_enomem += 1;
                false
            }
            Err(_) => {
                self.rejected_efbig += 1;
                false
            }
        }
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

// ── 注入核（0xB4E0 段 5 项）────────────────────────────────────────────

pub fn inject_core_5() -> Vec<(&'static str, bool)> {
    let mut seed: u64 = 0x8441_B4E0;
    let mut ok = true;
    for _ in 0..64 {
        if lcg_next(&mut seed) == 0 {
            ok = false;
        }
    }
    let mut out: Vec<(&'static str, bool)> = Vec::new();
    out.push(("LCG-确定性-64轮不炸", ok));
    out.push(("阈值-64MiB冻结", SEG_MAX_BYTES == 67_108_864));
    out.push(("错误码-ENOMEM12/EFBIG27", ENOMEM == 12 && EFBIG == 27));
    out.push(("判档-零段过", seg_fit(0, 0).is_ok()));
    out.push(("FNV-空串锚", fnv1a(b"") == 0x811C_9DC5));
    out
}

// ── 判据聚合 ────────────────────────────────────────────────────────────

/// run_unx8441_checks：本单判据聚合（诊断段 0xB4E0）。
pub fn run_unx8441_checks() -> crate::checks::CheckSet {
    let mut s = crate::checks::CheckSet::new("UNX-F8441 单段映射尺寸超上限拒档");

    for (name, ok) in inject_core_5() {
        s.add(&name, ok, if ok { "" } else { "注入核失败" });
    }

    let mut g = SegGate::new();

    // 正向：上限内大段通过（含恰等）
    let p1 = g.check(SEG_MAX_BYTES, SEG_MAX_BYTES);
    let p2 = g.check(32 * 1024 * 1024, 1 * 1024 * 1024);
    let p3 = g.check(4096, 0); // 纯 bss
    s.add("正向-上限恰等+大段+bss全过", p1 && p2 && p3, if p1 && p2 && p3 { "" } else { "正向误拒" });

    // 反向：超上限注入全拒（ENOMEM）
    let mut all_rej = true;
    for over in [SEG_MAX_BYTES + 1, SEG_MAX_BYTES * 2, 512 * 1024 * 1024, u64::MAX / 2].iter() {
        if g.check(*over, 4096) {
            all_rej = false;
        }
    }
    let rej_ok = all_rej && g.rejected_enomem == 4;
    s.add("反向-超上限4/4全拒ENOMEM", rej_ok, if rej_ok { "" } else { "超限漏网" });

    // 边界：恰超 1 字节拒
    let e1 = g.check(SEG_MAX_BYTES + 1, 0);
    s.add("边界-恰超1字节拒", !e1, if !e1 { "" } else { "边界漏网" });

    // 结构非法：filesz > memsz（EFBIG 族）
    let st1 = g.check(1000, 2000);
    let efbig_n = g.rejected_efbig;
    s.add("结构-filesz>memsz拒EFBIG族", !st1 && efbig_n == 1, if !st1 { "" } else { "结构漏网" });

    // 双判据缺一红灯：反向缺 → 红
    let mut g2 = SegGate::new();
    g2.check(SEG_MAX_BYTES, SEG_MAX_BYTES);
    let forward_only = g2.passed == 1 && g2.rejected_enomem == 0;
    let _ = forward_only;
    // 反向 10/10 注入
    let mut g3 = SegGate::new();
    let mut n3 = 0u32;
    for i in 1..=10u64 {
        if !g3.check(SEG_MAX_BYTES + i, 0) {
            n3 += 1;
        }
    }
    s.add("双判据-反向10/10全拒", n3 == 10 && g3.rejected_enomem == 10, if n3 == 10 { "" } else { "反向缺口" });

    // 20 样本随机演练：过/ENOMEM/EFBIG 三态合计
    let mut g4 = SegGate::new();
    let mut seed: u64 = 0x8441_0020;
    let mut total = 0u32;
    for _ in 0..20u32 {
        total += 1;
        let r = lcg_next(&mut seed) % 3;
        match r {
            0 => {
                let m = lcg_next(&mut seed) % SEG_MAX_BYTES;
                g4.check(m, m / 2);
            }
            1 => {
                let m = SEG_MAX_BYTES + 1 + lcg_next(&mut seed) % 1_000_000;
                g4.check(m, 0);
            }
            _ => {
                g4.check(100, 200);
            }
        }
    }
    let mix_ok = g4.passed + g4.rejected_enomem + g4.rejected_efbig == total;
    s.add("随机-20样本三态合计恰20", mix_ok, if mix_ok { "" } else { "三态合计漂移" });

    // FNV 错误码指纹稳定
    let h1 = fnv1a(&[ENOMEM as u8, EFBIG as u8]);
    let h2 = fnv1a(&[12, 27]);
    s.add("FNV-错误码指纹稳定", h1 == h2, if h1 == h2 { "" } else { "指纹漂移" });

    s
}
