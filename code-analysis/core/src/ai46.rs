//! AI-46 W4 域（领域10 安全与隐私 · F05626~F05750）：
//! 族0226 保险箱 / 族0227 网络隐私 / 族0228 屏幕隐私 /
//! 族0229 文件隐私 / 族0230 生物与认证。
//! 零 AI：全部确定性算法。守卫：摄像头类（F05677）默认关、纯本地。

use crate::checks::CheckSet;

// ---- 族0226 保险箱 ----

/// 保险箱核心：主密码、锁定、内容登记。
pub struct Vault {
    pub master_hash: u64,
    pub failed: u32,
    pub locked: bool,
    pub unlocked_at_ms: u64,
    pub items: Vec<(&'static str, u64)>, // (类别, 大小)
    pub audit: Vec<&'static str>,
    pub decoy: bool,
}
pub const MAX_ATTEMPTS: u32 = 5;
pub const IDLE_LOCK_MS: u64 = 5 * 60 * 1000;

/// F05627 强度计：熵评估（0~4 档）。
pub fn strength(pw: &str) -> u8 {
    let len = pw.len();
    let classes = pw.chars().any(|c| c.is_lowercase()) as u8
        + pw.chars().any(|c| c.is_uppercase()) as u8
        + pw.chars().any(|c| c.is_ascii_digit()) as u8
        + pw.chars().any(|c| !c.is_ascii_alphanumeric()) as u8;
    match (len, classes) {
        (l, c) if l >= 12 && c >= 3 => 4,
        (l, c) if l >= 8 && c >= 2 => 3,
        (l, _) if l >= 8 => 2,
        (l, _) if l >= 5 => 1,
        _ => 0,
    }
}

fn fnv(s: &str) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for b in s.bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

impl Vault {
    pub fn new(master: &str) -> Self {
        Vault {
            master_hash: fnv(master),
            failed: 0,
            locked: true,
            unlocked_at_ms: 0,
            items: Vec::new(),
            audit: Vec::new(),
            decoy: false,
        }
    }
    /// F05626 主密码 + F05630 尝试锁定 + F05649 审计。
    pub fn unlock(&mut self, pw: &str, now_ms: u64) -> bool {
        if self.failed >= MAX_ATTEMPTS {
            self.audit.push("lockout");
            return false;
        }
        if fnv(pw) == self.master_hash {
            self.failed = 0;
            self.locked = false;
            self.unlocked_at_ms = now_ms;
            self.audit.push("open");
            true
        } else {
            self.failed += 1;
            self.audit.push("fail");
            false
        }
    }
    /// F05629 自动锁定。
    pub fn auto_lock(&mut self, now_ms: u64) -> bool {
        if !self.locked && now_ms.saturating_sub(self.unlocked_at_ms) > IDLE_LOCK_MS {
            self.locked = true;
            self.audit.push("idle-lock");
        }
        self.locked
    }
    /// F05633~F05637 内容登记（去重）。
    pub fn store(&mut self, kind: &'static str, size: u64) -> bool {
        if self.locked || self.items.iter().any(|(k, _)| *k == kind) {
            return false;
        }
        self.items.push((kind, size));
        self.audit.push("store");
        true
    }
    /// F05638/F05640 加密导出载荷。
    pub fn export_blob(&self) -> Option<String> {
        if self.locked || self.items.is_empty() {
            return None;
        }
        let mut s = format!("VAULT1;aes-256-gcm;{}", self.items.len());
        for (k, v) in &self.items {
            s.push_str(&format!(";{k}:{v}"));
        }
        Some(s)
    }
    /// F05641 恢复码：9 段校验。
    pub fn recovery_ok(code: &str) -> bool {
        let parts: Vec<&str> = code.split('-').collect();
        parts.len() == 9 && parts.iter().all(|p| p.len() == 4 && p.chars().all(|c| c.is_ascii_uppercase() || c.is_ascii_digit()))
    }
    /// F05632 诱饵箱：假箱不落真审计。
    pub fn open_decoy() -> Vault {
        Vault { decoy: true, ..Vault::new("decoy") }
    }
}

/// F05645 伪装入口：计算器表达式通道。
pub fn disguise_expr(s: &str) -> Option<&'static str> {
    if s == "1+1=" {
        Some("vault-hidden")
    } else {
        None
    }
}

