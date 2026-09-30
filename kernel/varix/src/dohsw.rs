//! dohsw — WP-204 · B-604 DoH 开关（MD2 篇 6.3）。
//!
//! 判据 B-604：切换即时生效，失败三要素。
//! MD2 原文（6.3）："DNS 解析走 hickory-dns 用户态库（借力清单），会话
//! 服务内置解析缓存与 DoH 开关（Q54）：默认用系统 DNS，设置中心可切
//! DoH；解析失败的三要素文案里带'请检查网络或切换 DoH'的下一步。"
//!
//! 宿主可测形态：双模式解析视图（系统 DNS / DoH 各一张固化结果表）+
//! 切换即时生效（切换即清缓存，下一次解析立即按新模式路由——无延迟窗口）
//! + 缓存命中语义（同模式连续查询第二次命中）+ 解析失败三要素文案
//! （WHAT/WHY/NEXT，NEXT 指向"请检查网络或切换 DoH"）。
//!
//! 数据锚定：系统视图对 github 域返回污染地址 0x14CDF3A6
//! （20.205.243.166——本仓库 push 战役实测污染段），DoH 视图返回可用
//! 地址 0x8C527204（140.82.114.4——实测可用段）。污染域是"切换 DoH"
//! 下一步文案的现实依据。

use crate::checks::CheckSet;

/// 解析视图域名数（固化表）。
pub const HOST_CAP: usize = 4;
/// 缓存容量（与域名数同宽——全量缓存即可）。
pub const CACHE_CAP: usize = HOST_CAP;

/// 域名索引（固化表坐标）。
pub const H_NORMAL: usize = 0; // 正常域：双视图一致
pub const H_GITHUB: usize = 1; // 系统视图污染、DoH 视图干净
pub const H_CDN: usize = 2; // 正常域：双视图一致（不同值亦合法——取一致值简化）
pub const H_DEAD: usize = 3; // 双视图都失败

/// 系统 DNS 视图：github 域返回污染地址（0x14CDF3A6 = 20.205.243.166）。
pub const SYS_GITHUB_POLLUTED: u32 = 0x14CD_F3A6;
/// DoH 视图：github 域返回可用地址（0x8C527204 = 140.82.114.4）。
pub const DOH_GITHUB_CLEAN: u32 = 0x8C52_7204;
/// 双视图一致域的地址。
pub const ADDR_NORMAL: u32 = 0x0102_0304;
pub const ADDR_CDN: u32 = 0x0506_0708;

/// DNS 模式（Q54 开关的两个挡位）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DnsMode {
    /// 默认：系统 DNS。
    System,
    /// 设置中心可切：DNS over HTTPS。
    Doh,
}

/// 解析结果视图（每模式一张固化表；None = 该视图解析失败）。
const fn view(mode: DnsMode, host: usize) -> Option<u32> {
    match (mode, host) {
        (_, H_DEAD) => None,
        (DnsMode::System, H_NORMAL) => Some(ADDR_NORMAL),
        (DnsMode::System, H_GITHUB) => Some(SYS_GITHUB_POLLUTED),
        (DnsMode::System, H_CDN) => Some(ADDR_CDN),
        (DnsMode::Doh, H_NORMAL) => Some(ADDR_NORMAL),
        (DnsMode::Doh, H_GITHUB) => Some(DOH_GITHUB_CLEAN),
        (DnsMode::Doh, H_CDN) => Some(ADDR_CDN),
        _ => None,
    }
}

/// 解析失败三要素（MD2 6.3：失败文案带下一步）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct FailTriple {
    pub what: &'static str,
    pub why: &'static str,
    pub next: &'static str,
}

/// 失败三要素文案（常量锁——NEXT 必须含"切换 DoH"）。
pub const FAIL_TRIPLE: FailTriple = FailTriple {
    what: "域名解析失败",
    why: "当前 DNS 模式下该域名无法解析",
    next: "请检查网络或切换 DoH",
};

