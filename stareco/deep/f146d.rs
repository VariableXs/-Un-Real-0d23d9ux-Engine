//! 深化层 · F146 插件化星图后端（2026-09-26 回炉补深化）。
//!
//! 补深：since 差量同步协议、源健康史（检查结果滚动账）、镜像搭建
//! 指南数据、并发同步互斥（同源不重入）。

use crate::checks::CheckSet;
use crate::stareco::ebase::fnv1a64;
use crate::stareco::starmapprov::{SourceHealth, SourceRouter, HEALTH_CHECK_INTERVAL_MS};

// ---------------------------------------------------------------------------
// since 差量同步协议（F128 复用）
// ---------------------------------------------------------------------------

/// 差量请求：客户端持本地版本指针 `since_fp`，服务端返回 (新指针, 变更数)。
pub struct DeltaSync {
    pub local_fp: u64,
    pub server_fp: u64,
}

impl DeltaSync {
    /// 本地已最新 → 零变更；落后 → 返回服务端指针（客户端拉差量）。
    /// 服务端比本地旧（不可能但防御）→ 显式报错不静默。
    pub fn plan(&self) -> Result<(u64, u32), &'static str> {
        if self.server_fp < self.local_fp {
            return Err("服务端版本指针落后于本地：同步链异常，拒绝降级");
        }
        if self.server_fp == self.local_fp {
            return Ok((self.local_fp, 0));
        }
        Ok((self.server_fp, 1))
    }
}

// ---------------------------------------------------------------------------
// 源健康史（检查结果滚动账）
// ---------------------------------------------------------------------------

pub struct HealthHistory {
    /// (时间戳, 连续失败计数)——最近一次检查的状态。
    last_fail_streak: u32,
    last_check_ms: u64,
    pub checks_total: u32,
}

impl HealthHistory {
    pub const fn new() -> HealthHistory {
        HealthHistory { last_fail_streak: 0, last_check_ms: 0, checks_total: 0 }
    }

    /// 记一次检查。连续失败 ≥3 → 建议摘牌（降优先级）。
    pub fn record(&mut self, now_ms: u64, ok: bool) -> bool {
        self.checks_total += 1;
        self.last_check_ms = now_ms;
        if ok {
            self.last_fail_streak = 0;
        } else {
            self.last_fail_streak += 1;
        }
        self.last_fail_streak >= 3
    }

    pub fn fail_streak(&self) -> u32 {
        self.last_fail_streak
    }

    /// 检查节拍到期判定（距上次检查 ≥ 6h）。
    pub fn due(&self, now_ms: u64) -> bool {
        now_ms.saturating_sub(self.last_check_ms) >= HEALTH_CHECK_INTERVAL_MS
    }
}

// ---------------------------------------------------------------------------
// 镜像搭建指南数据（F128 联动）
// ---------------------------------------------------------------------------

/// 镜像三步（指南章节数据）：同步全量 → 挂签名公钥 → 健康探针上线。
pub const MIRROR_STEPS: [&str; 3] = ["rsync 全量目录", "登记签名公钥", "healthz 探针上线"];

pub fn mirror_guide_complete(has: &[bool]) -> Result<(), &'static str> {
    if has.len() != MIRROR_STEPS.len() {
        return Err("步骤数不符");
    }
    if has.iter().any(|&b| !b) {
        return Err("镜像三步未完成：缺步不上目录");
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// 并发同步互斥（同源不重入）
// ---------------------------------------------------------------------------

pub struct SyncMutex {
    busy: Option<u64>, // 源指纹
}

impl SyncMutex {
    pub const fn new() -> SyncMutex {
        SyncMutex { busy: None }
    }

    pub fn acquire(&mut self, src_fp: u64) -> Result<(), &'static str> {
        if self.busy == Some(src_fp) {
            return Err("同源同步已在进行：不重入");
        }
        if self.busy.is_some() {
            return Err("同步器单槽：等待当前源完成");
        }
        self.busy = Some(src_fp);
        Ok(())
    }

    pub fn release(&mut self, src_fp: u64) -> Result<(), &'static str> {
        if self.busy != Some(src_fp) {
            return Err("释放非持有源");
        }
        self.busy = None;
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

pub const F146D_TAG: &str = "stareco-F146-deep";

pub fn run_f146_deep_checks() -> CheckSet {
    let mut set = CheckSet::new(F146D_TAG);

    // 差量同步
    let ds = DeltaSync { local_fp: 100, server_fp: 200 };
    set.add(
        "f146d delta plan",
        ds.plan() == Ok((200, 1)) && DeltaSync { local_fp: 200, server_fp: 200 }.plan() == Ok((200, 0)),
        "落后拉差量/同版零变更",
    );
    set.add(
        "f146d server regression refused",
        DeltaSync { local_fp: 200, server_fp: 100 }.plan().is_err(),
        "异常显性化",
    );

    // 健康史
    let mut hh = HealthHistory::new();
    let tripped = hh.record(0, false) || hh.record(1, false) || hh.record(2, false);
    set.add(
        "f146d three strikes",
        tripped && hh.fail_streak() == 3 && hh.checks_total == 3,
        "连败三次建议摘牌",
    );
    hh.record(3, true);
    set.add("f146d success resets streak", hh.fail_streak() == 0, "恢复清零");
    set.add(
        "f146d due beat",
        !hh.due(3 + HEALTH_CHECK_INTERVAL_MS - 1) && hh.due(3 + HEALTH_CHECK_INTERVAL_MS),
        "6h 节拍（自最后检查时刻起算）",
    );

    // 镜像指南
    set.add(
        "f146d mirror guide",
        mirror_guide_complete(&[true, true, true]).is_ok() && mirror_guide_complete(&[true, false, true]).is_err(),
        "三步缺一不上目录",
    );

    // 同步互斥
    let mut mx = SyncMutex::new();
    mx.acquire(fnv1a64(b"official")).ok();
    set.add(
        "f146d reentry blocked",
        mx.acquire(fnv1a64(b"official")).is_err() && mx.acquire(fnv1a64(b"mirror-x")).is_err(),
        "单槽互斥",
    );
    mx.release(fnv1a64(b"official")).ok();
    set.add("f146d release then acquire", mx.acquire(fnv1a64(b"mirror-x")).is_ok(), "释放后可换源");
    set.add("f146d wrong release refused", mx.release(fnv1a64(b"official")).is_err(), "释放校验");

    // 路由联动：健康史连败 → 路由层摘牌 → 查询走回退链
    let mut router = SourceRouter::new();
    router
        .register(crate::stareco::starmapprov::StarSource {
            name: "flaky",
            kind: crate::stareco::starmapprov::SourceKind::Mirror,
            health: SourceHealth::Unreachable,
            data_fp: 7,
            last_sync_ms: 0,
        }, false)
        .ok();
    router
        .register(crate::stareco::starmapprov::StarSource {
            name: "official",
            kind: crate::stareco::starmapprov::SourceKind::Official,
            health: SourceHealth::Ok,
            data_fp: 7,
            last_sync_ms: 0,
        }, false)
        .ok();
    set.add(
        "f146d unhealthy failover",
        router.query(7) == Ok("official") && router.failovers >= 1,
        "摘牌源回退",
    );

    set
}

#[cfg(test)]
mod deep_tests {
    use super::*;

    #[test]
    fn mutex_discipline() {
        let mut mx = SyncMutex::new();
        assert!(mx.acquire(1).is_ok());
        assert!(mx.release(1).is_ok());
        assert!(mx.release(1).is_err());
    }
}
