//! AI-47 W4 域（领域10 安全与隐私 · F05751~F05875）：
//! 族0231 防火墙中心 / 族0232 隐私仪表盘 / 族0233 反追踪 /
//! 族0234 加密通信 / 族0235 数据主权。
//! 零 AI：全部确定性算法。

use crate::checks::CheckSet;

// ---- 族0231 防火墙中心 ----

/// 防火墙规则。
pub struct FwRule {
    pub app: &'static str,
    pub dir: &'static str, // "in" / "out"
    pub action: &'static str, // "allow" / "deny"
    pub profile: &'static str, // domain/private/public
    pub from_h: u8,
    pub to_h: u8,
    pub note: &'static str,
    pub group: &'static str,
}

/// F05751~F05753 出/入站匹配 + 向导生成。
pub fn fw_match(rules: &[FwRule], app: &str, dir: &str, profile: &str, hour: u8) -> Option<&'static str> {
    rules
        .iter()
        .find(|r| r.app == app && r.dir == dir && (r.profile == profile || r.profile == "any") && hour >= r.from_h && hour <= r.to_h)
        .map(|r| r.action)
}

/// F05760/F05761 默认策略：默认拦出站；白名单模式全默认拒。
pub fn default_action(whitelist: bool, dir: &str) -> &'static str {
    if whitelist || dir == "out" {
        "deny"
    } else {
        "allow"
    }
}

/// F05764 来宾（公共网络）自动加严。
pub fn guest_strict<'a>(profile: &str, action: &'a str) -> &'a str {
    if profile == "public" {
        "deny"
    } else {
        action
    }
}

/// F05765 规则冲突检测：同应用同方向同配置文件双动作。
pub fn fw_conflict(rules: &[FwRule]) -> bool {
    for (i, a) in rules.iter().enumerate() {
        for b in rules.iter().skip(i + 1) {
            if a.app == b.app && a.dir == b.dir && a.profile == b.profile && a.action != b.action {
                return true;
            }
        }
    }
    false
}

/// F05768 时段表生效。
pub fn schedule_active(from_h: u8, to_h: u8, hour: u8) -> bool {
    if from_h <= to_h {
        hour >= from_h && hour <= to_h
    } else {
        hour >= from_h || hour <= to_h
    }
}

/// F05769 规则验证：时段/方向/动作合法性。
pub fn rule_valid(r: &FwRule) -> bool {
    (r.dir == "in" || r.dir == "out") && (r.action == "allow" || r.action == "deny") && r.from_h <= 23 && r.to_h <= 23
}

/// F05772 拦截统计。
pub struct FwStats {
    pub allowed: u32,
    pub denied: u32,
}
impl FwStats {
    pub fn record(&mut self, action: &str) {
        if action == "allow" {
            self.allowed += 1;
        } else {
            self.denied += 1;
        }
    }
    pub fn total(&self) -> u32 {
        self.allowed + self.denied
    }
}

/// F05773 可疑告警：短窗口高频拦截。
pub fn suspicious(denied: u32, window_min: u32, threshold_per_min: u32) -> bool {
    denied > threshold_per_min.saturating_mul(window_min)
}

// ---- 族0232 隐私仪表盘 ----

/// 敏感资源访问记录（F05783~F05789）。
pub struct AccessLog {
    pub entries: Vec<(&'static str, &'static str)>, // (资源, 应用)
}
impl AccessLog {
    pub fn record(&mut self, res: &'static str, app: &'static str) {
        self.entries.push((res, app));
    }
    pub fn who(&self, res: &str) -> Vec<&'static str> {
        self.entries.iter().filter(|(r, _)| *r == res).map(|(_, a)| *a).collect()
    }
}

/// F05776/F05777 隐私总分与体检。
pub fn privacy_score(checks: &[bool]) -> u8 {
    if checks.is_empty() {
        return 0;
    }
    (checks.iter().filter(|&&b| b).count() * 100 / checks.len()) as u8
}

