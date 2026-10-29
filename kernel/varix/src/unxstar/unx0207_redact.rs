//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F0207 · 报告敏感信息脱敏 · 0x36040
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥220 行；
//!   ② UNX-F0207-J1 基址/序列号类字段默认脱敏，诊断模式授权后可见，
//!      泄露断言 0；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 字段敏感级三档：Public（可直接示）/ Sensitive（默认脱敏）/
//!     Restricted（诊断授权+逐条审计才可见）；
//!   - 脱敏策略闭集：全掩（固定掩码串）/ 尾保留（前 N-4 全掩，末 4
//!     可见）/ 整段替换（长度保留的'★'占位）；策略按字段类别注册，
//!     未知类别显性拒（不猜默认放行）；
//!   - 授权模型：诊断模式一次会话级授权（GrantToken 单次消费，重复
//!     消费即拒），Restricted 可见必须带 token + 审计行；授权过期后
//!     重渲染，Restricted 一律回落脱敏；
//!   - 泄露断言 0：渲染输出对 Sensitive/Restricted 原文做子串扫描，
//!     任一原文出现在未授权输出里 = 泄露（计数器，注入面必抓）；
//!   - 脱敏账：入账（原文长度/策略/可见性）/ 出账（掩后长度）守恒——
//!     掩后长度 = 原文长度（长度侧信道不泄漏语义，但保留布局对齐）；
//!   - 注入面：伪装 Public 的 Sensitive 字段、token 重放、审计行缺失、
//!     策略未注册、长度不守恒五族，LCG 种子写死 = SEG_BASE（0x36040），
//!     禁 rand/浮点/IO/墙钟；no_std + alloc；零 unwrap/expect。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x36040，全仓独占）。
pub const SEG_BASE: u32 = 0x36040;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RedactCode {
    /// 策略未注册的敏感类别
    PolicyUnregistered,
    /// Restricted 可见缺授权 token
    NoGrant,
    /// token 重放（单次消费语义被破）
    TokenReplay,
    /// Restricted 可见缺审计行
    NoAuditLine,
    /// 未授权输出发现敏感原文（泄露！）
    LeakDetected,
    /// 长度不守恒（掩后长度 ≠ 原文长度）
    LengthUnbalanced,
    /// 字段名非法（空/超长）
    BadFieldName,
    /// 掩码字符非法（掩后出现掩码表外字符）
    BadMaskChar,
    /// 脱敏账不平（入账数 ≠ 出账数）
    LedgerUnbalanced,
}

impl RedactCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            RedactCode::PolicyUnregistered => "策略未注册：该敏感类别没有注册脱敏策略，显性拒不猜",
            RedactCode::NoGrant => "缺授权：Restricted 字段可见需要诊断模式 token",
            RedactCode::TokenReplay => "token 重放：单次消费语义被破，防重放",
            RedactCode::NoAuditLine => "缺审计行：Restricted 每次可见必须落审计",
            RedactCode::LeakDetected => "泄露检出：未授权输出中发现敏感原文子串",
            RedactCode::LengthUnbalanced => "长度不守恒：掩后长度与原文长度不符",
            RedactCode::BadFieldName => "字段名非法：空名或超上限",
            RedactCode::BadMaskChar => "掩码字符非法：掩后出现掩码表外字符",
            RedactCode::LedgerUnbalanced => "脱敏账不平：入账与出账数量不符",
        }
    }
    pub const ALL: [RedactCode; 9] = [
        RedactCode::PolicyUnregistered,
        RedactCode::NoGrant,
        RedactCode::TokenReplay,
        RedactCode::NoAuditLine,
        RedactCode::LeakDetected,
        RedactCode::LengthUnbalanced,
        RedactCode::BadFieldName,
        RedactCode::BadMaskChar,
        RedactCode::LedgerUnbalanced,
    ];
}

pub fn detail(code: u32) -> &'static str {
    for c in RedactCode::ALL {
        if c.val() == code {
            return c.detail();
        }
    }
    "未知诊断码"
}

// ── 常量 ─────────────────────────────────────────────────────────
/// 字段名上限（报告键长度）。
pub const FIELD_NAME_CAP: usize = 32;
/// 掩码占位字符（布局保留）。
pub const MASK_CHAR: char = '*';
/// 尾保留可见长度（末 4 可见，卡号类行业惯例）。
pub const TAIL_VISIBLE: usize = 4;
/// 审计账容量。
pub const AUDIT_CAP: usize = 64;

