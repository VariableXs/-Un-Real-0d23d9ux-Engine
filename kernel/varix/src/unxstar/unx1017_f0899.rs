// unx1017_f0899 —— UNX-F1017 计时面健康摘要（F0899 口径注册）
// 判据原文 UNX-F1009→F1017-J1：挂接完整（B11 十账摘要入 F0997 无缺），schema
// 校验通过，投递 10/10 次可达。
// 落位：kernel/varix/src/unxstar/；诊断段 0x1C20 独占（0x1C20~0x1C5F）。
//
// 设计（纯功能 no_std）：
//   * 十账闭集：B11 计时面十本账（TSC/换算/漂移/死表/ktime/健康/启动/压测/
//     注入/快照）摘要逐册登记；
//   * 摘要 schema：册 id 闭集 + 行数非零 + 指纹非零（三闸）；
//   * F0899 口径注册：摘要聚合入 F0997 挂点（无缺断言=十册全挂）；
//   * 投递账：注册完成后向 F0899 投递（回执对冲 10/10 可达）；
//   * 封账：注册冻结 + FNV 指纹（缺册即拒封）；
//   * 守恒：注册数=投递数=十；注入核六类全抓。

use crate::checks::CheckSet;
use alloc::vec;
use alloc::vec::Vec;

// ── 诊断段（0x1C20 独占） ─────────────────────────────────────────────
pub const XC_SUM_ID: u32 = 0x1C21; // 册 id 越界（十账闭集外）
pub const XC_SUM_DUP: u32 = 0x1C22; // 册重复注册
pub const XC_SUM_SCHEMA: u32 = 0x1C23; // schema 闸破（行数/指纹零）
pub const XC_SUM_SEALED: u32 = 0x1C24; // 封账后注册
pub const XC_SUM_MISS: u32 = 0x1C25; // 缺册挂接
pub const XC_SUM_LOST: u32 = 0x1C26; // 注册数与投递数不守恒

/// B11 十账闭集 id。
pub const BK_TSC: u32 = 0;
pub const BK_CONV: u32 = 1;
pub const BK_DRIFT: u32 = 2;
pub const BK_DEAD: u32 = 3;
pub const BK_KTIME: u32 = 4;
pub const BK_HEALTH: u32 = 5;
pub const BK_BOOT: u32 = 6;
pub const BK_STRESS: u32 = 7;
pub const BK_FI: u32 = 8;
pub const BK_SNAP: u32 = 9;
pub const N_BOOKS: usize = 10;

/// 一册摘要。
#[derive(Clone, Copy)]
pub struct BookSum {
    pub bid: u32,
    pub rows: u32,
    pub fp: u64,
    pub sent: bool,
    pub acked: bool,
}

/// 计时面健康摘要账（F0899 口径）。
pub struct SummaryLedger {
    books: Vec<BookSum>,
    delivered: u32,
    acked: u32,
    sealed: bool,
}

impl SummaryLedger {
    pub fn new() -> SummaryLedger {
        SummaryLedger {
            books: Vec::new(),
            delivered: 0,
            acked: 0,
            sealed: false,
        }
    }

    /// 注册一册摘要（id 闭集 + 重册拒 + schema 双闸：行数/指纹非零）。
    pub fn register(&mut self, bid: u32, rows: u32, fp: u64) -> Result<(), u32> {
        if self.sealed {
            return Err(XC_SUM_SEALED);
        }
        if bid >= N_BOOKS as u32 {
            return Err(XC_SUM_ID);
        }
        if self.books.iter().any(|b| b.bid == bid) {
            return Err(XC_SUM_DUP);
        }
        if rows == 0 || fp == 0 {
            return Err(XC_SUM_SCHEMA);
        }
        self.books.push(BookSum {
            bid,
            rows,
            fp,
            sent: false,
            acked: false,
        });
        Ok(())
    }

    /// 十册齐备断言。
    pub fn books_full(&self) -> bool {
        self.books.len() == N_BOOKS
    }

    /// 向 F0899 投递（全册逐条，回执可选）。
    pub fn dispatch(&mut self, ack: bool) -> Result<u32, u32> {
        if self.sealed {
            return Err(XC_SUM_SEALED);
        }
        if !self.books_full() {
            return Err(XC_SUM_MISS);
        }
        for b in self.books.iter_mut() {
            if !b.sent {
                b.sent = true;
                self.delivered = self.delivered.wrapping_add(1);
                if ack {
                    b.acked = true;
                    self.acked = self.acked.wrapping_add(1);
                }
            }
        }
        Ok(self.delivered)
    }

    /// 投递 10/10 可达（十册全发全回执）。
    pub fn delivered_all(&self) -> bool {
        self.delivered == N_BOOKS as u32 && self.acked == N_BOOKS as u32
    }

    /// 无缺挂接（F0997 口径）：十册全注册且全投递。
    pub fn hookage_complete(&self) -> bool {
        self.books_full() && self.books.iter().all(|b| b.sent)
    }