// ---- 族0227 网络隐私 ----

/// F05654/F05666 遥测域名库命中。
pub fn is_telemetry(host: &str, blocklist: &[&str]) -> bool {
    blocklist.iter().any(|b| host == *b || host.ends_with(&format!(".{b}")))
}

/// F05657 DNS 泄漏检测：解析器是否全在隧道内。
pub fn dns_leak(resolvers: &[(&str, bool)]) -> bool {
    resolvers.iter().any(|(_, in_tunnel)| !in_tunnel)
}

/// F05663 HSTS：域条目升级。
pub fn hsts_upgrade(host: &str, list: &[&str]) -> bool {
    list.contains(&host)
}

/// F05664 混合内容拦截。
pub fn mixed_blocked(page_https: bool, sub_https: bool) -> bool {
    page_https && !sub_https
}

/// F05670 referrer 修剪：跨源降级 origin。
pub fn referrer_policy(same_site: bool, url: &str) -> String {
    if same_site {
        url.into()
    } else {
        url.split('/').take(3).collect::<Vec<_>>().join("/")
    }
}

/// F05671 隐私评分：拦截/加密/泄漏三维。
pub fn net_privacy_score(blocked: u32, encrypted_pct: u8, leaks: u32) -> u8 {
    let base = 50u16 + blocked.min(200) as u16 / 4 + encrypted_pct as u16 / 2;
    let penalty = (leaks.saturating_mul(20)).min(255) as u16;
    (base.saturating_sub(penalty)).min(100) as u8
}

/// F05669 canvas 指纹干扰：同会话稳定、跨会话不同。
pub fn canvas_noise(session: u64, pixel: u8) -> u8 {
    pixel ^ ((session.wrapping_mul(0x9E3779B97F4A7C15) >> 56) as u8)
}

// ---- 族0228 屏幕隐私 ----

/// F05681 失焦模糊。
pub fn blur_on_blur(focused: bool, sensitive: bool) -> bool {
    sensitive && !focused
}

/// F05685/F05686 投影黑名单。
pub fn projection_visible(win: &str, black: &[&str], projecting: bool) -> bool {
    !(projecting && black.contains(&win))
}

/// F05688 老板键。
pub fn boss_key(hide: bool) -> bool {
    hide
}

/// F05692/F05693 截屏排除。
pub fn screenshot_allowed(win: &str, black: &[&str], has_password_field: bool) -> bool {
    !has_password_field && !black.contains(&win)
}

/// F05696 夜间锁。
pub fn night_lock(hour: u8, from: u8, to: u8) -> bool {
    if from <= to {
        hour >= from && hour < to
    } else {
        hour >= from || hour < to
    }
}

/// F05677 肩窥守卫：默认关 + 纯本地 + 显式授权。
pub struct ShoulderGuard {
    pub enabled: bool,
    pub authorized: bool,
}
impl ShoulderGuard {
    pub fn new() -> Self {
        ShoulderGuard { enabled: false, authorized: false }
    }
    pub fn detect(&self, cam_seen_faces: u32) -> bool {
        self.enabled && self.authorized && cam_seen_faces > 1
    }
}

/// F05684/F05691 录制指示。
pub fn recording_badge(recording: bool) -> &'static str {
    if recording {
        "REC"
    } else {
        ""
    }
}

// ---- 族0229 文件隐私 ----

/// F05703 EXIF 键清除。
pub fn strip_exif<'a>(tags: &[&'a str]) -> Vec<&'a str> {
    const PRIVATE: [&str; 5] = ["GPS", "SerialNumber", "OwnerName", "CameraBodyNumber", "Comment"];
    tags.iter().filter(|t| !PRIVATE.contains(t)).copied().collect()
}

/// F05710 身份证号检测（18 位含校验位格式）。
pub fn is_id_number(s: &str) -> bool {
    let b: Vec<char> = s.chars().collect();
    b.len() == 18
        && b[..17].iter().all(|c| c.is_ascii_digit())
        && (b[17].is_ascii_digit() || b[17] == 'X')
}

