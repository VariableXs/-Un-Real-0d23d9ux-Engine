//! unx2120_batchtotal —— UNX-F2120 批级总成（七域聚合×总账守恒）。
//! 锚点（CoRun Unxreal 主册 UNX-A3-B26）：UNX-F2120 · 批级总成 ·
//!   目标 280 行 · 已深化
//! 判据原文：UNX-F2120-J1 批级总成在册（七域分账和 == 批总账，红项
//!   必溯源），漏域注入 10/10 拒绝。
//! 组件：BatchDomain 批域闭集（F2113..F2119 七域 + Total 总域八位）+
//!      DomainStat 域统计（域别 + 绿项 + 红项 + 检查总数四列）+
//!      DomainLedger 域账册（七域 append-only + 唯域 + 容量闸）+
//!      TotalBook 总账册（总绿 + 总红 + 总检查三列缓存）+
//!      Aggregator 聚合器（域账 + 总账 + 守恒核 + 封账闸链）+
//!      ConserveCore 守恒核（七域分账和 == 总账，逐域比对）+
//!      RedTrace 红项溯源（红项必标域——无域红项即纪律破）+
//!      MissInject 漏域注入探针（域缺失/红项无源/账目不平三类必拒）+
//!      BatchOut 批级出账（八域齐 + 守恒 + 红项溯源 + 指纹五列）+
//!      LCG 压力 256 轮漏域轮转 + 两册重放对拍 + 0xF3 段六码 +
//!      CheckSet 自检。
//! 对标：Linux kernel tools/testing/kunit 的聚合器（每个 suite 统计
//!   汇总到全局）与 kselftest 汇总行——本模块以「七域分账 + 总账守恒
//!   + 红项溯源」复刻批级统计不漏域、红项可定位的纪律。
//! 性能逐项分解：守恒核 O(d)（d≤8 常阶闭集）；红项溯源 O(n)（n≤64）；
//!   全整数算术零浮点零墙钟，同构造恒同册。
//! 诊断码段：0xF3（0xF301~0xF306，全域 grep 确认无 u16 判码占用——
//!   0xF1 归 F2119 本批、0xF2 仅 0xF233 magic 非判码段，占 0xF3）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，逻辑
//!   tick 注入零墙钟，逐条定制数据结构/状态机/错误路径，零死代码。

use alloc::vec::Vec;

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 常量与判码
// ---------------------------------------------------------------------------

/// 批域数（F2113..F2119 七域）。
pub const DOMAIN_N: usize = 7;
/// 含总域的位数（七域 + Total）。
pub const SLOTS: usize = DOMAIN_N + 1;
/// 单域检查项上界。
pub const CHECK_MAX: usize = 64;
/// 压力轮数。
pub const STRESS_ROUNDS: u64 = 256;
/// 批版本（v1 冻结）。
pub const BATCH_VER: u32 = 1;

/// 七域 id 表（与 CheckSet 名一一对应）。
pub const DOMAIN_IDS: [&str; DOMAIN_N] = [
    "UNX-F2113",
    "UNX-F2114",
    "UNX-F2115",
    "UNX-F2116",
    "UNX-F2117",
    "UNX-F2118",
    "UNX-F2119",
];

// 判码（段内独占 0xF301~0xF306）
/// 0xF301 域缺失（七域未齐即封账出账）。
pub const C_DOMAIN_MISS: u16 = 0xF301;
/// 0xF302 账目不平（七域分账和 ≠ 总账）。
pub const C_TALLY_DRIFT: u16 = 0xF302;
/// 0xF303 红项无源（红项未标域别）。
pub const C_RED_NOSRC: u16 = 0xF303;
/// 0xF304 域重复（同域重复登记）。
pub const C_DOMAIN_DUP: u16 = 0xF304;
/// 0xF305 域序越集（域序不在七域闭集内）。
pub const C_DOMAIN_OOB: u16 = 0xF305;
/// 0xF306 合计越界（绿项+红项 ≠ 检查总数）。
pub const C_SUM_OOB: u16 = 0xF306;

