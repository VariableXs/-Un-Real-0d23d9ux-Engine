//! CGPU-F0362 · 配方开放格式（判据段 0x0910）
//! 开放格式：配方 JSON 规范（开放可手编——14 章不给用户上锁）；
//! 字段版本化（演进兼容）；社区配方通道（导出导入分发）；安全钳
//! 制（导入配方 schema 校验+上限钳制——恶意配方无害化）。判据：
//! 开放规范、版本化、分发、安全钳制、判据。
//!
//! 锚点原文摘录（施工书 1611 行）：
//! 「配方 JSON 规范（开放可手编）；字段版本化（演进兼容）；社区
//! 配方通道（导出导入分发）；安全钳制（导入配方 schema 校验+上限
//! 钳制——恶意配方无害化）。判据：开放规范、版本化、分发、安全钳
//! 制、判据。」
//!
//! 诊断码段 0x0910（本模块独占：0x0910~0x091F）。
//! no_std + alloc 零 unwrap（测试除外），全部 Result 化。

use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

/// 诊断码段基址。
pub const SEG_BASE: u16 = 0x0910;

/// 格式版本（判据「版本化」锚点）：v1 = 1。
pub const FORMAT_VERSION: u32 = 1;

/// 兼容读入的最低版本（低于即拒——不知如何解释旧字段）。
pub const FORMAT_VERSION_MIN: u32 = 1;

/// 配方名长度上限（字节）。
pub const NAME_MAX_BYTES: usize = 32;

/// 单字段份额上限（判据「安全钳制」钳制点）：任何份额 ≤ 1000。
pub const SHARE_CLAMP_MAX: u32 = 1000;

/// 配方字段（判据「开放规范」的字段集——v1 冻结七字段）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct RecipeFields {
    /// 配方名（≤32 字节）。
    pub name: String,
    /// 格式版本。
    pub version: u32,
    /// 配方族号（1=视频 2=网页 3=全屏——与 F0357~59 对应）。
    pub kind: u32,
    /// 三账户份额（解码, 渲染, 音频）千分比。
    pub accounts: (u32, u32, u32),
    /// 是否允许画中画/多路。
    pub multi_lane: bool,
}

impl RecipeFields {
    /// 空字段（默认名）。
    pub fn blank() -> Self {
        RecipeFields {
            name: String::from("unnamed"),
            version: FORMAT_VERSION,
            kind: 1,
            accounts: (400, 400, 200),
            multi_lane: false,
        }
    }
    /// schema 校验（判据「安全钳制」第一道闸）。
    pub fn validate(&self) -> Result<(), u16> {
        if self.name.is_empty() || self.name.len() > NAME_MAX_BYTES {
            return Err(SEG_BASE);
        }
        if self.version < FORMAT_VERSION_MIN || self.version > FORMAT_VERSION {
            return Err(SEG_BASE + 1);
        }
        if !(1..=3).contains(&self.kind) {
            return Err(SEG_BASE + 2);
        }
        let (a, b, c) = self.accounts;
        if a + b + c != 1000 {
            return Err(SEG_BASE + 3);
        }
        Ok(())
    }
    /// 上限钳制（判据「安全钳制」第二道闸）：越界份额逐项钳到
    /// SHARE_CLAMP_MAX，并按比例回配其余两项保守恒。
    pub fn clamp_inplace(&mut self) {
        let (a, b, c) = self.accounts;
        let clamped = |v: u32| v.min(SHARE_CLAMP_MAX);
        let (na, nb, nc) = (clamped(a), clamped(b), clamped(c));
        let sum = na + nb + nc;
        if sum != 1000 {
            // 差额按未钳比例回摊到最大项（确定性归口）。
            let diff = 1000i64 - sum as i64;
            let maxv = na.max(nb).max(nc);
            let fixed = if maxv == na {
                (na as i64 + diff).max(0) as u32
            } else if maxv == nb {
                (nb as i64 + diff).max(0) as u32
            } else {
                (nc as i64 + diff).max(0) as u32
            };
            self.accounts = if maxv == na { (fixed, nb, nc) } else if maxv == nb { (na, fixed, nc) } else { (na, nb, fixed) };
        } else {
            self.accounts = (na, nb, nc);
        }
        // 钳后仍不守恒（极端输入）→ 回落出厂默认（无害化终点）。
        if self.accounts.0 + self.accounts.1 + self.accounts.2 != 1000 {
            self.accounts = (400, 400, 200);
        }
    }
}

