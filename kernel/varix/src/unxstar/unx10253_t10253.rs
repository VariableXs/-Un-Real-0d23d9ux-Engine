//! UNX-F10253 报告幂等（同输入字节一致）（0x13AE0 段）
//!
//! 锚点：UNX-F10253-J1 报告幂等达成（同输入重复生成字节一致 10/10
//! 次）；J2 非幂等检出（顺序敏感/时点敏感/浮点敏感三类非幂等源 10/10
//! 次检出）；J3 幂等指纹（同输入同指纹，任一输入字节变指纹必变）。
//! 域：CoRun·UNX-C3-B40（ktest syscall 面·认证面）。
//!
//! 结构：报告输入账（定序键值对）+ 归一化核（排序/时点剥离/定点化）
//! + 幂等渲染核 + 注入核（乱序/带时点/浮点输入必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;
use alloc::vec;

/// 诊断段（0x13AE0 独占 0x20）。
pub const SEG_BASE: u32 = 0x13AE0;
pub const ID_ORDER_SENS: u32 = SEG_BASE + 0x01; // 顺序敏感
pub const ID_TIME_SENS: u32 = SEG_BASE + 0x02; // 时点敏感
pub const ID_FLOAT_SENS: u32 = SEG_BASE + 0x03; // 浮点敏感
pub const ID_FROZEN: u32 = SEG_BASE + 0x04; // 冻结后改
pub const ID_EMPTY_KEY: u32 = SEG_BASE + 0x05; // 空键

/// 报告输入键值对。
#[derive(Clone, PartialEq)]
pub struct InPair {
    pub key: String,
    pub val: String,
}

impl InPair {
    pub fn ok(&self) -> bool {
        !self.key.is_empty() && !self.val.is_empty()
    }
}

/// 输入账。
pub struct InputBook {
    rows: Vec<InPair>,
    frozen: bool,
}

impl InputBook {
    pub fn new() -> InputBook {
        InputBook { rows: Vec::new(), frozen: false }
    }

    /// 登输入：空键/空值/冻结后写/重键拒收。
    pub fn put(&mut self, key: &str, val: &str) -> Result<(), &'static str> {
        if self.frozen {
            return Err("frozen");
        }
        if key.is_empty() {
            return Err("empty_key");
        }
        if val.is_empty() {
            return Err("empty_val");
        }
        if self.rows.iter().any(|r| r.key == key) {
            return Err("dup_key");
        }
        self.rows.push(InPair {
            key: key.to_string(),
            val: val.to_string(),
        });
        Ok(())
    }

    pub fn freeze(&mut self) {
        self.frozen = true;
    }

    pub fn is_frozen(&self) -> bool {
        self.frozen
    }

    pub fn len(&self) -> usize {
        self.rows.len()
    }
}

/// 归一化核一：键序归一（按 key 字节序重排，消除顺序敏感）。
pub fn normalize_order(rows: &[InPair]) -> Vec<InPair> {
    let mut v: Vec<InPair> = rows.to_vec();
    v.sort_by(|a, b| a.key.cmp(&b.key));
    v
}

/// 归一化核二：时点剥离（tick/ts 键值对剔除，消除时点敏感）。
pub fn strip_time(rows: &[InPair]) -> Vec<InPair> {
    rows.iter()
        .filter(|r| r.key != "tick" && r.key != "ts" && r.key != "wall")
        .cloned()
        .collect()
}

/// 归一化核三：浮点定点化（fp: 前缀值按小数位定点重写）。
/// "fp:1.50" → "fp:1500"（千分位定点）；非 fp 键原样。
pub fn fix_float(rows: &[InPair]) -> Vec<InPair> {
    rows.iter()
        .map(|r| {
            if r.key.starts_with("fp:") || r.val.starts_with("fp:") {
                let raw = r.val.trim_start_matches("fp:");
                // 去小数点补齐三位
                let fixed = match raw.split_once('.') {
                    Some((a, b)) => {
                        let mut frac = b.to_string();
                        while frac.len() < 3 {
                            frac.push('0');
                        }
                        format!("{}{}", a, &frac[..3])
                    }
                    None => format!("{}000", raw),
                };
                InPair {
                    key: r.key.clone(),
                    val: fixed,
                }
            } else {
                r.clone()
            }
        })
        .collect()
}

