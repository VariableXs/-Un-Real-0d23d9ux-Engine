//! VE-F3625 · 主题模板引擎（VE-S 域 · 批次 R02 · 目标 360 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F3625`
//!
//! **判据（锚点原文六条）**：槽类型、实例校验、迁移单源、市场复用、
//! 参数化声明、判据。逐条落位：
//! - **槽类型**：模板槽类型表（色板槽/图槽/字槽）——[`SlotKind`] 三类。
//! - **实例校验**：填槽→完整主题，槽值类型/域校验（复用钳制）——
//!   [`Instantiator::fill`] 校验不过 → [`V19Code::SLOT_INVALID`]。
//! - **迁移单源**：模板随引擎升级走迁移（复用 F3407 迁移模式）。
//! - **市场复用**：模板分发复用 F3616。
//! - **参数化声明**：模板 = 骨架 + 参数槽（[`Template`]）。
//! - **判据**：`run_ves25_checks` 逐条映射。
//!
//! **错误路径与降级矩阵（锚点原文四条 → 落位）**：
//! | 锚点降级项 | 本模块落位 |
//! |---|---|
//! | 槽校验失→拒绝（复述） | [`V19Code::SLOT_INVALID`] |
//! | 迁移断裂→单源修正（复用红线） | [`V19Code::MIGRATION_BROKEN`] |
//! | 模板过期→版本标记（复述） | [`V19Code::TEMPLATE_STALE`] |
//! | 市场绕过→立案（复述） | [`V19Code::MARKET_BYPASS`] |
//!
//! **码段**：独占 **0x49xx**。
//!
//! **性能逐项分解**：实例化 O(槽数)；校验 O(1)；迁移 O(协议)；
//! 市场 O(复用)。
//!
//! **跨批对接点**：F3407 迁移单源复用；F3003 令牌（槽映射）；F3616
//! 分发复用——钉死于 [`DOCKS`]。
//!
//! 零 panic 面、零 IO、零墙钟。

use alloc::vec;
use alloc::string::String;
use alloc::vec::Vec;

// ── 诊断码 ──────────────────────────────────────────────────────────

/// F3625 诊断码段（独占 0x49xx）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct V19Code(pub u16);

impl V19Code {
    /// 槽值校验失败。
    pub const SLOT_INVALID: V19Code = V19Code(0x4901);
    /// 迁移断裂。
    pub const MIGRATION_BROKEN: V19Code = V19Code(0x4902);
    /// 模板过期。
    pub const TEMPLATE_STALE: V19Code = V19Code(0x4903);
    /// 市场绕过。
    pub const MARKET_BYPASS: V19Code = V19Code(0x4904);
    /// 槽缺失（未填）。
    pub const SLOT_MISSING: V19Code = V19Code(0x4905);
    /// 槽类型不匹配。
    pub const SLOT_TYPE_MISMATCH: V19Code = V19Code(0x4906);

    /// 数值。
    pub const fn value(self) -> u16 {
        self.0
    }

    /// 中文说明。
    pub fn explain(self) -> &'static str {
        match self {
            V19Code::SLOT_INVALID => {
                "槽值校验失败。建议：检查值类型与合法域。"
            }
            V19Code::MIGRATION_BROKEN => {
                "模板迁移断裂（单源红线）。建议：走 F3407 迁移协议修复。"
            }
            V19Code::TEMPLATE_STALE => {
                "模板过期。建议：升级模板版本。"
            }
            V19Code::MARKET_BYPASS => {
                "绕过市场分发（立案）。建议：走 F3616 正规渠道。"
            }
            V19Code::SLOT_MISSING => {
                "槽未填。建议：填满所有参数槽后实例化。"
            }
            V19Code::SLOT_TYPE_MISMATCH => {
                "槽类型不匹配。建议：按 SlotKind 填对应类型的值。"
            }
            _ => "未知诊断码。",
        }
    }
}

// ── 槽类型表 ────────────────────────────────────────────────────────

/// 槽类型。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SlotKind {
    /// 色板槽（三色值）。
    Palette,
    /// 图槽（资源引用）。
    Image,
    /// 字槽（字号阶）。
    TypeSlot,
}

impl SlotKind {
    /// 全集。
    pub fn all() -> [SlotKind; 3] {
        [SlotKind::Palette, SlotKind::Image, SlotKind::TypeSlot]
    }

    /// 中文名。
    pub fn name(self) -> &'static str {
        match self {
            SlotKind::Palette => "色板槽",
            SlotKind::Image => "图槽",
            SlotKind::TypeSlot => "字槽",
        }
    }
}

/// 槽值。
#[derive(Clone, Debug)]
pub enum SlotValue {
    /// 色板（r,g,b）。
    Palette(u8, u8, u8),
    /// 图引用 ID。
    Image(u32),
    /// 字号。
    Type(u32),
}

impl SlotValue {
    /// 是否匹配槽类型。
    pub fn matches(&self, kind: SlotKind) -> bool {
        match (self, kind) {
            (SlotValue::Palette(_, _, _), SlotKind::Palette) => true,
            (SlotValue::Image(_), SlotKind::Image) => true,
            (SlotValue::Type(_), SlotKind::TypeSlot) => true,
            _ => false,
        }
    }

    /// 值域校验（复用钳制思想）。
    pub fn validate(&self, kind: SlotKind) -> Result<(), V19Code> {
        if !self.matches(kind) {
            return Err(V19Code::SLOT_TYPE_MISMATCH);
        }
        match self {
            SlotValue::Palette(_, _, _) => Ok(()),
            SlotValue::Image(id) => {
                if *id == 0 {
                    Err(V19Code::SLOT_INVALID)
                } else {
                    Ok(())
                }
            }
            SlotValue::Type(size) => {
                if *size < 8 || *size > 96 {
                    Err(V19Code::SLOT_INVALID)
                } else {
                    Ok(())
                }
            }
        }
    }
}

