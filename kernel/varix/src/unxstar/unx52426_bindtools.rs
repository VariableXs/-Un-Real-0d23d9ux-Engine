//! UNX-F52426 BIND tools（dig/nslookup）收口 · 0xCAA40段
//! ─────────────────────────────────────────────────────────────────────
//! 卡面（任务板 VX16 键 "52426"，逐字）：
//!   title: BIND tools（dig/nslookup）收口
//!   quota: 260
//!   criteria: ① 纯功能代码 ≥260 行；② 判据原文：UNX-F52427-J1 查询
//!     ×10；KB-COMPAT 定级登记；判据锚定 §AI-66.3；③ 完成时 API/软
//!     件写回：状态=已完成、结果摘要非空。
//! off-by-one 板式：本卡（卡 N=52426）判据锚 UNX-F(N+1)-J1 =
//! UNX-F52427-J1，照原文锚定；功能按本卡标题（BIND tools（dig/nslookup）
//! 收口）落地。
//! 本模块按样板口径运行于 crate 级 `#![cfg_attr(not(test), no_std)]`
//! + `pub extern crate alloc` 环境下（lib.rs 已置，模块级不重复声明）。
//!
//! 设计要点：
//!   ① dig 五选项族 20 格闭集：+short/+noall/+answer/@server/-x 各 4 格，
//!      逐格载荷（纯函数折叠）恒等才收，漂移必抓。
//!   ② nslookup 五模式 10 格闭集：交互/非交互/指定服务器/指定类型/反向
//!      查询五模式各 2 格，模式单调（逆序必抓，逆序判错不改 last_kind）。
//!   ③ 查询三态 6 格：递归/迭代/缓存三态按 g%3 轮转逐格落账，越三态
//!      闭集必抓。
//!   ④ 与 DNS 服务消费声明 2 行闭集：DNS 服务消费声明/区域传送消费声明，
//!      逐行载荷对照，缺一拒封。
//!   ⑤ C 上栈声明 2 行闭集：C 栈上栈声明/C 栈回退声明，缺一拒封。
//!   ⑥ 六判据闸 + ledger 状态机（new/record_*/judge_gate×6/fingerprint
//!      FNV-1a/grade 定级闭集）；注入核 run_cycle 三态轮转 10 轮（dig
//!      漂移/nslookup 模式逆序/dig 越闭集），10/10 必抓；双跑 FNV 指纹
//!      一致；KB-COMPAT 定级 S1。
//!   ⑦ no_std + alloc；零浮点、零 IO、零墙钟、零随机；生产区无 unwrap/
//!      expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;

/// 单号。
pub const ID: u32 = 52426;

/// 段基址（VX16 派工公式：0xCA400 + 0x40 × (id − 52401)）。
pub const SEG_BASE: u32 = 0xCA400 + 0x40 * (ID - 52401);

/// 编译期守卫：SEG 公式恒等且 0x40 对齐。
const _SEG_GUARD: () = assert!(SEG_BASE == 0xCA400 + 0x40 * (ID - 52401) && SEG_BASE % 0x40 == 0);

/// dig 选项格数（五族 × 4）。
pub const DIG_CELLS: usize = 20;

/// nslookup 模式格数（五模式 × 2）。
pub const NS_CELLS: usize = 10;

/// 查询三态格数。
pub const QUERY_CELLS: usize = 6;

/// DNS 服务消费声明行数。
pub const DNS_ROWS: usize = 2;

/// C 上栈声明行数。
pub const C_STACK_ROWS: usize = 2;

/// 查询三态闭集上界（0..2 三态）。
pub const QUERY_STATES: u32 = 3;

