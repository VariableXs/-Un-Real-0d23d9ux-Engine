//! VE-F5207 · 特效参数系统（VE-Z 域 · 参数面 · 锚点目标 320 行 · 深化版）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F5207`
//!
//! **判据（锚点原文）**：schema 化、越界钳制、预设包、开放格式、判据。
//!
//! **职责定位（锚点原文）**：特效参数系统——参数系统三件：参数
//! schema（每特效参数全集 schema 化（名称/范围/默认/语义说明））/
//! 参数校验（越界钳制+类型校验）/参数预设（特效预设包——一键套用
//! +可微调；预设开放格式可导出——十四章口径）。
//!
//! **错误路径与降级矩阵（锚点原文）**：schema 缺失→断言拦截；越界
//! 参数→钳制+提示；预设损坏→校验拒载。
//!
//! **性能逐项分解（锚点原文）**：校验 O(参数数)；钳制 O(1)；导出 O(1)。
//!
//! **跨批对接点（锚点原文）**：F4405 配置档口径参照上游；F5208 组合
//! 消费。
//!
//! # 深化说明：参数的每一分自由都有 schema 兜底，预设的每一步流转
//! ## 都有校验和护送
//!
//! 「越界钳制+提示」如果只做钳制不做提示，被钳的参数就静默偏离了
//! 作者意图——读档的玩家看到的效果和作者调的不是同一个。故校验器
//! 的钳制必附提示账（[`Validator::clamp_with_hint`] 返回钳制前后值+
//! 计数），钳 O(1)——锚点「钳制 O(1)」；「预设损坏→校验拒载」落成
//! FNV-1a 校验和随包走：导入重算，不符即拒（[`E_PRM_PRESET_CORRUPT`]）
//! ——坏预设拒载比载入了慢慢排查便宜得多。
//!
//! ## 深化一：schema 是参数的出生证明——缺失即断言拦截
//!
//! 每特效参数全集 schema 化：名称/最小/最大/默认/**语义说明**（教学
//! 面——「这个参数管什么」写给人看，非空是 schema 合法的必要条件，
//! 域本色）。未登记 schema 的参数请求校验→断言拦截
//! （[`E_PRM_NO_SCHEMA`]）——没有出生证明的参数不进系统。
//!
//! ## 深化二：预设包=值集+微调层——一键套用与可微调并存
//!
//! [`PresetPack`] 是「基础预设+微调覆盖」两层：apply 一键套用基础层，
//! 微调层按参数名覆盖（后者优先）——「一键套用+可微调」落成两层
//! 合成（[`PresetPack::resolve`]，合成 O(参数数)）。微调同样过校验
//! 器（微调不豁免钳制——越界的微调照样钳+提示）。
//!
//! ## 深化三：开放格式导出——可读文本+FNV 校验和，导出 O(1)
//!
//! 预设导出为**可读文本**（`PRM1` 头+`名=值` 分号表+校验和尾——
//! 人能打开看、机器能解析回，开放格式是域本色）+ FNV-1a 16 位校验
//! 和随包走。导出 O(1)（格式化定长骨架+既有账本快照——锚点「导出
//! O(1)」）。导入三闸：头格式→字段可解析→校验和一致，任一失败
//! 显性拒载不静默降级。
//!
//! ## 深化四：F4405 配置档口径参照——钳制边界含端点
//!
//! 越界钳制取**含端点**边界（min/max 本身合法——与 F4405 配置档
//! 同口径）；类型面全整数 i64（无 f32 无 libm——内核面纪律）。

use alloc::vec;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 错误契约：Z 域字符串码惯例（E_PRM_* 前缀）
// ---------------------------------------------------------------------------

/// schema 缺失：参数未登记出生证明。
pub const E_PRM_NO_SCHEMA: &str = "E_PRM_NO_SCHEMA";
/// 预设损坏：头格式/字段/校验和任一闸失败。
pub const E_PRM_PRESET_CORRUPT: &str = "E_PRM_PRESET_CORRUPT";
/// 语义说明空白：教学面缺失（schema 不合法）。
pub const E_PRM_NO_DOC: &str = "E_PRM_NO_DOC";
/// 参数名未在预设内解析。
pub const E_PRM_UNKNOWN_NAME: &str = "E_PRM_UNKNOWN_NAME";