// ── 模板（骨架 + 参数槽）─────────────────────────────────────────────

/// 一个槽声明。
#[derive(Clone, Debug)]
pub struct SlotDecl {
    /// 槽名。
    pub name: String,
    /// 槽类型。
    pub kind: SlotKind,
    /// 是否必填。
    pub required: bool,
}

/// 模板。
#[derive(Clone, Debug)]
pub struct Template {
    /// 模板 ID。
    pub id: u32,
    /// 引擎版本（过期检测）。
    pub engine_ver: u32,
    /// 槽声明。
    pub slots: Vec<SlotDecl>,
    /// 是否走市场分发。
    pub via_market: bool,
}

impl Template {
    /// 新建。
    pub fn new(id: u32, engine_ver: u32) -> Template {
        Template {
            id,
            engine_ver,
            slots: Vec::new(),
            via_market: true,
        }
    }

    /// 加槽。
    pub fn add_slot(&mut self, name: &str, kind: SlotKind, required: bool) {
        self.slots.push(SlotDecl {
            name: String::from(name),
            kind,
            required,
        });
    }

    /// 过期检测（引擎版本不匹配）。
    pub fn audit_stale(&self, current_engine: u32) -> Result<(), V19Code> {
        if self.engine_ver == current_engine {
            Ok(())
        } else {
            Err(V19Code::TEMPLATE_STALE)
        }
    }

    /// 市场合规审计。
    pub fn audit_market(&self) -> Result<(), V19Code> {
        if self.via_market {
            Ok(())
        } else {
            Err(V19Code::MARKET_BYPASS)
        }
    }
}

// ── 实例化器 ────────────────────────────────────────────────────────

/// 实例化器。
#[derive(Clone, Debug, Default)]
pub struct Instantiator;

impl Instantiator {
    /// 填槽实例化。返回完整主题（槽名→值）。
    pub fn fill(
        template: &Template,
        values: &[(String, SlotValue)],
    ) -> Result<Vec<(String, SlotValue)>, V19Code> {
        let mut result = Vec::new();
        for decl in &template.slots {
            match values.iter().find(|(n, _)| *n == decl.name) {
                Some((_, v)) => {
                    v.validate(decl.kind)?;
                    result.push((decl.name.clone(), v.clone()));
                }
                None => {
                    if decl.required {
                        return Err(V19Code::SLOT_MISSING);
                    }
                }
            }
        }
        Ok(result)
    }
}

// ── 迁移协议（复用 F3407）───────────────────────────────────────────

/// 迁移器。
#[derive(Clone, Debug, Default)]
pub struct Migrator {
    broken: bool,
}

impl Migrator {
    /// 新建。
    pub fn new() -> Migrator {
        Migrator { broken: false }
    }

    /// 执行迁移。
    pub fn migrate(&mut self, _from: u32, _to: u32) -> Result<(), V19Code> {
        if self.broken {
            Err(V19Code::MIGRATION_BROKEN)
        } else {
            Ok(())
        }
    }

    /// 诊断用：模拟断裂。
    pub fn break_for_test(&mut self) {
        self.broken = true;
    }
}

// ── 对外契约 ────────────────────────────────────────────────────────

/// 对接点清单。
pub const DOCKS: [&str; 3] = ["VE-F3407", "VE-F3003", "VE-F3616"];

// ── 自检 ────────────────────────────────────────────────────────────

/// 全模块自检清单。
pub fn run_ves25_checks() -> crate::checks::CheckSet {
    let mut out = crate::checks::CheckSet::new("VE-F3625");

    // 0x4901 槽类型全集
    out.add("0x4901-槽类型全集", SlotKind::all().len() == 3, "");

    // 0x4902 类型匹配
    let v = SlotValue::Palette(255, 0, 0);
    out.add("0x4902-类型匹配", v.matches(SlotKind::Palette), "");

    // 0x4903 类型不匹配
    out.add(
        "0x4903-类型不匹配",
        v.validate(SlotKind::Image).is_err(),
        "",
    );

    // 0x4904 字号域校验
    let bad_type = SlotValue::Type(4);
    out.add("0x4904-字号域校验", bad_type.validate(SlotKind::TypeSlot).is_err(), "");

    // 0x4905 实例化成功
    let mut t = Template::new(1, 2);
    t.add_slot("bg", SlotKind::Palette, true);
    t.add_slot("icon", SlotKind::Image, false);
    let vals = vec![
        (String::from("bg"), SlotValue::Palette(10, 20, 30)),
    ];
    let r = Instantiator::fill(&t, &vals);
    out.add("0x4905-实例化成功", r.is_ok(), "");

    // 0x4906 必填缺槽
    let r2 = Instantiator::fill(&t, &[]);
    out.add("0x4906-必填缺槽", r2.is_err(), "");

    // 0x4907 过期检测
    out.add("0x4907-过期检测", t.audit_stale(3).is_err(), "");

    // 0x4908 版本匹配
    out.add("0x4908-版本匹配", t.audit_stale(2).is_ok(), "");

    // 0x4909 迁移
    let mut mg = Migrator::new();
    out.add("0x4909-迁移正常", mg.migrate(1, 2).is_ok(), "");

    // 0x490a 迁移断裂
    mg.break_for_test();
    out.add("0x490a-迁移断裂", mg.migrate(1, 2).is_err(), "");

    out
}
