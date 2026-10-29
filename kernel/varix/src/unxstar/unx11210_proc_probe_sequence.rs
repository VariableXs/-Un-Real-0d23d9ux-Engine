//! UNX-F11210 /proc 类接口探测序列登记（0x1f810 段）
//!
//! 锚点：UNX-F11210-J1 glibc 启动探测的 /proc 路径清单逐项登记探测序与
//! 容忍态（缺失降级/缺失致命），与 F11341 procfs 面对账。
//! 域：CoRun·UNX-C5-B01——探测计划按序推进，序不可跳；每条路径带容忍位，
//! 缺失时降级只记账，致命则整轮失败，两种结局都可回读。

use crate::alloc::string::String;
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u32 = 0x1f810;
pub const PR_ORDER: u32 = SEG_BASE + 0x01; // 探测序跳号
pub const PR_UNKNOWN: u32 = SEG_BASE + 0x02; // 未登记路径
pub const PR_FATAL: u32 = SEG_BASE + 0x03; // 致命缺失
pub const PR_DUP: u32 = SEG_BASE + 0x04; // 路径重录
pub const PR_PATH: u32 = SEG_BASE + 0x05; // 路径非 /proc 前缀
pub const PR_FROZEN: u32 = SEG_BASE + 0x06; // 冻结后登记
pub const PR_DONE: u32 = SEG_BASE + 0x07; // 已完成后再探

/// 探测条数（glibc 启动面清单）。
pub const PROBES: usize = 12;
/// 容忍态：降级 / 致命。
pub const TOL_DEGRADE: u32 = 1;
pub const TOL_FATAL: u32 = 0;
pub const PR_TAG: u64 = 0x1f810c5;
pub const FP_STEP: u64 = 0x100000001b3;

/// 单条探测：路径、序号、容忍态、命中态。
#[derive(Clone, Debug)]
pub struct Probe {
    pub path: &'static str,
    pub order: u32,
    pub tolerant: u32,
    pub hit: bool,
}

impl Probe {
    pub fn degradable(&self) -> bool {
        self.tolerant == TOL_DEGRADE
    }

    pub fn render(&self) -> String {
        format!("{}#{}|{}", self.path, self.order, if self.hit { "hit" } else { "miss" })
    }
}

/// 一轮探测的汇总。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Summary {
    pub reads: u32,
    pub degraded: u32,
    pub fatal: u32,
}

/// 探测计划。
#[derive(Clone, Debug)]
pub struct ProcPlan {
    pub probes: Vec<Probe>,
    pub cursor: u32,
    pub degraded: u32,
    pub frozen: bool,
    pub root: Option<u64>,
}

impl ProcPlan {
    pub fn new() -> ProcPlan {
        ProcPlan { probes: Vec::new(), cursor: 0, degraded: 0, frozen: false, root: None }
    }

    /// 登记：冻结/非 /proc 前缀/重录/超容四路拒；序号自动递增。
    pub fn add(&mut self, path: &'static str, tolerant: u32) -> Result<u32, u32> {
        if self.frozen {
            return Err(PR_FROZEN);
        }
        if !starts_proc(path) {
            return Err(PR_PATH);
        }
        if self.probes.iter().any(|p| p.path == path) {
            return Err(PR_DUP);
        }
        if self.probes.len() >= PROBES {
            return Err(PR_DONE);
        }
        let order = self.probes.len() as u32 + 1;
        self.probes.push(Probe { path, order, tolerant, hit: false });
        Ok(order)
    }

    /// 顺序推进探测：路径必须在册且恰为游标下一条。
    pub fn probe(&mut self, path: &'static str, present: bool) -> Result<u32, u32> {
        if self.frozen {
            return Err(PR_FROZEN);
        }
        let mut idx: Option<usize> = None;
        let mut k = 0usize;
        while k < self.probes.len() {
            if self.probes[k].path == path {
                idx = Some(k);
            }
            k += 1;
        }
        let i = match idx {
            Some(i) => i,
            None => return Err(PR_UNKNOWN),
        };
        if self.probes[i].order != self.cursor + 1 {
            return Err(PR_ORDER);
        }
        self.cursor = self.probes[i].order;
        if present {
            self.probes[i].hit = true;
            return Ok(0);
        }
        if self.probes[i].degradable() {
            self.degraded = self.degraded.wrapping_add(1);
            Ok(1)
        } else {
            Err(PR_FATAL)
        }
    }

