//! VE-F0094 · 提交频率限制器（VE-A 域 · A02 提交治理组 · 目标 300 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0094`
//!
//! **职责定位（锚点原文）**：提交频率限制器——命令提交的频率限制（提交风暴
//! 的限流不崩），**限流参数公开**，超限的**三要素告知**；含限流对合法高负载
//! 的豁免申请（白名单申请通道）。
//!
//! **数据结构（锚点原文）**：限流器。
//!
//! **错误路径与降级矩阵（锚点原文）**：
//! - 风暴 → 限流（突发流量不得把队列打穿，令牌桶按容量硬顶）；
//! - 误限 → 调参留痕（每次调参记旧值/新值/理由，禁止无声调参）；
//! - 限流失效 → 账本（令牌账必须可对：发放 = 消耗 + 结余）。
//!
//! **性能逐项分解（锚点原文）**：O(提交)。每次准入只做一次乘法与两次比较；
//! 补桶按「整 µs × 速率」整数换算。
//!
//! **跨批对接点（锚点原文）**：AA07 限流同构——同构点在三要素字段与
//! 令牌账口径，本模块的 `LimitNotice` 字段名与 AA07 保持一致，便于上层
//! 统一展示。
//!
//! **无障碍与隐私（锚点原文）**：限流状态读屏可达——`status_line()` 输出
//! 纯文本「配额/已用/重试等待 µs」。
//!
//! **判据（锚点原文）**：频率限制、风暴防护、参数公开、三要素。
//!
//! 纯功能线：no_std + alloc；零 unwrap/expect/panic；整数；确定性。
//!
//! 诊断段：0xE700 段 **0xE74x 子块**（0xE741~0xE74A 十码）。

use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const RL_VERSION: &str = "vea94_ratelimit-v1";

/// 诊断段（子块）基址。
pub const RL_SEG_BASE: u16 = 0xE740;

/// 诊断码闭集（0xE741~0xE74A）。
pub const XC_BUCKET_BAD: u32 = 0xE741;
pub const XC_RATE_ZERO: u32 = 0xE742;
pub const XC_TICK_BACKWARD: u32 = 0xE743;
pub const XC_THROTTLED: u32 = 0xE744;
pub const XC_STORM: u32 = 0xE745;
pub const XC_EXEMPT_EXPIRED: u32 = 0xE746;
pub const XC_EXEMPT_BAD: u32 = 0xE747;
pub const XC_TUNE_NO_REASON: u32 = 0xE748;
pub const XC_LEDGER_BROKEN: u32 = 0xE749;
pub const XC_CONTEXT_FULL: u32 = 0xE74A;

/// 令牌汇率：1 令牌 = 1 次提交。
pub const TOKENS_PER_SUBMIT: u64 = 1;

/// 风暴判据：单位窗口内请求数超过容量的该倍数即判风暴。
pub const STORM_FACTOR: u64 = 4;

/// 调参账容量。
pub const TUNE_LOG_CAP: usize = 32;

/// 一档限流参数（公开面：这些值必须能被上层读出并展示）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LimitParams {
    /// 桶容量（突发上限，次）。
    pub burst: u32,
    /// 稳态速率（次/秒）。
    pub rate_per_sec: u32,
}

impl LimitParams {
    pub const fn new(burst: u32, rate_per_sec: u32) -> LimitParams {
        LimitParams { burst, rate_per_sec }
    }

    pub const fn valid(&self) -> bool {
        self.burst > 0 && self.rate_per_sec > 0
    }

    /// 补桶：`elapsed_us` 内应补的令牌数（整数，向下取整）。
    ///
    /// 用 u64 中间量：rate × elapsed_us 在 rate=10000、elapsed=10^7µs 时
    /// 已达 10^11，u32 会回绕成「补了一大堆令牌」——这是限流器最经典的
    /// 绕过手法，必须用宽类型。
    pub const fn refill(&self, elapsed_us: u64) -> u64 {
        (self.rate_per_sec as u64) * elapsed_us / 1_000_000u64
    }

    /// 三要素之一：稳态配额（次/秒）。
    pub const fn limit_per_sec(&self) -> u32 {
        self.rate_per_sec
    }
}