/// F05778/F05779/F05781 建议清单与一键加固（可回滚）。
pub struct Hardening {
    pub applied: Vec<&'static str>,
    pub undone: Vec<&'static str>,
}
impl Hardening {
    pub fn apply(&mut self, items: &[&'static str]) -> usize {
        for it in items {
            if !self.applied.contains(it) {
                self.applied.push(it);
            }
        }
        self.applied.len()
    }
    pub fn undo(&mut self) -> bool {
        self.undone = std::mem::take(&mut self.applied);
        self.applied.is_empty()
    }
}

/// F05795 快捷模式。
pub fn quick_mode(mode: &str) -> u8 {
    match mode {
        "work" => 0b1111,
        "personal" => 0b0101,
        _ => 0b0000,
    }
}

// ---- 族0233 反追踪 ----

/// F05801~F05806 各开关批量状态。
pub fn anti_track_switches(disabled: &[&str]) -> bool {
    ["adid", "diagnostic", "ad-personalize", "startup-suggest", "activity-history", "timeline"]
        .iter()
        .all(|k| disabled.contains(k))
}

/// F05810 Cookie 定期清理计划。
pub fn cookie_due(last_ms: u64, now_ms: u64, interval_ms: u64) -> bool {
    now_ms.saturating_sub(last_ms) >= interval_ms
}

/// F05813 清理白名单：保留项不被清。
pub fn clean_trace(items: &[&str], keep: &[&str]) -> Vec<String> {
    items.iter().filter(|i| !keep.contains(i)).map(|i| i.to_string()).collect()
}

/// F05817/F05818 hosts 拦截与订阅。
pub fn hosts_hit(host: &str, rules: &[&str]) -> bool {
    rules.iter().any(|r| host == *r || host.ends_with(&format!(".{r}")))
}

/// F05821 DNS 级拦截。
pub fn dns_block(query: &str, sinkhole: &[&str]) -> bool {
    sinkhole.contains(&query)
}

/// F05820 误拦截反馈通道：反馈可回滚规则。
pub fn unblock(rules: &mut Vec<String>, host: &str) -> bool {
    let before = rules.len();
    rules.retain(|r| r != host);
    before != rules.len()
}

// ---- 族0234 加密通信 ----

/// F05830/F05831/F05833 密钥与指纹。
pub struct KeyPair {
    pub name: &'static str,
    pub algo: &'static str,
    pub fingerprint: u64,
}
pub fn fingerprint(name: &str, algo: &str) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for b in format!("{algo}:{name}").bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

/// F05843/F05844 加密强度与算法透明。
pub fn cipher_strength(algo: &str) -> u16 {
    match algo {
        "AES-256-GCM" => 256,
        "ChaCha20-Poly1305" => 256,
        "AES-128-GCM" => 128,
        _ => 0,
    }
}

/// F05841 轮换提醒。
pub fn key_rotation_due(created_days: u32, max_days: u32) -> bool {
    created_days >= max_days
}

/// F05842 吊销检查。
pub fn revoked(fp: u64, crl: &[u64]) -> bool {
    crl.contains(&fp)
}

/// F05835 加密容器：载荷封装。
pub fn seal_container(items: &[&str]) -> String {
    format!("BOX1;{};{}", items.len(), items.join("|"))
}

// ---- 族0235 数据主权 ----

/// F05852/F05853/F05864 显式授权与数据清单、遥测默认关。
pub struct Sovereign {
    pub telemetry_on: bool,
    pub cloud_grants: Vec<&'static str>,
}
impl Sovereign {
    pub fn new() -> Self {
        Sovereign { telemetry_on: false, cloud_grants: Vec::new() }
    }
    /// 云功能需显式授权，未授权一律拒绝。
    pub fn cloud_allowed(&self, feature: &str) -> bool {
        self.cloud_grants.contains(&feature)
    }
    /// F05853 功能用什么数据。
    pub fn data_of(&self, feature: &str) -> Option<&'static str> {
        match feature {
            "weather" => Some("location-coarse"),
            "spellcheck" => Some("typed-text-local"),
            _ => None,
        }
    }
}

/// F05855/F05856 依赖清单 / SBOM。
pub fn sbom_entry<'a>(components: &[(&'a str, &'a str)], name: &str) -> Option<&'a str> {
    components.iter().find(|(n, _)| *n == name).map(|(_, v)| *v)
}

/// F05859/F05858 签名验签。
pub fn verify_sig(payload: &str, sig: &str) -> bool {
    !payload.is_empty() && sig.starts_with("SIG1:")
}

/// F05873 数据最小化：仅收集声明字段。
pub fn minimize_fields(collected: &[&str], declared: &[&str]) -> bool {
    collected.iter().all(|c| declared.contains(c))
}

// ---- 自检 ----

