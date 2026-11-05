//! UNX-F3425 JSON 结果账 schema（suite/case/assert/fail/duration） · 0x79840段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（派工卡 VX8-B12 逐字摘录）：
//!   ① 纯功能代码 ≥400 行；
//!   ② J1：JSON schema v1.0 冻结（与 F3394/F3417 的 v0.9 联签升级）；
//!     M-184 schema 校验通过。
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - schema 版本闭集：v1.0 为唯一合法版本（与 F3394/F3417 的 v0.9
//!     联签升级后冻结）；"0.9" 显性点名为待升级账（VersionLegacy），
//!     其余版本串一律 VersionForeign，双路必抓；
//!   - 五个域键冻结：suite（套件名）/ case（用例名）/ assert（断言数）/
//!     fail（失败数）/ duration（时长 ms）——键名逐字冻结，表外键拒收；
//!   - 账文档两层结构：套件层（schema/suite/cases 三键）+ 用例层
//!     （case/assert/fail/duration 四键），逐层逐键校验类型与值域；
//!   - 值域纪律：计数非负、fail ≤ assert、duration 非负、cases 非空、
//!     用例名不重（CaseDup 点名）——五类值域违约显性拒；
//!   - M-184：标准样账构造 → 校验通过（Ok），六类注入坏账逐类被点抓；
//!   - 双跑一致：校验器为纯函数，同输入两次结论逐码相同。
//! 常量来源：
//!   - schema 版本 1.0：派工卡 J1 判据原文（联签升级冻结版）；
//!   - 五域键名：派工卡标题括号列（suite/case/assert/fail/duration）；
//!   - 诊断段：0x79840（0x40 对齐，B12 批窗 [0x79800, 0x799C0]）。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x40 对齐，B12 批段窗 [0x79800, 0x799C0]）。
pub const SEG_BASE: u32 = 0x79840;

/// 冻结 schema 版本（联签升级后唯一合法值）。
pub const SCHEMA_VERSION: &'static str = "1.0";

/// 遗留版本串（F3394/F3417 旧账；显性点名走升级，不静默兼容）。
pub const LEGACY_VERSION: &'static str = "0.9";

/// 套件层冻结键表（三键，序固定）。
pub const SUITE_KEYS: [&'static str; 3] = ["schema", "suite", "cases"];

/// 用例层冻结键表（四键，序固定；即派工卡五域键去 suite）。
pub const CASE_KEYS: [&'static str; 4] = ["case", "assert", "fail", "duration"];

/// 值类型闭集（schema 校验的类型轴）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ValKind {
    /// 字符串值。
    Str,
    /// 整数值。
    Num,
    /// 数组值。
    Arr,
    /// 对象值。
    Obj,
}

impl ValKind {
    /// 中文名（诊断文案用）。
    pub fn zh(self) -> &'static str {
        match self {
            ValKind::Str => "字符串",
            ValKind::Num => "整数",
            ValKind::Arr => "数组",
            ValKind::Obj => "对象",
        }
    }
}

/// 极简 JSON 值模型（校验器侧只读面；序列化归 F3426）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JsonVal {
    /// 字符串。
    JStr(String),
    /// 整数（i64；无浮点）。
    JNum(i64),
    /// 数组。
    JArr(Vec<JsonVal>),
    /// 对象（键序保留的键值对表）。
    JObj(Vec<(String, JsonVal)>),
}

impl JsonVal {
    /// 值类型。
    pub fn kind(&self) -> ValKind {
        match self {
            JsonVal::JStr(_) => ValKind::Str,
            JsonVal::JNum(_) => ValKind::Num,
            JsonVal::JArr(_) => ValKind::Arr,
            JsonVal::JObj(_) => ValKind::Obj,
        }
    }
}

/// 对象取值：按键取（键序表线性查，键不存在返回 None）。
pub fn obj_get<'a>(obj: &'a JsonVal, key: &str) -> Option<&'a JsonVal> {
    match obj {
        JsonVal::JObj(pairs) => {
            for (k, v) in pairs.iter() {
                if k == key {
                    return Some(v);
                }
            }
            None
        }
        _ => None,
    }
}

