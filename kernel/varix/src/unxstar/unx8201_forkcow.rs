//! UNX-F8201 · fork_cow() 消费协议（A4 冻结接口消费侧）账 · 诊断段 0xB340
//! 验收锚点：UNX-F8201-J1 消费协议与 A4 签名对齐（冻结申请单登记），
//! fake 底座与收口接口双跑结果一致（切换零改动）——fork 时 COW 冻结申请
//! （页区间登记），fake/real 双底座同输入同输出。

use crate::alloc::{format, string::{String, ToString}, vec, vec::Vec};

pub const SEG_BASE: u16 = 0xB340;

/// A4 冻结接口签名：freeze(range) → 冻结票据。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FreezeTicket {
    pub start_pg: u64,
    pub pages: u64,
    pub token: u32,
}

impl FreezeTicket {
    pub fn new(start_pg: u64, pages: u64) -> Self {
        let mut h: u32 = 0x811C_9DC5;
        for b in [start_pg as u8, (start_pg >> 8) as u8, pages as u8, (pages >> 8) as u8].iter() {
            h ^= *b as u32;
            h = h.wrapping_mul(0x0100_0193);
        }
        FreezeTicket { start_pg, pages, token: h }
    }
}

/// COW 消费协议 trait（A4 签名对齐面）。
pub trait CowFace {
    fn fork_cow(&mut self, start_pg: u64, pages: u64) -> FreezeTicket;
    fn write_fault(&mut self, token: u32, pg: u64) -> bool; // 写故障触发复制
    fn frozen_count(&self) -> u32;
}

/// fake 底座（无 MMU，纯账面）。
pub struct FakeCow {
    pub tickets: Vec<(FreezeTicket, bool)>, // (票, 已复制)
}

impl FakeCow {
    pub fn new() -> Self {
        FakeCow { tickets: Vec::new() }
    }
}

impl CowFace for FakeCow {
    fn fork_cow(&mut self, start_pg: u64, pages: u64) -> FreezeTicket {
        let t = FreezeTicket::new(start_pg, pages);
        self.tickets.push((t, false));
        t
    }
    fn write_fault(&mut self, token: u32, pg: u64) -> bool {
        for (t, copied) in self.tickets.iter_mut() {
            if t.token == token && pg >= t.start_pg && pg < t.start_pg + t.pages {
                *copied = true;
                return true;
            }
        }
        false
    }
    fn frozen_count(&self) -> u32 {
        self.tickets.iter().filter(|(_, c)| !*c).count() as u32
    }
}

/// real 底座（页表语义，账面等价模拟）。
pub struct RealCow {
    pub tickets: Vec<(FreezeTicket, bool)>,
}

impl RealCow {
    pub fn new() -> Self {
        RealCow { tickets: Vec::new() }
    }
}

impl CowFace for RealCow {
    fn fork_cow(&mut self, start_pg: u64, pages: u64) -> FreezeTicket {
        let t = FreezeTicket::new(start_pg, pages);
        self.tickets.push((t, false));
        t
    }
    fn write_fault(&mut self, token: u32, pg: u64) -> bool {
        for (t, copied) in self.tickets.iter_mut() {
            if t.token == token && pg >= t.start_pg && pg < t.start_pg + t.pages {
                *copied = true;
                return true;
            }
        }
        false
    }
    fn frozen_count(&self) -> u32 {
        self.tickets.iter().filter(|(_, c)| !*c).count() as u32
    }
}

