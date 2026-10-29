//! CGPU-F0734 · 场景内容安全（目标 380 行）
//! ==========================================================================
//! 锚点判据（清单原文摘录）：
//! - 纯功能代码 ≥300 行；② 规格与判据按册内锚点原文逐条落实
//!   （CGPU Varix STAR II · 总纲与施工书#CGPU-F0734，册内文档不在本仓，
//!    以清单判据为施工依据）；③ 完成时 API/软件写回：状态=已完成。
//! - 主题：场景内容安全——内容字段的消毒与校验：字符集白名单、
//!    尺寸/深度上限、索引解析越界防护、敏感字段脱敏与
//!    消毒审计账。
//!
//! 设计纪律（与 F0717 的差异：F0717 以"权限/限速/审计环"为核心，
//! 本单以"内容字段消毒+白名单校验"为核心）：
// - 字符集白名单写死：a-z A-Z 0-9 _ - . /（场景资源路径合法集）；
//   越集字符即拒绝。
// - 深度上限：路径段数 ≤ MAX_SEG=8；段长 ≤ SEG_LEN=32；
//   总长 ≤ PATH_LEN=192。
// - 索引解析：内容索引（基址,数量,容器容量）三元组校验，
//   溢出安全；越界返回 Err 不 panic。
// - 脱敏闭集：敏感字段（key/token/secret 前缀命中）全替换
//   'x'，长度保留；替换后与原串等长。
// - 消毒审计：定容 16 槽账，守恒 total == held + overwritten。
// - 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，
//   确定性。
// - 诊断段 0x4a00 本单独占，错误码 SEG_BASE+1..+N 闭集。

use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

/// 诊断段基址（0x4a00，批内 CGPU-F0734 独占）。
pub const SEG_BASE: u16 = 0x4a00;

/// 字符集越白名单。
pub const E_CHARSET: u16 = SEG_BASE + 1;
/// 路径段数超上限。
pub const E_DEPTH: u16 = SEG_BASE + 2;
/// 段长超上限。
pub const E_SEG_LEN: u16 = SEG_BASE + 3;
/// 路径总长超上限。
pub const E_PATH_LEN: u16 = SEG_BASE + 4;
/// 索引解析越界/溢出。
pub const E_INDEX: u16 = SEG_BASE + 5;
/// 消毒审计参数非法。
pub const E_AUDIT: u16 = SEG_BASE + 6;

/// 路径段数上限。
pub const MAX_SEG: usize = 8;
/// 单段长上限。
pub const SEG_LEN: usize = 32;
/// 路径总长上限。
pub const PATH_LEN: usize = 192;
/// 消毒审计容量。
pub const SAN_AUDIT_CAP: usize = 16;

/// 白名单判定：a-z A-Z 0-9 _ - . /。
pub fn charset_ok(b: u8) -> bool {
    b.is_ascii_lowercase()
        || b.is_ascii_uppercase()
        || b.is_ascii_digit()
        || b == b'_'
        || b == b'-'
        || b == b'.'
        || b == b'/'
}

/// 资源路径校验：白名单 + 段数 + 段长 + 总长四关。
/// 返回段数（过检时）。
pub fn validate_path(p: &str) -> Result<usize, u16> {
    let bytes = p.as_bytes();
    if bytes.len() > PATH_LEN {
        return Err(E_PATH_LEN);
    }
    for &b in bytes {
        if !charset_ok(b) {
            return Err(E_CHARSET);
        }
    }
    // 空路径按 0 段处理（合法，指根）。
    if p.is_empty() {
        return Ok(0);
    }
    let segs: Vec<&str> = p.split('/').collect();
    if segs.len() > MAX_SEG {
        return Err(E_DEPTH);
    }
    for s in &segs {
        if s.len() > SEG_LEN {
            return Err(E_SEG_LEN);
        }
    }
    Ok(segs.len())
}