/// 超限告知（三要素：配额 / 当前 / 重试等待）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LimitNotice {
    pub limit_per_sec: u32,
    /// 当前桶内剩余令牌（连续被拒时为 0）。
    pub current_tokens: u32,
    /// 需要等待多少 µs 才有 1 个令牌。
    pub retry_after_us: u32,
}

/// 准入结果。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Admit {
    Granted { tokens_left: u32, exempt: bool },
    Throttled(LimitNotice),
    Rejected { code: u32 },
}

/// 一次调参记录。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TuneRecord {
    pub tick: u64,
    pub old_rate: u32,
    pub new_rate: u32,
    /// 理由码（0 = 无理由，禁止）。
    pub reason: u8,
}

/// 豁免申请（白名单通道：容量 + 理由 + 到期）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Exemption {
    /// 提高后的突发容量（豁免只提容量，不提速率——速率是设备能力红线）。
    pub extra_burst: u32,
    pub expires_tick: u64,
    pub reason: u8,
    pub revoked: bool,
}

/// 单上下文令牌桶。
pub struct Bucket {
    params: LimitParams,
    tokens: u32,
    last_tick_us: u64,
    /// 窗口内请求数（用于风暴判定；按 1 秒窗口滚动）。
    window_start_us: u64,
    window_reqs: u64,
    /// 豁免（可选一条）。
    exemption: Option<Exemption>,
    /// 账：累计发放令牌（补桶 + 初始）。
    issued: u64,
    /// 账：累计消耗令牌。
    consumed: u64,
    /// 被拒次数。
    throttled: u32,
    /// 风暴检出次数。
    storm_hits: u32,
    /// 当前桶内令牌（公开可读）。
    pub_last_notice: Option<LimitNotice>,
    /// 最近诊断码。
    last_code: u32,
}

impl Bucket {
    pub fn new(params: LimitParams, now_us: u64) -> Bucket {
        Bucket {
            params,
            tokens: params.burst,
            last_tick_us: now_us,
            window_start_us: now_us,
            window_reqs: 0,
            exemption: None,
            issued: params.burst as u64,
            consumed: 0,
            throttled: 0,
            storm_hits: 0,
            pub_last_notice: None,
            last_code: 0,
        }
    }

    pub fn params(&self) -> LimitParams {
        self.params
    }

    pub fn tokens(&self) -> u32 {
        self.tokens
    }

    pub fn counters(&self) -> (u64, u64, u32, u32) {
        (self.issued, self.consumed, self.throttled, self.storm_hits)
    }

    pub fn last_code(&self) -> u32 {
        self.last_code
    }

    /// 生效容量 = 基础突发 + 未过期豁免。
    pub fn effective_burst(&self, now_us: u64) -> u32 {
        match self.exemption {
            Some(e) if !e.revoked && now_us < e.expires_tick => {
                self.params.burst.saturating_add(e.extra_burst)
            }
            _ => self.params.burst,
        }
    }

    /// 时间推进 + 补桶（幂等：同一 tick 重复调用不重复补）。
    fn advance(&mut self, now_us: u64) -> bool {
        if now_us < self.last_tick_us {
            self.last_code = XC_TICK_BACKWARD;
            return false;
        }
        let elapsed = now_us - self.last_tick_us;
        if elapsed == 0 {
            return true;
        }
        let add = self.params.refill(elapsed);
        if add > 0 {
            let cap = self.effective_burst(now_us);
            let room = cap.saturating_sub(self.tokens) as u64;
            let real = if add > room { room } else { add };
            self.tokens = self.tokens.saturating_add(real as u32);
            self.issued = self.issued.saturating_add(real);
        }
        self.last_tick_us = now_us;
        true
    }

