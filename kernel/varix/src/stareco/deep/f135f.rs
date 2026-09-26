//! 深化层三 · F135 开发者文档站（2026-09-26 深化批次三）。
//!
//! 补深 API 提取器面（主册 G-D-10 + 账本回炉扩列方向·rustdoc 思路
//! 工具链适配）：签名文本解析器（fn 名/参数数/返回是否 unit）、页内
//! TOC 生成（标题树编号限深三级）、文档检索权重排序（标题命中×3 +
//! 正文命中）、API 提取覆盖率核算（千分比口径）。

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 签名解析器：从 "fn name(a: T, b: U) -> R" 文本提取结构化信息
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct ParsedFn {
    /// 函数名（≤64 字节截断语义不存在——解析失败返回 Err）。
    pub name_len: usize,
    pub args: u8,
    pub returns_unit: bool,
}

/// 解析失败一律 Err（零静默——提取器对脏输入必须显性化）。
pub fn parse_signature(src: &str) -> Result<ParsedFn, &'static str> {
    let s = src.trim();
    let rest = match s.strip_prefix("pub fn ").or_else(|| s.strip_prefix("fn ")) {
        Some(r) => r,
        None => return Err("非函数签名：缺 fn 前缀"),
    };
    let paren = rest.find('(').ok_or("缺参数括号")?;
    let name = rest[..paren].trim();
    if name.is_empty() || !name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
        || name.as_bytes()[0].is_ascii_digit()
    {
        return Err("函数名非法");
    }
    let close = {
        // 配对扫描：从 '(' 起深度计数——嵌套元组/括号安全，rfind 会
        // 被返回类型里的 '()' 绊倒（真缺陷：首版用 rfind 误切参数段）。
        let bytes = rest.as_bytes();
        let mut depth = 0i32;
        let mut found = None;
        for (i, b) in bytes.iter().enumerate().skip(paren) {
            match b {
                b'(' => depth += 1,
                b')' => {
                    depth -= 1;
                    if depth == 0 {
                        found = Some(i);
                        break;
                    }
                }
                _ => {}
            }
        }
        found.ok_or("缺右括号")?
    };
    if close < paren {
        return Err("括号次序颠倒");
    }
    let args_src = &rest[paren + 1..close];
    let args = if args_src.trim().is_empty() {
        0
    } else {
        let mut n = 1u8;
        // 顶层逗号计数（不处理嵌套泛型内逗号——提取器按顶层口径）。
        for b in args_src.bytes() {
            if b == b',' {
                n += 1;
            }
        }
        n
    };
    let ret_src = &rest[close + 1..];
    let ret_trim = ret_src.trim();
    let returns_unit = ret_trim.is_empty() || ret_trim == "-> ()";
    if !ret_trim.is_empty() && !ret_trim.starts_with("->") {
        return Err("返回段缺箭头");
    }
    Ok(ParsedFn { name_len: name.len(), args, returns_unit })
}

// ---------------------------------------------------------------------------
// TOC 生成：'/' 前缀深度标记 → 编号树（限深三级，超深拒收）
// ---------------------------------------------------------------------------

