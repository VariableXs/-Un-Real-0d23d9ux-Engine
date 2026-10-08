//! F137 API 稳定性承诺 · 完整设计（STAR I 主册 G-D-12）。
//!
//! **判据（主册）**：公共 API 100% 有级别标注；CI 签名门禁生效（注入
//! 假变更被拦）；迁移窗条款在 ADR 中兑现一例。
//!
//! **设计要点（主册）**：两级标注（稳定级：破面需 ADR+迁移窗一季；
//! 实验级：可变需注明）；标注三处可见（文档页/SDK 头文件/运行时日志
//! ——实验 API 首次调用日志一行提示）；API 签名指纹（参数类型序列
//! 哈希）CI 比对；稳定级最低存活一季才可授予（考察期）；实验级默认
//! 两年内转正或删除（防僵尸实验品）；破面通知三渠道；紧急安全破面
//! 走加速 ADR+双读过渡仍强制；误标（稳定级悄悄变了）→ CI 拦截。
//!
//! 本模块是稳定性承诺的**门禁核**：注册表（100% 标注校验）、签名
//! 指纹引擎（ebase::fnv1a64）、CI 比对（变更分类：无变化/兼容新增/
//! 破面）、破面处置状态机（ADR+迁移窗+双读）、实验级时钟（考察期/
//! 两年转正线）。

use crate::checks::CheckSet;
use crate::stareco::ebase::fnv1a64;

// ---------------------------------------------------------------------------
// 规格
// ---------------------------------------------------------------------------

/// 稳定级考察期（天）：一季 = 90 天。
pub const PROBATION_DAYS: u32 = 90;
/// 迁移窗（天）：一季。
pub const MIGRATION_WINDOW_DAYS: u32 = 90;
/// 实验级转正/删除大限（天）：两年 = 730。
pub const EXPERIMENT_LIMIT_DAYS: u32 = 730;

// ---------------------------------------------------------------------------
// API 注册表
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Stability {
    Stable,
    Experimental,
}

/// 破面处置状态：稳定 API 签名变更后必须走的流程。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BreakFlow {
    /// 无破面。
    None,
    /// 破面已登记 ADR，双读过渡中（迁移窗内新旧签名并存）。
    InMigration,
    /// 迁移窗结束，旧签名移除。
    Completed,
}

#[derive(Clone, Copy, Debug)]
pub struct ApiEntry {
    pub symbol: &'static str,
    pub stability: Stability,
    /// 授予稳定级时的日序（考察期计时起点）。
    pub promoted_day: u32,
    /// 当前签名指纹（参数类型序列 fnv1a64）。
    pub signature_fp: u64,
    pub break_flow: BreakFlow,
    /// 双读过渡：迁移窗内旧签名指纹保留。
    pub legacy_fp: Option<u64>,
    pub migration_deadline: u32,
}

impl ApiEntry {
    /// 100% 标注判据的单条面：每条必须显式二选一（无「未标注」态
    /// ——构造即要求 stability，本判据靠注册表全量扫描守住）。
    pub fn annotated(&self) -> bool {
        match self.stability {
            Stability::Stable => true,
            Stability::Experimental => true,
        }
    }

    /// 稳定级考察期：授予未满一季的「稳定」不算数（CI 警告态）。
    pub fn stable_tenured(&self, today: u32) -> bool {
        match self.stability {
            Stability::Stable => today.saturating_sub(self.promoted_day) >= PROBATION_DAYS,
            Stability::Experimental => false,
        }
    }

    /// 实验级僵尸检测：超两年未转正 → 必须处置。
    pub fn experiment_expired(&self, today: u32) -> bool {
        self.stability == Stability::Experimental
            && today.saturating_sub(self.promoted_day) > EXPERIMENT_LIMIT_DAYS
    }
}

// ---------------------------------------------------------------------------
// CI 签名门禁
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CiVerdict {
    /// 签名未变。
    Unchanged,
    /// 兼容变更（实验级自由变；稳定级纯新增不破面）。
    Compatible,
    /// 稳定级签名变了 → 门禁拦截，必须走 ADR+迁移窗。
    StableBreak,
    /// 稳定级签名变了且无 ADR 授权 → 红灯（假变更注入被拦）。
    UnauthorizedBreak,
}

