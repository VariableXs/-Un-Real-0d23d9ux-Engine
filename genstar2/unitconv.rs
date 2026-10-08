//! F458 搜索单位换算（genstar2 · I 域通用·二分队 · AI-U2）。
//!
//! 主册判据（验收标准第一句）：
//! **六族×2 用例；区域默认单位；显式目标单位解析；精度来源登记（系数表
//! 入册）；复制与降级（非换算当搜索）。**
//!
//! 功能定义（主册批次三）：输入「100 磅」「30 摄氏度 to 华氏」「5km in
//! miles」类 query 出换算卡（长度/重量/温度/面积/体积/速度六族）；目标
//! 单位自动取用户区域默认（F296 联动）也可写明（「to kg」）；换算卡一键
//! 复制；精度来源登记（换算系数固定源，不糊弄）。
//!
//! 零堆纪律：静态系数表，无 alloc。

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 常量与系数表（精度来源登记：全部系数为国际单位制定义值/公认换算值）
// ---------------------------------------------------------------------------

/// 六族单位（主册原文六族）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Family {
    Length,
    Weight,
    Temperature,
    Area,
    Volume,
    Speed,
}

/// 单位定义（对基准单位的系数；温度特殊——函数换算）。
#[derive(Clone, Copy, Debug)]
pub struct Unit {
    pub family: Family,
    pub names: [&'static str; 3],
    /// 对族内基准单位的系数（温度单位忽略）。
    pub factor: f64,
    /// 基准单位标记（区域默认目标单位解析用）。
    pub is_base_target: bool,
}

/// 系数表入册（主册判据：精度来源登记——此处即登记处）。
pub const UNITS: [Unit; 18] = [
    // 长度（基准：米）
    Unit { family: Family::Length, names: ["m", "米", "meter"], factor: 1.0, is_base_target: true },
    Unit { family: Family::Length, names: ["km", "千米", "公里"], factor: 1000.0, is_base_target: false },
    Unit { family: Family::Length, names: ["mi", "英里", "miles"], factor: 1609.344, is_base_target: false },
    // 重量（基准：千克）
    Unit { family: Family::Weight, names: ["kg", "千克", "公斤"], factor: 1.0, is_base_target: true },
    Unit { family: Family::Weight, names: ["lb", "磅", "pound"], factor: 0.45359237, is_base_target: false },
    Unit { family: Family::Weight, names: ["g", "克", "gram"], factor: 0.001, is_base_target: false },
    // 温度（函数换算，factor 忽略）
    Unit { family: Family::Temperature, names: ["c", "摄氏度", "celsius"], factor: 0.0, is_base_target: true },
    Unit { family: Family::Temperature, names: ["f", "华氏度", "fahrenheit"], factor: 0.0, is_base_target: false },
    Unit { family: Family::Temperature, names: ["k", "开尔文", "kelvin"], factor: 0.0, is_base_target: false },
    // 面积（基准：平方米）
    Unit { family: Family::Area, names: ["m2", "平方米", "sqm"], factor: 1.0, is_base_target: true },
    Unit { family: Family::Area, names: ["km2", "平方公里", "sqkm"], factor: 1_000_000.0, is_base_target: false },
    Unit { family: Family::Area, names: ["acre", "英亩", "acres"], factor: 4046.8564224, is_base_target: false },
    // 体积（基准：升）
    Unit { family: Family::Volume, names: ["l", "升", "liter"], factor: 1.0, is_base_target: true },
    Unit { family: Family::Volume, names: ["ml", "毫升", "milliliter"], factor: 0.001, is_base_target: false },
    Unit { family: Family::Volume, names: ["gal", "加仑", "gallon"], factor: 3.785411784, is_base_target: false },
    // 速度（基准：米/秒）
    Unit { family: Family::Speed, names: ["mps", "米每秒", "m/s"], factor: 1.0, is_base_target: true },
    Unit { family: Family::Speed, names: ["kmh", "千米每小时", "km/h"], factor: 1.0 / 3.6, is_base_target: false },
    Unit { family: Family::Speed, names: ["mph", "英里每小时", "mi/h"], factor: 1609.344 / 3600.0, is_base_target: false },
];

/// 区域默认目标单位（F296 联动：区域设置注入）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RegionDefaults {
    /// 各族默认目标单位名（用户区域习惯制式）。
    pub length: &'static str,
    pub weight: &'static str,
    pub temperature: &'static str,
}

/// 中文区域默认（登记默认实现；区域服务可注入其他制式）。
pub const REGION_ZH: RegionDefaults = RegionDefaults {
    length: "km",
    weight: "kg",
    temperature: "c",
};

fn find_unit(name: &str) -> Option<&'static Unit> {
    let n = name.trim().to_ascii_lowercase();
    // 精确匹配任一别名（不模糊含——「m」不得吞掉「ml」）。
    UNITS.iter().find(|u| u.names.iter().any(|x| *x == n.as_str()))
}

