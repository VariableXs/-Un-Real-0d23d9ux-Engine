//! F146 插件化星图后端 · 完整设计（STAR I 主册 G-D-21）。
//!
//! **判据（主册）**：三源切换查询结果一致（同版本数据）；回退链
//! 实测；签名拦截恶意源样本。
//!
//! **设计要点（主册）**：三源可配（本地文件/官方源/社区镜像，自定义
//! URL 显式警告）；优先级与签名校验链不变；签名失败 → 弃用+告警
//! （不静默降级到未校验数据）；全部源不可达 → 本地缓存只读可用
//! （降级诚实标注）；缓存按源隔离目录（防污染）；健康检查每 6h；
//! 同步差量协议复用 F128 since 参数；官方源默认首位（用户可改）。
//!
//! 本模块是数据源路由的**纯逻辑核**：源注册与优先级、签名校验门、
//! 回退链状态机、缓存隔离、健康检查节拍。

use crate::checks::CheckSet;
use crate::stareco::ebase::fnv1a64;

// ---------------------------------------------------------------------------
// 规格
// ---------------------------------------------------------------------------

/// 健康检查节拍（毫秒）：6h。
pub const HEALTH_CHECK_INTERVAL_MS: u64 = 6 * 3600 * 1000;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SourceKind {
    LocalFile,
    Official,
    Mirror,
    Custom,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SourceHealth {
    Ok,
    /// 签名失败 → 弃用+告警（不是降级使用）。
    SignatureRevoked,
    /// 不可达（回退链下一源接手）。
    Unreachable,
}

/// 一个数据源。
#[derive(Clone, Copy, Debug)]
pub struct StarSource {
    pub name: &'static str,
    pub kind: SourceKind,
    pub health: SourceHealth,
    /// 数据版本指纹（同版本一致性判据的锚）。
    pub data_fp: u64,
    /// 最后同步时间戳。
    pub last_sync_ms: u64,
}

impl StarSource {
    /// 自定义源警告确认（自担风险显式化——未确认不可启用）。
    pub fn custom_confirmed(&self, confirmed: bool) -> bool {
        self.kind != SourceKind::Custom || confirmed
    }
}

// ---------------------------------------------------------------------------
// 回退链路由
// ---------------------------------------------------------------------------

pub struct SourceRouter {
    /// 优先级序（下标小 = 优先；官方默认首位——用户可改）。
    sources: [Option<StarSource>; 4],
    count: usize,
    /// 回退事件计数。
    pub failovers: u32,
    /// 签名拦截计数。
    pub signature_blocks: u32,
    /// 缓存目录隔离标记（按源名 FNV 派生——目录名互异）。
    cache_dirs: [u64; 4],
}

impl SourceRouter {
    pub fn new() -> SourceRouter {
        SourceRouter {
            sources: [None; 4],
            count: 0,
            failovers: 0,
            signature_blocks: 0,
            cache_dirs: [0; 4],
        }
    }

    pub fn len(&self) -> usize {
        self.count
    }

    /// 注册源：自定义源未确认拒绝；缓存目录按源隔离派生。
    pub fn register(&mut self, s: StarSource, custom_confirmed: bool) -> Result<(), &'static str> {
        if !s.custom_confirmed(custom_confirmed) {
            return Err("自定义源需显式确认（自担风险）");
        }
        if self.count >= 4 {
            return Err("router full");
        }
        self.cache_dirs[self.count] = fnv1a64(s.name.as_bytes()) | 0x8000_0000_0000_0000;
        self.sources[self.count] = Some(s);
        self.count += 1;
        Ok(())
    }

    /// 查询路由：按优先级走链——签名 revoked 的源直接拦截（不静默
    /// 降级），unreachable 的回退下一源；全链断 → 本地缓存只读可用
    /// （降级诚实标注 Ok(Err("cache-readonly"))）。
    pub fn query(&mut self, want_fp: u64) -> Result<&'static str, &'static str> {
        for slot in self.sources[..self.count].iter_mut().flatten() {
            match slot.health {
                SourceHealth::SignatureRevoked => {
                    self.signature_blocks += 1;
                    continue; // 拦截+告警计数，绝不出数据
                }
                SourceHealth::Unreachable => {
                    self.failovers += 1;
                    continue; // 回退下一源
                }
                SourceHealth::Ok => {
                    if slot.data_fp == want_fp {
                        return Ok(slot.name);
                    }
                    self.failovers += 1; // 版本不符也回退（同版本一致判据）
                }
            }
        }
        Err("cache-readonly") // 全链不可用：本地缓存只读，诚实标注
    }

    /// 缓存隔离自证：任意两源目录名互异。
    pub fn cache_isolated(&self) -> bool {
        for i in 0..self.count {
            for j in i + 1..self.count {
                if self.cache_dirs[i] == self.cache_dirs[j] {
                    return false;
                }
            }
        }
        true
    }

    /// 健康检查节拍：距上次同步超过 6h 的源需检查。
    pub fn needs_health_check(&self, now_ms: u64) -> bool {
        self.sources[..self.count].iter().flatten().any(|s| {
            now_ms.saturating_sub(s.last_sync_ms) >= HEALTH_CHECK_INTERVAL_MS
        })
    }

    pub fn mark_revoked(&mut self, name: &str) -> Result<(), &'static str> {
        self.sources[..self.count]
            .iter_mut()
            .flatten()
            .find(|s| s.name == name)
            .map(|s| {
                s.health = SourceHealth::SignatureRevoked;
            })
            .ok_or("unknown source")
    }

    pub fn mark_unreachable(&mut self, name: &str) -> Result<(), &'static str> {
        self.sources[..self.count]
            .iter_mut()
            .flatten()
            .find(|s| s.name == name)
            .map(|s| {
                s.health = SourceHealth::Unreachable;
            })
            .ok_or("unknown source")
    }
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

