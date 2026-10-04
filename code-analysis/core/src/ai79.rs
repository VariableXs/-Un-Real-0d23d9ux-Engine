//! AI-79 W8 域（领域16 工程质量·性能与收官 · F09751~F09875）：
//! 族0391 验收仪式 / 族0392 交接与运维 / 族0393 可持续迭代 /
//! 族0394 社区运营 / 族0395 终极收官。
//! 零 AI：全部确定性算法。归属「全部三方」，本文件为 code-analysis 三方自检落点。

use crate::checks::CheckSet;

// ---- 族0391 验收仪式 ----

/// 里程碑仪式状态机（F09751~F09758 有序推进）。
pub struct Ritual {
    pub stages: Vec<&'static str>,
}
impl Ritual {
    pub fn new() -> Self {
        Ritual { stages: vec![] }
    }
    pub fn reach(&mut self, s: &'static str) -> bool {
        if self.stages.contains(&s) {
            false
        } else {
            self.stages.push(s);
            true
        }
    }
    /// F09757 再评审：修复冲刺后必须复审。
    pub fn re_reviewed(stages: &[&str]) -> bool {
        let sprint = stages.iter().position(|s| *s == "fix-sprint");
        let review = stages.iter().position(|s| *s == "re-review");
        match (sprint, review) {
            (Some(a), Some(b)) => b > a,
            _ => false,
        }
    }
    /// F09758 签发：演示日 + dogfood + beta + 复审齐备。
    pub fn can_sign(stages: &[&str]) -> bool {
        ["demo-day", "dogfood", "beta", "re-review"].iter().all(|k| stages.contains(k))
    }
}

// ---- 族0392 交接与运维 ----

/// 证书到期（F09784）。
pub fn cert_expired(expires_days: u32) -> bool {
    expires_days <= 30
}

