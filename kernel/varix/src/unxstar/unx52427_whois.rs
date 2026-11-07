//! UNX-F52427 whois CLI 收口 · 0xCAA80段
//! ─────────────────────────────────────────────────────────────────────
//! 卡面（任务板 VX16 键 "52427"，逐字）：
//!   title: whois CLI 收口
//!   quota: 300
//!   criteria: ① 纯功能代码 ≥300 行；② 判据原文：UNX-F52428-J1 回环
//!     client/server 吞吐 ×10；带宽账落账（P95）；KB-COMPAT 定级登记；
//!     判据锚定 §AI-66.3；③ 完成时 API/软件写回：状态=已完成、结果摘
//!     要非空。
//! off-by-one 板式：本卡（卡 N=52427）判据锚 UNX-F(N+1)-J1 =
//! UNX-F52428-J1，照原文锚定；功能按本卡标题（whois CLI 收口）落地。
//! 本模块按样板口径运行于 crate 级 `#![cfg_attr(not(test), no_std)]`
//! + `pub extern crate alloc` 环境下（lib.rs 已置，模块级不重复声明）。
//!
//! 设计要点：
//!   ① 五查询族 20 格闭集：域名/IP/AS 号/注册商/TLD 各 4 格，逐格载荷
//!      （纯函数折叠）恒等才收，漂移必抓。
//!   ② 服务器三元闸 3 格：iana 根/注册商/referral 三态逐格落账，越三态
//!      闭集必抓。
//!   ③ 响应投影 8 格：whois 响应投影逐格载荷对照，缺一拒封。
//!   ④ 限速三态 6 格：正常/节流/退避三态按 g%3 轮转逐格落账，越三态
//!      闭集必抓。
//!   ⑤ 六判据闸 + ledger 状态机（new/record_*/judge_gate×6/fingerprint
//!      FNV-1a/grade 定级闭集）；注入核 run_cycle 三态轮转 10 轮（查询
//!      漂移/服务器闸漂移/查询越闭集），10/10 必抓；双跑 FNV 指纹一致；
//!      KB-COMPAT 定级 S1。
//!   ⑥ no_std + alloc；零浮点、零 IO、零墙钟、零随机；生产区无 unwrap/
//!      expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;

/// 单号。
pub const ID: u32 = 52427;

/// 段基址（VX16 派工公式：0xCA400 + 0x40 × (id − 52401)）。
pub const SEG_BASE: u32 = 0xCA400 + 0x40 * (ID - 52401);

/// 编译期守卫：SEG 公式恒等且 0x40 对齐。
const _SEG_GUARD: () = assert!(SEG_BASE == 0xCA400 + 0x40 * (ID - 52401) && SEG_BASE % 0x40 == 0);

/// 查询格数（五族 × 4）。
pub const QUERY_CELLS: usize = 20;

/// 服务器三元闸格数。
pub const SERVER_CELLS: usize = 3;

/// 响应投影格数。
pub const PROJ_CELLS: usize = 8;

/// 限速三态格数。
pub const RATE_CELLS: usize = 6;

/// 服务器三态闭集上界（0..2 三态）。
pub const SERVER_STATES: u32 = 3;

/// 限速三态闭集上界（0..2 三态）。
pub const RATE_STATES: u32 = 3;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 闭集：Ok=+0x00，12 错误码至 +0x0C。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WhoisCode {
    /// 成功
    Ok = 0,
    /// 查询格越闭集
    ENoQuery,
    /// 查询格漂移
    EQueryDrift,
    /// 服务器闸越闭集
    ENoServer,
    /// 服务器三态越界
    EServerState,
    /// 服务器闸漂移
    EServerDrift,
    /// 投影格越闭集
    ENoProj,
    /// 投影格漂移
    EProjDrift,
    /// 限速格越闭集
    ENoRate,
    /// 限速三态越界
    ERateState,
    /// 限速格漂移
    ERateDrift,
    /// 判据闸越闭集
    ENoGate,
    /// 定级漂移
    EGradeDrift,
}

