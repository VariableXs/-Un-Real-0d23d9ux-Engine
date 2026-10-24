//! VE-F0764 九宫格元格式
//! 职责：九宫格元数据文本格式（key=value 行式）、解析与序列化往返、
//!       字段校验（未知键拒绝/必填缺失拒绝）、格式版本协商。
//! 契约：序列化→解析恒等（往返无损）；非法输入全部拒绝且报定位。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 元格式版本
#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct MetaVer {
    pub major: u32,
    pub minor: u32,
}

pub const CURRENT_VER: MetaVer = MetaVer { major: 1, minor: 0 };

/// 九宫格元数据
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct NineMeta {
    pub tex_w: f64,
    pub tex_h: f64,
    pub left: f64,
    pub top: f64,
    pub right: f64,
    pub bottom: f64,
    pub radius: f64,
    pub ver: MetaVer,
}

impl NineMeta {
    pub fn new(tex_w: f64, tex_h: f64, l: f64, t: f64, r: f64, b: f64) -> NineMeta {
        NineMeta { tex_w, tex_h, left: l, top: t, right: r, bottom: b, radius: 0.0, ver: CURRENT_VER }
    }
}

/// 解析错误（含行号）
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct ParseErr {
    pub line: usize,
    pub reason: &'static str,
}

/// 必填键
const REQUIRED: [&str; 6] = ["tex_w", "tex_h", "inset_l", "inset_t", "inset_r", "inset_b"];
/// 已知键
const KNOWN: [&str; 9] = ["tex_w", "tex_h", "inset_l", "inset_t", "inset_r", "inset_b", "radius", "ver", "name"];

/// 解析
pub fn parse(src: &str) -> Result<NineMeta, ParseErr> {
    let mut m = NineMeta::new(0.0, 0.0, 0.0, 0.0, 0.0, 0.0);
    let mut seen: Vec<&str> = alloc::vec![];
    for (i, raw) in src.lines().enumerate() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let (k, v) = match line.split_once('=') {
            Some(kv) => kv,
            None => return Err(ParseErr { line: i + 1, reason: "no '=' separator" }),
        };
        let k = k.trim();
        let v = v.trim();
        if !KNOWN.contains(&k) {
            return Err(ParseErr { line: i + 1, reason: "unknown key" });
        }
        let val: f64 = v.parse().map_err(|_| ParseErr { line: i + 1, reason: "bad number" })?;
        match k {
            "tex_w" => m.tex_w = val,
            "tex_h" => m.tex_h = val,
            "inset_l" => m.left = val,
            "inset_t" => m.top = val,
            "inset_r" => m.right = val,
            "inset_b" => m.bottom = val,
            "radius" => m.radius = val,
            "ver" => {
                let (mj, mi) = v.split_once('.').ok_or(ParseErr { line: i + 1, reason: "bad ver" })?;
                let mj: u32 = mj.parse().map_err(|_| ParseErr { line: i + 1, reason: "bad ver" })?;
                let mi: u32 = mi.parse().map_err(|_| ParseErr { line: i + 1, reason: "bad ver" })?;
                m.ver = MetaVer { major: mj, minor: mi };
            }
            _ => {} // name 等文本键忽略
        }
        seen.push(k);
    }
    for req in REQUIRED {
        if !seen.contains(&req) {
            return Err(ParseErr { line: 0, reason: "missing required key" });
        }
    }
    Ok(m)
}

/// 序列化
pub fn serialize(m: &NineMeta) -> String {
    let mut out = alloc::format!("tex_w={}\ntex_h={}\n", fmt(m.tex_w), fmt(m.tex_h));
    out.push_str(&alloc::format!("inset_l={}\ninset_t={}\ninset_r={}\ninset_b={}\n", fmt(m.left), fmt(m.top), fmt(m.right), fmt(m.bottom)));
    if m.radius != 0.0 {
        out.push_str(&alloc::format!("radius={}\n", fmt(m.radius)));
    }
    out.push_str(&alloc::format!("ver={}.{}\n", m.ver.major, m.ver.minor));
    out
}