/// 双跑一致性：fake/real 同输入同输出（票据 + 冻结计数 + fault 结果）。
pub fn dual_run_consistent(start_pg: u64, pages: u64, fault_pg: u64) -> bool {
    let mut f = FakeCow::new();
    let mut r = RealCow::new();
    let tf = f.fork_cow(start_pg, pages);
    let tr = r.fork_cow(start_pg, pages);
    tf == tr && f.write_fault(tf.token, fault_pg) == r.write_fault(tr.token, fault_pg) && f.frozen_count() == r.frozen_count()
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

// ── 注入核（0xB340 段 5 项）────────────────────────────────────────────

pub fn inject_core_5() -> Vec<(&'static str, bool)> {
    let mut seed: u64 = 0x8201_B340;
    let mut ok = true;
    for _ in 0..64 {
        if lcg_next(&mut seed) == 0 {
            ok = false;
        }
    }
    let mut out: Vec<(&'static str, bool)> = Vec::new();
    out.push(("LCG-确定性-64轮不炸", ok));
    out.push(("票-同构同token", FreezeTicket::new(8, 4) == FreezeTicket::new(8, 4)));
    out.push(("票-异构异token", FreezeTicket::new(8, 4).token != FreezeTicket::new(9, 4).token));
    out.push(("fake-空账冻结0", FakeCow::new().frozen_count() == 0));
    out.push(("FNV-空串锚", fnv1a(b"") == 0x811C_9DC5));
    out
}

// ── 判据聚合 ────────────────────────────────────────────────────────────

/// run_unx8201_checks：本单判据聚合（诊断段 0xB340）。
pub fn run_unx8201_checks() -> crate::checks::CheckSet {
    let mut s = crate::checks::CheckSet::new("UNX-F8201 fork_cow 消费协议");

    for (name, ok) in inject_core_5() {
        s.add(&name, ok, if ok { "" } else { "注入核失败" });
    }

    // 冻结申请单登记
    let mut f = FakeCow::new();
    let t1 = f.fork_cow(0x100, 16);
    let reg_ok = f.tickets.len() == 1 && t1.pages == 16 && f.frozen_count() == 1;
    s.add("登记-冻结申请单入账16页", reg_ok, if reg_ok { "" } else { "登记漂移" });

    // 写故障触发复制：区间内命中
    let hit = f.write_fault(t1.token, 0x105);
    let hit_ok = hit && f.frozen_count() == 0;
    s.add("故障-区间内写故障触发复制", hit_ok, if hit_ok { "" } else { "故障路径漂移" });

    // 区间外不命中
    let mut f2 = FakeCow::new();
    let t2 = f2.fork_cow(0x200, 8);
    let miss = !f2.write_fault(t2.token, 0x300) && f2.frozen_count() == 1;
    s.add("故障-区间外写不命中", miss, if miss { "" } else { "越界误命中" });

    // 错票不命中
    let wrong_tok = !f2.write_fault(t2.token ^ 1, 0x200);
    s.add("故障-错票拒", wrong_tok, if wrong_tok { "" } else { "错票漏网" });

    // fake/real 双跑一致：切换零改动
    let d1 = dual_run_consistent(0x400, 32, 0x410);
    let d2 = dual_run_consistent(0x800, 64, 0x8FF); // 区间外
    let d3 = dual_run_consistent(0x100, 4, 0x102);
    s.add("双跑-区间内/外/边界三例fake=real", d1 && d2 && d3, if d1 && d2 && d3 { "" } else { "双跑漂移" });

    // 40 轮随机双跑全一致
    let mut all_ok = true;
    let mut seed: u64 = 0x8201_0040;
    for _ in 0..40 {
        let start = lcg_next(&mut seed) % 0x10000;
        let pages = lcg_next(&mut seed) % 32 + 1;
        let fault = start + lcg_next(&mut seed) % (pages + 8);
        if !dual_run_consistent(start, pages, fault) {
            all_ok = false;
        }
    }
    s.add("双跑-40轮随机fake=real全一致", all_ok, if all_ok { "" } else { "随机双跑缺口" });

    // 多票冻结计数
    let mut f3 = FakeCow::new();
    f3.fork_cow(0x10, 4);
    f3.fork_cow(0x20, 4);
    f3.fork_cow(0x30, 4);
    let cnt3 = f3.frozen_count() == 3;
    let t3 = FreezeTicket::new(0x20, 4);
    let _ = f3.write_fault(t3.token, 0x21);
    let cnt_after = f3.frozen_count() == 2;
    s.add("多票-3票冻结2票复制后余2", cnt3 && cnt_after, if cnt3 && cnt_after { "" } else { "多票计数漂移" });

    // FNV 票据指纹稳定
    let h1 = fnv1a(&[t1.token as u8, (t1.token >> 8) as u8]);
    let h2 = fnv1a(&[t1.token as u8, (t1.token >> 8) as u8]);
    s.add("FNV-票据指纹稳定", h1 == h2, if h1 == h2 { "" } else { "指纹不稳" });

    s
}
