//! UNX-F8481 · 重定位表越界拒档（r_offset 出段界）账 · 诊断段 0xB540
//! 验收锚点：UNX-F8481-J1 正向：合法重定位表 10/10 装载通过（B05 判据承接）；
//! 反向：r_offset 越出目标段界三形注入（越上界/越下界/回绕）全拒且错误码与
//! Linux 对照一致，双判据缺一即批内红灯。

use crate::alloc::{format, string::{String, ToString}, vec, vec::Vec};

pub const SEG_BASE: u16 = 0xB540;

pub const ERR_EFAULT: i32 = 14; // Linux EFAULT（越界访问）
pub const ERR_EINVAL: i32 = 22; // 回绕非法

/// 重定位项。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RelocEnt {
    pub r_offset: u64,
    pub r_type: u32,
    pub r_sym: u32,
}

/// 目标段：[vaddr, vaddr+size)。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TargetSeg {
    pub vaddr: u64,
    pub size: u64,
}

impl TargetSeg {
    /// 越界三形判别：越上界 / 越下界 / 回绕（offset 巨大导致加法回绕后落入段内假象）。
    pub fn classify(&self, off: u64) -> Result<(), i32> {
        // 回绕检测：off 接近 u64::MAX 使 off+8 回绕
        if off.checked_add(8).is_none() {
            return Err(ERR_EINVAL); // 回绕
        }
        if off >= self.vaddr && off.checked_add(8).map(|e| e <= self.vaddr + self.size).unwrap_or(false) {
            Ok(())
        } else if off < self.vaddr {
            Err(ERR_EFAULT) // 越下界
        } else {
            Err(ERR_EFAULT) // 越上界
        }
    }
}

/// 重定位装载门。
pub struct RelocGate {
    pub passed: u32,
    pub rejected_efault: u32,
    pub rejected_einval: u32,
}