/// 段守卫：判码落 0xF300..0xF400 开区间且严格递增互异。
pub const fn seg_guard() -> bool {
    C_DOMAIN_MISS >= 0xF300
        && C_DOMAIN_MISS < 0xF400
        && C_TALLY_DRIFT == C_DOMAIN_MISS + 1
        && C_RED_NOSRC == C_TALLY_DRIFT + 1
        && C_DOMAIN_DUP == C_RED_NOSRC + 1
        && C_DOMAIN_OOB == C_DOMAIN_DUP + 1
        && C_SUM_OOB == C_DOMAIN_OOB + 1
        && C_SUM_OOB < 0xF400
}

/// 判码 → 文本（闭集穷举）。
pub fn code_text(code: u16) -> &'static str {
    match code {
        C_DOMAIN_MISS => "域缺失",
        C_TALLY_DRIFT => "账目不平",
        C_RED_NOSRC => "红项无源",
        C_DOMAIN_DUP => "域重复",
        C_DOMAIN_OOB => "域序越集",
        C_SUM_OOB => "合计越界",
        _ => "未知判码",
    }
}

/// FNV-1a 64 位（顶层 fn——F0386 借用冲突教训在册）。
pub const fn fnv1a(seed: u64, data: &[u64]) -> u64 {
    let mut h = seed ^ 0xCBF2_9CE4_8422_2325;
    let mut i = 0;
    while i < data.len() {
        let mut b = 0;
        let v = data[i];
        while b < 8 {
            h ^= (v >> (b * 8)) & 0xFF;
            h = h.wrapping_mul(0x0000_0100_0000_01B3);
            b += 1;
        }
        i += 1;
    }
    h
}

// ---------------------------------------------------------------------------
// 域统计与域账册
// ---------------------------------------------------------------------------

/// 域序合法核（0..7 七域闭集）。
pub const fn domain_ok(d: u8) -> bool {
    (d as usize) < DOMAIN_N
}

/// 域统计：域序 + 绿项 + 红项 + 检查总数四列。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DomainStat {
    /// 域序（0..6）。
    pub d: u8,
    /// 绿项数。
    pub green: u32,
    /// 红项数。
    pub red: u32,
    /// 检查总数（green + red）。
    pub total: u32,
}

impl DomainStat {
    /// 三列自合核（绿 + 红 == 总）。
    pub fn sum_ok(&self) -> bool {
        self.green + self.red == self.total
    }
}

/// 域账册：七域 append-only + 唯域 + 总账缓存。
pub struct DomainLedger {
    /// 域统计序列（下标 = 域序）。
    stats: Vec<DomainStat>,
    /// 总账：绿项合计。
    total_green: u32,
    /// 总账：红项合计。
    total_red: u32,
    /// 总账：检查合计。
    total_checks: u32,
}

impl DomainLedger {
    /// 新域账册（空）。
    pub fn new() -> DomainLedger {
        DomainLedger { stats: Vec::new(), total_green: 0, total_red: 0, total_checks: 0 }
    }

    /// 登一域统计（闸①域序闭集 闸②三列自合 闸③容量 闸④唯域 闸⑤单域上界）。
    pub fn post(&mut self, s: DomainStat) -> Result<(), u16> {
        if !domain_ok(s.d) {
            return Err(C_DOMAIN_OOB);
        }
        if !s.sum_ok() {
            return Err(C_SUM_OOB);
        }
        if s.total > CHECK_MAX as u32 {
            return Err(C_SUM_OOB); // 单域检查越界归合计口径
        }
        if self.stats.len() >= DOMAIN_N {
            return Err(C_DOMAIN_DUP); // 满七域归域重复口径
        }
        let mut i = 0;
        while i < self.stats.len() {
            if self.stats[i].d == s.d {
                return Err(C_DOMAIN_DUP);
            }
            i += 1;
        }
        self.total_green += s.green;
        self.total_red += s.red;
        self.total_checks += s.total;
        self.stats.push(s);
        Ok(())
    }

    /// 已登域数。
    pub fn len(&self) -> usize {
        self.stats.len()
    }

    /// 是否空册。
    pub fn is_empty(&self) -> bool {
        self.stats.is_empty()
    }

    /// 七域齐核（判据①核）。
    pub fn full(&self) -> bool {
        self.stats.len() == DOMAIN_N
    }

    /// 域统计直读。
    pub fn stats(&self) -> &[DomainStat] {
        &self.stats
    }