    /// 申请一次提交。
    pub fn admit(&mut self, now_us: u64) -> Admit {
        if !self.params.valid() {
            self.last_code = XC_RATE_ZERO;
            return Admit::Rejected { code: XC_RATE_ZERO };
        }
        if !self.advance(now_us) {
            return Admit::Rejected { code: XC_TICK_BACKWARD };
        }

        // 风暴计数（1 秒滚动窗）。
        if now_us.saturating_sub(self.window_start_us) >= 1_000_000 {
            self.window_start_us = now_us;
            self.window_reqs = 0;
        }
        self.window_reqs += 1;
        let storm_threshold = (self.params.burst as u64) * STORM_FACTOR;
        if self.window_reqs > storm_threshold {
            self.storm_hits = self.storm_hits.saturating_add(1);
            self.last_code = XC_STORM;
        }

        let exempt = match self.exemption {
            Some(e) if !e.revoked && now_us < e.expires_tick => true,
            _ => false,
        };

        if self.tokens >= TOKENS_PER_SUBMIT as u32 {
            self.tokens -= TOKENS_PER_SUBMIT as u32;
            self.consumed = self.consumed.saturating_add(TOKENS_PER_SUBMIT);
            return Admit::Granted { tokens_left: self.tokens, exempt };
        }

        // 拒绝：三要素必须齐全。
        let notice = LimitNotice {
            limit_per_sec: self.params.limit_per_sec(),
            current_tokens: self.tokens,
            retry_after_us: self.retry_us(),
        };
        self.pub_last_notice = Some(notice);
        self.throttled = self.throttled.saturating_add(1);
        if self.last_code != XC_STORM {
            self.last_code = XC_THROTTLED;
        }
        Admit::Throttled(notice)
    }

    /// 补齐 1 个令牌所需等待（µs，向上取整）。
    pub fn retry_us(&self) -> u32 {
        if self.params.rate_per_sec == 0 {
            return u32::MAX;
        }
        if (self.tokens as u64) >= TOKENS_PER_SUBMIT {
            return 0;
        }
        // 桶里已有足够令牌时无需等待（0）。这里必须显式分支：
        // 初版直接写 `TOKENS_PER_SUBMIT - self.tokens`，当 tokens > 1 时
        // u64 下溢 panic（实测在 status_line 里被引爆）——「等待时间」函数
        // 在**不缺令牌**时也必须可调用，否则状态展示就会崩。
        let need = if (self.tokens as u64) >= TOKENS_PER_SUBMIT {
            0u64
        } else {
            TOKENS_PER_SUBMIT - self.tokens as u64
        };
        // rate 次/秒 ⇒ 单个令牌 1e6/rate µs；向上取整。
        let per = (1_000_000u64 + self.params.rate_per_sec as u64 - 1) / self.params.rate_per_sec as u64;
        let t = per.saturating_mul(need);
        if t > u32::MAX as u64 {
            u32::MAX
        } else {
            t as u32
        }
    }

    /// 豁免申请（带理由与到期；无理由拒绝）。
    pub fn apply_exemption(&mut self, extra_burst: u32, expires_tick: u64, reason: u8) -> bool {
        if extra_burst == 0 || reason == 0 || expires_tick == 0 {
            self.last_code = XC_EXEMPT_BAD;
            return false;
        }
        self.exemption = Some(Exemption { extra_burst, expires_tick, reason, revoked: false });
        true
    }

    pub fn revoke_exemption(&mut self) -> bool {
        match self.exemption {
            Some(mut e) => {
                e.revoked = true;
                self.exemption = Some(e);
                true
            }
            None => false,
        }
    }

    /// 令牌账守恒：issued ≡ consumed + tokens。
    pub fn ledger_balanced(&self) -> bool {
        self.issued == self.consumed + self.tokens as u64
    }

    /// 读屏状态行（参数公开 + 三要素）。
    pub fn status_line(&self) -> String {
        let mut s = String::new();
        s.push_str("限流 配额=");
        push_u32(&mut s, self.params.limit_per_sec());
        s.push_str("/s 突发=");
        push_u32(&mut s, self.params.burst);
        s.push_str(" 余=");
        push_u32(&mut s, self.tokens);
        s.push_str(" 重试=");
        push_u32(&mut s, self.retry_us());
        s.push_str("us");
        s
    }
}