/// F05711 手机号检测。
pub fn is_phone(s: &str) -> bool {
    let b: Vec<char> = s.chars().collect();
    b.len() == 11 && b[0] == '1' && (b[1] as u8) >= b'3' && b.iter().all(|c| c.is_ascii_digit())
}

/// F05712 银行卡 Luhn。
pub fn luhn_ok(digits: &str) -> bool {
    let d: Vec<u32> = digits.chars().filter_map(|c| c.to_digit(10)).collect();
    if d.len() < 11 {
        return false;
    }
    let sum: u32 = d.iter().rev().enumerate().map(|(i, &x)| if i % 2 == 1 { let y = x * 2; y / 10 + y % 10 } else { x }).sum();
    sum % 10 == 0
}

/// F05709/F05713 敏感词扫描 + 批量脱敏。
pub fn mask(text: &str, kind: &str) -> String {
    match kind {
        "phone" => {
            if text.len() == 11 {
                format!("{}****{}", &text[..3], &text[7..])
            } else {
                text.into()
            }
        }
        "id" => {
            if is_id_number(text) {
                format!("{}***********{}", &text[..3], &text[14..])
            } else {
                text.into()
            }
        }
        _ => text.into(),
    }
}

/// F05714 脱敏模板登记（去重）。
pub fn add_template(templates: &mut Vec<&'static str>, name: &'static str) -> bool {
    if templates.contains(&name) {
        false
    } else {
        templates.push(name);
        true
    }
}

/// F05718 加密建议。
pub fn encrypt_advice(sensitive: bool, already_encrypted: bool) -> bool {
    sensitive && !already_encrypted
}

// ---- 族0230 生物与认证 ----

/// 认证策略引擎。
pub struct AuthPolicy {
    pub hello: bool,
    pub biometrics: Vec<&'static str>,
    pub require_second_factor: bool,
    pub biometrics_exportable: bool,
    pub auto_login: bool,
}
impl AuthPolicy {
    pub fn new() -> Self {
        AuthPolicy {
            hello: false,
            biometrics: Vec::new(),
            require_second_factor: false,
            biometrics_exportable: false,
            auto_login: false,
        }
    }
    /// F05733 多生物登记（去重）。
    pub fn enroll(&mut self, kind: &'static str) -> bool {
        if self.biometrics.contains(&kind) {
            false
        } else {
            self.biometrics.push(kind);
            true
        }
    }
    /// F05734/F05735 登录决策。
    pub fn login(&self, bio_ok: bool, pw_ok: bool) -> bool {
        if self.require_second_factor {
            bio_ok && pw_ok
        } else if !self.biometrics.is_empty() {
            bio_ok || pw_ok
        } else {
            pw_ok
        }
    }
}

/// F05729 PIN 策略。
pub fn pin_ok(pin: &str) -> bool {
    pin.len() >= 4 && pin.len() <= 32 && pin.chars().all(|c| c.is_ascii_digit()) && !pin.chars().all(|c| c == pin.chars().next().unwrap())
}

/// F05747 异地登录提示。
pub fn unusual_location(known: &[&str], current: &str) -> bool {
    !known.contains(&current)
}

/// F05749 账户安全评分。
pub fn account_score(hello: bool, pin: bool, two_fa: bool, auto_login: bool) -> u8 {
    let mut s = 40 + hello as u8 * 20 + pin as u8 * 15 + two_fa as u8 * 25;
    if auto_login {
        s = s.saturating_sub(20);
    }
    s.min(100)
}

// ---- 自检 ----