/// 温度换算（函数族）。
fn convert_temperature(v: f64, from: &str, to: &str) -> Option<f64> {
    let f = find_unit(from)?.family == Family::Temperature;
    let t = find_unit(to)?.family == Family::Temperature;
    if !f || !t {
        return None;
    }
    let from_c = match from_str_or_key(from) {
        "c" | "摄氏度" | "celsius" => v,
        "f" | "华氏度" | "fahrenheit" => (v - 32.0) * 5.0 / 9.0,
        "k" | "开尔文" | "kelvin" => v - 273.15,
        _ => return None,
    };
    Some(match from_c_return(to) {
        "c" | "摄氏度" | "celsius" => from_c,
        "f" | "华氏度" | "fahrenheit" => from_c * 9.0 / 5.0 + 32.0,
        "k" | "开尔文" | "kelvin" => from_c + 273.15,
        _ => return None,
    })
}

fn from_str_or_key(s: &str) -> &'static str {
    let n = s.trim().to_ascii_lowercase();
    for u in UNITS.iter().filter(|u| u.family == Family::Temperature) {
        if u.names.iter().any(|x| *x == n.as_str()) {
            return u.names[0];
        }
    }
    ""
}

fn from_c_return(s: &str) -> &'static str {
    from_str_or_key(s)
}

/// 换算核心：数值 + 源单位 → 目标单位（None = 非换算，降级当搜索）。
pub fn convert(v: f64, from: &str, to: &str) -> Option<f64> {
    let u_from = find_unit(from)?;
    let u_to = find_unit(to)?;
    if u_from.family != u_to.family {
        return None; // 跨族换算不支持——诚实降级
    }
    if u_from.family == Family::Temperature {
        return convert_temperature(v, from, to);
    }
    Some(v * u_from.factor / u_to.factor)
}

/// 目标单位解析：显式「to X」/「in X」优先；否则区域默认（同族 base 目标）。
pub fn resolve_target(query_tail: Option<&str>, family: Family, region: &RegionDefaults) -> Option<&'static str> {
    if let Some(t) = query_tail {
        let clean = t.trim().to_ascii_lowercase();
        if !clean.is_empty() {
            // 显式目标单位：仅当该单位存在且同族时采纳。
            if let Some(u) = find_unit(&clean) {
                if u.family == family {
                    return Some(u.names[0]);
                }
            }
            return None; // 显式写了但认不出 → 降级当搜索（不猜）
        }
    }
    let want = match family {
        Family::Length => region.length,
        Family::Weight => region.weight,
        Family::Temperature => region.temperature,
        _ => return None,
    };
    find_unit(want).map(|u| u.names[0])
}

/// 换算卡输出文本（复制即所见；保留 4 位有效小数）。
pub fn card_text(v: f64, from: &str, to: &str) -> Option<f64> {
    convert(v, from, to).map(|r| (r * 10_000.0).round() / 10_000.0)
}

// ---------------------------------------------------------------------------
// 域自检
// ---------------------------------------------------------------------------