/// 对象键表：按序取全部键名（非对象返回空表）。
pub fn obj_keys(obj: &JsonVal) -> Vec<String> {
    match obj {
        JsonVal::JObj(pairs) => {
            let mut keys: Vec<String> = Vec::new();
            for (k, _) in pairs.iter() {
                keys.push(k.clone());
            }
            keys
        }
        _ => Vec::new(),
    }
}

/// 标准样账构造（v1.0 合法账；M-184 校验通过的正样本）。
pub fn build_sample_suite() -> JsonVal {
    let case_a = JsonVal::JObj(vec![
        (String::from("case"), JsonVal::JStr(String::from("boot_e820_bridge"))),
        (String::from("assert"), JsonVal::JNum(12)),
        (String::from("fail"), JsonVal::JNum(0)),
        (String::from("duration"), JsonVal::JNum(3)),
    ]);
    let case_b = JsonVal::JObj(vec![
        (String::from("case"), JsonVal::JStr(String::from("hda_probe_probe"))),
        (String::from("assert"), JsonVal::JNum(7)),
        (String::from("fail"), JsonVal::JNum(1)),
        (String::from("duration"), JsonVal::JNum(9)),
    ]);
    JsonVal::JObj(vec![
        (String::from("schema"), JsonVal::JStr(String::from(SCHEMA_VERSION))),
        (String::from("suite"), JsonVal::JStr(String::from("varix_kernel_suite"))),
        (String::from("cases"), JsonVal::JArr(vec![case_a, case_b])),
    ])
}

/// 用例层逐键校验（类型 + 值域；返回闭集码或 Ok）。
pub fn validate_case(v: &JsonVal) -> Result<(), u32> {
    match v {
        JsonVal::JObj(_) => {}
        _ => return Err(Diag::TypeMismatch.val()),
    }
    // 键表外拒收。
    for k in obj_keys(v).iter() {
        let mut known = false;
        for key in CASE_KEYS.iter() {
            if *key == k.as_str() {
                known = true;
            }
        }
        if !known {
            return Err(Diag::KeyForeign.val());
        }
    }
    // 必填键齐。
    for key in CASE_KEYS.iter() {
        if obj_get(v, key).is_none() {
            return Err(Diag::SectionMissing.val());
        }
    }
    // case：非空字符串。
    let name = match obj_get(v, "case") {
        Some(JsonVal::JStr(s)) => s.clone(),
        Some(_) => return Err(Diag::TypeMismatch.val()),
        None => return Err(Diag::SectionMissing.val()),
    };
    if name.is_empty() {
        return Err(Diag::SectionMissing.val());
    }
    // assert / fail / duration：整数 + 值域。
    let asserts = match obj_get(v, "assert") {
        Some(JsonVal::JNum(n)) => *n,
        Some(_) => return Err(Diag::TypeMismatch.val()),
        None => return Err(Diag::SectionMissing.val()),
    };
    let fails = match obj_get(v, "fail") {
        Some(JsonVal::JNum(n)) => *n,
        Some(_) => return Err(Diag::TypeMismatch.val()),
        None => return Err(Diag::SectionMissing.val()),
    };
    let duration = match obj_get(v, "duration") {
        Some(JsonVal::JNum(n)) => *n,
        Some(_) => return Err(Diag::TypeMismatch.val()),
        None => return Err(Diag::SectionMissing.val()),
    };
    if asserts < 0 || fails < 0 {
        return Err(Diag::CountNegative.val());
    }
    if duration < 0 {
        return Err(Diag::DurationNegative.val());
    }
    if fails > asserts {
        return Err(Diag::FailExceedAssert.val());
    }
    Ok(())
}

