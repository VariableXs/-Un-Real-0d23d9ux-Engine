//! 深化层四 · F146 插件化星图后端（2026-09-27 深化批次四 · g 层）。
//!
//! 插件加载器状态机（发现→验签→注册→就绪，失败隔离检疫）、插件
//! API 版本协商（取公共最大）、镜像清单差异计算、配额管理（窗口
//! 计数/字节）、降级决策树。

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 插件加载器：Discovered→Verified→Registered→Ready；验签败→Quarantined
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PluginStage {
    Discovered,
    Verified,
    Registered,
    Ready,
    Quarantined,
}

pub struct PluginLoader {
    pub name: &'static str,
    pub stage: PluginStage,
}

impl PluginLoader {
    pub fn verify(&mut self, signer_ok: bool) -> Result<(), &'static str> {
        if self.stage != PluginStage::Discovered {
            return Err("验签只在发现段执行");
        }
        if signer_ok {
            self.stage = PluginStage::Verified;
            Ok(())
        } else {
            // 验签失败 → 检疫（不删除，可溯；永不静默丢弃）。
            self.stage = PluginStage::Quarantined;
            Err("验签失败：已检疫")
        }
    }

    pub fn advance(&mut self, to: PluginStage) -> Result<(), &'static str> {
        let legal = match (self.stage, to) {
            (PluginStage::Verified, PluginStage::Registered)
            | (PluginStage::Registered, PluginStage::Ready) => true,
            _ => false,
        };
        if !legal {
            return Err("非法插件迁移");
        }
        self.stage = to;
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// 插件 API 版本协商：host 支持集 × plugin 需要 → 公共最大；无交集拒绝
// ---------------------------------------------------------------------------

pub fn negotiate_api(host_supports: &[u32], plugin_wants: &[u32]) -> Result<u32, &'static str> {
    let common: alloc::vec::Vec<u32> = host_supports
        .iter()
        .filter(|h| plugin_wants.contains(h))
        .copied()
        .collect();
    if common.is_empty() {
        return Err("无公共 API 版本：协商失败（诚实拒绝，不降级猜测）");
    }
    Ok(*common.iter().max().expect("non-empty"))
}

// ---------------------------------------------------------------------------
// 镜像清单差异：两源 (名, 版本) → 仅A/仅B/版本分叉
// ---------------------------------------------------------------------------

pub struct MirrorDiff {
    pub only_a: alloc::vec::Vec<&'static str>,
    pub only_b: alloc::vec::Vec<&'static str>,
    pub diverged: alloc::vec::Vec<&'static str>,
}

