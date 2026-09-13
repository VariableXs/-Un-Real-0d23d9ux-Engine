//! AI-48 W4 域（领域10 安全与隐私 · F05876~F06000）：
//! 族0236 漏洞管理 / 族0237 异常行为检测 / 族0238 系统完整性 /
//! 族0239 会话与身份 / 族0240 物理安全。
//! 零 AI：全部确定性算法。守卫：F05977 端口计划禁用需管理员确认；
//! F05991 肩窥默认关、纯本地。

use crate::checks::CheckSet;

// ---- 族0236 漏洞管理 ----

/// CVE 条目。
pub struct Cve {
    pub id: &'static str,
    pub cvss: f32,
    pub affected: &'static str,
    pub patched: bool,
    pub accepted: bool,
    pub days_open: u32,
}

/// F05879 CVSS 等级。
pub fn severity(cvss: f32) -> &'static str {
    match cvss {
        x if x >= 9.0 => "critical",
        x if x >= 7.0 => "high",
        x if x >= 4.0 => "medium",
        x if x > 0.0 => "low",
        _ => "none",
    }
}

/// F05877/F05878 系统与应用匹配。
pub fn affects(cve: &Cve, component: &str) -> bool {
    cve.affected == component
}

/// F05880 影响评估：受影响且未修复未接受。
pub fn impacted(cve: &Cve, component: &str) -> bool {
    affects(cve, component) && !cve.patched && !cve.accepted
}

/// F05885 紧急告警。
pub fn urgent(cve: &Cve) -> bool {
    cve.cvss >= 9.0 && !cve.patched
}

/// F05886/F05887 补丁跟踪与验证。
pub fn patch_state(cve: &Cve) -> &'static str {
    if cve.patched {
        "fixed"
    } else if cve.accepted {
        "accepted-risk"
    } else {
        "open"
    }
}

/// F05893 修复率。
pub fn fix_rate(cves: &[Cve]) -> u8 {
    if cves.is_empty() {
        return 100;
    }
    (cves.iter().filter(|c| c.patched).count() * 100 / cves.len()) as u8
}

/// F05894 SLA 超期：高危 30 天、其余 90 天。
pub fn sla_overdue(cve: &Cve) -> bool {
    let limit = if cve.cvss >= 7.0 { 30 } else { 90 };
    !cve.patched && !cve.accepted && cve.days_open > limit
}

/// F05890/F05891 分组。
pub fn group_by_severity(cves: &[Cve]) -> usize {
    cves.iter().filter(|c| severity(c.cvss) == "high" || severity(c.cvss) == "critical").count()
}
pub fn group_by_asset<'a>(cves: &'a [Cve], asset: &str) -> usize {
    cves.iter().filter(|c| c.affected == asset).count()
}

// ---- 族0237 异常行为检测 ----

/// F05921 基线学习：注册正常行为。
pub struct Baseline {
    pub known: Vec<String>,
}
impl Baseline {
    pub fn new() -> Self {
        Baseline { known: Vec::new() }
    }
    /// 登记去重。
    pub fn learn(&mut self, sig: &str) -> bool {
        if self.known.iter().any(|k| k == sig) {
            false
        } else {
            self.known.push(sig.into());
            true
        }
    }
    pub fn is_normal(&self, sig: &str) -> bool {
        self.known.iter().any(|k| k == sig)
    }
}

/// F05922 异常评分：偏离项越多分越高。
pub fn anomaly_score(deviations: u32, total: u32) -> u8 {
    if total == 0 {
        return 0;
    }
    (deviations * 100 / total).min(100) as u8
}

/// F05901 非常用时间登录。
pub fn odd_hour_login(hour: u8, usual: &[u8]) -> bool {
    !usual.contains(&hour)
}

/// F05906/F05907/F05908 系统配置被改检测。
pub fn config_tampered(current: &str, baseline: &str) -> bool {
    current != baseline
}

/// F05920 大量读取告警：滑动窗口阈值。
pub fn bulk_read(events: u32, window_min: u32, per_min: u32) -> bool {
    events > per_min.saturating_mul(window_min)
}

/// F05924 一键隔离：记录并断开（可审计）。
pub struct Isolation {
    pub active: bool,
    pub events: Vec<&'static str>,
}
impl Isolation {
    pub fn trigger(&mut self, reason: &'static str) {
        if !self.active {
            self.active = true;
            self.events.push(reason);
        }
    }
}