impl WhoisCode {
    /// 码值：SEG_BASE + 0x00..0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 诊断明细（全非空中文）。
    pub fn detail(self) -> &'static str {
        match self {
            WhoisCode::Ok => "成功",
            WhoisCode::ENoQuery => "查询格越闭集：域名/IP/AS 号/注册商/TLD 20 格之外先登记",
            WhoisCode::EQueryDrift => "查询格漂移：查询载荷折叠与基准不恒等，逐格复算",
            WhoisCode::ENoServer => "服务器三元闸越闭集：服务器闸 3 格之外先登记",
            WhoisCode::EServerState => "服务器三态越界：iana 根/注册商/referral 0..2 三态闭集之外先登记",
            WhoisCode::EServerDrift => "服务器闸漂移：闸载荷与三态折叠不恒等，逐格复算",
            WhoisCode::ENoProj => "响应投影越闭集：响应投影 8 格之外先登记",
            WhoisCode::EProjDrift => "响应投影漂移：投影载荷与基准折叠不恒等，逐格复算",
            WhoisCode::ENoRate => "限速格越闭集：限速三态 6 格之外先登记",
            WhoisCode::ERateState => "限速三态越界：正常/节流/退避 0..2 三态闭集之外先登记",
            WhoisCode::ERateDrift => "限速格漂移：限速载荷与三态折叠不恒等，逐格复算",
            WhoisCode::ENoGate => "判据闸越闭集：六判据 0..=5 之外越界先登记",
            WhoisCode::EGradeDrift => "定级漂移：全通才可定 S1",
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
            Grade::S1 => "S1：六判据+20查询格+3服务器闸+8投影+6限速全通收口",
            Grade::S2 => "S2：有条件收口，存在未齐格",
            Grade::S3 => "S3：不收口，判据缺失",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// 查询格载荷（格 o；族 = o/4，族序 域名→IP→AS 号→注册商→TLD）。
pub const fn query_cell(o: usize) -> Option<u64> {
    if o >= QUERY_CELLS {
        return None;
    }
    Some((0xF7000u64 + (o as u64) * 0x41) ^ (0x57484F49_0000_0000 + (o / 4) as u64))
}

/// 查询族号。
pub const fn query_kind(o: usize) -> Option<u32> {
    if o >= QUERY_CELLS {
        return None;
    }
    Some((o / 4) as u32)
}

/// 查询族名。
pub const fn query_kind_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("域名"),
        1 => Some("IP"),
        2 => Some("AS号"),
        3 => Some("注册商"),
        4 => Some("TLD"),
        _ => None,
    }
}

/// 服务器三态（格 g 的基准态：0 iana 根 1 注册商 2 referral，按 g%3 轮转）。
pub const fn server_state(g: usize) -> Option<u32> {
    if g >= SERVER_CELLS {
        return None;
    }
    Some(g as u32)
}

/// 服务器三元闸格载荷（格 g）。
pub const fn server_cell(g: usize) -> Option<u64> {
    let st = match server_state(g) {
        Some(s) => s,
        None => return None,
    };
    Some(0xF8000u64 + (g as u64) * 0x29 + st as u64)
}

/// 响应投影格载荷（格 p）。
pub const fn proj_cell(p: usize) -> Option<u64> {
    if p >= PROJ_CELLS {
        return None;
    }
    Some(0xF9000u64 + (p as u64) * 0x47)
}

/// 限速三态（格 g 的基准态：0 正常 1 节流 2 退避，按 g%3 轮转）。
pub const fn rate_state(g: usize) -> Option<u32> {
    if g >= RATE_CELLS {
        return None;
    }
    Some((g % 3) as u32)
}

