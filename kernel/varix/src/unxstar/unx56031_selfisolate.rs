//! UNX-F56031 延迟采集器资源自隔离件（0x603C0 段）
//!
//! 锚点：UNX-F56031-J1 采集器自身内存/句柄/线程数上限自限额（三限额
//! 逐项落账，逐笔获取计账）；J2 自隔离失败即中止采集（超额获取一律
//! 拒并留痕，禁污染样本）；J3 自检 ×3 零泄漏（全释放后三计数归零 ×3
//! 轮全绿）三判据全绿。判据锚定 §AI-71.2 专题二①。
//! 域：CoRun·UNX-F2-B40（syscall 延迟域·资源自隔离面）。
//!
//! 结构：三限额闭集账（内存页 × 句柄 × 线程上限）+ 逐笔获取/释放计账
//! 核（超额即拒留痕）+ 泄漏自检核（全释放归零 ×3 轮）+ 采集闸（自隔
//! 离未立禁采）+ 注入核（未立账即采/超额内存/超额句柄/超额线程/封账
//! 改写必抓）。

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x603C0 独占 0x20）。
pub const SEG_BASE: u32 = 0x603C0;
pub const RS_NO_ACCOUNT: u32 = SEG_BASE + 0x01; // 资源账未立即采
pub const RS_MEM_OVER: u32 = SEG_BASE + 0x02; // 内存页超限
pub const RS_HANDLE_OVER: u32 = SEG_BASE + 0x03; // 句柄超限
pub const RS_THREAD_OVER: u32 = SEG_BASE + 0x04; // 线程超限
pub const RS_REL_OVER: u32 = SEG_BASE + 0x05; // 超释放（计数下溢守闸）
pub const RS_SEALED: u32 = SEG_BASE + 0x06; // 资源账已封账拒改
pub const RS_LIMIT_ZERO: u32 = SEG_BASE + 0x07; // 零上限（限额未声明）

/// 三限额闭集（页 / 个 / 个）。
pub const LIM_MEM_PAGES: u32 = 512;
pub const LIM_HANDLES: u32 = 64;
pub const LIM_THREADS: u32 = 8;

/// 资源种类闭集。
pub const K_MEM: usize = 0;
pub const K_HANDLE: usize = 1;
pub const K_THREAD: usize = 2;
pub const K_COUNT: usize = 3;
pub const K_NAMES: [&str; K_COUNT] = ["mem_pages", "handles", "threads"];

/// 三限额声明账。
#[derive(Clone)]
pub struct Limits {
    pub mem_pages: u32,
    pub handles: u32,
    pub threads: u32,
}

impl Limits {
    pub fn new(mem_pages: u32, handles: u32, threads: u32) -> Result<Self, u32> {
        if mem_pages == 0 || handles == 0 || threads == 0 {
            return Err(RS_LIMIT_ZERO);
        }
        Ok(Limits { mem_pages, handles, threads })
    }

    /// 按种类取限额。
    pub fn limit_of(&self, kind: usize) -> Result<u32, u32> {
        match kind {
            K_MEM => Ok(self.mem_pages),
            K_HANDLE => Ok(self.handles),
            K_THREAD => Ok(self.threads),
            _ => Err(RS_NO_ACCOUNT),
        }
    }
}

/// 资源账：逐笔获取/释放计账。
pub struct ResourceAccount {
    pub limits: Option<Limits>,
    pub used: [u32; K_COUNT],
    pub over_events: Vec<(usize, u32)>,
    pub sealed: bool,
}

impl ResourceAccount {
    pub fn new() -> Self {
        ResourceAccount {
            limits: None,
            used: [0; K_COUNT],
            over_events: Vec::new(),
            sealed: false,
        }
    }

    /// 立账（限额声明）。
    pub fn open(&mut self, l: Limits) -> Result<(), u32> {
        if self.sealed {
            return Err(RS_SEALED);
        }
        self.limits = Some(l);
        Ok(())
    }

    /// J1：自隔离立账核。
    pub fn ready(&self) -> bool {
        self.limits.is_some()
    }