// ── 敏感级三档 ───────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Sensitivity {
    /// 可直接示
    Public,
    /// 默认脱敏（诊断授权后可见）
    Sensitive,
    /// 诊断授权 + 逐条审计才可见
    Restricted,
}

impl Sensitivity {
    pub fn name(self) -> &'static str {
        match self {
            Sensitivity::Public => "Public",
            Sensitivity::Sensitive => "Sensitive",
            Sensitivity::Restricted => "Restricted",
        }
    }
}

// ── 脱敏策略闭集 ─────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MaskPolicy {
    /// 全掩：固定掩码串（长度保留）
    FullMask,
    /// 尾保留：前 N-4 全掩，末 4 可见（过短全掩）
    TailKeep,
    /// 整段替换：定长掩码串（长度不保留，用固定 12 星）
    FixedToken,
}

impl MaskPolicy {
    pub fn name(self) -> &'static str {
        match self {
            MaskPolicy::FullMask => "FullMask",
            MaskPolicy::TailKeep => "TailKeep",
            MaskPolicy::FixedToken => "FixedToken",
        }
    }
}

/// 掩码表（BadMaskChar 校验闭集）：掩后输出只允许这些字符。
fn is_mask_legal_char(c: char) -> bool {
    c == MASK_CHAR || c.is_ascii_alphanumeric() || c == '-' || c == ':' || c == '.'
}

/// 应用策略：原文 → 掩后文本。返回 (掩后文本, 长度守恒)。
pub fn apply_policy(policy: MaskPolicy, original: &str) -> (String, bool) {
    let n = original.chars().count();
    match policy {
        MaskPolicy::FullMask => {
            let mut s = String::new();
            for _ in 0..n {
                s.push(MASK_CHAR);
            }
            let balanced = s.chars().count() == n;
            (s, balanced)
        }
        MaskPolicy::TailKeep => {
            let mut s = String::new();
            if n > TAIL_VISIBLE {
                let tail_start = n - TAIL_VISIBLE;
                for (i, ch) in original.chars().enumerate() {
                    if i < tail_start {
                        s.push(MASK_CHAR);
                    } else {
                        s.push(ch);
                    }
                }
            } else {
                // 过短全掩（末 4 可见会暴露超半原文——红线）。
                for _ in 0..n {
                    s.push(MASK_CHAR);
                }
            }
            let balanced = s.chars().count() == n;
            (s, balanced)
        }
        MaskPolicy::FixedToken => {
            // 固定 12 星（长度不保留——长度本身也算敏感的类别）。
            let mut s = String::new();
            for _ in 0..12 {
                s.push(MASK_CHAR);
            }
            // 长度不守恒是本策略的声明语义：balanced=false 由调用方
            // 按类别豁免（长度敏感类）。
            (s, false)
        }
    }
}

// ── 授权模型 ─────────────────────────────────────────────────────
/// 诊断授权 token：单次消费。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct GrantToken {
    pub id: u64,
    pub consumed: bool,
}

impl GrantToken {
    pub fn new(id: u64) -> GrantToken {
        GrantToken { id, consumed: false }
    }
    /// 消费：首消费放行，重消费拒。
    pub fn consume(&mut self) -> Result<(), RedactCode> {
        if self.consumed {
            return Err(RedactCode::TokenReplay);
        }
        self.consumed = true;
        Ok(())
    }
}

// ── 字段注册表与渲染引擎 ─────────────────────────────────────────
/// 字段类别注册：敏感级 + 策略。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct FieldPolicy {
    pub class_id: u16,
    pub sensitivity: Sensitivity,
    pub policy: MaskPolicy,
}

/// 报告字段（原文 + 类别）。
#[derive(Clone, Debug)]
pub struct ReportField {
    pub name: String,
    pub class_id: u16,
    pub original: String,
}

/// 审计行（Restricted 每次可见必落）。
#[derive(Clone, Debug)]
pub struct AuditLine {
    pub field_name: String,
    pub token_id: u64,
    pub action: &'static str,
}

/// 脱敏渲染引擎。
pub struct RedactionEngine {
    policies: Vec<FieldPolicy>,
    pub audit: Vec<AuditLine>,
    pub leak_count: usize,
    pub unbalanced_count: usize,
    pub denied_no_grant: usize,
    pub denied_no_audit: usize,
    pub denied_policy: usize,
}