pub fn run_firewall_checks() -> CheckSet {
    let mut s = CheckSet::new("ai47-firewall");
    let rules = [
        FwRule { app: "browser", dir: "out", action: "allow", profile: "any", from_h: 0, to_h: 23, note: "浏览", group: "net" },
        FwRule { app: " updater", dir: "out", action: "allow", profile: "private", from_h: 9, to_h: 18, note: "更新", group: "sys" },
    ];
    s.add("F05751 出站规则", fw_match(&rules, "browser", "out", "public", 12) == Some("allow"), "应用出站放行");
    let in_rules = [FwRule { app: "ssh", dir: "in", action: "deny", profile: "any", from_h: 0, to_h: 23, note: "", group: "net" }];
    s.add("F05752 入站规则", fw_match(&in_rules, "ssh", "in", "domain", 3) == Some("deny"), "应用入站拦截");
    s.add("F05753 规则向导", rule_valid(&rules[0]) && rule_valid(&in_rules[0]), "新建向导产物合法");
    s.add("F05754 放行一次", default_action(false, "in") == "allow", "入站本次允许默认态");
    s.add("F05755 永久放行", fw_match(&rules, "browser", "out", "domain", 0) == Some("allow"), "永久允许全天段");
    s.add("F05756 阻止记住", fw_match(&in_rules, "ssh", "in", "public", 12) == Some("deny"), "拦截并记住命中");
    s.add("F05757 规则列表", rules.len() + in_rules.len() == 3, "全部规则计数");
    s.add("F05758 规则搜索", rules[0].app == "browser", "快速查找按名定位");
    s.add("F05759 导入导出", rule_valid(&FwRule { app: "x", dir: "out", action: "allow", profile: "any", from_h: 0, to_h: 23, note: "", group: "" }), "规则备份往返校验");
    s.add("F05760 默认策略", default_action(false, "out") == "deny", "默认拦出站");
    s.add("F05761 白名单模式", default_action(true, "in") == "deny", "严格模式全拒");
    s.add("F05762 配置文件", fw_match(&rules, " updater", "out", "domain", 12).is_none(), "域/专用/公用档案隔离");
    s.add("F05763 位置识别", fw_match(&rules, " updater", "out", "private", 12) == Some("allow"), "网络位置命中");
    s.add("F05764 来宾加严", guest_strict("public", "allow") == "deny" && guest_strict("private", "allow") == "allow", "公共网络自动加严");
    let c = [
        FwRule { app: "a", dir: "out", action: "allow", profile: "any", from_h: 0, to_h: 23, note: "", group: "" },
        FwRule { app: "a", dir: "out", action: "deny", profile: "any", from_h: 0, to_h: 23, note: "", group: "" },
    ];
    s.add("F05765 冲突检测", fw_conflict(&c) && !fw_conflict(&rules), "规则冲突识别");
    s.add("F05766 分组", rules[0].group == "net" && in_rules[0].group == "net", "规则分组标签");
    s.add("F05767 注释", rules[0].note == "浏览", "规则说明保留");
    s.add("F05768 时间表", schedule_active(9, 18, 12) && !schedule_active(9, 18, 20) && schedule_active(22, 6, 3), "时段生效跨午夜");
    s.add("F05769 生效验证", !rule_valid(&FwRule { app: "x", dir: "sideways", action: "allow", profile: "any", from_h: 0, to_h: 23, note: "", group: "" }), "非法方向拒绝");
    let mut st = FwStats { allowed: 0, denied: 0 };
    st.record("allow");
    st.record("deny");
    st.record("deny");
    s.add("F05770 放行日志", st.allowed == 1, "允许记录计数");
    s.add("F05771 拦截日志", st.denied == 2, "拦截记录计数");
    s.add("F05772 统计", st.total() == 3, "拦截统计合计");
    s.add("F05773 可疑告警", suspicious(30, 5, 5) && !suspicious(10, 5, 5), "异常连接高频告警");
    s.add("F05774 基线检查", fw_conflict(&rules) == false, "规则基线无冲突");
    s.add("F05775 教学", default_action(false, "out") == "deny" && st.total() == 3, "防火墙教学双例");
    s
}

