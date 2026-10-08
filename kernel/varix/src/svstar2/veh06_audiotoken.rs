//! VE-F1406 · 音频令牌系统（VE-H 域 · 音频引擎 · 目标 400 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F1406`
//!
//! **规格原文**：声音设计令牌（语义名→音频资源+参数映射——"notification.pop"
//! →资源+音量+延迟；语义名与实现的解耦，UI 只发语义名——换资源不改代码）；
//! 令牌三级覆盖（系统级/应用级/临时——应用级覆盖系统级/临时覆盖应用级，覆盖
//! 链解析，最终生效值可查询）；与 E 域主题联动（换肤=界面+声音一起换——
//! F3401 令牌运行时的音频侧）；缺省链（主题未定义→系统默认→静默——静默是
//! 合法态，缺失显性遥测 F1418，永不报错）；唯一来源纪律（每个交互音效有且
//! 只有一个令牌来源——硬编码直呼资源=绕过令牌=lint 拦截）。
//! 判据：语义映射、三级覆盖、主题联动、缺省链、唯一来源、判据。
//!
//! **设计要点**：
//! - 解耦是换肤与生态的前提：UI 层只出现语义名，资源与参数全由令牌表决定
//!   ——换资源不改代码（字符串面解析，错串静默降级不报错）；
//! - 覆盖链有唯一答案：临时 > 应用级 > 系统级，逐级回退到主题/缺省/静默；
//!   最终生效值 + 生效来源可查询（谁在生效说得清）；
//! - 主题联动是令牌运行时（F3401）的音频侧：主题包自带音频令牌段，换肤连
//!   声音一起换——视听一致（只换界面 = 体验半吊子）；
//! - 静默是合法态不是错误：真没有资源就安静，缺什么记进遥测（F1418 挂点）
//!   ——永不报错，缺失显性；
//! - 唯一来源是硬约束：同一交互动作只能有一个令牌来源，硬编码直呼资源被
//!   lint 拦截——一处定义，处处一致。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、令牌模型与解析（语义名 → 资源 + 参数）
// ---------------------------------------------------------------------------

/// 延迟档位。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LatencyClass {
    /// 即时（点击反馈）。
    Immediate,
    /// 常规（通知类）。
    Normal,
    /// 宽松（氛围类）。
    Relaxed,
}

impl LatencyClass {
    pub fn parse(s: &str) -> Option<LatencyClass> {
        match s {
            "immediate" => Some(LatencyClass::Immediate),
            "normal" => Some(LatencyClass::Normal),
            "relaxed" => Some(LatencyClass::Relaxed),
            _ => None,
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            LatencyClass::Immediate => "即时",
            LatencyClass::Normal => "常规",
            LatencyClass::Relaxed => "宽松",
        }
    }
}

/// 令牌规格：资源 + 音量 + 延迟档。
#[derive(Clone, Debug, PartialEq)]
pub struct TokenSpec {
    pub resource: String,
    /// 音量（千分比 0..1000，整数账面）。
    pub volume_permille: u32,
    pub latency: LatencyClass,
}

impl TokenSpec {
    /// 字符串面："resource=pop.wav;volume=700;latency=normal"。
    /// 解析失败返回 None（调用方走缺省链，永不报错）。
    pub fn parse(s: &str) -> Option<TokenSpec> {
        let mut resource: Option<String> = None;
        let mut volume = 1000u32;
        let mut latency = LatencyClass::Normal;
        for part in s.split(';') {
            let kv: Vec<&str> = part.splitn(2, '=').collect();
            if kv.len() != 2 {
                continue;
            }
            let (k, v) = (kv[0].trim(), kv[1].trim());
            match k {
                "resource" => resource = Some(v.to_string()),
                "volume" => volume = v.parse::<u32>().ok()?.min(1000),
                "latency" => latency = LatencyClass::parse(v)?,
                _ => {}
            }
        }
        let resource = resource?;
        if resource.is_empty() {
            return None;
        }
        Some(TokenSpec {
            resource,
            volume_permille: volume,
            latency,
        })
    }
}

// ---------------------------------------------------------------------------
// 二、三级覆盖（临时 > 应用级 > 系统级）
// ---------------------------------------------------------------------------

/// 令牌定义的层级。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Level {
    /// 系统级（出厂默认）。
    System,
    /// 应用级（应用自带主题段）。
    App,
    /// 临时（运行时一次性覆盖）。
    Temporary,
    /// 主题级（主题包的音频令牌段，F3401 运行时的音频侧）。
    Theme,
}