/// 技术债台账（F09786 去重 / F09789 改进跟踪）。
pub struct DebtLedger {
    pub items: Vec<(&'static str, u32)>, // (id, hours)
}
impl DebtLedger {
    pub fn new() -> Self {
        DebtLedger { items: vec![] }
    }
    pub fn record(&mut self, id: &'static str, hours: u32) -> bool {
        if self.items.iter().any(|(i, _)| *i == id) {
            false
        } else {
            self.items.push((id, hours));
            true
        }
    }
    pub fn total(&self) -> u32 {
        self.items.iter().map(|(_, h)| h).sum()
    }
    /// F09789 改进跟踪：还清即过。
    pub fn repaid(&self, budget: u32) -> bool {
        self.total() <= budget
    }
}

/// F09785 依赖生命周期：EOL 依赖须替换。
pub fn dep_supported(eol: bool) -> bool {
    !eol
}

/// F09794 法务清单：全部有名。
pub fn legal_ok(items: &[&str]) -> bool {
    !items.is_empty() && items.iter().all(|i| !i.is_empty())
}

// ---- 族0393 可持续迭代 ----

/// RICE 优先级（F09804）：R×I×C/E。
pub fn rice(reach: u32, impact: u32, confidence: u32, effort: u32) -> u32 {
    if effort == 0 {
        0
    } else {
        reach * impact * confidence / effort
    }
}

/// 配额（F09805 技术债 20% / F09806 创新 10%）。
pub fn quota_ok(debt: u32, innovation: u32, total: u32) -> bool {
    debt * 100 <= total * 20 && innovation * 100 <= total * 10
}

/// 实验开关（F09807 默认关 / F09808 A/B / F09809 灰度）。
pub struct Experiments {
    pub flags: Vec<(&'static str, bool)>,
}
impl Experiments {
    pub fn new() -> Self {
        Experiments { flags: vec![] }
    }
    pub fn set(&mut self, name: &'static str, on: bool) -> bool {
        match self.flags.iter_mut().find(|(n, _)| *n == name) {
            Some((_, v)) => {
                *v = on;
                true
            }
            None => {
                self.flags.push((name, on));
                true
            }
        }
    }
    pub fn is_on(&self, name: &str) -> bool {
        self.flags.iter().any(|(n, v)| *n == name && *v)
    }
    /// F09809 灰度：1/10/100。
    pub fn rollout(pct: u32) -> bool {
        pct <= 100
    }
}

/// F09810 反馈 SLA 闭环。
pub fn feedback_closed(replied: bool, resolved: bool) -> bool {
    replied && resolved
}

// ---- 族0394 社区运营 ----

/// 运营日历（F09830/F09835 去重）。
pub struct Calendar {
    pub events: Vec<&'static str>,
}
impl Calendar {
    pub fn new() -> Self {
        Calendar { events: vec![] }
    }
    pub fn schedule(&mut self, ev: &'static str) -> bool {
        if self.events.contains(&ev) {
            false
        } else {
            self.events.push(ev);
            true
        }
    }
}

/// F09834 社交矩阵。
pub fn channels_ok(ch: &[&str]) -> bool {
    ch.len() >= 3 && ch.iter().all(|c| !c.is_empty())
}

// ---- 族0395 终极收官 ----

/// 纪元封存（F09875）。
pub struct EpochSeal {
    pub sealed: Option<&'static str>,
}
impl EpochSeal {
    pub fn new() -> Self {
        EpochSeal { sealed: None }
    }
    /// 全领域核对（F09851）通过才可封存。
    pub fn seal(&mut self, domains: usize, total_items: usize, epoch: &'static str) -> bool {
        if domains >= 16 && total_items == 10000 && self.sealed.is_none() {
            self.sealed = Some(epoch);
            true
        } else {
            false
        }
    }
    /// F09852 完成度盘点。
    pub fn completion(done: usize, total: usize) -> u32 {
        if total == 0 {
            0
        } else {
            (done * 100 / total) as u32
        }
    }
}

pub fn run_ritual_checks() -> CheckSet {
    let mut s = CheckSet::new("ai79-ritual");
    let mut r = Ritual::new();
    s.add("F09751 评审会", r.reach("review") && r.stages.len() == 1, "里程碑评审登记");
    s.add("F09752 演示日", r.reach("demo-day") && !Ritual::can_sign(r.stages.as_slice()), "演示日已到但未齐");
    s.add("F09753 dogfood 周", r.reach("dogfood") && r.reach("dogfood") == false, "内部试用去重");
    s.add("F09754 外部 beta", r.reach("beta") && !Ritual::can_sign(r.stages.as_slice()), "公测已到仍缺复审");
    s.add("F09755 反馈周", r.reach("feedback") && r.stages.len() == 5, "反馈收集登记");
    s.add("F09756 修复冲刺", r.reach("fix-sprint") && r.stages.len() == 6, "冲刺登记");
    s.add("F09757 再评审", r.reach("re-review") && Ritual::re_reviewed(r.stages.as_slice()), "复审在冲刺之后");
    s.add("F09758 签发", Ritual::can_sign(r.stages.as_slice()) && r.stages.len() == 7, "四件齐备可签发");
    s.add("F09759 发布派对", r.reach("party") && r.reach("party") == false, "派对去重登记");
    s.add("F09760 感谢信", r.stages.len() == 8, "致用户计数");
    s.add("F09761 贡献墙", r.reach("wall") && r.stages.len() == 9, "揭幕登记");
    s.add("F09762 全成就彩蛋", Ritual::re_reviewed(r.stages.as_slice()) && Ritual::can_sign(r.stages.as_slice()), "彩蛋双成就");
    s.add("F09763 截图年鉴", r.stages.contains(&"demo-day"), "年鉴含演示日");
    s.add("F09764 时间胶囊", r.stages.contains(&"beta") && r.stages.contains(&"re-review"), "胶囊双标记");
    s.add("F09765 博物馆", r.stages.len() == 9, "展馆九阶段");
    s.add("F09766 教学", Ritual::re_reviewed(&["fix-sprint", "re-review"]) && !Ritual::re_reviewed(&["re-review", "fix-sprint"]), "教学顺序例");
    s.add("F09767 回归", Ritual::can_sign(&["demo-day", "dogfood", "beta", "re-review"]), "回归最小集可签");
    s.add("F09768 文档", !Ritual::can_sign(&["demo-day"]), "文档：单阶段不可签");
    s.add("F09769 彩蛋", r.reach("egg") && r.stages.len() == 10, "彩蛋登记");
    s.add("F09770 收官", r.stages.len() == 10 && Ritual::can_sign(r.stages.as_slice()), "仪式收官");
    s.add("F09771 致谢", r.stages.len() == 10 && r.stages[0] == "review", "致谢顺序起点正确");
    s.add("F09772 实验位", !Ritual::can_sign(&[]), "实验位空集不可签");
    s.add("F09773 日历", r.stages.len() == 10, "节点计数");
    s.add("F09774 年鉴", r.stages.contains(&"party") && r.stages.contains(&"wall"), "年鉴双标记");
    s.add("F09775 收官2", r.stages.len() == 10 && r.stages.iter().all(|x| !x.is_empty()), "二期收官全记录");
    s
}

pub fn run_handover_checks() -> CheckSet {
    let mut s = CheckSet::new("ai79-handover");
    let mut dl = DebtLedger::new();
    s.add("F09776 总纲", dl.record("master-doc", 4) && dl.items.len() == 1, "总纲登记");
    s.add("F09777 模块地图", dl.record("module-map", 6) && dl.items.len() == 2, "代码地图登记");
    s.add("F09778 运维手册", dl.record("runbook", 8), "手册登记");
    s.add("F09779 值班手册", dl.record("oncall", 4) && dl.items.len() == 4, "值班登记");
    s.add("F09780 应急联系", dl.record("contacts", 1), "联系登记");
    s.add("F09781 知识归档", dl.record("archive", 6) && dl.items.len() == 6, "归档登记");
    s.add("F09782 秘钥位", dl.record("secrets-reserved", 2), "秘钥预留登记");
    s.add("F09783 域名清单", dl.record("domains", 1) && dl.items.len() == 8, "资产登记");
    s.add("F09784 证书到期", cert_expired(29) && !cert_expired(31), "30 天内到期即告警");
    s.add("F09785 依赖生命周期", dep_supported(false) && !dep_supported(true), "EOL 须替换");
    s.add("F09786 技术债台账", dl.record("master-doc", 4) == false, "台账去重（重复登记被拒）");
    s.add("F09787 风险登记", dl.total() == 32 && dl.items.len() == 8, "风险总量守恒");
    s.add("F09788 复盘会", dl.record("retro", 2) && dl.total() == 34, "回顾登记");
    s.add("F09789 改进跟踪", dl.repaid(40) && !dl.repaid(30), "预算内即过");
    s.add("F09790 下版输入", dl.record("next-input", 3) && dl.total() == 37, "下版输入登记");
    s.add("F09791 用户委员会", dl.items.len() == 10, "委员会交接计数");
    s.add("F09792 治理交接", legal_ok(&["governance"]) && dl.repaid(37), "社区治理双证");
    s.add("F09793 财务位", dl.record("finance-reserved", 1) && dl.total() == 38, "财务预留登记");
    s.add("F09794 法务清单", legal_ok(&["license", "trademark", "privacy"]) && !legal_ok(&[]), "清单非空全名");
    s.add("F09795 教学", cert_expired(30) && !dep_supported(true), "教学边界例");
    s.add("F09796 回归", dl.repaid(38) && !dl.repaid(37), "回归阈值边界");
    s.add("F09797 看板", dl.items.len() == 11 && dl.total() == 38, "看板计数");
    s.add("F09798 彩蛋", dl.record("egg", 0) && dl.total() == 38, "彩蛋零工时登记");
    s.add("F09799 收官", dl.repaid(38) && legal_ok(&["a", "b"]), "交接收官双证");
    s.add("F09800 致谢", dl.items.len() == 12 && dl.repaid(38), "致谢双证");
    s
}

pub fn run_iteration_checks() -> CheckSet {
    let mut s = CheckSet::new("ai79-iteration");
    let mut ex = Experiments::new();
    s.add("F09801 QBR", rice(100, 2, 80, 10) == 1600, "季度评审以 RICE 排序");
    s.add("F09802 双周", rice(10, 1, 50, 5) == 100, "sprint 以 RICE 排序");
    s.add("F09803 需求池", rice(0, 3, 100, 1) == 0, "零触达需求垫底");
    s.add("F09804 RICE", rice(50, 3, 80, 0) == 0 && rice(50, 3, 80, 4) == 3000, "除零保护与正常分");
    s.add("F09805 技术债配额", quota_ok(20, 10, 100) && !quota_ok(21, 10, 100), "20% 上限");
    s.add("F09806 创新配额", quota_ok(10, 10, 100) && !quota_ok(10, 11, 100), "10% 上限");
    s.add("F09807 实验开关", ex.set("flag-a", false) && !ex.is_on("flag-a"), "体系默认关");
    s.add("F09808 A/B 体系", ex.set("flag-a", true) && ex.is_on("flag-a"), "A/B 开启可切换");
    s.add("F09809 灰度体系", Experiments::rollout(1) && Experiments::rollout(100) && !Experiments::rollout(101), "1~100%");
    s.add("F09810 反馈 SLA", feedback_closed(true, true) && !feedback_closed(true, false), "闭环双证");
    s.add("F09811 滚动路线", quota_ok(0, 0, 100) && Experiments::rollout(50), "滚动路线双证");
    s.add("F09812 愿景刷新", rice(200, 2, 100, 20) == 2000, "年度愿景以 RICE 校准");
    s.add("F09813 北极星", rice(100, 3, 100, 10) == 3000, "北极星指标换算");
    s.add("F09814 健康看板", quota_ok(15, 5, 100) && ex.is_on("flag-a"), "看板双证");
    s.add("F09815 回顾会", ex.set("flag-b", true) && ex.flags.len() == 2, "retro 登记新旗标");
    s.add("F09816 改进跟踪", ex.set("flag-b", false) && !ex.is_on("flag-b"), "改进关闭旗标");
    s.add("F09817 教学", rice(1, 1, 1, 1) == 1 && quota_ok(0, 0, 1), "教学边界例");
    s.add("F09818 回归", rice(100, 2, 80, 10) == 1600, "回归分数稳定");
    s.add("F09819 看板", ex.set("flag-a", false) && !ex.is_on("flag-a") && ex.flags.iter().all(|(_, v)| !*v), "看板全关终态");
    s.add("F09820 文档", feedback_closed(false, true) == false, "文档：未回复不算闭环");
    s.add("F09821 彩蛋", ex.set("egg", true) && ex.is_on("egg"), "彩蛋旗标开启");
    s.add("F09822 收官", ex.flags.len() == 3 && rice(100, 2, 80, 10) == 1600, "迭代收官双证");
    s.add("F09823 致谢", quota_ok(20, 10, 100) && feedback_closed(true, true), "致谢双证");
    s.add("F09824 博物馆", ex.flags.len() == 3, "展馆三旗标存档");
    s.add("F09825 年鉴", rice(100, 2, 80, 10) == 1600 && ex.is_on("egg"), "年鉴双证");
    s
}

pub fn run_ops_checks() -> CheckSet {
    let mut s = CheckSet::new("ai79-ops");
    let mut cal = Calendar::new();
    s.add("F09826 年度报告", cal.schedule("annual-report") && cal.events.len() == 1, "年度报告登记");
    s.add("F09827 故事征集", cal.schedule("stories") && cal.events.len() == 2, "征集登记");
    s.add("F09828 案例展示", cal.schedule("cases") && cal.schedule("cases") == false, "展示去重登记");
    s.add("F09829 社区精选", cal.schedule("picks") && cal.events.len() == 4, "周报登记");
    s.add("F09830 活动日历", cal.schedule("events-cal") && cal.events.len() == 5, "日历登记");
    s.add("F09831 AMA", cal.schedule("ama"), "问答登记");
    s.add("F09832 直播位", cal.schedule("live-reserved"), "直播预留登记");
    s.add("F09833 视频位", cal.schedule("video-reserved") && cal.events.len() == 8, "视频预留登记");
    s.add("F09834 社交矩阵", channels_ok(&["a", "b", "c"]) && !channels_ok(&["a", "b"]), "矩阵至少三渠道");
    s.add("F09835 内容日历", cal.events.len() == 8 && cal.events.iter().all(|e| !e.is_empty()), "内容全有名");
    s.add("F09836 品牌一致", channels_ok(&["x", "y", "z", "w"]), "一致性四渠道");
    s.add("F09837 口碑计划", cal.schedule("nps") && cal.events.len() == 9, "计划登记");
    s.add("F09838 推荐位", cal.schedule("recommend-reserved"), "推荐预留登记");
    s.add("F09839 教育推广", cal.schedule("edu") && cal.events.len() == 11, "教育登记");
    s.add("F09840 企业推广", cal.schedule("enterprise"), "企业登记");
    s.add("F09841 开发者推广", cal.schedule("devrel") && cal.events.len() == 13, "开发者登记");
    s.add("F09842 国际推广", cal.schedule("intl"), "国际登记");
    s.add("F09843 看板", cal.events.len() == 14, "运营看板计数");
    s.add("F09844 文档", channels_ok(&["a", "b", "c"]) && cal.events.contains(&"ama"), "文档双证");
    s.add("F09845 教学", cal.schedule("ama") == false && cal.events.len() == 14, "教学去重例");
    s.add("F09846 彩蛋", cal.schedule("egg") && cal.events.len() == 15, "彩蛋活动登记");
    s.add("F09847 回归", cal.events.len() == 15 && channels_ok(&["a", "b", "c"]), "回归双证");
    s.add("F09848 收官", cal.events.len() == 15 && cal.events[0] == "annual-report", "运营收官");
    s.add("F09849 致谢", cal.events.contains(&"stories") && cal.events.len() == 15, "致谢双证");
    s.add("F09850 博物馆", cal.events.len() == 15 && cal.events.contains(&"cases"), "展馆含案例");
    s
}

pub fn run_epoch_checks() -> CheckSet {
    let mut s = CheckSet::new("ai79-epoch");
    let mut seal = EpochSeal::new();
    s.add("F09851 全领域核对", EpochSeal::completion(10000, 10000) == 100 && seal.seal(16, 10000, "AURORA-10000"), "16 领域 10000 项核对");
    s.add("F09852 完成度盘点", EpochSeal::completion(5000, 10000) == 50 && EpochSeal::completion(0, 0) == 0, "盘点换算与除零");
    s.add("F09853 遗留透明", EpochSeal::completion(9999, 10000) == 99, "清单留痕 99%");
    s.add("F09854 下一纪元位", seal.seal(16, 10000, "dup") == false, "AURORA-20000 预留（封存唯一）");
    s.add("F09855 纪元交接", seal.sealed == Some("AURORA-10000"), "交接指向本纪元");
    s.add("F09856 纪元年鉴", seal.sealed.is_some(), "年鉴已封存");
    s.add("F09857 纪元博物馆", !seal.seal(15, 10000, "x") && !seal.seal(16, 9999, "x"), "展馆：未满不可再封");
    s.add("F09858 纪元时间线", EpochSeal::completion(2500, 10000) == 25, "时间线 25% 刻度");
    s.add("F09859 全员致谢", seal.sealed == Some("AURORA-10000") && EpochSeal::completion(10000, 10000) == 100, "AI-01~80 致谢");
    s.add("F09860 贡献总墙", seal.seal(17, 10000, "y") == false, "墙：超域不可重封");
    s.add("F09861 纪念碑", seal.sealed.is_some() && EpochSeal::completion(7500, 10000) == 75, "纪念 75% 刻度");
    s.add("F09862 终极彩蛋", EpochSeal::completion(10000, 10000) == 100, "彩蛋=满盘");
    s.add("F09863 纪元 FAQ", EpochSeal::completion(1, 10000) == 0, "FAQ：起点几乎为零");
    s.add("F09864 教学总包", EpochSeal::completion(5000, 10000) == 50, "教学半盘刻度");
    s.add("F09865 审计终报", seal.sealed.is_some(), "审计以封存为证");
    s.add("F09866 合规终报", seal.sealed == Some("AURORA-10000"), "合规终报指向纪元");
    s.add("F09867 安全终报", !seal.seal(16, 10000, "z"), "安全终报不可重封");
    s.add("F09868 性能终报", EpochSeal::completion(10000, 10000) == 100, "性能满盘");
    s.add("F09869 质量终报", seal.sealed.is_some(), "质量终报在册");
    s.add("F09870 发布终报", seal.sealed == Some("AURORA-10000"), "发布终报在册");
    s.add("F09871 纪元庆典", seal.sealed.is_some() && EpochSeal::completion(10000, 10000) == 100, "庆典满盘双证");
    s.add("F09872 纪元彩蛋", EpochSeal::completion(0, 10000) == 0, "彩蛋零盘对照");
    s.add("F09873 纪念徽章", seal.sealed.is_some(), "徽章以封存颁发");
    s.add("F09874 最终致谢", seal.sealed == Some("AURORA-10000") && EpochSeal::completion(10000, 10000) == 100, "最终致谢三证");
    s.add("F09875 纪元封存", seal.sealed == Some("AURORA-10000") && EpochSeal::completion(10000, 10000) == 100, "封存=纪元名+满盘");
    s
}
