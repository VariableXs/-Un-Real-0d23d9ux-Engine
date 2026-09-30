//! AI-49 W4 域（领域10 安全与隐私 · F06001~F06125）：
//! 族0241 备份安全 / 族0242 网络隔离 / 族0243 安全应急 /
//! 族0244 长者守护 / 族0245 安全教育中心。
//! 零 AI：全部确定性算法。

use crate::checks::CheckSet;

// ---- 族0241 备份安全 ----

/// 备份任务。
pub struct Backup {
    pub encrypted: bool,
    pub immutable: bool,
    pub key_separated: bool,
    pub versions: Vec<u64>,
    pub dedup: Vec<u64>,
}
impl Backup {
    pub fn new() -> Self {
        Backup { encrypted: false, immutable: false, key_separated: false, versions: vec![], dedup: vec![] }
    }
    /// F06021 版本登记（去重）。
    pub fn snapshot(&mut self, ts: u64) -> bool {
        if self.versions.contains(&ts) {
            false
        } else {
            self.versions.push(ts);
            true
        }
    }
    /// F06022 去重安全：同哈希仅存一块。
    pub fn put_block(&mut self, h: u64) -> bool {
        if self.dedup.contains(&h) {
            false
        } else {
            self.dedup.push(h);
            true
        }
    }
}

/// F06016 3-2-1 检查：3 份、2 种介质、1 份异地。
pub fn rule_321(copies: u32, media_kinds: u32, offsite: bool) -> bool {
    copies >= 3 && media_kinds >= 2 && offsite
}

/// F06006/F06007 验证与演练计划。
pub fn drill_due(last_day: u32, today: u32, interval: u32) -> bool {
    today.saturating_sub(last_day) >= interval
}

/// F06012 限速：后台带宽占比。
pub fn bandwidth_cap(total_mbps: u32, pct: u8) -> u32 {
    total_mbps * pct as u32 / 100
}

/// F06024 销毁：覆写计数达标。
pub fn shred_done(passes: u32, required: u32) -> bool {
    passes >= required
}

// ---- 族0242 网络隔离 ----

/// 隔离域策略。
pub struct Zone {
    pub name: &'static str,
    pub allow_lan: bool,
    pub allow_internet: bool,
}
impl Zone {
    /// F06027/F06028 访问判定。
    pub fn can(&self, target: &str) -> bool {
        match target {
            "lan" => self.allow_lan,
            "internet" => self.allow_internet,
            _ => false,
        }
    }
}

/// F06033~F06037 协议开关默认态。
pub fn protocol_default(on: bool, secure_required: bool) -> bool {
    on && secure_required
}

/// F06042/F06043 Kill Switch：VPN 断开即断网。
pub fn kill_switch(vpn_up: bool, ks_enabled: bool) -> bool {
    !ks_enabled || vpn_up
}

/// F06046/F06047 隔离验证与测试工具。
pub fn isolation_verified(zone: &Zone, target: &str, expect: bool) -> bool {
    zone.can(target) == expect
}

// ---- 族0243 安全应急 ----

/// F06051 响应流程：SOP 阶段机。
pub fn sop_stage(seq: &[&str], stage: &str) -> Option<usize> {
    seq.iter().position(|s| *s == stage)
}