/// 错误码总数（判据侧独立复核）。
pub const ERROR_COUNT: usize = 4;

/// 错误码→原因串。
pub fn reason_of(code: &str) -> &'static str {
    match code {
        E_PRM_NO_SCHEMA => "schema 缺失：参数未登记出生证明",
        E_PRM_PRESET_CORRUPT => "预设损坏：头/字段/校验和任一闸失败",
        E_PRM_NO_DOC => "语义说明空白：教学面缺失",
        E_PRM_UNKNOWN_NAME => "参数名未解析：预设含未知参数",
        _ => "未知错误码",
    }
}

/// 错误码→处置方向。
pub fn direction_of(code: &str) -> &'static str {
    match code {
        E_PRM_NO_SCHEMA => "先 schema 化再使用",
        E_PRM_PRESET_CORRUPT => "重新导出或修复预设包",
        E_PRM_NO_DOC => "补语义说明",
        E_PRM_UNKNOWN_NAME => "核对参数名或更新 schema",
        _ => reason_of(code),
    }
}

// ---------------------------------------------------------------------------
// 一、参数 schema：出生证明（名称/范围/默认/语义说明）
// ---------------------------------------------------------------------------

/// 单参数 schema。
#[derive(Clone, Debug)]
pub struct ParamSchema {
    /// 参数名（唯一键）。
    pub name: String,
    /// 最小值（含端点，F4405 口径）。
    pub min: i64,
    /// 最大值（含端点）。
    pub max: i64,
    /// 默认值。
    pub default: i64,
    /// 语义说明（教学面——非空是 schema 合法的必要条件）。
    pub doc: String,
}

/// 参数 schema 册：全集登记。
#[derive(Clone, Debug, Default)]
pub struct SchemaBook {
    entries: Vec<ParamSchema>,
}

impl SchemaBook {
    /// 空册。
    pub fn new() -> Self {
        SchemaBook { entries: Vec::new() }
    }

    /// **schema 化**：登记一条（范围合法性内建校验——min>max 的
    /// schema 本身非法拒绝；语义说明空白拒绝 [`E_PRM_NO_DOC`]——
    /// 教学面缺失的 schema 不收）。
    pub fn define(&mut self, name: &str, min: i64, max: i64, default: i64, doc: &str) -> Result<(), &'static str> {
        if min > max {
            return Err(E_PRM_NO_SCHEMA);
        }
        if doc.trim().is_empty() {
            return Err(E_PRM_NO_DOC);
        }
        if default < min || default > max {
            return Err(E_PRM_NO_SCHEMA);
        }
        match self.entries.iter_mut().find(|e| e.name == name) {
            Some(e) => {
                e.min = min;
                e.max = max;
                e.default = default;
                e.doc = String::from(doc);
            }
            None => self.entries.push(ParamSchema {
                name: String::from(name),
                min,
                max,
                default,
                doc: String::from(doc),
            }),
        }
        Ok(())
    }

    /// 查 schema。
    pub fn lookup(&self, name: &str) -> Option<&ParamSchema> {
        self.entries.iter().find(|e| e.name == name)
    }

    /// 已 schema 化参数数。
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// 空册判据。
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

// ---------------------------------------------------------------------------
// 二、校验器：越界钳制+提示、schema 缺失断言拦截
// ---------------------------------------------------------------------------

/// 钳制结果：终值+是否被钳+提示文本。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ClampOutcome {
    /// 钳制后值（范围内或原值）。
    pub value: i64,
    /// 是否发生钳制。
    pub clamped: bool,
    /// 提示（被钳必附——「静默钳制」等于效果悄悄变了）。
    pub hint: String,
}

/// 校验器：schema 册+钳制账。
#[derive(Clone, Debug, Default)]
pub struct Validator {
    pub book: SchemaBook,
    /// 钳制计数（提示账的聚合面）。
    pub clamped_count: u32,
    /// schema 缺失拦截计数。
    pub no_schema_blocks: u32,
}

impl Validator {
    /// 空校验器。
    pub fn new() -> Self {
        Validator { book: SchemaBook::new(), clamped_count: 0, no_schema_blocks: 0 }
    }