    /// 一轮全走：命中集为 present 列表。
    pub fn walk(&mut self, present: &[&str]) -> Result<Summary, u32> {
        let mut reads = 0u32;
        let mut degraded = 0u32;
        let mut k = 0usize;
        while k < self.probes.len() {
            let path = self.probes[k].path;
            let hit = present.contains(&path);
            match self.probe(path, hit) {
                Ok(1) => degraded = degraded.wrapping_add(1),
                Ok(_) => {}
                Err(e) => return Err(e),
            }
            if hit {
                reads = reads.wrapping_add(1);
            }
            k += 1;
        }
        Ok(Summary { reads, degraded, fatal: 0 })
    }

    /// 序连续无跳号。
    pub fn ordered(&self) -> bool {
        let mut k = 0usize;
        while k < self.probes.len() {
            if self.probes[k].order != k as u32 + 1 {
                return false;
            }
            k += 1;
        }
        !self.probes.is_empty()
    }

    pub fn tolerant_count(&self) -> usize {
        self.probes.iter().filter(|p| p.degradable()).count()
    }

    pub fn fold(&self) -> u64 {
        let mut h: u64 = PR_TAG;
        let mut k = 0usize;
        while k < self.probes.len() {
            let bytes = self.probes[k].path.as_bytes();
            let mut b = 0usize;
            while b < bytes.len() {
                h = (h ^ bytes[b] as u64).wrapping_mul(FP_STEP);
                b += 1;
            }
            h = (h ^ self.probes[k].order as u64).wrapping_mul(FP_STEP);
            h = (h ^ self.probes[k].tolerant as u64).wrapping_mul(FP_STEP);
            h = (h ^ self.probes[k].hit as u64).wrapping_mul(FP_STEP);
            k += 1;
        }
        h = (h ^ self.degraded as u64).wrapping_mul(FP_STEP);
        h
    }

    pub fn freeze(&mut self) -> u64 {
        self.frozen = true;
        let r = self.fold();
        self.root = Some(r);
        r
    }
}

/// /proc 前缀判（逐字节，不依赖标准库）。
pub fn starts_proc(p: &str) -> bool {
    let b = p.as_bytes();
    b.len() > 6 && b[0] == b'/' && b[1] == b'p' && b[2] == b'r' && b[3] == b'o' && b[4] == b'c' && b[5] == b'/'
}