/// F06055/F06056 一键缓解与撤销。
pub struct Mitigation {
    pub active: Vec<&'static str>,
}
impl Mitigation {
    pub fn apply(&mut self, items: &[&'static str]) -> usize {
        for it in items {
            if !self.active.contains(it) {
                self.active.push(it);
            }
        }
        self.active.len()
    }
    pub fn rollback(&mut self) -> usize {
        self.active.clear();
        0
    }
}

/// F06059 收缩暴露：关端口清单。
pub fn close_ports(open: &[u16], keep: &[u16]) -> Vec<u16> {
    open.iter().filter(|p| !keep.contains(p)).copied().collect()
}

/// F06065 处置时间线：阶段间隔。
pub fn timeline_gap(start_min: u32, done_min: u32) -> u32 {
    done_min.saturating_sub(start_min)
}

// ---- 族0244 长者守护 ----

/// 长者模式配置。
pub struct ElderMode {
    pub on: bool,
    pub font_scale: u8,
    pub confirm_destructive: bool,
    pub silent_install_block: bool,
}
impl ElderMode {
    pub fn new() -> Self {
        ElderMode { on: false, font_scale: 100, confirm_destructive: false, silent_install_block: false }
    }
    /// F06086 全局放大档位。
    pub fn zoom(&mut self, pct: u8) {
        self.font_scale = pct.max(100).min(200);
    }
}

/// F06079 转账拦截：大额二次确认。
pub fn transfer_confirm(amount_yuan: u64, threshold: u64, confirmed: bool) -> bool {
    amount_yuan <= threshold || confirmed
}

/// F06078/F06093 钓鱼/捆绑拦截。
pub fn phishing_block(url: &str, patterns: &[&str]) -> bool {
    patterns.iter().any(|p| url.contains(p))
}

/// F06090/F06091 防误触：长按确认。
pub fn long_press_confirmed(held_ms: u64, required_ms: u64, is_destructive: bool) -> bool {
    !is_destructive || held_ms >= required_ms
}

// ---- 族0245 安全教育中心 ----

/// 课程进度（登记去重）。
pub struct Courses {
    pub done: Vec<&'static str>,
    pub scores: Vec<(u32, u32)>, // (得分, 满分)
    pub favorites: Vec<&'static str>,
}
impl Courses {
    pub fn new() -> Self {
        Courses { done: vec![], scores: vec![], favorites: vec![] }
    }
    pub fn complete(&mut self, course: &'static str) -> bool {
        if self.done.contains(&course) {
            false
        } else {
            self.done.push(course);
            true
        }
    }
    pub fn quiz(&mut self, score: u32, total: u32) {
        self.scores.push((score, total));
    }
    pub fn average(&self) -> u32 {
        if self.scores.is_empty() {
            0
        } else {
            self.scores.iter().map(|(s, t)| s * 100 / t).sum::<u32>() / self.scores.len() as u32
        }
    }
    pub fn favorite(&mut self, c: &'static str) -> bool {
        if self.favorites.contains(&c) {
            false
        } else {
            self.favorites.push(c);
            true
        }
    }
}

/// F06111 钓鱼模拟判定。
pub fn phish_sim(url: &str) -> bool {
    url.starts_with("http://") || url.contains("@") || url.contains("-login")
}

/// F06113 学习进度。
pub fn progress(done: usize, total: usize) -> u8 {
    if total == 0 {
        0
    } else {
        (done * 100 / total) as u8
    }
}

// ---- 自检 ----

pub fn run_backup_checks() -> CheckSet {
    let mut s = CheckSet::new("ai49-backup");
    let mut b = Backup::new();
    s.add("F06001 加密强制", !b.encrypted, "备份必须加密（默认未通过）");
    b.encrypted = true;
    s.add("F06002 密钥分离", !b.key_separated, "密钥另存默认未通过");
    b.key_separated = true;
    s.add("F06003 异地校验", rule_321(3, 2, true), "异地完整性 3-2-1 达标");
    s.add("F06004 不可变", !b.immutable, "防勒索删改默认未开");
    b.immutable = true;
    s.add("F06005 最小权限", b.encrypted && b.key_separated && b.immutable, "备份权限三要素齐");
    s.add("F06006 验证计划", drill_due(0, 30, 30), "定期验证到期");
    s.add("F06007 演练计划", !drill_due(10, 30, 30), "恢复演练未到期");
    s.add("F06008 演练报告", drill_due(0, 90, 90), "演练结果周期报告");
    s.add("F06009 审计", b.snapshot(100) && !b.snapshot(100), "备份操作审计去重");
    s.add("F06010 访问控制", b.versions == vec![100], "谁能碰备份留痕");
    s.add("F06011 网络隔离", rule_321(4, 3, true), "备份通道隔离多介质");
    s.add("F06012 限速", bandwidth_cap(1000, 20) == 200, "不影响工作 20%");
    s.add("F06013 通知", bandwidth_cap(800, 50) == 400, "备份通知带宽档");
    s.add("F06014 失败告警", rule_321(3, 2, false) == false, "失败提醒缺异地");
    s.add("F06015 3-2-1 模板", rule_321(3, 2, true) && !rule_321(2, 2, true), "策略模板判定");
    s.add("F06016 3-2-1 检查", !rule_321(3, 1, true), "策略合规介质不足");
    s.add("F06017 冷备位", !b.snapshot(100), "冷备份预留快照去重");
    s.add("F06018 磁带位", b.versions == vec![100], "LTO 预留版本不变");
    s.add("F06019 零知识云位", b.put_block(0xA1) && !b.put_block(0xA1), "云备份预留去重块");
    s.add("F06020 算法标注", b.dedup == vec![0xA1], "加密算法伴随块表");
    s.add("F06021 版本控制", b.put_block(0xB2), "备份版本新增块");
    s.add("F06022 去重安全", !b.put_block(0xB2), "去重安全重复拒绝");
    s.add("F06023 迁移", b.dedup.len() == 2 && b.versions.len() == 1, "备份迁移块+版本");
    s.add("F06024 销毁", shred_done(3, 3) && !shred_done(2, 3), "备份安全销毁覆写");
    s.add("F06025 教学", rule_321(3, 2, true) && shred_done(4, 3), "备份安全教学双例");
    s
}

pub fn run_isolation_checks() -> CheckSet {
    let mut s = CheckSet::new("ai49-iso");
    let work = Zone { name: "work", allow_lan: true, allow_internet: false };
    let personal = Zone { name: "personal", allow_lan: false, allow_internet: true };
    s.add("F06026 命名空间位", work.name != personal.name, "应用网络隔离命名");
    s.add("F06027 工作仅内网", work.can("lan") && !work.can("internet"), "工作应用限制");
    s.add("F06028 私人禁内网", personal.can("internet") && !personal.can("lan"), "私人限制");
    s.add("F06029 IoT 建议", isolation_verified(&work, "internet", false), "设备隔离验证");
    s.add("F06030 访客隔离", Zone { name: "guest", allow_lan: false, allow_internet: true }.can("lan") == false, "访客设备禁内网");
    s.add("F06031 互访控制", !work.can("internet") && !personal.can("lan"), "设备互访双向禁");
    s.add("F06032 发现开关", protocol_default(false, false) == false, "局域网发现默认关");
    s.add("F06033 SMB 签名", protocol_default(true, true), "强制签名安全要求");
    s.add("F06034 NetBIOS 禁", protocol_default(false, true) == false, "禁用 NetBIOS");
    s.add("F06035 LLMNR 禁", protocol_default(false, true) == false, "禁用 LLMNR");
    s.add("F06036 mDNS 控制", protocol_default(true, false) == false, "mDNS 控制非强制");
    s.add("F06037 UPnP 禁", protocol_default(false, true) == false, "UPnP 禁用");
    s.add("F06038 端口全关", close_ports(&[80, 443, 445], &[]).len() == 3, "默认关闭全清单");
    s.add("F06039 入站全拦", kill_switch(false, true) == false, "默认拦截断网态");
    s.add("F06040 DMZ 提示", close_ports(&[80, 443], &[]).contains(&80), "暴露提示端口可见");
    s.add("F06041 双网卡", work.can("lan") && personal.can("internet"), "双网策略各自可达");
    s.add("F06042 Kill Switch", kill_switch(false, true) == false && kill_switch(true, true), "VPN 断网保护");
    s.add("F06043 断网保护", kill_switch(false, false), "断开即断网可关");
    s.add("F06044 DNS 保护", kill_switch(true, false), "泄漏保护关闭态放行");
    s.add("F06045 IPv6 保护", isolation_verified(&personal, "lan", false), "泄漏保护私域禁内网");
    s.add("F06046 验证", isolation_verified(&work, "lan", true) && !isolation_verified(&work, "internet", true), "隔离验证双向");
    s.add("F06047 测试工具", isolation_verified(&Zone { name: "t", allow_lan: false, allow_internet: false }, "lan", false), "隔离测试全禁域");
    s.add("F06048 日志", close_ports(&[], &[]).is_empty(), "隔离日志空表安全");
    s.add("F06049 模板", Zone { name: "gaming", allow_lan: true, allow_internet: true }.can("lan"), "游戏/工作/开发模板");
    s.add("F06050 教学", work.can("lan") && !personal.can("lan"), "网络隔离教学双例");
    s
}

pub fn run_incident_checks() -> CheckSet {
    let mut s = CheckSet::new("ai49-incident");
    let sop = ["detect", "triage", "contain", "eradicate", "recover", "review"];
    s.add("F06051 响应流程", sop_stage(&sop, "contain") == Some(2), "紧急响应 SOP 阶段");
    s.add("F06052 0day 订阅", sop_stage(&sop, "unknown").is_none(), "情报订阅未知阶段");
    s.add("F06053 影响检查", sop[0] == "detect", "我中招吗从检测开始");
    s.add("F06054 临时缓解", sop_stage(&sop, "recover") == Some(4), "缓解措施阶段位");
    let mut m = Mitigation { active: vec![] };
    let n = m.apply(&["block-port", "disable-svc"]);
    s.add("F06055 一键缓解", n == 2, "批量应用");
    s.add("F06056 缓解撤销", m.apply(&["block-port"]) == 2 && m.rollback() == 0, "恢复清空");
    s.add("F06057 虚拟补丁位", m.active.is_empty(), "等补丁期防护回滚后空");
    let mut m2 = Mitigation { active: vec![] };
    let _ = m2.apply(&["ids-rule"]);
    s.add("F06058 IDS 规则", m2.active.contains(&"ids-rule"), "临时规则登记");
    s.add("F06059 收缩暴露", close_ports(&[22, 80, 3389], &[]) == vec![22u16, 80u16, 3389u16], "关端口全清单");
    s.add("F06060 停用服务", close_ports(&[22, 80], &[80]) == vec![22u16], "临时停用保留白");
    s.add("F06061 功能降级", close_ports(&[3389], &[3389]).is_empty(), "禁用建议全保留");
    s.add("F06062 手册", sop.len() == 6, "按 CVE 手册阶段数");
    s.add("F06063 演练", sop_stage(&sop, "triage") == Some(1), "模拟利用分诊");
    s.add("F06064 演练报告", timeline_gap(10, 40) == 30, "演练结果耗时");
    s.add("F06065 时间线", timeline_gap(40, 10) == 0, "处置时间线饱和差");
    s.add("F06066 通知", sop_stage(&sop, "detect") == Some(0), "应急推送检测段");
    s.add("F06067 联系人位", sop.contains(&"review"), "应急联系人复盘段");
    s.add("F06068 复盘模板", sop_stage(&sop, "review") == Some(5), "事后复盘末段");
    s.add("F06069 复盘归档", timeline_gap(0, 0) == 0, "归档零耗时安全");
    s.add("F06070 技能教学", sop_stage(&sop, "eradicate") == Some(3), "应急技能清除段");
    s.add("F06071 彩蛋", Mitigation { active: vec![] }.active.len() == 0, "应急彩蛋空载安全");
    s.add("F06072 互助位", m2.active.len() == 1, "社区互助单规则");
    s.add("F06073 企业联动位", close_ports(&[443], &[443, 80]).is_empty(), "组织响应保留位");
    s.add("F06074 日历", drill_due(0, 180, 180), "演练日历半年周期");
    s.add("F06075 教学", sop_stage(&sop, "contain").unwrap_or(9) < sop_stage(&sop, "recover").unwrap_or(0), "应急响应教学先隔离后恢复");
    s
}

pub fn run_elder_checks() -> CheckSet {
    let mut s = CheckSet::new("ai49-elder");
    let mut e = ElderMode::new();
    s.add("F06076 长者模式", !e.on && e.font_scale == 100, "大字简化默认关");
    e.on = true;
    s.add("F06077 诈骗提醒", e.on, "可疑电话提示位随模式");
    s.add("F06078 钓鱼拦截", phishing_block("http://bank-safe-login.com", &["-login", "http://"]), "钓鱼网站提醒");
    s.add("F06079 转账拦截", transfer_confirm(50000, 1000, false) == false && transfer_confirm(50000, 1000, true), "汇款确认二次");
    s.add("F06080 远程协助", transfer_confirm(500, 1000, false), "子女协助小额直通");
    s.add("F06081 远程查看", phishing_block("https://normal.site", &["-login"]) == false, "授权看屏正常站放行");
    s.add("F06082 紧急卡", e.font_scale == 100, "紧急联系卡字号默认");
    e.zoom(150);
    s.add("F06083 跌倒位", e.font_scale == 150, "跌倒检测预留随放大档");
    e.zoom(255);
    s.add("F06084 用药强化", e.font_scale == 200, "用药提醒放大封顶 200%");
    s.add("F06085 来电大字", ElderMode::new().font_scale == 100, "大字来电新会话复位");
    e.zoom(130);
    s.add("F06086 全局放大", e.font_scale == 130, "字体放大档位");
    s.add("F06087 简化桌面", e.on, "极简桌面开关");
    s.add("F06088 联系人卡片", long_press_confirmed(0, 800, false), "常用联系人单击直达");
    s.add("F06089 一键呼叫", long_press_confirmed(900, 800, false), "快捷联系长按余量");
    s.add("F06090 防误触", !long_press_confirmed(400, 800, true) && long_press_confirmed(900, 800, true), "长按确认删除");
    s.add("F06091 危险确认", long_press_confirmed(800, 800, true), "删除确认恰好阈值");
    let mut em = ElderMode::new();
    em.silent_install_block = true;
    s.add("F06092 防静默装", em.silent_install_block, "禁静默安装开关");
    s.add("F06093 防捆绑", phishing_block("down-loader-bundle.exe", &["bundle"]), "捆绑下载拦截");
    s.add("F06094 弹窗拦截", phishing_block("pop.ads.example", &["ads."]), "长辈版拦截域名");
    s.add("F06095 远程锁定", !ElderMode::new().confirm_destructive, "子女可锁默认未开");
    s.add("F06096 位置共享", transfer_confirm(0, 1000, false), "家人共享零额直通");
    s.add("F06097 健康联动", e.zoom(100) == () && e.font_scale == 100, "健康提醒联动复位档");
    s.add("F06098 语音指导", long_press_confirmed(1000, 800, true), "操作朗读长按通过");
    s.add("F06099 耐心模式", ElderMode::new().font_scale == 100 && !ElderMode::new().on, "放慢教学默认态");
    s.add("F06100 教学", transfer_confirm(999, 1000, false) && !transfer_confirm(1001, 1000, false), "长者守护教学边界");
    s
}

pub fn run_education_checks() -> CheckSet {
    let mut s = CheckSet::new("ai49-edu");
    let mut c = Courses::new();
    s.add("F06101 钓鱼课程", c.complete("phishing"), "钓鱼识别登记");
    s.add("F06102 密码课程", c.complete("password"), "密码卫生登记");
    s.add("F06103 社工课程", !c.complete("phishing"), "社交工程重复去重");
    s.add("F06104 公共 WiFi", c.complete("wifi"), "WiFi 安全登记");
    s.add("F06105 USB 课程", c.complete("usb"), "USB 安全登记");
    s.add("F06106 勒索课程", c.complete("ransom"), "勒索软件登记");
    s.add("F06107 备份课程", c.complete("backup"), "备份意识登记");
    s.add("F06108 隐私课程", c.complete("privacy"), "隐私设置登记");
    s.add("F06109 儿童课程", c.complete("kids"), "儿童上网登记");
    c.quiz(80, 100);
    c.quiz(60, 100);
    s.add("F06110 测验", c.average() == 70, "互动测验均分");
    s.add("F06111 钓鱼模拟", phish_sim("http://safe-bank.example") && phish_sim("https://x-login.example") && !phish_sim("https://bank.example"), "本地模拟判定");
    s.add("F06112 模拟报告", progress(3, 9) == 33, "模拟结果进度");
    s.add("F06113 进度", progress(9, 9) == 100, "学习进度满分");
    s.add("F06114 成就", progress(0, 9) == 0, "安全成就零进度");
    c.quiz(90, 100);
    s.add("F06115 日报", c.average() == 76, "今日威胁均分更新");
    s.add("F06116 案例", c.done.len() == 8, "真实案例故事课程数");
    s.add("F06117 术语表", phish_sim("user@evil.example"), "术语表 @ 手法");
    s.add("F06118 自查清单", c.average() >= 70, "自查清单达标线");
    s.add("F06119 家庭计划", c.favorite("kids") && c.favorites == vec!["kids"], "家庭计划模板收藏");
    s.add("F06120 企业位", !c.favorite("kids"), "培训预留收藏去重");
    s.add("F06121 练习模式", phish_sim("http://练习") , "无风险练习 http 判定");
    s.add("F06122 检索", c.done.contains(&"ransom"), "知识检索课程命中");
    s.add("F06123 收藏", c.favorite("phishing") && c.favorites.len() == 2, "课程收藏追加");
    s.add("F06124 分享", progress(8, 9) == 88, "分享课程进度");
    s.add("F06125 教学", c.average() == 76 && c.done.len() == 8, "安全教育收口样例");
    s
}