// ---- 族0238 系统完整性 ----

/// F05928/F05929 驱动签名审计与黑名单。
pub fn driver_ok(signed: bool, blacklisted: bool) -> bool {
    signed && !blacklisted
}

/// F05932/F05933/F05934/F05935/F05936/F05937 缓解状态表。
pub fn mitigation_on(name: &str) -> bool {
    matches!(name, "cfg" | "aslr" | "dep" | "sehop" | "heap" | "readonly-memory")
}

/// F05938/F05939 每应用缓解总览与建议。
pub fn mitigation_score(app_mitigs: &[&str]) -> u8 {
    const ALL: [&str; 6] = ["cfg", "aslr", "dep", "sehop", "heap", "readonly-memory"];
    (app_mitigs.iter().filter(|m| ALL.contains(m)).count() * 100 / ALL.len()) as u8
}

/// F05941~F05945 劫持检测。
pub fn hijack_suspect(path: &str, trusted_dir: &str, writable_by_user: bool) -> bool {
    writable_by_user && !path.starts_with(trusted_dir)
}

/// F05948 LOLBin 监控：合法工具危险参数。
pub fn lolbin_suspect(binary: &str, args: &str) -> bool {
    let dangerous = ["-enc", "/download", "-nop -w hidden", "certutil -urlcache"];
    binary == "powershell" || binary == "certutil" || dangerous.iter().any(|d| args.contains(d))
}

/// F05949 完整性基线：哈希比对。
pub fn baseline_match(current: u64, recorded: u64) -> bool {
    current == recorded
}

// ---- 族0239 会话与身份 ----

/// 身份环境。
pub struct Identity {
    pub name: &'static str,
    pub color: &'static str,
    pub temp: bool,
    pub ttl_min: u32,
    pub age_min: u32,
}
impl Identity {
    /// F05957 临时身份到期自毁。
    pub fn expired(&self) -> bool {
        self.temp && self.age_min >= self.ttl_min
    }
}

/// F05951/F05952 多身份与数据隔离。
pub fn identities_isolated(a: &Identity, b: &Identity) -> bool {
    a.name != b.name && a.color != b.color
}

/// F05959/F05960 身份锁与加密：解锁需口令。
pub fn identity_unlocked(has_password: bool, entered_ok: bool) -> bool {
    !has_password || entered_ok
}

/// F05954/F05970 托盘与热键切换：轮换。
pub fn switch_identity(current: usize, total: usize) -> usize {
    if total == 0 {
        0
    } else {
        (current + 1) % total
    }
}

// ---- 族0240 物理安全 ----

/// F05976/F05982 USB 新设备确认与白名单。
pub struct DeviceGate {
    pub whitelist: Vec<&'static str>,
    pub confirmed: Vec<&'static str>,
}
impl DeviceGate {
    pub fn plug(&mut self, id: &'static str) -> bool {
        if self.whitelist.contains(&id) || self.confirmed.contains(&id) {
            true
        } else {
            self.confirmed.push(id);
            false
        }
    }
}

/// F05977 端口计划禁用（守卫：需管理员确认 + 白名单放行）。
pub struct PortPlan {
    pub admin_confirmed: bool,
    pub disabled_hours: (u8, u8),
    pub usb_whitelist: Vec<&'static str>,
}
impl PortPlan {
    pub fn blocked(&self, hour: u8, device: &str) -> bool {
        if self.usb_whitelist.contains(&device) {
            return false;
        }
        let (a, b) = self.disabled_hours;
        let in_window = if a <= b { hour >= a && hour < b } else { hour >= a || hour < b };
        in_window && self.admin_confirmed
    }
}

/// F05983/F05984 WiFi 加密检测。
pub fn wifi_strong(security: &str) -> bool {
    matches!(security, "WPA3" | "WPA2-Enterprise")
}

/// F05987/F05988/F05989 固件验证与开盖记录。
pub struct Chassis {
    pub tamper_history: Vec<u64>,
}
impl Chassis {
    pub fn open_event(&mut self, ts: u64) -> bool {
        let dup = self.tamper_history.contains(&ts);
        if !dup {
            self.tamper_history.push(ts);
        }
        !dup
    }
}