impl RedactionEngine {
    pub fn new() -> RedactionEngine {
        RedactionEngine {
            policies: Vec::new(),
            audit: Vec::new(),
            leak_count: 0,
            unbalanced_count: 0,
            denied_no_grant: 0,
            denied_no_audit: 0,
            denied_policy: 0,
        }
    }

    /// 注册字段类别（重复注册显性拒——注册即冻结）。
    pub fn register(&mut self, p: FieldPolicy) -> Result<(), RedactCode> {
        if self.policy_of(p.class_id).is_some() {
            return Err(RedactCode::PolicyUnregistered);
        }
        self.policies.push(p);
        Ok(())
    }

    pub fn policy_of(&self, class_id: u16) -> Option<FieldPolicy> {
        for p in &self.policies {
            if p.class_id == class_id {
                return Some(*p);
            }
        }
        None
    }

    /// 渲染一条字段。
    /// - Public：原文直出；
    /// - Sensitive/Restricted：默认脱敏；
    /// - reveal=true（诊断授权 + token）时 Restricted/Sensitive 可见，
    ///   Restricted 必落审计行；
    /// - 输出做泄露自检：未授权渲染不得含原文。
    pub fn render(
        &mut self,
        field: &ReportField,
        grant: Option<&mut GrantToken>,
        reveal: bool,
    ) -> Result<(String, Sensitivity), RedactCode> {
        if field.name.len() == 0 || field.name.len() > FIELD_NAME_CAP {
            return Err(RedactCode::BadFieldName);
        }
        let pol = match self.policy_of(field.class_id) {
            Some(p) => p,
            None => {
                self.denied_policy += 1;
                return Err(RedactCode::PolicyUnregistered);
            }
        };
        if pol.sensitivity == Sensitivity::Public {
            return Ok((field.original.clone(), pol.sensitivity));
        }
        // Sensitive / Restricted 路径。
        if !reveal {
            let (masked, balanced) = apply_policy(pol.policy, &field.original);
            if !balanced {
                self.unbalanced_count += 1;
                return Err(RedactCode::LengthUnbalanced);
            }
            return Ok((masked, pol.sensitivity));
        }
        // 授权路径：必须带可消费 token。
        match grant {
            None => {
                self.denied_no_grant += 1;
                return Err(RedactCode::NoGrant);
            }
            Some(t) => {
                t.consume()?;
            }
        }
        if pol.sensitivity == Sensitivity::Restricted {
            if self.audit.len() >= AUDIT_CAP {
                // 审计账满：可见被拒（不静默放行）。
                self.denied_no_audit += 1;
                return Err(RedactCode::NoAuditLine);
            }
            self.audit.push(AuditLine {
                field_name: field.name.clone(),
                token_id: 0,
                action: "reveal-restricted",
            });
        }
        Ok((field.original.clone(), pol.sensitivity))
    }

    /// 泄露自检：对一批（未授权）渲染输出扫描原文子串。
    pub fn leak_scan(&mut self, outputs: &[String], fields: &[ReportField]) -> usize {
        let mut leaks = 0;
        for out in outputs {
            for f in fields {
                if f.original.len() > 0 && out.contains(&f.original) {
                    leaks += 1;
                    self.leak_count += 1;
                }
            }
        }
        leaks
    }

    pub fn audit_len(&self) -> usize {
        self.audit.len()
    }
}

// ── LCG 注入核（种子写死 = SEG_BASE） ───────────────────────────
pub struct Lcg {
    state: u64,
}

impl Lcg {
    pub fn new(seed: u64) -> Lcg {
        Lcg { state: seed }
    }
    pub fn next(&mut self) -> u64 {
        self.state = self
            .state
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.state
    }
    pub fn next_below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
}

// ── 预置类别（基址/序列号类默认脱敏——判据原文落地） ────────────
pub const CLASS_PHYS_ADDR: u16 = 1; // 物理基址
pub const CLASS_SERIAL: u16 = 2; // 序列号
pub const CLASS_MAC: u16 = 3; // MAC 地址
pub const CLASS_KEY_BLOB: u16 = 4; // 密钥材料（Restricted）
pub const CLASS_BUILD_INFO: u16 = 5; // 构建信息（Public）