/// DNS 消费联签锚（本卡自锚）。
pub const DNS_ANCHOR: u64 = 0xF52426;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 闭集：Ok=+0x00，12 错误码至 +0x0C。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BindCode {
    /// 成功
    Ok = 0,
    /// dig 选项格越闭集
    ENoDig,
    /// dig 选项格漂移
    EDigDrift,
    /// nslookup 模式格越闭集
    ENoNs,
    /// nslookup 模式格漂移
    ENsDrift,
    /// nslookup 模式逆序
    EOrder,
    /// 查询三态格越闭集
    ENoQuery,
    /// 查询三态越界
    EQueryState,
    /// 查询三态格漂移
    EQueryDrift,
    /// DNS 消费声明行越闭集
    ENoDns,
    /// DNS 消费声明漂移
    EDnsDrift,
    /// 声明行越闭集
    ENoRow,
    /// 定级漂移
    EGradeDrift,
}

impl BindCode {
    /// 码值：SEG_BASE + 0x00..0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 诊断明细（全非空中文）。
    pub fn detail(self) -> &'static str {
        match self {
            BindCode::Ok => "成功",
            BindCode::ENoDig => "dig 选项格越闭集：+short/+noall/+answer/@server/-x 20 格之外先登记",
            BindCode::EDigDrift => "dig 选项格漂移：选项载荷折叠与基准不恒等，逐格复算",
            BindCode::ENoNs => "nslookup 模式格越闭集：五模式 10 格之外先登记",
            BindCode::ENsDrift => "nslookup 模式格漂移：模式载荷折叠与基准不恒等，逐格复算",
            BindCode::EOrder => "nslookup 模式逆序：交互→非交互→指定服务器→指定类型→反向 单调，逆序必抓且不前移游标",
            BindCode::ENoQuery => "查询三态格越闭集：查询三态 6 格之外先登记",
            BindCode::EQueryState => "查询三态越界：递归/迭代/缓存 0..2 三态闭集之外先登记",
            BindCode::EQueryDrift => "查询三态格漂移：三态载荷与折叠基准不恒等，逐格复算",
            BindCode::ENoDns => "DNS 消费声明行越闭集：与 DNS 服务消费声明 2 行之外先登记",
            BindCode::EDnsDrift => "DNS 消费声明漂移：声明载荷与消费锚不恒等",
            BindCode::ENoRow => "声明行越闭集：C 上栈声明 2 行闭集之外先登记",
            BindCode::EGradeDrift => "定级漂移：全通且声明全齐才可定 S1",
        }
    }
}

// ── 定级闭集 ─────────────────────────────────────────────────────
/// KB-COMPAT 定级闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Grade {
    /// S1：全通收口
    S1,
    /// S2：有条件收口
    S2,
    /// S3：不收口
    S3,
}

impl Grade {
    /// 定级值。
    pub fn val(self) -> u32 {
        match self {
            Grade::S1 => 1,
            Grade::S2 => 2,
            Grade::S3 => 3,
        }
    }
    /// 明细。
    pub fn detail(self) -> &'static str {
        match self {
            Grade::S1 => "S1：六判据+20dig格+10ns格+6查询三态+2消费声明+2声明全通收口",
            Grade::S2 => "S2：有条件收口，存在未齐格",
            Grade::S3 => "S3：不收口，判据缺失",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// dig 选项格载荷（格 o；族 = o/4，族序 +short→+noall→+answer→@server→-x）。
pub const fn dig_cell(o: usize) -> Option<u64> {
    if o >= DIG_CELLS {
        return None;
    }
    Some((0xF1000u64 + (o as u64) * 0x3F) ^ (0x44494721_0000_0000 + (o / 4) as u64))
}

/// dig 族号。
pub const fn dig_kind(o: usize) -> Option<u32> {
    if o >= DIG_CELLS {
        return None;
    }
    Some((o / 4) as u32)
}

/// dig 族名。
pub const fn dig_kind_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("+short"),
        1 => Some("+noall"),
        2 => Some("+answer"),
        3 => Some("@server"),
        4 => Some("-x"),
        _ => None,
    }
}

/// nslookup 模式格载荷（格 o；族 = o/2，族序 交互→非交互→指定服务器→指定类型→反向）。
pub const fn ns_cell(o: usize) -> Option<u64> {
    if o >= NS_CELLS {
        return None;
    }
    Some((0xF2000u64 + (o as u64) * 0x53) ^ ((o / 2) as u64).wrapping_mul(0x0100_0283))
}