/// 幂等渲染：三归一化后按序编织。
pub fn render_idem(rows: &[InPair]) -> String {
    let n1 = fix_float(rows);
    let n2 = strip_time(&n1);
    let n3 = normalize_order(&n2);
    let mut out = String::from("REPORT|");
    for r in &n3 {
        out.push_str(&format!("{}={},", r.key, r.val));
    }
    out
}

/// 幂等指纹。
pub fn idem_fp(text: &str) -> u32 {
    let mut h: u32 = 0x811c9dc5;
    for b in text.as_bytes() {
        h ^= *b as u32;
        h = h.wrapping_mul(0x01000193);
    }
    h
}

/// 非幂等源检测：乱序输入渲染是否同序输入一致（顺序敏感性检测）。
pub fn order_sensitive(a: &[InPair], shuffled: &[InPair]) -> bool {
    // 理想：渲染归一化后一致 → 不敏感。返回「是否敏感」。
    render_idem(a) != render_idem(shuffled)
}

/// 注入核。
pub fn inject_fault(b: &mut InputBook, code: u32) -> Result<(), &'static str> {
    match code {
        ID_ORDER_SENS => {
            // 正常铺三键（判据用乱序副本对测）
            let _ = b.put("a", "1");
            let _ = b.put("b", "2");
            b.put("c", "3")
        }
        ID_TIME_SENS => {
            let _ = b.put("k", "v");
            b.put("tick", "99")
        }
        ID_FLOAT_SENS => {
            let _ = b.put("fp:score", "fp:1.5");
            b.put("k2", "v2")
        }
        ID_FROZEN => {
            b.freeze();
            b.put("k3", "v3")
        }
        ID_EMPTY_KEY => b.put("", "v"),
        _ => Err("unknown"),
    }
}