    /// 获取一笔：未立账拒 / 超额拒（留痕）。
    pub fn acquire(&mut self, kind: usize, n: u32) -> Result<u32, u32> {
        if self.sealed {
            return Err(RS_SEALED);
        }
        if kind >= K_COUNT {
            return Err(RS_NO_ACCOUNT);
        }
        let lim = match self.limits.as_ref() {
            None => return Err(RS_NO_ACCOUNT),
            Some(l) => l.limit_of(kind)?,
        };
        let next = self.used[kind] + n;
        if next > lim {
            self.over_events.push((kind, n));
            match kind {
                K_MEM => return Err(RS_MEM_OVER),
                K_HANDLE => return Err(RS_HANDLE_OVER),
                _ => return Err(RS_THREAD_OVER),
            }
        }
        self.used[kind] = next;
        Ok(self.used[kind])
    }

    /// 释放一笔：超释放拒（计数下溢守闸）。
    pub fn release(&mut self, kind: usize, n: u32) -> Result<u32, u32> {
        if self.sealed {
            return Err(RS_SEALED);
        }
        if kind >= K_COUNT {
            return Err(RS_NO_ACCOUNT);
        }
        if n > self.used[kind] {
            return Err(RS_REL_OVER);
        }
        self.used[kind] -= n;
        Ok(self.used[kind])
    }

    /// J3：零泄漏核（三计数全零）。
    pub fn no_leak(&self) -> bool {
        self.used == [0; K_COUNT]
    }

    /// 超额事件计数。
    pub fn over_count(&self) -> usize {
        self.over_events.len()
    }

    /// 封账。
    pub fn seal(&mut self) {
        self.sealed = true;
    }
}

/// ×3 自检轮：获取→释放→归零（逐轮全绿计数）。
pub fn selfcheck_rounds() -> (usize, usize) {
    let mut green = 0usize;
    for _ in 0..3 {
        let mut a = ResourceAccount::new();
        a.open(Limits::new(LIM_MEM_PAGES, LIM_HANDLES, LIM_THREADS).expect("lim"))
            .expect("open");
        let _ = a.acquire(K_MEM, 64);
        let _ = a.acquire(K_HANDLE, 4);
        let _ = a.acquire(K_THREAD, 2);
        let _ = a.release(K_MEM, 64);
        let _ = a.release(K_HANDLE, 4);
        let _ = a.release(K_THREAD, 2);
        if a.no_leak() {
            green += 1;
        }
    }
    (green, 3)
}

/// 注入核：5 类非法操作必抓。
pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    // 1) 未立账即采
    let mut a = ResourceAccount::new();
    if a.acquire(K_MEM, 1) == Err(RS_NO_ACCOUNT) {
        caught += 1;
    }
    // 2) 内存超限（513 > 512）
    let mut a2 = fresh_account();
    if a2.acquire(K_MEM, LIM_MEM_PAGES + 1) == Err(RS_MEM_OVER) && a2.over_count() == 1 {
        caught += 1;
    }
    // 3) 句柄超限（累计 65 > 64）
    let mut a3 = fresh_account();
    let _ = a3.acquire(K_HANDLE, LIM_HANDLES);
    if a3.acquire(K_HANDLE, 1) == Err(RS_HANDLE_OVER) {
        caught += 1;
    }
    // 4) 超释放（下溢守闸）
    let mut a4 = fresh_account();
    if a4.release(K_THREAD, 1) == Err(RS_REL_OVER) {
        caught += 1;
    }
    // 5) 零上限声明
    if Limits::new(0, 64, 8).err() == Some(RS_LIMIT_ZERO) {
        caught += 1;
    }
    (caught, 5)
}

/// 新立账夹具。
pub fn fresh_account() -> ResourceAccount {
    let mut a = ResourceAccount::new();
    a.open(Limits::new(LIM_MEM_PAGES, LIM_HANDLES, LIM_THREADS).expect("lim"))
        .expect("open");
    a
}