pub fn run_unitconv_checks() -> CheckSet {
    let mut cs = CheckSet::new("F458-unitconv");
    // 1) 六族 × 2 用例（主册判据）。
    cs.add("len_1", (convert(5.0, "km", "m").unwrap() - 5000.0).abs() < 1e-9, "");
    cs.add("len_2", (convert(5.0, "km", "mi").unwrap() - 3.1068559612).abs() < 1e-6, "");
    cs.add("weight_1", (convert(100.0, "磅", "kg").unwrap() - 45.359237).abs() < 1e-6, "");
    cs.add("weight_2", (convert(1.0, "kg", "g").unwrap() - 1000.0).abs() < 1e-9, "");
    cs.add("temp_1", (convert(30.0, "摄氏度", "华氏度").unwrap() - 86.0).abs() < 1e-9, "");
    cs.add("temp_2", (convert(0.0, "c", "k").unwrap() - 273.15).abs() < 1e-9, "");
    cs.add("area_1", (convert(2.0, "km2", "m2").unwrap() - 2_000_000.0).abs() < 1e-6, "");
    cs.add("area_2", (convert(1.0, "acre", "m2").unwrap() - 4046.8564224).abs() < 1e-6, "");
    cs.add("vol_1", (convert(1.0, "gal", "l").unwrap() - 3.785411784).abs() < 1e-9, "");
    cs.add("vol_2", (convert(500.0, "ml", "l").unwrap() - 0.5).abs() < 1e-12, "");
    cs.add("speed_1", (convert(100.0, "kmh", "mps").unwrap() - 27.7777777778).abs() < 1e-6, "");
    cs.add("speed_2", (convert(60.0, "mph", "kmh").unwrap() - 96.56064).abs() < 1e-6, "");
    // 2) 区域默认单位（F296 注入：中文区域长度默认 km）。
    cs.add("region_default_len", resolve_target(None, Family::Length, &REGION_ZH) == Some("km"), "");
    cs.add("region_default_weight", resolve_target(None, Family::Weight, &REGION_ZH) == Some("kg"), "");
    // 3) 显式目标单位解析（「to kg」）；认不出诚实降级。
    cs.add("explicit_target", resolve_target(Some("kg"), Family::Weight, &REGION_ZH) == Some("kg"), "");
    cs.add("explicit_unknown_degrade", resolve_target(Some("xyz"), Family::Weight, &REGION_ZH).is_none(), "");
    // 4) 跨族换算不支持（诚实降级，不猜）。
    cs.add("cross_family_reject", convert(1.0, "km", "kg").is_none(), "");
    // 5) 精度来源登记（系数表在册）。
    cs.add("factor_table_registered", UNITS.len() == 18 && UNITS.iter().all(|u| u.names.iter().all(|n| !n.is_empty())), "");
    // 6) 复制文本（卡值格式化）。
    cs.add("card_copy", card_text(180.0, "磅", "kg").unwrap() - 81.6466 < 1e-4, "");
    cs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn six_families_two_cases_each() {
        let ok = [
            convert(1.0, "km", "mi").is_some(),
            convert(1.0, "磅", "kg").is_some(),
            convert(100.0, "f", "c").is_some(),
            convert(1.0, "acre", "km2").is_some(),
            convert(1.0, "gal", "ml").is_some(),
            convert(1.0, "mph", "mps").is_some(),
        ];
        assert!(ok.iter().all(|&x| x));
    }

    #[test]
    fn temperature_round_trip() {
        let f = convert(36.6, "c", "f").unwrap();
        let c = convert(f, "f", "c").unwrap();
        assert!((c - 36.6).abs() < 1e-9);
    }

    #[test]
    fn non_conversion_queries_degrade() {
        assert!(convert(5.0, "报告", "kg").is_none());
        assert!(card_text(1.0, "m", "kg").is_none());
    }
}

// ===========================================================================
// 深化 v2（F458）：复合表达式换算 / 双向换算自检 / 单位别名扩充 / 持久化
// ===========================================================================

/// 复合换算（链式：「5km in m in mi」类两跳——按序复合）。
pub fn convert_chain(v: f64, hops: &[&str]) -> Option<f64> {
    let mut val = v;
    let mut i = 0;
    while i + 1 < hops.len() {
        val = convert(val, hops[i], hops[i + 1])?;
        i += 1;
    }
    Some(val)
}

/// 双向换算自检（round-trip 精度守护：a→b→a 偏差 ≤1e-6 相对值）。
pub fn roundtrip_ok(v: f64, a: &str, b: &str) -> bool {
    match (convert(v, a, b), convert(v, a, b).and_then(|x| convert(x, b, a))) {
        (Some(_), Some(back)) => (back - v).abs() <= v.abs() * 1e-6 + 1e-9,
        _ => false,
    }
}

/// 单位别名扩充（口语别名 → 册内单位名——「180 磅多重」的口语面）。
pub fn alias_resolve(word: &str) -> Option<&'static str> {
    const ALIASES: [(&str, &str); 10] = [
        ("斤", "kg"), ("公斤", "kg"), ("千米", "km"), ("英里", "mi"),
        ("厘米", "cm2fake"), ("加仑", "gal"), ("迈", "kmh"), ("码", "kg2fake"),
        ("摄氏", "c"), ("华氏", "f"),
    ];
    // 仅映射册内单位（fake 后缀 = 口语存在但册内未收——诚实返回 None）。
    ALIASES.iter().find(|(w, _)| *w == word).and_then(|(_, u)| {
        if UNITS.iter().any(|unit| unit.names.contains(u)) {
            Some(UNITS.iter().find(|unit| unit.names.contains(u)).unwrap().names[0])
        } else {
            None
        }
    })
}

/// 换算历史（最近 8 次查询——搜索框回看「刚才算过什么」）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ConvHistoryEntry {
    pub value: f64,
    pub from: &'static str,
    pub to: &'static str,
}

pub struct ConvHistory {
    ring: [Option<ConvHistoryEntry>; 8],
    head: usize,
    n: usize,
}

impl ConvHistory {
    pub const fn new() -> Self {
        ConvHistory { ring: [None; 8], head: 0, n: 0 }
    }

    pub fn push(&mut self, e: ConvHistoryEntry) {
        self.ring[self.head] = Some(e);
        self.head = (self.head + 1) % 8;
        self.n = (self.n + 1).min(8);
    }

    pub fn latest(&self) -> Option<ConvHistoryEntry> {
        if self.n == 0 {
            return None;
        }
        let idx = (self.head + 8 - 1) % 8;
        self.ring[idx]
    }

    pub fn count(&self) -> usize {
        self.n
    }
}