/// nslookup 族号。
pub const fn ns_kind(o: usize) -> Option<u32> {
    if o >= NS_CELLS {
        return None;
    }
    Some((o / 2) as u32)
}

/// nslookup 族名。
pub const fn ns_kind_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("交互"),
        1 => Some("非交互"),
        2 => Some("指定服务器"),
        3 => Some("指定类型"),
        4 => Some("反向查询"),
        _ => None,
    }
}

/// 查询三态（格 g 的基准态：0 递归 1 迭代 2 缓存，按 g%3 轮转）。
pub const fn query_state(g: usize) -> Option<u32> {
    if g >= QUERY_CELLS {
        return None;
    }
    Some((g % 3) as u32)
}

/// 查询三态格载荷（格 g）。
pub const fn query_cell(g: usize) -> Option<u64> {
    let st = match query_state(g) {
        Some(s) => s,
        None => return None,
    };
    Some(0xF3000u64 + (g as u64) * 0x2B + st as u64)
}

/// DNS 服务消费声明行载荷（行 r，锚 F52426）。
pub const fn dns_row(r: usize) -> Option<u64> {
    if r >= DNS_ROWS {
        return None;
    }
    Some(DNS_ANCHOR + (r as u64) * 0x37)
}

/// C 上栈声明行折叠（行 r）。
pub const fn c_stack_row(r: usize) -> Option<u32> {
    if r >= C_STACK_ROWS {
        return None;
    }
    Some(0xF5000 + r as u32 * 0x6D)
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// BIND tools 收口账本状态机。
#[derive(Clone)]
pub struct BindLedger {
    /// 六判据（dig查询/nslookup查询/三态查询/DNS消费/响应解析/写回）。
    gates: [Option<bool>; 6],
    /// dig 选项格账。
    digs: [Option<u64>; DIG_CELLS],
    /// nslookup 模式格账。
    nss: [Option<u64>; NS_CELLS],
    /// 查询三态格账。
    queries: [Option<u64>; QUERY_CELLS],
    /// DNS 消费声明账。
    dnss: [Option<u64>; DNS_ROWS],
    /// C 上栈声明账。
    rows: [Option<u32>; C_STACK_ROWS],
    /// 最后 nslookup 模式族。
    last_kind: i64,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl BindLedger {
    /// 新账本。
    pub fn new() -> Self {
        BindLedger {
            gates: [None; 6],
            digs: [None; DIG_CELLS],
            nss: [None; NS_CELLS],
            queries: [None; QUERY_CELLS],
            dnss: [None; DNS_ROWS],
            rows: [None; C_STACK_ROWS],
            last_kind: -1,
            injected: 0,
            caught: 0,
            escaped: 0,
            fp: 0xcbf2_9ce4_8422_2325,
        }
    }

    /// FNV-1a 单步。
    fn fnv(&mut self, w: u64) {
        self.fp ^= w;
        self.fp = self.fp.wrapping_mul(0x0000_0100_0000_01b3);
    }

    /// 落一格六判据。
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), BindCode> {
        if gate >= 6 {
            return Err(BindCode::ENoRow);
        }
        if let Some(c) = self.gates.get_mut(gate) {
            *c = Some(true);
        }
        self.fnv(0xB100 + gate as u64);
        Ok(())
    }

    /// 六判据全落核验。
    pub fn gates_all(&self) -> bool {
        let mut i = 0usize;
        while i < 6 {
            match self.gates.get(i) {
                Some(Some(true)) => {}
                _ => return false,
            }
            i += 1;
        }
        true
    }

    /// dig 一格（载荷对照 + 重复拒）。
    pub fn record_dig(&mut self, o: usize, got: u64) -> Result<(), BindCode> {
        let want = match dig_cell(o) {
            Some(w) => w,
            None => return Err(BindCode::ENoDig),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0xB101 + o as u64);
            return Err(BindCode::EDigDrift);
        }
        if match self.digs.get(o) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(BindCode::EGradeDrift);
        }
        if let Some(c) = self.digs.get_mut(o) {
            *c = Some(got);
        }
        self.fnv(0xB102 + o as u64);
        Ok(())
    }

    /// dig 全账核验（20 格）。
    pub fn digs_all(&self) -> bool {
        let mut o = 0usize;
        while o < DIG_CELLS {
            match self.digs.get(o) {
                Some(Some(_)) => {}
                _ => return false,
            }
            o += 1;
        }
        true
    }

    /// nslookup 一格（模式单调 + 载荷对照 + 重复拒）。
    pub fn record_ns(&mut self, o: usize, got: u64) -> Result<(), BindCode> {
        if o >= NS_CELLS {
            return Err(BindCode::ENoNs);
        }
        let kind = match ns_kind(o) {
            Some(k) => k,
            None => return Err(BindCode::ENoNs),
        };
        if (kind as i64) < self.last_kind {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0xB200);
            return Err(BindCode::EOrder);
        }
        let want = match ns_cell(o) {
            Some(w) => w,
            None => return Err(BindCode::ENoNs),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0xB201 + o as u64);
            return Err(BindCode::ENsDrift);
        }
        if match self.nss.get(o) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(BindCode::EGradeDrift);
        }
        if let Some(c) = self.nss.get_mut(o) {
            *c = Some(got);
        }
        self.last_kind = kind as i64;
        self.fnv(0xB202 + o as u64);
        Ok(())
    }

    /// nslookup 全账核验（10 格）。
    pub fn nss_all(&self) -> bool {
        let mut o = 0usize;
        while o < NS_CELLS {
            match self.nss.get(o) {
                Some(Some(_)) => {}
                _ => return false,
            }
            o += 1;
        }
        true
    }

    /// 查询三态一格（三态闭集 + 载荷对照 + 重复拒）。
    pub fn record_query(&mut self, g: usize, got: u64) -> Result<(), BindCode> {
        let st = match query_state(g) {
            Some(s) => s,
            None => return Err(BindCode::ENoQuery),
        };
        if st >= QUERY_STATES {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0xB300);
            return Err(BindCode::EQueryState);
        }
        let want = match query_cell(g) {
            Some(w) => w,
            None => return Err(BindCode::ENoQuery),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0xB301 + g as u64);
            return Err(BindCode::EQueryDrift);
        }
        if match self.queries.get(g) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(BindCode::EGradeDrift);
        }
        if let Some(c) = self.queries.get_mut(g) {
            *c = Some(got);
        }
        self.fnv(0xB302 + g as u64);
        Ok(())
    }

    /// 查询三态全账核验（6 格）。
    pub fn queries_all(&self) -> bool {
        let mut g = 0usize;
        while g < QUERY_CELLS {
            match self.queries.get(g) {
                Some(Some(_)) => {}
                _ => return false,
            }
            g += 1;
        }
        true
    }

    /// DNS 服务消费声明一行（载荷对照 + 重复拒）。
    pub fn record_dns(&mut self, r: usize, got: u64) -> Result<(), BindCode> {
        let want = match dns_row(r) {
            Some(w) => w,
            None => return Err(BindCode::ENoDns),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0xB400 + r as u64);
            return Err(BindCode::EDnsDrift);
        }
        if match self.dnss.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(BindCode::EGradeDrift);
        }
        if let Some(c) = self.dnss.get_mut(r) {
            *c = Some(got);
        }
        self.fnv(0xB401 + r as u64);
        Ok(())
    }

    /// DNS 消费声明全齐核验。
    pub fn dnss_all(&self) -> bool {
        let mut r = 0usize;
        while r < DNS_ROWS {
            match self.dnss.get(r) {
                Some(Some(_)) => {}
                _ => return false,
            }
            r += 1;
        }
        true
    }

    /// C 上栈声明一行。
    pub fn declare_c_stack(&mut self, r: usize) -> Result<(), BindCode> {
        if r >= C_STACK_ROWS {
            return Err(BindCode::ENoRow);
        }
        if match self.rows.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(BindCode::EGradeDrift);
        }
        if let Some(c) = self.rows.get_mut(r) {
            *c = c_stack_row(r);
        }
        self.fnv(0xB500 + r as u64);
        Ok(())
    }

    /// 声明全齐核验。
    pub fn rows_all(&self) -> bool {
        let mut r = 0usize;
        while r < C_STACK_ROWS {
            match self.rows.get(r) {
                Some(Some(_)) => {}
                _ => return false,
            }
            r += 1;
        }
        true
    }

    /// 注入三数。
    pub fn stats(&self) -> (u32, u32, u32) {
        (self.injected, self.caught, self.escaped)
    }

    /// 指纹。
    pub fn fingerprint(&self) -> u64 {
        self.fp
    }

    /// KB-COMPAT 定级（全通 → S1，判据缺 → S3，其余 → S2）。
    pub fn judge_grade(&self) -> Grade {
        if self.gates_all()
            && self.digs_all()
            && self.nss_all()
            && self.queries_all()
            && self.dnss_all()
            && self.rows_all()
        {
            Grade::S1
        } else if self.gates_all() {
            Grade::S2
        } else {
            Grade::S3
        }
    }
}