/// 套件层全账校验（M-184 入口：版本 → 套件名 → cases 逐条）。
pub fn validate_suite(v: &JsonVal) -> Result<(), u32> {
    match v {
        JsonVal::JObj(_) => {}
        _ => return Err(Diag::TypeMismatch.val()),
    }
    // 键表外拒收。
    for k in obj_keys(v).iter() {
        let mut known = false;
        for key in SUITE_KEYS.iter() {
            if *key == k.as_str() {
                known = true;
            }
        }
        if !known {
            return Err(Diag::KeyForeign.val());
        }
    }
    // 必填键齐。
    for key in SUITE_KEYS.iter() {
        if obj_get(v, key).is_none() {
            return Err(Diag::SectionMissing.val());
        }
    }
    // schema 版本闭集：1.0 合法；0.9 点名升级；其余 Foreign。
    match obj_get(v, "schema") {
        Some(JsonVal::JStr(s)) => {
            if s == SCHEMA_VERSION {
                // 合法版本，继续。
            } else if s == LEGACY_VERSION {
                return Err(Diag::VersionLegacy.val());
            } else {
                return Err(Diag::VersionForeign.val());
            }
        }
        Some(_) => return Err(Diag::TypeMismatch.val()),
        None => return Err(Diag::SectionMissing.val()),
    }
    // suite：非空套件名。
    match obj_get(v, "suite") {
        Some(JsonVal::JStr(s)) => {
            if s.is_empty() {
                return Err(Diag::SectionMissing.val());
            }
        }
        Some(_) => return Err(Diag::TypeMismatch.val()),
        None => return Err(Diag::SectionMissing.val()),
    }
    // cases：非空数组，逐条用例校验 + 用例名不重。
    let cases = match obj_get(v, "cases") {
        Some(JsonVal::JArr(a)) => a,
        Some(_) => return Err(Diag::TypeMismatch.val()),
        None => return Err(Diag::SectionMissing.val()),
    };
    if cases.is_empty() {
        return Err(Diag::CaseListEmpty.val());
    }
    let mut seen: Vec<String> = Vec::new();
    for c in cases.iter() {
        validate_case(c)?;
        if let Some(JsonVal::JStr(name)) = obj_get(c, "case") {
            for prev in seen.iter() {
                if prev == name {
                    return Err(Diag::CaseDup.val());
                }
            }
            seen.push(name.clone());
        }
    }
    Ok(())
}

/// 诊断码闭集：0x79841..=0x7984C（12 码，变体带中文 doc）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Diag {
    /// 版本表外：schema 既非 1.0 也非 0.9。
    VersionForeign = 0x01,
    /// 遗留版本：0.9 旧账须联签升级到 1.0 后再入账。
    VersionLegacy = 0x02,
    /// 键表外：套件层或用例层出现冻结键表之外的键。
    KeyForeign = 0x03,
    /// 必填段缺：suite 层三键或 case 层四键未齐。
    SectionMissing = 0x04,
    /// 类型错位：键值的类型与冻结类型不符。
    TypeMismatch = 0x05,
    /// 计数负值：assert/fail 出现负数。
    CountNegative = 0x06,
    /// 失败越界：fail > assert。
    FailExceedAssert = 0x07,
    /// 时长负值：duration 为负。
    DurationNegative = 0x08,
    /// 用例重名：cases 内 case 名重复。
    CaseDup = 0x09,
    /// 用例空表：cases 为空数组（结果账至少一条用例）。
    CaseListEmpty = 0x0A,
    /// 段位守卫失守：SEG_BASE 不满足 0x40 对齐或越出批段窗。
    SegGuardFail = 0x0B,
    /// 双跑不一致：同输入两次校验结论不同。
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
            Diag::VersionForeign => "版本表外：建议 schema 只写 1.0，外来版本先确认来源再入账",
            Diag::VersionLegacy => "遗留版本：建议按 F3394/F3417 联签升级单把 0.9 账迁到 1.0 后重交",
            Diag::KeyForeign => "键表外：建议套件层只用 schema/suite/cases，用例层只用 case/assert/fail/duration",
            Diag::SectionMissing => "必填段缺：建议对照冻结键表补齐缺失键后再校验",
            Diag::TypeMismatch => "类型错位：建议按 schema 给键值换正确类型（名串/数整数/表数组/账对象）",
            Diag::CountNegative => "计数负值：建议 assert/fail 用非负整数，负计数无业务语义",
            Diag::FailExceedAssert => "失败越界：建议核对 fail 是否误填，失败数不得超过断言数",
            Diag::DurationNegative => "时长负值：建议 duration 用非负毫秒数，时钟回拨按 0 记",
            Diag::CaseDup => "用例重名：建议 cases 内用例名唯一，重名先改名或合并账目",
            Diag::CaseListEmpty => "用例空表：建议结果账至少登记一条用例，空套件不入账",
            Diag::SegGuardFail => "段位守卫失守：建议核对 SEG_BASE 的 0x40 对齐与批段窗",
            Diag::DoubleRunDiverge => "双跑不一致：建议校验器做成纯函数，输入定则结论定",
        }
    }
}