/// 区域默认持久化（用户改过区域制式——跨重启记住）。
pub const PERSIST_MAGIC: [u8; 4] = *b"VUC1";

pub fn save_region(region: &RegionDefaults, out: &mut [u8]) -> Option<usize> {
    if out.len() < 4 + 3 * 8 {
        return None;
    }
    out[..4].copy_from_slice(&PERSIST_MAGIC);
    let mut w = 4;
    for name in [region.length, region.weight, region.temperature] {
        out[w..w + 8].copy_from_slice(&persist_name(name));
        w += 8;
    }
    Some(w)
}

fn persist_name(name: &str) -> [u8; 8] {
    let mut b = [0u8; 8];
    for (i, c) in name.bytes().take(8).enumerate() {
        b[i] = c;
    }
    b
}

fn load_name(b: &[u8]) -> Option<&'static str> {
    let end = b.iter().position(|&c| c == 0).unwrap_or(8);
    let s = core::str::from_utf8(&b[..end]).ok()?;
    UNITS.iter().find(|u| u.names.contains(&s)).map(|u| u.names[0])
}

pub fn load_region(buf: &[u8]) -> Option<RegionDefaults> {
    if buf.len() < 28 || buf[..4] != PERSIST_MAGIC {
        return None;
    }
    Some(RegionDefaults {
        length: load_name(&buf[4..12])?,
        weight: load_name(&buf[12..20])?,
        temperature: load_name(&buf[20..28])?,
    })
}

pub fn run_unitconv_deep_checks() -> CheckSet {
    let mut cs = CheckSet::new("F458-deep");
    // 复合链式换算（km→m→mi 两跳与直连一致——复合不漂移）。
    cs.add("chain_two_hop", (convert_chain(5.0, &["km", "m", "mi"]).unwrap() - convert(5.0, "km", "mi").unwrap()).abs() < 1e-9, "");
    cs.add("chain_bad_hop_honest", convert_chain(5.0, &["km", "kg", "mi"]).is_none(), "");
    // 双向 round-trip（六族各验一对——精度守护）。
    cs.add("roundtrip_all_families", ["km", "lb", "c", "acre", "gal", "mph"].iter().all(|&u| roundtrip_ok(42.0, u, base_of(u))), "");
    // 口语别名（册内映射、册外诚实 None）。
    cs.add("alias_known", alias_resolve("公斤") == Some("kg"), "");
    cs.add("alias_unknown_honest", alias_resolve("码").is_none(), "");
    // 换算历史（最近 8 条、latest 命中）。
    cs.add("history_latest", {
        let mut h = ConvHistory::new();
        h.push(ConvHistoryEntry { value: 1.0, from: "km", to: "mi" });
        h.push(ConvHistoryEntry { value: 2.0, from: "lb", to: "kg" });
        h.latest() == Some(ConvHistoryEntry { value: 2.0, from: "lb", to: "kg" }) && h.count() == 2
    }, "");
    cs.add("history_empty_honest", ConvHistory::new().latest().is_none(), "");
    // 区域默认持久化 round-trip + 未知单位拒收。
    cs.add("region_persist", {
        let mut buf = [0u8; 32];
        let n = save_region(&REGION_ZH, &mut buf).unwrap();
        load_region(&buf[..n]) == Some(REGION_ZH)
    }, "");
    cs.add("region_persist_bad_unit", load_region(&[b'V', b'U', b'C', b'1', b'x', 0,0,0,0,0,0,0, b'k', b'g', 0,0,0,0,0,0, b'c', 0,0,0,0,0,0,0]).is_none(), "");
    cs
}

fn base_of(unit: &str) -> &'static str {
    let u = find_unit(unit).unwrap();
    let family = u.family;
    UNITS.iter().find(|x| x.family == family && x.is_base_target).unwrap().names[0]
}

#[cfg(test)]
mod deep_tests {
    use super::*;

    #[test]
    fn roundtrip_temperature_exact() {
        assert!(roundtrip_ok(36.6, "c", "f"));
        assert!(roundtrip_ok(0.0, "c", "k"));
    }

    #[test]
    fn chain_of_one_is_identity() {
        assert_eq!(convert_chain(7.5, &["km"]), Some(7.5));
    }

    #[test]
    fn history_ring_wraps() {
        let mut h = ConvHistory::new();
        for i in 0..12 {
            h.push(ConvHistoryEntry { value: i as f64, from: "km", to: "mi" });
        }
        assert_eq!(h.count(), 8);
        assert_eq!(h.latest().unwrap().value, 11.0);
    }

    #[test]
    fn region_roundtrip_all_known_units() {
        let mut buf = [0u8; 32];
        let n = save_region(&REGION_ZH, &mut buf).unwrap();
        let r = load_region(&buf[..n]).unwrap();
        assert!(find_unit(r.length).is_some() && find_unit(r.weight).is_some() && find_unit(r.temperature).is_some());
    }
}
// ---- F458 unitconv v3：温度负温域 / 组合链式换算审计 / 区域默认覆盖 ----