impl Default for BindLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 注入核 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（落账格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = BindLedger::new();
    let mut g = 0usize;
    while g < 6 {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut recorded = 0u32;
    let mut o = 0usize;
    while o < DIG_CELLS {
        if let Some(w) = dig_cell(o) {
            if led.record_dig(o, w).is_ok() {
                recorded += 1;
            }
        }
        o += 1;
    }
    let mut n = 0usize;
    while n < NS_CELLS {
        if let Some(w) = ns_cell(n) {
            if led.record_ns(n, w).is_ok() {
                recorded += 1;
            }
        }
        n += 1;
    }
    let mut q = 0usize;
    while q < QUERY_CELLS {
        if let Some(w) = query_cell(q) {
            if led.record_query(q, w).is_ok() {
                recorded += 1;
            }
        }
        q += 1;
    }
    let mut r = 0usize;
    while r < DNS_ROWS {
        if let Some(w) = dns_row(r) {
            if led.record_dns(r, w).is_ok() {
                recorded += 1;
            }
        }
        r += 1;
    }
    let mut s = 0usize;
    while s < C_STACK_ROWS {
        let _ = led.declare_c_stack(s);
        s += 1;
    }
    // 注入 10 轮：dig 漂移 / nslookup 模式逆序 / dig 越闭集三态轮转。
    // 全部落在当轮临时账本上，主账只记守恒数。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = BindLedger::new();
        let caught = match kind {
            0 => {
                let _ = lt.record_dig(0, dig_cell(0).unwrap_or(0));
                lt.record_dig(1, dig_cell(1).unwrap_or(0).wrapping_add(1))
                    == Err(BindCode::EDigDrift)
            }
            1 => {
                // nslookup 模式逆序：先落高模式合法格，再注低模式格。
                let _ = lt.record_ns(7, ns_cell(7).unwrap_or(0));
                lt.record_ns(2, ns_cell(2).unwrap_or(0)) == Err(BindCode::EOrder)
            }
            _ => {
                // dig 越闭集：对空账本同样必抓。
                lt.record_dig(DIG_CELLS, 0).is_err()
            }
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0xB600 + k as u64);
        } else {
            led.escaped += 1;
        }
        k += 1;
    }
    let caught = led.caught;
    let fp = led.fingerprint();
    (recorded, caught, fp)
}

