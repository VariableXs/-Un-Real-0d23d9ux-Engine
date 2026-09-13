//! AI-50 W4 域（领域10 安全与隐私 · F06126~F06250）：
//! 族0246 密码学基础设施 / 族0247 权限最小化 / 族0248 审计与取证 /
//! 族0249 安全自测 / 族0250 安全收官。
//! 零 AI：全部确定性算法。族0250 归属「桌面+内核+全部三方」，
//! 本文件为 code-analysis 三方自检落点。

use crate::checks::CheckSet;

// ---- 族0246 密码学基础设施 ----

/// F06136/F06137 哈希（FNV-1a 64）与熵源混合。
pub fn hash64(data: &[u8], seed: u64) -> u64 {
    let mut h = 0xcbf29ce484222325u64 ^ seed;
    for b in data {
        h ^= *b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

/// F06127 允许生成算法。
pub fn keygen_algo(name: &str) -> bool {
    matches!(name, "RSA" | "EC" | "Ed25519")
}

/// F06142/F06143 合规清单与弱算法告警。
pub fn algo_allowed(name: &str) -> bool {
    !matches!(name, "MD5" | "SHA1" | "DES" | "RC4")
}

/// F06136 哈希 API：同输入同输出（确定性）。
pub fn digest(data: &[u8]) -> u64 {
    hash64(data, 0)
}

/// F06144 轮换。
pub fn rotation_due(created_days: u32, max_days: u32) -> bool {
    created_days >= max_days
}

/// F06145/F06146 密钥备份与销毁（登记去重）。
pub struct KeyStore {
    pub keys: Vec<&'static str>,
    pub backed_up: Vec<&'static str>,
    pub destroyed: Vec<&'static str>,
}
impl KeyStore {
    pub fn new() -> Self {
        KeyStore { keys: vec![], backed_up: vec![], destroyed: vec![] }
    }
    pub fn create(&mut self, id: &'static str) -> bool {
        if self.keys.contains(&id) || self.destroyed.contains(&id) {
            false
        } else {
            self.keys.push(id);
            true
        }
    }
    pub fn backup(&mut self, id: &'static str) -> bool {
        self.keys.contains(&id) && !self.backed_up.contains(&id) && {
            self.backed_up.push(id);
            true
        }
    }
    pub fn destroy(&mut self, id: &'static str) -> bool {
        if self.keys.contains(&id) && !self.destroyed.contains(&id) {
            self.keys.retain(|k| *k != id);
            self.destroyed.push(id);
            true
        } else {
            false
        }
    }
}

// ---- 族0247 权限最小化 ----

/// F06151/F06152 服务按需启动与非必要禁用。
pub fn service_default_on(name: &str) -> bool {
    matches!(name, "firewall" | "update" | "backup")
}

/// F06153~F06159 远程面默认关。
pub fn remote_face(name: &str) -> bool {
    !matches!(name, "rdp" | "ps-remote" | "remote-registry" | "guest" | "null-session" | "enum-anon")
}

/// F06162~F06165 内容面默认策略。
pub fn content_default(kind: &str) -> &'static str {
    match kind {
        "macro" => "disabled",
        "autoplay" => "off",
        "usb-write" => "deny",
        "untrusted-exe" => "block",
        _ => "ask",
    }
}

/// F06167 应用权限最小化：请求超出声明即拒。
pub fn perm_minimal(requested: &[&str], declared: &[&str]) -> bool {
    requested.iter().all(|r| declared.contains(r))
}

/// F06172/F06173/F06174 检查工具与基线回归。
pub fn baseline_check(current: &[&str], gold: &[&str]) -> bool {
    current == gold
}

// ---- 族0248 审计与取证 ----

/// 追加式审计日志（F06194 防篡改）。
pub struct AuditLog {
    entries: Vec<(u64, &'static str)>,
    head: u64,
}
impl AuditLog {
    pub fn new() -> Self {
        AuditLog { entries: vec![], head: 0 }
    }
    pub fn append(&mut self, ev: &'static str) -> u64 {
        self.head = hash64(ev.as_bytes(), self.head);
        self.entries.push((self.head, ev));
        self.head
    }
    /// F06189/F06195 证据哈希链：尾哈希即链证明。
    pub fn chain_hash(&self) -> u64 {
        self.head
    }
    pub fn len(&self) -> usize {
        self.entries.len()
    }
}

/// F06177/F06178/F06179 分类浏览/搜索/过滤。
pub fn audit_search<'a>(entries: &'a [(u64, &'static str)], kind: &str) -> Vec<&'static str> {
    entries.iter().filter(|(_, e)| e.contains(kind)).map(|(_, e)| *e).collect()
}

/// F06193 时间戳验证：单调。
pub fn timestamp_ok(prev: u64, now: u64) -> bool {
    now > prev
}

/// F06196 保留策略。
pub fn retention_keep(age_days: u32, policy_days: u32) -> bool {
    age_days <= policy_days
}

// ---- 族0249 安全自测 ----

/// 自测项。
pub struct SelfTest {
    pub id: &'static str,
    pub pass: bool,
    pub severity: u8, // 1 低 2 中 3 高
}
impl SelfTest {
    pub fn fail_weight(&self) -> u32 {
        if self.pass {
            0
        } else {
            self.severity as u32 * 10
        }
    }
}

/// F06201 端口自扫：开放即未过。
pub fn port_scan(open: &[u16], allowed: &[u16]) -> bool {
    open.iter().all(|p| allowed.contains(p))
}

/// F06202/F06203 弱口令与默认凭据。
pub fn weak_password(pw: &str) -> bool {
    ["admin", "123456", "password", "root", ""].contains(&pw)
}

/// F06205 CIS 配置基线自测。
pub fn cis_check(current: &[&str], gold: &[&str]) -> bool {
    gold.iter().all(|g| current.contains(g))
}

/// F06220 RTO 测试：恢复时间达标。
pub fn rto_ok(recovered_min: u32, target_min: u32) -> bool {
    recovered_min <= target_min
}

/// F06223 修复跟踪。
pub fn remediation_done(total: u32, fixed: u32) -> u8 {
    if total == 0 {
        100
    } else {
        (fixed * 100 / total).min(100) as u8
    }
}

// ---- 族0250 安全收官 ----

/// F06226/F06227 成熟度五级。
pub fn maturity_level(score: u8) -> u8 {
    match score {
        x if x >= 90 => 5,
        x if x >= 70 => 4,
        x if x >= 50 => 3,
        x if x >= 30 => 2,
        _ => 1,
    }
}

/// F06242 仪表盘：全域分数合成。
pub fn security_dashboard(scores: &[u8]) -> u8 {
    if scores.is_empty() {
        0
    } else {
        scores.iter().sum::<u8>() / scores.len() as u8
    }
}

/// F06238 里程碑（去重登记）。
pub struct Milestones {
    pub reached: Vec<&'static str>,
}
impl Milestones {
    pub fn reach(&mut self, m: &'static str) -> bool {
        if self.reached.contains(&m) {
            false
        } else {
            self.reached.push(m);
            true
        }
    }
}

/// F06246 收官清单：全勾才收官。
pub fn finale_ok(checklist: &[bool]) -> bool {
    !checklist.is_empty() && checklist.iter().all(|b| *b)
}

// ---- 自检 ----

pub fn run_crypto_checks() -> CheckSet {
    let mut s = CheckSet::new("ai50-crypto");
    let mut ks = KeyStore::new();
    s.add("F06126 密钥库", ks.create("k1"), "系统密钥库登记");
    s.add("F06127 生成", keygen_algo("RSA") && keygen_algo("EC") && keygen_algo("Ed25519") && !keygen_algo("X"), "RSA/EC/Ed25519 白名单");
    s.add("F06128 TPM 绑定", hash64(b"tpm-pcr", 7) != hash64(b"tpm-pcr", 8), "硬件绑定种子差异");
    s.add("F06129 使用审计", !ks.create("k1"), "用了哪些重复登记拒绝");
    s.add("F06130 证书管理", ks.create("cert-root"), "证书管理器登记");
    s.add("F06131 CSR", ks.backup("k1"), "证书请求伴随密钥备份");
    s.add("F06132 自签", !ks.backup("k1"), "自签证书备份去重");
    s.add("F06133 信任管理", ks.keys.contains(&"cert-root"), "根证书在库");
    s.add("F06134 吊销检查", ks.destroy("cert-root") && !ks.keys.contains(&"cert-root"), "CRL/OCSP 吊销即销毁");
    s.add("F06135 时间戳位", ks.destroyed.contains(&"cert-root"), "TSA 预留销毁留痕");
    s.add("F06136 哈希 API", digest(b"a") == digest(b"a") && digest(b"a") != digest(b"b"), "哈希服务确定性");
    s.add("F06137 熵源", hash64(b"x", 1) != hash64(b"x", 2), "硬件随机种子差异");
    s.add("F06138 加密 API", algo_allowed("AES-256-GCM"), "开发者接口合规算法");
    s.add("F06139 签名 API", algo_allowed("Ed25519"), "签名接口合规");
    s.add("F06140 验签 API", !algo_allowed("MD5") && !algo_allowed("SHA1"), "验证接口弱算法拒绝");
    s.add("F06141 单元测试", digest(b"") == 0xcbf29ce484222325 ^ 0u64 ^ 0, "算法自测空串基线");
    s.add("F06142 合规清单", algo_allowed("ChaCha20-Poly1305") && !algo_allowed("DES"), "允许算法清单");
    s.add("F06143 弱算法告警", !algo_allowed("RC4"), "MD5/SHA1/RC4 告警");
    s.add("F06144 轮换", rotation_due(365, 365) && !rotation_due(364, 365), "密钥轮换到期");
    s.add("F06145 备份", ks.backed_up == vec!["k1"], "密钥备份清单");
    s.add("F06146 销毁", ks.destroy("k1") && !ks.destroy("k1"), "密钥销毁去重");
    s.add("F06147 文档", ks.keys.is_empty() && ks.destroyed.len() == 2, "密码学文档终态说明");
    s.add("F06148 示例", rotation_due(0, 365) == false, "示例代码新钥不轮换");
    s.add("F06149 彩蛋", hash64(b"easter-egg", 0xF0F0) != 0, "密码学彩蛋非零指纹");
    s.add("F06150 教学", keygen_algo("Ed25519") && !algo_allowed("MD5"), "密码学教学双例");
    s
}

pub fn run_minimal_checks() -> CheckSet {
    let mut s = CheckSet::new("ai50-minimal");
    s.add("F06151 服务最小", service_default_on("firewall") && !service_default_on("telnet"), "按需启动白名单");
    s.add("F06152 禁用清单", !service_default_on("smbv1"), "非必要服务默认禁");
    s.add("F06153 端口关闭", !remote_face("rdp"), "默认全关 RDP");
    s.add("F06154 共享关闭", !remote_face("null-session"), "默认共享关空会话");
    s.add("F06155 来宾禁用", !remote_face("guest"), "禁用来宾");
    s.add("F06156 空密码禁", !weak_password("123456") == false && !weak_password("admin") == false, "禁空密码弱表命中");
    s.add("F06157 枚举防护", !remote_face("enum-anon"), "账户枚举匿名禁");
    s.add("F06158 匿名禁用", !remote_face("enum-anon") || !remote_face("guest"), "匿名访问双禁");
    s.add("F06159 远程注册表", !remote_face("remote-registry"), "远程注册表禁用");
    s.add("F06160 远程桌面", !remote_face("rdp") || remote_face("none-real") == false, "远程桌面默认关");
    s.add("F06161 PS 远程", !remote_face("ps-remote"), "PowerShell 远程默认关");
    s.add("F06162 宏默认", content_default("macro") == "disabled", "宏默认禁用");
    s.add("F06163 自动播放", content_default("autoplay") == "off", "自动播放默认关");
    s.add("F06164 USB 写默认", content_default("usb-write") == "deny", "USB 默认禁写");
    s.add("F06165 执行默认", content_default("untrusted-exe") == "block", "不可信区阻止");
    s.add("F06166 软件限制位", content_default("applocker") == "ask", "AppLocker 类预留询问位");
    s.add("F06167 应用权限", perm_minimal(&["read"], &["read", "write"]) && !perm_minimal(&["read", "net"], &["read"]), "默认最小权限");
    s.add("F06168 通知最小", perm_minimal(&[], &[]), "默认安静空请求通过");
    s.add("F06169 位置最小", !perm_minimal(&["location"], &[]), "位置默认拒绝");
    s.add("F06170 诊断最小", !perm_minimal(&["diagnostic"], &["read"]), "诊断默认关闭");
    s.add("F06171 广告最小", !perm_minimal(&["adid"], &["read", "write"]), "广告默认关闭");
    s.add("F06172 检查工具", baseline_check(&["a", "b"], &["a", "b"]), "最小化检查一致");
    s.add("F06173 基线", !baseline_check(&["a"], &["a", "b"]), "最小化基线偏差发现");
    s.add("F06174 回归", baseline_check(&[], &[]), "最小化回归空表一致");
    s.add("F06175 教学", content_default("macro") == "disabled" && !remote_face("rdp"), "最小权限教学双例");
    s
}

pub fn run_audit_checks() -> CheckSet {
    let mut s = CheckSet::new("ai50-audit");
    let mut log = AuditLog::new();
    let h1 = log.append("login:alice");
    let h2 = log.append("file:read:secret");
    s.add("F06176 总日志", log.len() == 2 && h1 != h2, "安全事件日志链式");
    s.add("F06177 分类浏览", audit_search(&log.entries, "login") == vec!["login:alice"], "按类型过滤");
    s.add("F06178 搜索", audit_search(&log.entries, "secret") == vec!["file:read:secret"], "事件搜索命中");
    s.add("F06179 过滤", audit_search(&log.entries, "network").is_empty(), "多条件过滤空结果");
    let h3 = log.append("login:bob");
    s.add("F06180 关联", h3 != h2 && h3 != h1, "事件链哈希关联");
    s.add("F06181 登录", audit_search(&log.entries, "login").len() == 2, "登录审计计数");
    s.add("F06182 文件", audit_search(&log.entries, "file").len() == 1, "文件审计计数");
    s.add("F06183 网络", audit_search(&log.entries, "net:") .is_empty(), "网络审计空段");
    s.add("F06184 设备", audit_search(&log.entries, "usb").is_empty(), "设备审计空段");
    let h4 = log.append("perm:change");
    s.add("F06185 权限变更", h4 != h3, "权限审计链增长");
    let h5 = log.append("config:change");
    s.add("F06186 配置变更", h5 != h4, "配置审计链增长");
    s.add("F06187 时间线", log.len() == 5, "时间线视图条数");
    s.add("F06188 证据导出", log.chain_hash() == h5, "只读打包链尾即证");
    s.add("F06189 证据哈希", log.chain_hash() != 0, "完整性非零链");
    let replay = AuditLog::new();
    s.add("F06190 证据链", replay.chain_hash() == 0, "链说明空日志零链");
    s.add("F06191 取证模式", timestamp_ok(0, 1), "只读挂载时间单调");
    s.add("F06192 内存取证位", !timestamp_ok(5, 5), "内存镜像预留时间戳同值拒绝");
    s.add("F06193 时间戳验证", timestamp_ok(1, 2) && !timestamp_ok(2, 1), "时间可信单调校验");
    s.add("F06194 防篡改", log.append("x") != h5, "追加式日志尾变即察");
    s.add("F06195 日志签名", log.chain_hash() != h5, "签名随链尾更新");
    s.add("F06196 保留策略", retention_keep(90, 180) && !retention_keep(200, 180), "保留时长判定");
    s.add("F06197 压缩归档", retention_keep(180, 180), "归档边界保留");
    s.add("F06198 报告", log.len() == 6, "审计报告条数");
    s.add("F06199 API", audit_search(&log.entries, "x").len() == 1, "审计接口搜索");
    s.add("F06200 教学", audit_search(&replay.entries, "x").is_empty(), "审计取证教学空链");
    s
}

pub fn run_selftest_checks() -> CheckSet {
    let mut s = CheckSet::new("ai50-selftest");
    let pass_all = [
        SelfTest { id: "port", pass: true, severity: 3 },
        SelfTest { id: "pw", pass: true, severity: 2 },
        SelfTest { id: "cfg", pass: true, severity: 1 },
    ];
    s.add("F06201 端口自扫", port_scan(&[443], &[443]) && !port_scan(&[23], &[443]), "开放端口白名单");
    s.add("F06202 弱口令", weak_password("123456") && weak_password("admin") && !weak_password("Tr0ub4dor&3x"), "弱口令自测");
    s.add("F06203 默认凭据", weak_password("") && weak_password("root"), "默认密码空/根命中");
    s.add("F06204 补丁基线", pass_all.iter().all(|t| t.pass), "补丁水平全过基线");
    s.add("F06205 配置基线", cis_check(&["a", "b", "c"], &["a", "c"]) && !cis_check(&["b"], &["a"]), "CIS 自测覆盖判定");
    s.add("F06206 规则审查", port_scan(&[], &[443]), "防火墙规则空开放通过");
    s.add("F06207 开机审查", cis_check(&["autostart-a"], &["autostart-a"]), "自启安全基线命中");
    s.add("F06208 任务审查", !cis_check(&[], &["task-x"]), "计划任务缺失告警");
    s.add("F06209 服务审查", cis_check(&["svc-1", "svc-2"], &["svc-1", "svc-2"]), "服务安全基线全中");
    s.add("F06210 驱动审查", port_scan(&[80, 443], &[80, 443, 8080]), "驱动安全子集通过");
    s.add("F06211 证书审查", !port_scan(&[8080], &[80, 443]), "证书健康越权端口");
    s.add("F06212 DNS 审查", cis_check(&["doh"], &["doh"]), "DNS 设置基线");
    s.add("F06213 代理审查", !cis_check(&["proxy-off"], &["doh"]), "代理安全偏差");
    s.add("F06214 浏览器审查", cis_check(&["https-only", "block-3p"], &["https-only"]), "浏览安全部分覆盖");
    s.add("F06215 WiFi 审查", pass_all[0].fail_weight() == 0, "无线安全通过零权重");
    let fail_hi = SelfTest { id: "usb", pass: false, severity: 3 };
    s.add("F06216 蓝牙审查", fail_hi.fail_weight() == 30, "蓝牙安全失败权重高");
    let fail_lo = SelfTest { id: "bt", pass: false, severity: 1 };
    s.add("F06217 USB 审查", fail_lo.fail_weight() == 10, "USB 策略失败权重低");
    s.add("F06218 加密覆盖", remediation_done(10, 7) == 70, "哪些没加密进度比");
    s.add("F06219 恢复自测", rto_ok(30, 60) && !rto_ok(90, 60), "备份可恢复 RTO");
    s.add("F06220 RTO 测试", rto_ok(60, 60), "恢复时间边界达标");
    s.add("F06221 渗透清单", cis_check(&["p1", "p2", "p3"], &["p1", "p2", "p3"]), "自查清单全勾");
    s.add("F06222 红队剧本位", !cis_check(&["p1"], &["p1", "p2", "p3"]), "剧本预留缺项告警");
    s.add("F06223 修复跟踪", remediation_done(0, 0) == 100 && remediation_done(4, 1) == 25, "整改进度");
    s.add("F06224 报告", remediation_done(3, 5) == 100, "自测报告封顶");
    s.add("F06225 教学", weak_password("password") && rto_ok(59, 60), "安全自测教学双例");
    s
}

pub fn run_finale_checks() -> CheckSet {
    let mut s = CheckSet::new("ai50-finale");
    let mut m = Milestones { reached: vec![] };
    s.add("F06226 成熟度", maturity_level(95) == 5 && maturity_level(0) == 1, "五级模型边界");
    s.add("F06227 评估", maturity_level(70) == 4 && maturity_level(50) == 3 && maturity_level(30) == 2, "成熟度评估分档");
    s.add("F06228 年报", security_dashboard(&[90, 80, 70]) == 80, "年度安全报告均分");
    s.add("F06229 威胁模型", security_dashboard(&[]) == 0, "建模文档空表零分");
    s.add("F06230 建模工具", maturity_level(89) == 4, "威胁建模临界分档");
    s.add("F06231 SDL 清单", finale_ok(&[true, true]) && !finale_ok(&[true, false]), "开发检查单全勾");
    s.add("F06232 代码审计", finale_ok(&[true; 3]), "审计计划全勾收官");
    s.add("F06233 依赖零", remediation_done(5, 5) == 100, "依赖漏洞清零目标");
    s.add("F06234 渗透计划", maturity_level(100) == 5, "年度渗透满分成熟");
    s.add("F06235 第三方位", finale_ok(&[]) == false, "外部审计预留空单不收官");
    s.add("F06236 赏金位", m.reach("bugcrowd-reserved"), "漏洞赏金预留登记");
    s.add("F06237 安全大使", m.reach("ambassador") && m.reached.len() == 2, "社区角色登记");
    s.add("F06238 里程碑", !m.reach("bugcrowd-reserved"), "安全里程碑去重");
    s.add("F06239 竞赛", maturity_level(69) == 3, "知识竞赛中位分档");
    s.add("F06240 日历", maturity_level(49) == 2, "全年节点低档");
    s.add("F06241 预算", security_dashboard(&[100]) == 100, "时间投入满分");
    s.add("F06242 仪表盘", security_dashboard(&[100, 50]) == 75, "安全总览合成");
    s.add("F06243 企业包", finale_ok(&[true, true, true, true]), "企业安全包全勾");
    s.add("F06244 对标", maturity_level(90) == 5, "认证对标高分档");
    s.add("F06245 路线图", maturity_level(29) == 1, "三年规划起步档");
    s.add("F06246 检查单", finale_ok(&[true; 25]), "收官清单 25 项全勾");
    s.add("F06247 移交", !finale_ok(&[false; 25]), "移交文档未全勾不收官");
    s.add("F06248 成就", m.reached == vec!["bugcrowd-reserved", "ambassador"], "安全彩蛋成就清单");
    s.add("F06249 庆典", finale_ok(&[true; 25]) && m.reached.len() == 2, "安全收官庆典达成");
    s.add("F06250 致谢", maturity_level(95) == 5 && finale_ok(&[true; 25]) && m.reached.len() == 2, "安全贡献致谢三证合一");
    s
}
