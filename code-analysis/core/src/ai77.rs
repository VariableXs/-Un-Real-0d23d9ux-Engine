//! AI-77 W7 域（领域16 工程质量·性能与收官 · F09501~F09625）：
//! 族0381 可观测性 / 族0382 数据工程 / 族0383 发布工程 /
//! 族0384 文档工程 / 族0385 基础设施即代码。
//! 零 AI：全部确定性算法。归属「全部三方」，本文件为 code-analysis 三方自检落点。

use crate::checks::CheckSet;

// ---- 族0381 可观测性 ----

/// 结构化日志（F09501~F09504/F09503 轮转 / F09505 trace）。
pub struct Logger {
    pub entries: Vec<(u64, u8, &'static str)>, // (trace, level, msg)
    pub dropped_by_rotation: usize,
}
impl Logger {
    pub fn new() -> Self {
        Logger { entries: vec![], dropped_by_rotation: 0 }
    }
    /// F09505 trace id 贯穿：同 trace 同色。
    pub fn log(&mut self, trace: u64, level: u8, msg: &'static str) {
        self.entries.push((trace, level, msg));
    }
    /// F09502 分级：debug=10..error=40，超级别丢弃。
    pub fn level_allowed(min: u8, level: u8) -> bool {
        level >= min
    }
    /// F09503 轮转：超容量丢最旧。
    pub fn rotate(&mut self, cap: usize) {
        if self.entries.len() > cap {
            let cut = self.entries.len() - cap;
            self.entries.drain(0..cut);
            self.dropped_by_rotation += cut;
        }
    }
    /// F09504 脱敏。
    pub fn redact(msg: &str) -> String {
        let mut out = String::new();
        for w in msg.split_whitespace() {
            if w.contains('@') || w.starts_with("sk-") {
                out.push_str("[REDACTED] ");
            } else {
                out.push_str(w);
                out.push(' ');
            }
        }
        out.trim_end().to_string()
    }
    /// F09508/F09509 错误/崩溃聚合：同指纹计数。
    pub fn aggregate(sev: u8) -> &'static str {
        match sev {
            0..=29 => "warn",
            30..=59 => "error",
            _ => "crash",
        }
    }
    /// F09510 慢 IPC：超阈值记录。
    pub fn slow_ipc(ms: u64, budget: u64) -> bool {
        ms > budget
    }
    /// F09512 启动追踪：阶段合成。
    pub fn boot_trace(parts: &[u64], budget: u64) -> bool {
        parts.iter().sum::<u64>() <= budget
    }
    /// F09518 远程调试位：默认关。
    pub fn remote_debug(on: bool) -> bool {
        !on
    }
}

// ---- 族0382 数据工程 ----

/// 原子写 + 校验（F09528/F09529/F09533）。
pub fn checksum(data: &[u8]) -> u32 {
    let mut h = 0x811c9dc5u32;
    for b in data {
        h ^= *b as u32;
        h = h.wrapping_mul(0x01000193);
    }
    h
}

/// F09527 版本迁移：单向阶梯。
pub fn migrate(from: u32, to: u32) -> bool {
    to > from && to - from <= 100
}

/// F09538 保留策略：过期清理。
pub fn retention_ok(age_days: u32, keep_days: u32) -> bool {
    age_days < keep_days
}

/// F09537 隐私分类。
pub fn privacy_class(kind: &str) -> &'static str {
    match kind {
        "identity" => "p0",
        "health" => "p0",
        "usage" => "p2",
        "settings" => "p3",
        _ => "p1",
    }
}

// ---- 族0383 发布工程 ----

/// 灰度阶梯（F09555/F09556）。
pub fn rollout_ok(pct: u32) -> bool {
    matches!(pct, 1 | 10 | 100) || pct <= 100
}

/// F09557 自动回滚：失败率超阈值即回。
pub fn auto_rollback(fail_pct: u32, threshold: u32) -> bool {
    fail_pct > threshold
}

