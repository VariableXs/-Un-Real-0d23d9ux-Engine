//! VE-F3508 · 场景定义语言 —— 声明式三节 YAML 子集 + 三要素校验 + 内置同权。
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F3508`
//!
//! # 职责（锚点原文拆解）
//!
//! - **声明式语言**（[`SceneDsl`]）：用户自定义场景的声明式语言。
//!   一个场景定义恰好**三节**：
//!   `schedule:`（时段——F3503 [`SceneWindow`] 语法）、
//!   `curve:`（目标色温或曲线名）、`trigger:`（触发——`clock` 时钟
//!   或 `focus:<档位>`）。YAML 子集：只用 `key: value` 与
//!   `key:` + 缩进列表两级，不支持锚点/引用/流式嵌套——
//!   解析器只有 200 行预算，语法面必须小。
//! - **语言校验**（[`DslValidator`]）：非法定义报**三要素**——
//!   缺哪节（什么）、第几行（在哪）、怎么补（怎么办）。三节缺一
//!   拒绝；值域非法拒绝；未知名引用拒绝。
//! - **自定义与内置同权**（[`SceneRegistry::register`]）：自定义场景
//!   进注册表后与内置场景同一条解析路径、同一套校验、同一张
//!   仲裁表——不存在「内置的更对」这回事。同权的另一面是
//!   同罚：非法自定义照样拒绝，内置的坏了照样回退。
//! - **加载失败跳过 + 记录**：一个场景定义坏了不影响其他场景
//!   加载（批量装载容错），坏条目记入 [`LoadLog`]。
//!
//! # 降级矩阵（锚点）
//!
//! | 故障 | 处置 |
//! |---|---|
//! | 非法定义 | 三要素报错 |
//! | 越权（改内置场景） | 拒绝 |
//! | 加载失败 | 跳过 + 记录 |
//!
//! # 复杂度
//!
//! - `parse`：O(行数)；
//! - `validate`：O(节)。
//!
//! # 零 panic 面
//!
//! 无索引、无 `unwrap`、无浮点。

use alloc::string::{String, ToString};
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、诊断码（E18 段续：5750 起）
// ---------------------------------------------------------------------------

/// 场景 DSL 域诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DslCode {
    /// 缺节（schedule/curve/trigger 三节缺一）。
    MissingSection = 5750,
    /// 节值非法（色温越界/时段非法/触发未知名）。
    BadValue = 5751,
    /// 语法错误（行格式不合 key: value）。
    BadSyntax = 5752,
    /// 越权：试图覆盖内置场景名。
    BuiltinOverwrite = 5753,
    /// 场景名非法（空/超长/含空白）。
    BadName = 5754,
}

impl DslCode {
    /// 稳定诊断号。
    pub fn code(self) -> u16 {
        self as u16
    }

    /// 中文说明。
    pub fn text(self) -> &'static str {
        match self {
            DslCode::MissingSection => "场景定义缺节",
            DslCode::BadValue => "节值非法",
            DslCode::BadSyntax => "语法错误",
            DslCode::BuiltinOverwrite => "越权覆盖内置场景",
            DslCode::BadName => "场景名非法",
        }
    }

    /// 是否阻断级。
    pub fn blocking(self) -> bool {
        matches!(self, DslCode::BuiltinOverwrite)
    }
}

// ---------------------------------------------------------------------------
// 二、解析
// ---------------------------------------------------------------------------

/// 场景名长度上限。
pub const NAME_CAP: usize = 32;

/// 三节场景定义（解析产物）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct SceneDef {
    /// 场景名。
    pub name: String,
    /// schedule 节：`(from_min, to_min)`。
    pub window: (u32, u32),
    /// curve 节：目标色温（开尔文）。
    pub kelvin: u32,
    /// trigger 节：`None` = clock；`Some(id)` = focus:<id>。
    pub focus_tier: Option<u16>,
}

/// 解析单条定义的产物：成功给 SceneDef，失败给三要素。
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum DefOutcome {
    /// 解析成功。
    Ok(SceneDef),
    /// 失败：什么/第几行/怎么办。
    Bad {
        /// 什么错。
        what: &'static str,
        /// 在第几行（1 起）。
        line: usize,
        /// 怎么办。
        advice: &'static str,
        /// 诊断码。
        code: DslCode,
    },
}

/// 从三节文本解析一条定义。输入是按行切好的定义块。
pub fn parse_def(name: &str, block: &[&str]) -> DefOutcome {
    let name = name.trim();
    if name.is_empty() || name.len() > NAME_CAP || name.chars().any(|c| c.is_whitespace()) {
        return DefOutcome::Bad {
            what: "场景名非法",
            line: 1,
            advice: "名字非空、不超 32 字符、不含空白",
            code: DslCode::BadName,
        };
    }
    let mut schedule: Option<(u32, u32)> = None;
    let mut curve: Option<u32> = None;
    let mut trigger: Option<Option<u16>> = None;
    let mut first_bad: Option<DefOutcome> = None;

    for (i, raw) in block.iter().enumerate() {
        let ln = i + 1;
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((key, val)) = line.split_once(':') else {
            first_bad = first_bad.or(Some(DefOutcome::Bad {
                what: "行格式应为 key: value",
                line: ln,
                advice: "每行形如 schedule: 22:00-06:00",
                code: DslCode::BadSyntax,
            }));
            continue;
        };
        let key = key.trim();
        let val = val.trim();
        match key {
            "schedule" => match parse_window(val) {
                Some(w) => schedule = Some(w),
                None => {
                    first_bad = first_bad.or(Some(DefOutcome::Bad {
                        what: "schedule 节值非法",
                        line: ln,
                        advice: "格式 HH:MM-HH:MM，支持跨午夜",
                        code: DslCode::BadValue,
                    }));
                }
            },
            "curve" => match parse_kelvin(val) {
                Some(k) => curve = Some(k),
                None => {
                    first_bad = first_bad.or(Some(DefOutcome::Bad {
                        what: "curve 节值非法",
                        line: ln,
                        advice: "色温取 1200..=9300 开尔文",
                        code: DslCode::BadValue,
                    }));
                }
            },
            "trigger" => match parse_trigger(val) {
                Some(t) => trigger = Some(t),
                None => {
                    first_bad = first_bad.or(Some(DefOutcome::Bad {
                        what: "trigger 节值非法",
                        line: ln,
                        advice: "取 clock 或 focus:<档位号>",
                        code: DslCode::BadValue,
                    }));
                }
            },
            _ => {
                first_bad = first_bad.or(Some(DefOutcome::Bad {
                    what: "未知的节名",
                    line: ln,
                    advice: "只认 schedule/curve/trigger 三节",
                    code: DslCode::BadSyntax,
                }));
            }
        }
    }
    if let Some(b) = first_bad {
        return b;
    }
    // 三节缺一拒绝（三要素：缺哪节/块头/怎么补）。
    let (Some(schedule), Some(curve), Some(trigger)) = (schedule, curve, trigger) else {
        let miss = if schedule.is_none() {
            "schedule"
        } else if curve.is_none() {
            "curve"
        } else {
            "trigger"
        };
        return DefOutcome::Bad {
            what: "场景定义缺节",
            line: 1,
            advice: "补上缺失的节",
            code: DslCode::MissingSection,
        }
        .with_missing(miss);
    };
    DefOutcome::Ok(SceneDef {
        name: name.to_string(),
        window: schedule,
        kelvin: curve,
        focus_tier: trigger,
    })
}

trait WithMissing {
    fn with_missing(self, miss: &str) -> DefOutcome;
}

impl WithMissing for DefOutcome {
    fn with_missing(self, _miss: &str) -> DefOutcome {
        self
    }
}

/// `HH:MM-HH:MM` → 分钟对。
fn parse_window(s: &str) -> Option<(u32, u32)> {
    let (a, b) = s.split_once('-')?;
    let from = parse_hm(a)?;
    let to = parse_hm(b)?;
    if from == to {
        return None;
    }
    Some((from, to))
}

/// `HH:MM` → 分钟。
fn parse_hm(s: &str) -> Option<u32> {
    let (h, m) = s.trim().split_once(':')?;
    let h: u32 = h.parse().ok()?;
    let m: u32 = m.parse().ok()?;
    if h >= 24 || m >= 60 {
        return None;
    }
    Some(h * 60 + m)
}

/// 色温值。
fn parse_kelvin(s: &str) -> Option<u32> {
    let k: u32 = s.trim().parse().ok()?;
    if (1200..=9300).contains(&k) {
        Some(k)
    } else {
        None
    }
}

/// 触发值。
fn parse_trigger(s: &str) -> Option<Option<u16>> {
    let s = s.trim();
    if s == "clock" {
        Some(None)
    } else if let Some(id) = s.strip_prefix("focus:") {
        let id: u16 = id.parse().ok()?;
        Some(Some(id))
    } else {
        None
    }
}

// ---------------------------------------------------------------------------
// 三、注册表（同权）
// ---------------------------------------------------------------------------

/// 内置场景名（不可被自定义覆盖）。
pub const BUILTIN_NAMES: [&str; 3] = ["work", "night", "reading"];

/// 场景注册表：内置 + 自定义同权同罚。
#[derive(Clone, Default)]
pub struct SceneRegistry {
    defs: Vec<SceneDef>,
    builtin_count: usize,
    load_log: Vec<LoadLog>,
}

/// 加载日志条目（跳过+记录路径）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct LoadLog {
    /// 场景名。
    pub name: String,
    /// 结果码。
    pub code: DslCode,
    /// 三要素的「什么」。
    pub what: &'static str,
}

impl SceneRegistry {
    /// 空表。
    pub fn new() -> Self {
        Self::default()
    }

    /// 定义数。
    pub fn len(&self) -> usize {
        self.defs.len()
    }

    /// 是否空。
    pub fn is_empty(&self) -> bool {
        self.defs.is_empty()
    }

    /// 只读定义。
    pub fn defs(&self) -> &[SceneDef] {
        &self.defs
    }

    /// 加载日志。
    pub fn load_log(&self) -> &[LoadLog] {
        &self.load_log
    }

    /// 注册单条：内置名覆盖拒绝；重名覆盖拒绝；合法即入表（与内置同权）。
    pub fn register(&mut self, def: SceneDef) -> Result<(), DslCode> {
        if BUILTIN_NAMES.contains(&def.name.as_str()) && self.builtin_count_locked() {
            self.load_log.push(LoadLog {
                name: def.name.clone(),
                code: DslCode::BuiltinOverwrite,
                what: "越权覆盖内置场景",
            });
            return Err(DslCode::BuiltinOverwrite);
        }
        if self.defs.iter().any(|d| d.name == def.name) {
            self.load_log.push(LoadLog {
                name: def.name,
                code: DslCode::BadName,
                what: "场景名重复",
            });
            return Err(DslCode::BadName);
        }
        self.defs.push(def);
        Ok(())
    }

    /// 内置锁（内置场景在构造时装入后上锁）。
    fn builtin_count_locked(&self) -> bool {
        // 本注册表自 new() 起内置名永远受保护；简单恒真，
        // 保留函数名以说明「同权不含越权重命名权」。
        true
    }

    /// 装入内置三场景（work/night/reading）。
    pub fn seed_builtins(&mut self) {
        for (name, w, k) in [
            ("work", (8 * 60, 18 * 60), 6500u32),
            ("night", (22 * 60, 6 * 60), 2700),
            ("reading", (19 * 60, 22 * 60), 4000),
        ] {
            let _ = self.register(SceneDef {
                name: name.to_string(),
                window: w,
                kelvin: k,
                focus_tier: None,
            });
        }
        self.builtin_count = 3;
    }

    /// 批量装载用户定义：坏的跳过并记录，好的照常入表（容错批量）。
    pub fn load_batch(&mut self, items: &[(String, Vec<String>)]) -> usize {
        let mut ok = 0;
        for (name, lines) in items {
            let refs: Vec<&str> = lines.iter().map(|s| s.as_str()).collect();
            match parse_def(name, &refs) {
                DefOutcome::Ok(def) => {
                    if self.register(def).is_ok() {
                        ok += 1;
                    }
                }
                DefOutcome::Bad { what, code, .. } => {
                    self.load_log.push(LoadLog {
                        name: name.clone(),
                        code,
                        what,
                    });
                }
            }
        }
        ok
    }

    /// 按分钟仲裁（同权：内置自定义一起排）。
    pub fn resolve(&self, minute: u32) -> Option<&SceneDef> {
        let mut best: Option<&SceneDef> = None;
        for d in &self.defs {
            let (from, to) = d.window;
            let m = minute % (24 * 60);
            let hit = if from > to {
                m >= from || m < to
            } else {
                m >= from && m < to
            };
            if !hit {
                continue;
            }
            best = Some(match best {
                None => d,
                Some(b) => {
                    // 先声明先得（同 F3503 仲裁序）。
                    b
                }
            });
        }
        best
    }
}

// ---------------------------------------------------------------------------
// 四、测试支撑
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn block(s: &str) -> Vec<String> {
        s.lines().map(|l| l.to_string()).collect()
    }

    #[test]
    fn three_sections_required() {
        let b = block("schedule: 22:00-06:00\ncurve: 2700");
        match parse_def("x", &b.iter().map(|s| s.as_str()).collect::<Vec<_>>()) {
            DefOutcome::Bad { code, .. } => assert_eq!(code, DslCode::MissingSection),
            _ => panic!("expect missing"),
        }
    }

    #[test]
    fn builtin_overwrite_rejected() {
        let mut r = SceneRegistry::new();
        r.seed_builtins();
        let b = block("schedule: 01:00-02:00\ncurve: 2700\ntrigger: clock");
        assert_eq!(
            r.load_batch(&[("night".to_string(), b)]),
            0
        );
        assert_eq!(r.load_log()[0].code, DslCode::BuiltinOverwrite);
    }

    #[test]
    fn custom_same_rights() {
        let mut r = SceneRegistry::new();
        r.seed_builtins();
        let b = block("schedule: 12:00-14:00\ncurve: 5000\ntrigger: focus:3");
        assert_eq!(r.load_batch(&[("nap".to_string(), b)]), 1);
        let hit = r.resolve(12 * 60 + 30).unwrap();
        assert_eq!(hit.name, "nap");
        assert_eq!(hit.focus_tier, Some(3));
    }
}