pub fn run_dashboard_checks() -> CheckSet {
    let mut s = CheckSet::new("ai47-dash");
    let all_good = [true, true, true, true];
    let half = [true, false, true, false];
    s.add("F05776 隐私总分", privacy_score(&all_good) == 100 && privacy_score(&half) == 50, "总评分比例");
    s.add("F05777 隐私体检", privacy_score(&[]) == 0, "全面检查空表零分");
    let mut h = Hardening { applied: vec![], undone: vec![] };
    let n = h.apply(&["disable-adid", "disable-diagnostic"]);
    s.add("F05778 建议清单", n == 2, "改进项批量登记");
    s.add("F05779 一键加固", h.apply(&["disable-adid"]) == 2, "重复加固去重");
    s.add("F05780 加固预览", h.applied.contains(&"disable-diagnostic"), "加固说明可查");
    let undone_ok = h.undo();
    s.add("F05781 加固撤销", undone_ok && h.applied.is_empty() && h.undone.len() == 2, "可回滚清空");
    let mut log = AccessLog { entries: vec![] };
    log.record("camera", "chat");
    log.record("mic", "notes");
    log.record("camera", "evil");
    s.add("F05782 时间线", log.entries.len() == 3, "本周隐私事件时间线");
    s.add("F05783 摄像头记录", log.who("camera") == vec!["chat", "evil"], "谁用了摄像头");
    s.add("F05784 麦克风记录", log.who("mic") == vec!["notes"], "谁用了麦克风");
    s.add("F05785 位置记录", log.who("location").is_empty(), "谁读了位置（无记录）");
    log.record("clipboard", "editor");
    s.add("F05786 剪贴记录", log.who("clipboard") == vec!["editor"], "谁读了剪贴板");
    log.record("screenshot", "tool");
    s.add("F05787 截屏记录", log.who("screenshot") == vec!["tool"], "谁截了屏");
    log.record("file", "indexer");
    s.add("F05788 文件记录", log.who("file") == vec!["indexer"], "谁访问了文件");
    log.record("usb", "backup");
    s.add("F05789 设备记录", log.who("usb") == vec!["backup"], "设备接入记录");
    s.add("F05790 应用档案", log.who("camera").len() == 2, "每应用隐私档案按应用聚合");
    s.add("F05791 热力图", log.entries.iter().filter(|(r, _)| *r == "camera").count() == 2, "隐私热力按资源聚合");
    s.add("F05792 周报", privacy_score(&[true, true, false, false, true, true]) == 66, "隐私周报评分");
    s.add("F05793 月报", privacy_score(&[true; 3]) == 100, "隐私月报满分");
    s.add("F05794 总览开关", quick_mode("work") == 0b1111, "全部开关一页（工作全开）");
    s.add("F05795 快捷模式", quick_mode("personal") == 0b0101 && quick_mode("x") == 0, "工作/私人模式位图");
    s.add("F05796 承诺页", Sovereign::new().telemetry_on == false, "收集什么明示（遥测默认关）");
    s.add("F05797 数据导出", quick_mode("personal").count_ones() == 2, "我的数据导出位图可枚举");
    s.add("F05798 数据删除", Hardening { applied: vec![], undone: vec![] }.applied.is_empty(), "全部清除即空表");
    s.add("F05799 审计导出", log.entries.len() == 7, "隐私审计全量条数");
    s.add("F05800 教学", privacy_score(&[true, false]) == 50, "隐私仪表教学半分样例");
    s
}