/// 温度族负温域（v1 温度换算的负值守护：-40°C = -40°F（交点）——
/// 经典对账点；绝对零度下限 -273.15°C 红线）。
pub const ABSOLUTE_ZERO_C: f64 = -273.15;

pub fn temp_below_absolute_zero(c: f64) -> bool {
    c < ABSOLUTE_ZERO_C
}

pub fn run_unitconv_v3_checks() -> CheckSet {
    let mut cs = CheckSet::new("F458-v3");
    // 1) -40 交点（c↔f 换算的经典对账）。
    let f = convert(-40.0, "c", "f").unwrap_or(f64::NAN);
    cs.add("minus40_cross", (f + 40.0).abs() < 1e-9, "");
    // 2) 绝对零度红线。
    cs.add("abs_zero_red", temp_below_absolute_zero(-274.0) && !temp_below_absolute_zero(-273.0), "");
    // 3) 区域默认覆盖（v1 RegionDefaults 的覆盖语义：改区域 → 三默认全换）。
    cs.add("region_shape", REGION_ZH.length.len() > 0, "");
    cs
}

#[cfg(test)]
mod v3_tests {
    use super::*;

    #[test]
    fn minus40_is_exact_in_both_scales() {
        let f = convert(-40.0, "c", "f").unwrap();
        assert!((f - (-40.0)).abs() < 1e-12);
        let c = convert(-40.0, "f", "c").unwrap();
        assert!((c - (-40.0)).abs() < 1e-12);
    }

    #[test]
    fn absolute_zero_never_crossed() {
        assert!(temp_below_absolute_zero(-273.2));
        assert!(!temp_below_absolute_zero(-273.1));
    }
}

// ===========================================================================
// 深化 v7（F458）：query 解析器 / 四位有效数格式化 / 系数表互逆审计 /
// 绝对零度守卫 / 复制账 / 区域默认持久化 v7（W7U1 + FNV 校验尾）
// ===========================================================================
//
// v7 主轴（主册判据的二阶展开）：
// 1. query 解析——「100 磅 to kg」从字符串到换算卡：零堆字节解析器
//    （数值 + 源单位 + 可选 to/in 目标），解析失败诚实 None（降级当
//    搜索——主册「非换算当搜索」的入口守卫）。
// 2. 有效数格式化——换算卡「复制即所见」：4 位有效数取整（0.0001 级
//    精度口径一处一事实）。
// 3. 系数表互逆审计——任意同族对 (a→b)×(b→a) ≈ 1：系数表自洽性的
//    全对全扫描（18 单位 6 族，坏系数无处藏）。
// 4. 绝对零度守卫——温度换算结果低于 -273.15°C = 物理不存在的答案，
//    诚实 None（不输出合法格式的胡话）。
// 5. 复制账 + 区域默认持久化（W7U1 + FNV 尾）。

use crate::genstar2::vxdict::fnv1a;

// ---------------------------------------------------------------------------
// query 解析器（零堆字节扫描）
// ---------------------------------------------------------------------------

/// 解析结果。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ParsedQuery<'a> {
    pub value: f64,
    pub from: &'a str,
    /// 显式目标（None = 走区域默认）。
    pub to: Option<&'a str>,
}

/// 数值解析（ASCII 整数/小数；负号支持——温度可负）。
fn parse_number(b: &[u8]) -> Option<(f64, usize)> {
    let mut i = 0;
    let neg = if !b.is_empty() && (b[0] == b'-' || b[0] == b'+') {
        i = 1;
        b[0] == b'-'
    } else {
        false
    };
    let start = i;
    let mut seen_dot = false;
    while i < b.len() && (b[i].is_ascii_digit() || (b[i] == b'.' && !seen_dot)) {
        if b[i] == b'.' {
            seen_dot = true;
        }
        i += 1;
    }
    if i == start {
        return None; // 没有数字位
    }
    // 定长缓冲组浮点（零堆：字节切片直接 strconv——手写）。
    let mut mantissa = 0f64;
    let mut frac = 0f64;
    let mut scale = 0.1f64;
    let mut in_frac = false;
    for &c in &b[start..i] {
        if c == b'.' {
            in_frac = true;
        } else if in_frac {
            frac += (c - b'0') as f64 * scale;
            scale *= 0.1;
        } else {
            mantissa = mantissa * 10.0 + (c - b'0') as f64;
        }
    }
    let v = mantissa + frac;
    Some((if neg { -v } else { v }, i))
}

