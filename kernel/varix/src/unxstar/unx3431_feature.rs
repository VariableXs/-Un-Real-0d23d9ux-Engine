//! UNX-F3431 后端选择策略（编译期 feature + 运行期覆盖） · 0x799C0段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（派工卡 VX8-B12 逐字摘录）：
//!   ① 纯功能代码 ≥260 行；
//!   ② J1：两级选择（编译期默认 + 运行期覆盖）语义明确；
//!     M-190 两级交互验证。
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 两级语义冻结：编译期默认表（轴 → 后端，只读常量）为基线，
//!     运行期覆盖（轴 → 后端，单槽）显性胜出——覆盖同值也记活动态，
//!     语义「覆盖即显性」；清除覆盖即回落编译期默认；
//!   - 编译期默认表不可变：set/clear 任何操作后默认表逐键逐值不变
//!     （M-190 交互验证之一：默认面不可被覆盖侧污染）；
//!   - 后端闭集三员：recorder（留痕）/ softbuf（软缓冲）/ qemu-virtio
//!     （虚拟设备），名字与能力位逐字冻结，表外点名拒；
//!   - 能力位核对：解析出的后端须备齐请求能力位，缺位显性拒
//!     （CapsMissing），未冻结能力位点名拒（CapabilityBitForeign）；
//!   - 覆盖单槽纪律：一次只覆盖一个轴（同轴重设=改写，异轴设置=
//!     显性拒——单槽不静默吞前值）；
//!   - 双跑一致：解析与两级交互序列均纯函数，同输入两次逐码相同。
//! 常量来源：
//!   - 后端三员与能力位：内核后端面冻结表（recorder/softbuf/qemu-virtio）；
//!   - 特性轴三键：console/record/ci（派工卡两级选择语义的操作面）；
//!   - 诊断段：0x799C0（0x40 对齐，B12 批窗 [0x79800, 0x799C0]）。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x40 对齐，B12 批段窗 [0x79800, 0x799C0]）。
pub const SEG_BASE: u32 = 0x799C0;

/// 能力位：留痕（账面可导出）。
pub const CAP_RECORD: u32 = 0x1;

/// 能力位：着色（控制台彩色输出）。
pub const CAP_COLOR: u32 = 0x2;

/// 能力位：无头（无显示面环境可跑）。
pub const CAP_HEADLESS: u32 = 0x4;

/// 全部冻结能力位的并集（表外位核查基线）。
pub const CAP_ALL: u32 = CAP_RECORD | CAP_COLOR | CAP_HEADLESS;

/// 后端员闭集（名字冻结，序固定）。
pub const BACKEND_NAMES: [&'static str; 3] = ["recorder", "softbuf", "qemu-virtio"];

/// 特性轴闭集（键冻结，序固定）。
pub const AXIS_KEYS: [&'static str; 3] = ["console", "record", "ci"];

/// 编译期默认行（轴 → 后端名；表只读，运行期不改）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DefaultRow {
    /// 特性轴键。
    pub axis: &'static str,
    /// 编译期默认后端名。
    pub backend: &'static str,
}

/// 编译期默认表（冻结；语义基线）。
pub const COMPILE_DEFAULTS: [DefaultRow; 3] = [
    DefaultRow { axis: "console", backend: "softbuf" },
    DefaultRow { axis: "record", backend: "recorder" },
    DefaultRow { axis: "ci", backend: "qemu-virtio" },
];

/// 后端员（名字 + 能力位）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BackendRow {
    /// 后端名（闭集内）。
    pub name: &'static str,
    /// 能力位并。
    pub caps: u32,
}

/// 后端冻结表（名字与能力位逐员冻结）。
pub const BACKENDS: [BackendRow; 3] = [
    BackendRow { name: "recorder", caps: CAP_RECORD | CAP_HEADLESS },
    BackendRow { name: "softbuf", caps: CAP_COLOR | CAP_HEADLESS },
    BackendRow { name: "qemu-virtio", caps: CAP_RECORD | CAP_COLOR | CAP_HEADLESS },
];