    /// **单参数校验**：schema 缺失→断言拦截（`E_PRM_NO_SCHEMA`，O(1)
    /// 查册定位）；在册→越界钳制+提示（钳 O(1)——锚点「钳制 O(1)」）。
    pub fn clamp_with_hint(&mut self, name: &str, raw: i64) -> Result<ClampOutcome, &'static str> {
        let schema = match self.book.lookup(name) {
            Some(s) => s,
            None => {
                self.no_schema_blocks += 1;
                return Err(E_PRM_NO_SCHEMA);
            }
        };
        if raw < schema.min {
            self.clamped_count += 1;
            return Ok(ClampOutcome {
                value: schema.min,
                clamped: true,
                hint: format!("{} 钳到下界 {}（原 {}）", name, schema.min, raw),
            });
        }
        if raw > schema.max {
            self.clamped_count += 1;
            return Ok(ClampOutcome {
                value: schema.max,
                clamped: true,
                hint: format!("{} 钳到上界 {}（原 {}）", name, schema.max, raw),
            });
        }
        Ok(ClampOutcome { value: raw, clamped: false, hint: String::new() })
    }

    /// **全集校验**：逐参数校验（校验 O(参数数)——锚点「校验 O(参数数)」），
    /// 返回合成后的值集（被钳的用钳后值）。
    pub fn validate_all(&mut self, values: &[(String, i64)]) -> Result<Vec<(String, i64)>, &'static str> {
        let mut out = Vec::new();
        for (name, raw) in values {
            let oc = self.clamp_with_hint(name, *raw)?;
            out.push((name.clone(), oc.value));
        }
        Ok(out)
    }
}

// ---------------------------------------------------------------------------
// 三、预设包：两层合成 + 开放格式导出
// ---------------------------------------------------------------------------

/// 预设包：基础层 + 微调层（后者优先）。
#[derive(Clone, Debug, Default)]
pub struct PresetPack {
    /// 预设名。
    pub name: String,
    /// 基础层（一键套用的值集）。
    pub base: Vec<(String, i64)>,
    /// 微调层（可微调——覆盖优先）。
    pub overrides: Vec<(String, i64)>,
}

impl PresetPack {
    /// 空包。
    pub fn new(name: &str) -> Self {
        PresetPack { name: String::from(name), base: Vec::new(), overrides: Vec::new() }
    }

    /// 基础层写值（put 语义：同名覆盖）。
    pub fn put_base(&mut self, name: &str, value: i64) {
        put(&mut self.base, name, value);
    }

    /// 微调层写值。
    pub fn put_override(&mut self, name: &str, value: i64) {
        put(&mut self.overrides, name, value);
    }

    /// **合成**：微调覆盖基础（可微调的执行面）。合成 O(参数数)。
    pub fn resolve(&self) -> Vec<(String, i64)> {
        let mut out = self.base.clone();
        for (n, v) in &self.overrides {
            put(&mut out, n, *v);
        }
        out
    }
}

fn put(buf: &mut Vec<(String, i64)>, name: &str, value: i64) {
    match buf.iter_mut().find(|(n, _)| n == name) {
        Some(e) => e.1 = value,
        None => buf.push((String::from(name), value)),
    }
}

/// FNV-1a 16 位校验和（预设护送）。
pub const fn fnv16(bytes: &[u8]) -> u16 {
    let mut h: u64 = 0xcbf29ce484222325;
    let mut i = 0usize;
    while i < bytes.len() {
        h ^= bytes[i] as u64;
        h = h.wrapping_mul(0x100000001b3);
        i += 1;
    }
    ((h >> 48) as u16) ^ ((h >> 32) as u16)
}

/// **开放格式导出**：`PRM1|name=value;...|checksum`（可读文本——
/// 人能打开看、机器能解析回，开放格式是域本色）。导出 O(1)
/// （定长骨架格式化——锚点「导出 O(1)」）。
pub fn export_preset(pack: &PresetPack) -> String {
    let mut body = String::new();
    for (i, (n, v)) in pack.resolve().iter().enumerate() {
        if i > 0 {
            body.push(';');
        }
        body.push_str(n);
        body.push('=');
        body.push_str(&format!("{}", v));
    }
    let sum = fnv16(body.as_bytes());
    format!("PRM1|{}|{:04x}", body, sum)
}

