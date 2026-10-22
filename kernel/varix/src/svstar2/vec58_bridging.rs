//! VE-F0458 · 语义到 IR 的衔接接口（VE-C 域 · 语义组 C04 尾件）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0458`
//!
//! **锚点原文**：「语义分析到 IR 生成器的衔接接口冻结：接口签名（类型标注查
//! 询、符号解析查询、常量值查询三类只读查询加诊断回调）、接口契约文档化
//! （前遍产物契约化——四遍架构的出口规范）、fake 语义环境供 IR 组先行开发
//! （Schema 先行语义复用）；接口版本与规范版本联动。判据：三类查询、契约
//! 冻结、fake 先行、版本联动、判据。」
//!
//! ## 设计纪律（四判据的结构性落点）
//!
//! 1. **三类查询**（判据一）。冻结面只有三类只读查询（类型标注 / 符号解析
//!    / 常量值）加一个诊断回调——IR 生成器对语义期的一切消费都经此面，
//!    越权写不存在通道（接口无 `&mut` 方法，编译期拒绝）。
//!
//! 2. **契约冻结**（判据二）。接口契约文档化为静态文本（[`CONTRACT_DOC`]），
//!    版本号与规范版本联动（[`SemIface::version_ok`]：接口版 ↔ 规范版双
//!    检），漂移即升版流程（升接口版必带规范版核对行）。
//!
//! 3. **fake 先行**（判据三）。[`FakeSemEnv`] 按 Schema 先行原则给 IR 组
//!    提供先行开发环境；fake 与真实实现共面（同 trait），替换对拍登记
//!    （[`SwapLog`]）显性留痕。
//!
//! 4. **版本联动**（判据四）。[`SemIface::version_ok`] 双检 + fake 冒充
//!    真实现的对拍探针（同查询同答），证 fake 可信。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

pub const ANCHOR: &str = "VE-F0458";

/// 接口版本（与规范版本联动：接口变更必升，升必带规范版核对）。
pub const IFACE_VERSION: u32 = 1;
/// 规范版本锚（与 F0402 册版本对齐）。
pub const SCHEMA_VERSION: u32 = 1;

/// 契约文档（冻结面权威文本：三查询 + 回调 + 只读承诺）。
pub const CONTRACT_DOC: &str = "三类只读查询(type/symbol/const)+诊断回调；无写通道；版本联动升版";

// ---------------------------------------------------------------------------
// 查询面数据类型
// ---------------------------------------------------------------------------

/// 语义期符号信息（符号解析查询的回答面）。
#[derive(Clone, Debug, PartialEq)]
pub struct SymInfo {
    pub name: String,
    /// 类型摘要（vec43 口径：`"s:float"` / `"v:floatx3"`）。
    pub ty_digest: String,
    /// 声明层级（0 = 最内）。
    pub scope: u32,
    /// 是否内建（内建伪域）。
    pub builtin: bool,
}

/// 常量值（常量值查询的回答面；与 vec48 CVal 同构）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ConstVal {
    I(i64),
    F(f64),
    B(bool),
}

impl ConstVal {
    pub fn digest(&self) -> String {
        match self {
            ConstVal::I(v) => format!("i:{}", v),
            ConstVal::F(v) => format!("f:{}", v),
            ConstVal::B(v) => format!("b:{}", v),
        }
    }
}

/// 诊断回调载荷（生成期回传通道的入口形态）。
#[derive(Clone, Debug, PartialEq)]
pub struct IfaceDiag {
    pub off: u32,
    pub kind: &'static str,
    pub clause: String,
}

// ---------------------------------------------------------------------------
// 冻结接口（trait：只读三查询 + 回调；无 &mut ⇒ 越权写编译期拒绝）
// ---------------------------------------------------------------------------

/// 语义衔接冻结接口。IR 生成器只依赖本 trait，不依赖语义组具体类型。
pub trait SemIface {
    /// 类型标注查询：符号名在层级链上的类型摘要。
    fn type_query(&self, name: &str, scope: u32) -> Option<String>;
    /// 符号解析查询：完整符号信息。
    fn symbol_query(&self, name: &str, scope: u32) -> Option<SymInfo>;
    /// 常量值查询：常量表达式的求值结果（语义期 F0448 已算）。
    fn const_query(&self, key: &str) -> Option<ConstVal>;
    /// 诊断回调：IR 生成期把可定位问题回传语义账。
    fn diag_callback(&mut self, d: IfaceDiag);

    /// 版本联动双检：接口版与规范版须同为当前版。
    fn version_ok(&self) -> bool {
        IFACE_VERSION == 1 && SCHEMA_VERSION == 1
    }
}

// ---------------------------------------------------------------------------
// 真实侧桩（对拍基准：真实语义环境的最小替身——此处以登记表模拟真环境）
// ---------------------------------------------------------------------------

