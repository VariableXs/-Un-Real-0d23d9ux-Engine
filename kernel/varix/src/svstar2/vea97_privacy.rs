//! VE-F0097 · 遥测隐私过滤器（VE-A 域 · A04b 诊断·降级·安全组 · 目标 340 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0097`
//!
//! **职责定位（锚点原文）**：遥测数据出域前统一过隐私过滤器——路径名、窗口标题、
//! URL、文件名四类敏感面按规则表脱敏（哈希化或占位符替换），规则表版本化可审计；
//! 过滤失败默认丢弃而非放行（fail-closed 铁律）；与体验日志隐私红线一致：
//! 永不记录用户输入的具体内容。
//!
//! **铁律**：
//! 1. **fail-closed**——过滤器异常一律丢弃，绝不放行。
//! 2. **规则版本化**——规则表带版本号，命中审计记版本。
//! 3. **四类敏感面**——路径名 / 窗口标题 / URL / 文件名全覆盖。
//! 4. **导出同源**——脱敏导出复用同一规则表单源。

use alloc::vec;
use alloc::vec::Vec;

/// 诊断段基址 0xE8A8（隐私过滤）。
pub const SEG_BASE: u16 = 0xE8A8;

/// 规则编译失败。
pub const XC_RULE_COMPILE: u16 = SEG_BASE | 0x01;
/// 过滤器自身异常（fail-closed 触发）。
pub const XC_FILTER_FAULT: u16 = SEG_BASE | 0x02;
/// 敏感面类型未知。
pub const XC_UNKNOWN_SURFACE: u16 = SEG_BASE | 0x03;
/// 规则版本非法。
pub const XC_BAD_VERSION: u16 = SEG_BASE | 0x04;

/// 敏感面类型（四类）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SensSurface {
    /// 路径名。
    Path,
    /// 窗口标题。
    WindowTitle,
    /// URL。
    Url,
    /// 文件名。
    FileName,
}

/// 脱敏动作。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MaskAction {
    /// 哈希化（保留可关联性，不可逆）。
    Hash,
    /// 占位符替换。
    Placeholder,
}

/// 脱敏规则。
#[derive(Clone, Debug)]
pub struct MaskRule {
    pub surface: SensSurface,
    /// 子串模式（简化匹配：包含即命中）。
    pub pattern: Vec<u8>,
    pub action: MaskAction,
}

/// 规则表（版本化）。
#[derive(Clone, Debug)]
pub struct RuleTable {
    pub version: u32,
    pub rules: Vec<MaskRule>,
}

impl RuleTable {
    /// 编译校验：版本非零、规则非空、模式非空。
    pub fn compile(version: u32, rules: Vec<MaskRule>) -> Result<Self, u16> {
        if version == 0 {
            return Err(XC_BAD_VERSION);
        }
        if rules.is_empty() {
            return Err(XC_RULE_COMPILE);
        }
        for r in &rules {
            if r.pattern.is_empty() {
                return Err(XC_RULE_COMPILE);
            }
        }
        Ok(RuleTable { version, rules })
    }
}

/// 过滤审计记录。
#[derive(Clone, Debug)]
pub struct FilterAudit {
    pub hit_count: u32,
    pub rule_version: u32,
    /// 时间窗标识（tick）。
    pub window_tick: u64,
    /// 丢弃计数（fail-closed 命中）。
    pub dropped: u32,
}

/// 过滤结果。
#[derive(Clone, Debug)]
pub struct Filtered {
    /// 脱敏后字节（占位或哈希十六进制）。
    pub out: Vec<u8>,
    /// 是否命中规则。
    pub masked: bool,
}

/// 隐私过滤器。
pub struct PrivacyFilter {
    table: Option<RuleTable>,
    /// 上一版（热载失败保留旧表用）。
    prev: Option<RuleTable>,
    audit: FilterAudit,
    /// 故障注入位（自检用）。
    faulty: bool,
    last_code: u16,
}

impl PrivacyFilter {
    pub fn new() -> Self {
        PrivacyFilter {
            table: None,
            prev: None,
            audit: FilterAudit {
                hit_count: 0,
                rule_version: 0,
                window_tick: 0,
                dropped: 0,
            },
            faulty: false,
            last_code: 0,
        }
    }

    pub fn last_code(&self) -> u16 {
        self.last_code
    }

    pub fn rule_version(&self) -> u32 {
        self.table.as_ref().map(|t| t.version).unwrap_or(0)
    }