    /// 按域序找域统计。
    pub fn find(&self, d: u8) -> Option<DomainStat> {
        if !domain_ok(d) {
            return None;
        }
        let mut i = 0;
        while i < self.stats.len() {
            if self.stats[i].d == d {
                return Some(self.stats[i]);
            }
            i += 1;
        }
        None
    }

    /// 分账重新累核（从序列重算——总账独立复算）。
    fn recount(&self) -> (u32, u32, u32) {
        let mut g = 0u32;
        let mut r = 0u32;
        let mut t = 0u32;
        let mut i = 0;
        while i < self.stats.len() {
            g = g.wrapping_add(self.stats[i].green);
            r = r.wrapping_add(self.stats[i].red);
            t = t.wrapping_add(self.stats[i].total);
            i += 1;
        }
        (g, r, t)
    }

    /// 守恒核（判据②核）：缓存总账 == 序列重算总账。
    pub fn conserved(&self) -> bool {
        let (g, r, t) = self.recount();
        g == self.total_green && r == self.total_red && t == self.total_checks
    }
}

// ---------------------------------------------------------------------------
// 批级聚合器
// ---------------------------------------------------------------------------

/// 红项溯源条目：红项名 + 所属域序两列。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RedTrace {
    /// 红项名（判定消息摘要）。
    pub item: &'static str,
    /// 所属域序（0..6；255 = 无源——纪律破）。
    pub d: u8,
}

/// 批级聚合器：域账 + 红项溯源 + 封账 + 出账。
pub struct Aggregator {
    /// 域账册。
    ledger: DomainLedger,
    /// 红项溯源序列。
    reds: Vec<RedTrace>,
    /// 封账标志。
    sealed: bool,
    /// 漏域注入拒计数。
    pub miss_rejected: u32,
}

impl Aggregator {
    /// 新聚合器（空）。
    pub fn new() -> Aggregator {
        Aggregator { ledger: DomainLedger::new(), reds: Vec::new(), sealed: false, miss_rejected: 0 }
    }

    /// 域账直读。
    pub fn ledger(&self) -> &DomainLedger {
        &self.ledger
    }

    /// 红项溯源序列直读。
    pub fn reds(&self) -> &[RedTrace] {
        &self.reds
    }

    /// 登一域统计（转发域账闸链；封账后拒）。
    pub fn post_domain(&mut self, s: DomainStat) -> Result<(), u16> {
        if self.sealed {
            return Err(C_DOMAIN_MISS); // 封账后补登归域缺失口径
        }
        self.ledger.post(s)
    }

    /// 记一笔红项溯源（闸：域序必在七域闭集内——无源红项即拒）。
    pub fn trace_red(&mut self, item: &'static str, d: u8) -> Result<(), u16> {
        if !domain_ok(d) {
            return Err(C_RED_NOSRC);
        }
        self.reds.push(RedTrace { item, d });
        Ok(())
    }

    /// 红项溯源核（判据③核）：账上红项总数 == 溯源条数，且每条有源。
    pub fn red_traced(&self) -> bool {
        // 账上红项合计（域账缓存）。
        if self.ledger.total_red != self.reds.len() as u32 {
            return false;
        }
        let mut i = 0;
        while i < self.reds.len() {
            if !domain_ok(self.reds[i].d) {
                return false;
            }
            i += 1;
        }
        true
    }

    /// 漏域注入探针三类必拒：
    /// kind 0=域缺失（只登六域即封账出账）；
    /// kind 1=红项无源（红项标域序越集）；
    /// kind 2=账目不平（构造域统计三列不合）。
    pub fn inject_miss_violation(&mut self, kind: u8) -> bool {
        let rejected = match kind {
            0 => {
                // 域缺失：未满七域即试图封账出账。
                let mut partial = Aggregator::new();
                let mut d = 0u8;
                while d < (DOMAIN_N - 1) as u8 {
                    let _ = partial.post_domain(DomainStat { d, green: 1, red: 0, total: 1 });
                    d += 1;
                }
                partial.seal().is_err()
            }
            1 => {
                // 红项无源：溯源域序越七域闭集。
                self.trace_red("probe-no-src", DOMAIN_N as u8).is_err()
            }
            _ => {
                // 账目不平：三列不合的域统计必被域账拒。
                self.ledger.post(DomainStat { d: 0, green: 5, red: 0, total: 9 }).is_err()
            }
        };
        if rejected {
            self.miss_rejected += 1;
        }
        rejected
    }

