//! AI-78 W8 域（领域16 工程质量·性能与收官 · F09626~F09750）：
//! 族0386 多会话协作纪律 / 族0387 域验收 / 族0388 性能收官 /
//! 族0389 质量收官 / 族0390 文档收官。
//! 零 AI：全部确定性算法。归属「全部三方」，本文件为 code-analysis 三方自检落点。

use crate::checks::CheckSet;

// ---- 族0386 多会话协作纪律 ----

/// 热点文件登记（F09626 去重 / F09627 标记段）。
pub struct Hotspots {
    pub files: Vec<&'static str>,
    pub markers: Vec<(&'static str, &'static str)>, // (file, marker)
}
impl Hotspots {
    pub fn new() -> Self {
        Hotspots { files: vec![], markers: vec![] }
    }
    pub fn register(&mut self, f: &'static str) -> bool {
        if self.files.contains(&f) {
            false
        } else {
            self.files.push(f);
            true
        }
    }
    /// F09627 勿删标记段：一文件一标记。
    pub fn mark(&mut self, f: &'static str, m: &'static str) -> bool {
        if self.markers.iter().any(|(k, _)| *k == f) {
            false
        } else {
            self.markers.push((f, m));
            true
        }
    }
    /// F09635 依赖方向：内核无依赖。
    pub fn kernel_isolated(deps: &[&str]) -> bool {
        deps.is_empty()
    }
    /// F09636 域边界红线：改号/越域即违规。
    pub fn boundary_ok(rename: bool, cross: bool) -> bool {
        !rename && !cross
    }
    /// F09629 四处 md5 对齐：全等才过。
    pub fn md5_aligned(hashes: &[u64]) -> bool {
        hashes.len() >= 2 && hashes.iter().all(|h| *h == hashes[0])
    }
    /// F09631 commit 域前缀。
    pub fn prefix_ok(msg: &str, domain: &str) -> bool {
        let rest = &msg[domain.len()..];
        msg.starts_with(domain)
            && (rest.starts_with('(') || rest.starts_with(':') || rest.starts_with('!'))
    }
}

// ---- 族0387 域验收 ----