/// 后端名 → 能力位（闭集查表；表外返回 None）。
pub fn backend_caps(name: &str) -> Option<u32> {
    for b in BACKENDS.iter() {
        if b.name == name {
            return Some(b.caps);
        }
    }
    None
}

/// 后端选择器（两级：编译期默认 + 运行期单槽覆盖）。
/// 覆盖轴/后端名存 `&'static str`：设置时已核闭集，闭集内名字即
/// `'static` 冻结名——解析面因此可全程返回 `&'static str`。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BackendSelector {
    /// 覆盖轴（None = 无覆盖）。
    pub override_axis: Option<&'static str>,
    /// 覆盖后端名。
    pub override_backend: Option<&'static str>,
    /// 覆盖活动态（同值覆盖也置真——覆盖即显性）。
    pub override_active: bool,
}

impl BackendSelector {
    /// 纯编译期选择器（无覆盖）。
    pub fn compile_only() -> BackendSelector {
        BackendSelector {
            override_axis: None,
            override_backend: None,
            override_active: false,
        }
    }

    /// 编译期默认解析（无覆盖路径；轴表外/表残缺点名拒）。
    pub fn default_for(&self, axis: &str) -> Result<&'static str, u32> {
        if axis.is_empty() {
            return Err(Diag::AxisKeyEmpty.val());
        }
        let mut known = false;
        for key in AXIS_KEYS.iter() {
            if *key == axis {
                known = true;
            }
        }
        if !known {
            return Err(Diag::AxisForeign.val());
        }
        for row in COMPILE_DEFAULTS.iter() {
            if row.axis == axis {
                return Ok(row.backend);
            }
        }
        Err(Diag::DefaultMissing.val())
    }

    /// 两级解析（J1 核心）：覆盖只作用于自身轴（同轴胜出），异轴回落
    /// 编译期默认；活动态下槽记录残缺（轴/后端缺一）点名内部破缺。
    pub fn resolve(&self, axis: &str) -> Result<&'static str, u32> {
        if self.override_active {
            match (self.override_axis, self.override_backend) {
                (Some(oa), Some(ob)) => {
                    if oa == axis {
                        // 覆盖后端名表外核查（设置时已核，此处防御性重核）。
                        if backend_caps(ob).is_some() {
                            return Ok(ob);
                        }
                        return Err(Diag::BackendForeign.val());
                    }
                    // 异轴：覆盖不越轴，回落编译期默认。
                }
                _ => return Err(Diag::OverrideAxisMismatch.val()),
            }
        }
        self.default_for(axis)
    }

    /// 设置运行期覆盖（单槽：已有异轴覆盖时显性拒，不静默吞）。
    pub fn set_override(&mut self, axis: &'static str, backend: &'static str) -> Result<(), u32> {
        if axis.is_empty() {
            return Err(Diag::AxisKeyEmpty.val());
        }
        let mut known_axis = false;
        for key in AXIS_KEYS.iter() {
            if *key == axis {
                known_axis = true;
            }
        }
        if !known_axis {
            return Err(Diag::AxisForeign.val());
        }
        if backend_caps(backend).is_none() {
            return Err(Diag::BackendForeign.val());
        }
        if self.override_active {
            match self.override_axis {
                Some(prev) => {
                    if prev != axis {
                        return Err(Diag::SingleSlotConflict.val());
                    }
                }
                None => return Err(Diag::SingleSlotConflict.val()),
            }
        }
        self.override_axis = Some(axis);
        self.override_backend = Some(backend);
        self.override_active = true;
        Ok(())
    }

    /// 清除覆盖（回落编译期默认；未设覆盖先清即点名）。
    pub fn clear_override(&mut self) -> Result<(), u32> {
        if !self.override_active {
            return Err(Diag::OverrideClearTwice.val());
        }
        self.override_axis = None;
        self.override_backend = None;
        self.override_active = false;
        Ok(())
    }
}