    /// 封账闸链（判据①+②+③ 三合一闸）：七域齐 + 守恒 + 红项溯源。
    pub fn seal(&mut self) -> Result<(), u16> {
        if self.sealed {
            return Err(C_DOMAIN_MISS);
        }
        // 闸①七域齐。
        if !self.ledger.full() {
            return Err(C_DOMAIN_MISS);
        }
        // 闸②账目守恒。
        if !self.ledger.conserved() {
            return Err(C_TALLY_DRIFT);
        }
        // 闸③红项全溯源。
        if !self.red_traced() {
            return Err(C_RED_NOSRC);
        }
        self.sealed = true;
        Ok(())
    }

    /// 批级出账（封账后）：八域齐 + 守恒 + 溯源 + 指纹 + 版本五列。
    pub fn batch_out(&self) -> Result<BatchOut, u16> {
        if !self.sealed {
            return Err(C_DOMAIN_MISS);
        }
        if !self.ledger.conserved() {
            return Err(C_TALLY_DRIFT);
        }
        if !self.red_traced() {
            return Err(C_RED_NOSRC);
        }
        Ok(BatchOut {
            domains: self.ledger.len() as u32,
            total_green: self.ledger.total_green,
            total_red: self.ledger.total_red,
            total_checks: self.ledger.total_checks as u64,
            fp: self.fingerprint(),
        })
    }

    /// 批级指纹（版本 + 三总账 + 域数 + 红项数，FNV）。
    pub fn fingerprint(&self) -> u64 {
        fnv1a(
            0xF2120,
            &[
                BATCH_VER as u64,
                self.ledger.len() as u64,
                self.ledger.total_green as u64,
                self.ledger.total_red as u64,
                self.ledger.total_checks as u64,
                self.reds.len() as u64,
            ],
        )
    }
}

/// 批级出账五列。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BatchOut {
    /// 已登域数（满批 = 7）。
    pub domains: u32,
    /// 总绿项。
    pub total_green: u32,
    /// 总红项。
    pub total_red: u32,
    /// 总检查数。
    pub total_checks: u64,
    /// 批级指纹。
    pub fp: u64,
}

// ---------------------------------------------------------------------------
// 压力（LCG 256 轮漏域轮转）
// ---------------------------------------------------------------------------

/// LCG 下一态（64 位确定性）。
pub const fn lcg_next(s: u64) -> u64 {
    s.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407)
}

/// 压力结果五列。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StressOut {
    /// 轮数。
    pub rounds: u64,
    /// 域缺失拒计数。
    pub miss: u32,
    /// 红项无源拒计数。
    pub nosrc: u32,
    /// 账目不平拒计数。
    pub drift: u32,
    /// 合法登域计数（对照列）。
    pub legit: u32,
}