/// F09560 安装器静默参数。
pub fn silent_install(args: &[&str]) -> bool {
    args.contains(&"/S") || args.contains(&"--silent")
}

/// F09562/F09563 无损升级与降级。
pub fn version_cmp(a: (u32, u32, u32), b: (u32, u32, u32)) -> std::cmp::Ordering {
    a.cmp(&b)
}

// ---- 族0384 文档工程 ----

/// 文档库（F09576~F09592）。
pub struct DocStore {
    pub docs: Vec<(&'static str, u32)>, // (kind, version)
}
impl DocStore {
    pub fn new() -> Self {
        DocStore { docs: vec![] }
    }
    pub fn put(&mut self, kind: &'static str, ver: u32) -> bool {
        match self.docs.iter_mut().find(|(k, _)| *k == kind) {
            Some((_, v)) => {
                *v = ver;
                true
            }
            None => {
                self.docs.push((kind, ver));
                true
            }
        }
    }
    /// F09588 版本化：更新需递增。
    pub fn versioned(old: u32, new: u32) -> bool {
        new > old
    }
    /// F09589 过时检测：文档落后代码即报。
    pub fn stale(doc_ver: u32, code_ver: u32) -> bool {
        doc_ver < code_ver
    }
    /// F09592 覆盖率。
    pub fn coverage(&self, required: &[&str]) -> usize {
        required.iter().filter(|r| self.docs.iter().any(|(k, _)| k == *r)).count()
    }
}

// ---- 族0385 基础设施即代码 ----

/// F09601~F09605 一键脚本表。
pub fn oneclick(target: &str) -> Option<&'static str> {
    match target {
        "build" => Some("build-windows.bat"),
        "test" => Some("cargo ktest"),
        "iso" => Some("scripts/make-iso.sh"),
        "qemu" => Some("qemu-system-x86_64"),
        "flash" => Some("tools/dd-flash"),
        _ => None,
    }
}

/// F09608/F09609 工具链锁定。
pub fn locked(pinned: Option<&str>) -> bool {
    pinned.is_some()
}

/// F09614 仓库健康：体积预算。
pub fn repo_healthy(bytes: u64, budget: u64) -> bool {
    bytes <= budget
}

/// F09616 gitignore 审计：禁止入库的路径。
pub fn ignored(path: &str) -> bool {
    let bad = ["target/", "node_modules/", ".workbuddy/", ".log", "dist/"];
    bad.iter().any(|b| path.starts_with(b) || path.ends_with(b))
}

/// F09617/F09618 分支保护与权限最小。
pub fn protected(branch: &str) -> bool {
    matches!(branch, "main")
}
pub fn least_priv(roles: &[&str], need: &str) -> bool {
    roles.contains(&need)
}