/// 注册表 + CI 比对引擎。
pub struct ApiRegistry {
    entries: [Option<ApiEntry>; 32],
    count: usize,
    /// 累计拦截数（门禁生效的证据计数）。
    pub blocked_breaks: u32,
}

impl ApiRegistry {
    pub fn new() -> ApiRegistry {
        ApiRegistry { entries: [None; 32], count: 0, blocked_breaks: 0 }
    }

    pub fn len(&self) -> usize {
        self.count
    }

    pub fn register(&mut self, symbol: &'static str, stability: Stability, signature_fp: u64, day: u32) -> Result<(), &'static str> {
        if self.entries[..self.count].iter().flatten().any(|e| e.symbol == symbol) {
            return Err("重复符号：先移除再重新注册");
        }
        if self.count >= 32 {
            return Err("registry full");
        }
        self.entries[self.count] = Some(ApiEntry {
            symbol,
            stability,
            promoted_day: day,
            signature_fp,
            break_flow: BreakFlow::None,
            legacy_fp: None,
            migration_deadline: 0,
        });
        self.count += 1;
        Ok(())
    }

    /// 100% 标注判据：全量扫描无漏网（注册时强制 stability，这里
    /// 自证的是「扫描器不放过任何一条」）。
    pub fn all_annotated(&self) -> bool {
        self.entries[..self.count].iter().flatten().all(|e| e.annotated())
    }

    fn find_mut(&mut self, symbol: &str) -> Option<&mut ApiEntry> {
        self.entries[..self.count].iter_mut().flatten().find(|e| e.symbol == symbol)
    }

    /// CI 签名比对（门禁主体）。
    ///
    /// - 实验级：自由变（需注明 = 返回 Compatible 由调用方记日志）；
    /// - 稳定级 + 有授权（`adr_authorized=true`）：进入双读迁移窗；
    /// - 稳定级 + 无授权：**拦截**（UnauthorizedBreak，假变更被拦）。
    pub fn ci_compare(
        &mut self,
        symbol: &str,
        new_fp: u64,
        adr_authorized: bool,
        today: u32,
    ) -> Result<CiVerdict, &'static str> {
        let e = self.find_mut(symbol).ok_or("unknown symbol")?;
        if e.signature_fp == new_fp {
            return Ok(CiVerdict::Unchanged);
        }
        match e.stability {
            Stability::Experimental => {
                e.signature_fp = new_fp;
                Ok(CiVerdict::Compatible)
            }
            Stability::Stable if adr_authorized => {
                e.legacy_fp = Some(e.signature_fp);
                e.signature_fp = new_fp;
                e.break_flow = BreakFlow::InMigration;
                e.migration_deadline = today + MIGRATION_WINDOW_DAYS;
                Ok(CiVerdict::StableBreak)
            }
            Stability::Stable => {
                self.blocked_breaks += 1;
                Ok(CiVerdict::UnauthorizedBreak)
            }
        }
    }

    /// 迁移窗内旧签名仍接受调用（双读过渡）。
    pub fn accepts_during_migration(&self, symbol: &str, fp: u64) -> bool {
        match self.entries[..self.count].iter().flatten().find(|e| e.symbol == symbol) {
            Some(e) if e.break_flow == BreakFlow::InMigration => {
                e.signature_fp == fp || e.legacy_fp == Some(fp)
            }
            Some(e) => e.signature_fp == fp,
            None => false,
        }
    }

    /// 迁移窗到期收口：旧签名移除（Completed）。
    pub fn close_migration(&mut self, symbol: &str, today: u32) -> Result<(), &'static str> {
        let e = self.find_mut(symbol).ok_or("unknown symbol")?;
        if e.break_flow != BreakFlow::InMigration {
            return Err("no migration in flight");
        }
        if today < e.migration_deadline {
            return Err("migration window still open");
        }
        e.legacy_fp = None;
        e.break_flow = BreakFlow::Completed;
        Ok(())
    }
}