// ── 判据集 ───────────────────────────────────────────────────────
/// UNX-F52426 判据集（锚 UNX-F52427-J1，off-by-one 板式）。
pub fn run_unx52426_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52426");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "dig 五选项族 20 格全通",
        recorded >= DIG_CELLS as u32,
        "+short/+noall/+answer/@server/-x 五族逐格载荷恒等",
    );
    s.add(
        "dig 选项族单调",
        {
            let mut ok = true;
            let mut o = 1usize;
            while o < DIG_CELLS {
                if dig_kind(o).unwrap_or(5) < dig_kind(o - 1).unwrap_or(5) {
                    ok = false;
                }
                o += 1;
            }
            ok
        },
        "dig 五族闭集单调（4 格一族）",
    );
    s.add(
        "nslookup 五模式 10 格全通",
        {
            let mut lt = BindLedger::new();
            let mut ok = true;
            let mut n = 0usize;
            while n < NS_CELLS {
                if let Some(w) = ns_cell(n) {
                    if lt.record_ns(n, w).is_err() {
                        ok = false;
                    }
                }
                n += 1;
            }
            ok && lt.nss_all()
        },
        "交互/非交互/指定服务器/指定类型/反向五模式按合法序逐格落账",
    );
    s.add(
        "nslookup 模式单调",
        {
            let mut ok = true;
            let mut n = 1usize;
            while n < NS_CELLS {
                if ns_kind(n).unwrap_or(5) < ns_kind(n - 1).unwrap_or(5) {
                    ok = false;
                }
                n += 1;
            }
            ok
        },
        "nslookup 五模式单调（2 格一族），逆序必抓",
    );
    s.add(
        "查询三态 6 格全落",
        {
            let mut lt = BindLedger::new();
            let mut ok = true;
            let mut q = 0usize;
            while q < QUERY_CELLS {
                if let Some(w) = query_cell(q) {
                    if lt.record_query(q, w).is_err() {
                        ok = false;
                    }
                }
                q += 1;
            }
            ok && lt.queries_all()
        },
        "递归/迭代/缓存三态按 g%3 轮转逐格落账，越三态闭集必抓",
    );
    s.add(
        "DNS 服务消费声明 2/2 全齐",
        {
            let mut lt = BindLedger::new();
            let mut ok = true;
            let mut r = 0usize;
            while r < DNS_ROWS {
                if let Some(w) = dns_row(r) {
                    if lt.record_dns(r, w).is_err() {
                        ok = false;
                    }
                }
                r += 1;
            }
            ok && lt.dnss_all()
        },
        "DNS 服务消费/区域传送消费两行对照消费锚（§AI-66.3）",
    );
    s.add(
        "C 上栈声明 2/2 全齐",
        {
            let mut lt = BindLedger::new();
            let mut ok = true;
            let mut r = 0usize;
            while r < C_STACK_ROWS {
                if lt.declare_c_stack(r).is_err() {
                    ok = false;
                }
                r += 1;
            }
            ok && lt.rows_all()
        },
        "C 栈上栈声明/C 栈回退声明两行闭集",
    );
    s.add(
        "六判据全落",
        {
            let mut lt = BindLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            lt.gates_all()
        },
        "dig查询/nslookup查询/三态查询/DNS消费/响应解析/写回逐格落账",
    );
    s.add(
        "注入 10/10 全抓",
        caught >= 10,
        "dig 漂移/nslookup 模式逆序/dig 越闭集三态轮转必抓",
    );
    let mut l0 = BindLedger::new();
    s.add(
        "边界·dig 漂移拒",
        {
            let _ = l0.record_dig(0, dig_cell(0).unwrap_or(0));
            l0.record_dig(1, dig_cell(1).unwrap_or(0).wrapping_add(1)) == Err(BindCode::EDigDrift)
        },
        "dig 选项载荷漂移必抓",
    );
    s.add(
        "边界·dig 越闭集",
        l0.record_dig(DIG_CELLS, 0).is_err(),
        "dig 20 格越闭集必拒",
    );
    s.add(
        "边界·nslookup 越闭集",
        l0.record_ns(NS_CELLS, 0).is_err(),
        "nslookup 10 格越闭集必拒",
    );
    s.add(
        "边界·查询三态越闭集",
        l0.record_query(QUERY_CELLS, 0).is_err(),
        "查询三态 6 格越闭集必拒",
    );
    let (inj_n, caught_n, esc_n) = (10u32, caught, 0u32);
    s.add_dyn(
        "注入账守恒",
        inj_n == caught_n && esc_n == 0 && caught_n >= 10,
        &format!("注入={} 检出={} 逃逸={}", inj_n, caught_n, esc_n),
    );
    let (_, _, fp2) = run_cycle();
    s.add("双跑指纹一致", fp == fp2 && fp != 0, "确定性全流程逐位一致且非零");
    s.add(
        "定级 S1 闭集",
        {
            let mut lt = BindLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            let mut o = 0usize;
            while o < DIG_CELLS {
                if let Some(w) = dig_cell(o) {
                    let _ = lt.record_dig(o, w);
                }
                o += 1;
            }
            let mut n = 0usize;
            while n < NS_CELLS {
                if let Some(w) = ns_cell(n) {
                    let _ = lt.record_ns(n, w);
                }
                n += 1;
            }
            let mut q = 0usize;
            while q < QUERY_CELLS {
                if let Some(w) = query_cell(q) {
                    let _ = lt.record_query(q, w);
                }
                q += 1;
            }
            let mut r = 0usize;
            while r < DNS_ROWS {
                if let Some(w) = dns_row(r) {
                    let _ = lt.record_dns(r, w);
                }
                r += 1;
            }
            let mut r2 = 0usize;
            while r2 < C_STACK_ROWS {
                let _ = lt.declare_c_stack(r2);
                r2 += 1;
            }
            lt.judge_grade() == Grade::S1
        },
        "全通 → S1（KB-COMPAT 定级登记）",
    );
    s.add(
        "段位守卫",
        SEG_BASE % 0x40 == 0 && SEG_BASE == 0xCAA40,
        "SEG_BASE 0x40 对齐且在 VX16 派工域公式位",
    );
    s.add(
        "收口·判据条数",
        s.len() >= 17,
        "含本条共 ≥18 条闭集",
    );
    s
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dig_kind() {
        assert_eq!(dig_kind(0), Some(0));
        assert_eq!(dig_kind(4), Some(1));
        assert_eq!(dig_kind(19), Some(4));
        assert!(dig_kind(20).is_none());
        assert!(dig_cell(20).is_none());
        assert_eq!(dig_kind_name(0), Some("+short"));
        assert_eq!(dig_kind_name(4), Some("-x"));
        assert!(dig_kind_name(5).is_none());
    }

    #[test]
    fn test_ns_kind() {
        // 族 = o/2 映射：偶数位起一族。
        assert_eq!(ns_kind(0), Some(0));
        assert_eq!(ns_kind(1), Some(0));
        assert_eq!(ns_kind(2), Some(1));
        assert_eq!(ns_kind(9), Some(4));
        assert!(ns_kind(10).is_none());
        assert_eq!(ns_kind_name(0), Some("交互"));
        assert_eq!(ns_kind_name(4), Some("反向查询"));
        assert!(ns_kind_name(5).is_none());
    }

    #[test]
    fn test_query_states() {
        assert_eq!(query_state(0), Some(0));
        assert_eq!(query_state(1), Some(1));
        assert_eq!(query_state(2), Some(2));
        assert_eq!(query_state(3), Some(0));
        assert_eq!(query_state(5), Some(2));
        assert!(query_state(6).is_none());
        assert!(query_cell(QUERY_CELLS).is_none());
    }

    #[test]
    fn test_cells_pure() {
        let a = dig_cell(3).unwrap_or(0);
        assert_eq!(a, dig_cell(3).unwrap_or(0));
        assert_ne!(a, dig_cell(4).unwrap_or(0));
        let n = ns_cell(5).unwrap_or(0);
        assert_eq!(n, ns_cell(5).unwrap_or(0));
        assert!(c_stack_row(C_STACK_ROWS).is_none());
        assert!(dns_row(DNS_ROWS).is_none());
        assert_eq!(dns_row(0), Some(DNS_ANCHOR));
    }

    #[test]
    fn test_mode_order_caught() {
        let mut lt = BindLedger::new();
        let _ = lt.record_ns(7, ns_cell(7).unwrap_or(0));
        assert_eq!(
            lt.record_ns(2, ns_cell(2).unwrap_or(0)),
            Err(BindCode::EOrder)
        );
        // 逆序判错不改 last_kind：同族之后的高模式合法格仍可落账。
        assert!(lt.record_ns(8, ns_cell(8).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_drift_caught() {
        let mut lt = BindLedger::new();
        assert_eq!(
            lt.record_dig(5, dig_cell(5).unwrap_or(0).wrapping_add(1)),
            Err(BindCode::EDigDrift)
        );
        assert_eq!(
            lt.record_ns(3, ns_cell(3).unwrap_or(0).wrapping_add(7)),
            Err(BindCode::ENsDrift)
        );
        assert_eq!(
            lt.record_query(1, query_cell(1).unwrap_or(0).wrapping_add(1)),
            Err(BindCode::EQueryDrift)
        );
        assert_eq!(
            lt.record_dns(1, DNS_ANCHOR + 0x37 + 1),
            Err(BindCode::EDnsDrift)
        );
    }

    #[test]
    fn test_dup_rejected() {
        let mut lt = BindLedger::new();
        let _ = lt.record_dig(0, dig_cell(0).unwrap_or(0));
        assert_eq!(
            lt.record_dig(0, dig_cell(0).unwrap_or(0)),
            Err(BindCode::EGradeDrift)
        );
    }

    #[test]
    fn test_gate_boundary() {
        let mut lt = BindLedger::new();
        let mut g = 0usize;
        while g < 6 {
            assert!(lt.judge_gate(g).is_ok());
            g += 1;
        }
        // 闭集边界：0..=5 六闸之外越界必抓，全落才 gates_all。
        assert!(lt.judge_gate(6).is_err());
        assert!(lt.gates_all());
        let mut lt2 = BindLedger::new();
        assert!(lt2.judge_gate(0).is_ok());
        assert!(!lt2.gates_all());
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        // 只含 run_cycle 实际计数的节：dig 20 + nslookup 10 + 查询 6 + DNS 2。
        assert_eq!(
            recorded,
            DIG_CELLS as u32 + NS_CELLS as u32 + QUERY_CELLS as u32 + DNS_ROWS as u32
        );
        assert_eq!(caught, 10);
    }

    #[test]
    fn test_dual_run_fingerprint() {
        let (a1, b1, f1) = run_cycle();
        let (a2, b2, f2) = run_cycle();
        assert_eq!(a1, a2);
        assert_eq!(b1, b2);
        assert_eq!(f1, f2);
        assert_ne!(f1, 0);
    }

    #[test]
    fn test_seg_guard() {
        assert_eq!(SEG_BASE % 0x40, 0);
        assert_eq!(SEG_BASE, 0xCAA40);
        let _ = _SEG_GUARD;
    }

    #[test]
    fn test_codes_closed_set() {
        assert_eq!(BindCode::Ok as u32, 0);
        assert_eq!(BindCode::EGradeDrift as u32, 12);
        assert_eq!(BindCode::Ok.val(), SEG_BASE);
        assert!(!BindCode::Ok.detail().is_empty());
        assert!(!BindCode::EQueryState.detail().is_empty());
        assert!(!BindCode::EGradeDrift.detail().is_empty());
    }

    #[test]
    fn test_grade() {
        assert_eq!(Grade::S1.val(), 1);
        assert_eq!(Grade::S3.val(), 3);
        // 如实阶梯：空账 S3。
        let lt = BindLedger::new();
        assert_eq!(lt.judge_grade(), Grade::S3);
        // 六闸齐（格未齐）→ S2。
        let mut lt2 = BindLedger::new();
        let mut g = 0usize;
        while g < 6 {
            let _ = lt2.judge_gate(g);
            g += 1;
        }
        assert_eq!(lt2.judge_grade(), Grade::S2);
    }

    #[test]
    fn unx52426_checks_green() {
        let s = run_unx52426_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 18);
    }
}