/// 解析器：模式 + 缓存（切换即时生效的数据面）。
pub struct Resolver {
    pub mode: DnsMode,
    /// 缓存：host_idx → ip（切换时全清——旧模式的答案不残留）。
    pub cache: [Option<u32>; CACHE_CAP],
    pub hits: u64,
    pub misses: u64,
    pub failures: u64,
}

impl Resolver {
    pub const fn new() -> Resolver {
        Resolver { mode: DnsMode::System, cache: [None; CACHE_CAP], hits: 0, misses: 0, failures: 0 }
    }

    /// 设置中心切 DoH / 切回系统：**即时生效** = 模式立翻 + 缓存全清。
    pub fn switch(&mut self, mode: DnsMode) {
        if self.mode != mode {
            self.mode = mode;
            self.cache = [None; CACHE_CAP];
        }
    }

    /// 解析：缓存命中优先（同模式语义）；未命中走**当前模式**视图。
    pub fn resolve(&mut self, host: usize) -> Result<u32, FailTriple> {
        if host >= HOST_CAP {
            return Err(FAIL_TRIPLE);
        }
        if let Some(ip) = self.cache[host] {
            self.hits += 1;
            return Ok(ip);
        }
        self.misses += 1;
        match view(self.mode, host) {
            Some(ip) => {
                self.cache[host] = Some(ip);
                Ok(ip)
            }
            None => {
                self.failures += 1;
                Err(FAIL_TRIPLE)
            }
        }
    }

    pub fn cached(&self, host: usize) -> bool {
        host < HOST_CAP && self.cache[host].is_some()
    }
}

// ---------------------------------------------------------------- 对练

/// DoH 开关对练摘要。
#[derive(Default, PartialEq, Eq, Debug)]
pub struct DohDrillSummary {
    pub rounds: u32,
    pub switches: u64,
    /// 切换后第一次解析必按新模式（污染域：System 视图出污染值、Doh 视图出干净值）
    pub switch_immediate: bool,
    /// 缓存命中语义（同模式第二次查询命中）
    pub cache_hit: bool,
    /// 失败域必返三要素
    pub fail_triple: bool,
}

/// 随机切换 + 定向域名探针对练：切换即时生效的语义闭环。
pub fn run_doh_drills(seed: u64, rounds: u32) -> DohDrillSummary {
    let mut g = crate::comprecover::Lcg(seed);
    let mut sum = DohDrillSummary::default();
    sum.rounds = rounds;
    sum.switch_immediate = true;
    sum.cache_hit = true;
    sum.fail_triple = true;
    let mut r = Resolver::new();
    for _ in 0..rounds {
        // 随机切换
        let next = if g.next() % 2 == 0 { DnsMode::System } else { DnsMode::Doh };
        r.switch(next);
        sum.switches += 1;
        // 探针一：污染域必须立即反映新模式（切换后缓存已清，解析走新视图）
        match r.resolve(H_GITHUB) {
            Ok(ip) => {
                let expect = match r.mode {
                    DnsMode::System => SYS_GITHUB_POLLUTED,
                    DnsMode::Doh => DOH_GITHUB_CLEAN,
                };
                if ip != expect {
                    sum.switch_immediate = false;
                }
            }
            Err(_) => sum.switch_immediate = false,
        }
        // 探针二：同域名第二次解析必命中缓存
        let before_hits = r.hits;
        let _ = r.resolve(H_GITHUB);
        if r.hits != before_hits + 1 {
            sum.cache_hit = false;
        }
        // 探针三：失败域必返三要素
        if r.resolve(H_DEAD) != Err(FAIL_TRIPLE) {
            sum.fail_triple = false;
        }
    }
    sum
}

// ---------------------------------------------------------------- 自检