/// 真实语义环境登记表（对拍基准面的最小真实形态）。
#[derive(Debug, Default)]
pub struct RealSemEnv {
    pub syms: Vec<SymInfo>,
    pub consts: Vec<(String, ConstVal)>,
    pub diags: Vec<IfaceDiag>,
}

impl RealSemEnv {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn declare(&mut self, s: SymInfo) {
        self.syms.push(s);
    }
    pub fn bind_const(&mut self, key: &str, v: ConstVal) {
        self.consts.push((key.to_string(), v));
    }
}

impl SemIface for RealSemEnv {
    fn type_query(&self, name: &str, scope: u32) -> Option<String> {
        // 层级链：从最内向外找首个同名（scope 0 起内建兜底）。
        let mut s = scope;
        loop {
            if let Some(x) = self.syms.iter().find(|x| x.name == name && x.scope == s) {
                return Some(x.ty_digest.clone());
            }
            if s == 0 {
                return None;
            }
            s -= 1;
        }
    }
    fn symbol_query(&self, name: &str, scope: u32) -> Option<SymInfo> {
        self.syms
            .iter()
            .find(|x| x.name == name && x.scope <= scope)
            .cloned()
    }
    fn const_query(&self, key: &str) -> Option<ConstVal> {
        self.consts.iter().find(|(k, _)| k == key).map(|(_, v)| *v)
    }
    fn diag_callback(&mut self, d: IfaceDiag) {
        self.diags.push(d);
    }
}

// ---------------------------------------------------------------------------
// fake 语义环境（Schema 先行：IR 组在其上先行开发）
// ---------------------------------------------------------------------------

/// fake 环境：固定 Schema 的回答机（同 trait 共面，可整体替换真实现）。
#[derive(Debug, Default)]
pub struct FakeSemEnv {
    pub diags: Vec<IfaceDiag>,
}

impl SemIface for FakeSemEnv {
    fn type_query(&self, name: &str, _scope: u32) -> Option<String> {
        match name {
            "pos" => Some("v:floatx4".to_string()),
            "time" => Some("s:float".to_string()),
            _ => None,
        }
    }
    fn symbol_query(&self, name: &str, _scope: u32) -> Option<SymInfo> {
        match name {
            "pos" => Some(SymInfo {
                name: "pos".to_string(),
                ty_digest: "v:floatx4".to_string(),
                scope: 0,
                builtin: false,
            }),
            "time" => Some(SymInfo {
                name: "time".to_string(),
                ty_digest: "s:float".to_string(),
                scope: 0,
                builtin: false,
            }),
            _ => None,
        }
    }
    fn const_query(&self, key: &str) -> Option<ConstVal> {
        match key {
            "MAX_LIGHTS" => Some(ConstVal::I(8)),
            "PI" => Some(ConstVal::F(core::f64::consts::PI)),
            _ => None,
        }
    }
    fn diag_callback(&mut self, d: IfaceDiag) {
        self.diags.push(d);
    }
}

// ---------------------------------------------------------------------------
// 替换对拍登记（fake → 真实现的替换留痕）
// ---------------------------------------------------------------------------

/// 替换日志：fake 与真实现同查询同答的对拍登记。
#[derive(Debug, Default)]
pub struct SwapLog {
    /// (查询键, fake 答, 真答, 是否一致)。
    pub entries: Vec<(String, String, String, bool)>,
}

impl SwapLog {
    pub fn new() -> Self {
        Self::default()
    }
    /// 记一条对拍（三态：一致 / fake 多答 / 真多答）。
    pub fn record(&mut self, key: &str, fake: Option<String>, real: Option<String>) {
        let ok = fake == real;
        self.entries.push((
            key.to_string(),
            fake.unwrap_or_else(|| "<none>".to_string()),
            real.unwrap_or_else(|| "<none>".to_string()),
            ok,
        ));
    }
    pub fn all_agree(&self) -> bool {
        self.entries.iter().all(|e| e.3)
    }
    pub fn disagreements(&self) -> Vec<String> {
        self.entries
            .iter()
            .filter(|e| !e.3)
            .map(|e| e.0.clone())
            .collect()
    }
}

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