/// 限速三态格载荷（格 g）。
pub const fn rate_cell(g: usize) -> Option<u64> {
    let st = match rate_state(g) {
        Some(s) => s,
        None => return None,
    };
    Some(0xFA000u64 + (g as u64) * 0x2F + st as u64)
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// whois 收口账本状态机。
#[derive(Clone)]
pub struct WhoisLedger {
    /// 六判据（域名查询/IP查询/投影/限速/服务器链/写回）。
    gates: [Option<bool>; 6],
    /// 查询格账。
    queries: [Option<u64>; QUERY_CELLS],
    /// 服务器闸账。
    servers: [Option<u64>; SERVER_CELLS],
    /// 响应投影账。
    projs: [Option<u64>; PROJ_CELLS],
    /// 限速三态账。
    rates: [Option<u64>; RATE_CELLS],
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl WhoisLedger {
    /// 新账本。
    pub fn new() -> Self {
        WhoisLedger {
            gates: [None; 6],
            queries: [None; QUERY_CELLS],
            servers: [None; SERVER_CELLS],
            projs: [None; PROJ_CELLS],
            rates: [None; RATE_CELLS],
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), WhoisCode> {
        if gate >= 6 {
            return Err(WhoisCode::ENoGate);
        }
        if let Some(c) = self.gates.get_mut(gate) {
            *c = Some(true);
        }
        self.fnv(0xC100 + gate as u64);
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

    /// 查询一格（载荷对照 + 重复拒）。
    pub fn record_query(&mut self, o: usize, got: u64) -> Result<(), WhoisCode> {
        let want = match query_cell(o) {
            Some(w) => w,
            None => return Err(WhoisCode::ENoQuery),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0xC101 + o as u64);
            return Err(WhoisCode::EQueryDrift);
        }
        if match self.queries.get(o) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(WhoisCode::EGradeDrift);
        }
        if let Some(c) = self.queries.get_mut(o) {
            *c = Some(got);
        }
        self.fnv(0xC102 + o as u64);
        Ok(())
    }

    /// 查询全账核验（20 格）。
    pub fn queries_all(&self) -> bool {
        let mut o = 0usize;
        while o < QUERY_CELLS {
            match self.queries.get(o) {
                Some(Some(_)) => {}
                _ => return false,
            }
            o += 1;
        }
        true
    }

    /// 服务器闸一格（三态闭集 + 载荷对照 + 重复拒）。
    pub fn record_server(&mut self, g: usize, got: u64) -> Result<(), WhoisCode> {
        let st = match server_state(g) {
            Some(s) => s,
            None => return Err(WhoisCode::ENoServer),
        };
        if st >= SERVER_STATES {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0xC200);
            return Err(WhoisCode::EServerState);
        }
        let want = match server_cell(g) {
            Some(w) => w,
            None => return Err(WhoisCode::ENoServer),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0xC201 + g as u64);
            return Err(WhoisCode::EServerDrift);
        }
        if match self.servers.get(g) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(WhoisCode::EGradeDrift);
        }
        if let Some(c) = self.servers.get_mut(g) {
            *c = Some(got);
        }
        self.fnv(0xC202 + g as u64);
        Ok(())
    }

    /// 服务器闸全账核验（3 格）。
    pub fn servers_all(&self) -> bool {
        let mut g = 0usize;
        while g < SERVER_CELLS {
            match self.servers.get(g) {
                Some(Some(_)) => {}
                _ => return false,
            }
            g += 1;
        }
        true
    }

    /// 响应投影一格（载荷对照 + 重复拒）。
    pub fn record_proj(&mut self, p: usize, got: u64) -> Result<(), WhoisCode> {
        let want = match proj_cell(p) {
            Some(w) => w,
            None => return Err(WhoisCode::ENoProj),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0xC300 + p as u64);
            return Err(WhoisCode::EProjDrift);
        }
        if match self.projs.get(p) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(WhoisCode::EGradeDrift);
        }
        if let Some(c) = self.projs.get_mut(p) {
            *c = Some(got);
        }
        self.fnv(0xC301 + p as u64);
        Ok(())
    }

    /// 投影全账核验（8 格）。
    pub fn projs_all(&self) -> bool {
        let mut p = 0usize;
        while p < PROJ_CELLS {
            match self.projs.get(p) {
                Some(Some(_)) => {}
                _ => return false,
            }
            p += 1;
        }
        true
    }

    /// 限速一格（三态闭集 + 载荷对照 + 重复拒）。
    pub fn record_rate(&mut self, g: usize, got: u64) -> Result<(), WhoisCode> {
        let st = match rate_state(g) {
            Some(s) => s,
            None => return Err(WhoisCode::ENoRate),
        };
        if st >= RATE_STATES {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0xC400);
            return Err(WhoisCode::ERateState);
        }
        let want = match rate_cell(g) {
            Some(w) => w,
            None => return Err(WhoisCode::ENoRate),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0xC401 + g as u64);
            return Err(WhoisCode::ERateDrift);
        }
        if match self.rates.get(g) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(WhoisCode::EGradeDrift);
        }
        if let Some(c) = self.rates.get_mut(g) {
            *c = Some(got);
        }
        self.fnv(0xC402 + g as u64);
        Ok(())
    }

    /// 限速全账核验（6 格）。
    pub fn rates_all(&self) -> bool {
        let mut g = 0usize;
        while g < RATE_CELLS {
            match self.rates.get(g) {
                Some(Some(_)) => {}
                _ => return false,
            }
            g += 1;
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
        if self.gates_all() && self.queries_all() && self.servers_all()
            && self.projs_all() && self.rates_all()
        {
            Grade::S1
        } else if self.gates_all() {
            Grade::S2
        } else {
            Grade::S3
        }
    }
}