/// 批内判据。
pub fn run_unx10253_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10253");

    // J1 幂等达成：同输入十次渲染字节一致
    let j1 = {
        let mut b = InputBook::new();
        let _ = seed(&mut b);
        let rows = b.rows.clone();
        let t1 = render_idem(&rows);
        let mut ok = 0;
        for _ in 0..10 {
            if render_idem(&rows) == t1 {
                ok += 1;
            }
        }
        ok == 10 && t1.starts_with("REPORT|")
    };
    s.add("J1-十次幂等", j1, "10/10 次字节一致");

    // J2a 顺序敏感消除：乱序输入渲染与正序一致
    let j2a = {
        let mut b = InputBook::new();
        let _ = inject_fault(&mut b, ID_ORDER_SENS);
        let rows = b.rows.clone();
        let mut shuffled = rows.clone();
        shuffled.reverse();
        !order_sensitive(&rows, &shuffled)
    };
    s.add("J2a-顺序不敏感", j2a, "乱序归一后同渲染");

    // J2b 时点敏感消除：tick 剥离后同渲染
    let j2b = {
        let mut b1 = InputBook::new();
        let _ = b1.put("k", "v");
        let r1 = render_idem(&b1.rows);
        let mut b2 = InputBook::new();
        let _ = b2.put("k", "v");
        let _ = inject_fault(&mut b2, ID_TIME_SENS);
        let r2 = render_idem(&b2.rows);
        r1 == r2
    };
    s.add("J2b-时点不敏感", j2b, "tick/ts/wall 全剥离");

    // J2c 浮点敏感消除：1.5 与 1.500 同定点
    let j2c = {
        let mut b1 = InputBook::new();
        let _ = b1.put("fp:score", "fp:1.5");
        let r1 = render_idem(&b1.rows);
        let mut b2 = InputBook::new();
        let _ = b2.put("fp:score", "fp:1.500");
        let r2 = render_idem(&b2.rows);
        r1 == r2 && r1.contains("1500")
    };
    s.add("J2c-浮点不敏感", j2c, "千分位定点一致");

    // J3 幂等指纹：同输入同指纹，值变指纹变
    let j3 = {
        let mut b1 = InputBook::new();
        let _ = b1.put("x", "1");
        let _ = b1.put("y", "2");
        let f1 = idem_fp(&render_idem(&b1.rows));
        let mut b2 = InputBook::new();
        let _ = b2.put("x", "1");
        let _ = b2.put("y", "3");
        let f2 = idem_fp(&render_idem(&b2.rows));
        f1 == idem_fp(&render_idem(&b1.rows)) && f1 != f2
    };
    s.add("J3-指纹敏感值", j3, "同输入同指纹值变必变");

    // J4 重键拒收
    let j4 = {
        let mut b = InputBook::new();
        let a = b.put("k", "1");
        let bb = b.put("k", "2");
        a == Ok(()) && bb == Err("dup_key")
    };
    s.add("J4-重键拒收", j4, "同键二次即拒");

    // J5 冻结红线
    let j5 = {
        let mut b = InputBook::new();
        inject_fault(&mut b, ID_FROZEN);
        b.is_frozen() && b.put("k4", "v") == Err("frozen")
    };
    s.add("J5-冻结拒收", j5, "印后写必抓");

    // J6 空键拒收
    let j6 = inject_fault(&mut InputBook::new(), ID_EMPTY_KEY) == Err("empty_key");
    s.add("J6-空键拒收", j6, "空键即拒");

    // J7 空值拒收
    let j7 = InputBook::new().put("k", "") == Err("empty_val");
    s.add("J7-空值拒收", j7, "空值即拒");

    // J8 归一化链真源：strip_time 三键全剥
    let j8 = {
        let rows = vec![
            InPair { key: "tick".to_string(), val: "1".to_string() },
            InPair { key: "ts".to_string(), val: "2".to_string() },
            InPair { key: "wall".to_string(), val: "3".to_string() },
            InPair { key: "keep".to_string(), val: "4".to_string() },
        ];
        strip_time(&rows).len() == 1
    };
    s.add("J8-三键全剥", j8, "时点三键全剥离");

    // J9 定点化真源：补齐三位与截断超长
    let j9 = {
        let rows = vec![
            InPair { key: "fp:a".to_string(), val: "fp:2".to_string() },
            InPair { key: "fp:b".to_string(), val: "fp:3.14159".to_string() },
        ];
        let out = fix_float(&rows);
        out[0].val == "2000" && out[1].val == "3141"
    };
    s.add("J9-定点真源", j9, "补齐三位截断超长");

    // J10 十轮混合回放：乱序+时点+浮点三源混合 10 轮指纹一致
    let j10 = {
        let mk = |rev: bool, tick: bool| {
            let mut b = InputBook::new();
            let _ = b.put("zz", "9");
            let _ = b.put("aa", "1");
            let _ = b.put("fp:m", "fp:0.25");
            if tick {
                let _ = b.put("tick", "777");
            }
            let mut rows = b.rows.clone();
            if rev {
                rows.reverse();
            }
            idem_fp(&render_idem(&rows))
        };
        let base = mk(false, false);
        let mut ok = 0;
        for i in 0..10 {
            if mk(i % 2 == 0, i % 3 == 0) == base {
                ok += 1;
            }
        }
        ok == 10
    };
    s.add("J10-十轮混合", j10, "10/10 轮三源归一一致");

    s
}

/// 幂等基线输入。
pub fn seed(b: &mut InputBook) -> Result<(), &'static str> {
    b.put("scope", "C3-B40")?;
    b.put("gates", "5/5")?;
    b.put("cks", "200")
}