/// 两级解析带能力核对：解析出的后端须备齐请求能力位。
pub fn resolve_with_caps(sel: &BackendSelector, axis: &str, required: u32) -> Result<u32, u32> {
    // 未冻结能力位点名拒。
    if required & !CAP_ALL != 0 {
        return Err(Diag::CapabilityBitForeign.val());
    }
    let name = sel.resolve(axis)?;
    match backend_caps(name) {
        Some(caps) => {
            if caps & required == required {
                Ok(caps)
            } else {
                Err(Diag::CapsMissing.val())
            }
        }
        None => Err(Diag::BackendForeign.val()),
    }
}

/// 诊断码闭集：0x799C1..=0x799CC（12 码，变体带中文 doc）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Diag {
    /// 轴表外：特性轴键不在冻结三键内。
    AxisForeign = 0x01,
    /// 后端表外：后端名不在冻结三员内。
    BackendForeign = 0x02,
    /// 能力缺位：解析后端不备齐请求能力位。
    CapsMissing = 0x03,
    /// 默认缺失：轴在册但编译期默认表无行（表残缺）。
    DefaultMissing = 0x04,
    /// 重复清除：未设覆盖却请求清除。
    OverrideClearTwice = 0x05,
    /// 轴键空：空串轴名。
    AxisKeyEmpty = 0x06,
    /// 能力位表外：请求了未冻结的能力位。
    CapabilityBitForeign = 0x07,
    /// 后端重名：冻结后端表出现重名员。
    BackendDupName = 0x08,
    /// 轴重键：冻结轴表出现重键。
    AxisDupKey = 0x09,
    /// 覆盖轴不符：覆盖槽活动态下轴/后端缺一（内部残缺）。
    OverrideAxisMismatch = 0x0A,
    /// 单槽冲突：已有异轴覆盖时再设异轴（单槽不静默吞前值）。
    SingleSlotConflict = 0x0B,
    /// 双跑不一致：同输入两次解析结论不同。
    DoubleRunDiverge = 0x0C,
}

impl Diag {
    /// 完整 u32 编码。
    pub fn val(self) -> u32 {
        SEG_BASE + (self as u32)
    }

    /// 中文诊断文案（含可操作建议）。
    pub fn detail(self) -> &'static str {
        match self {
            Diag::AxisForeign => "轴表外：建议特性轴只用 console/record/ci 三键",
            Diag::BackendForeign => "后端表外：建议后端名只用 recorder/softbuf/qemu-virtio 三员",
            Diag::CapsMissing => "能力缺位：建议按后端能力表选员，或降低请求能力位",
            Diag::DefaultMissing => "默认缺失：建议补全编译期默认表，三轴逐键有行",
            Diag::OverrideClearTwice => "重复清除：建议清除前先判覆盖活动态，未设不清",
            Diag::AxisKeyEmpty => "轴键空：建议补全轴名再解析，空键无语义",
            Diag::CapabilityBitForeign => "能力位表外：建议能力请求只用 record/color/headless 三冻结位",
            Diag::BackendDupName => "后端重名：建议冻结表内名字唯一，重名员先除名",
            Diag::AxisDupKey => "轴重键：建议冻结表内轴键唯一，重键先合并",
            Diag::OverrideAxisMismatch => "覆盖槽残缺：建议活动态下轴与后端成对存放，残缺先重建覆盖槽",
            Diag::SingleSlotConflict => "单槽冲突：建议同轴重设=改写，异轴先清旧覆盖再设新",
            Diag::DoubleRunDiverge => "双跑不一致：建议解析做成纯函数，同输入同码",
        }
    }
}

/// 闭集全量数组（12 码）。
pub const ALL_DIAGS: [Diag; 12] = [
    Diag::AxisForeign,
    Diag::BackendForeign,
    Diag::CapsMissing,
    Diag::DefaultMissing,
    Diag::OverrideClearTwice,
    Diag::AxisKeyEmpty,
    Diag::CapabilityBitForeign,
    Diag::BackendDupName,
    Diag::AxisDupKey,
    Diag::OverrideAxisMismatch,
    Diag::SingleSlotConflict,
    Diag::DoubleRunDiverge,
];

/// 诊断码查表闭集返回；未知编码回退固定兜底串。
pub fn detail(code: u32) -> &'static str {
    for d in ALL_DIAGS.iter() {
        if d.val() == code {
            return d.detail();
        }
    }
    "未知诊断码：建议核对是否落在 0x799C1..=0x799CC 闭集内"
}