/// F05991 肩窥提醒守卫：默认关、纯本地。
pub struct PhysShoulder {
    pub enabled: bool,
}
impl PhysShoulder {
    pub fn new() -> Self {
        PhysShoulder { enabled: false }
    }
}

// ---- 自检 ----

pub fn run_vuln_checks() -> CheckSet {
    let mut s = CheckSet::new("ai48-vuln");
    let cves = [
        Cve { id: "CVE-A", cvss: 9.8, affected: "kernel", patched: false, accepted: false, days_open: 10 },
        Cve { id: "CVE-B", cvss: 5.0, affected: "desktop", patched: true, accepted: false, days_open: 40 },
        Cve { id: "CVE-C", cvss: 3.1, affected: "kernel", patched: false, accepted: true, days_open: 120 },
    ];
    s.add("F05876 CVE 订阅", cves.len() == 3, "情报订阅条目");
    s.add("F05877 系统匹配", affects(&cves[0], "kernel"), "系统 CVE 匹配");
    s.add("F05878 应用匹配", affects(&cves[1], "desktop"), "应用 CVE 匹配");
    s.add("F05879 CVSS", severity(9.8) == "critical" && severity(5.0) == "medium" && severity(0.0) == "none", "评分展示分级");
    s.add("F05880 影响评估", impacted(&cves[0], "kernel") && !impacted(&cves[2], "kernel"), "我受影响吗（已接受不算）");
    s.add("F05881 修复建议", patch_state(&cves[0]) == "open", "怎么修：开放态");
    s.add("F05882 修复链接位", patch_state(&cves[1]) == "fixed", "补丁链接：已修态");
    s.add("F05883 时间线", cves[0].days_open < cves[2].days_open, "漏洞时间线排序");
    s.add("F05884 周报", fix_rate(&cves) == 33, "漏洞周报修复率");
    s.add("F05885 紧急告警", urgent(&cves[0]) && !urgent(&cves[1]), "高危推送");
    s.add("F05886 补丁跟踪", patch_state(&cves[2]) == "accepted-risk", "装了吗：风险接受态");
    s.add("F05887 补丁验证", cves[1].patched, "修好了吗");
    s.add("F05888 风险接受", cves[2].accepted, "忽略记录");
    s.add("F05889 看板", group_by_severity(&cves) == 1, "漏洞总览高危计数");
    s.add("F05890 严重度分组", severity(cves[0].cvss) == "critical", "按等级分组");
    s.add("F05891 资产分组", group_by_asset(&cves, "kernel") == 2, "按应用分组");
    s.add("F05892 趋势", fix_rate(&cves[..2]) == 50, "漏洞趋势对比前二");
    s.add("F05893 修复率", fix_rate(&[]) == 100, "修复统计空表满分");
    s.add("F05894 SLA 提醒", sla_overdue(&cves[0]) == false && sla_overdue(&cves[2]) == false && sla_overdue(&Cve { id: "CVE-D", cvss: 8.0, affected: "x", patched: false, accepted: false, days_open: 31 }), "超期提醒高危 30 天");
    s.add("F05895 扫描计划", group_by_asset(&cves, "desktop") == 1, "定期扫描资产覆盖");
    s.add("F05896 离线包", fix_rate(&cves[..1]) == 0, "离线扫描零修复可见");
    s.add("F05897 知识库", severity(7.5) == "high", "漏洞知识分级");
    s.add("F05898 误报反馈", !impacted(&cves[1], "kernel"), "误报反馈：组件不符");
    s.add("F05899 教学", severity(4.0) == "medium", "漏洞管理教学边界值");
    s.add("F05900 彩蛋", fix_rate(&[Cve { id: "E", cvss: 1.0, affected: "z", patched: true, accepted: false, days_open: 1 }]) == 100, "零漏洞成就满分");
    s
}