fn push_u32(s: &mut String, mut v: u32) {
    if v == 0 {
        s.push('0');
        return;
    }
    let mut buf = [0u8; 10];
    let mut n = 0usize;
    while v > 0 {
        buf[n] = b'0' + (v % 10) as u8;
        v /= 10;
        n += 1;
    }
    while n > 0 {
        n -= 1;
        s.push(buf[n] as char);
    }
}

/// 多上下文限流表（每上下文一只桶；满即拒新上下文）。
pub struct LimitTable {
    buckets: Vec<(u32, Bucket)>,
    tunes: Vec<TuneRecord>,
    tune_dropped: u32,
    last_code: u32,
}

impl LimitTable {
    pub fn new() -> LimitTable {
        LimitTable { buckets: Vec::new(), tunes: Vec::new(), tune_dropped: 0, last_code: 0 }
    }

    pub fn register(&mut self, ctx: u32, params: LimitParams, now_us: u64) -> bool {
        if self.buckets.len() >= 16 || !params.valid() {
            self.last_code = if self.buckets.len() >= 16 { XC_CONTEXT_FULL } else { XC_BUCKET_BAD };
            return false;
        }
        if self.buckets.iter().any(|(id, _)| *id == ctx) {
            self.last_code = XC_BUCKET_BAD;
            return false;
        }
        self.buckets.push((ctx, Bucket::new(params, now_us)));
        true
    }

    pub fn admit(&mut self, ctx: u32, now_us: u64) -> Option<Admit> {
        let mut i = 0usize;
        while i < self.buckets.len() {
            if self.buckets[i].0 == ctx {
                return Some(self.buckets[i].1.admit(now_us));
            }
            i += 1;
        }
        None
    }

    /// 调参（必须带理由；记录旧值/新值/理由，禁止无声调参）。
    pub fn tune(&mut self, ctx: u32, new_rate: u32, now_us: u64, reason: u8) -> bool {
        if reason == 0 {
            self.last_code = XC_TUNE_NO_REASON;
            return false;
        }
        let mut i = 0usize;
        while i < self.buckets.len() {
            if self.buckets[i].0 == ctx {
                let old = self.buckets[i].1.params.rate_per_sec;
                self.buckets[i].1.params.rate_per_sec = new_rate;
                if self.tunes.len() < TUNE_LOG_CAP {
                    self.tunes.push(TuneRecord { tick: now_us, old_rate: old, new_rate, reason });
                } else {
                    self.tune_dropped += 1;
                }
                return true;
            }
            i += 1;
        }
        false
    }

    pub fn tune_log(&self) -> &[TuneRecord] {
        &self.tunes
    }

    pub fn bucket(&self, ctx: u32) -> Option<&Bucket> {
        self.buckets.iter().find(|(id, _)| *id == ctx).map(|(_, b)| b)
    }
}

// ---------------------------------------------------------------------------
// 判据套件
// ---------------------------------------------------------------------------

fn chk(set: &mut CheckSet, name: &'static str, ok: bool, detail: &'static str) {
    set.add(name, ok, if ok { "" } else { detail });
}