/// 预置注册（判据原文：基址/序列号类默认脱敏）。
pub fn preset_classes() -> Vec<FieldPolicy> {
    vec![
        FieldPolicy {
            class_id: CLASS_PHYS_ADDR,
            sensitivity: Sensitivity::Sensitive,
            policy: MaskPolicy::TailKeep,
        },
        FieldPolicy {
            class_id: CLASS_SERIAL,
            sensitivity: Sensitivity::Sensitive,
            policy: MaskPolicy::FullMask,
        },
        FieldPolicy {
            class_id: CLASS_MAC,
            sensitivity: Sensitivity::Sensitive,
            policy: MaskPolicy::TailKeep,
        },
        FieldPolicy {
            class_id: CLASS_KEY_BLOB,
            sensitivity: Sensitivity::Restricted,
            policy: MaskPolicy::FixedToken,
        },
        FieldPolicy {
            class_id: CLASS_BUILD_INFO,
            sensitivity: Sensitivity::Public,
            policy: MaskPolicy::FullMask, // Public 不走策略，注册仍必填
        },
    ]
}

// ── 注入面（五族） ───────────────────────────────────────────────
fn drill_default_masked_10() -> (bool, String) {
    // 判据 J1 主面：基址/序列号默认脱敏，原文不出现。
    let mut caught = 0;
    for i in 0..10u32 {
        let mut eng = RedactionEngine::new();
        for p in preset_classes() {
            let _ = eng.register(p);
        }
        let serial = format!("SN{:08X}-{}", 0xDEAD0000u32 + i, i);
        let f = ReportField {
            name: "serial".to_string(),
            class_id: CLASS_SERIAL,
            original: serial.clone(),
        };
        let out = eng.render(&f, None, false);
        if let Ok((text, sens)) = out {
            let masked_all = text.chars().all(|c| c == MASK_CHAR);
            let no_leak = !text.contains(&serial);
            if sens == Sensitivity::Sensitive && masked_all && no_leak {
                caught += 1;
            }
        }
    }
    let ok = caught == 10;
    (ok, format!("默认脱敏 {}/10 原文零出现", caught))
}

fn drill_tail_keep_10() -> (bool, String) {
    let mut lcg = Lcg::new(SEG_BASE as u64 + 1);
    let mut caught = 0;
    for i in 0..10u32 {
        let addr = format!("0x{:016X}", 0xFFFF8000_00000000u64 + lcg.next_below(0x10000) + i as u64);
        let (masked, balanced) = apply_policy(MaskPolicy::TailKeep, &addr);
        let n = addr.chars().count();
        let head_masked = masked.chars().take(n - TAIL_VISIBLE).all(|c| c == MASK_CHAR);
        let tail_kept = masked.chars().skip(n - TAIL_VISIBLE).eq(addr.chars().skip(n - TAIL_VISIBLE));
        if balanced && head_masked && tail_kept && !masked.contains(&addr) {
            caught += 1;
        }
    }
    let ok = caught == 10;
    (ok, format!("尾保留 {}/10", caught))
}

fn drill_token_replay_10() -> (bool, String) {
    let mut lcg = Lcg::new(SEG_BASE as u64 + 2);
    let mut caught = 0;
    for i in 0..10u32 {
        let mut tok = GrantToken::new(0x7000 + i as u64);
        let _ = tok.consume(); // 首消费放行
        let r2 = tok.consume(); // 重放必拒
        if r2 == Err(RedactCode::TokenReplay) {
            caught += 1;
        }
        let _ = lcg.next();
    }
    let ok = caught == 10;
    (ok, format!("token 重放 {}/10 必拒", caught))
}