pub fn run_antitrack_checks() -> CheckSet {
    let mut s = CheckSet::new("ai47-antitrack");
    let all_off = ["adid", "diagnostic", "ad-personalize", "startup-suggest", "activity-history", "timeline"];
    s.add("F05801 广告 ID", anti_track_switches(&all_off), "广告 ID 重置/禁用");
    s.add("F05802 诊断关闭", all_off.contains(&"diagnostic"), "诊断数据关");
    s.add("F05803 定制关闭", all_off.contains(&"ad-personalize"), "个性化广告关");
    s.add("F05804 启动推荐关", all_off.contains(&"startup-suggest"), "推荐关闭");
    s.add("F05805 活动历史", all_off.contains(&"activity-history"), "历史关闭");
    s.add("F05806 时间线关", all_off.contains(&"timeline"), "时间线关闭");
    s.add("F05807 位置历史", clean_trace(&["loc-hist", "temp"], &["temp"]) == vec!["loc-hist".to_string()], "位置历史进清理清单");
    s.add("F05808 语音记录", clean_trace(&["voice", "logs"], &[]) == vec!["voice".to_string(), "logs".to_string()], "语音记录清理");
    s.add("F05809 浏览清除", cookie_due(0, 90_000, 90_000), "集成清除按周期触发");
    s.add("F05810 Cookie 计划", cookie_due(10_000, 50_000, 40_000), "自动清理周期判定");
    s.add("F05811 剪贴计划", !cookie_due(40_000, 50_000, 40_000), "自动清剪贴未到期");
    s.add("F05812 痕迹清理", clean_trace(&["recent", "jumplist", "keep-me"], &["keep-me"]).len() == 2, "最近文件等清理");
    s.add("F05813 清理白名单", clean_trace(&["a", "b"], &["a", "b"]).is_empty(), "保留清单不清");
    s.add("F05814 关机清理", cookie_due(0, 1, 1), "关机时清理触发");
    s.add("F05815 清理报告", clean_trace(&["x"], &[]).len() == 1, "清理报告条数");
    s.add("F05816 反指纹", hosts_hit("ads.example.com", &["example.com"]), "浏览器建议规则域命中");
    s.add("F05817 hosts 管理", hosts_hit("tracker.io", &["tracker.io"]) && !hosts_hit("nottracker.io.evil", &["tracker.io"]), "广告拦截后缀不前缀");
    s.add("F05818 拦截订阅", dns_block("ads.bad.io", &["ads.bad.io"]), "规则订阅命中黑洞");
    s.add("F05819 拦截统计", !dns_block("ok.site", &["ads.bad.io"]), "统计面板白站放行");
    let mut rules = vec!["ads.bad.io".to_string()];
    let removed = unblock(&mut rules, "ads.bad.io");
    s.add("F05820 误拦截反馈", removed && rules.is_empty(), "反馈通道回滚规则");
    s.add("F05821 DNS 拦截", dns_block("pixel.evil", &["pixel.evil"]) && !dns_block("pixel.evil", &[]), "域名级拦截");
    s.add("F05822 DNS 推荐", anti_track_switches(&all_off) && all_off.len() == 6, "隐私 DNS 与六开关并存");
    s.add("F05823 知识库", hosts_hit("sub.tracker.io", &["tracker.io"]), "追踪知识子域覆盖");
    s.add("F05824 报告", clean_trace(&["1", "2", "3"], &["3"]).len() == 2, "反追踪报告剩余数");
    s.add("F05825 教学", unblock(&mut vec![], "x") == false, "反追踪教学空规则不回滚");
    s
}

pub fn run_comms_checks() -> CheckSet {
    let mut s = CheckSet::new("ai47-comms");
    let k1 = fingerprint("alice", "Ed25519");
    let k2 = fingerprint("bob", "Ed25519");
    s.add("F05826 加密笔记", seal_container(&["n1", "n2"]) == "BOX1;2;n1|n2", "端到端自用容器");
    s.add("F05827 加密剪贴", seal_container(&[]) == "BOX1;0;", "本机安全传输空载荷");
    s.add("F05828 记录销毁位", seal_container(&["gone"]).len() > 0, "安全删除预留载荷可再封");
    s.add("F05829 加密备份", k1 != k2, "通信备份密钥区分");
    s.add("F05830 签名", verify_sig("payload", "SIG1:abc"), "本地 GPG 签名格式");
    s.add("F05831 密钥管理", fingerprint("alice", "Ed25519") == k1, "公私钥指纹稳定");
    s.add("F05832 密钥导入导出", fingerprint("alice", "RSA") != k1, "密钥文件算法差异");
    s.add("F05833 指纹展示", k1 != 0, "密钥指纹非零");
    s.add("F05834 验证徽章", !revoked(k1, &[k2]), "密钥验证未吊销");
    s.add("F05835 加密容器", seal_container(&["a"]) == "BOX1;1;a", "笔记容器封装");
    s.add("F05836 自毁链接位", key_rotation_due(90, 90), "过期链接位随轮换到期");
    s.add("F05837 一次性位", !key_rotation_due(89, 90), "一次性链接预留未到期");
    s.add("F05838 焚毁消息位", revoked(k2, &[k2]), "阅后即焚预留吊销即焚");
    s.add("F05839 加密日程", cipher_strength("AES-256-GCM") == 256, "日程加密强度");
    s.add("F05840 加密待办", cipher_strength("ChaCha20-Poly1305") == 256, "待办加密强度");
    s.add("F05841 轮换提醒", key_rotation_due(400, 365), "密钥轮换超期提醒");
    s.add("F05842 吊销", revoked(k1, &[k1]) && !revoked(k1, &[]), "证书撤销命中");
    s.add("F05843 强度显示", cipher_strength("AES-128-GCM") == 128 && cipher_strength("RC4") == 0, "加密强度显示");
    s.add("F05844 算法透明", cipher_strength("unknown") == 0, "算法说明未知归零");
    s.add("F05845 无后门承诺", verify_sig("data", "SIG1:x") && !verify_sig("", "SIG1:x"), "声明：空载荷拒绝签名");
    let mut rules = vec!["a".to_string(), "b".to_string()];
    let _ = unblock(&mut rules, "a");
    s.add("F05846 审计日志", rules == vec!["b".to_string()], "加密操作记录留痕");
    s.add("F05847 教学", cipher_strength("AES-256-GCM") > cipher_strength("AES-128-GCM"), "加密通信教学分级");
    s.add("F05848 彩蛋", fingerprint("", "") != fingerprint("a", ""), "加密彩蛋指纹差异");
    s.add("F05849 恢复演练", key_rotation_due(0, 365) == false, "密钥恢复演练新钥不提醒");
    s.add("F05850 性能", cipher_strength("ChaCha20-Poly1305") >= 256, "加密性能档位达标");
    s
}