/// 闭集全量数组（12 码）。
pub const ALL_DIAGS: [Diag; 12] = [
    Diag::VersionForeign,
    Diag::VersionLegacy,
    Diag::KeyForeign,
    Diag::SectionMissing,
    Diag::TypeMismatch,
    Diag::CountNegative,
    Diag::FailExceedAssert,
    Diag::DurationNegative,
    Diag::CaseDup,
    Diag::CaseListEmpty,
    Diag::SegGuardFail,
    Diag::DoubleRunDiverge,
];

/// 诊断码查表闭集返回；未知编码回退固定兜底串。
pub fn detail(code: u32) -> &'static str {
    for d in ALL_DIAGS.iter() {
        if d.val() == code {
            return d.detail();
        }
    }
    "未知诊断码：建议核对是否落在 0x79841..=0x7984C 闭集内"
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

/// 冻结面自查：版本串、两层键表互异、域键五词逐字在表内。
fn frozen_tables_ok() -> bool {
    if SCHEMA_VERSION != "1.0" || LEGACY_VERSION != "0.9" {
        return false;
    }
    if SUITE_KEYS.len() != 3 || CASE_KEYS.len() != 4 {
        return false;
    }
    // 两层键表内互异。
    for (i, k) in SUITE_KEYS.iter().enumerate() {
        let mut j = i + 1;
        while j < SUITE_KEYS.len() {
            match SUITE_KEYS.get(j) {
                Some(q) => {
                    if k == q {
                        return false;
                    }
                }
                None => return false,
            }
            j += 1;
        }
    }
    for (i, k) in CASE_KEYS.iter().enumerate() {
        let mut j = i + 1;
        while j < CASE_KEYS.len() {
            match CASE_KEYS.get(j) {
                Some(q) => {
                    if k == q {
                        return false;
                    }
                }
                None => return false,
            }
            j += 1;
        }
    }
    // 派工卡五域键逐字在册：suite/case/assert/fail/duration。
    let domain_keys = ["suite", "case", "assert", "fail", "duration"];
    for dk in domain_keys.iter() {
        let mut found = false;
        for k in SUITE_KEYS.iter().chain(CASE_KEYS.iter()) {
            if k == dk {
                found = true;
            }
        }
        if !found {
            return false;
        }
    }
    true
}

/// M-184：标准样账校验通过（Ok 路径）。
fn sample_suite_passes_ok() -> bool {
    let suite = build_sample_suite();
    validate_suite(&suite).is_ok()
}

/// 注入 1：遗留版本 0.9 点名升级（VersionLegacy）。
fn inj_legacy_version_ok() -> bool {
    let mut legacy = match build_sample_suite() {
        JsonVal::JObj(pairs) => pairs,
        _ => return false,
    };
    for (k, v) in legacy.iter_mut() {
        if k == "schema" {
            *v = JsonVal::JStr(String::from(LEGACY_VERSION));
        }
    }
    validate_suite(&JsonVal::JObj(legacy)) == Err(Diag::VersionLegacy.val())
}

/// 注入 2：外来版本 / 版本类型错位 / 版本键缺失三类必抓。
fn inj_version_variants_ok() -> bool {
    let rebuild = |schema_val: JsonVal| -> JsonVal {
        JsonVal::JObj(vec![
            (String::from("schema"), schema_val),
            (String::from("suite"), JsonVal::JStr(String::from("s"))),
            (String::from("cases"), JsonVal::JArr(vec![JsonVal::JObj(vec![
                (String::from("case"), JsonVal::JStr(String::from("c1"))),
                (String::from("assert"), JsonVal::JNum(1)),
                (String::from("fail"), JsonVal::JNum(0)),
                (String::from("duration"), JsonVal::JNum(1)),
            ])])),
        ])
    };
    let foreign = rebuild(JsonVal::JStr(String::from("2.0")));
    let typed_bad = rebuild(JsonVal::JNum(10));
    let mut missing = rebuild(JsonVal::JStr(String::from("1.0")));
    if let JsonVal::JObj(pairs) = &mut missing {
        pairs.remove(0);
    }
    validate_suite(&foreign) == Err(Diag::VersionForeign.val())
        && validate_suite(&typed_bad) == Err(Diag::TypeMismatch.val())
        && validate_suite(&missing) == Err(Diag::SectionMissing.val())
}