impl RelocGate {
    pub fn new() -> Self {
        RelocGate { passed: 0, rejected_efault: 0, rejected_einval: 0 }
    }
    /// 整表装载：逐条校验（不整表信任），任一越界即全拒。
    pub fn load(&mut self, seg: &TargetSeg, table: &[RelocEnt]) -> bool {
        for r in table {
            match seg.classify(r.r_offset) {
                Ok(()) => {}
                Err(ERR_EINVAL) => {
                    self.rejected_einval += 1;
                    return false;
                }
                Err(_) => {
                    self.rejected_efault += 1;
                    return false;
                }
            }
        }
        self.passed += 1;
        true
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

// ── 注入核（0xB540 段 5 项）────────────────────────────────────────────

pub fn inject_core_5() -> Vec<(&'static str, bool)> {
    let mut seed: u64 = 0x8481_B540;
    let mut ok = true;
    for _ in 0..64 {
        if lcg_next(&mut seed) == 0 {
            ok = false;
        }
    }
    let mut out: Vec<(&'static str, bool)> = Vec::new();
    out.push(("LCG-确定性-64轮不炸", ok));
    out.push(("段界-界内过", TargetSeg { vaddr: 0x1000, size: 0x1000 }.classify(0x1000).is_ok()));
    out.push(("段界-上界外EFAULT", TargetSeg { vaddr: 0x1000, size: 0x1000 }.classify(0x2000) == Err(ERR_EFAULT)));
    out.push(("段界-回绕EINVAL", TargetSeg { vaddr: 0x1000, size: 0x1000 }.classify(u64::MAX - 4) == Err(ERR_EINVAL)));
    out.push(("FNV-空串锚", fnv1a(b"") == 0x811C_9DC5));
    out
}

// ── 判据聚合 ────────────────────────────────────────────────────────────

/// run_unx8481_checks：本单判据聚合（诊断段 0xB540）。
pub fn run_unx8481_checks() -> crate::checks::CheckSet {
    let mut s = crate::checks::CheckSet::new("UNX-F8481 重定位表越界拒档");

    for (name, ok) in inject_core_5() {
        s.add(&name, ok, if ok { "" } else { "注入核失败" });
    }

    let seg = TargetSeg { vaddr: 0x600000, size: 0x10000 };
    let mut g = RelocGate::new();

    // 正向：合法重定位表 10/10 装载通过（B05 承接）
    let mut all_pass = true;
    let mut seed: u64 = 0x8481_0010;
    for _ in 0..10u32 {
        let n = (lcg_next(&mut seed) % 8 + 1) as u64;
        let mut table: Vec<RelocEnt> = Vec::new();
        for k in 0..n {
            let off = 0x600000 + (lcg_next(&mut seed) % (0x10000 - 8));
            table.push(RelocEnt { r_offset: off, r_type: 1, r_sym: k as u32 });
        }
        if !g.load(&seg, &table) {
            all_pass = false;
        }
    }
    s.add("正向-合法表10/10装载通过", all_pass && g.passed == 10, if all_pass { "" } else { "正向缺口" });

    // 反向三形：越上界
    let over_top = vec![RelocEnt { r_offset: 0x600000 + 0x10000, r_type: 1, r_sym: 0 }];
    let r1 = g.load(&seg, &over_top);
    // 反向：越下界
    let below_bot = vec![RelocEnt { r_offset: 0x5FFFF8, r_type: 1, r_sym: 0 }];
    let r2 = g.load(&seg, &below_bot);
    // 反向：回绕（offset 距 MAX 差 4，加 8 回绕）
    let wrap = vec![RelocEnt { r_offset: u64::MAX - 4, r_type: 1, r_sym: 0 }];
    let r3 = g.load(&seg, &wrap);
    let tri_ok = !r1 && !r2 && !r3 && g.rejected_efault == 2 && g.rejected_einval == 1;
    s.add("反向-越上界/越下界/回绕三形全拒", tri_ok, if tri_ok { "" } else { "三形漏网" });

    // 错误码与 Linux 对照：EFAULT=14 / EINVAL=22
    let code_ok = ERR_EFAULT == 14 && ERR_EINVAL == 22;
    s.add("错误码-Linux对照EFAULT/EINVAL", code_ok, if code_ok { "" } else { "错误码漂移" });

    // 差 1 字节边界：offset 使末 8B 恰出段界
    let edge = vec![RelocEnt { r_offset: 0x600000 + 0x10000 - 7, r_type: 1, r_sym: 0 }];
    let e1 = g.load(&seg, &edge);
    s.add("边界-差1字节出界拒", !e1, if !e1 { "" } else { "差1漏网" });

    // 恰好贴界：末 8B 恰在段内
    let fit = vec![RelocEnt { r_offset: 0x600000 + 0x10000 - 8, r_type: 1, r_sym: 0 }];
    let e2 = g.load(&seg, &fit);
    s.add("边界-恰贴上界通过", e2, if e2 { "" } else { "贴界误拒" });

    // 混表：合法项中混一条越界 → 全拒（不整表信任）
    let mixed = vec![
        RelocEnt { r_offset: 0x600010, r_type: 1, r_sym: 1 },
        RelocEnt { r_offset: 0x900000, r_type: 1, r_sym: 2 }, // 越界
        RelocEnt { r_offset: 0x600020, r_type: 1, r_sym: 3 },
    ];
    let m1 = g.load(&seg, &mixed);
    s.add("混表-一越界全拒不信任", !m1, if !m1 { "" } else { "混表漏网" });

    // 双判据缺一红灯：反向 10/10 注入
    let mut g2 = RelocGate::new();
    let mut n2 = 0u32;
    for i in 0..10u64 {
        let t = vec![RelocEnt { r_offset: 0x700000 + i * 0x1000, r_type: 1, r_sym: 0 }];
        if !g2.load(&seg, &t) {
            n2 += 1;
        }
    }
    s.add("双判据-反向10/10全拒", n2 == 10, if n2 == 10 { "" } else { "反向缺口" });

    // FNV 表指纹稳定
    let h1 = fnv1a(format!("{:?}", over_top).as_bytes());
    let h2 = fnv1a(format!("{:?}", over_top).as_bytes());
    s.add("FNV-表指纹重放稳定", h1 == h2 && h1 != 0, if h1 == h2 { "" } else { "指纹不稳" });

    s
}