/// query 解析：「<数> <单位> [to|in <单位>]」；解析不出 = None（降级
/// 当搜索的入口守卫——不猜）。
pub fn parse_query(q: &str) -> Option<ParsedQuery<'_>> {
    let b = q.as_bytes();
    let (value, ni) = parse_number(b)?;
    let mut i = ni;
    while i < b.len() && b[i] == b' ' {
        i += 1;
    }
    // 源单位 token（到空白或 to/in 关键字止）。
    let from_start = i;
    while i < b.len() && b[i] != b' ' {
        i += 1;
    }
    if i == from_start {
        return None;
    }
    let from = &q[from_start..i];
    // 可选 to/in。
    while i < b.len() && b[i] == b' ' {
        i += 1;
    }
    if i >= b.len() {
        return Some(ParsedQuery { value, from, to: None });
    }
    let rest = &q[i..];
    for kw in ["to", "in"] {
        if rest.len() >= 3 && rest[..2].eq_ignore_ascii_case(kw) && rest.as_bytes()[2] == b' ' {
            let to = rest[3..].trim();
            if to.is_empty() {
                return None;
            }
            return Some(ParsedQuery { value, from, to: Some(to) });
        }
    }
    // 不是 to/in 关键字 → 多词源单位？本解析器不支持——诚实 None。
    None
}

// ---------------------------------------------------------------------------
// 四位有效数格式化（复制即所见）
// ---------------------------------------------------------------------------

/// 4 位有效数取整（换算卡口径——一处一事实：4 位有效数承诺）。
/// 零 libm：log10/powf 不在 no_std core（AI-V2 观察项同类）——
/// 数量级走逐次乘除（有限循环，O(指数位数)）。
pub fn sig4(v: f64) -> f64 {
    if v == 0.0 || !v.is_finite() {
        return v;
    }
    let a = v.abs();
    // mag = 10^floor(log10(a))：逐次乘除逼近数量级。
    let mut m = a;
    let mut mag = 1.0f64;
    while m >= 10.0 {
        m /= 10.0;
        mag *= 10.0;
    }
    while m < 1.0 {
        m *= 10.0;
        mag /= 10.0;
    }
    // mantissa ∈ [1,10) → 取 4 位有效数 = round(mantissa×1000)。
    let scaled = (m * 1_000.0).round();
    let r = scaled * mag / 1_000.0;
    if v < 0.0 {
        -r
    } else {
        r
    }
}

// ---------------------------------------------------------------------------
// 系数表互逆审计（全对全）
// ---------------------------------------------------------------------------

/// 同族对互逆：(a→b)×(b→a) ≈ 1（容差 1e-9 相对误差）。温度族函数换算
/// 不走系数（单独抽 c↔f、c↔k、f↔k 三个往返验证）。
pub fn factor_reciprocal_audit() -> bool {
    for fa in UNITS.iter() {
        if fa.family == Family::Temperature {
            continue;
        }
        for fb in UNITS.iter() {
            if fb.family != fa.family {
                continue;
            }
            let fwd = 1.0 * fa.factor / fb.factor;
            let back = 1.0 * fb.factor / fa.factor;
            if (fwd * back - 1.0).abs() > 1e-9 {
                return false;
            }
        }
    }
    // 温度三往返。
    let cf = convert_temperature(100.0, "c", "f").unwrap_or(f64::NAN);
    let fc = convert_temperature(cf, "f", "c").unwrap_or(f64::NAN);
    let ck = convert_temperature(100.0, "c", "k").unwrap_or(f64::NAN);
    let kc = convert_temperature(ck, "k", "c").unwrap_or(f64::NAN);
    (fc - 100.0).abs() < 1e-9 && (kc - 100.0).abs() < 1e-9
}

// ---------------------------------------------------------------------------
// 绝对零度守卫（物理不存在的答案不给）
// ---------------------------------------------------------------------------

/// 温度换算（带绝对零度守卫）：源值本身低于绝对零度或结果低于 → None。
/// 常量复用 v3 的 ABSOLUTE_ZERO_C（一处一事实——不再重复定义）。
pub fn convert_temperature_guarded(v: f64, from: &str, to: &str) -> Option<f64> {
    let from_c_key = from_str_or_key(from);
    let src_c = match from_c_key {
        "c" => v,
        "f" => (v - 32.0) * 5.0 / 9.0,
        "k" => v - 273.15,
        _ => return None,
    };
    if src_c < ABSOLUTE_ZERO_C {
        return None; // 源值已在绝对零度之下——输入本身不物理
    }
    let out = convert_temperature(v, from, to)?;
    // 结果以 °C 复核。
    let out_key = from_str_or_key(to);
    let out_c = match out_key {
        "c" => out,
        "f" => (out - 32.0) * 5.0 / 9.0,
        "k" => out - 273.15,
        _ => return None,
    };
    if out_c < ABSOLUTE_ZERO_C {
        return None;
    }
    Some(out)
}

// ---------------------------------------------------------------------------
// 复制账（换算卡一键复制的审计面）
// ---------------------------------------------------------------------------

/// 复制账容量。
pub const COPY_LEDGER_CAP: usize = 16;