/// 判据入口（CheckSet）。
pub fn run_unx56031_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F56031");
    // J1-1 三限额落账（512/64/8）
    let a = fresh_account();
    let ok = a.ready()
        && a.limits.as_ref().expect("l").mem_pages == 512
        && a.limits.as_ref().expect("l").handles == 64
        && a.limits.as_ref().expect("l").threads == 8;
    s.add("56031-J1-三限额落账", ok, if ok { "" } else { "限额账缺" });
    // J1-2 逐笔获取计账（64+4+2 在限内全过）
    let mut a2 = fresh_account();
    let m = a2.acquire(K_MEM, 64).expect("m");
    let h = a2.acquire(K_HANDLE, 4).expect("h");
    let t = a2.acquire(K_THREAD, 2).expect("t");
    let ok = m == 64 && h == 4 && t == 2 && a2.used == [64, 4, 2];
    s.add("56031-J1-逐笔计账", ok, if ok { "" } else { "计账漂移" });
    // J1-3 限额种类闭集同源
    let ok = K_NAMES[K_MEM] == "mem_pages" && K_NAMES[K_THREAD] == "threads";
    s.add("56031-J1-种类名同源", ok, if ok { "" } else { "命名漂移" });
    // J2-1 超额拒采留痕（内存/句柄/线程三路各拒一次）
    let mut a3 = fresh_account();
    let e1 = a3.acquire(K_MEM, 600);
    let e2 = a3.acquire(K_HANDLE, 100);
    let e3 = a3.acquire(K_THREAD, 100);
    let ok = e1 == Err(RS_MEM_OVER)
        && e2 == Err(RS_HANDLE_OVER)
        && e3 == Err(RS_THREAD_OVER)
        && a3.over_count() == 3
        && a3.used == [0, 0, 0];
    s.add("56031-J2-超额拒采留痕", ok, if ok { "" } else { "超额穿透" });
    // J2-2 累计超限（先取 500 再取 13 → 513 > 512 拒）
    let mut a4 = fresh_account();
    let _ = a4.acquire(K_MEM, 500);
    let ok = a4.acquire(K_MEM, 13) == Err(RS_MEM_OVER) && a4.used[K_MEM] == 500;
    s.add("56031-J2-累计超限", ok, if ok { "" } else { "累计口径漂移" });
    // J2-3 采集闸（未立账 ready=false）
    let bare = ResourceAccount::new();
    let ok = !bare.ready();
    s.add("56031-J2-未立账禁采", ok, if ok { "" } else { "未立账放行" });
    // J3-1 自检 ×3 零泄漏
    let (g, n) = selfcheck_rounds();
    let ok = g == n && n == 3;
    let detail = if ok { String::new() } else { format!("自检 {g}/{n}") };
    s.add_dyn("56031-J3-自检三轮零泄漏", ok, &detail);
    // J3-2 释放后部分归零（逐项释放语义）
    let mut a5 = fresh_account();
    let _ = a5.acquire(K_MEM, 100);
    let _ = a5.acquire(K_HANDLE, 8);
    let _ = a5.release(K_MEM, 100);
    let ok = a5.used[K_MEM] == 0 && a5.used[K_HANDLE] == 8;
    s.add("56031-J3-逐项释放", ok, if ok { "" } else { "释放语义漂移" });
    // J3-3 注入核 5 类必抓
    let (c, n2) = injection_core();
    let ok = c == n2;
    let detail = if ok { String::new() } else { format!("注入 {c}/{n2}") };
    s.add_dyn("56031-J3-注入核必抓", ok, &detail);
    // J1-4 超释放守闸语义
    let mut a6 = fresh_account();
    let ok = a6.release(K_HANDLE, 1) == Err(RS_REL_OVER);
    s.add("56031-J1-超释放守闸", ok, if ok { "" } else { "下溢穿透" });
    // J2-4 封账冻结
    let mut a7 = fresh_account();
    a7.seal();
    let ok = a7.acquire(K_MEM, 1) == Err(RS_SEALED)
        && a7.open(Limits::new(1, 1, 1).expect("l")) == Err(RS_SEALED);
    s.add("56031-J2-封账冻结", ok, if ok { "" } else { "封账后可写" });
    // J1-5 零上限拒（限额未声明）
    let ok = Limits::new(512, 0, 8).err() == Some(RS_LIMIT_ZERO);
    s.add("56031-J1-零上限拒", ok, if ok { "" } else { "零限额穿透" });
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx56031_all_checks_green() {
        let set = run_unx56031_checks();
        assert!(set.all_passed(), "UNX-F56031 checks must be green: {}", set.red_items_text());
    }

    #[test]
    fn unx56031_selfcheck_zero_leak() {
        assert_eq!(selfcheck_rounds(), (3, 3));
    }
}