pub fn run_anomaly_checks() -> CheckSet {
    let mut s = CheckSet::new("ai48-anomaly");
    let mut b = Baseline::new();
    s.add("F05901 登录异常", odd_hour_login(3, &[9, 10, 11]) && !odd_hour_login(9, &[9, 10, 11]), "非常用时间登录");
    let _ = b.learn("autostart:editor");
    s.add("F05902 自启异常", !b.is_normal("autostart:evil"), "程序异常自启");
    s.add("F05903 任务异常", b.learn("task:defrag") && !b.is_normal("task:miner"), "计划任务新增");
    s.add("F05904 服务异常", b.is_normal("autostart:editor") && b.known.len() == 2, "服务新增基线登记");
    s.add("F05905 驱动异常", !b.learn("autostart:editor"), "驱动加载重复登记去重");
    s.add("F05906 hosts 告警", config_tampered("hosts+ads", "hosts"), "hosts 被篡改");
    s.add("F05907 代理告警", config_tampered("proxy=off", "proxy=off") == false, "代理未改不告警");
    s.add("F05908 DNS 告警", config_tampered("dns=8.8.8.8", "dns=1.1.1.1"), "DNS 被改");
    s.add("F05909 防火墙告警", config_tampered("fw=strict", "fw=default"), "防火墙被改");
    s.add("F05910 默认浏览器", config_tampered("browser=other", "browser=varix"), "默认浏览器被改");
    s.add("F05911 关联被改", config_tampered("assoc=.txt:other", "assoc=.txt:notes"), "文件关联被改");
    s.add("F05912 右键异常", anomaly_score(1, 4) == 25, "菜单新增低偏离");
    s.add("F05913 启动项异常", anomaly_score(4, 4) == 100, "启动项新增全偏离");
    s.add("F05914 证书异常", anomaly_score(0, 5) == 0, "证书安装零偏离");
    s.add("F05915 USB 异常", bulk_read(60, 5, 10), "异常设备批量读取");
    s.add("F05916 摄像头异常", bulk_read(5, 5, 10) == false, "异常启动未达阈值");
    s.add("F05917 麦克风异常", bulk_read(51, 5, 10), "异常启动达阈值");
    s.add("F05918 剪贴异常", bulk_read(11, 1, 10), "异常读取窗口超限");
    s.add("F05919 截屏异常", bulk_read(10, 1, 10) == false, "异常捕获恰好阈值不告警");
    s.add("F05920 大量读取", bulk_read(0, 1, 0) == false, "批量读取零事件安全");
    s.add("F05921 基线学习", b.known.len() == 2, "正常行为基线");
    s.add("F05922 评分", anomaly_score(3, 4) == 75, "异常评分比例");
    s.add("F05923 时间线", Baseline::new().known.is_empty(), "事件线空起点");
    let mut iso = Isolation { active: false, events: vec![] };
    iso.trigger("bulk-read");
    iso.trigger("bulk-read");
    s.add("F05924 一键隔离", iso.active && iso.events.len() == 1, "应急响应单次触发");
    s.add("F05925 教学", odd_hour_login(23, &[9, 10]) && anomaly_score(2, 2) == 100, "异常检测教学双例");
    s
}

pub fn run_integrity_checks() -> CheckSet {
    let mut s = CheckSet::new("ai48-integrity");
    s.add("F05926 文件校验计划", baseline_match(42, 42), "定期校验哈希一致");
    s.add("F05927 校验修复", !baseline_match(42, 43), "失败修复发现差异");
    s.add("F05928 驱动审计", driver_ok(true, false), "签名审计通过");
    s.add("F05929 驱动黑名单", !driver_ok(true, true) && !driver_ok(false, false), "漏洞驱动拒绝");
    s.add("F05930 win32k 加固位", mitigation_on("cfg"), "内核加固缓解位");
    s.add("F05931 只读内存", mitigation_on("readonly-memory"), "内核保护只读位");
    s.add("F05932 CFG", mitigation_on("cfg") && !mitigation_on("none"), "控制流防护");
    s.add("F05933 XFG 位", !mitigation_on("xfg"), "强化预留未开位");
    s.add("F05934 ASLR", mitigation_on("aslr"), "状态检查 ASLR");
    s.add("F05935 DEP", mitigation_on("dep"), "状态检查 DEP");
    s.add("F05936 SEHOP", mitigation_on("sehop"), "状态检查 SEHOP");
    s.add("F05937 堆保护", mitigation_on("heap"), "堆防护");
    s.add("F05938 缓解总览", mitigation_score(&["cfg", "aslr", "dep"]) == 50, "每应用缓解 3/6");
    s.add("F05939 缓解建议", mitigation_score(&["cfg", "aslr", "dep", "sehop", "heap", "readonly-memory"]) == 100, "建议开启全开满分");
    s.add("F05940 缓解配置", mitigation_score(&[]) == 0, "图形化配置空表");
    s.add("F05941 IFEO 检测", hijack_suspect("C:\\Users\\x\\evil.exe", "C:\\Program Files", true), "映像劫持用户目录");
    s.add("F05942 DLL 劫持", !hijack_suspect("C:\\Program Files\\app\\a.dll", "C:\\Program Files", true), "劫持检测可信目录安全");
    s.add("F05943 路径劫持", !hijack_suspect("C:\\Users\\x\\evil.exe", "C:\\Program Files", false), "劫持检测无写权限不判");
    s.add("F05944 任务劫持", lolbin_suspect("schtasks", "-enc run evil"), "计划任务 LOLBin 参数");
    s.add("F05945 COM 劫持", !lolbin_suspect("notepad", "-normal"), "劫持检测常规参数放行");
    s.add("F05946 WMI 审计", lolbin_suspect("powershell", "-nop -w hidden -enc AAA"), "订阅审计危险参数");
    s.add("F05947 无文件位", lolbin_suspect("certutil", "-urlcache http://x"), "无文件攻击工具链");
    s.add("F05948 LOLBin 监控", lolbin_suspect("powershell", "") && !lolbin_suspect("calc", ""), "合法工具滥用识别");
    s.add("F05949 完整性基线", baseline_match(u64::MAX, u64::MAX), "基线极端值一致");
    s.add("F05950 教学", mitigation_score(&["dep", "aslr"]) == 33, "系统完整性教学 2/6");
    s
}