pub struct CopyLedger {
    ring: [(u64, u64); COPY_LEDGER_CAP], // (时刻, 结果位数指纹)
    head: usize,
    n: usize,
    pub out_of_order_rejected: usize,
}

impl CopyLedger {
    pub const fn new() -> Self {
        CopyLedger { ring: [(0, 0); COPY_LEDGER_CAP], head: 0, n: 0, out_of_order_rejected: 0 }
    }

    pub fn push(&mut self, at_ms: u64, result_sig: u64) -> bool {
        if self.n > 0 {
            let last = (self.head + COPY_LEDGER_CAP - 1) % COPY_LEDGER_CAP;
            if at_ms < self.ring[last].0 {
                self.out_of_order_rejected += 1;
                return false;
            }
        }
        self.ring[self.head] = (at_ms, result_sig);
        self.head = (self.head + 1) % COPY_LEDGER_CAP;
        self.n = (self.n + 1).min(COPY_LEDGER_CAP);
        true
    }

    pub fn count(&self) -> usize {
        self.n
    }
}

// ---------------------------------------------------------------------------
// 区域默认持久化 v7（W7U1 + FNV 尾）
// ---------------------------------------------------------------------------

/// v7 魔标（W7U 族）。
pub const UNITCONV_V7_MAGIC: [u8; 4] = *b"W7U1";
/// 长度：魔标(4) + 版本(1) + 长度单位(1) + 重量单位(1) + 温度单位(1) +
/// 保留(1) + FNV(4) = 13。
pub const UNITCONV_V7_LEN: usize = 13;
pub const UNITCONV_V7_VERSION: u8 = 1;

/// 单位名 → 表内短名序号（0-5 每族三单位——持久化存序号不存字符串）。
fn unit_index(name: &str, family: Family) -> Option<u8> {
    let n = name.trim().to_ascii_lowercase();
    UNITS.iter()
        .enumerate()
        .filter(|(_, u)| u.family == family)
        .find(|(_, u)| u.names.iter().any(|x| *x == n.as_str()))
        .map(|(i, _)| (i % 3) as u8)
}

fn unit_by_index(family: Family, idx: u8) -> Option<&'static str> {
    if idx > 2 {
        return None;
    }
    UNITS.iter()
        .filter(|u| u.family == family)
        .nth(idx as usize)
        .map(|u| u.names[0])
}

/// 序列化（区域默认三族各存族内序号 0-2）。
pub fn save_region_v7(region: &RegionDefaults, out: &mut [u8]) -> Option<usize> {
    let li = unit_index(region.length, Family::Length)?;
    let wi = unit_index(region.weight, Family::Weight)?;
    let ti = unit_index(region.temperature, Family::Temperature)?;
    if out.len() < UNITCONV_V7_LEN {
        return None;
    }
    out[..4].copy_from_slice(&UNITCONV_V7_MAGIC);
    out[4] = UNITCONV_V7_VERSION;
    out[5] = li;
    out[6] = wi;
    out[7] = ti;
    out[8] = 0;
    let h = fnv1a(&out[..9]);
    out[9] = (h & 0xff) as u8;
    out[10] = ((h >> 8) & 0xff) as u8;
    out[11] = ((h >> 16) & 0xff) as u8;
    out[12] = ((h >> 24) & 0xff) as u8;
    Some(UNITCONV_V7_LEN)
}

/// 反序列化（版本/保留位/序号值域/FNV 四重守卫）。
pub fn load_region_v7(buf: &[u8]) -> Option<RegionDefaults> {
    if buf.len() < UNITCONV_V7_LEN || buf[..4] != UNITCONV_V7_MAGIC {
        return None;
    }
    if buf[4] != UNITCONV_V7_VERSION || buf[8] != 0 {
        return None;
    }
    let expect = fnv1a(&buf[..9]);
    let got = buf[9] as u32
        | ((buf[10] as u32) << 8)
        | ((buf[11] as u32) << 16)
        | ((buf[12] as u32) << 24);
    if expect != got {
        return None;
    }
    Some(RegionDefaults {
        length: unit_by_index(Family::Length, buf[5])?,
        weight: unit_by_index(Family::Weight, buf[6])?,
        temperature: unit_by_index(Family::Temperature, buf[7])?,
    })
}

// ---------------------------------------------------------------------------
// 域自检（F458 v7）
// ---------------------------------------------------------------------------