/// 数值格式化（去尾零）
fn fmt(v: f64) -> String {
    if v == v.trunc() && v.abs() < 1e15 {
        alloc::format!("{}", v as i64)
    } else {
        alloc::format!("{}", v)
    }
}

/// 版本协商：文件版本与宿主兼容（同主版本）
pub fn ver_ok(m: &NineMeta) -> bool {
    m.ver.major == CURRENT_VER.major && m.ver.minor <= CURRENT_VER.minor
}

pub fn run_vec364_checks() -> crate::checks::CheckSet {
    let mut s = CheckSet::new("svstar2-vec364");
    let meta = NineMeta::new(64.0, 64.0, 8.0, 8.0, 8.0, 8.0);
    // 族一：序列化
    let text = serialize(&meta);
    s.add("序列-含尺寸", text.contains("tex_w=64"), "tex_w line");
    s.add("序列-含边距", text.contains("inset_l=8"), "inset line");
    s.add("序列-含版本", text.contains("ver=1.0"), "ver line");
    s.add("序列-零半径省略", !text.contains("radius"), "omitted");
    // 带半径
    let mut mr = meta;
    mr.radius = 6.0;
    s.add("序列-半径在", serialize(&mr).contains("radius=6"), "radius line");
    // 族二：解析
    let back = parse(&text).expect("back");
    s.add("解析-往返", back == meta, "roundtrip");
    let back_r = parse(&serialize(&mr)).expect("back_r");
    s.add("解析-半径往返", back_r == mr, "radius roundtrip");
    // 注释与空行
    let commented = "# header\ntex_w=64\n\ntex_h=64\ninset_l=8\ninset_t=8\ninset_r=8\ninset_b=8\n";
    s.add("解析-注释跳过", parse(commented).expect("c") == meta, "comments ok");
    // 族三：解析错误
    s.add("错误-无等号", parse("tex_w") == Err(ParseErr { line: 1, reason: "no '=' separator" }), "no sep");
    s.add("错误-未知键", parse("foo=1\ntex_w=64\ntex_h=64\ninset_l=8\ninset_t=8\ninset_r=8\ninset_b=8") == Err(ParseErr { line: 1, reason: "unknown key" }), "unknown");
    s.add("错误-坏数字", parse("tex_w=abc") == Err(ParseErr { line: 1, reason: "bad number" }), "bad num");
    s.add("错误-缺必填", parse("tex_w=64") == Err(ParseErr { line: 0, reason: "missing required key" }), "missing");
    // 行号定位
    s.add("错误-行号", {
        let e = parse("tex_w=64\ntex_h=64\nbad_key=1\ninset_l=8\ninset_t=8\ninset_r=8\ninset_b=8");
        e == Err(ParseErr { line: 3, reason: "unknown key" })
    }, "line 3");
    // 族四：版本协商
    s.add("版本-当前合规", ver_ok(&meta), "1.0 ok");
    let mut old = meta;
    old.ver = MetaVer { major: 0, minor: 9 };
    s.add("版本-跨主拒", !ver_ok(&old), "0.x rejected");
    let mut future = meta;
    future.ver = MetaVer { major: 1, minor: 5 };
    s.add("版本-超minor拒", !ver_ok(&future), "1.5 > host 1.0");
    // 族五：小数格式化
    s.add("格式-小数", serialize(&NineMeta::new(64.5, 64.0, 8.0, 8.0, 8.0, 8.0)).contains("tex_w=64.5"), "decimal kept");
    s.add("格式-整数化", fmt(8.0) == "8", "no trailing zeros");
    // 族六：解析后可构造切片（联动 vec361）
    let m2 = parse(&text).expect("m2");
    let slice = super::vec361_ninecore::Slice::build(
        m2.tex_w, m2.tex_h,
        super::vec361_ninecore::Insets { left: m2.left, top: m2.top, right: m2.right, bottom: m2.bottom },
    );
    s.add("联动-可建切片", slice.is_some(), "meta → slice");
    s
}