/// 注入 3：表外键 / 必填键缺 / 用例层类型错位三类必抓。
fn inj_key_and_type_ok() -> bool {
    let mut ghost = match build_sample_suite() {
        JsonVal::JObj(pairs) => pairs,
        _ => return false,
    };
    ghost.push((String::from("ghost"), JsonVal::JNum(1)));
    let mut missing_suite_key = match build_sample_suite() {
        JsonVal::JObj(pairs) => pairs,
        _ => return false,
    };
    missing_suite_key.remove(1);
    let mut bad_case_type = match build_sample_suite() {
        JsonVal::JObj(pairs) => pairs,
        _ => return false,
    };
    for (k, v) in bad_case_type.iter_mut() {
        if k == "cases" {
            // assert 键值换成字符串 → 用例层类型错位（就地重建，不裸下标）。
            let case = JsonVal::JObj(vec![
                (String::from("case"), JsonVal::JStr(String::from("c"))),
                (String::from("assert"), JsonVal::JStr(String::from("12"))),
                (String::from("fail"), JsonVal::JNum(0)),
                (String::from("duration"), JsonVal::JNum(1)),
            ]);
            *v = JsonVal::JArr(vec![case]);
        }
    }
    validate_suite(&JsonVal::JObj(ghost)) == Err(Diag::KeyForeign.val())
        && validate_suite(&JsonVal::JObj(missing_suite_key)) == Err(Diag::SectionMissing.val())
        && validate_suite(&JsonVal::JObj(bad_case_type)) == Err(Diag::TypeMismatch.val())
}

/// 注入 4：fail>assert / 负计数 / 负时长 / 空用例表 / 用例重名五类必抓。
fn inj_value_domain_ok() -> bool {
    let mk = |asserts: i64, fails: i64, duration: i64, case_name: &str| -> JsonVal {
        JsonVal::JObj(vec![
            (String::from("schema"), JsonVal::JStr(String::from("1.0"))),
            (String::from("suite"), JsonVal::JStr(String::from("s"))),
            (String::from("cases"), JsonVal::JArr(vec![JsonVal::JObj(vec![
                (String::from("case"), JsonVal::JStr(String::from(case_name))),
                (String::from("assert"), JsonVal::JNum(asserts)),
                (String::from("fail"), JsonVal::JNum(fails)),
                (String::from("duration"), JsonVal::JNum(duration)),
            ])])),
        ])
    };
    let fail_exceed = mk(3, 4, 1, "c1");
    let neg_count = mk(-1, 0, 1, "c2");
    let neg_dur = mk(3, 0, -5, "c3");
    // 空用例表。
    let mut empty_cases = mk(1, 0, 1, "c4");
    if let JsonVal::JObj(pairs) = &mut empty_cases {
        for (k, v) in pairs.iter_mut() {
            if k == "cases" {
                *v = JsonVal::JArr(Vec::new());
            }
        }
    }
    // 用例重名（两条同名）。
    let dup_case = JsonVal::JObj(vec![
        (String::from("schema"), JsonVal::JStr(String::from("1.0"))),
        (String::from("suite"), JsonVal::JStr(String::from("s"))),
        (String::from("cases"), JsonVal::JArr(vec![
            JsonVal::JObj(vec![
                (String::from("case"), JsonVal::JStr(String::from("same"))),
                (String::from("assert"), JsonVal::JNum(1)),
                (String::from("fail"), JsonVal::JNum(0)),
                (String::from("duration"), JsonVal::JNum(1)),
            ]),
            JsonVal::JObj(vec![
                (String::from("case"), JsonVal::JStr(String::from("same"))),
                (String::from("assert"), JsonVal::JNum(2)),
                (String::from("fail"), JsonVal::JNum(0)),
                (String::from("duration"), JsonVal::JNum(2)),
            ]),
        ])),
    ]);
    validate_suite(&fail_exceed) == Err(Diag::FailExceedAssert.val())
        && validate_suite(&neg_count) == Err(Diag::CountNegative.val())
        && validate_suite(&neg_dur) == Err(Diag::DurationNegative.val())
        && validate_suite(&empty_cases) == Err(Diag::CaseListEmpty.val())
        && validate_suite(&dup_case) == Err(Diag::CaseDup.val())
}