/// 探测路径与容忍态样本（12 条，三条致命九条可降级）。
pub fn build_plan() -> (ProcPlan, bool) {
    let rows: [(&'static str, u32); 12] = [
        ("/proc/self/maps", TOL_FATAL),
        ("/proc/self/stat", TOL_FATAL),
        ("/proc/self/auxv", TOL_FATAL),
        ("/proc/self/status", TOL_DEGRADE),
        ("/proc/cpuinfo", TOL_DEGRADE),
        ("/proc/meminfo", TOL_DEGRADE),
        ("/proc/self/cmdline", TOL_DEGRADE),
        ("/proc/self/exe", TOL_DEGRADE),
        ("/proc/self/mountinfo", TOL_DEGRADE),
        ("/proc/self/cgroup", TOL_DEGRADE),
        ("/proc/sys/kernel/osrelease", TOL_DEGRADE),
        ("/proc/sys/vm/overcommit_memory", TOL_DEGRADE),
    ];
    let mut p = ProcPlan::new();
    let mut k = 0usize;
    while k < rows.len() {
        if p.add(rows[k].0, rows[k].1).is_err() {
            return (p, false);
        }
        k += 1;
    }
    let ok = p.ordered() && p.probes.len() == PROBES;
    (p, ok)
}

/// 注入核：四路坏样本必抓。
pub fn inject_four() -> [bool; 4] {
    let mut got = [false; 4];
    // 1) 非 /proc 前缀必拒。
    let mut a = ProcPlan::new();
    got[0] = a.add("/sys/kernel/x", TOL_DEGRADE).err() == Some(PR_PATH);
    // 2) 未登记路径探测必拒。
    let (mut b, _) = build_plan();
    got[1] = b.probe("/proc/nope", true).err() == Some(PR_UNKNOWN);
    // 3) 跳号探测必拒（先探第二条）。
    let (mut c, _) = build_plan();
    got[2] = c.probe("/proc/self/stat", true).err() == Some(PR_ORDER);
    // 4) 致命缺失必整轮失败，且游标停在该条。
    let (mut d, _) = build_plan();
    let e = d.probe("/proc/self/maps", false);
    got[3] = e.err() == Some(PR_FATAL) && d.cursor == 1 && d.degraded == 0;
    got
}

/// 批内判据。
pub fn run_unx11210_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F11210");
    let (mut p, built) = build_plan();

    s.add("J1-十二路径登记", built && p.probes.len() == PROBES, "12 条 /proc 探测路径逐项落账");
    s.add("J2-序连续", p.ordered() && p.probes[0].order == 1 && p.probes[11].order == 12,
        "探测序 1..12 连续无跳号");
    s.add("J3-容忍态配额", p.tolerant_count() == 9, "九条可降级三条致命");
    s.add("J4-前缀合规", p.probes.iter().all(|x| starts_proc(x.path)), "全部路径以 /proc/ 开头");

    let full: [&str; 3] = ["/proc/self/maps", "/proc/self/stat", "/proc/self/auxv"];
    let (mut w, _) = build_plan();
    let r = w.walk(&full);
    s.add("J5-全走成功", r == Ok(Summary { reads: 3, degraded: 9, fatal: 0 }),
        "仅三条命中时其余九条降级且整轮成功");

    let (mut d, _) = build_plan();
    let dg = d.probe("/proc/self/maps", true)
        .and_then(|_| d.probe("/proc/self/stat", true))
        .and_then(|_| d.probe("/proc/self/auxv", true))
        .and_then(|_| d.probe("/proc/self/status", false));
    s.add("J6-降级入账", dg == Ok(1) && d.degraded == 1 && d.cursor == 4 && !d.probes[3].hit,
        "可降级路径缺失记降级且游标推进");

    let (mut f, _) = build_plan();
    let fat = f.probe("/proc/self/maps", false);
    s.add("J7-致命即停", fat.err() == Some(PR_FATAL) && f.cursor == 1,
        "致命路径缺失整轮失败且游标不越过");

    s.add("J8-非前缀拒", {
        let mut a = ProcPlan::new();
        a.add("/etc/passwd", TOL_DEGRADE).err() == Some(PR_PATH)
    }, "非 /proc 路径登记必拒");

    s.add("J9-重录拒", {
        let mut a = ProcPlan::new();
        let _ = a.add("/proc/self/maps", TOL_FATAL);
        a.add("/proc/self/maps", TOL_DEGRADE).err() == Some(PR_DUP)
    }, "同路径二次登记必拒");

    let inj = inject_four();
    s.add("J10-注入四路", inj[0] && inj[1] && inj[2] && inj[3],
        "非前缀/未登记/跳号/致命四路必抓");

    let root = p.freeze();
    s.add("J11-冻结根", p.fold() == root && root != 0, "冻结后计划指纹可重算");
    s.add("J12-冻结拒登", p.add("/proc/self/limits", TOL_DEGRADE).err() == Some(PR_FROZEN),
        "冻结后新路径登记必拒");

    let fp = p.fold();
    s.add_dyn("J13-计划明细", p.ordered() && fp == root,
        &format!("probes={} tol={}", p.probes.len(), p.tolerant_count()));
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx11210_proc_flow() {
        let (mut p, built) = build_plan();
        assert!(built);
        assert!(p.ordered());
        assert_eq!(p.probe("/proc/self/stat", true), Err(PR_ORDER));
        assert_eq!(p.probe("/proc/self/maps", true), Ok(0));
        assert_eq!(p.probe("/proc/self/stat", false), Err(PR_FATAL));
        assert_eq!(p.probe("/proc/nope", true), Err(PR_UNKNOWN));
        let root = p.freeze();
        assert_eq!(p.fold(), root);
        assert!(p.add("/proc/x", TOL_DEGRADE).is_err());
        assert!(starts_proc("/proc/self/maps") && !starts_proc("/proc"));
    }

    #[test]
    fn unx11210_checks_green() {
        let s = run_unx11210_checks();
        assert!(s.all_passed(), "UNX-F11210 判据未全绿");
    }
}