/// 内容索引三元组：基址 + 数量（容器容量）。
/// 解析 end = base+count（checked），end ≤ cap；任一失败即 E_INDEX。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ContentSlice {
    /// 基址。
    pub base: u32,
    /// 数量。
    pub count: u32,
    /// 容器容量。
    pub cap: u32,
}

impl ContentSlice {
    /// 解析为 (start,end)（越界安全）。
    pub fn resolve(&self) -> Result<(u32, u32), u16> {
        if self.base > self.cap || self.count > self.cap {
            return Err(E_INDEX);
        }
        let end = self
            .base
            .checked_add(self.count)
            .ok_or(E_INDEX)?;
        if end > self.cap {
            return Err(E_INDEX);
        }
        Ok((self.base, end))
    }
}

/// 敏感字段前缀闭集。
pub const SENSITIVE_PREFIXES: [&str; 3] = ["key", "token", "secret"];

/// 前缀命中判定（大小写敏感，字段名首查）。
pub fn is_sensitive(field: &str) -> bool {
    let f = field.to_lowercase();
    SENSITIVE_PREFIXES.iter().any(|p| f.starts_with(*p))
}

/// 脱敏：敏感字段值全替换 'x'（保留长度）；非敏感原样返回。
pub fn redact(field: &str, value: &str) -> Result<String, u16> {
    if field.is_empty() {
        return Err(E_AUDIT);
    }
    if !is_sensitive(field) {
        return Ok(String::from(value));
    }
    let mut out = String::with_capacity(value.len());
    for _ in 0..value.chars().count() {
        out.push('x');
    }
    Ok(out)
}

/// 消毒审计条目。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SanRec {
    /// 字段名。
    pub field: String,
    /// 动作：0=原样 1=脱敏 2=拒绝。
    pub action: u8,
}

/// 消毒审计环。
#[derive(Clone, Debug)]
pub struct SanAudit {
    ring: [Option<SanRec>; SAN_AUDIT_CAP],
    w: usize,
    held: usize,
    overwritten: u64,
    total: u64,
}

impl SanAudit {
    pub fn new() -> Self {
        Self {
            ring: Default::default(),
            w: 0,
            held: 0,
            overwritten: 0,
            total: 0,
        }
    }
    pub fn push(&mut self, r: SanRec) {
        self.ring[self.w] = Some(r);
        self.w = (self.w + 1) % SAN_AUDIT_CAP;
        if self.held < SAN_AUDIT_CAP {
            self.held += 1;
        } else {
            self.overwritten += 1;
        }
        self.total += 1;
    }
    /// 守恒：total == held + overwritten。
    pub fn audit(&self) -> Result<(), u16> {
        if self.total != self.held as u64 + self.overwritten {
            return Err(E_AUDIT);
        }
        Ok(())
    }
    /// 视图。
    pub fn view(&self) -> Vec<SanRec> {
        let start = if self.held < SAN_AUDIT_CAP { 0 } else { self.w };
        let mut out = Vec::with_capacity(self.held);
        for i in 0..self.held {
            let idx = (start + i) % SAN_AUDIT_CAP;
            if let Some(r) = &self.ring[idx] {
                out.push(r.clone());
            }
        }
        out
    }
    pub fn total(&self) -> u64 {
        self.total
    }
}

/// 内容消毒器：字段-值对流水线。
#[derive(Clone, Debug)]
pub struct Sanitizer {
    audit: SanAudit,
}