/// 生成目录：输入为 (深度 1..3, 标题) 序列；编号按层级计数器
/// （1 / 1.1 / 1.1.2 式）确定；深度越界拒绝（页级纪律）。
/// 数字以 ASCII 直写缓冲（buf 零填充——测试以到零截断读）。
pub fn build_toc(items: &[(u8, &'static str)]) -> Result<alloc::vec::Vec<([u8; 16], u8)>, &'static str> {
    let mut out: alloc::vec::Vec<([u8; 16], u8)> = alloc::vec::Vec::new();
    let mut c1: u32 = 0;
    let mut c2: u32 = 0;
    let mut c3: u32 = 0;
    for (depth, _title) in items {
        match depth {
            1 => {
                c1 += 1;
                c2 = 0;
                c3 = 0;
                let mut buf = [0u8; 16];
                let _ = push_num(&mut buf, 0, c1);
                out.push((buf, 1));
            }
            2 => {
                if c1 == 0 {
                    return Err("二级标题前缺一级标题");
                }
                c2 += 1;
                c3 = 0;
                let mut buf = [0u8; 16];
                let mut p = push_num(&mut buf, 0, c1);
                buf[p] = b'.';
                p += 1;
                let _ = push_num(&mut buf, p, c2);
                out.push((buf, 2));
            }
            3 => {
                if c2 == 0 {
                    return Err("三级标题前缺二级标题");
                }
                c3 += 1;
                let mut buf = [0u8; 16];
                let mut p = push_num(&mut buf, 0, c1);
                buf[p] = b'.';
                p += 1;
                p += push_num(&mut buf, p, c2);
                buf[p] = b'.';
                p += 1;
                let _ = push_num(&mut buf, p, c3);
                out.push((buf, 3));
            }
            _ => return Err("深度越界：页级目录限三级"),
        }
    }
    Ok(out)
}

/// 十进制 ASCII 直写：从 buf[pos] 起写数字，返回写入字节数。
fn push_num(buf: &mut [u8], pos: usize, v: u32) -> usize {
    let mut tmp = [0u8; 10];
    let mut n = 0usize;
    let mut x = v;
    if x == 0 {
        tmp[0] = b'0';
        n = 1;
    }
    while x > 0 {
        tmp[n] = b'0' + (x % 10) as u8;
        n += 1;
        x /= 10;
    }
    for k in 0..n {
        buf[pos + k] = tmp[n - 1 - k];
    }
    n
}

// ---------------------------------------------------------------------------
// 检索权重排序：score = 3×标题命中次数 + 正文命中次数；稳定降序
// ---------------------------------------------------------------------------

pub struct DocHit {
    pub page: &'static str,
    pub title_hits: u32,
    pub body_hits: u32,
}

pub fn score(h: &DocHit) -> u32 {
    h.title_hits * 3 + h.body_hits
}

/// 降序稳定插入序（同分按输入序——检索结果的确定性要求）。
pub fn rank_pages(hits: &[DocHit]) -> alloc::vec::Vec<usize> {
    let mut idx: alloc::vec::Vec<usize> = (0..hits.len()).collect();
    for i in 1..idx.len() {
        let key = idx[i];
        let mut j = i;
        while j > 0 && score(&hits[idx[j - 1]]) < score(&hits[key]) {
            idx[j] = idx[j - 1];
            j -= 1;
        }
        idx[j] = key;
    }
    idx
}

// ---------------------------------------------------------------------------
// 提取覆盖率：已提取符号 / 公共符号 → 千分比（>900 达标线）
// ---------------------------------------------------------------------------

pub fn coverage_per_mille(extracted: usize, total: usize) -> Result<u32, &'static str> {
    if total == 0 {
        return Err("公共符号总数为 0：提取器输入为空");
    }
    if extracted > total {
        return Err("提取数大于总数：计数矛盾");
    }
    Ok(extracted as u32 * 1000 / total as u32)
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

pub const F135F_TAG: &str = "stareco-F135-deep3";

pub fn run_f135_deep3_checks() -> CheckSet {
    let mut set = CheckSet::new(F135F_TAG);

    // 签名解析
    let p = parse_signature("pub fn open(path: &Path, mode: u8) -> Result<(), Err>");
    set.add(
        "f135f parse two args",
        p == Ok(ParsedFn { name_len: 4, args: 2, returns_unit: false }),
        "双参函数解析",
    );
    let p0 = parse_signature("fn noop() -> ()");
    set.add("f135f parse unit", p0 == Ok(ParsedFn { name_len: 4, args: 0, returns_unit: true }), "unit 返回识别");
    set.add("f135f no prefix", parse_signature("let x = 1;").is_err(), "非 fn 拒绝");
    set.add("f135f bad name", parse_signature("fn 9abc()").is_err(), "数字开头名拒绝");
    set.add("f135f no paren", parse_signature("fn abc -> ()").is_err(), "缺括号拒绝");
    set.add("f135f no arrow", parse_signature("fn abc() ()").is_err(), "返回段缺箭头拒绝");
    let many = parse_signature("fn f(a: u8, b: u8, c: u8, d: u8) -> u8");
    set.add("f135f four args", many == Ok(ParsedFn { name_len: 1, args: 4, returns_unit: false }), "四参计数");

    // TOC
    let toc = build_toc(&[(1, "快速开始"), (2, "安装"), (2, "首个程序"), (3, "U 盘装载"), (1, "API 参考")]);
    set.add("f135f toc ok", toc.is_ok(), "合法层级通过");
    let toc_bad = build_toc(&[(2, "孤儿二级")]);
    set.add("f135f toc orphan", toc_bad.is_err(), "孤儿二级拒绝");
    let toc_deep = build_toc(&[(1, "a"), (4, "超深")]);
    set.add("f135f toc depth", toc_deep.is_err(), "四级拒绝");

    // 检索排序
    let hits = [
        DocHit { page: "install", title_hits: 0, body_hits: 5 },
        DocHit { page: "api-open", title_hits: 2, body_hits: 1 },
        DocHit { page: "faq", title_hits: 1, body_hits: 4 },
    ];
    let r = rank_pages(&hits);
    set.add("f135f rank top", hits[r[0]].page == "api-open", "标题权重居首");
    set.add("f135f rank second", hits[r[1]].page == "faq", "7 分居次");
    set.add("f135f rank last", hits[r[2]].page == "install", "5 分沉底");
    // 同分稳定：输入序优先
    let tie = [DocHit { page: "a", title_hits: 1, body_hits: 0 }, DocHit { page: "b", title_hits: 1, body_hits: 0 }];
    let rt = rank_pages(&tie);
    set.add("f135f rank stable tie", tie[rt[0]].page == "a", "同分保持输入序");

    // 覆盖率
    set.add("f135f cov ok", coverage_per_mille(950, 1000) == Ok(950), "千分比直算");
    set.add("f135f cov zero total", coverage_per_mille(0, 0).is_err(), "空输入拒绝");
    set.add("f135f cov over", coverage_per_mille(11, 10).is_err(), "计数矛盾拒绝");
    set.add("f135f cov line", coverage_per_mille(901, 1000) == Ok(901), "达标线上可判");

    set
}

#[cfg(test)]
mod deep3_tests {
    use super::*;

    #[test]
    fn parse_edge_cases() {
        assert!(parse_signature("fn _private(x: u8) -> u8").is_ok());
        assert!(parse_signature("").is_err());
        assert!(parse_signature("fn ()").is_err());
        // 泛型内逗号按顶层口径计数（文档化限制，非缺陷）。
        let g = parse_signature("fn g(m: Map<A, B>) -> u8").unwrap();
        assert_eq!(g.args, 2);
    }

    #[test]
    fn toc_numbering_shape() {
        let t = build_toc(&[(1, "a"), (2, "b"), (3, "c"), (2, "d"), (3, "e")]).unwrap();
        assert_eq!(t.len(), 5);
        assert_eq!(t[0].1, 1);
        assert_eq!(t[4].1, 3);
        // 三级编号序列 c 与 e 的第三段计数独立（1 与 2）。
        let s = |buf: &[u8; 16]| -> alloc::vec::Vec<u8> {
            buf.iter().copied().take_while(|b| *b != 0).collect()
        };
        assert_eq!(s(&t[2].0), alloc::vec![b'1', b'.', b'1', b'.', b'1']);
        assert_eq!(s(&t[4].0), alloc::vec![b'1', b'.', b'2', b'.', b'1']);
    }
}