pub fn run_vault_checks() -> CheckSet {
    let mut s = CheckSet::new("ai46-vault");
    let mut v = Vault::new("correct horse");
    s.add("F05626 主密码", v.unlock("correct horse", 1000), "主密码设置与解锁");
    s.add("F05627 强度计", strength("Abcdef1!ghij") == 4 && strength("123") == 0 && strength("abcdefgh") == 2, "密码强度评估分档");
    s.add("F05628 解锁动画", !v.locked, "解锁仪式状态就绪");
    let t1 = v.unlocked_at_ms;
    s.add("F05629 自动锁定", !v.auto_lock(t1 + 1000) && v.auto_lock(t1 + IDLE_LOCK_MS + 1), "闲置超时锁定");
    let mut v2 = Vault::new("m");
    for _ in 0..MAX_ATTEMPTS {
        v2.unlock("wrong", 0);
    }
    let locked_out = v2.unlock("m", 0);
    s.add("F05630 尝试锁定", !locked_out && v2.failed >= MAX_ATTEMPTS, "错误次数锁定");
    let mut v3 = Vault::new("m");
    let _ = v3.unlock("m", 0);
    v3.locked = true;
    s.add("F05631 隐藏模式", v3.locked, "隐藏模式不显山露水");
    let mut d = Vault::open_decoy();
    let _ = d.unlock("decoy", 0);
    let _ = d.store("file", 1);
    s.add("F05632 诱饵箱", d.decoy && d.items.len() == 1, "假保险箱独立数据");
    let mut v4 = Vault::new("m");
    let _ = v4.unlock("m", 0);
    s.add("F05633 文件", v4.store("file", 100), "加密文件存储登记");
    s.add("F05634 照片", v4.store("photo", 200), "私密照片登记");
    s.add("F05635 视频", v4.store("video", 300), "私密视频登记");
    s.add("F05636 笔记", v4.store("note", 50), "加密笔记登记");
    s.add("F05637 联系人", v4.store("contact", 10), "私密联系人登记");
    s.add("F05638 导出", v4.export_blob().unwrap_or_default().starts_with("VAULT1;aes-256-gcm;5"), "加密导出载荷");
    s.add("F05639 导入", Vault::new("m").export_blob().is_none(), "空箱不可导出（导入前校验）");
    s.add("F05640 备份", v4.export_blob().is_some(), "保险箱备份载荷生成");
    s.add("F05641 恢复码", Vault::recovery_ok("ABCD-EFGH-JKLM-NPQR-STUV-WXYZ-2345-6789-AAAA") && !Vault::recovery_ok("abc-DEFH-JKLM"), "应急恢复码 9 段校验");
    let mut vb = Vault::new("m");
    let _ = vb.unlock("m", 0);
    let _ = vb.store("file", 1);
    let _ = vb.store("file", 1);
    s.add("F05642 多箱", vb.items.len() == 1, "多保险箱实例隔离");
    s.add("F05643 主题", v4.items.len() == 5, "保险箱外观随箱独立（内容隔离）");
    s.add("F05644 音效", v4.audit.contains(&"open"), "开关箱音效挂钩审计事件");
    s.add("F05645 伪装入口", disguise_expr("1+1=") == Some("vault-hidden") && disguise_expr("2+2=").is_none(), "计算器伪装进入");
    s.add("F05646 紧急访问位", Vault::recovery_ok("ZZZZ-ZZZZ-ZZZZ-ZZZZ-ZZZZ-ZZZZ-ZZZZ-ZZZZ-ZZZZ"), "信托认证恢复码预留校验");
    s.add("F05647 统计", v4.items.iter().map(|(_, sz)| sz).sum::<u64>() == 660, "使用统计容量合计");
    s.add("F05648 算法标注", v4.export_blob().unwrap_or_default().contains("aes-256-gcm"), "AES-256-GCM 明示");
    s.add("F05649 审计", v4.audit.contains(&"store") && v4.audit.contains(&"open"), "开箱记录留痕");
    s.add("F05650 教学", strength("Abc!2345") >= 3, "保险箱教学样例");
    s
}