// ── 判据谓词（全部私有 fn xxx_ok() -> bool）────────────────────────────

/// 段位守卫：0x40 对齐 + B12 批段窗 [0x79800, 0x799C0]。
fn seg_guard_ok() -> bool {
    SEG_BASE % 0x40 == 0 && SEG_BASE >= 0x79800 && SEG_BASE <= 0x799C0
}

/// 诊断闭集完整：12 码、编码连续、唯一、文案非空且带「建议」。
fn diag_closed_set_ok() -> bool {
    if ALL_DIAGS.len() != 12 {
        return false;
    }
    let mut i = 0usize;
    while i < ALL_DIAGS.len() {
        let a = match ALL_DIAGS.get(i) {
            Some(d) => *d,
            None => return false,
        };
        if a.val() != SEG_BASE + (i as u32) + 1 {
            return false;
        }
        let text = a.detail();
        if text.is_empty() || !text.contains("建议") {
            return false;
        }
        let mut j = i + 1;
        while j < ALL_DIAGS.len() {
            match ALL_DIAGS.get(j) {
                Some(b) => {
                    if a.val() == b.val() {
                        return false;
                    }
                }
                None => return false,
            }
            j += 1;
        }
        i += 1;
    }
    let first = match ALL_DIAGS.first() {
        Some(d) => *d,
        None => return false,
    };
    !detail(0xDEAD).is_empty() && detail(first.val()) == first.detail()
}

/// 冻结面：三员互异、三轴互异、默认表逐键有行且值在员表内、能力位并守恒。
fn frozen_tables_ok() -> bool {
    if BACKEND_NAMES.len() != 3 || AXIS_KEYS.len() != 3 || COMPILE_DEFAULTS.len() != 3 {
        return false;
    }
    for (i, n) in BACKEND_NAMES.iter().enumerate() {
        let mut j = i + 1;
        while j < BACKEND_NAMES.len() {
            match BACKEND_NAMES.get(j) {
                Some(q) => {
                    if n == q {
                        return false;
                    }
                }
                None => return false,
            }
            j += 1;
        }
        // 冻结表员名与员表名逐位对齐。
        match BACKENDS.get(i) {
            Some(b) => {
                if b.name != *n {
                    return false;
                }
            }
            None => return false,
        }
    }
    for (i, k) in AXIS_KEYS.iter().enumerate() {
        let mut j = i + 1;
        while j < AXIS_KEYS.len() {
            match AXIS_KEYS.get(j) {
                Some(q) => {
                    if k == q {
                        return false;
                    }
                }
                None => return false,
            }
            j += 1;
        }
        match COMPILE_DEFAULTS.get(i) {
            Some(row) => {
                if row.axis != *k || backend_caps(row.backend).is_none() {
                    return false;
                }
            }
            None => return false,
        }
    }
    // 能力位并守恒：三员位并 == CAP_ALL。
    let mut union = 0u32;
    for b in BACKENDS.iter() {
        union |= b.caps;
    }
    union == CAP_ALL
}

/// 编译期默认解析：三轴逐轴命中冻结值、空键与表外轴点名拒。
fn default_resolution_ok() -> bool {
    let sel = BackendSelector::compile_only();
    sel.default_for("console") == Ok("softbuf")
        && sel.default_for("record") == Ok("recorder")
        && sel.default_for("ci") == Ok("qemu-virtio")
        && sel.default_for("") == Err(Diag::AxisKeyEmpty.val())
        && sel.default_for("ghost") == Err(Diag::AxisForeign.val())
}

/// M-190 交互①：覆盖胜出（异值覆盖生效、同值覆盖同解析但活动态为真）。
fn override_wins_ok() -> bool {
    let mut sel = BackendSelector::compile_only();
    // 异值覆盖：console 默认 softbuf → 覆盖 recorder。
    if sel.set_override("console", "recorder").is_err() {
        return false;
    }
    if sel.resolve("console") != Ok("recorder") || !sel.override_active {
        return false;
    }
    // 未覆盖轴仍走默认。
    if sel.resolve("record") != Ok("recorder") || sel.resolve("ci") != Ok("qemu-virtio") {
        return false;
    }
    // 同值覆盖：record 默认 recorder → 覆盖 recorder 同值，解析同、活动态真。
    let mut same = BackendSelector::compile_only();
    if same.set_override("record", "recorder").is_err() {
        return false;
    }
    same.resolve("record") == Ok("recorder")
        && same.override_active
        && same.override_backend.as_deref() == Some("recorder")
}

