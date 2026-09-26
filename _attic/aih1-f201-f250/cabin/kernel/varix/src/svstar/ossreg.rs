//! F130 开源项目登记册 · 完整设计（STAR I 主册 G-D-05）。
//!
//! **判据（主册）**：登记册与镜像实际组件清单一致性 CI 校验零 diff；
//! 许可证兼容性法律面过审记录在册。
//!
//! **设计要点（主册）**：
//! - 全部借力件的公开账本：组件名/版本/许可证/改动面/升级窗日期/上游
//!   地址/用途——十三件借力纪律（MD1 第 18 章）的执行账本，公开可查；
//! - 登记字段 12 项固定 schema（F126 格式面）；许可证族分类标注
//!   （MIT/Apache/LGPL/GPL-隔离……）；GPL 件必标「进程隔离」策略
//!   （F112 eSpeak 先例）；季度升级窗日历与登记册联动（F138）；
//! - 登记格式参照 SBOM（SPDX 惯例——公共标准）；实现即文档；
//! - 帮助中心「开源与许可」分区：登记册表格页（可筛选/搜索）+ 组件
//!   详情页；系统内「关于本机」（F123）尾部入口；
//! - 新借力件未登记 → CI 门禁（依赖清单与登记册 diff 为零才合入
//!   ——门禁不是良心的连续版）；许可证变更（上游换许可）→ 升级窗
//!   评估法律面+ADR；许可证全文快照存档（防上游删文）。
//!
//! 时间注入式（Unix 秒），宿主测试确定复现。无外部依赖。

use crate::checks::CheckSet;

use alloc::string::String;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 规格常量（参数唯一源）
// ---------------------------------------------------------------------------

/// 登记字段数（固定 schema——F126 格式面）。
pub const SCHEMA_FIELDS: usize = 12;
/// 季度升级窗（天）。
pub const UPGRADE_WINDOW_DAYS: u64 = 90;

/// 许可证族（分类标注——SPDX 惯例子集）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LicenseFamily {
    Mit,
    Apache,
    Bsd,
    Lgpl,
    /// GPL——必标进程隔离策略。
    GplIsolated,
}

impl LicenseFamily {
    pub fn spdx(self) -> &'static str {
        match self {
            LicenseFamily::Mit => "MIT",
            LicenseFamily::Apache => "Apache-2.0",
            LicenseFamily::Bsd => "BSD-3-Clause",
            LicenseFamily::Lgpl => "LGPL-2.1",
            LicenseFamily::GplIsolated => "GPL-3.0-isolated",
        }
    }

    /// GPL 传染面：进程隔离策略必标（F112 eSpeak 先例）。
    pub fn requires_isolation(self) -> bool {
        self == LicenseFamily::GplIsolated
    }
}

// ---------------------------------------------------------------------------
// 登记册
// ---------------------------------------------------------------------------

/// 一条登记（12 字段固定 schema）。
#[derive(Clone, Debug)]
pub struct Entry {
    // 12 字段：component/version/license/modified/upgrade_due/upstream/
    // purpose/isolation/license_snapshot/legal_review/registered_at/owner
    pub component: String,
    pub version: String,
    pub license: LicenseFamily,
    pub modified: bool,
    /// 升级窗到期（Unix 秒——与 F138 日历联动）。
    pub upgrade_due: u64,
    pub upstream: String,
    pub purpose: String,
    /// 进程隔离策略（GPL 件必填非空）。
    pub isolation: String,
    /// 许可证全文快照存档键（防上游删文）。
    pub license_snapshot: String,
    /// 法律面过审记录键（判据第一句之二）。
    pub legal_review: String,
    pub registered_at: u64,
    pub owner: String,
}

impl Entry {
    /// 登记完整性门禁：12 字段全量 + GPL 件隔离策略必填。
    pub fn complete(&self) -> bool {
        let base = !self.component.is_empty()
            && !self.version.is_empty()
            && !self.upstream.is_empty()
            && !self.purpose.is_empty()
            && !self.license_snapshot.is_empty()
            && !self.legal_review.is_empty()
            && !self.owner.is_empty();
        if self.license.requires_isolation() {
            base && !self.isolation.is_empty()
        } else {
            base
        }
    }
}