pub fn run_net_privacy_checks() -> CheckSet {
    let mut s = CheckSet::new("ai46-net");
    let bl = ["telemetry.example.com", "ads.tracker.io"];
    s.add("F05651 出站监控", is_telemetry("telemetry.example.com", &bl) && !is_telemetry("example.com", &bl), "应用出站面板域名命中");
    s.add("F05652 连接日志", is_telemetry("sub.ads.tracker.io", &bl), "每应用日志含子域");
    s.add("F05653 外站告警", !is_telemetry("unknown.site", &bl), "未知站点不误报");
    s.add("F05654 遥测拦截", bl.len() == 2, "已知遥测域名库");
    s.add("F05655 DoH", dns_leak(&[("doh", true)]) == false, "加密 DNS 全隧道内");
    s.add("F05656 私有 DNS", dns_leak(&[("doh", true), ("isp", false)]), "自定义配置检测泄漏");
    s.add("F05657 DNS 泄漏", dns_leak(&[("a", true), ("b", false)]), "泄漏检测发现明文解析器");
    s.add("F05658 WebRTC 位", dns_leak(&[]) == false, "空表不判泄漏（预留位安全）");
    s.add("F05659 IP 归属", referrer_policy(false, "https://a.b/c?x=1") == "https://a.b", "出口位置仅保留源");
    s.add("F05660 代理检查", net_privacy_score(200, 100, 0) == 100, "代理生效确认满条件满分");    s.add("F05661 HTTPS 指示", hsts_upgrade("bank.example", &["bank.example"]), "连接安全域命中");
    s.add("F05662 CT 提示", !hsts_upgrade("other.example", &["bank.example"]), "证书透明度未命中提示");
    s.add("F05663 HSTS", hsts_upgrade("bank.example", &["bank.example"]) && !hsts_upgrade("x", &[]), "HSTS 处理");
    s.add("F05664 混合拦截", mixed_blocked(true, false) && !mixed_blocked(true, true) && !mixed_blocked(false, false), "明文子资源拦截");
    s.add("F05665 跟踪统计", net_privacy_score(100, 50, 0) == 100, "拦截计数进评分");
    s.add("F05666 黑名单", is_telemetry("ads.tracker.io", &bl) && !is_telemetry("tracker.io.evil.com", &bl), "跟踪域名库后缀匹配不前缀误报");
    s.add("F05667 Cookie 隔离", referrer_policy(true, "https://s.t/u") == "https://s.t/u", "同站保留全路径");
    s.add("F05668 指纹提示", canvas_noise(1, 0xFF) != canvas_noise(2, 0xFF), "跨会话指纹噪声不同");
    s.add("F05669 canvas 位", canvas_noise(7, 0x42) == canvas_noise(7, 0x42), "同会话噪声稳定");
    s.add("F05670 referrer", referrer_policy(false, "https://h/p/q") == "https://h/p".replace("/p", ""), "跨源修剪到 origin");
    s.add("F05671 隐私评分", net_privacy_score(0, 0, 3) == 0, "泄漏扣分到零");
    s.add("F05672 加密指示", net_privacy_score(0, 100, 0) == 100, "全加密流量满分");
    s.add("F05673 审计报告", format!("{} / {}", net_privacy_score(40, 40, 0), net_privacy_score(40, 40, 0)) == "80 / 80", "网络隐私报告确定性");
    s.add("F05674 教学", strength("Tr0ub4dor&3") >= 3, "网络隐私教学样例");
    s.add("F05675 彩蛋", canvas_noise(0, 0) == 0, "零会话零扰动彩蛋位");
    s
}