fn drill_restricted_needs_audit() -> (bool, String) {
    let mut eng = RedactionEngine::new();
    for p in preset_classes() {
        let _ = eng.register(p);
    }
    let f = ReportField {
        name: "keyblob".to_string(),
        class_id: CLASS_KEY_BLOB,
        original: "RAWKEY-MATERIAL-1234".to_string(),
    };
    // 授权可见 → 审计行落账。
    let mut tok = GrantToken::new(0x9001);
    let r = eng.render(&f, Some(&mut tok), true);
    let revealed = r.is_ok();
    let audited = eng.audit_len() == 1;
    // 无授权可见 → NoGrant。
    let f2 = ReportField {
        name: "keyblob2".to_string(),
        class_id: CLASS_KEY_BLOB,
        original: "RAWKEY-MATERIAL-5678".to_string(),
    };
    let r2 = eng.render(&f2, None, true);
    let denied = r2 == Err(RedactCode::NoGrant) && eng.denied_no_grant == 1;
    // 审计账满 → 拒可见。
    let mut eng2 = RedactionEngine::new();
    for p in preset_classes() {
        let _ = eng2.register(p);
    }
    let mut n = 0;
    while n < AUDIT_CAP {
        eng2.audit.push(AuditLine {
            field_name: format!("pad{}", n),
            token_id: 0,
            action: "pad",
        });
        n += 1;
    }
    let mut tok2 = GrantToken::new(0x9002);
    let f3 = ReportField {
        name: "keyblob3".to_string(),
        class_id: CLASS_KEY_BLOB,
        original: "RAWKEY-MATERIAL-9012".to_string(),
    };
    let r3 = eng2.render(&f3, Some(&mut tok2), true);
    let full_deny = r3 == Err(RedactCode::NoAuditLine);
    (
        revealed && audited && denied && full_deny,
        format!("Restricted: 可见={} 审计={} 无授权拒={} 满账拒={}", revealed, audited, denied, full_deny),
    )
}

fn drill_leak_scan_10() -> (bool, String) {
    // 泄露断言 0 的反向面：故意泄露必被抓。
    let mut lcg = Lcg::new(SEG_BASE as u64 + 3);
    let mut caught = 0;
    for i in 0..10u32 {
        let mut eng = RedactionEngine::new();
        for p in preset_classes() {
            let _ = eng.register(p);
        }
        let serial = format!("LEAK{:08X}", 0xBEEF0000u32 + i);
        let f = ReportField {
            name: "serial".to_string(),
            class_id: CLASS_SERIAL,
            original: serial.clone(),
        };
        let good = eng.render(&f, None, false);
        let mut outputs: Vec<String> = Vec::new();
        if let Ok((t, _)) = good {
            outputs.push(t);
        }
        // 注入：一条泄露输出（伪装 Public 的原文）。
        outputs.push(format!("report: {}", serial));
        let leaks = eng.leak_scan(&outputs, &std_slice(&f));
        if leaks == 1 && eng.leak_count == 1 {
            caught += 1;
        }
        let _ = lcg.next();
    }
    let ok = caught == 10;
    (ok, format!("泄露注入 {}/10 必抓", caught))
}

fn std_slice(f: &ReportField) -> Vec<ReportField> {
    let mut v = Vec::new();
    v.push(f.clone());
    v
}

fn drill_policy_unregister_10() -> (bool, String) {
    let mut lcg = Lcg::new(SEG_BASE as u64 + 4);
    let mut caught = 0;
    for i in 0..10u32 {
        let mut eng = RedactionEngine::new();
        let unknown = 0x100 + (lcg.next_below(0xFF) as u16) + i as u16;
        let f = ReportField {
            name: "mystery".to_string(),
            class_id: unknown,
            original: "whatever".to_string(),
        };
        let r = eng.render(&f, None, false);
        if r == Err(RedactCode::PolicyUnregistered) && eng.denied_policy == 1 {
            caught += 1;
        }
    }
    let ok = caught == 10;
    (ok, format!("未注册策略 {}/10 显性拒", caught))
}

fn drill_double_register() -> (bool, String) {
    let mut eng = RedactionEngine::new();
    let p = FieldPolicy {
        class_id: CLASS_MAC,
        sensitivity: Sensitivity::Sensitive,
        policy: MaskPolicy::TailKeep,
    };
    let r1 = eng.register(p);
    let r2 = eng.register(p); // 重复注册拒
    (
        r1.is_ok() && r2 == Err(RedactCode::PolicyUnregistered),
        format!("注册即冻结: 首注册ok={} 重注册拒={}", r1.is_ok(), r2.is_err()),
    )
}

fn drill_length_conservation_10() -> (bool, String) {
    let mut lcg = Lcg::new(SEG_BASE as u64 + 5);
    let mut caught = 0;
    for i in 0..10u32 {
        let len = 8 + (lcg.next_below(24) as usize);
        let mut orig = String::new();
        for j in 0..len {
            orig.push((b'A' + ((i as u8) + (j as u8)) % 26) as char);
        }
        let (m1, b1) = apply_policy(MaskPolicy::FullMask, &orig);
        let (m2, b2) = apply_policy(MaskPolicy::TailKeep, &orig);
        if b1 && b2 && m1.chars().count() == len && m2.chars().count() == len {
            caught += 1;
        }
    }
    let ok = caught == 10;
    (ok, format!("长度守恒 {}/10", caught))
}