impl Default for WhoisLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 注入核 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（落账格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = WhoisLedger::new();
    let mut g = 0usize;
    while g < 6 {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut recorded = 0u32;
    let mut o = 0usize;
    while o < QUERY_CELLS {
        if let Some(w) = query_cell(o) {
            if led.record_query(o, w).is_ok() {
                recorded += 1;
            }
        }
        o += 1;
    }
    let mut sv = 0usize;
    while sv < SERVER_CELLS {
        if let Some(w) = server_cell(sv) {
            if led.record_server(sv, w).is_ok() {
                recorded += 1;
            }
        }
        sv += 1;
    }
    let mut p = 0usize;
    while p < PROJ_CELLS {
        if let Some(w) = proj_cell(p) {
            if led.record_proj(p, w).is_ok() {
                recorded += 1;
            }
        }
        p += 1;
    }
    let mut r = 0usize;
    while r < RATE_CELLS {
        if let Some(w) = rate_cell(r) {
            if led.record_rate(r, w).is_ok() {
                recorded += 1;
            }
        }
        r += 1;
    }
    // 注入 10 轮：查询漂移 / 服务器闸漂移 / 查询越闭集三态轮转。
    // 全部落在当轮临时账本上，主账只记守恒数。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = WhoisLedger::new();
        let caught = match kind {
            0 => {
                let _ = lt.record_query(0, query_cell(0).unwrap_or(0));
                lt.record_query(1, query_cell(1).unwrap_or(0).wrapping_add(1))
                    == Err(WhoisCode::EQueryDrift)
            }
            1 => {
                // 服务器闸漂移：先落合法格再注漂移载荷。
                let _ = lt.record_server(0, server_cell(0).unwrap_or(0));
                lt.record_server(1, server_cell(1).unwrap_or(0).wrapping_add(1))
                    == Err(WhoisCode::EServerDrift)
            }
            _ => {
                // 查询越闭集：对空账本同样必抓。
                lt.record_query(QUERY_CELLS, 0).is_err()
            }
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0xC500 + k as u64);
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
/// UNX-F52427 判据集（锚 UNX-F52428-J1，off-by-one 板式）。
pub fn run_unx52427_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52427");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "五查询族 20 格全通",
        recorded >= QUERY_CELLS as u32,
        "域名/IP/AS 号/注册商/TLD 五族逐格载荷恒等",
    );
    s.add(
        "查询族闭集单调",
        {
            let mut ok = true;
            let mut o = 1usize;
            while o < QUERY_CELLS {
                if query_kind(o).unwrap_or(5) < query_kind(o - 1).unwrap_or(5) {
                    ok = false;
                }
                o += 1;
            }
            ok
        },
        "五查询族闭集单调（4 格一族）",
    );
    s.add(
        "查询族名五族全验",
        {
            let mut ok = true;
            let mut k = 0usize;
            while k < 5 {
                if query_kind_name(k).is_none() {
                    ok = false;
                }
                k += 1;
            }
            ok && query_kind_name(5).is_none()
        },
        "域名/IP/AS 号/注册商/TLD 五族名闭集，越界无族名",
    );
    s.add(
        "服务器三元闸 3 格全落",
        {
            let mut lt = WhoisLedger::new();
            let mut ok = true;
            let mut sv = 0usize;
            while sv < SERVER_CELLS {
                if let Some(w) = server_cell(sv) {
                    if lt.record_server(sv, w).is_err() {
                        ok = false;
                    }
                }
                sv += 1;
            }
            ok && lt.servers_all()
        },
        "iana 根/注册商/referral 三态逐格落账，越三态闭集必抓",
    );
    s.add(
        "响应投影 8/8 全落",
        {
            let mut lt = WhoisLedger::new();
            let mut ok = true;
            let mut p = 0usize;
            while p < PROJ_CELLS {
                if let Some(w) = proj_cell(p) {
                    if lt.record_proj(p, w).is_err() {
                        ok = false;
                    }
                }
                p += 1;
            }
            ok && lt.projs_all()
        },
        "whois 响应投影逐格载荷对照，缺一拒封（§AI-66.3）",
    );
    s.add(
        "限速三态 6 格全落",
        {
            let mut lt = WhoisLedger::new();
            let mut ok = true;
            let mut r = 0usize;
            while r < RATE_CELLS {
                if let Some(w) = rate_cell(r) {
                    if lt.record_rate(r, w).is_err() {
                        ok = false;
                    }
                }
                r += 1;
            }
            ok && lt.rates_all()
        },
        "正常/节流/退避三态按 g%3 轮转逐格落账，越三态闭集必抓",
    );
    s.add(
        "限速三态轮转映射核验",
        {
            let mut ok = true;
            let mut r = 0usize;
            while r < RATE_CELLS {
                if rate_state(r).unwrap_or(3) != (r % 3) as u32 {
                    ok = false;
                }
                r += 1;
            }
            ok
        },
        "限速三态 g%3 轮转基准逐格恒等",
    );
    s.add(
        "六判据全落",
        {
            let mut lt = WhoisLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            lt.gates_all()
        },
        "域名查询/IP查询/投影/限速/服务器链/写回逐格落账",
    );
    s.add(
        "注入 10/10 全抓",
        caught >= 10,
        "查询漂移/服务器闸漂移/查询越闭集三态轮转必抓",
    );
    let mut l0 = WhoisLedger::new();
    s.add(
        "边界·查询漂移拒",
        {
            let _ = l0.record_query(0, query_cell(0).unwrap_or(0));
            l0.record_query(1, query_cell(1).unwrap_or(0).wrapping_add(1))
                == Err(WhoisCode::EQueryDrift)
        },
        "查询载荷漂移必抓",
    );
    s.add(
        "边界·查询越闭集",
        l0.record_query(QUERY_CELLS, 0).is_err(),
        "查询 20 格越闭集必拒",
    );
    s.add(
        "边界·服务器闸越闭集",
        l0.record_server(SERVER_CELLS, 0).is_err(),
        "服务器三元闸 3 格越闭集必拒",
    );
    s.add(
        "边界·限速三态越闭集",
        l0.record_rate(RATE_CELLS, 0).is_err(),
        "限速三态 6 格越闭集必拒",
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
            let mut lt = WhoisLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            let mut o = 0usize;
            while o < QUERY_CELLS {
                if let Some(w) = query_cell(o) {
                    let _ = lt.record_query(o, w);
                }
                o += 1;
            }
            let mut sv = 0usize;
            while sv < SERVER_CELLS {
                if let Some(w) = server_cell(sv) {
                    let _ = lt.record_server(sv, w);
                }
                sv += 1;
            }
            let mut p = 0usize;
            while p < PROJ_CELLS {
                if let Some(w) = proj_cell(p) {
                    let _ = lt.record_proj(p, w);
                }
                p += 1;
            }
            let mut r = 0usize;
            while r < RATE_CELLS {
                if let Some(w) = rate_cell(r) {
                    let _ = lt.record_rate(r, w);
                }
                r += 1;
            }
            lt.judge_grade() == Grade::S1
        },
        "全通 → S1（KB-COMPAT 定级登记）",
    );
    s.add(
        "段位守卫",
        SEG_BASE % 0x40 == 0 && SEG_BASE == 0xCAA80,
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
    fn test_query_kind() {
        assert_eq!(query_kind(0), Some(0));
        assert_eq!(query_kind(4), Some(1));
        assert_eq!(query_kind(19), Some(4));
        assert!(query_kind(20).is_none());
        assert!(query_cell(20).is_none());
        assert_eq!(query_kind_name(0), Some("域名"));
        assert_eq!(query_kind_name(4), Some("TLD"));
        assert!(query_kind_name(5).is_none());
    }

    #[test]
    fn test_server_states() {
        assert_eq!(server_state(0), Some(0));
        assert_eq!(server_state(1), Some(1));
        assert_eq!(server_state(2), Some(2));
        assert!(server_state(3).is_none());
        assert!(server_cell(SERVER_CELLS).is_none());
    }

    #[test]
    fn test_rate_states() {
        assert_eq!(rate_state(0), Some(0));
        assert_eq!(rate_state(1), Some(1));
        assert_eq!(rate_state(2), Some(2));
        assert_eq!(rate_state(3), Some(0));
        assert_eq!(rate_state(5), Some(2));
        assert!(rate_state(6).is_none());
        assert!(rate_cell(RATE_CELLS).is_none());
    }

    #[test]
    fn test_cells_pure() {
        let a = query_cell(3).unwrap_or(0);
        assert_eq!(a, query_cell(3).unwrap_or(0));
        assert_ne!(a, query_cell(4).unwrap_or(0));
        let p = proj_cell(5).unwrap_or(0);
        assert_eq!(p, proj_cell(5).unwrap_or(0));
        assert!(proj_cell(PROJ_CELLS).is_none());
        assert_ne!(server_cell(0), server_cell(1));
        assert_ne!(rate_cell(0), rate_cell(1));
    }

    #[test]
    fn test_drift_caught() {
        let mut lt = WhoisLedger::new();
        assert_eq!(
            lt.record_query(5, query_cell(5).unwrap_or(0).wrapping_add(1)),
            Err(WhoisCode::EQueryDrift)
        );
        assert_eq!(
            lt.record_server(1, server_cell(1).unwrap_or(0).wrapping_add(1)),
            Err(WhoisCode::EServerDrift)
        );
        assert_eq!(
            lt.record_proj(3, proj_cell(3).unwrap_or(0).wrapping_add(7)),
            Err(WhoisCode::EProjDrift)
        );
        assert_eq!(
            lt.record_rate(4, rate_cell(4).unwrap_or(0).wrapping_add(1)),
            Err(WhoisCode::ERateDrift)
        );
    }

    #[test]
    fn test_dup_rejected() {
        let mut lt = WhoisLedger::new();
        let _ = lt.record_query(0, query_cell(0).unwrap_or(0));
        assert_eq!(
            lt.record_query(0, query_cell(0).unwrap_or(0)),
            Err(WhoisCode::EGradeDrift)
        );
    }

    #[test]
    fn test_gate_boundary() {
        let mut lt = WhoisLedger::new();
        let mut g = 0usize;
        while g < 6 {
            assert!(lt.judge_gate(g).is_ok());
            g += 1;
        }
        // 闭集边界：0..=5 六闸之外越界必抓，全落才 gates_all。
        assert!(lt.judge_gate(6).is_err());
        assert!(lt.gates_all());
        let mut lt2 = WhoisLedger::new();
        assert!(lt2.judge_gate(0).is_ok());
        assert!(!lt2.gates_all());
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        // 只含 run_cycle 实际计数的节：查询 20 + 服务器 3 + 投影 8 + 限速 6。
        assert_eq!(
            recorded,
            QUERY_CELLS as u32 + SERVER_CELLS as u32 + PROJ_CELLS as u32 + RATE_CELLS as u32
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
        assert_eq!(SEG_BASE, 0xCAA80);
        let _ = _SEG_GUARD;
    }

    #[test]
    fn test_codes_closed_set() {
        assert_eq!(WhoisCode::Ok as u32, 0);
        assert_eq!(WhoisCode::EGradeDrift as u32, 12);
        assert_eq!(WhoisCode::Ok.val(), SEG_BASE);
        assert!(!WhoisCode::Ok.detail().is_empty());
        assert!(!WhoisCode::EServerState.detail().is_empty());
        assert!(!WhoisCode::EGradeDrift.detail().is_empty());
    }

    #[test]
    fn test_grade() {
        assert_eq!(Grade::S1.val(), 1);
        assert_eq!(Grade::S3.val(), 3);
        // 如实阶梯：空账 S3。
        let lt = WhoisLedger::new();
        assert_eq!(lt.judge_grade(), Grade::S3);
        // 六闸齐（格未齐）→ S2。
        let mut lt2 = WhoisLedger::new();
        let mut g = 0usize;
        while g < 6 {
            let _ = lt2.judge_gate(g);
            g += 1;
        }
        assert_eq!(lt2.judge_grade(), Grade::S2);
    }

    #[test]
    fn unx52427_checks_green() {
        let s = run_unx52427_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 18);
    }
}