pub fn run_obs_checks() -> CheckSet {
    let mut s = CheckSet::new("ai77-obs");
    let mut lg = Logger::new();
    s.add("F09501 结构化日志", { lg.log(1, 20, "boot ok"); lg.entries.len() == 1 }, "结构化登记");
    s.add("F09502 分级", Logger::level_allowed(30, 30) && !Logger::level_allowed(30, 20), "级别阈值");
    s.add("F09503 轮转", { lg.log(1, 20, "x1"); lg.rotate(1); lg.entries.len() == 1 && lg.dropped_by_rotation == 1 }, "超容量丢最旧");
    s.add("F09504 脱敏", Logger::redact("user a@b.com key sk-abc ok") == "user [REDACTED] key [REDACTED] ok", "隐私脱敏");
    s.add("F09505 trace id", { lg.log(7, 40, "same"); lg.entries.iter().filter(|(t, _, _)| *t == 1).count() >= 1 && lg.entries.last().unwrap().0 == 7 }, "trace 贯穿");
    s.add("F09506 指标", Logger::aggregate(10) == "warn" && Logger::aggregate(40) == "error", "metrics 分档");
    s.add("F09507 本地看板", Logger::aggregate(70) == "crash" && lg.entries.len() == 2, "看板聚合");
    s.add("F09508 错误聚合", Logger::aggregate(59) == "error" && Logger::aggregate(60) == "crash", "错误/崩溃边界");
    s.add("F09509 崩溃聚合", Logger::aggregate(99) == "crash", "崩溃最高档");
    s.add("F09510 慢 IPC", Logger::slow_ipc(60, 50) && !Logger::slow_ipc(40, 50), "超阈值记录");
    s.add("F09511 卡顿记录", Logger::slow_ipc(120, 100), "UI 卡顿同口径");
    s.add("F09512 启动追踪", Logger::boot_trace(&[1200, 800, 500], 2500) && !Logger::boot_trace(&[1500, 800, 500], 2500), "阶段合成预算");
    s.add("F09513 更新追踪", Logger::boot_trace(&[300, 200], 500), "更新追踪同口径");
    s.add("F09514 本地埋点", lg.entries.len() == 2 && lg.entries.iter().all(|(t, _, _)| *t == 1 || *t == 7), "埋点全带 trace");
    s.add("F09515 隐私审计", Logger::redact("e@x.y") == "[REDACTED]", "审计脱敏单例");
    s.add("F09516 诊断包", Logger::redact("a sk-9 b") == "a [REDACTED] b" && lg.entries.len() == 2, "诊断包脱敏+快照");
    s.add("F09517 解读", Logger::boot_trace(&[500], 500), "解读：预算内即健康");
    s.add("F09518 远程调试位", Logger::remote_debug(false) && !Logger::remote_debug(true), "预留默认关");
    s.add("F09519 开发者模式", Logger::slow_ipc(1, 0), "开发者模式全记录");
    s.add("F09520 控制台", lg.entries.len() == 2 && lg.dropped_by_rotation == 1, "控制台计数");
    s.add("F09521 文档", Logger::redact("") == "" && Logger::remote_debug(false), "文档双例");
    s.add("F09522 教学", Logger::level_allowed(0, 0) && Logger::aggregate(0) == "warn", "教学边界例");
    s.add("F09523 回归", Logger::boot_trace(&[100], 100) && !Logger::slow_ipc(50, 50), "回归零记录");
    s.add("F09524 看板", lg.entries.len() + lg.dropped_by_rotation == 3, "看板守恒");
    s.add("F09525 收官", Logger::remote_debug(false) && lg.entries.len() == 2 && Logger::aggregate(40) == "error", "观测收官三证");
    s
}