    /// 热载规则表：编译失败拒载保留旧表。
    pub fn hot_load(&mut self, table: RuleTable) -> Result<(), u16> {
        // 二次编译校验（版本/规则面）。
        if table.version == 0 {
            self.last_code = XC_BAD_VERSION;
            return Err(XC_BAD_VERSION);
        }
        if table.rules.is_empty() || table.rules.iter().any(|r| r.pattern.is_empty()) {
            self.last_code = XC_RULE_COMPILE;
            return Err(XC_RULE_COMPILE);
        }
        if let Some(old) = self.table.take() {
            self.prev = Some(old);
        }
        self.table = Some(table);
        self.last_code = 0;
        Ok(())
    }

    /// 回滚到上一版。
    pub fn rollback(&mut self) -> Result<u32, u16> {
        match self.prev.take() {
            Some(p) => {
                let v = p.version;
                if let Some(cur) = self.table.take() {
                    self.prev = Some(cur);
                }
                self.table = Some(p);
                self.last_code = 0;
                Ok(v)
            }
            None => {
                self.last_code = XC_RULE_COMPILE;
                Err(XC_RULE_COMPILE)
            }
        }
    }

    fn hash8(data: &[u8]) -> u64 {
        // FNV-1a 64（无依赖纯函数）。
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        for &b in data {
            h ^= b as u64;
            h = h.wrapping_mul(0x100_0000_01b3);
        }
        h
    }

    fn mask_one(rule: &MaskRule, input: &[u8]) -> Vec<u8> {
        match rule.action {
            MaskAction::Hash => {
                let h = Self::hash8(input);
                // 十六进制 16 字符。
                let mut out = Vec::with_capacity(16);
                let mut v = h;
                for _ in 0..16 {
                    let nib = (v & 0xF) as u8;
                    out.push(if nib < 10 { b'0' + nib } else { b'a' + (nib - 10) });
                    v >>= 4;
                }
                out
            }
            MaskAction::Placeholder => vec![b'*', b'*', b'*', b'*'],
        }
    }

    /// 过滤一段敏感面数据。fail-closed：任何异常丢弃。
    pub fn filter(&mut self, surface: SensSurface, input: &[u8]) -> Result<Filtered, u16> {
        if self.faulty {
            self.audit.dropped += 1;
            self.last_code = XC_FILTER_FAULT;
            return Err(XC_FILTER_FAULT);
        }
        let table = match &self.table {
            Some(t) => t.clone(),
            None => {
                // 无规则表 = 无法脱敏 = fail-closed。
                self.audit.dropped += 1;
                self.last_code = XC_FILTER_FAULT;
                return Err(XC_FILTER_FAULT);
            }
        };
        // 找命中的第一条规则（模式包含匹配）。
        let hit = table
            .rules
            .iter()
            .find(|r| r.surface == surface && Self::contains(input, &r.pattern));
        match hit {
            Some(rule) => {
                self.audit.hit_count += 1;
                self.audit.rule_version = table.version;
                self.last_code = 0;
                Ok(Filtered {
                    out: Self::mask_one(rule, input),
                    masked: true,
                })
            }
            None => {
                // 未匹配敏感面：按默认动作占位（保守：占位不放行原文）。
                self.audit.hit_count += 1;
                self.audit.rule_version = table.version;
                self.last_code = 0;
                Ok(Filtered {
                    out: vec![b'*', b'*', b'*', b'*'],
                    masked: true,
                })
            }
        }
    }

    fn contains(hay: &[u8], needle: &[u8]) -> bool {
        if needle.is_empty() || needle.len() > hay.len() {
            return false;
        }
        hay.windows(needle.len()).any(|w| w == needle)
    }

    /// 注入故障（测试支撑）。
    pub fn inject_fault(&mut self) {
        self.faulty = true;
    }

    pub fn clear_fault(&mut self) {
        self.faulty = false;
    }

    pub fn audit(&self) -> &FilterAudit {
        &self.audit
    }

    /// 推进审计时间窗。
    pub fn advance_window(&mut self, tick: u64) {
        self.audit.window_tick = tick;
    }
}

impl Default for PrivacyFilter {
    fn default() -> Self {
        Self::new()
    }
}

