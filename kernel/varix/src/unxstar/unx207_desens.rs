//! UNX-F0207 · 报告敏感信息脱敏
//!
//! 锚点判据（施工书原文）：UNX-F0207-J1 基址/序列号类字段默认脱敏，
//! 诊断模式授权后可见，泄露断言 0。
//!
//! 设计：字段分级（Public / Sensitive{Base, Serial, Token}）→ 脱敏器
//! （Sensitive 默认渲染为掩码 `****`，诊断授权后按需放行并逐笔记账）
//! → 泄露断言（未授权态渲染产物中不得出现任何敏感明文）。三重红线：
//! ①未授权明文出现即红；②放行必逐笔留账（谁在何时看了什么）；
//! ③掩码长度与原值无关（防长度侧信道）。
//!
//! 错误码段：0x2BD0（全仓普查后独占分配，AI-WBX1022）。

use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// 模块版本。
pub const F0207_VERSION: &str = "F0207-desens-v1";

/// 诊断码段基址（UNX-F0207 独占）。
pub const SEG_BASE: u16 = 0x2BD0;

/// 掩码定长（与原值长度解耦）。
pub const MASK_LEN: usize = 8;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u16)]
pub enum E {
    /// 未授权明文泄露。
    LeakPlain = SEG_BASE + 1,
    /// 放行未记账。
    GrantNoLedger = SEG_BASE + 2,
    /// 未知字段。
    UnknownField = SEG_BASE + 3,
    /// 掩码长度泄漏原值长度。
    MaskLenLeak = SEG_BASE + 4,
    /// 授权态重复授权。
    DupGrant = SEG_BASE + 5,
}

impl E {
    pub const fn code(self) -> u16 {
        self as u16
    }
}

/// 字段敏感级。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Level {
    /// 公开字段：直接渲染。
    Public,
    /// 敏感：基址类。
    SensitiveBase,
    /// 敏感：序列号类。
    SensitiveSerial,
    /// 敏感：令牌类。
    SensitiveToken,
}

/// 报告字段。
#[derive(Debug, Clone)]
pub struct Field {
    /// 字段名。
    pub name: String,
    /// 明文值。
    pub value: String,
    /// 敏感级。
    pub level: Level,
}

/// 授权放行账（一笔 = 一次诊断态读值）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GrantRec {
    /// 字段名。
    pub field: String,
    /// 授权理由。
    pub reason: String,
}

/// 脱敏器。
#[derive(Debug, Default)]
pub struct Desensitizer {
    /// 字段集。
    pub fields: Vec<Field>,
    /// 诊断授权态。
    diag_granted: bool,
    /// 放行账。
    pub grants: Vec<GrantRec>,
}

impl Desensitizer {
    /// 登记字段。
    pub fn put(&mut self, name: &str, value: &str, level: Level) {
        self.fields.push(Field {
            name: String::from(name),
            value: String::from(value),
            level,
        });
    }

    /// 进入诊断授权态。
    pub fn grant_diag(&mut self, reason: &str) -> Result<(), u16> {
        if self.diag_granted {
            return Err(E::DupGrant.code());
        }
        self.diag_granted = true;
        // 授权本身不入放行账（账只记逐字段读值）。
        let _ = reason;
        Ok(())
    }

    /// 读取字段渲染值：未授权敏感级一律掩码；授权态明文 + 记账。
    pub fn render(&mut self, name: &str, reason: &str) -> Result<String, u16> {
        let i = self
            .fields
            .iter()
            .position(|f| f.name == name)
            .ok_or(E::UnknownField.code())?;
        let sensitive = self.fields[i].level != Level::Public;
        if !sensitive {
            return Ok(self.fields[i].value.clone());
        }
        if self.diag_granted {
            self.grants.push(GrantRec {
                field: self.fields[i].name.clone(),
                reason: String::from(reason),
            });
            return Ok(self.fields[i].value.clone());
        }
        Ok(String::from("*").repeat(MASK_LEN))
    }