/// VE-F0094 判据入口。
pub fn run_vea94_checks() -> CheckSet {
    let mut set = CheckSet::new("vea94_ratelimit");

    // C94-01 突发容量内全部放行
    {
        let p = LimitParams::new(8, 100);
        let mut b = Bucket::new(p, 0);
        let mut ok = true;
        for i in 0..8 {
            if !matches!(b.admit(i), Admit::Granted { .. }) {
                ok = false;
            }
        }
        chk(&mut set, "C94-01 突发容量放行", ok && b.tokens() == 0, "突发未放满");
    }

    // C94-02 容量用尽后限流且三要素非零
    {
        let p = LimitParams::new(2, 1000);
        let mut b = Bucket::new(p, 0);
        let _ = b.admit(0);
        let _ = b.admit(0);
        match b.admit(0) {
            Admit::Throttled(n) => {
                let ok = n.limit_per_sec == 1000 && n.current_tokens == 0 && n.retry_after_us > 0;
                chk(&mut set, "C94-02 三要素齐全", ok, "三要素缺失");
            }
            _ => chk(&mut set, "C94-02 三要素齐全", false, "未限流"),
        }
    }

    // C94-03 重试等待精确（1000/s ⇒ 单令牌 1000µs；余 0 时需 1 个）
    {
        let p = LimitParams::new(1, 1000);
        let mut b = Bucket::new(p, 0);
        let _ = b.admit(0);
        let ok = b.retry_us() == 1000;
        chk(&mut set, "C94-03 重试等待精确", ok, "等待算错");
    }

    // C94-04 补桶精确（1e6 µs @1000/s ⇒ 恰好 1000 个，但受容量钳制）
    {
        let p = LimitParams::new(4, 1000);
        let mut b = Bucket::new(p, 0);
        for _ in 0..4 {
            let _ = b.admit(0);
        }
        // 走 10ms：应补 10 个，但容量只有 4 ⇒ 补到 4
        let a = b.admit(10_000);
        let ok = matches!(a, Admit::Granted { tokens_left, .. } if tokens_left == 3);
        chk(&mut set, "C94-04 补桶钳容量", ok, "补桶未钳容量");
    }

    // C94-05 补桶不越容量上限（长时间空闲不攒无限令牌）
    {
        let p = LimitParams::new(4, 1000);
        let mut b = Bucket::new(p, 0);
        let a = b.admit(100_000_000); // 100 秒空闲
        let ok = matches!(a, Admit::Granted { tokens_left, .. } if tokens_left == 3);
        chk(&mut set, "C94-05 空闲不攒爆", ok, "空闲攒出超额");
    }

    // C94-06 时钟倒退显性拒绝
    {
        let p = LimitParams::new(4, 1000);
        let mut b = Bucket::new(p, 1000);
        let r = b.admit(500);
        let ok = matches!(r, Admit::Rejected { code } if code == XC_TICK_BACKWARD);
        chk(&mut set, "C94-06 时钟倒退", ok, "倒退被接受");
    }

    // C94-07 参数非法显性拒绝（速率 0）
    {
        let p = LimitParams::new(4, 0);
        let mut b = Bucket::new(p, 0);
        let r = b.admit(0);
        let ok = matches!(r, Admit::Rejected { code } if code == XC_RATE_ZERO);
        chk(&mut set, "C94-07 参数闸", ok, "零速率被接受");
    }

    // C94-08 风暴检出（1 秒窗口内超过 4×容量）
    {
        let p = LimitParams::new(4, 100_000);
        let mut b = Bucket::new(p, 0);
        let mut storm = false;
        for i in 0..20u64 {
            let _ = b.admit(i * 1000); // 20ms 内 20 次 ⇒ 超 4×4=16
            if b.last_code() == XC_STORM {
                storm = true;
            }
        }
        chk(&mut set, "C94-08 风暴检出", storm && b.counters().3 > 0, "风暴未检出");
    }

    // C94-09 令牌账守恒（发放 = 消耗 + 结余）
    {
        let p = LimitParams::new(6, 500);
        let mut b = Bucket::new(p, 0);
        for i in 0..10u64 {
            let _ = b.admit(i * 2000);
        }
        chk(&mut set, "C94-09 令牌账守恒", b.ledger_balanced(), "令牌账不平");
    }

    // C94-10 豁免提容量（不偷改速率）且到期失效
    {
        let p = LimitParams::new(2, 10);
        let mut b = Bucket::new(p, 0);
        // 豁免只提「突发容量」：2 → 8，速率仍是 10/s（速率是设备能力红线）。
        let ok_apply = b.apply_exemption(6, 1_000_000, 1);
        let eff = b.effective_burst(500);
        // 0.5s 后补桶 5 个，被上限 8 承接 ⇒ 桶内 8 个，连发三次必须都放行；
        // 若豁免没生效（容量仍 2），第三次必被限流。
        let _ = b.admit(500_000);
        let _ = b.admit(500_001);
        let third = b.admit(500_002);
        let granted_under_exempt = matches!(third, Admit::Granted { .. });
        // 到期后容量回落（到期时刻起恢复基础容量）。
        let eff_expired = b.effective_burst(1_000_000);
        let ok = ok_apply && eff == 8 && granted_under_exempt && eff_expired == 2;
        chk(&mut set, "C94-10 豁免与到期", ok, "豁免语义错");
    }

    // C94-11 无理由豁免拒绝
    {
        let p = LimitParams::new(2, 10);
        let mut b = Bucket::new(p, 0);
        let ok = !b.apply_exemption(4, 100, 0) && b.last_code() == XC_EXEMPT_BAD;
        chk(&mut set, "C94-11 无理由豁免拒绝", ok, "空理由被接受");
    }

    // C94-12 豁免吊销
    {
        let p = LimitParams::new(2, 10);
        let mut b = Bucket::new(p, 0);
        let _ = b.apply_exemption(4, 1000, 3);
        let _ = b.revoke_exemption();
        let ok = b.effective_burst(10) == 2;
        chk(&mut set, "C94-12 豁免吊销", ok, "吊销后仍生效");
    }

    // C94-13 调参留痕（旧值/新值/理由全在册）
    {
        let mut t = LimitTable::new();
        let _ = t.register(7, LimitParams::new(4, 100), 0);
        let _ = t.tune(7, 250, 10, 2);
        let log = t.tune_log();
        let ok = log.len() == 1 && log[0].old_rate == 100 && log[0].new_rate == 250 && log[0].reason == 2;
        chk(&mut set, "C94-13 调参留痕", ok, "调参未留痕");
    }

    // C94-14 无理由调参拒绝
    {
        let mut t = LimitTable::new();
        let _ = t.register(7, LimitParams::new(4, 100), 0);
        let ok = !t.tune(7, 250, 10, 0) && t.tune_log().is_empty();
        chk(&mut set, "C94-14 无理由调参拒绝", ok, "空理由调参");
    }

    // C94-15 重复上下文注册拒绝
    {
        let mut t = LimitTable::new();
        let _ = t.register(3, LimitParams::new(2, 10), 0);
        let ok = !t.register(3, LimitParams::new(2, 10), 0);
        chk(&mut set, "C94-15 重复注册拒绝", ok, "重复注册被接受");
    }

    // C94-16 上下文表满显性
    {
        let mut t = LimitTable::new();
        for i in 0..16u32 {
            let _ = t.register(i, LimitParams::new(2, 10), 0);
        }
        let ok = !t.register(99, LimitParams::new(2, 10), 0) && t.last_code == XC_CONTEXT_FULL;
        chk(&mut set, "C94-16 上下文表满", ok, "表满未显性");
    }

    // C94-17 参数公开（状态行含配额/突发/余量/重试四要素）
    {
        let p = LimitParams::new(5, 300);
        let mut b = Bucket::new(p, 0);
        let _ = b.admit(0);
        let line = b.status_line();
        let ok = line.contains("配额") && line.contains("突发") && line.contains("余") && line.contains("重试");
        chk(&mut set, "C94-17 参数公开", ok, "参数未公开");
    }

    // C94-18 多上下文互不影响（一只桶打满不牵连另一只）
    {
        let mut t = LimitTable::new();
        let _ = t.register(1, LimitParams::new(1, 10), 0);
        let _ = t.register(2, LimitParams::new(1, 10), 0);
        let _ = t.admit(1, 0);
        let a1 = t.admit(1, 0); // 打满
        let a2 = t.admit(2, 0); // 另一只桶应放行
        let ok = matches!(a1, Some(Admit::Throttled(_))) && matches!(a2, Some(Admit::Granted { .. }));
        chk(&mut set, "C94-18 上下文隔离", ok, "上下文互相牵连");
    }

    // C94-19 确定性：同序列两次的计数与结余一致
    {
        let mk = || {
            let p = LimitParams::new(5, 700);
            let mut b = Bucket::new(p, 0);
            for i in 0..25u64 {
                let _ = b.admit(i * 1500);
            }
            (b.counters(), b.tokens())
        };
        let x = mk();
        let y = mk();
        chk(&mut set, "C94-19 确定性", x == y, "两次不一致");
    }

    set
}

/// 深度驱动入口。
pub fn drill() -> (bool, String) {
    let s = run_vea94_checks();
    let ok = s.all_passed() && !s.truncated();
    (ok, if ok { String::new() } else { s.red_items_text() })
}