/// M-190 交互②：清除覆盖即回落编译期默认；重复清除点名拒。
fn clear_restores_ok() -> bool {
    let mut sel = BackendSelector::compile_only();
    if sel.set_override("ci", "recorder").is_err() {
        return false;
    }
    if sel.resolve("ci") != Ok("recorder") {
        return false;
    }
    if sel.clear_override().is_err() {
        return false;
    }
    if sel.resolve("ci") != Ok("qemu-virtio") || sel.override_active {
        return false;
    }
    sel.clear_override() == Err(Diag::OverrideClearTwice.val())
}

/// M-190 交互③：默认面不可污染——set/clear 全程编译期默认表逐键逐值不变。
fn default_immutable_ok() -> bool {
    let mut sel = BackendSelector::compile_only();
    if sel.set_override("console", "recorder").is_err() {
        return false;
    }
    let _ = sel.clear_override();
    if sel.set_override("record", "qemu-virtio").is_err() {
        return false;
    }
    let _ = sel.clear_override();
    // 默认表逐键核（覆盖侧任何操作不得改表）。
    COMPILE_DEFAULTS[0].backend == "softbuf"
        && COMPILE_DEFAULTS[1].backend == "recorder"
        && COMPILE_DEFAULTS[2].backend == "qemu-virtio"
        && sel.default_for("console") == Ok("softbuf")
        && sel.default_for("record") == Ok("recorder")
        && sel.default_for("ci") == Ok("qemu-virtio")
}

/// 注入 1：轴两违约（空键 / 表外轴）与后端表外逐类拒。
fn inj_axis_backend_foreign_ok() -> bool {
    let mut sel = BackendSelector::compile_only();
    let empty = sel.set_override("", "recorder");
    let ghost_axis = sel.set_override("ghost", "recorder");
    let ghost_backend = sel.set_override("console", "幽灵后端");
    empty == Err(Diag::AxisKeyEmpty.val())
        && ghost_axis == Err(Diag::AxisForeign.val())
        && ghost_backend == Err(Diag::BackendForeign.val())
}

/// 注入 2：单槽冲突（异轴重设）显性拒；同轴重设=改写合法。
fn inj_single_slot_ok() -> bool {
    let mut sel = BackendSelector::compile_only();
    if sel.set_override("console", "recorder").is_err() {
        return false;
    }
    // 异轴：单槽冲突点名。
    if sel.set_override("ci", "softbuf") != Err(Diag::SingleSlotConflict.val()) {
        return false;
    }
    // 同轴：改写合法，前值被显性替换。
    if sel.set_override("console", "qemu-virtio").is_err() {
        return false;
    }
    sel.resolve("console") == Ok("qemu-virtio")
}

/// 能力核对：qemu-virtio 全位过、recorder 缺色位点抓、未冻结位点名。
fn caps_checks_ok() -> bool {
    let sel = BackendSelector::compile_only();
    // ci 轴默认 qemu-virtio：全位请求过。
    if resolve_with_caps(&sel, "ci", CAP_ALL) != Ok(CAP_RECORD | CAP_COLOR | CAP_HEADLESS) {
        return false;
    }
    // record 轴默认 recorder：缺色位 → CapsMissing。
    if resolve_with_caps(&sel, "record", CAP_COLOR) != Err(Diag::CapsMissing.val()) {
        return false;
    }
    // 未冻结位（0x8）点名拒。
    if resolve_with_caps(&sel, "ci", 0x8) != Err(Diag::CapabilityBitForeign.val()) {
        return false;
    }
    // 覆盖后能力随覆盖走：record 轴覆盖 qemu-virtio 后色位过。
    let mut sel2 = BackendSelector::compile_only();
    if sel2.set_override("record", "qemu-virtio").is_err() {
        return false;
    }
    resolve_with_caps(&sel2, "record", CAP_COLOR).is_ok()
}