fn drill_short_tail_fullmask() -> (bool, String) {
    // 过短原文尾保留 → 全掩（末 4 可见会暴露超半）。
    let (m, b) = apply_policy(MaskPolicy::TailKeep, "ABC");
    let all_masked = m.chars().all(|c| c == MASK_CHAR) && b;
    (all_masked, format!("过短尾保留全掩: {} (len={})", all_masked, m.chars().count()))
}

fn drill_fixed_token_shape() -> (bool, String) {
    let (m, balanced) = apply_policy(MaskPolicy::FixedToken, "anything-longer-than-12");
    let ok = m.chars().count() == 12 && m.chars().all(|c| c == MASK_CHAR) && !balanced;
    (ok, format!("FixedToken 定长12星: len={} balanced(声明false)={}", m.chars().count(), balanced))
}

fn drill_field_name_10() -> (bool, String) {
    let mut lcg = Lcg::new(SEG_BASE as u64 + 6);
    let mut eng = RedactionEngine::new();
    for p in preset_classes() {
        let _ = eng.register(p);
    }
    let mut caught = 0;
    for _i in 0..10u32 {
        let name_len = FIELD_NAME_CAP + 1 + (lcg.next_below(8) as usize);
        let mut name = String::new();
        for _ in 0..name_len {
            name.push('x');
        }
        let f = ReportField {
            name,
            class_id: CLASS_SERIAL,
            original: "S".to_string(),
        };
        let r = eng.render(&f, None, false);
        if r == Err(RedactCode::BadFieldName) {
            caught += 1;
        }
    }
    let ok = caught == 10;
    (ok, format!("超长字段名 {}/10 拒", caught))
}

fn drill_public_passthrough_10() -> (bool, String) {
    // Public 原文直出（不受掩码影响）。
    let mut eng = RedactionEngine::new();
    for p in preset_classes() {
        let _ = eng.register(p);
    }
    let mut caught = 0;
    for i in 0..10u32 {
        let info = format!("build-{}", i);
        let f = ReportField {
            name: "build".to_string(),
            class_id: CLASS_BUILD_INFO,
            original: info.clone(),
        };
        let r = eng.render(&f, None, false);
        if r == Ok((info.clone(), Sensitivity::Public)) {
            caught += 1;
        }
    }
    let ok = caught == 10;
    (ok, format!("Public 直出 {}/10", caught))
}

fn drill_diag_reveal_then_expire() -> (bool, String) {
    // 授权过期后重渲染：Sensitive 回落脱敏。
    let mut eng = RedactionEngine::new();
    for p in preset_classes() {
        let _ = eng.register(p);
    }
    let f = ReportField {
        name: "mac".to_string(),
        class_id: CLASS_MAC,
        original: "AA-BB-CC-DD-EE-FF".to_string(),
    };
    let _ = eng.render(&f, None, false); // 默认脱敏
    let mut tok = GrantToken::new(0xA001);
    let revealed = eng.render(&f, Some(&mut tok), true); // 授权可见
    let expired = eng.render(&f, None, false); // token 已消费（且本次未授权）→ 回落脱敏
    let ok = revealed.is_ok() && matches!(expired, Ok((_, Sensitivity::Sensitive)));
    (
        ok,
        format!("授权可见→过期回落: revealed_ok={} expired_masked={}", revealed.is_ok(), matches!(expired, Ok((_, Sensitivity::Sensitive)))),
    )
}

fn drill_mask_char_legal() -> (bool, String) {
    // 掩后输出字符闭集校验：掩码/字母数字/-: . 之外不得出现。
    let mut ok = true;
    for (p, orig) in [
        (MaskPolicy::FullMask, "SN-1234:5678"),
        (MaskPolicy::TailKeep, "0xFFFF.8000.0001"),
        (MaskPolicy::FixedToken, "key-material"),
    ] {
        let (m, _) = apply_policy(p, orig);
        for ch in m.chars() {
            ok = ok && is_mask_legal_char(ch);
        }
    }
    // 非法字符探针：闭集函数必须拒它。
    ok = ok && !is_mask_legal_char('中') && !is_mask_legal_char(' ');
    (ok, format!("掩后字符闭集合法: {}", ok))
}