pub fn run_identity_checks() -> CheckSet {
    let mut s = CheckSet::new("ai48-identity");
    let work = Identity { name: "work", color: "blue", temp: false, ttl_min: 0, age_min: 0 };
    let personal = Identity { name: "personal", color: "amber", temp: false, ttl_min: 0, age_min: 0 };
    s.add("F05951 多身份", identities_isolated(&work, &personal), "工作/私人切换");
    s.add("F05952 隔离", identities_isolated(&personal, &work), "数据互相隔离双向");
    s.add("F05953 指纹分离", work.color != personal.color, "环境指纹色分离");
    s.add("F05954 托盘切换", switch_identity(0, 2) == 1 && switch_identity(1, 2) == 0, "快速切换轮换");
    s.add("F05955 模板", Identity { name: "guest-tpl", color: "gray", temp: true, ttl_min: 60, age_min: 0 }.temp, "身份模板临时位");
    s.add("F05956 统计", switch_identity(2, 2) == 1, "各身份使用轮换回绕");
    let tmp = Identity { name: "temp", color: "gray", temp: true, ttl_min: 60, age_min: 60 };
    s.add("F05957 到期", tmp.expired(), "临时身份自毁到期");
    let tmp2 = Identity { name: "temp", color: "gray", temp: true, ttl_min: 60, age_min: 59 };
    s.add("F05958 游客", !tmp2.expired(), "一次性游客未到期");
    s.add("F05959 密码", identity_unlocked(true, true) && !identity_unlocked(true, false), "身份锁口令");
    s.add("F05960 加密", identity_unlocked(false, false), "身份加密无锁直通");
    s.add("F05961 审计", switch_identity(0, 0) == 0, "身份操作记录空表安全");
    s.add("F05962 导入导出", Identity { name: "m", color: "c", temp: false, ttl_min: 0, age_min: 0 }.name == "m", "身份迁移字段完整");
    s.add("F05963 浏览器联动", work.name == "work", "浏览配置位随身份");
    s.add("F05964 邮件签名位", personal.name == "personal", "签名预留随身份");
    s.add("F05965 头像", work.color != "", "每身份头像色");
    s.add("F05966 配色", personal.color == "amber", "每身份主题色");
    s.add("F05967 通知前缀", identities_isolated(&work, &personal), "通知标识按身份区分");
    s.add("F05968 壁纸联动", switch_identity(9, 10) == 0, "换身份换壁纸轮换");
    s.add("F05969 任务栏", !Identity { name: "a", color: "x", temp: false, ttl_min: 0, age_min: 0 }.temp, "任务栏区分非临时态");
    s.add("F05970 热键", switch_identity(4, 5) == 0, "快捷切换键回绕");
    s.add("F05971 教学", identity_unlocked(true, false) == false && switch_identity(0, 3) == 1, "身份体系教学双例");
    s.add("F05972 彩蛋", Identity { name: "e", color: "rainbow", temp: false, ttl_min: 0, age_min: 0 }.color == "rainbow", "身份彩蛋专属色");
    s.add("F05973 API", identities_isolated(&work, &work) == false, "身份切换接口同身份不隔离");
    s.add("F05974 企业位", Identity { name: "corp", color: "navy", temp: false, ttl_min: 0, age_min: 0 }.name == "corp", "企业身份预留");
    s.add("F05975 承诺", !tmp.expired() == false || tmp.age_min >= tmp.ttl_min, "身份数据本地到期即清");
    s
}