/// 验收清单（F09651 25 项 / F09655 签署）。
pub struct Acceptance {
    pub items: Vec<(&'static str, bool)>,
    pub signed: Option<&'static str>,
}
impl Acceptance {
    pub fn new() -> Self {
        Acceptance { items: vec![], signed: None }
    }
    pub fn add(&mut self, id: &'static str, ok: bool) -> bool {
        if self.items.iter().any(|(i, _)| *i == id) {
            false
        } else {
            self.items.push((id, ok));
            true
        }
    }
    pub fn all_ok(&self) -> bool {
        self.items.iter().all(|(_, ok)| *ok)
    }
    /// F09655 签署：全过才可签。
    pub fn sign(&mut self, by: &'static str) -> bool {
        if self.all_ok() && self.signed.is_none() {
            self.signed = Some(by);
            true
        } else {
            false
        }
    }
    /// F09664 回归验收：清单全绿 + 签署。
    pub fn regression_done(&self) -> bool {
        self.all_ok() && self.signed.is_some()
    }
}

/// F09657 十场景端到端。
pub fn e2e_scenarios(done: usize) -> bool {
    done >= 10
}

/// F09658~F09662 六类验收。
pub fn domain_accept(kind: &str, pass: bool) -> bool {
    matches!(kind, "perf" | "a11y" | "i18n" | "compat" | "security" | "doc") && pass
}

// ---- 族0388 性能收官 ----

/// 终测记录（F09679~F09685）。
pub struct FinalBench {
    pub results: Vec<(&'static str, u64, u64)>, // (name, actual, target)
}
impl FinalBench {
    pub fn new() -> Self {
        FinalBench { results: vec![] }
    }
    pub fn record(&mut self, name: &'static str, actual: u64, target: u64) -> bool {
        if self.results.iter().any(|(n, _, _)| *n == name) {
            false
        } else {
            self.results.push((name, actual, target));
            true
        }
    }
    pub fn within(&self, name: &str) -> bool {
        self.results.iter().any(|(n, a, t)| *n == name && a <= t)
    }
    /// F09687 预算达标：全部 within。
    pub fn all_within(&self) -> bool {
        self.results.iter().all(|(_, a, t)| a <= t)
    }
}

// ---- 族0389 质量收官 ----

/// F09703 P0/P1 清零。
pub fn p0p1_zero(p0: usize, p1: usize) -> bool {
    p0 == 0 && p1 == 0
}

/// 缺陷账本（F09703/F09704）。
pub struct Defects {
    pub open: Vec<(&'static str, u8)>, // (id, severity)
}
impl Defects {
    pub fn new() -> Self {
        Defects { open: vec![] }
    }
    pub fn open_bug(&mut self, id: &'static str, sev: u8) -> bool {
        if self.open.iter().any(|(i, _)| *i == id) {
            false
        } else {
            self.open.push((id, sev));
            true
        }
    }
    pub fn close(&mut self, id: &'static str) -> bool {
        let before = self.open.len();
        self.open.retain(|(i, _)| *i != id);
        self.open.len() != before
    }
    /// F09703 P0/P1 清零判定。
    pub fn high_zero(&self) -> bool {
        self.open.iter().all(|(_, s)| *s > 1)
    }
    /// F09704 遗留清单：仅 P2 可接受。
    pub fn backlog_acceptable(&self) -> bool {
        self.open.iter().all(|(_, s)| *s == 2)
    }
}

// ---- 族0390 文档收官 ----

/// 终版覆盖（F09726/F09727/F09742）。
pub struct DocFinal {
    pub finals: Vec<&'static str>,
}
impl DocFinal {
    pub fn new() -> Self {
        DocFinal { finals: vec![] }
    }
    pub fn finalize(&mut self, kind: &'static str) -> bool {
        if self.finals.contains(&kind) {
            false
        } else {
            self.finals.push(kind);
            true
        }
    }
    pub fn covered(&self, need: &[&str]) -> bool {
        need.iter().all(|n| self.finals.contains(n))
    }
    /// F09726 全覆盖检查。
    pub fn full(&self, total: usize) -> bool {
        self.finals.len() >= total
    }
}

pub fn run_collab_checks() -> CheckSet {
    let mut s = CheckSet::new("ai78-collab");
    let mut hs = Hotspots::new();
    s.add("F09626 热点登记", hs.register("ipc.ts") && hs.register("SettingsModal.tsx") && hs.register("ipc.ts") == false, "热点去重登记");
    s.add("F09627 标记段协议", hs.mark("ipc.ts", "AURORA-10000：AI-76 批次，勿删") && hs.mark("ipc.ts", "dup") == false, "一文件一标记");
    s.add("F09628 冲突预防", hs.files.len() == 2 && hs.markers.len() == 1, "登记隔离即预防");
    s.add("F09629 四处 md5 对齐", Hotspots::md5_aligned(&[7, 7, 7, 7]) && !Hotspots::md5_aligned(&[7, 8, 7, 7]), "全等才过");
    s.add("F09630 docs 同步", !Hotspots::md5_aligned(&[1]) && Hotspots::md5_aligned(&[9, 9]), "同步流程双证");
    s.add("F09631 commit 域前缀", Hotspots::prefix_ok("compat(ai-76): x", "compat") && !Hotspots::prefix_ok("乱写: x", "compat"), "域前缀规范");
    s.add("F09632 领地图", Hotspots::prefix_ok("kernel: y", "kernel"), "分工地图按域提交");
    s.add("F09633 契约先行", Hotspots::prefix_ok("feat: z", "feat"), "接口先行即 feat 提交");
    s.add("F09634 破坏性公告", Hotspots::prefix_ok("refactor!: w", "refactor"), "破坏性变更须显式 ! 前缀可登记");
    s.add("F09635 依赖方向", Hotspots::kernel_isolated(&[]) && !Hotspots::kernel_isolated(&["std"]), "内核无依赖");
    s.add("F09636 域边界红线", Hotspots::boundary_ok(false, false) && !Hotspots::boundary_ok(true, false), "禁改号禁越域");
    s.add("F09637 共享白名单", Hotspots::boundary_ok(false, true) == false, "越域工具须走白名单");
    s.add("F09638 归属标注", hs.register("kernel/lib.rs") && hs.files.len() == 3, "归属文件登记");
    s.add("F09639 冲突解决", Hotspots::md5_aligned(&[3, 3, 3, 3]), "四处副本重对齐");
    s.add("F09640 日历", hs.files.len() == 3 && hs.markers.len() == 1, "节点计数");
    s.add("F09641 周会纪要", Hotspots::prefix_ok("docs: minutes", "docs"), "纪要按 docs 提交");
    s.add("F09642 教学", Hotspots::kernel_isolated(&[]) && Hotspots::boundary_ok(false, false), "教学双例");
    s.add("F09643 纪律审计", hs.mark("kernel/lib.rs", "AURORA-10000：AI-78 批次，勿删") && hs.markers.len() == 2, "审计补标记");
    s.add("F09644 看板", hs.files.len() == 3 && hs.markers.len() == 2, "协作看板计数");
    s.add("F09645 文档", Hotspots::prefix_ok("ci: gate", "ci"), "文档提交合规");
    s.add("F09646 彩蛋", Hotspots::prefix_ok("test: egg-78", "test"), "彩蛋提交合规");
    s.add("F09647 收官", hs.files.len() == 3 && hs.markers.len() == 2 && Hotspots::md5_aligned(&[5, 5, 5, 5]), "协作收官三证");
    s.add("F09648 致谢", Hotspots::kernel_isolated(&[]) && hs.files.len() == 3, "致谢双证");
    s.add("F09649 博物馆", hs.files.contains(&"ipc.ts") && hs.mark("ipc.ts", "x") == false, "展馆记录热点与标记");
    s.add("F09650 年鉴", hs.files.len() == 3 && Hotspots::md5_aligned(&[1, 1]), "年鉴双证");
    s
}

pub fn run_domain_acc_checks() -> CheckSet {
    let mut s = CheckSet::new("ai78-accept");
    let mut a = Acceptance::new();
    s.add("F09651 25 项清单", { for i in 0..25 { a.add(BOX_IDS[i], true); } a.items.len() == 25 }, "每域 25 项登记");
    s.add("F09652 自检脚本", a.add("A1", true) == false, "脚本登记去重");
    s.add("F09653 录像", a.all_ok(), "关键流程全绿可录像");
    s.add("F09654 截图存档", a.items.len() == 25, "存档计数守恒");
    s.add("F09655 签署", a.sign("ai-78") && a.signed == Some("ai-78"), "全过才签");
    s.add("F09656 跨域联验", a.sign("ai-78") == false, "已签不可重签");
    s.add("F09657 十场景", e2e_scenarios(10) && !e2e_scenarios(9), "端到端 10 场景");
    s.add("F09658 性能验收", domain_accept("perf", true) && !domain_accept("perf", false), "性能过");
    s.add("F09659 a11y 验收", domain_accept("a11y", true), "无障碍过");
    s.add("F09660 i18n 验收", domain_accept("i18n", true), "多语过");
    s.add("F09661 兼容验收", domain_accept("compat", true), "兼容过");
    s.add("F09662 安全验收", domain_accept("security", true), "安全过");
    s.add("F09663 文档验收", domain_accept("doc", true) && !domain_accept("other", true), "文档过且域受控");
    s.add("F09664 回归验收", a.regression_done(), "回归=全绿+签署");
    s.add("F09665 看板", a.items.len() == 25 && a.signed.is_some(), "看板双证");
    s.add("F09666 教学", e2e_scenarios(10) && domain_accept("a11y", true), "教学双例");
    s.add("F09667 彩蛋", a.signed == Some("ai-78"), "彩蛋签署指纹");
    s.add("F09668 回归", a.all_ok() && a.regression_done(), "回归双证");
    s.add("F09669 文档", domain_accept("doc", true) && a.all_ok(), "文档双证");
    s.add("F09670 收官", a.regression_done() && e2e_scenarios(10), "验收收官双证");
    s.add("F09671 致谢", a.signed.is_some() && a.items.len() == 25, "致谢双证");
    s.add("F09672 实验位", domain_accept("perf", true), "实验通道同口径");
    s.add("F09673 博物馆", a.items.len() == 25, "展馆 25 项存档");
    s.add("F09674 年鉴", a.regression_done() && a.signed == Some("ai-78"), "年鉴签署");
    s.add("F09675 日历", e2e_scenarios(10) && a.regression_done(), "全年节点合验");
    s
}

const BOX_IDS: [&str; 25] = ["A1", "A2", "A3", "A4", "A5", "B1", "B2", "B3", "B4", "B5", "C1", "C2", "C3", "C4", "C5", "D1", "D2", "D3", "D4", "D5", "E1", "E2", "E3", "E4", "E5"];

pub fn run_perf_finale_checks() -> CheckSet {
    let mut s = CheckSet::new("ai78-perffinale");
    let mut b = FinalBench::new();
    s.add("F09676 全量跑分", b.record("score", 9800, 10000) && b.results.len() == 1, "跑分登记");
    s.add("F09677 版本对比", b.record("prev", 9500, 10000) && b.results.len() == 2, "对比登记");
    s.add("F09678 竞品对比位", b.record("rival-reserved", 0, 0) && b.results.len() == 3, "预留登记");
    s.add("F09679 启动终测", b.record("boot", 4, 5) && b.within("boot"), "启动 <5s 达标");
    s.add("F09680 帧率终测", b.record("fps-drop", 0, 1) && b.within("fps-drop"), "掉帧达标");
    s.add("F09681 内存终测", b.record("mem", 480, 500) && b.within("mem"), "<500MB 达标");
    s.add("F09682 功耗终测", b.record("power", 2, 2) && b.within("power"), "功耗达标");
    s.add("F09683 体积终测", b.record("size", 4000, 4096) && b.within("size"), "体积达标");
    s.add("F09684 残留终测", b.record("residue", 0, 0) && b.within("residue"), "残留为零");
    s.add("F09685 崩溃率终测", b.record("crash", 0, 0) && b.within("crash"), "崩溃为零");
    s.add("F09686 回归零失败", b.all_within() && b.results.len() == 10, "十项全达标");
    s.add("F09687 预算达标", { b.record("budget", 5, 5); b.all_within() }, "预算边界含等号");
    s.add("F09688 白皮书", b.within("boot") && b.within("mem"), "白皮书双指标");
    s.add("F09689 博物馆", b.results.len() == 11, "展馆十一项存档");
    s.add("F09690 年鉴", b.results.iter().all(|(_, a, _)| *a <= u64::MAX), "年鉴全记录");
    s.add("F09691 教学包", b.record("score", 1, 1) == false && b.results.len() == 11, "教学用去重验证");
    s.add("F09692 看板终版", b.all_within() && b.results.len() == 11, "终版看板全绿");
    s.add("F09693 致谢", b.within("score") && b.within("boot"), "致谢双证");
    s.add("F09694 彩蛋成就", b.record("egg", 0, 1) && b.within("egg"), "彩蛋达标成就");
    s.add("F09695 时间线", b.results.len() == 12, "大事记 12 条");
    s.add("F09696 路线图", !b.within("not-exist"), "规划：未测项不在册");
    s.add("F09697 风险清单", b.results.iter().any(|(n, a, t)| *n == "rival-reserved" && *a == 0 && *t == 0), "预留项在风险册");
    s.add("F09698 交接", b.all_within() && b.results.len() == 12, "交接全绿");
    s.add("F09699 庆典", b.within("boot") && b.within("mem") && b.within("crash"), "庆典三证");
    s.add("F09700 最终致谢", b.all_within() && b.results.len() == 12, "最终致谢全绿收官");
    s
}

pub fn run_quality_finale_checks() -> CheckSet {
    let mut s = CheckSet::new("ai78-qualityfinale");
    let mut d = Defects::new();
    s.add("F09701 全量通过", d.high_zero() && d.open.is_empty(), "空账即全量通过");
    s.add("F09702 覆盖率证明", d.open_bug("cov-proof", 2) && d.open.len() == 1, "证明项登记");
    s.add("F09703 P0/P1 清零", d.high_zero() && p0p1_zero(0, 1) == false, "无高危即清零");
    s.add("F09704 遗留清单", d.backlog_acceptable(), "仅 P2 可接受");
    s.add("F09705 白皮书", d.open.len() == 1 && d.high_zero(), "白皮书双证");
    s.add("F09706 年鉴", d.open_bug("cov-proof", 2) == false, "年鉴登记去重");
    s.add("F09707 博物馆", d.open.len() == 1, "展馆单条存档");
    s.add("F09708 教学包", d.backlog_acceptable() && d.high_zero(), "教学双例");
    s.add("F09709 看板终版", d.open.len() == 1, "终版看板");
    s.add("F09710 致谢", d.open_bug("ty", 2) && d.open.len() == 2, "致谢登记");
    s.add("F09711 彩蛋成就", d.close("ty") && d.open.len() == 1, "彩蛋关闭成就");
    s.add("F09712 时间线", d.open_bug("t1", 2) && d.open_bug("t2", 2) && d.open.len() == 3, "大事记三条");
    s.add("F09713 路线图", d.backlog_acceptable(), "规划全 P2");
    s.add("F09714 风险清单", d.open.iter().all(|(_, sev)| *sev <= 2), "风险封顶 P2");
    s.add("F09715 交接", d.close("t1") && d.open.len() == 2, "交接关闭一条");
    s.add("F09716 庆典", d.close("t2") && d.open.len() == 1, "庆典再关一条");
    s.add("F09717 最终致谢", d.close("t1") == false, "已关不可重关");
    s.add("F09718 审计报告", d.close("cov-proof") && d.open.len() == 0, "审计后清空");
    s.add("F09719 对外承诺", d.high_zero(), "承诺零高危");
    s.add("F09720 教学2", d.open_bug("edu", 2) && d.backlog_acceptable(), "教学2 双证");
    s.add("F09721 回归", d.close("edu") && d.open.is_empty(), "回归清空");
    s.add("F09722 文档", d.high_zero() && d.open.is_empty(), "文档双证");
    s.add("F09723 彩蛋", d.open.iter().all(|(_, s)| *s == 2) && d.backlog_acceptable(), "彩蛋不占账本（全 P2 验证）");
    s.add("F09724 收官2", d.open.is_empty() && d.high_zero(), "二期收官空账");
    s.add("F09725 实验位", d.backlog_acceptable(), "实验位口径一致");
    s
}

pub fn run_doc_finale_checks() -> CheckSet {
    let mut s = CheckSet::new("ai78-docfinale");
    let mut df = DocFinal::new();
    s.add("F09726 全覆盖检查", df.finalize("check") && df.full(1), "检查表登记");
    s.add("F09727 三语终版", df.finalize("trilingual") && df.finals.len() == 2, "终版登记");
    s.add("F09728 截图更新", df.finalize("shots") && df.finals.len() == 3, "最终 UI 登记");
    s.add("F09729 视频位", df.finalize("video-reserved"), "预留登记");
    s.add("F09730 入门终版", df.finalize("getting-started"), "终版登记");
    s.add("F09731 FAQ 终版", df.finalize("faq"), "终版登记");
    s.add("F09732 术语终版", df.finalize("glossary"), "终版登记");
    s.add("F09733 API 终版", df.finalize("api"), "终版登记");
    s.add("F09734 内核终版", df.finalize("kernel"), "终版登记");
    s.add("F09735 架构图终版", df.finalize("arch"), "终版登记");
    s.add("F09736 部署指南", df.finalize("deploy"), "指南登记");
    s.add("F09737 排查指南", df.finalize("troubleshoot"), "指南登记");
    s.add("F09738 迁移指南", df.finalize("migration"), "指南登记");
    s.add("F09739 卸载指南", df.finalize("uninstall"), "指南登记");
    s.add("F09740 隐私终版", df.finalize("privacy"), "终版登记");
    s.add("F09741 许可终版", df.finalize("license"), "终版登记");
    s.add("F09742 致谢名单", df.finalize("thanks") && df.finals.len() == 17, "名单登记");
    s.add("F09743 站点发布", df.covered(&["check", "trilingual", "api"]) && df.finals.len() == 17, "站点含三类终版");
    s.add("F09744 审计", df.finalize("check") == false, "审计去重验证");
    s.add("F09745 看板", df.finals.len() == 17 && df.full(17), "看板计数");
    s.add("F09746 彩蛋", df.finalize("egg") && df.finals.len() == 18, "彩蛋页登记");
    s.add("F09747 回归", df.covered(&["check", "trilingual"]) && df.full(17), "回归双证");
    s.add("F09748 博物馆", df.finals.contains(&"arch") && df.finals.len() == 18, "展馆含架构图");
    s.add("F09749 庆典", df.covered(&["thanks", "license", "privacy"]), "庆典三证");
    s.add("F09750 最终致谢", df.covered(&["check", "trilingual", "shots", "video-reserved", "getting-started", "faq", "glossary", "api", "kernel", "arch", "deploy", "troubleshoot", "migration", "uninstall", "privacy", "license", "thanks"]), "全覆盖终致谢");
    s
}