pub fn run_dohsw_checks() -> crate::checks::CheckSet {
    let mut set = CheckSet::new("B-604 DoH 开关");
    {
        // 默认系统 DNS
        let r = Resolver::new();
        set.add(
            "B-604 默认系统 DNS",
            r.mode == DnsMode::System,
            "MD2 6.3：默认用系统 DNS",
        );
    }
    {
        // 系统视图污染域出污染地址（实测锚定）
        let mut r = Resolver::new();
        set.add(
            "B-604 系统视图污染域出污染地址",
            r.resolve(H_GITHUB) == Ok(SYS_GITHUB_POLLUTED),
            "0x14CDF3A6 = 20.205.243.166（实测污染段）",
        );
    }
    {
        // 切换即时生效：切 DoH 后第一次解析即干净地址
        let mut r = Resolver::new();
        let _ = r.resolve(H_GITHUB); // 先在系统模式下解析（污染值进缓存）
        r.switch(DnsMode::Doh);
        set.add(
            "B-604 切换即时生效",
            r.mode == DnsMode::Doh
                && r.resolve(H_GITHUB) == Ok(DOH_GITHUB_CLEAN),
            "0x8C527204 = 140.82.114.4（实测可用段）",
        );
    }
    {
        // 切换清缓存（旧模式答案不残留）
        let mut r = Resolver::new();
        let _ = r.resolve(H_NORMAL);
        r.switch(DnsMode::Doh);
        set.add(
            "B-604 切换清缓存",
            r.cache.iter().all(|c| c.is_none()),
            "切换后旧缓存作废——即时生效的防残留面",
        );
    }
    {
        // 缓存命中语义（同模式内）
        let mut r = Resolver::new();
        let _ = r.resolve(H_NORMAL);
        let h0 = r.hits;
        let _ = r.resolve(H_NORMAL);
        set.add(
            "B-604 缓存命中",
            r.hits == h0 + 1 && r.misses == 1,
            "内置解析缓存（MD2 6.3）",
        );
    }
    {
        // 失败域三要素
        let mut r = Resolver::new();
        set.add(
            "B-604 失败域返三要素",
            r.resolve(H_DEAD) == Err(FAIL_TRIPLE) && r.failures == 1,
            "双模式都失败 → WHAT/WHY/NEXT",
        );
    }
    {
        // 三要素文案锁
        set.add(
            "B-604 三要素文案锁",
            !FAIL_TRIPLE.what.is_empty()
                && !FAIL_TRIPLE.why.is_empty()
                && FAIL_TRIPLE.next.contains("切换 DoH"),
            "MD2 6.3：三要素文案带'请检查网络或切换 DoH'的下一步",
        );
    }
    {
        // DoH 开关对练
        let sum = run_doh_drills(0xB604, 80);
        set.add(
            "B-604 DoH 开关对练",
            sum.rounds == 80 && sum.switch_immediate && sum.cache_hit && sum.fail_triple,
            "切换即时生效 + 失败三要素（判据原文）",
        );
    }
    set
}

// ---------------------------------------------------------------- 单测

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f704_default_system() {
        let mut r = Resolver::new();
        assert_eq!(r.mode, DnsMode::System);
        assert_eq!(r.resolve(H_GITHUB), Ok(SYS_GITHUB_POLLUTED));
    }

    #[test]
    fn f704_switch_immediate() {
        let mut r = Resolver::new();
        let _ = r.resolve(H_GITHUB);
        r.switch(DnsMode::Doh);
        // 切换后第一次解析必须走新模式（不能被旧缓存短路）
        assert_eq!(r.resolve(H_GITHUB), Ok(DOH_GITHUB_CLEAN));
        r.switch(DnsMode::System);
        assert_eq!(r.resolve(H_GITHUB), Ok(SYS_GITHUB_POLLUTED));
    }

    #[test]
    fn f704_cache_semantics() {
        let mut r = Resolver::new();
        let _ = r.resolve(H_CDN);
        assert!(r.cached(H_CDN));
        assert!(!r.cached(H_NORMAL));
        let h = r.hits;
        let _ = r.resolve(H_CDN);
        assert_eq!(r.hits, h + 1);
        // 同值切换不清缓存（模式未变）
        r.switch(DnsMode::System);
        assert!(r.cached(H_CDN));
    }

    #[test]
    fn f704_drill_deterministic() {
        let a = run_doh_drills(7, 40);
        let b = run_doh_drills(7, 40);
        assert_eq!(a, b);
        assert!(a.switch_immediate && a.cache_hit && a.fail_triple);
        assert_eq!(a.switches, 40);
    }
}