pub fn mirror_diff(
    a: &[(&'static str, u32)],
    b: &[(&'static str, u32)],
) -> MirrorDiff {
    let mut d = MirrorDiff { only_a: alloc::vec::Vec::new(), only_b: alloc::vec::Vec::new(), diverged: alloc::vec::Vec::new() };
    for (n, v) in a {
        match b.iter().find(|(bn, _)| bn == n) {
            None => d.only_a.push(n),
            Some((_, bv)) if bv != v => d.diverged.push(n),
            _ => {}
        }
    }
    for (n, _) in b {
        if !a.iter().any(|(an, _)| an == n) {
            d.only_b.push(n);
        }
    }
    d
}

// ---------------------------------------------------------------------------
// 配额管理：每源 (请求数, 字节) 双计数，窗口重置
// ---------------------------------------------------------------------------

pub struct SourceQuota {
    pub max_requests: u32,
    pub max_bytes: u32,
    requests: u32,
    bytes: u32,
}

impl SourceQuota {
    pub fn new(max_requests: u32, max_bytes: u32) -> SourceQuota {
        SourceQuota { max_requests, max_bytes, requests: 0, bytes: 0 }
    }

    pub fn charge(&mut self, bytes: u32) -> Result<(), &'static str> {
        if self.requests + 1 > self.max_requests {
            return Err("请求超配额");
        }
        if self.bytes as u64 + bytes as u64 > self.max_bytes as u64 {
            return Err("字节超配额");
        }
        self.requests += 1;
        self.bytes += bytes;
        Ok(())
    }

    pub fn reset(&mut self) {
        self.requests = 0;
        self.bytes = 0;
    }

    pub fn used(&self) -> (u32, u32) {
        (self.requests, self.bytes)
    }
}

// ---------------------------------------------------------------------------
// 降级决策树：健康源→镜像→本地缓存→只读离线（逐级判定）
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DegradedLevel {
    Primary,
    Mirror,
    LocalCache,
    Offline,
}

/// 判定：主源健康→Primary；主坏镜像好→Mirror；双坏有缓存→LocalCache；
/// 全坏→Offline（每一级都有诚实标注）。
pub fn degrade(primary_ok: bool, mirror_ok: bool, cache_present: bool) -> DegradedLevel {
    if primary_ok {
        DegradedLevel::Primary
    } else if mirror_ok {
        DegradedLevel::Mirror
    } else if cache_present {
        DegradedLevel::LocalCache
    } else {
        DegradedLevel::Offline
    }
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

pub const F146G_TAG: &str = "stareco-F146-deep4";

pub fn run_f146_deep4_checks() -> CheckSet {
    let mut set = CheckSet::new(F146G_TAG);

    // 插件加载器
    let mut p = PluginLoader { name: "starmap-weather", stage: PluginStage::Discovered };
    set.add("f146g verify fail", p.verify(false).is_err() && p.stage == PluginStage::Quarantined, "验签败检疫");
    let mut p2 = PluginLoader { name: "ok", stage: PluginStage::Discovered };
    let _ = p2.verify(true);
    set.add("f146g skip ready", p2.advance(PluginStage::Ready).is_err(), "未注册直达就绪拒绝");
    let _ = p2.advance(PluginStage::Registered);
    let _ = p2.advance(PluginStage::Ready);
    set.add("f146g ready", p2.stage == PluginStage::Ready, "四段走通");

    // 版本协商
    set.add(
        "f146g negotiate",
        negotiate_api(&[1, 2, 3], &[2, 5]) == Ok(2),
        "公共最大版本",
    );
    set.add("f146g negotiate none", negotiate_api(&[1], &[2]).is_err(), "无交集拒绝");

    // 镜像差异
    let a = [("m1", 3u32), ("m2", 1), ("m3", 2)];
    let b = [("m1", 3u32), ("m2", 4), ("m4", 1)];
    let d = mirror_diff(&a, &b);
    set.add(
        "f146g diff",
        d.only_a == alloc::vec!["m3"]
            && d.only_b == alloc::vec!["m4"]
            && d.diverged == alloc::vec!["m2"],
        "三向差异",
    );

    // 配额
    let mut q = SourceQuota::new(2, 100);
    set.add("f146g charge ok", q.charge(60).is_ok() && q.charge(40).is_ok(), "额度内两次");
    set.add("f146g bytes over", q.charge(1).is_err(), "字节超限");
    q.reset();
    set.add("f146g reset", q.used() == (0, 0) && q.charge(10).is_ok(), "窗口重置");
    set.add("f146g req over", q.charge(10).is_ok() && q.charge(10).is_err(), "请求超限");

    // 降级树
    set.add("f146g lvl primary", degrade(true, true, true) == DegradedLevel::Primary, "主源健康");
    set.add("f146g lvl mirror", degrade(false, true, true) == DegradedLevel::Mirror, "镜像接棒");
    set.add("f146g lvl cache", degrade(false, false, true) == DegradedLevel::LocalCache, "本地缓存");
    set.add("f146g lvl offline", degrade(false, false, false) == DegradedLevel::Offline, "离线只读");

    set
}

#[cfg(test)]
mod deep4_tests {
    use super::*;

    #[test]
    fn quarantine_is_terminal() {
        let mut p = PluginLoader { name: "x", stage: PluginStage::Quarantined };
        assert!(p.advance(PluginStage::Registered).is_err());
        assert!(p.verify(true).is_err()); // 检疫段不可再验
    }

    #[test]
    fn negotiate_single() {
        assert_eq!(negotiate_api(&[7], &[7]), Ok(7));
        assert!(negotiate_api(&[], &[1]).is_err());
    }
}