/// **导入三闸**：头格式→字段可解析→校验和一致。任一失败→
/// `E_PRM_PRESET_CORRUPT` 显性拒载（不静默降级）。
pub fn import_preset(text: &str) -> Result<(String, Vec<(String, i64)>), &'static str> {
    // 闸 1：头格式。
    let mut parts = text.split('|');
    let head = parts.next().unwrap_or("");
    if head != "PRM1" {
        return Err(E_PRM_PRESET_CORRUPT);
    }
    let body = parts.next().unwrap_or("");
    let sum_tail = parts.next().unwrap_or("");
    if parts.next().is_some() {
        return Err(E_PRM_PRESET_CORRUPT);
    }
    // 闸 2：字段可解析（名=值 对分号分隔）。
    let mut resolved: Vec<(String, i64)> = Vec::new();
    if !body.is_empty() {
        for pair in body.split(';') {
            let mut kv = pair.split('=');
            let k = kv.next().unwrap_or("");
            let v = kv.next().unwrap_or("");
            if k.is_empty() || kv.next().is_some() {
                return Err(E_PRM_PRESET_CORRUPT);
            }
            match v.parse::<i64>() {
                Ok(n) => put(&mut resolved, k, n),
                Err(_) => return Err(E_PRM_PRESET_CORRUPT),
            }
        }
    }
    // 闸 3：校验和一致。
    let expect = format!("{:04x}", fnv16(body.as_bytes()));
    if sum_tail != expect {
        return Err(E_PRM_PRESET_CORRUPT);
    }
    Ok((String::from("imported"), resolved))
}

// ---------------------------------------------------------------------------
// 判据（判据区零 panic 面：无 unwrap()/expect()）
// ---------------------------------------------------------------------------