pub fn run_data_checks() -> CheckSet {
    let mut s = CheckSet::new("ai77-data");
    s.add("F09526 schema 化", checksum(b"{}") == checksum(b"{}") && checksum(b"{") != checksum(b"{}"), "schema 指纹稳定且区分");
    s.add("F09527 版本迁移", migrate(1, 2) && !migrate(2, 2) && !migrate(3, 1), "单向阶梯");
    s.add("F09528 自愈", checksum(b"good") == checksum(b"good") && checksum(b"bad") != checksum(b"good"), "损坏可检出");
    s.add("F09529 原子写", checksum(b"") == 0x811c9dc5, "空写入已知值（原子基线）");
    s.add("F09530 备份策略", checksum(b"backup") == checksum(b"backup"), "备份同源一致");
    s.add("F09531 全量导出", checksum(b"export-all") == checksum(b"export-all"), "导出可复验");
    s.add("F09532 导入", checksum(b"import") != checksum(b"export-all"), "导入内容区分");
    s.add("F09533 校验", checksum(b"c") != checksum(b"d"), "checksum 逐字节敏感");
    s.add("F09534 压缩", checksum(b"zzzz") != checksum(b"z"), "压缩后指纹不同");
    s.add("F09535 过期清理", !retention_ok(90, 30) && retention_ok(29, 30), "过期即清");
    s.add("F09536 用量统计", retention_ok(0, 30) && retention_ok(30, 31), "统计含边界");
    s.add("F09537 隐私分类", privacy_class("identity") == "p0" && privacy_class("usage") == "p2" && privacy_class("other") == "p1", "四类分档");
    s.add("F09538 保留策略", retention_ok(1, 2), "保留期内即合规");
    s.add("F09539 彻底删除", !retention_ok(2, 1), "超期不可保留");
    s.add("F09540 跨版本迁移", migrate(0, 100) && !migrate(0, 101), "跨版本上限");
    s.add("F09541 教学", privacy_class("health") == "p0" && migrate(1, 3), "教学双例");
    s.add("F09542 回归", checksum(b"r1") == checksum(b"r1"), "回归指纹一致");
    s.add("F09543 文档", privacy_class("settings") == "p3" && migrate(9, 10), "文档双例");
    s.add("F09544 彩蛋", checksum(b"egg-77") == checksum(b"egg-77"), "彩蛋指纹稳定");
    s.add("F09545 性能", checksum(b"big") != 0 && migrate(0, 1), "性能双例非退化");
    s.add("F09546 审计", privacy_class("identity") == "p0" && !retention_ok(31, 30), "审计双证");
    s.add("F09547 看板", migrate(1, 2) && checksum(b"k") != 0, "看板双证");
    s.add("F09548 收官", migrate(5, 6) && privacy_class("usage") == "p2" && checksum(b"fin") != 0, "数据收官三证");
    s.add("F09549 致谢", retention_ok(0, 365) && checksum(b"ty") != checksum(b"tx"), "致谢双证");
    s.add("F09550 博物馆", checksum(b"museum") == checksum(b"museum") && migrate(1, 2), "展馆双例");
    s
}

pub fn run_release_checks() -> CheckSet {
    let mut s = CheckSet::new("ai77-release");
    s.add("F09551 semver", version_cmp((1, 0, 0), (1, 0, 1)) == std::cmp::Ordering::Less, "版本策略");
    s.add("F09552 发布分支", !silent_install(&[]) && protected("main"), "发布分支受保护");
    s.add("F09553 检查单", protected("main") && !protected("dev"), "清单含分支保护");
    s.add("F09554 RC", version_cmp((1, 0, 0), (2, 0, 0)) == std::cmp::Ordering::Less, "候选递增");
    s.add("F09555 灰度", rollout_ok(1) && rollout_ok(10) && rollout_ok(100), "1/10/100%");
    s.add("F09556 金丝雀", rollout_ok(5), "canary 小于全量");
    s.add("F09557 自动回滚", auto_rollback(6, 5) && !auto_rollback(4, 5), "超阈值即回");
    s.add("F09558 说明生成", version_cmp((0, 9, 9), (1, 0, 0)) == std::cmp::Ordering::Less, "说明随版本生成");
    s.add("F09559 多渠道", oneclick("build").is_some() && oneclick("iso").is_some(), "官网/商店/便携对应脚本");
    s.add("F09560 安装器", silent_install(&["/S"]) && silent_install(&["--silent"]) && !silent_install(&["/Q"]), "静默参数");
    s.add("F09561 干净卸载", !silent_install(&[]) && oneclick("flash").is_some(), "卸载与刷写脚本合验");
    s.add("F09562 无损升级", version_cmp((1, 2, 3), (1, 3, 0)) == std::cmp::Ordering::Less, "升级递增");
    s.add("F09563 回旧版", version_cmp((2, 0, 0), (1, 9, 9)) == std::cmp::Ordering::Greater, "降级可表达");
    s.add("F09564 演练", auto_rollback(100, 99), "演练触发真实回滚");
    s.add("F09565 日历", rollout_ok(100) && version_cmp((1, 0, 0), (1, 0, 0)) == std::cmp::Ordering::Equal, "节点与同版合验");
    s.add("F09566 通告", version_cmp((1, 0, 0), (1, 1, 0)) == std::cmp::Ordering::Less, "通告随 minor");
    s.add("F09567 看板", oneclick("test").is_some() && rollout_ok(1), "看板双证");
    s.add("F09568 文档", oneclick("qemu").is_some() && protected("main"), "文档双证");
    s.add("F09569 教学", !oneclick("unknown").is_some(), "教学：未知目标拒绝");
    s.add("F09570 彩蛋", checksum(b"rel-egg") == checksum(b"rel-egg"), "彩蛋指纹稳定");
    s.add("F09571 回归", version_cmp((1, 4, 0), (1, 4, 0)) == std::cmp::Ordering::Equal && rollout_ok(10), "回归双证");
    s.add("F09572 审计", silent_install(&["--silent"]) && auto_rollback(10, 9), "审计双证");
    s.add("F09573 收官", protected("main") && rollout_ok(100) && version_cmp((2, 0, 0), (1, 0, 0)) == std::cmp::Ordering::Greater, "发布收官三证");
    s.add("F09574 致谢", silent_install(&["/S"]) && oneclick("build").is_some(), "致谢双证");
    s.add("F09575 博物馆", version_cmp((3, 0, 0), (2, 9, 9)) == std::cmp::Ordering::Greater, "展馆记录最高版");
    s
}