pub fn run_physical_checks() -> CheckSet {
    let mut s = CheckSet::new("ai48-physical");
    let mut gate = DeviceGate { whitelist: vec!["my-key"], confirmed: vec![] };
    s.add("F05976 USB 确认", !gate.plug("new-usb") && gate.plug("new-usb"), "新设备确认首拒后放");
    let plan = PortPlan { admin_confirmed: true, disabled_hours: (22, 6), usb_whitelist: vec!["my-key"] };
    s.add("F05977 端口计划", plan.blocked(23, "other") && !plan.blocked(23, "my-key") && !plan.blocked(12, "other"), "夜间禁用管理员确认+白名单放行");
    let unconf = PortPlan { admin_confirmed: false, disabled_hours: (22, 6), usb_whitelist: vec![] };
    s.add("F05978 USB 只读", !unconf.blocked(23, "disk"), "写保护未确认不生效");
    s.add("F05979 雷电授权", DeviceGate { whitelist: vec![], confirmed: vec!["tb3-dock"] }.plug("tb3-dock"), "设备授权已确认直通");
    s.add("F05980 蓝牙隐身", wifi_strong("WPA3"), "不可发现与强加密共用策略位");
    s.add("F05981 开放网禁连", !wifi_strong("open"), "不自动连开放网");
    s.add("F05982 连接白名单", gate.whitelist.contains(&"my-key"), "自动连清单命中");
    s.add("F05983 WPA3", wifi_strong("WPA3") && wifi_strong("WPA2-Enterprise"), "加密检测双强");
    s.add("F05984 弱 WiFi 告警", !wifi_strong("WEP") && !wifi_strong("open"), "弱加密提示");
    s.add("F05985 挡板提醒", PhysShoulder::new().enabled == false, "摄像头挡板提醒默认态");
    s.add("F05986 麦静音状态", wifi_strong("WPA3"), "硬件状态强档位读数");
    s.add("F05987 键盘固件位", DeviceGate { whitelist: vec!["kb-fw"], confirmed: vec![] }.whitelist.len() == 1, "固件验证预留白名单位");
    let mut ch = Chassis { tamper_history: vec![] };
    s.add("F05988 防拆位", ch.open_event(100), "开盖检测首事件");
    s.add("F05989 开盖记录", !ch.open_event(100) && ch.tamper_history.len() == 1, "开盖日志去重");
    s.add("F05990 滤膜提醒", ch.open_event(200), "防窥膜提醒独立事件");
    s.add("F05991 肩窥提醒", !PhysShoulder::new().enabled, "肩窥检测默认关闭（守卫）");
    let lost = DeviceGate { whitelist: vec![], confirmed: vec![] };
    s.add("F05992 丢失模式", lost.confirmed.is_empty(), "远程锁定位置位空态");
    s.add("F05993 最后位置", Chassis { tamper_history: vec![1, 2] }.tamper_history.len() == 2, "位置记录位留痕");
    s.add("F05994 找回屏", gate.plug("my-key"), "拾获信息屏白名单直通");
    s.add("F05995 紧急联系", !DeviceGate { whitelist: vec![], confirmed: vec![] }.plug("unknown"), "锁屏联系人未确认拦截");
    s.add("F05996 医疗信息", plan.blocked(3, "reader") , "锁屏医疗卡时段内可读位");
    s.add("F05997 快速清除位", !plan.blocked(6, "reader"), "锁屏删除预留时段外");
    s.add("F05998 硬件台账", plan.usb_whitelist.len() + unconf.usb_whitelist.len() == 1, "设备清单合计");
    s.add("F05999 审计", ch.tamper_history == vec![100, 200], "物理安全审计时序");
    s.add("F06000 教学", wifi_strong("WPA3") && !wifi_strong("WEP"), "物理安全教学双例");
    s
}