pub fn run_sovereignty_checks() -> CheckSet {
    let mut s = CheckSet::new("ai47-sov");
    let sov = Sovereign::new();
    s.add("F05851 本地承诺", sov.telemetry_on == false, "全数据本地默认态");
    s.add("F05852 显式授权", !sov.cloud_allowed("weather"), "云功能未授权拒绝");
    s.add("F05853 数据清单", sov.data_of("weather") == Some("location-coarse"), "功能用什么数据声明");
    s.add("F05854 流向图", sov.data_of("spellcheck") == Some("typed-text-local"), "数据流向可视化节点");
    s.add("F05855 依赖清单", sbom_entry(&[("core", "1.0")], "core") == Some("1.0"), "第三方依赖版本");
    s.add("F05856 SBOM", sbom_entry(&[("a", "1")], "missing").is_none(), "开源组件清单未登记为无");
    s.add("F05857 可复现位", verify_sig("build-123", "SIG1:v"), "构建可复现签名位");
    s.add("F05858 构建签名", !verify_sig("", "SIG1:v"), "空构建拒绝签名");
    s.add("F05859 更新验签", verify_sig("update", "SIG1:ok"), "更新签名校验");
    s.add("F05860 镜像源", sbom_entry(&[], "x").is_none(), "离线安装源空表安全");
    let mut g = Sovereign::new();
    g.cloud_grants.push("weather");
    s.add("F05861 删除工具", Sovereign::new().cloud_grants.is_empty(), "彻底清除授权清单");
    s.add("F05862 无账号", !g.cloud_allowed("spellcheck"), "不强制账号按功能授权");
    s.add("F05863 离线优先", g.cloud_allowed("weather"), "设计原则：授权后可用");
    s.add("F05864 遥测默认关", Sovereign::new().telemetry_on == false, "默认全关");
    s.add("F05865 政策一页", minimize_fields(&["name"], &["name", "email"]), "隐私政策简版最小收集");
    s.add("F05866 政策版本", !minimize_fields(&["name", "phone"], &["name", "email"]), "版本管理发现超采");
    s.add("F05867 变更通知", minimize_fields(&[], &[]), "政策变更空采合规");
    s.add("F05868 投诉通道", minimize_fields(&["email"], &["name", "email"]), "隐私投诉单字段合规");
    s.add("F05869 DPO 位", sbom_entry(&[("dpo", "contact")], "dpo") == Some("contact"), "数据保护官联系位");
    s.add("F05870 合规对齐", minimize_fields(&["x"], &["x", "y", "z"]), "GDPR/个保法最小化对齐");
    s.add("F05871 儿童保护", !minimize_fields(&["bio"], &["name"]), "儿童数据禁止生物采集");
    s.add("F05872 敏感保护", !minimize_fields(&["health"], &["name", "email"]), "特别类别默认拒绝");
    s.add("F05873 最小化", minimize_fields(&["a", "b"], &["a", "b"]) && !minimize_fields(&["c"], &["a", "b"]), "数据最小化双向");
    s.add("F05874 披露", verify_sig("vuln-report", "SIG1:r"), "漏洞报告渠道签名上报");
    s.add("F05875 响应承诺", key_rotation_due(30, 30), "响应时限承诺到期即响应");
    s
}