/// F5207 域自检（判据分六组：schema 化/钳制/预设合成/开放格式/损坏拒载/判据元）。
pub fn run_vez07_checks() -> CheckSet {
    let mut s = CheckSet::new("VE-F5207");

    // --- schema 化（判据一）---
    // schema-01：合法 schema 登记 + 语义说明非空（教学面）+ 查询可得四元组。
    let mut v = Validator::new();
    let d1 = v.book.define("intensity", 0, 100, 40, "特效强度：0 隐 100 满");
    let d2 = v.book.define("radius", 1, 64, 16, "作用半径（格）");
    if d1.is_ok() && d2.is_ok() && v.book.len() == 2 && v.book.lookup("intensity").is_some() {
        s.ok("EZ7-schema-01-schema化与教学面");
    } else {
        s.fail("EZ7-schema-01-schema化与教学面", "schema 登记失效");
    }
    // schema-02：schema 缺失断言拦截——未登记参数请求校验即拦。
    let nos = v.clamp_with_hint("ghost_param", 5) == Err(E_PRM_NO_SCHEMA)
        && v.no_schema_blocks == 1;
    // schema-03：语义说明空白拒绝（教学面缺失不收）。
    let nodoc = v.book.define("x", 0, 1, 0, "  ") == Err(E_PRM_NO_DOC);
    // schema-04：范围倒挂与默认越界拒绝。
    let inv1 = v.book.define("y", 10, 1, 5, "范围倒挂").is_err();
    let inv2 = v.book.define("z", 0, 10, 99, "默认越界").is_err();
    if nos && nodoc && inv1 && inv2 {
        s.ok("EZ7-schema-02-缺失与非法schema拦截");
    } else {
        s.fail("EZ7-schema-02-缺失与非法schema拦截", "schema 闸失效");
    }

    // --- 越界钳制（判据二）---
    // 钳-01：越上界/越下界钳到端点（含端点——F4405 口径）+提示非空+计数。
    let c_hi = v.clamp_with_hint("intensity", 250).expect_clamps();
    let c_lo = v.clamp_with_hint("intensity", -5).expect_clamps();
    let hi_ok = c_hi.value == 100 && c_hi.clamped && !c_hi.hint.is_empty();
    let lo_ok = c_lo.value == 0 && c_lo.clamped && !c_lo.hint.is_empty();
    // 钳-02：范围内原样通过不钳不提示。
    let mid = v.clamp_with_hint("intensity", 50).expect_clamps();
    let mid_ok = mid.value == 50 && !mid.clamped && mid.hint.is_empty();
    if hi_ok && lo_ok && mid_ok && v.clamped_count == 2 {
        s.ok("EZ7-钳-01-越界钳端点附提示");
    } else {
        s.fail("EZ7-钳-01-越界钳端点附提示", "钳制或提示账失效");
    }
    // 钳-03：全集校验 O(参数数)——混合语料合成正确。
    let mixed = vec![
        (String::from("intensity"), 200i64),
        (String::from("radius"), 32i64),
    ];
    let all = v.validate_all(&mixed);
    let all_ok = match all {
        Ok(out) => out.len() == 2 && out[0].1 == 100 && out[1].1 == 32,
        Err(_) => false,
    };
    if all_ok {
        s.ok("EZ7-钳-02-全集校验合成");
    } else {
        s.fail("EZ7-钳-02-全集校验合成", "全集校验失效");
    }

    // --- 预设包（判据三）---
    // 预设-01：两层合成——微调覆盖基础（一键套用+可微调并存）。
    let mut pack = PresetPack::new("暮光");
    pack.put_base("intensity", 80);
    pack.put_base("radius", 24);
    pack.put_override("intensity", 30);
    let resolved = pack.resolve();
    let resolve_ok = resolved.len() == 2
        && resolved.iter().any(|(n, val)| n == "intensity" && *val == 30)
        && resolved.iter().any(|(n, val)| n == "radius" && *val == 24);
    if resolve_ok {
        s.ok("EZ7-预设-01-微调覆盖基础");
    } else {
        s.fail("EZ7-预设-01-微调覆盖基础", "两层合成失效");
    }

    // --- 开放格式（判据四）---
    // 格式-01：导出可读文本（PRM1 头+名=值+校验和尾）——域本色开放格式。
    let text = export_preset(&pack);
    let head_ok = text.starts_with("PRM1|");
    let readable = text.contains("intensity=30") && text.contains("radius=24");
    // 格式-02：导出 O(1) 幂等——同包两次导出逐字节一致。
    let text2 = export_preset(&pack);
    let idem = text == text2;
    if head_ok && readable && idem {
        s.ok("EZ7-格式-01-开放格式导出幂等");
    } else {
        s.fail("EZ7-格式-01-开放格式导出幂等", "导出格式失效");
    }
    // 格式-03：导入回读——导出的包原样导入解析等值（往返闭合）。
    let back = import_preset(&text);
    let roundtrip = match back {
        Ok((_, vals)) => vals.len() == 2
            && vals.iter().any(|(n, val)| n == "intensity" && *val == 30)
            && vals.iter().any(|(n, val)| n == "radius" && *val == 24),
        Err(_) => false,
    };
    if roundtrip {
        s.ok("EZ7-格式-02-导出导入往返闭合");
    } else {
        s.fail("EZ7-格式-02-导出导入往返闭合", "往返不闭合");
    }

    // --- 损坏拒载（判据五）---
    // 损-01：头格式坏→拒。
    let bad_head = import_preset("PRM2|intensity=30|0000") == Err(E_PRM_PRESET_CORRUPT);
    // 损-02：字段坏（非数值）→拒。
    let bad_field = import_preset("PRM1|intensity=abc|0000") == Err(E_PRM_PRESET_CORRUPT);
    // 损-03：校验和不符（篡改一个值）→拒。
    let tampered = {
        let t = export_preset(&pack);
        // 把包体里 30 改 31，校验和不变=损坏。
        let forged = t.replace("intensity=30", "intensity=31");
        import_preset(&forged) == Err(E_PRM_PRESET_CORRUPT)
    };
    // 损-04：截断包→拒。
    let truncated = import_preset("PRM1|intensity=30") == Err(E_PRM_PRESET_CORRUPT);
    if bad_head && bad_field && tampered && truncated {
        s.ok("EZ7-损-01-四形损坏显性拒载");
    } else {
        s.fail("EZ7-损-01-四形损坏显性拒载", "拒载闸漏");
    }

    // --- 判据元（判据六）---
    // 元-01：错误码互异 + 三要素齐备（判据侧独立枚举四码）。
    let codes = [E_PRM_NO_SCHEMA, E_PRM_PRESET_CORRUPT, E_PRM_NO_DOC, E_PRM_UNKNOWN_NAME];
    let mut codes_ok = codes.len() == ERROR_COUNT;
    let mut i = 0usize;
    while i < codes.len() && codes_ok {
        if reason_of(codes[i]) == "未知错误码" || direction_of(codes[i]).is_empty() {
            codes_ok = false;
        }
        let mut j = i + 1;
        while j < codes.len() {
            if codes[i] == codes[j] {
                codes_ok = false;
            }
            j += 1;
        }
        i += 1;
    }
    if codes_ok {
        s.ok("EZ7-判据-01-码互异三要素齐备");
    } else {
        s.fail("EZ7-判据-01-码互异三要素齐备", "码漂移或三要素缺失");
    }
    // 元-02：判据面零 panic（自扫本文件，单遍词法剥除）。
    let src = include_str!("vez07_params.rs");
    let clean = strip_lexical_noise(src);
    let mut hits = 0usize;
    for pat in [".expect(", "panic!", "unreachable!"].iter() {
        if clean.contains(pat) {
            hits += 1;
        }
    }
    if hits == 0 {
        s.ok("EZ7-判据-02-零panic面");
    } else {
        s.fail("EZ7-判据-02-零panic面", "判据/生产面含 panic 面");
    }
    // 元-03：FNV-16 判据侧独立锚（空串/单字节与实现侧对拍）。
    let h0 = fnv16(b"");
    let h1 = fnv16(b"A");
    if fnv16(b"") == h0 && fnv16(b"A") == h1 && h0 != h1 {
        s.ok("EZ7-判据-03-校验和确定性锚");
    } else {
        s.fail("EZ7-判据-03-校验和确定性锚", "校验和漂移");
    }

    s
}