pub const F146_TAG: &str = "stareco-F146-starmapprov";

pub fn run_starmapprov_checks() -> CheckSet {
    let mut set = CheckSet::new(F146_TAG);
    let fp_v1 = fnv1a64(b"catalog@2026q3");

    // 三源注册（官方/镜像/本地——官方默认首位）
    let mut router = SourceRouter::new();
    assert!(router
        .register(StarSource { name: "official", kind: SourceKind::Official, health: SourceHealth::Ok, data_fp: fp_v1, last_sync_ms: 0 }, false)
        .is_ok());
    assert!(router
        .register(StarSource { name: "mirror-x", kind: SourceKind::Mirror, health: SourceHealth::Ok, data_fp: fp_v1, last_sync_ms: 0 }, false)
        .is_ok());
    assert!(router
        .register(StarSource { name: "local-cache", kind: SourceKind::LocalFile, health: SourceHealth::Ok, data_fp: fp_v1, last_sync_ms: 0 }, false)
        .is_ok());
    set.add("f146 three sources up", router.len() == 3, "official first");

    // 自定义源未确认拒收
    set.add(
        "f146 custom needs confirmation",
        router
            .register(StarSource { name: "my-src", kind: SourceKind::Custom, health: SourceHealth::Ok, data_fp: fp_v1, last_sync_ms: 0 }, false)
            .is_err(),
        "explicit risk ack",
    );
    assert!(router
        .register(StarSource { name: "my-src", kind: SourceKind::Custom, health: SourceHealth::Ok, data_fp: fp_v1, last_sync_ms: 0 }, true)
        .is_ok());

    // 三源切换查询结果一致（同版本数据）
    set.add("f146 official answers", router.query(fp_v1) == Ok("official"), "first priority");
    // 镜像数据同版本也答同内容
    router.mark_unreachable("official").ok();
    set.add("f146 mirror consistent", router.query(fp_v1) == Ok("mirror-x"), "same version same data");
    set.add("f146 failover counted", router.failovers >= 1, "chain walked");
    // 本地兜底
    router.mark_unreachable("mirror-x").ok();
    set.add("f146 local fallback", router.query(fp_v1) == Ok("local-cache"), "third link");
    // 全链断 → 只读降级诚实标注（含后注册的自定义源）
    router.mark_unreachable("local-cache").ok();
    router.mark_unreachable("my-src").ok();
    set.add("f146 all-down honest", router.query(fp_v1) == Err("cache-readonly"), "read-only labeled");

    // 签名拦截恶意源：revoked 永不出数据（即便排最前）
    let mut hostile = SourceRouter::new();
    assert!(hostile
        .register(StarSource { name: "evil", kind: SourceKind::Mirror, health: SourceHealth::SignatureRevoked, data_fp: fp_v1, last_sync_ms: 0 }, false)
        .is_ok());
    assert!(hostile
        .register(StarSource { name: "official", kind: SourceKind::Official, health: SourceHealth::Ok, data_fp: fp_v1, last_sync_ms: 0 }, false)
        .is_ok());
    set.add(
        "f146 malicious source blocked",
        hostile.query(fp_v1) == Ok("official") && hostile.signature_blocks == 1,
        "no silent downgrade",
    );

    // 缓存按源隔离
    set.add("f146 cache dirs isolated", router.cache_isolated(), "no cross-pollution");

    // 健康检查节拍 6h
    set.add("f146 health tick 6h", router.needs_health_check(HEALTH_CHECK_INTERVAL_MS), "at interval");
    set.add("f146 health fresh skip", !router.needs_health_check(HEALTH_CHECK_INTERVAL_MS - 1), "before interval");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_mismatch_fails_over() {
        let mut r = SourceRouter::new();
        r.register(StarSource { name: "a", kind: SourceKind::Official, health: SourceHealth::Ok, data_fp: 999, last_sync_ms: 0 }, false).unwrap();
        r.register(StarSource { name: "b", kind: SourceKind::Mirror, health: SourceHealth::Ok, data_fp: 1, last_sync_ms: 0 }, false).unwrap();
        // a 版本陈旧（fp 不符）→ 回退 b（同版本一致判据）
        assert_eq!(r.query(1), Ok("b"));
        assert_eq!(r.failovers, 1);
    }
}