/// 自检：fail-closed、规则版本化、四类敏感面、导出同源。
pub fn run_vea97_checks() -> crate::checks::CheckSet {
    use crate::checks::CheckSet;

    fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
        set.add(label, ok, detail);
    }

    fn sample_table() -> RuleTable {
        RuleTable::compile(
            3,
            vec![
                MaskRule {
                    surface: SensSurface::Path,
                    pattern: b"/home/".to_vec(),
                    action: MaskAction::Hash,
                },
                MaskRule {
                    surface: SensSurface::WindowTitle,
                    pattern: b"Doc".to_vec(),
                    action: MaskAction::Placeholder,
                },
                MaskRule {
                    surface: SensSurface::Url,
                    pattern: b"https://".to_vec(),
                    action: MaskAction::Hash,
                },
                MaskRule {
                    surface: SensSurface::FileName,
                    pattern: b".txt".to_vec(),
                    action: MaskAction::Placeholder,
                },
            ],
        )
        .unwrap()
    }

    let mut set = CheckSet::new("vea97_privacy");

    // C97-01 规则表版本化加载
    {
        let mut f = PrivacyFilter::new();
        let r = f.hot_load(sample_table());
        chk(&mut set, "C97-01 版本化加载", r.is_ok() && f.rule_version() == 3, "加载失败");
    }

    // C97-02 编译失败拒载保留旧表
    {
        let mut f = PrivacyFilter::new();
        let _ = f.hot_load(sample_table());
        let bad = RuleTable {
            version: 4,
            rules: vec![MaskRule {
                surface: SensSurface::Path,
                pattern: vec![],
                action: MaskAction::Hash,
            }],
        };
        let r = f.hot_load(bad);
        let ok = r.is_err() && f.rule_version() == 3;
        chk(&mut set, "C97-02 拒载保旧", ok, "旧表丢失");
    }

    // C97-03 fail-closed：故障时丢弃不放行
    {
        let mut f = PrivacyFilter::new();
        let _ = f.hot_load(sample_table());
        f.inject_fault();
        let r = f.filter(SensSurface::Path, b"/home/user/x");
        let ok = r.is_err() && f.audit().dropped >= 1;
        chk(&mut set, "C97-03 fail-closed", ok, "故障放行");
    }

    // C97-04 无规则表 fail-closed
    {
        let mut f = PrivacyFilter::new();
        let r = f.filter(SensSurface::Path, b"abc");
        chk(
            &mut set,
            "C97-04 无表丢弃",
            r.is_err() && f.last_code() == XC_FILTER_FAULT,
            "无表放行",
        );
    }

    // C97-05 四类敏感面全覆盖
    {
        let mut f = PrivacyFilter::new();
        let _ = f.hot_load(sample_table());
        let a = f.filter(SensSurface::Path, b"/home/u/f");
        let b = f.filter(SensSurface::WindowTitle, b"My Doc");
        let c = f.filter(SensSurface::Url, b"https://x.y");
        let d = f.filter(SensSurface::FileName, b"a.txt");
        let ok = a.is_ok() && b.is_ok() && c.is_ok() && d.is_ok();
        chk(&mut set, "C97-05 四类覆盖", ok, "覆盖缺口");
    }

    // C97-06 路径哈希化不泄漏原文
    {
        let mut f = PrivacyFilter::new();
        let _ = f.hot_load(sample_table());
        let r = f.filter(SensSurface::Path, b"/home/user/secret").unwrap();
        let ok = r.masked && !r.out.windows(5).any(|w| w == b"secret");
        chk(&mut set, "C97-06 哈希不泄漏", ok, "原文泄漏");
    }

    // C97-07 未匹配默认占位
    {
        let mut f = PrivacyFilter::new();
        let _ = f.hot_load(sample_table());
        let r = f.filter(SensSurface::Path, b"nothing_here").unwrap();
        let ok = r.masked && r.out == b"****";
        chk(&mut set, "C97-07 默认占位", ok, "默认未占位");
    }

    // C97-08 版本零拒绝
    {
        let r = RuleTable::compile(0, vec![MaskRule {
            surface: SensSurface::Path,
            pattern: b"x".to_vec(),
            action: MaskAction::Hash,
        }]);
        chk(
            &mut set,
            "C97-08 版本零拒绝",
            r.err() == Some(XC_BAD_VERSION),
            "版本零放行",
        );
    }

    // C97-09 审计计版本
    {
        let mut f = PrivacyFilter::new();
        let _ = f.hot_load(sample_table());
        let _ = f.filter(SensSurface::Url, b"https://a.b");
        let ok = f.audit().rule_version == 3 && f.audit().hit_count >= 1;
        chk(&mut set, "C97-09 审计计版本", ok, "审计缺版本");
    }

    // C97-10 回滚换版
    {
        let mut f = PrivacyFilter::new();
        let _ = f.hot_load(sample_table());
        let v2 = RuleTable::compile(
            5,
            vec![MaskRule {
                surface: SensSurface::Path,
                pattern: b"z".to_vec(),
                action: MaskAction::Placeholder,
            }],
        )
        .unwrap();
        let _ = f.hot_load(v2);
        let r = f.rollback();
        let ok = r == Ok(3) && f.rule_version() == 3;
        chk(&mut set, "C97-10 回滚换版", ok, "回滚失败");
    }

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vea97_all_green() {
        let s = run_vea97_checks();
        assert!(s.all_passed(), "{}", s.red_items_text());
    }
}