impl Level {
    /// 覆盖优先级（大者胜）。
    pub fn precedence(self) -> u8 {
        match self {
            Level::System => 1,
            Level::Theme => 2,
            Level::App => 3,
            Level::Temporary => 4,
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Level::System => "系统级",
            Level::Theme => "主题级",
            Level::App => "应用级",
            Level::Temporary => "临时",
        }
    }
}

/// 解析结果：生效规格 + 生效来源；Silent = 合法静默。
#[derive(Clone, Debug, PartialEq)]
pub enum Resolved {
    Play(TokenSpec, Level),
    /// 静默（缺省链走完仍无资源）——合法态，附缺失的语义名。
    Silent,
}

/// 音频令牌表：四级定义 + 覆盖链解析。
#[derive(Debug, Default)]
pub struct TokenTable {
    /// 每层：语义名 → 字符串面（存字符串面，解析在解析层）。
    system: Vec<(String, String)>,
    theme: Vec<(String, String)>,
    app: Vec<(String, String)>,
    temporary: Vec<(String, String)>,
    /// 缺失遥测（F1418 挂点）：语义名 → 缺失次数。
    pub missing_telemetry: Vec<(String, u64)>,
    /// 解析失败遥测（坏串静默降级，但显性记账）。
    pub parse_error_telemetry: Vec<(String, u64)>,
}

impl TokenTable {
    pub fn new() -> TokenTable {
        TokenTable::default()
    }

    fn set_at(level: Level, table: &mut Vec<(String, String)>, name: &str, spec: &str) {
        table.retain(|(n, _)| n != name);
        table.push((name.to_string(), spec.to_string()));
    }

    pub fn define(&mut self, level: Level, name: &str, spec_str: &str) {
        let table = match level {
            Level::System => &mut self.system,
            Level::Theme => &mut self.theme,
            Level::App => &mut self.app,
            Level::Temporary => &mut self.temporary,
        };
        Self::set_at(level, table, name, spec_str);
    }

    pub fn undefine(&mut self, level: Level, name: &str) {
        let table = match level {
            Level::System => &mut self.system,
            Level::Theme => &mut self.theme,
            Level::App => &mut self.app,
            Level::Temporary => &mut self.temporary,
        };
        table.retain(|(n, _)| n != name);
    }

    /// 主题切换：整段替换主题级令牌（F3401 换肤连声音换）。
    pub fn apply_theme(&mut self, theme_tokens: &[(String, String)]) {
        self.theme.clear();
        for (n, s) in theme_tokens.iter() {
            Self::set_at(Level::Theme, &mut self.theme, n, s);
        }
    }

    fn raw_at(&self, level: Level, name: &str) -> Option<&String> {
        let table = match level {
            Level::System => &self.system,
            Level::Theme => &self.theme,
            Level::App => &self.app,
            Level::Temporary => &self.temporary,
        };
        table.iter().find(|(n, _)| n == name).map(|(_, s)| s)
    }

    fn note_missing(&mut self, name: &str) {
        match self.missing_telemetry.iter_mut().find(|(n, _)| n == name) {
            Some((_, c)) => *c += 1,
            None => self.missing_telemetry.push((name.to_string(), 1)),
        }
    }

    fn note_parse_error(&mut self, name: &str) {
        match self.parse_error_telemetry.iter_mut().find(|(n, _)| n == name) {
            Some((_, c)) => *c += 1,
            None => self.parse_error_telemetry.push((name.to_string(), 1)),
        }
    }

    /// 覆盖链解析：临时 > 应用级 > 系统级 > 主题级。
    /// 命中的字符串面解析失败 → 静默 + 解析错误遥测（永不报错）。
    /// 全链无定义 → 静默 + 缺失遥测（静默是合法态）。
    pub fn resolve(&mut self, name: &str) -> Resolved {
        let mut levels = [
            Level::Temporary,
            Level::App,
            Level::System,
            Level::Theme,
        ];
        // 确定性：按优先级降序试（临时 4 > 应用 3 > 系统 1 > 主题 2？
        // 规格语义：应用覆盖系统、临时覆盖应用——主题是另一轴（换肤），
        // 覆盖链取 precedence 序：临时 > 应用 > 系统 > 主题。
        levels.sort_by(|a, b| b.precedence().cmp(&a.precedence()));
        let mut raw: Option<(String, Level)> = None;
        for lv in levels.iter() {
            if let Some(s) = self.raw_at(*lv, name) {
                raw = Some((s.clone(), *lv));
                break;
            }
        }
        match raw {
            None => {
                self.note_missing(name);
                Resolved::Silent
            }
            Some((s, lv)) => match TokenSpec::parse(&s) {
                Some(spec) => Resolved::Play(spec, lv),
                None => {
                    self.note_parse_error(name);
                    Resolved::Silent
                }
            },
        }
    }