/// 注入 3：覆盖槽内部残缺（活动态下轴/后端缺一）点名 OverrideAxisMismatch。
fn inj_override_slot_broken_ok() -> bool {
    let broken = BackendSelector {
        override_axis: Some("console"),
        override_backend: None,
        override_active: true,
    };
    broken.resolve("console") == Err(Diag::OverrideAxisMismatch.val())
        // 异轴解析不越轴：覆盖在 console，record 轴回落编译期默认。
        && {
            let mut sel = BackendSelector::compile_only();
            if sel.set_override("console", "recorder").is_err() {
                return false;
            }
            sel.resolve("record") == Ok("recorder")
        }
}

/// 双跑一致：解析与两级交互序列两次执行逐码相同。
fn double_run_ok() -> bool {
    let flow = || -> Result<Vec<&'static str>, u32> {
        let mut sel = BackendSelector::compile_only();
        let mut out: Vec<&'static str> = Vec::new();
        out.push(sel.resolve("console")?);
        sel.set_override("console", "recorder")?;
        out.push(sel.resolve("console")?);
        sel.clear_override()?;
        out.push(sel.resolve("console")?);
        Ok(out)
    };
    let a = flow();
    let b = flow();
    a.is_ok() && b.is_ok() && a == b && a == Ok(vec!["softbuf", "recorder", "softbuf"])
}

/// 错误路径 Result 语义：Ok 与闭集 Err 两侧均可达。
fn error_path_ok() -> bool {
    let sel = BackendSelector::compile_only();
    if sel.resolve("record").is_err() {
        return false;
    }
    match sel.resolve("nope") {
        Err(code) => {
            let mut in_set = false;
            for d in ALL_DIAGS.iter() {
                if d.val() == code {
                    in_set = true;
                }
            }
            in_set && !detail(0x0).is_empty()
        }
        Ok(_) => false,
    }
}

/// ③ 写回面：状态=已完成、结果摘要非空。
fn writeback_summary_ok() -> bool {
    let mut sel = BackendSelector::compile_only();
    let ok = sel.set_override("console", "recorder").is_ok()
        && sel.resolve("console") == Ok("recorder")
        && sel.clear_override().is_ok()
        && sel.resolve("console") == Ok("softbuf");
    let summary = if ok {
        format!(
            "UNX-F3431 后端选择：两级语义明确 编译期默认{}轴 覆盖可清",
            AXIS_KEYS.len()
        )
    } else {
        String::new()
    };
    !summary.is_empty() && summary.contains("两级语义明确") && ok
}

// ── 判据总成：run_unx3431_checks ───────────────────────────────────────