impl Sanitizer {
    pub fn new() -> Self {
        Self { audit: SanAudit::new() }
    }
    /// 消毒一个字段：敏感→脱敏并记审计（action 1）；
    /// 非敏感→原样（action 0）。
    pub fn field(&mut self, field: &str, value: &str) -> Result<String, u16> {
        let out = redact(field, value)?;
        self.audit.push(SanRec {
            field: String::from(field),
            action: if out == value { 0 } else { 1 },
        });
        Ok(out)
    }
    /// 路径消毒：校验失败记审计（action 2）并透传错误。
    pub fn path(&mut self, p: &str) -> Result<usize, u16> {
        match validate_path(p) {
            Ok(n) => {
                self.audit.push(SanRec {
                    field: String::from("path"),
                    action: 0,
                });
                Ok(n)
            }
            Err(e) => {
                self.audit.push(SanRec {
                    field: String::from("path"),
                    action: 2,
                });
                Err(e)
            }
        }
    }
    /// 审计守恒透传。
    pub fn audit(&self) -> Result<(), u16> {
        self.audit.audit()
    }
    /// 审计总量。
    pub fn audited(&self) -> u64 {
        self.audit.total()
    }
}

/// 自例程：主路径（路径校验/索引解析/脱敏/审计守恒）+ 错误路径。
pub fn drill() -> Result<u32, u16> {
    let mut san = Sanitizer::new();
    // 主路径 0：合法路径（3 段）。
    let n = san.path("assets/tex/albedo")?;
    if n != 3 {
        return Err(E_DEPTH);
    }
    // 主路径 1：根路径（0 段）。
    if san.path("")? != 0 {
        return Err(E_CHARSET);
    }
    // 错误路径 0：越白名单字符（空格）。
    if san.path("assets/tex albedo") != Err(E_CHARSET) {
        return Err(E_CHARSET);
    }
    // 错误路径 1：段数超限（9 段）。
    if san.path("a/b/c/d/e/f/g/h/i") != Err(E_DEPTH) {
        return Err(E_DEPTH);
    }
    // 错误路径 2：段长超限（33 字节段）。
    let long_seg = "a/".to_string() + &"x".repeat(33);
    if san.path(&long_seg) != Err(E_SEG_LEN) {
        return Err(E_SEG_LEN);
    }
    // 错误路径 3：总长超限（193 字节）。
    let long_path: String = "x".repeat(PATH_LEN + 1);
    if san.path(&long_path) != Err(E_PATH_LEN) {
        return Err(E_PATH_LEN);
    }
    // 主路径 2：索引解析。
    let s = ContentSlice { base: 10, count: 20, cap: 100 };
    if s.resolve()? != (10, 30) {
        return Err(E_INDEX);
    }
    // 错误路径 4：base+count 溢出。
    let s2 = ContentSlice {
        base: u32::MAX,
        count: 1,
        cap: u32::MAX,
    };
    if s2.resolve() != Err(E_INDEX) {
        return Err(E_INDEX);
    }
    // 错误路径 5：越容量。
    let s3 = ContentSlice { base: 99, count: 2, cap: 100 };
    if s3.resolve() != Err(E_INDEX) {
        return Err(E_INDEX);
    }
    // 主路径 3：脱敏。
    if san.field("token", "abc123")? != "xxxxxx" {
        return Err(E_AUDIT);
    }
    if san.field("color", "red")? != "red" {
        return Err(E_AUDIT);
    }
    if san.field("SECRET_KEY", "top")? != "xxx" {
        return Err(E_AUDIT);
    }
    // 主路径 4：审计守恒（6 次路径 + 3 次字段 = 9 条）。
    san.audit()?;
    if san.audited() != 9 {
        return Err(E_AUDIT);
    }
    let view = san.audit.view();
    if view.len() != 9 {
        return Err(E_AUDIT);
    }
    // 前缀判定闭集：key/token/secret 前缀命中（大小写不敏感）。
    if !is_sensitive("key") || !is_sensitive("TokenBox") || !is_sensitive("SECRET_KEY") {
        return Err(E_AUDIT);
    }
    if is_sensitive("color") {
        return Err(E_AUDIT);
    }
    // 错误路径 6：空字段名脱敏。
    if redact("", "v") != Err(E_AUDIT) {
        return Err(E_AUDIT);
    }
    Ok(0)
}