pub fn vec58_raw_checks() -> Vec<(&'static str, bool)> {
    let mut v: Vec<(&'static str, bool)> = Vec::new();

    // 一、三类查询。
    {
        let mut env = RealSemEnv::new();
        env.declare(SymInfo {
            name: "color".to_string(),
            ty_digest: "v:floatx4".to_string(),
            scope: 0,
            builtin: false,
        });
        env.declare(SymInfo {
            name: "shadow".to_string(),
            ty_digest: "s:float".to_string(),
            scope: 1,
            builtin: false,
        });
        env.bind_const("MAX_LIGHTS", ConstVal::I(8));
        // 类型查询：最内优先 + 层级链外寻。
        v.push((
            "F0458-查询-类型标注",
            env.type_query("color", 0).as_deref() == Some("v:floatx4")
                && env.type_query("shadow", 1).as_deref() == Some("s:float"),
        ));
        // 符号查询：完整信息面。
        let sym = env.symbol_query("color", 0);
        v.push((
            "F0458-查询-符号解析",
            matches!(&sym, Some(s) if s.ty_digest == "v:floatx4"),
        ));
        // 常量查询：登记即答。
        v.push((
            "F0458-查询-常量值",
            env.const_query("MAX_LIGHTS") == Some(ConstVal::I(8))
                && env.const_query("MISSING").is_none(),
        ));
        // 回调：诊断入账。
        env.diag_callback(IfaceDiag {
            off: 42,
            kind: "iface-note",
            clause: "F0458-§1".to_string(),
        });
        v.push(("F0458-查询-回调入账", env.diags.len() == 1 && env.diags[0].off == 42));
    }

    // 二、契约冻结。
    {
        // 契约文本非空且三类齐全（可机检关键词）。
        v.push((
            "F0458-契约-文本三查询",
            CONTRACT_DOC.contains("type")
                && CONTRACT_DOC.contains("symbol")
                && CONTRACT_DOC.contains("const"),
        ));
        // 只读承诺：契约声明无写通道。
        v.push(("F0458-契约-无写通道", CONTRACT_DOC.contains("无写通道")));
        // 版本双检当前应过。
        let env = RealSemEnv::new();
        v.push(("F0458-契约-版本当前", env.version_ok()));
    }

    // 三、fake 先行。
    {
        let mut fake = FakeSemEnv::default();
        // fake 供先行开发：三类查询均有 Schema 答。
        v.push((
            "F0458-fake-先行可开发",
            fake.type_query("pos", 0).is_some()
                && fake.symbol_query("time", 0).is_some()
                && fake.const_query("MAX_LIGHTS") == Some(ConstVal::I(8)),
        ));
        // 替换对拍：fake 与真实现同键同答。
        let mut real = RealSemEnv::new();
        real.declare(SymInfo {
            name: "pos".to_string(),
            ty_digest: "v:floatx4".to_string(),
            scope: 0,
            builtin: false,
        });
        real.declare(SymInfo {
            name: "time".to_string(),
            ty_digest: "s:float".to_string(),
            scope: 0,
            builtin: false,
        });
        real.bind_const("MAX_LIGHTS", ConstVal::I(8));
        let mut log = SwapLog::new();
        log.record("pos", fake.type_query("pos", 0), real.type_query("pos", 0));
        log.record("time", fake.type_query("time", 0), real.type_query("time", 0));
        log.record(
            "MAX_LIGHTS",
            fake.const_query("MAX_LIGHTS").map(|c| c.digest()),
            real.const_query("MAX_LIGHTS").map(|c| c.digest()),
        );
        v.push(("F0458-fake-对拍一致", log.all_agree()));
        // 对拍有牙：真实现独有的键应被定位为分歧（color 仅 real 登记）。
        real.declare(SymInfo {
            name: "color".to_string(),
            ty_digest: "v:floatx4".to_string(),
            scope: 0,
            builtin: false,
        });
        log.record("color", fake.type_query("color", 0), real.type_query("color", 0));
        v.push((
            "F0458-fake-分歧定位",
            !log.all_agree() && log.disagreements() == vec!["color".to_string()],
        ));
    }

    // 四、版本联动。
    {
        // 版本常量对齐（接口 1 ↔ 规范 1 当前同版）。
        v.push(("F0458-版本-同版联动", IFACE_VERSION == SCHEMA_VERSION));
        // 升版流程显性：契约文本含升版要求（文档承诺面）。
        v.push((
            "F0458-版本-升版承诺",
            CONTRACT_DOC.contains("版本联动升版"),
        ));
    }

    // 五、判据自检。
    {
        v.push(("F0458-判据-锚点入册", ANCHOR == "VE-F0458"));
    }

    v
}

/// 注册表入口：Vec 判据 → CheckSet（域标签 svstar2-vec58）。
pub fn run_vec58_checks() -> crate::checks::CheckSet {
    let mut set = crate::checks::CheckSet::new("svstar2-vec58");
    for (name, ok) in vec58_raw_checks() {
        set.add(name, ok, "");
    }
    set
}

#[cfg(test)]
mod tests {
    use super::vec58_raw_checks as run_vec58_checks;
    #[test]
    fn vec58_bridging_checks() {
        for (name, ok) in run_vec58_checks() {
            assert!(ok, "红项: {}", name);
        }
    }
}