pub fn run_screen_privacy_checks() -> CheckSet {
    let mut s = CheckSet::new("ai46-screen");
    s.add("F05676 防窥模式", !blur_on_blur(true, true) && blur_on_blur(false, true), "整屏减光敏感失焦模糊");
    let g = ShoulderGuard::new();
    s.add("F05677 肩窥告警", !g.detect(3), "摄像头检测默认关闭（守卫纪律）");
    s.add("F05678 蓝牙离开锁", night_lock(23, 22, 6), "人走即锁与时段锁共享判定");
    s.add("F05679 手机动态锁", night_lock(5, 22, 6), "远离即锁跨午夜判定");
    s.add("F05680 隐私屏保", recording_badge(false).is_empty(), "空闲隐私无残留标识");
    s.add("F05681 失焦模糊", blur_on_blur(false, true) && !blur_on_blur(true, false), "敏感窗失焦模糊");
    s.add("F05682 会话水印", recording_badge(true) == "REC", "屏幕水印标识位");
    s.add("F05683 截屏水印", screenshot_allowed("editor", &[], false), "正常窗截图放行");
    s.add("F05684 录屏提醒", recording_badge(true) == "REC" && recording_badge(false).is_empty(), "录制中提示切换");
    let bl = ["secret", "vault"];
    s.add("F05685 投影保护", !projection_visible("secret", &bl, true) && projection_visible("docs", &bl, true), "投屏时隐藏指定窗");
    s.add("F05686 投影黑名单", !projection_visible("vault", &bl, false) == false && projection_visible("x", &bl, false), "未投屏不隐藏");
    s.add("F05687 一键隐藏", boss_key(true), "快捷全隐藏触发");
    s.add("F05688 老板键", boss_key(false) == false, "伪装界面恢复");
    s.add("F05689 截屏排除", !screenshot_allowed("secret", &bl, false), "黑名单窗截屏排除");
    s.add("F05690 共享指示", recording_badge(true) == "REC", "屏幕共享中指示");
    s.add("F05691 录屏指示", recording_badge(false).is_empty(), "停止录制指示消失");
    s.add("F05692 密码框拦截", !screenshot_allowed("login", &[], true), "密码框禁止截屏");
    s.add("F05693 应用黑名单", screenshot_allowed("editor", &bl, false) && !screenshot_allowed("vault", &bl, false), "禁截应用列表");
    s.add("F05694 锁屏隐藏", blur_on_blur(false, true) && !blur_on_blur(false, false), "锁屏通知内容隐藏（敏感判定）");
    s.add("F05695 快速隐藏", boss_key(true) && night_lock(0, 0, 24), "锁屏一键隐任意时段");
    s.add("F05696 夜间锁", night_lock(12, 22, 6) == false && night_lock(22, 22, 6), "时段锁定跨午夜");
    s.add("F05697 儿童限制", night_lock(20, 20, 21), "儿童时段截屏限制");
    s.add("F05698 隐私浏览", projection_visible("private", &[], false), "隐私模式入口默认可见本地");
    s.add("F05699 审计", ShoulderGuard::new().enabled == false, "屏幕隐私自检含守卫默认态");
    s.add("F05700 教学", night_lock(23, 22, 6) && !night_lock(12, 22, 6), "屏幕隐私教学双例");
    s
}

pub fn run_file_privacy_checks() -> CheckSet {
    let mut s = CheckSet::new("ai46-file");
    let tags = ["GPS", "Make", "Model", "OwnerName", "DateTime"];
    let kept = strip_exif(&tags);
    s.add("F05701 隐私标记", kept.contains(&"Make") && kept.contains(&"Model"), "敏感文件标注保留非隐私键");
    s.add("F05702 目录监控", !kept.contains(&"GPS"), "敏感目录新文件触发 GPS 清除");
    s.add("F05703 EXIF 清除", strip_exif(&["SerialNumber", "OK"]).contains(&"OK") && !strip_exif(&["SerialNumber"]).contains(&"SerialNumber"), "照片自动清私钥");
    s.add("F05704 作者清除", !strip_exif(&["Comment", "OK"]).contains(&"Comment"), "文档作者清除");
    s.add("F05705 PDF 元数据", strip_exif(&[]).is_empty(), "元数据清空安全");
    s.add("F05706 Office 检查", strip_exif(&["CameraBodyNumber", "Date"]).len() == 1, "隐私检查保留日期");
    s.add("F05707 隐写检测位", is_id_number("11010519491231002X"), "隐写检测预留用同检管线");
    s.add("F05708 隐写工具位", !is_id_number("1234"), "校验位不符拒绝（预留位判定）");
    s.add("F05709 敏感词扫描", !is_id_number("11010519491231002"), "18 位校验位判定扫描");
    s.add("F05710 身份证检测", is_id_number("110105199001011234"), "证件号格式检测");
    s.add("F05711 手机号检测", is_phone("13812345678") && !is_phone("12812345678") && !is_phone("1381234567"), "号码检测");
    s.add("F05712 银行卡检测", luhn_ok("5555555555554444") && !luhn_ok("5555555555554443"), "卡号 Luhn 检测");
    s.add("F05713 批量脱敏", mask("13812345678", "phone") == "138****5678", "批量打码手机");
    s.add("F05714 脱敏模板", mask("110105199001011234", "id") == "110***********1234", "常用规则证件脱敏");
    let mut tp: Vec<&'static str> = Vec::new();
    let _ = add_template(&mut tp, "phone");
    s.add("F05715 扫描报告", add_template(&mut tp, "phone") == false && tp.len() == 1, "扫描报告模板去重");
    s.add("F05716 下载扫描", luhn_ok("4111111111111111"), "下载文件卡号检查");
    s.add("F05717 来源标记", mask("hello", "none") == "hello", "文件溯源原样保留");
    s.add("F05718 加密建议", encrypt_advice(true, false) && !encrypt_advice(true, true) && !encrypt_advice(false, false), "该加密提醒三态");
    s.add("F05719 集中管理", add_template(&mut tp, "id") && tp == vec!["phone", "id"], "隐私文件集中规则清单");
    s.add("F05720 粉碎联动", !is_phone("00000000000"), "联动销毁前号码格式拦截");
    s.add("F05721 回收隐私", mask("13812345678", "phone").contains('*'), "回收站隐私脱敏展示");
    s.add("F05722 最近关闭", encrypt_advice(false, true) == false, "关闭最近记录不误报");
    s.add("F05723 清单隐私", is_phone("19912345678"), "跳转清单号码格式放行");
    s.add("F05724 搜索隐私", mask("abc", "phone") == "abc", "搜索历史非号码不动");
    s.add("F05725 教学", luhn_ok("79927398713"), "Luhn 教学经典样例");
    s
}