/// 序列化（判据「开放规范」文本面）：确定性键序输出（手编友好、
/// 双跑逐位同）。转义：反斜杠与引号（最小转义集）。
pub fn serialize(f: &RecipeFields) -> Result<String, u16> {
    f.validate()?;
    let esc = |s: &str| -> String {
        let mut out = String::new();
        for ch in s.chars() {
            if ch == '\\' || ch == '"' {
                out.push('\\');
            }
            out.push(ch);
        }
        out
    };
    Ok(alloc::format!(
        "{{\"version\":{},\"kind\":{},\"name\":\"{}\",\"accounts\":[{},{},{}],\"multi_lane\":{}}}",
        f.version,
        f.kind,
        esc(&f.name),
        f.accounts.0,
        f.accounts.1,
        f.accounts.2,
        f.multi_lane
    ))
}

/// 反序列化（判据「分发」入口）：字段解析 + schema 校验 + 钳制，
/// 三步走完才算导入成功。
pub fn deserialize(text: &str) -> Result<RecipeFields, u16> {
    let mut out = parse_raw(text)?;
    // 分发通道安全钳制：先钳后验（钳不掉的才拒）。
    out.clamp_inplace();
    out.validate()?;
    Ok(out)
}

/// 原始解析（判据「分发」第一步）：只解析不钳制——钳制时点由调用方
/// 决定（通道计数需要钳前态）。accounts 数组先整体摘出再解析，
/// 避免顶层逗号分裂破坏数组。
fn parse_raw(text: &str) -> Result<RecipeFields, u16> {
    let t = text.trim();
    if !t.starts_with('{') || !t.ends_with('}') {
        return Err(SEG_BASE + 4);
    }
    let body = &t[1..t.len() - 1];
    let mut out = RecipeFields::blank();
    let mut found_accounts = false;
    // 摘数组：定位 "accounts" 键，取 [..] 整段，余文挖除该键后逐键扫。
    let mut rest = body;
    // 挖除缓冲（提升到函数域——rest 可能转指它）。
    let mut cleaned = String::new();
    if let Some(kpos) = body.find("\"accounts\"") {
        let tail = &body[kpos..];
        let lbr = tail.find('[').ok_or(SEG_BASE + 8)?;
        let rbr = tail[lbr..].find(']').ok_or(SEG_BASE + 8)? + lbr;
        let inner = &tail[lbr + 1..rbr];
        let mut it = inner.split(',');
        let a = it.next().ok_or(SEG_BASE + 8)?.trim().parse::<u32>().map_err(|_| SEG_BASE + 8)?;
        let b = it.next().ok_or(SEG_BASE + 8)?.trim().parse::<u32>().map_err(|_| SEG_BASE + 8)?;
        let c = it.next().ok_or(SEG_BASE + 8)?.trim().parse::<u32>().map_err(|_| SEG_BASE + 8)?;
        out.accounts = (a, b, c);
        found_accounts = true;
        let after = tail[rbr + 1..].trim_start();
        let after2 = after.strip_prefix(',').unwrap_or(after);
        cleaned.push_str(&body[..kpos]);
        cleaned.push_str(after2);
        rest = cleaned.trim();
        if rest.ends_with(',') {
            rest = &rest[..rest.len() - 1];
        }
        if rest.starts_with(',') {
            rest = &rest[1..];
        }
    }
    // 逐键扫描（no_std 手写解析——开放格式不依赖外部解析器）。
    for seg in rest.split(',') {
        let seg = seg.trim();
        if seg.is_empty() {
            continue;
        }
        let (k, v) = match seg.split_once(':') {
            Some(kv) => kv,
            None => return Err(SEG_BASE + 5),
        };
        let k = k.trim().trim_matches('"');
        let v = v.trim();
        match k {
            "version" => out.version = v.parse::<u32>().map_err(|_| SEG_BASE + 6)?,
            "kind" => out.kind = v.parse::<u32>().map_err(|_| SEG_BASE + 7)?,
            "name" => out.name = String::from(v.trim_matches('"')),
            "multi_lane" => out.multi_lane = v == "true",
            "accounts" => {
                // 已在数组摘除阶段处理；重复键视为 schema 拒。
                return Err(SEG_BASE + 9);
            }
            _ => return Err(SEG_BASE + 9),
        }
    }
    if !found_accounts {
        return Err(SEG_BASE + 10);
    }
    Ok(out)
}

/// 社区通道账（判据「分发」的通道面）：导入/导出双向计数 + 拒收
/// 归因（哪个配方、为何被钳/被拒）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct ChannelLedger {
    /// 导入成功数。
    pub imported: u32,
    /// 导出数。
    pub exported: u32,
    /// 钳制数（无害化成功）。
    pub clamped: u32,
    /// 拒收数（schema 不可救）。
    pub rejected: u32,
}