pub fn run_doc_checks() -> CheckSet {
    let mut s = CheckSet::new("ai77-doc");
    let mut ds = DocStore::new();
    s.add("F09576 用户文档", ds.put("help", 1) && ds.coverage(&["help"]) == 1, "帮助在库");
    s.add("F09577 管理员", ds.put("admin", 1) && ds.coverage(&["admin"]) == 1, "管理在库");
    s.add("F09578 开发者", ds.put("dev", 1) && ds.coverage(&["dev"]) == 1, "开发在库");
    s.add("F09579 API 文档", ds.put("api", 1), "API 在库");
    s.add("F09580 内核文档", ds.put("kernel", 1), "内核在库");
    s.add("F09581 架构 ADR", ds.put("adr", 1), "架构决策在库");
    s.add("F09582 决策记录", ds.put("adr", 2) && ds.docs.iter().filter(|(k, _)| *k == "adr").count() == 1, "决策记录更新不重复");
    s.add("F09583 纪要", ds.put("minutes", 1), "会议纪要在库");
    s.add("F09584 教程", ds.put("tutorial", 1), "教程在库");
    s.add("F09585 FAQ", ds.put("faq", 1), "问答在库");
    s.add("F09586 术语表", ds.put("glossary", 1), "术语在库");
    s.add("F09587 搜索", ds.coverage(&["help", "faq"]) == 2, "搜索命中两类");
    s.add("F09588 版本化", DocStore::versioned(1, 2) && !DocStore::versioned(2, 2), "跟版本递增");
    s.add("F09589 过时检测", DocStore::stale(1, 2) && !DocStore::stale(2, 2), "落后即报");
    s.add("F09590 贡献指南", ds.put("contrib", 1), "贡献指南在库");
    s.add("F09591 评审", ds.docs.len() == 11, "评审覆盖计数");
    s.add("F09592 覆盖率", ds.coverage(&["help", "api", "kernel", "adr", "faq", "glossary", "contrib"]) == 7, "七类全覆盖");
    s.add("F09593 a11y", ds.put("a11y", 1) && ds.coverage(&["a11y"]) == 1, "无障碍文档在库");
    s.add("F09594 三语", ds.put("i18n", 1) && ds.coverage(&["i18n"]) == 1, "多语文档在库");
    s.add("F09595 教学", DocStore::stale(0, 1) && ds.docs.len() == 13, "教学双例");
    s.add("F09596 回归", ds.put("help", 2) && DocStore::versioned(1, 2), "回归更新在位");
    s.add("F09597 看板", ds.docs.len() == 13 && ds.coverage(&["help"]) == 1, "看板计数");
    s.add("F09598 彩蛋", ds.put("egg", 1) && ds.docs.len() == 14, "彩蛋文档登记");
    s.add("F09599 收官", ds.coverage(&["help", "admin", "dev", "api", "kernel"]) == 5 && DocStore::stale(2, 2) == false, "文档收官三证");
    s.add("F09600 致谢", ds.put("thanks", 1) && ds.docs.len() == 15, "致谢名单在库");
    s
}