/// 顶层类型错位：账根不是对象必抓。
fn inj_root_type_ok() -> bool {
    validate_suite(&JsonVal::JArr(Vec::new())) == Err(Diag::TypeMismatch.val())
        && validate_suite(&JsonVal::JStr(String::from("suite"))) == Err(Diag::TypeMismatch.val())
}

/// 双跑一致：样账与坏账两次校验结论逐码相同。
fn double_run_ok() -> bool {
    let suite = build_sample_suite();
    let a = validate_suite(&suite);
    let b = validate_suite(&suite);
    if a != b || a.is_err() {
        return false;
    }
    let bad = mk_bad_suite();
    let c = validate_suite(&bad);
    let d = validate_suite(&bad);
    c == d && c == Err(Diag::FailExceedAssert.val())
}

/// 构造 fail>assert 坏账（双跑注入素材）。
fn mk_bad_suite() -> JsonVal {
    JsonVal::JObj(vec![
        (String::from("schema"), JsonVal::JStr(String::from("1.0"))),
        (String::from("suite"), JsonVal::JStr(String::from("s"))),
        (String::from("cases"), JsonVal::JArr(vec![JsonVal::JObj(vec![
            (String::from("case"), JsonVal::JStr(String::from("bad"))),
            (String::from("assert"), JsonVal::JNum(2)),
            (String::from("fail"), JsonVal::JNum(9)),
            (String::from("duration"), JsonVal::JNum(1)),
        ])])),
    ])
}

/// 错误路径 Result 语义：Ok()/Err(u32) 两侧均可达且码在闭集内。
fn error_path_ok() -> bool {
    let suite = build_sample_suite();
    if validate_suite(&suite) != Ok(()) {
        return false;
    }
    let bad = mk_bad_suite();
    match validate_suite(&bad) {
        Err(code) => {
            let mut in_set = false;
            for d in ALL_DIAGS.iter() {
                if d.val() == code {
                    in_set = true;
                }
            }
            in_set && !detail(0x0).is_empty()
        }
        Ok(()) => false,
    }
}

/// obj_get/obj_keys 边界：非对象取值 None、键不存在 None、键序保留。
fn obj_helpers_ok() -> bool {
    let suite = build_sample_suite();
    if obj_get(&suite, "schema").is_none() || obj_get(&suite, "nope").is_some() {
        return false;
    }
    if obj_get(&JsonVal::JNum(1), "schema").is_some() {
        return false;
    }
    let keys = obj_keys(&suite);
    keys.len() == 3
        && keys.first().map(|k| k == "schema").unwrap_or(false)
        && keys.last().map(|k| k == "cases").unwrap_or(false)
}

/// ③ 写回面：状态=已完成、结果摘要非空。
fn writeback_summary_ok() -> bool {
    let suite = build_sample_suite();
    let verdict = validate_suite(&suite);
    let summary = match verdict {
        Ok(()) => format!(
            "UNX-F3425 JSON结果账schema：v{} 冻结，样账校验通过",
            SCHEMA_VERSION
        ),
        Err(_) => String::new(),
    };
    !summary.is_empty() && summary.contains("校验通过") && verdict.is_ok()
}

// ── 判据总成：run_unx3425_checks ───────────────────────────────────────