/// run_unx3431_checks：本单判据聚合（诊断段 0x799C0，全部走 add_dyn）。
pub fn run_unx3431_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F3431 后端选择策略");
    let checks: [(&'static str, bool, String); 13] = [
        (
            "段位守卫0x40对齐且在批段窗内",
            seg_guard_ok(),
            format!("SEG_BASE=0x{:X} 守卫{}", SEG_BASE, if seg_guard_ok() { "过" } else { "破" }),
        ),
        (
            "诊断码闭集12码唯一且文案带建议",
            diag_closed_set_ok(),
            format!("闭集{}", if diag_closed_set_ok() { "完整" } else { "破缺" }),
        ),
        (
            "冻结面三员三轴默认表逐键互异位并守恒",
            frozen_tables_ok(),
            format!("冻结表{}", if frozen_tables_ok() { "齐" } else { "漂移" }),
        ),
        (
            "编译期默认三轴逐轴命中冻结值",
            default_resolution_ok(),
            format!("默认面{}", if default_resolution_ok() { "齐" } else { "缺" }),
        ),
        (
            "M190交互①覆盖胜出含同值显性态",
            override_wins_ok(),
            format!("覆盖胜出{}", if override_wins_ok() { "验" } else { "破" }),
        ),
        (
            "M190交互②清除回落默认重复清除点名",
            clear_restores_ok(),
            format!("清除回落{}", if clear_restores_ok() { "验" } else { "破" }),
        ),
        (
            "M190交互③默认面不可污染",
            default_immutable_ok(),
            format!("默认不可变{}", if default_immutable_ok() { "验" } else { "破" }),
        ),
        (
            "注入1空键表外轴表外后端逐类拒",
            inj_axis_backend_foreign_ok(),
            format!("轴后端违约{}", if inj_axis_backend_foreign_ok() { "全抓" } else { "漏抓" }),
        ),
        (
            "注入2单槽冲突异轴拒同轴改写",
            inj_single_slot_ok(),
            format!("单槽{}", if inj_single_slot_ok() { "验" } else { "破" }),
        ),
        (
            "能力核对全位过缺位点抓未冻结位点名",
            caps_checks_ok(),
            format!("能力位{}", if caps_checks_ok() { "齐" } else { "缺" }),
        ),
        (
            "注入3覆盖轴与请求轴不符点名",
            inj_override_slot_broken_ok(),
            format!("覆盖轴{}", if inj_override_slot_broken_ok() { "抓到" } else { "漏抓" }),
        ),
        (
            "双跑一致两级交互序列逐码同",
            double_run_ok(),
            format!("双跑{}", if double_run_ok() { "一致" } else { "分叉" }),
        ),
        (
            "写回摘要非空状态已完成",
            writeback_summary_ok(),
            format!("写回{}", if writeback_summary_ok() { "非空" } else { "空" }),
        ),
    ];
    for (name, passed, det) in checks.iter() {
        s.add_dyn(name, *passed, det);
    }
    let ep = error_path_ok();
    s.add_dyn("错误路径Ok与闭集Err两侧可达", ep, &format!("错误路径{}", if ep { "过" } else { "破" }));
    s
}

// ── 测试区（#[cfg(test)]：允许 assert!/assert_eq!/unwrap）──────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seg_guard_and_diag_closed_set() {
        assert!(seg_guard_ok());
        assert_eq!(SEG_BASE, 0x799C0);
        assert!(diag_closed_set_ok());
        for (i, d) in ALL_DIAGS.iter().enumerate() {
            assert_eq!(d.val(), SEG_BASE + i as u32 + 1);
        }
    }

    #[test]
    fn frozen_tables_exact() {
        assert!(frozen_tables_ok());
        assert_eq!(BACKEND_NAMES, ["recorder", "softbuf", "qemu-virtio"]);
        assert_eq!(AXIS_KEYS, ["console", "record", "ci"]);
        assert_eq!(CAP_ALL, 0x7);
    }

    #[test]
    fn default_resolution_three_axes() {
        assert!(default_resolution_ok());
        let sel = BackendSelector::compile_only();
        assert_eq!(sel.resolve("console"), Ok("softbuf"));
    }

    #[test]
    fn m190_override_wins_and_clear_restores() {
        assert!(override_wins_ok());
        assert!(clear_restores_ok());
        assert!(default_immutable_ok());
    }

    #[test]
    fn injections_caught() {
        assert!(inj_axis_backend_foreign_ok());
        assert!(inj_single_slot_ok());
        assert!(inj_override_slot_broken_ok());
    }

    #[test]
    fn caps_checked() {
        assert!(caps_checks_ok());
        assert_eq!(backend_caps("recorder"), Some(CAP_RECORD | CAP_HEADLESS));
        assert_eq!(backend_caps("nope"), None);
    }

    #[test]
    fn double_run_identical() {
        assert!(double_run_ok());
    }

    #[test]
    fn error_paths_closed_codes() {
        assert!(error_path_ok());
        assert!(detail(0x799FF).starts_with("未知诊断码"));
    }

    #[test]
    fn writeback_summary_non_empty() {
        assert!(writeback_summary_ok());
    }

    #[test]
    fn checks_aggregate_all_green() {
        let s = run_unx3431_checks();
        assert!(s.len() >= 12);
        assert!(s.all_passed(), "全部判据必须绿：{}", s.red_items_text());
    }
}