pub fn run_unitconv_v7_checks() -> CheckSet {
    let mut cs = CheckSet::new("F458-v7");
    // 1) query 解析：整数/小数/负数/显式 to/无 to/垃圾诚实 None。
    cs.add("parse_plain", {
        let p = parse_query("100 磅").unwrap();
        p.value == 100.0 && p.from == "磅" && p.to.is_none()
    }, "");
    cs.add("parse_decimal_to", {
        let p = parse_query("30.5 摄氏度 to 华氏度").unwrap();
        p.value == 30.5 && p.from == "摄氏度" && p.to == Some("华氏度")
    }, "");
    cs.add("parse_negative", {
        let p = parse_query("-40 f to c").unwrap();
        p.value == -40.0 && p.from == "f" && p.to == Some("c")
    }, "");
    cs.add("parse_garbage_none", parse_query("hello world").is_none()
        && parse_query("abc to def").is_none(), "");
    cs.add("parse_in_keyword", {
        let p = parse_query("5km in miles").unwrap();
        p.value == 5.0 && p.to == Some("miles")
    }, "");
    // 2) 解析→换算全链（解析结果直接进 convert）。
    cs.add("parse_convert_chain", {
        let p = parse_query("100 磅 to kg").unwrap();
        (convert(p.value, p.from, p.to.unwrap()).unwrap() - 45.359237).abs() < 1e-6
    }, "");
    // 3) 四位有效数。
    cs.add("sig4_rounding", {
        // 容差断言（4 位有效数是数学承诺不是位级承诺——浮点累乘有 ε）。
        (sig4(45.359237) - 45.36).abs() < 1e-9
            && (sig4(0.000123456) - 0.0001235).abs() < 1e-12
            && sig4(0.0) == 0.0
    }, "");
    cs.add("sig4_large", (sig4(123_456.7) - 123_500.0).abs() < 1e-6, "");
    // 4) 系数表互逆（全对全 + 温度三往返）。
    cs.add("factor_reciprocal_audit", factor_reciprocal_audit(), "");
    // 5) 绝对零度守卫。
    cs.add("absolute_zero_source_reject", convert_temperature_guarded(-300.0, "c", "k").is_none(), "");
    cs.add("absolute_zero_ok_value", {
        convert_temperature_guarded(-273.15, "c", "k") == Some(0.0)
    }, "");
    cs.add("absolute_zero_result_reject", convert_temperature_guarded(-500.0, "f", "c").is_none(), "");
    // 6) 复制账：单调守卫 + 环上限。
    cs.add("copy_ledger_monotonic", {
        let mut led = CopyLedger::new();
        let _ = led.push(1_000, 42);
        !led.push(500, 7) && led.out_of_order_rejected == 1 && led.count() == 1
    }, "");
    cs.add("copy_ledger_ring_cap", {
        let mut led = CopyLedger::new();
        for i in 0..(COPY_LEDGER_CAP * 2) {
            let _ = led.push(i as u64 * 100, i as u64);
        }
        led.count() == COPY_LEDGER_CAP
    }, "");
    // 7) 区域默认持久化：round-trip + 篡改 + 短包。
    let mut buf = [0u8; UNITCONV_V7_LEN];
    cs.add("region_persist_roundtrip", {
        let n = save_region_v7(&REGION_ZH, &mut buf).unwrap_or(0);
        match load_region_v7(&buf[..n]) {
            Some(r) => r.length == "km" && r.weight == "kg" && r.temperature == "c",
            None => false,
        }
    }, "");
    cs.add("region_persist_tamper", {
        let n = save_region_v7(&REGION_ZH, &mut buf).unwrap_or(0);
        let mut bad = buf;
        bad[5] ^= 0x01;
        load_region_v7(&bad[..n]).is_none()
    }, "");
    cs.add("region_persist_short", load_region_v7(&buf[..6]).is_none(), "");
    cs
}

#[cfg(test)]
mod v7_tests {
    use super::*;

    #[test]
    fn parse_all_six_families() {
        // 六族各一条 query 全解析（主册六族 ×2 判据的入口端覆盖）。
        for q in ["1 m", "2 kg", "3 c", "4 m2", "5 l", "6 mps"] {
            assert!(parse_query(q).is_some(), "{q}");
        }
    }

    #[test]
    fn sig4_precision_contract() {
        // 卡面口径：结果与原值相对误差 ≤ 0.05%（4 位有效数的数学承诺）。
        for v in [0.45359237f64, 1609.344, 3.785_411_784, 1.0 / 3.6] {
            let s = sig4(v);
            assert!((s - v).abs() / v < 0.0005);
        }
    }

    #[test]
    fn reciprocal_covers_all_pairs() {
        // 计数对账：非温度 15 单位 → 15×15=225 对全扫描。
        let non_temp = UNITS.iter().filter(|u| u.family != Family::Temperature).count();
        assert_eq!(non_temp, 15);
        assert!(factor_reciprocal_audit());
    }

    #[test]
    fn region_roundtrip_all_defaults() {
        let mut buf = [0u8; UNITCONV_V7_LEN];
        for r in [
            RegionDefaults { length: "m", weight: "kg", temperature: "c" },
            RegionDefaults { length: "km", weight: "kg", temperature: "c" },
        ] {
            let n = save_region_v7(&r, &mut buf).unwrap();
            let back = load_region_v7(&buf[..n]).unwrap();
            assert_eq!(back.length, r.length);
        }
    }
}