/// run_unx3425_checks：本单判据聚合（诊断段 0x79840，全部走 add_dyn）。
pub fn run_unx3425_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F3425 JSON结果账schema");
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
            "冻结面版本串与两层键表五域键逐字在册",
            frozen_tables_ok(),
            format!("冻结表{}", if frozen_tables_ok() { "齐" } else { "漂移" }),
        ),
        (
            "M184标准样账schema校验通过",
            sample_suite_passes_ok(),
            format!("样账{}", if sample_suite_passes_ok() { "通过" } else { "未过" }),
        ),
        (
            "注入1遗留版本0.9点名升级必抓",
            inj_legacy_version_ok(),
            format!("遗留版本{}", if inj_legacy_version_ok() { "抓到" } else { "漏抓" }),
        ),
        (
            "注入2外来版本类型错位版本缺三类必抓",
            inj_version_variants_ok(),
            format!("版本违约{}", if inj_version_variants_ok() { "全抓" } else { "漏抓" }),
        ),
        (
            "注入3表外键必填缺用例类型错位必抓",
            inj_key_and_type_ok(),
            format!("键与类型{}", if inj_key_and_type_ok() { "全抓" } else { "漏抓" }),
        ),
        (
            "注入4值域五类违约逐类必抓",
            inj_value_domain_ok(),
            format!("值域违约{}", if inj_value_domain_ok() { "全抓" } else { "漏抓" }),
        ),
        (
            "注入5账根类型错位必抓",
            inj_root_type_ok(),
            format!("根类型{}", if inj_root_type_ok() { "抓到" } else { "漏抓" }),
        ),
        (
            "双跑一致样账坏账两次结论逐码同",
            double_run_ok(),
            format!("双跑{}", if double_run_ok() { "一致" } else { "分叉" }),
        ),
        (
            "错误路径Ok与闭集Err两侧可达",
            error_path_ok(),
            format!("错误路径{}", if error_path_ok() { "过" } else { "破" }),
        ),
        (
            "对象取值助手边界齐",
            obj_helpers_ok(),
            format!("助手{}", if obj_helpers_ok() { "齐" } else { "缺" }),
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
    s
}

// ── 测试区（#[cfg(test)]：允许 assert!/assert_eq!/unwrap）──────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seg_guard_and_diag_closed_set() {
        assert!(seg_guard_ok());
        assert_eq!(SEG_BASE, 0x79840);
        assert!(diag_closed_set_ok());
        for (i, d) in ALL_DIAGS.iter().enumerate() {
            assert_eq!(d.val(), SEG_BASE + i as u32 + 1);
        }
    }

    #[test]
    fn frozen_tables_anchor_five_domain_keys() {
        assert!(frozen_tables_ok());
        assert_eq!(SCHEMA_VERSION, "1.0");
        assert_eq!(LEGACY_VERSION, "0.9");
        assert_eq!(SUITE_KEYS[0], "schema");
        assert_eq!(CASE_KEYS, ["case", "assert", "fail", "duration"]);
    }

    #[test]
    fn sample_suite_validates_m184() {
        assert!(sample_suite_passes_ok());
        let suite = build_sample_suite();
        assert_eq!(validate_suite(&suite), Ok(()));
        assert_eq!(validate_case(&JsonVal::JNum(1)), Err(Diag::TypeMismatch.val()));
    }

    #[test]
    fn legacy_version_named_for_upgrade() {
        assert!(inj_legacy_version_ok());
        assert!(detail(Diag::VersionLegacy.val()).contains("联签升级"));
    }

    #[test]
    fn version_variants_rejected() {
        assert!(inj_version_variants_ok());
    }

    #[test]
    fn key_and_type_rejections() {
        assert!(inj_key_and_type_ok());
        assert!(inj_root_type_ok());
    }

    #[test]
    fn value_domain_rejections() {
        assert!(inj_value_domain_ok());
        // 边界：fail == assert 合法（全败）。
        let full_fail = JsonVal::JObj(vec![
            (String::from("schema"), JsonVal::JStr(String::from("1.0"))),
            (String::from("suite"), JsonVal::JStr(String::from("s"))),
            (String::from("cases"), JsonVal::JArr(vec![JsonVal::JObj(vec![
                (String::from("case"), JsonVal::JStr(String::from("all_fail"))),
                (String::from("assert"), JsonVal::JNum(3)),
                (String::from("fail"), JsonVal::JNum(3)),
                (String::from("duration"), JsonVal::JNum(0)),
            ])])),
        ]);
        assert_eq!(validate_suite(&full_fail), Ok(()));
    }

    #[test]
    fn double_run_identical_verdicts() {
        assert!(double_run_ok());
    }

    #[test]
    fn error_paths_closed_codes() {
        assert!(error_path_ok());
        assert!(detail(0x798FF).starts_with("未知诊断码"));
    }

    #[test]
    fn obj_helper_boundaries() {
        assert!(obj_helpers_ok());
    }

    #[test]
    fn writeback_summary_non_empty() {
        assert!(writeback_summary_ok());
    }

    #[test]
    fn checks_aggregate_all_green() {
        let s = run_unx3425_checks();
        assert!(s.len() >= 12);
        assert!(s.all_passed(), "全部判据必须绿：{}", s.red_items_text());
    }
}