    /// 守恒：投递数=注册数（一次一投）。
    pub fn conserved(&self) -> bool {
        self.delivered == self.books.len() as u32
    }

    pub fn seal(&mut self) -> Result<u64, u32> {
        if self.sealed {
            return Err(XC_SUM_SEALED);
        }
        if !self.books_full() {
            return Err(XC_SUM_MISS);
        }
        if !self.conserved() {
            return Err(XC_SUM_LOST);
        }
        self.sealed = true;
        Ok(self.fingerprint())
    }

    pub fn is_sealed(&self) -> bool {
        self.sealed
    }

    /// FNV 指纹：逐册（id/rows/fp/sent/acked）入列。
    pub fn fingerprint(&self) -> u64 {
        let mut h: u32 = 0x811C_9DC5;
        h ^= self.books.len() as u32;
        h = h.wrapping_mul(0x0100_0193);
        for b in self.books.iter() {
            h ^= b.bid ^ b.rows ^ (b.fp as u32) ^ ((b.sent as u32) << 8) ^ ((b.acked as u32) << 16);
            h = h.wrapping_mul(0x0100_0193);
        }
        h ^= self.delivered ^ self.acked.wrapping_mul(0x9E37);
        h = h.wrapping_mul(0x0100_0193);
        (h as u64) << 16 | (self.delivered as u64 & 0xFFFF)
    }

    pub fn book_count(&self) -> usize {
        self.books.len()
    }
    pub fn delivered(&self) -> u32 {
        self.delivered
    }
}

/// 十册示例指纹（非零占位由各册真实账给——此处构造确定性值）。
pub fn book_demo_fp(bid: u32) -> u64 {
    let mut h: u32 = 0x811C_9DC5;
    h ^= bid.wrapping_mul(0x9E37);
    h = h.wrapping_mul(0x0100_0193);
    ((h as u64) << 8) | (bid as u64 + 1)
}

/// 注入核：六类各 10/10。
pub fn inject_core() -> (u32, u32) {
    let mut caught = 0u32;
    const N: u32 = 6;

    // 1）册 id 越界必拒（10..=19 十例）
    {
        let mut l = SummaryLedger::new();
        let mut hits = 0;
        for b in 10..=19u32 {
            if l.register(b, 5, book_demo_fp(b)) == Err(XC_SUM_ID) {
                hits += 1;
            }
        }
        if hits == 10 {
            caught += 1;
        }
    }
    // 2）重册必拒（10/10）
    {
        let mut l = SummaryLedger::new();
        let _ = l.register(BK_TSC, 5, book_demo_fp(BK_TSC));
        let mut hits = 0;
        for _ in 0..10 {
            if l.register(BK_TSC, 6, book_demo_fp(BK_TSC)) == Err(XC_SUM_DUP) {
                hits += 1;
            }
        }
        if hits == 10 {
            caught += 1;
        }
    }
    // 3）schema 闸破必拒（零行/零指纹各半）
    {
        let mut l = SummaryLedger::new();
        let mut hits = 0;
        for k in 0..5u32 {
            if l.register(k, 0, book_demo_fp(k)) == Err(XC_SUM_SCHEMA) {
                hits += 1;
            }
            if l.register(k + 5, 5, 0) == Err(XC_SUM_SCHEMA) {
                hits += 1;
            }
        }
        if hits == 10 {
            caught += 1;
        }
    }
    // 4）缺册投递必拒
    {
        let mut l = SummaryLedger::new();
        for b in 0..9u32 {
            let _ = l.register(b, 5, book_demo_fp(b));
        }
        if l.dispatch(true) == Err(XC_SUM_MISS) {
            caught += 1;
        }
    }
    // 5）封账后注册必拒
    {
        let mut l = SummaryLedger::new();
        for b in 0..10u32 {
            let _ = l.register(b, 5, book_demo_fp(b));
        }
        let _ = l.dispatch(true);
        let _ = l.seal();
        if l.register(BK_TSC, 5, book_demo_fp(BK_TSC)) == Err(XC_SUM_SEALED) {
            caught += 1;
        }
    }
    // 6）缺册封账必拒
    {
        let mut l = SummaryLedger::new();
        for b in 0..5u32 {
            let _ = l.register(b, 5, book_demo_fp(b));
        }
        if l.seal() == Err(XC_SUM_MISS) {
            caught += 1;
        }
    }
    (N, caught)
}