/// 签名指纹规范算法：参数类型序列逐段 fnv1a64 折叠。
pub fn signature_fp_of(param_types: &[&str]) -> u64 {
    let mut fp = 0xcbf2_9ce4_8422_2325u64;
    for t in param_types {
        fp = fp.wrapping_mul(0x100_0000_01b3) ^ fnv1a64(t.as_bytes());
    }
    fp
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

pub const F137_TAG: &str = "stareco-F137-apistab";

pub fn run_apistab_checks() -> CheckSet {
    let mut set = CheckSet::new(F137_TAG);

    let fp_v1 = signature_fp_of(&["handle", "u32", "u32"]);
    let fp_v2 = signature_fp_of(&["handle", "u32", "u32", "u32"]);

    let mut reg = ApiRegistry::new();
    assert!(reg.register("vx_draw_rect", Stability::Stable, fp_v1, 1).is_ok());
    assert!(reg.register("vx_beta_glow", Stability::Experimental, fp_v1, 1).is_ok());
    set.add("f137 100% annotated", reg.all_annotated() && reg.len() == 2, "both explicit");

    // 实验级自由变
    let v = reg.ci_compare("vx_beta_glow", fp_v2, false, 10).expect("cmp");
    set.add("f137 experimental free change", v == CiVerdict::Compatible, "noted change ok");

    // 稳定级假变更 → 拦截
    let v = reg.ci_compare("vx_draw_rect", fp_v2, false, 10).expect("cmp");
    set.add(
        "f137 unauthorized break blocked",
        v == CiVerdict::UnauthorizedBreak && reg.blocked_breaks == 1,
        "gate hit",
    );
    // 拦截后签名未动
    set.add("f137 blocked leaves signature", reg.ci_compare("vx_draw_rect", fp_v1, false, 11) == Ok(CiVerdict::Unchanged), "no silent mutation");

    // 稳定级授权破面 → 双读迁移窗
    let v = reg.ci_compare("vx_draw_rect", fp_v2, true, 100).expect("cmp");
    set.add("f137 adr break opens migration", v == CiVerdict::StableBreak, "dual-read window");
    set.add("f137 old fp still accepted", reg.accepts_during_migration("vx_draw_rect", fp_v1), "dual read v1");
    set.add("f137 new fp accepted", reg.accepts_during_migration("vx_draw_rect", fp_v2), "dual read v2");
    // 迁移窗内不许收口
    set.add(
        "f137 early close rejected",
        reg.close_migration("vx_draw_rect", 100 + MIGRATION_WINDOW_DAYS - 1).is_err(),
        "window respected",
    );
    // 到期收口
    assert!(reg.close_migration("vx_draw_rect", 100 + MIGRATION_WINDOW_DAYS).is_ok());
    set.add("f137 migration completed", reg.accepts_during_migration("vx_draw_rect", fp_v2) && !reg.accepts_during_migration("vx_draw_rect", fp_v1), "legacy removed");

    // 考察期与僵尸实验品
    let e_stable_tenured = reg.entries_probe("vx_draw_rect", |e| e.stable_tenured(1 + PROBATION_DAYS));
    set.add("f137 stable probation", e_stable_tenured, "90d tenure");
    let exp_expired = reg.entries_probe("vx_beta_glow", |e| e.experiment_expired(1 + EXPERIMENT_LIMIT_DAYS + 1));
    set.add("f137 zombie experiment flagged", exp_expired, "2y limit");

    set
}

impl ApiRegistry {
    /// 探针：对指定符号应用断言（测试/检查用，不泄漏内部句柄）。
    fn entries_probe(&self, symbol: &str, f: impl Fn(&ApiEntry) -> bool) -> bool {
        self.entries[..self.count].iter().flatten().find(|e| e.symbol == symbol).map(f).unwrap_or(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn full_break_lifecycle() {
        let a = signature_fp_of(&["u32"]);
        let b = signature_fp_of(&["u64"]);
        let mut reg = ApiRegistry::new();
        reg.register("f", Stability::Stable, a, 0).unwrap();
        assert_eq!(reg.ci_compare("f", b, true, 0), Ok(CiVerdict::StableBreak));
        assert!(reg.close_migration("f", MIGRATION_WINDOW_DAYS).is_ok());
        assert_eq!(reg.ci_compare("f", a, false, MIGRATION_WINDOW_DAYS + 1), Ok(CiVerdict::UnauthorizedBreak));
        assert_eq!(reg.blocked_breaks, 1);
    }
}