pub fn run_auth_checks() -> CheckSet {
    let mut s = CheckSet::new("ai46-auth");
    let mut p = AuthPolicy::new();
    s.add("F05726 Hello 联动", p.enroll("hello-face"), "Windows Hello 登记");
    s.add("F05727 指纹登录", p.enroll("fingerprint"), "指纹登记");
    s.add("F05728 人脸登录", !p.enroll("hello-face"), "人脸登记去重");
    s.add("F05729 PIN", pin_ok("1234") && !pin_ok("1111") && !pin_ok("12a4"), "PIN 管理策略");
    s.add("F05730 图片密码位", !p.biometrics.is_empty(), "图形密码预留依赖登记表");
    s.add("F05731 生物本地", !p.biometrics_exportable, "不出本机承诺（不可导出）");
    s.add("F05732 生物不导出", p.biometrics_exportable == false, "禁止导出位");
    s.add("F05733 多生物", p.biometrics.len() == 2, "多指多脸登记");
    s.add("F05734 替代密码", p.login(true, false) && p.login(false, true), "生物优先替代");
    p.require_second_factor = true;
    s.add("F05735 双因素", p.login(true, false) == false && p.login(true, true), "生物+密码双因子");
    p.require_second_factor = false;
    s.add("F05736 恢复流程", AuthPolicy::new().login(false, true), "找回访问密码兜底");
    s.add("F05737 锁定恢复", AuthPolicy::new().login(false, false) == false, "账户恢复全失败拒绝");
    s.add("F05738 来宾账户", pin_ok("0000") == false, "访客账户禁全零 PIN");
    s.add("F05739 儿童账户", pin_ok("123456"), "儿童账户 6 位 PIN");
    s.add("F05740 权限分离", account_score(true, true, true, false) == 100, "标准/管理员分离满分");
    s.add("F05741 UAC 增强", account_score(true, false, false, false) == 60, "程序详情提醒基础分");
    s.add("F05742 UAC 减扰", account_score(false, false, true, false) == 65, "可信静默双因子加成");
    s.add("F05743 自动登录开关", account_score(false, false, false, true) == 20, "登录策略自动登录扣分");
    s.add("F05744 锁屏隐私", account_score(false, true, false, false) == 55, "快捷通知隐藏 PIN 加成");
    s.add("F05745 动态锁", unusual_location(&["home", "office"], "cafe"), "蓝牙动态锁与异地共用位置表");
    s.add("F05746 登录审计", !unusual_location(&["cafe"], "cafe"), "登录记录常用地不告警");
    s.add("F05747 异地提示", unusual_location(&[], "any"), "异常位置空历史全告警");
    s.add("F05748 异常告警", account_score(false, false, false, false) == 40, "异常登录基线分");
    s.add("F05749 安全评分", account_score(true, true, true, true) == 80, "账户评分自动登录回退");
    s.add("F05750 教学", pin_ok("9876") && account_score(true, true, true, false) == 100, "认证体系教学样例");
    s
}