impl ChannelLedger {
    /// 空账。
    pub fn new() -> Self {
        ChannelLedger { imported: 0, exported: 0, clamped: 0, rejected: 0 }
    }
    /// 导入一笔（判据「分发 + 安全钳制」联动主判据）。
    pub fn import(&mut self, text: &str) -> Result<RecipeFields, u16> {
        match parse_raw(text) {
            Ok(mut f) => {
                // 钳前留影：钳前≠钳后 = 该配方被无害化（判据「安全钳制」联动）。
                let before = f.accounts;
                f.clamp_inplace();
                if f.accounts != before {
                    self.clamped += 1;
                }
                // schema 校验失败同样计入拒收（拒收归因完整性）。
                match f.validate() {
                    Ok(()) => {
                        self.imported += 1;
                        Ok(f)
                    }
                    Err(e) => {
                        self.rejected += 1;
                        Err(e)
                    }
                }
            }
            Err(e) => {
                self.rejected += 1;
                Err(e)
            }
        }
    }
    /// 导出一笔。
    pub fn export(&mut self, f: &RecipeFields) -> Result<String, u16> {
        let s = serialize(f)?;
        self.exported += 1;
        Ok(s)
    }
    /// 往返守恒（判据「分发」守卫）：导出→导入 必须还原同字段。
    pub fn roundtrip_ok(f: &RecipeFields) -> Result<bool, u16> {
        let s = serialize(f)?;
        let back = deserialize(&s)?;
        Ok(back == *f)
    }
}

impl Default for ChannelLedger {
    fn default() -> Self {
        Self::new()
    }
}

/// 版本演进核（判据「版本化」守卫）：v1 唯一可读；假想 v2 到来时
/// 字段只增不改（新字段缺省即可读 v1）——这里冻结 v1 字段集签名。
pub fn v1_field_set_fp() -> u64 {
    let mut acc: u64 = 0xcbf29ce484222325;
    for key in ["version", "kind", "name", "accounts", "multi_lane"] {
        for b in key.bytes() {
            acc = acc.wrapping_mul(0x100000001b3) ^ b as u64;
        }
    }
    acc
}

/// 开放格式诊断明细（错误码 → 中文可操作建议）。
pub fn diagnose(code: u16) -> &'static str {
    let d = code.wrapping_sub(SEG_BASE);
    match d {
        0 => "配方名为空或超 32 字节",
        1 => "版本越界（只认 v1）",
        2 => "配方族号必须 1~3",
        3 => "三账户和≠1000",
        4 => "非对象文本（缺花括号）",
        5 => "键值缺冒号",
        6 => "version 字段非数字",
        7 => "kind 字段非数字",
        8 => "accounts 必须是三元素数组",
        9 => "未知键——开放格式拒绝私造字段",
        10 => "缺 accounts 字段",
        _ => "未知诊断码——查本模块 SEG_BASE 0x0910",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serialize_deserialize_roundtrip() {
        let mut f = RecipeFields::blank();
        f.name = String::from("movie-night");
        f.kind = 1;
        let s = serialize(&f).unwrap();
        assert!(s.contains("\"kind\":1"));
        let back = deserialize(&s).unwrap();
        assert_eq!(back, f);
        assert!(ChannelLedger::roundtrip_ok(&f).unwrap());
        assert!(v1_field_set_fp() != 0);
    }

    #[test]
    fn malicious_clamped() {
        let mut f = RecipeFields::blank();
        f.accounts = (5000, 0, 0);
        f.clamp_inplace();
        // 钳后守恒：总和回 1000 且单顶不超 1000。
        let (a, b, c) = f.accounts;
        assert!(a <= SHARE_CLAMP_MAX && a + b + c == 1000);
        // 导入侧联动：钳制计数。
        let mut led = ChannelLedger::new();
        let got = led.import("{\"version\":1,\"kind\":1,\"name\":\"x\",\"accounts\":[900,900,900],\"multi_lane\":false}").unwrap();
        assert_eq!(led.clamped, 1);
        assert_eq!(got.accounts.0 + got.accounts.1 + got.accounts.2, 1000);
    }

    #[test]
    fn rejects_attribution() {
        let mut led = ChannelLedger::new();
        assert!(led.import("not json").is_err());
        assert!(led.import("{\"version\":2,\"kind\":1,\"name\":\"x\",\"accounts\":[1,1,998],\"multi_lane\":false}").is_err());
        assert!(led.import("{\"version\":1,\"kind\":9,\"name\":\"x\",\"accounts\":[1,1,998],\"multi_lane\":false}").is_err());
        assert!(led.import("{\"version\":1,\"kind\":1,\"name\":\"x\",\"accounts\":[1,1,998],\"zork\":1}").is_err());
        assert_eq!(led.rejected, 4);
        assert!(diagnose(SEG_BASE + 9).contains("私造"));
        // 空名拒绝。
        let mut f = RecipeFields::blank();
        f.name = String::new();
        assert_eq!(f.validate().unwrap_err(), SEG_BASE);
    }
}