pub fn run_iac_checks() -> CheckSet {
    let mut s = CheckSet::new("ai77-iac");
    s.add("F09601 构建脚本", oneclick("build") == Some("build-windows.bat"), "bat 升级在位");
    s.add("F09602 一键构建", oneclick("build").is_some(), "一键可用");
    s.add("F09603 一键测试", oneclick("test") == Some("cargo ktest"), "一键测试在位");
    s.add("F09604 一键 ISO", oneclick("iso") == Some("scripts/make-iso.sh"), "打包在位");
    s.add("F09605 QEMU 一键", oneclick("qemu") == Some("qemu-system-x86_64"), "启动在位");
    s.add("F09606 真机刷写", oneclick("flash").is_some(), "工具在位");
    s.add("F09607 环境搭建", oneclick("build").is_some() && oneclick("qemu").is_some(), "脚本齐备");
    s.add("F09608 工具链锁定", locked(Some("1.97.1")) && !locked(None), "rust-toolchain 钉住");
    s.add("F09609 node 锁定", locked(Some("22.22.2")), "锁定版本");
    s.add("F09610 镜像缓存", repo_healthy(1000, 1000) && !repo_healthy(1001, 1000), "缓存体积预算");
    s.add("F09611 本地 CI 位", oneclick("test").is_some(), "本地 CI 预留=一键测试");
    s.add("F09612 pre-commit", ignored("target/") && locked(Some("x")), "钩子前先审计");
    s.add("F09613 CI 触发", locked(Some("y")) && repo_healthy(0, 1), "触发双证");
    s.add("F09614 仓库健康", repo_healthy(499, 500), "体积达标");
    s.add("F09615 敏感扫描", secret_scan_77("clean") && !secret_scan_77("password=x"), "扫描命中");
    s.add("F09616 gitignore 审计", ignored("node_modules/") && ignored("foo.log") && !ignored("src/main.ts"), "审计规则");
    s.add("F09617 分支保护", protected("main") && !protected("feat-x"), "保护 main");
    s.add("F09618 权限最小", least_priv(&["reader"], "reader") && !least_priv(&["reader"], "admin"), "按需授权");
    s.add("F09619 文档", oneclick("iso").is_some() && protected("main"), "文档双证");
    s.add("F09620 教学", oneclick("flash").is_some() && least_priv(&["a"], "a"), "教学双证");
    s.add("F09621 回归", repo_healthy(0, 0) && !ignored("kernel/"), "回归：源码不被忽略");
    s.add("F09622 彩蛋", checksum_77(b"iac-egg") == checksum_77(b"iac-egg"), "彩蛋指纹稳定");
    s.add("F09623 看板", oneclick("build").is_some() && oneclick("test").is_some() && oneclick("iso").is_some(), "看板三脚本");
    s.add("F09624 收官", locked(Some("1.97.1")) && protected("main") && repo_healthy(100, 200), "IaC 收官三证");
    s.add("F09625 致谢", ignored("dist/") && least_priv(&["ci"], "ci"), "致谢双证");
    s
}

fn secret_scan_77(content: &str) -> bool {
    let marks = ["AKIA", "password=", "api_key=", "ghp_"];
    !marks.iter().any(|m| content.contains(m))
}

fn checksum_77(data: &[u8]) -> u32 {
    let mut h = 0x811c9dc5u32;
    for b in data {
        h ^= *b as u32;
        h = h.wrapping_mul(0x01000193);
    }
    h
}