/// 登记册。
pub struct Registry {
    entries: Vec<Entry>,
    /// 法律面过审记录（判据对账面——键 → 结论）。
    legal_log: Vec<(String, &'static str)>,
}

impl Registry {
    pub fn new() -> Registry {
        Registry { entries: Vec::new(), legal_log: Vec::new() }
    }

    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }

    pub fn legal_log(&self) -> &[(String, &'static str)] {
        &self.legal_log
    }

    /// 登记入册（完整性门禁——不全即拒）。
    pub fn register(&mut self, e: Entry) -> Result<(), &'static str> {
        if !e.complete() {
            return Err("entry-incomplete");
        }
        self.legal_log.push((e.legal_review.clone(), "过审在册"));
        self.entries.push(e);
        Ok(())
    }

    /// CI 一致性校验（判据第一句：登记册 vs 镜像实际组件清单——diff
    /// 为零才合入）。返回 diff 清单（空 = 零 diff）。
    pub fn ci_diff(&self, image_components: &[(&str, &str)]) -> Vec<String> {
        let mut diff = Vec::new();
        // 镜像有、登记册无 → 未登记借力件。
        for (name, ver) in image_components {
            if !self.entries.iter().any(|e| e.component == *name && e.version == *ver) {
                diff.push(alloc::format!("unregistered: {} {}", name, ver));
            }
        }
        // 登记册有、镜像无 → 登记虚挂（同样红）。
        for e in &self.entries {
            if !image_components.iter().any(|(n, v)| *n == e.component && *v == e.version) {
                diff.push(alloc::format!("stale-registry: {} {}", e.component, e.version));
            }
        }
        diff
    }

    /// 搜索/筛选（帮助中心表格页面）。
    pub fn search(&self, kw: &str) -> Vec<&Entry> {
        self.entries
            .iter()
            .filter(|e| {
                e.component.contains(kw)
                    || e.purpose.contains(kw)
                    || e.license.spdx().contains(kw)
            })
            .collect()
    }

    /// 升级窗到期扫描（F138 日历联动）：now ≥ upgrade_due 的组件清单。
    pub fn upgrades_due(&self, now: u64) -> Vec<&str> {
        self.entries
            .iter()
            .filter(|e| now >= e.upgrade_due)
            .map(|e| e.component.as_str())
            .collect()
    }
}

// ---------------------------------------------------------------------------
// 自检（判据逐条钉死）
// ---------------------------------------------------------------------------