    /// 泄露断言：未授权态全量渲染产物中不得含任何敏感明文。
    pub fn leak_audit(&mut self) -> Result<(), u16> {
        let saved = self.diag_granted;
        self.diag_granted = false;
        // 先收集敏感字段快照，避免同时借 fields 与 render 的可变借用。
        let mut snap: Vec<(String, String)> = Vec::new();
        for f in self.fields.iter() {
            if f.level != Level::Public {
                snap.push((f.name.clone(), f.value.clone()));
            }
        }
        for (name, plain) in snap.iter() {
            let r = self.render(name, "audit")?;
            if r.contains(plain.as_str()) {
                self.diag_granted = saved;
                return Err(E::LeakPlain.code());
            }
            if r.chars().count() != MASK_LEN {
                self.diag_granted = saved;
                return Err(E::MaskLenLeak.code());
            }
        }
        self.diag_granted = saved;
        Ok(())
    }

    /// 放行账红线：授权态读敏感字段必须留账。
    pub fn grant_ledger_balanced(&self, reads: usize) -> Result<(), u16> {
        if self.grants.len() != reads {
            return Err(E::GrantNoLedger.code());
        }
        if self.grants.iter().any(|g| g.reason.is_empty()) {
            return Err(E::GrantNoLedger.code());
        }
        Ok(())
    }
}

/// J1 自证（判据原文：默认脱敏，诊断授权后可见，泄露断言 0）。
pub fn self_audit() -> Result<(), u16> {
    // 一、夹具：2 公开 + 3 敏感（基址/序列号/令牌）。
    let mut d = Desensitizer::default();
    d.put("boot-stage", "handoff", Level::Public);
    d.put("page-table-level", "4", Level::Public);
    d.put("kernel-base", "0x100000000", Level::SensitiveBase);
    d.put("disk-serial", "WD-WX21A9", Level::SensitiveSerial);
    d.put("attest-token", "t-9981", Level::SensitiveToken);

    // 二、J1 主体：默认态泄露断言 0 + 敏感读值全是掩码。
    d.leak_audit()?;
    let m = d.render("kernel-base", "x")?;
    if m.chars().count() != MASK_LEN || m.contains("0x100000000") {
        return Err(E::LeakPlain.code());
    }
    let pubv = d.render("boot-stage", "x")?;
    if pubv != "handoff" {
        return Err(E::UnknownField.code());
    }

    // 三、诊断授权：明文可见且逐笔留账。
    d.grant_diag("operator-console")?;
    let plain = d.render("disk-serial", "serial-check")?;
    if plain != "WD-WX21A9" {
        return Err(E::GrantNoLedger.code());
    }
    let _ = d.render("attest-token", "token-check")?;
    d.grant_ledger_balanced(2)?;
    if d.grants[0].reason != "serial-check" {
        return Err(E::GrantNoLedger.code());
    }

    // 四、红线逐项：重复授权 / 未知字段 / 长度侧信道（不同长度原值同长掩码）。
    if d.grant_diag("again").is_ok() {
        return Err(E::DupGrant.code());
    }
    if d.render("no-such", "x").is_ok() {
        return Err(E::UnknownField.code());
    }
    let mut e = Desensitizer::default();
    e.put("a", "1", Level::SensitiveBase);
    e.put("b", "0123456789abcdef0123456789", Level::SensitiveBase);
    let ma = e.render("a", "x")?;
    let mb = e.render("b", "x")?;
    if ma.chars().count() != mb.chars().count() {
        return Err(E::MaskLenLeak.code());
    }

    // 五、判别式段位对拍。
    if E::LeakPlain.code() != SEG_BASE + 1 || E::DupGrant.code() != SEG_BASE + 5 {
        return Err(E::LeakPlain.code());
    }
    Ok(())
}