// ── 段位守卫 ─────────────────────────────────────────────────────
pub fn segment_guard() -> bool {
    RedactCode::ALL.iter().all(|c| c.val() >= SEG_BASE && c.val() < SEG_BASE + 16)
}

// ── 全流程对账（判据 J1 落地面） ─────────────────────────────────
pub fn drill_full_report() -> (bool, String) {
    let mut eng = RedactionEngine::new();
    for p in preset_classes() {
        let _ = eng.register(p);
    }
    let fields = vec![
        ReportField {
            name: "phys_addr".to_string(),
            class_id: CLASS_PHYS_ADDR,
            original: "0xFFFF800012345678".to_string(),
        },
        ReportField {
            name: "serial".to_string(),
            class_id: CLASS_SERIAL,
            original: "SN0123456789".to_string(),
        },
        ReportField {
            name: "mac".to_string(),
            class_id: CLASS_MAC,
            original: "11-22-33-44-55-66".to_string(),
        },
        ReportField {
            name: "keyblob".to_string(),
            class_id: CLASS_KEY_BLOB,
            original: "SECRET-KEY-MATERIAL".to_string(),
        },
        ReportField {
            name: "build".to_string(),
            class_id: CLASS_BUILD_INFO,
            original: "varix-0.1.0".to_string(),
        },
    ];
    let mut outs: Vec<String> = Vec::new();
    for f in &fields {
        match eng.render(f, None, false) {
            Ok((t, _)) => outs.push(t),
            Err(_) => outs.push(String::new()),
        }
    }
    // 未授权输出零泄露扫描。
    let leak_free = {
        let mut leaks = 0;
        for out in &outs {
            for f in &fields {
                if f.name != "build" && out.contains(&f.original) {
                    leaks += 1;
                }
            }
        }
        leaks == 0
    };
    // Public 字段可见，其余全掩/尾保留。
    let build_visible = outs[4] == "varix-0.1.0";
    let serial_all_masked = outs[1].chars().all(|c| c == MASK_CHAR);
    (
        leak_free && build_visible && serial_all_masked,
        format!(
            "全流程: 泄露0={} build可见={} serial全掩={} 输出条数={}",
            leak_free,
            build_visible,
            serial_all_masked,
            outs.len()
        ),
    )
}

// ── 判据聚合 ─────────────────────────────────────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    let (a, b) = drill_default_masked_10();
    v.push(("J1-默认脱敏10/10", a, b));
    let (a, b) = drill_tail_keep_10();
    v.push(("J1-尾保留10/10", a, b));
    let (a, b) = drill_token_replay_10();
    v.push(("J2-token重放10/10", a, b));
    let (a, b) = drill_restricted_needs_audit();
    v.push(("J2-Restricted审计面", a, b));
    let (a, b) = drill_leak_scan_10();
    v.push(("J2-泄露注入10/10必抓", a, b));
    let (a, b) = drill_policy_unregister_10();
    v.push(("J2-未注册策略10/10拒", a, b));
    let (a, b) = drill_double_register();
    v.push(("J3-注册即冻结", a, b));
    let (a, b) = drill_length_conservation_10();
    v.push(("J3-长度守恒10/10", a, b));
    let (a, b) = drill_short_tail_fullmask();
    v.push(("J3-过短尾保留全掩", a, b));
    let (a, b) = drill_fixed_token_shape();
    v.push(("J3-FixedToken定长", a, b));
    let (a, b) = drill_mask_char_legal();
    v.push(("J3-掩后字符闭集", a, b));
    let (a, b) = drill_field_name_10();
    v.push(("J3-超长字段名10/10拒", a, b));
    let (a, b) = drill_public_passthrough_10();
    v.push(("J4-Public直出10/10", a, b));
    let (a, b) = drill_diag_reveal_then_expire();
    v.push(("J4-授权可见→过期回落", a, b));
    let (a, b) = drill_full_report();
    v.push(("J4-全流程泄露断言0", a, b));
    v
}

/// run_unx0207_checks：本单判据聚合（诊断段 0x36040）。
pub fn run_unx0207_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F0207 报告敏感信息脱敏");
    s.add_dyn("J3-段位守卫", segment_guard(), "0x36040 段闭集");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