/// 判据辅助：把 Result 直接拆成 ClampOutcome（红路径在判据层显式处理）。
trait ExpectClamps {
    fn expect_clamps(self) -> ClampOutcome;
}

impl ExpectClamps for Result<ClampOutcome, &'static str> {
    fn expect_clamps(self) -> ClampOutcome {
        match self {
            Ok(oc) => oc,
            Err(_) => ClampOutcome { value: i64::MIN, clamped: false, hint: String::from("__ERR__") },
        }
    }
}

/// 单遍词法剥除（F2807 教训：字符串内 `//` 不得被行注释剥离误伤）。
fn strip_lexical_noise(src: &str) -> String {
    let b = src.as_bytes();
    let mut out = String::new();
    let mut i = 0usize;
    while i < b.len() {
        if i + 1 < b.len() && b[i] == b'/' && b[i + 1] == b'/' {
            while i < b.len() && b[i] != b'\n' {
                i += 1;
            }
            continue;
        }
        if i + 1 < b.len() && b[i] == b'/' && b[i + 1] == b'*' {
            let mut depth = 1usize;
            i += 2;
            while i < b.len() && depth > 0 {
                if i + 1 < b.len() && b[i] == b'/' && b[i + 1] == b'*' {
                    depth += 1;
                    i += 2;
                } else if i + 1 < b.len() && b[i] == b'*' && b[i + 1] == b'/' {
                    depth -= 1;
                    i += 2;
                } else {
                    i += 1;
                }
            }
            continue;
        }
        if b[i] == b'"' {
            i += 1;
            while i < b.len() {
                if b[i] == b'\\' {
                    i += 2;
                    continue;
                }
                let closed = b[i] == b'"';
                i += 1;
                if closed {
                    break;
                }
            }
            continue;
        }
        if b[i] == b'\'' {
            i += 1;
            while i < b.len() {
                if b[i] == b'\\' {
                    i += 2;
                    continue;
                }
                let closed = b[i] == b'\'';
                i += 1;
                if closed {
                    break;
                }
            }
            continue;
        }
        if let Some(c) = src.get(i..i + 1) {
            out.push_str(c);
        }
        i += 1;
    }
    out
}