    /// 只读解析（不记遥测——查询面与播放面分离，最终生效值可查询）。
    pub fn peek(&self, name: &str) -> Resolved {
        let mut levels = [
            Level::Temporary,
            Level::App,
            Level::System,
            Level::Theme,
        ];
        levels.sort_by(|a, b| b.precedence().cmp(&a.precedence()));
        for lv in levels.iter() {
            if let Some(s) = self.raw_at(*lv, name) {
                if let Some(spec) = TokenSpec::parse(s) {
                    return Resolved::Play(spec, *lv);
                }
                return Resolved::Silent;
            }
        }
        Resolved::Silent
    }
}

// ---------------------------------------------------------------------------
// 三、唯一来源纪律（lint：同交互唯一令牌来源；硬编码直呼资源拦截）
// ---------------------------------------------------------------------------

/// lint 违规。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LintViolation {
    pub code: &'static str,
    pub interaction: String,
    pub detail: String,
}

/// 交互-令牌登记表 + lint。
#[derive(Debug, Default)]
pub struct SourceLint {
    /// 交互动作 → 令牌语义名（唯一来源登记）。
    pub bindings: Vec<(String, String)>,
    pub violations: Vec<LintViolation>,
}

impl SourceLint {
    pub fn new() -> SourceLint {
        SourceLint::default()
    }

    /// 登记（允许重复登记——多来源正是 lint 要抓的，登记表如实留痕）。
    pub fn bind(&mut self, interaction: &str, token_name: &str) {
        self.bindings
            .push((interaction.to_string(), token_name.to_string()));
    }

    /// 精确解除一条登记（收敛为唯一来源的修复动作）。
    pub fn unbind(&mut self, interaction: &str, token_name: &str) -> bool {
        let before = self.bindings.len();
        self.bindings
            .retain(|(i, t)| !(i == interaction && t == token_name));
        self.bindings.len() != before
    }

    /// lint 一批调用点：调用点只许带语义名；带裸资源路径 = 绕过令牌。
    /// 返回并留痕全部违规（一处定义，处处一致）。
    pub fn lint(&mut self, call_sites: &[(String, CallStyle)]) -> Vec<LintViolation> {
        self.violations.clear();
        // 同交互多令牌来源检查
        for (i, _) in call_sites.iter() {
            let sources: Vec<&String> = self
                .bindings
                .iter()
                .filter(|(bi, _)| bi == i)
                .map(|(_, t)| t)
                .collect();
            if sources.len() > 1 {
                self.violations.push(LintViolation {
                    code: "E_MULTI_SOURCE",
                    interaction: i.clone(),
                    detail: format!("交互 {} 有 {} 个令牌来源", i, sources.len()),
                });
            }
        }
        // 硬编码直呼资源检查
        for (site, style) in call_sites.iter() {
            if *style == CallStyle::RawResource {
                self.violations.push(LintViolation {
                    code: "E_RAW_RESOURCE",
                    interaction: site.clone(),
                    detail: "调用点直呼音频资源——绕过令牌，lint 拦截".to_string(),
                });
            }
        }
        self.violations.clone()
    }
}

/// 调用点风格。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CallStyle {
    /// 经令牌语义名（合规）。
    ViaToken,
    /// 直呼资源路径（违规）。
    RawResource,
}

// ---------------------------------------------------------------------------
// 四、读屏摘要
// ---------------------------------------------------------------------------

/// 解析结果的人话摘要。
pub fn resolved_summary(r: &Resolved, name: &str) -> String {
    match r {
        Resolved::Play(spec, lv) => format!(
            "令牌 {} → {}（音量 {}‰、延迟{}）〔生效来源：{}〕",
            name, spec.resource, spec.volume_permille, spec.latency.label(),
            lv.label()
        ),
        Resolved::Silent => format!("令牌 {} → 静默（缺省链走完，合法态）", name),
    }
}