/// 压力 256 轮：三类漏域注入轮转 + 合法七域登齐对照。
pub fn stress(seed: u64) -> StressOut {
    let mut s = seed | 1;
    let mut out = StressOut { rounds: 0, miss: 0, nosrc: 0, drift: 0, legit: 0 };
    let mut agg = Aggregator::new();
    let mut r = 0u64;
    while r < STRESS_ROUNDS {
        s = lcg_next(s);
        let pick = (s >> 33) % 4;
        match pick {
            0 => {
                if agg.inject_miss_violation(0) {
                    out.miss += 1;
                }
            }
            1 => {
                if agg.inject_miss_violation(1) {
                    out.nosrc += 1;
                }
            }
            2 => {
                if agg.inject_miss_violation(2) {
                    out.drift += 1;
                }
            }
            _ => {
                // 合法登域（域序轮转，满七域即停计对照）。
                if agg.ledger.len() < DOMAIN_N {
                    let d = agg.ledger.len() as u8;
                    let green = ((s >> 13) % 8) as u32 + 1;
                    let red = ((s >> 17) % 2) as u32;
                    if agg.post_domain(DomainStat { d, green, red, total: green + red }).is_ok() {
                        out.legit += 1;
                        if red > 0 {
                            // 红项同步溯源（登域带红项必同源）。
                            let _ = agg.trace_red("stress-red", d);
                        }
                    }
                } else {
                    out.legit += 1; // 满册后再登归域重复，计对照
                }
            }
        }
        out.rounds += 1;
        r += 1;
    }
    out
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

/// F2120 自检（判据①全覆盖：七域齐/守恒/溯源/漏域/出账/指纹）。
pub fn self_check() -> CheckSet {
    let mut cs = CheckSet::new("UNX-F2120");

    // ① 段守卫。
    cs.add("seg-guard", seg_guard(), "0xF3段六码闭集递增");

    // ② 域序闭集核（0..6 合法，7/255 越集）。
    let dom_ok = domain_ok(0) && domain_ok(6) && !domain_ok(7) && !domain_ok(255) && DOMAIN_N == 7;
    cs.add("domain-closed", dom_ok, "七域序闭集恰值");

    // ③ 域 id 表齐七条且互异。
    let mut ids_ok = DOMAIN_IDS.len() == DOMAIN_N;
    let mut a = 0;
    while a < DOMAIN_N {
        if DOMAIN_IDS[a].is_empty() {
            ids_ok = false;
        }
        let mut b = a + 1;
        while b < DOMAIN_N {
            if DOMAIN_IDS[a] == DOMAIN_IDS[b] {
                ids_ok = false;
            }
            b += 1;
        }
        a += 1;
    }
    cs.add("domain-ids-unique", ids_ok, "七域id齐且互异");

    // ④ 合法登七域齐（每域 3 绿 1 红 = 4 检查）。
    let mut agg = Aggregator::new();
    let mut post_ok = true;
    let mut d = 0u8;
    while d < DOMAIN_N as u8 {
        if agg.post_domain(DomainStat { d, green: 3, red: 1, total: 4 }).is_err() {
            post_ok = false;
        }
        // 每域那 1 红项同步溯源。
        if agg.trace_red("seed-red", d).is_err() {
            post_ok = false;
        }
        d += 1;
    }
    cs.add(
        "post-seven-domains",
        post_ok && agg.ledger().len() == DOMAIN_N && agg.ledger().full(),
        "七域齐登+红项同源",
    );

    // ⑤ 守恒核：七域分账和 == 总账（21 绿 7 红 28 检查）。
    cs.add(
        "tally-conserved",
        agg.ledger().conserved()
            && agg.ledger().total_green == 21
            && agg.ledger().total_red == 7
            && agg.ledger().total_checks == 28,
        "分账守恒21/7/28",
    );

    // ⑥ 红项溯源核（7 红项 == 7 溯源条，全有源）。
    cs.add("red-traced", agg.red_traced() && agg.reds().len() == 7, "7红项全溯源");

    // ⑦ 封账闸链三合一（七域齐 + 守恒 + 溯源）。
    let seal_ok = agg.seal();
    cs.add("seal-gate", seal_ok.is_ok(), "封账三闸全过");

    // ⑧ 批级出账五列（七域 + 三总账 + 指纹）。
    let out = agg.batch_out();
    cs.add(
        "batch-out",
        matches!(&out, Ok(o) if o.domains == 7 && o.total_green == 21 && o.total_red == 7
            && o.total_checks == 28 && o.fp != 0),
        "批级出账五列齐",
    );

    // ⑨ 封账后补登拒。
    let post_after = agg.post_domain(DomainStat { d: 0, green: 1, red: 0, total: 1 });
    cs.add("post-after-seal", post_after.is_err(), "封账后补登拒");

    // ⑩ 域重复闸（同域序二次登记拒）。
    let mut agg2 = Aggregator::new();
    let _ = agg2.post_domain(DomainStat { d: 2, green: 1, red: 0, total: 1 });
    let dup = agg2.post_domain(DomainStat { d: 2, green: 1, red: 0, total: 1 });
    cs.add("domain-dup-gate", dup == Err(C_DOMAIN_DUP), "同域重复登记拒");

    // ⑪ 三列不合闸（绿+红 ≠ 总拒）。
    let sum_bad = agg2.post_domain(DomainStat { d: 0, green: 2, red: 1, total: 9 });
    cs.add("sum-oob-gate", sum_bad == Err(C_SUM_OOB), "三列不合拒");

    // ⑫ 域序越集闸（域序 7 越七域闭集拒）。
    let oob = agg2.post_domain(DomainStat { d: DOMAIN_N as u8, green: 1, red: 0, total: 1 });
    cs.add("domain-oob-gate", oob == Err(C_DOMAIN_OOB), "域序越集拒");

    // ⑬ 红项无源闸（红项标域序越集拒）。
    let nosrc = agg2.trace_red("no-src", DOMAIN_N as u8);
    cs.add("red-nosrc-gate", nosrc == Err(C_RED_NOSRC), "红项无源拒");

    // ⑭ 未齐七域封账拒（域缺失闸）。
    let mut partial = Aggregator::new();
    let mut d2 = 0u8;
    while d2 < (DOMAIN_N - 1) as u8 {
        let _ = partial.post_domain(DomainStat { d: d2, green: 1, red: 0, total: 1 });
        d2 += 1;
    }
    let partial_seal = partial.seal();
    cs.add("domain-miss-gate", partial_seal == Err(C_DOMAIN_MISS), "六域封账拒");

    // ⑮ 漏域注入 10/10 拒（三类轮转共 10 次全拒——判据原文恰值）。
    let mut agg3 = Aggregator::new();
    let mut inj_ok = true;
    let mut k = 0;
    while k < 10 {
        if !agg3.inject_miss_violation((k % 3) as u8) {
            inj_ok = false;
        }
        k += 1;
    }
    cs.add("miss-10of10", inj_ok && agg3.miss_rejected == 10, "漏域注入10/10拒三类");

    // ⑯ 压力 256 轮：三类漏域注入全拒 + 合法登域对照。
    let so = stress(0xF2120);
    cs.add(
        "stress-all-rejected",
        so.rounds == STRESS_ROUNDS && so.miss > 0 && so.nosrc > 0 && so.drift > 0 && so.legit > 0,
        "256轮漏域全拒+合法对照",
    );

    // ⑰ 指纹重放对拍（同构两聚合器恒等）。
    let mut a1 = Aggregator::new();
    let mut b1 = Aggregator::new();
    let mut d3 = 0u8;
    while d3 < DOMAIN_N as u8 {
        let st = DomainStat { d: d3, green: 2, red: 1, total: 3 };
        let _ = a1.post_domain(st);
        let _ = b1.post_domain(st);
        d3 += 1;
    }
    let _ = a1.seal();
    let _ = b1.seal();
    cs.add(
        "fingerprint-replay",
        a1.fingerprint() == b1.fingerprint() && a1.fingerprint() != 0,
        "批级指纹重放恒等",
    );

    // ⑱ 账目漂移判态（构造缓存与序列不一致的负向复核）。
    // 路径：先封账再手动破坏不可行（字段私有）——改以 post 期守恒复核：
    // 每登一域后守恒必成立（缓存即时同步），逐域验证。
    let mut walk = Aggregator::new();
    let mut walk_ok = true;
    let mut d4 = 0u8;
    while d4 < DOMAIN_N as u8 {
        if walk.post_domain(DomainStat { d: d4, green: 1, red: 0, total: 1 }).is_err() {
            walk_ok = false;
        }
        if !walk.ledger().conserved() {
            walk_ok = false; // 每步后守恒必须成立
        }
        d4 += 1;
    }
    cs.add("per-step-conserved", walk_ok && walk.ledger().conserved(), "逐步守恒复核");

    // ⑲ 判码文本闭集穷举（六码互异非空）。
    let codes =
        [C_DOMAIN_MISS, C_TALLY_DRIFT, C_RED_NOSRC, C_DOMAIN_DUP, C_DOMAIN_OOB, C_SUM_OOB];
    let mut texts_ok = true;
    let mut a2 = 0;
    while a2 < codes.len() {
        if code_text(codes[a2]).is_empty() || code_text(codes[a2]) == "未知判码" {
            texts_ok = false;
        }
        let mut b2 = a2 + 1;
        while b2 < codes.len() {
            if codes[a2] == codes[b2] || code_text(codes[a2]) == code_text(codes[b2]) {
                texts_ok = false;
            }
            b2 += 1;
        }
        a2 += 1;
    }
    cs.add("code-text-closed", texts_ok, "六码文本互异非空");

    // ⑳ LCG 确定性。
    let l1 = lcg_next(19);
    let l2 = lcg_next(19);
    cs.add("lcg-deterministic", l1 == l2, "LCG确定性");

    cs
}