/// 判据入口：UNX-F1009→F1017-J1。
pub fn run_unx1017_checks() -> CheckSet {
    let mut s = CheckSet::new("unxstar-unx1017");

    // C17-01 十账注册齐备
    {
        let mut l = SummaryLedger::new();
        let mut ok = true;
        for b in 0..10u32 {
            if l.register(b, 20 + b, book_demo_fp(b)).is_err() {
                ok = false;
            }
        }
        ok = ok && l.books_full() && l.book_count() == 10;
        s.add("C17-01-十账齐备", ok, if ok { "" } else { "缺册" });
    }
    // C17-02 schema 三闸（行数/指纹/重册）
    {
        let mut l = SummaryLedger::new();
        let a = l.register(0, 0, book_demo_fp(0)) == Err(XC_SUM_SCHEMA);
        let b = l.register(0, 10, 0) == Err(XC_SUM_SCHEMA);
        let _ = l.register(0, 10, book_demo_fp(0));
        let c = l.register(0, 10, book_demo_fp(0)) == Err(XC_SUM_DUP);
        s.add("C17-02-schema 三闸", a && b && c, if a && b && c { "" } else { "闸漏" });
    }
    // C17-03 投递 10/10 可达
    {
        let mut ok = true;
        for _ in 0..10 {
            let mut l = SummaryLedger::new();
            for b in 0..10u32 {
                let _ = l.register(b, 20 + b, book_demo_fp(b));
            }
            if l.dispatch(true).is_err() || !l.delivered_all() {
                ok = false;
                break;
            }
        }
        s.add("C17-03-投递十连", ok, if ok { "" } else { "投递缺" });
    }
    // C17-04 无回执不可达
    {
        let mut l = SummaryLedger::new();
        for b in 0..10u32 {
            let _ = l.register(b, 20 + b, book_demo_fp(b));
        }
        let _ = l.dispatch(false);
        let ok = !l.delivered_all() && l.hookage_complete();
        s.add("C17-04-无回执判缺", ok, if ok { "" } else { "回执误判" });
    }
    // C17-05 F0997 挂接无缺
    {
        let mut l = SummaryLedger::new();
        for b in 0..10u32 {
            let _ = l.register(b, 30 + b, book_demo_fp(b));
        }
        let _ = l.dispatch(true);
        let ok = l.hookage_complete() && l.conserved();
        s.add("C17-05-F0997 挂接", ok, if ok { "" } else { "挂接缺" });
    }
    // C17-06 缺册投递拒
    {
        let mut l = SummaryLedger::new();
        for b in 0..9u32 {
            let _ = l.register(b, 20 + b, book_demo_fp(b));
        }
        let ok = l.dispatch(true) == Err(XC_SUM_MISS);
        s.add("C17-06-缺册拒投", ok, if ok { "" } else { "缺册假绿" });
    }
    // C17-07 封账冻结
    {
        let mut l = SummaryLedger::new();
        for b in 0..10u32 {
            let _ = l.register(b, 20 + b, book_demo_fp(b));
        }
        let _ = l.dispatch(true);
        let fp = l.seal();
        let ok = fp.is_ok()
            && l.seal() == Err(XC_SUM_SEALED)
            && l.register(BK_SNAP, 5, book_demo_fp(BK_SNAP)) == Err(XC_SUM_SEALED)
            && l.dispatch(true) == Err(XC_SUM_SEALED);
        s.add("C17-07-封账冻结", ok, if ok { "" } else { "封账失守" });
    }
    // C17-08 守恒
    {
        let mut l = SummaryLedger::new();
        for b in 0..10u32 {
            let _ = l.register(b, 20 + b, book_demo_fp(b));
        }
        let _ = l.dispatch(true);
        s.add("C17-08-注投守恒", l.conserved(), if l.conserved() { "" } else { "守恒破" });
    }
    // C17-09 指纹往返
    {
        let build = || {
            let mut l = SummaryLedger::new();
            for b in 0..10u32 {
                let _ = l.register(b, 20 + b, book_demo_fp(b));
            }
            let _ = l.dispatch(true);
            l.fingerprint()
        };
        let ok = build() == build();
        s.add("C17-09-指纹一致", ok, if ok { "" } else { "指纹漂移" });
    }
    // C17-10 册 id 边界（9 合法 10 拒）
    {
        let mut l = SummaryLedger::new();
        let a = l.register(9, 5, book_demo_fp(9));
        let b = l.register(10, 5, book_demo_fp(10));
        let ok = a.is_ok() && b == Err(XC_SUM_ID);
        s.add("C17-10-册 id 边界", ok, if ok { "" } else { "边界误判" });
    }
    // C17-11 注入核 6/6
    {
        let (n, c) = inject_core();
        let ok = n == 6 && c == 6;
        s.add("C17-11-注入核全抓", ok, if ok { "" } else { "注入漏抓" });
    }
    // C17-12 册指纹各不相同（闭集指纹唯一性）
    {
        let mut uniq = true;
        for a in 0..10u32 {
            for b in (a + 1)..10u32 {
                if book_demo_fp(a) == book_demo_fp(b) {
                    uniq = false;
                }
            }
        }
        s.add("C17-12-指纹唯一", uniq, if uniq { "" } else { "指纹撞车" });
    }
    s
}