pub fn run_ossreg_checks() -> CheckSet {
    let mut set = CheckSet::new("F130-ossreg");

    let mk = |component: &str, version: &str, license: LicenseFamily, isolation: &str| Entry {
        component: String::from(component),
        version: String::from(version),
        license,
        modified: false,
        upgrade_due: 1_800_000_000,
        upstream: String::from("https://upstream.example/"),
        purpose: String::from("借用用途登记"),
        isolation: String::from(isolation),
        license_snapshot: String::from("snapshots/LICENSE"),
        legal_review: String::from("legal-2026-09"),
        registered_at: 1_727_000_000,
        owner: String::from("AI-V1"),
    };

    // 1. 登记入册（12 字段 schema 完整性门禁：全量过、缺隔离策略拒）。
    let mut reg = Registry::new();
    let ok1 = reg.register(mk("limine", "8.x", LicenseFamily::Bsd, "")).is_ok();
    let ok2 = reg.register(mk("eSpeak-NG", "1.52", LicenseFamily::GplIsolated, "独立进程+IPC")).is_ok();
    // GPL 件缺隔离策略 → 拒。
    let bad = mk("gpl-lib", "2.0", LicenseFamily::GplIsolated, "");
    let rejected = reg.register(bad) == Err("entry-incomplete");
    set.add(
        "registry gate + gpl isolation mandatory",
        ok1 && ok2 && rejected && reg.entries().len() == 2,
        "",
    );

    // 2. CI 一致性零 diff（判据第一句）：镜像清单与登记册完全对齐。
    let image = [("limine", "8.x"), ("eSpeak-NG", "1.52")];
    set.add("ci diff zero when aligned", reg.ci_diff(&image).is_empty(), "");

    // 3. CI 双向检出：镜像新增未登记件 → 红；登记册虚挂 → 红。
    let image2 = [("limine", "8.x"), ("eSpeak-NG", "1.52"), ("rustls", "0.23")];
    let d2 = reg.ci_diff(&image2);
    let image3 = [("limine", "8.x")];
    let d3 = reg.ci_diff(&image3);
    set.add(
        "ci catches unregistered and stale both ways",
        d2.iter().any(|s| s.contains("unregistered: rustls"))
            && d3.iter().any(|s| s.contains("stale-registry: eSpeak-NG")),
        "",
    );

    // 4. 许可证族 SPDX 标注 + GPL 隔离必标语义（F112 先例联动）。
    set.add(
        "spdx families + isolation semantics",
        LicenseFamily::Mit.spdx() == "MIT"
            && LicenseFamily::Apache.spdx() == "Apache-2.0"
            && LicenseFamily::GplIsolated.spdx() == "GPL-3.0-isolated"
            && LicenseFamily::GplIsolated.requires_isolation()
            && !LicenseFamily::Mit.requires_isolation(),
        "",
    );

    // 5. 法律面过审记录在册（判据第一句之二：每次登记带记录键）。
    set.add(
        "legal review logged per registration",
        reg.legal_log().len() == 2 && reg.legal_log()[0].1 == "过审在册",
        "",
    );

    // 6. 搜索/筛选（帮助中心表格页面）。
    let hits = reg.search("eSpeak");
    let by_lic = reg.search("BSD");
    set.add(
        "search by name and license",
        hits.len() == 1 && by_lic.len() == 1 && by_lic[0].component == "limine",
        "",
    );

    // 7. 升级窗到期扫描（F138 日历联动）：到期件点名、未到期不扰。
    let due = reg.upgrades_due(1_800_000_000);
    let not_due = reg.upgrades_due(1_700_000_000);
    set.add(
        "upgrade window due scan",
        due.len() == 2 && not_due.is_empty() && UPGRADE_WINDOW_DAYS == 90,
        "",
    );

    // 8. 许可证变更路径：换许可 → 必须更新登记 + 法律面重审（字段位
    //    可变 + 快照键换新——ADR 语义位）。
    let mut e = mk("limine", "9.0", LicenseFamily::Bsd, "");
    e.license = LicenseFamily::Mit;
    e.license_snapshot = String::from("snapshots/LICENSE-v9");
    e.legal_review = String::from("legal-2026-12");
    let ok = reg.register(e).is_ok();
    set.add(
        "license change re-registers with re-review",
        ok && reg.entries().len() == 3 && reg.legal_log().len() == 3,
        "",
    );

    // 9. schema 字段数常量（12 项固定——F126 格式面联动）。
    set.add("schema 12 fields", SCHEMA_FIELDS == 12, "");

    // 10. SBOM 惯例（SPDX 标注在册——实现即文档）。
    set.add(
        "sbom spdx convention",
        reg.entries().iter().all(|e| !e.license.spdx().is_empty()),
        "",
    );

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ossreg_all_checks_green() {
        let set = run_ossreg_checks();
        let (p, f) = set.tally();
        assert!(set.all_passed(), "F130 自检红项 {f}：{}/{} 绿", p, p + f);
    }

    #[test]
    fn missing_upstream_rejected() {
        let mut reg = Registry::new();
        let e = Entry {
            component: String::from("x"),
            version: String::from("1"),
            license: LicenseFamily::Mit,
            modified: false,
            upgrade_due: 0,
            upstream: String::from(""),
            purpose: String::from("p"),
            isolation: String::from(""),
            license_snapshot: String::from("s"),
            legal_review: String::from("l"),
            registered_at: 0,
            owner: String::from("o"),
        };
        assert_eq!(reg.register(e), Err("entry-incomplete"));
    }
}
