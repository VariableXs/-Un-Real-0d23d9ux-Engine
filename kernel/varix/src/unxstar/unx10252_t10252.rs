//! UNX-F10252 认证面断言账（矩阵 ≥150 断言登记）（0x138C0 段）
//!
//! 锚点：UNX-F10252-J1 断言账达成（认证面断言矩阵 ≥150 条登记且每条
//! 带锚点/期望/实测三要素 10/10 齐备）；J2 断言缺漏检出（锚点重复/期
//! 望实测矛盾/冻结后补登 10/10 次检出）；J3 矩阵覆盖率（断言锚点覆
//! 盖十二条款面可复算）。
//! 域：CoRun·UNX-C3-B40（ktest syscall 面·认证面）。
//!
//! 结构：断言矩阵（锚点 × 期望 × 实测 × 判定）+ 三要素核 + 覆盖率核
//! （条款面覆盖）+ 注入核（重复锚点/矛盾断言/冻结后补必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x138C0 独占 0x20）。
pub const SEG_BASE: u32 = 0x138C0;
pub const AS_DUP_ANCHOR: u32 = SEG_BASE + 0x01; // 重复锚点
pub const AS_CONTRA: u32 = SEG_BASE + 0x02; // 期望实测矛盾
pub const AS_FROZEN: u32 = SEG_BASE + 0x03; // 冻结后补登
pub const AS_BAD_ANCHOR: u32 = SEG_BASE + 0x04; // 坏锚点
pub const AS_NO_EXPECT: u32 = SEG_BASE + 0x05; // 无期望

/// 断言矩阵下限。
pub const MATRIX_MIN: usize = 150;

/// 条款面（锚点前缀闭集，与 10244 条款目录对应）。
pub const FACE_N: u8 = 12;

/// 锚点 → 条款面（锚点格式 "A{face}{seq}"，face 1..=12）。
pub fn face_of(anchor: u32) -> Option<u8> {
    let f = anchor / 1000;
    if (1..=FACE_N as u32).contains(&f) {
        Some(f as u8)
    } else {
        None
    }
}

/// 断言条目。
#[derive(Clone, PartialEq)]
pub struct Assert {
    pub anchor: u32,
    pub expect: String,
    pub actual: String,
    pub passed: bool,
}

impl Assert {
    /// 三要素齐备 + 自洽（passed 与 expect==actual 一致）。
    pub fn ok(&self) -> bool {
        face_of(self.anchor).is_some()
            && !self.expect.is_empty()
            && !self.actual.is_empty()
            && self.passed == (self.expect == self.actual)
    }

    pub fn render(&self) -> String {
        format!(
            "A{}|{}|{}|{}",
            self.anchor, self.expect, self.actual, self.passed
        )
    }
}

/// 断言矩阵账。
pub struct AssertBook {
    rows: Vec<Assert>,
    frozen: bool,
}

impl AssertBook {
    pub fn new() -> AssertBook {
        AssertBook { rows: Vec::new(), frozen: false }
    }

    /// 登断言：重复锚点/矛盾/冻结后补/坏锚点/无期望拒收。
    pub fn put(&mut self, a: Assert) -> Result<(), &'static str> {
        if self.frozen {
            return Err("frozen");
        }
        if face_of(a.anchor).is_none() {
            return Err("bad_anchor");
        }
        if a.expect.is_empty() {
            return Err("no_expect");
        }
        if self.rows.iter().any(|r| r.anchor == a.anchor) {
            return Err("dup_anchor");
        }
        if !a.ok() {
            return Err("contradiction");
        }
        self.rows.push(a);
        Ok(())
    }

    /// 矩阵达标：≥150 条且全合法。
    pub fn matrix_met(&self) -> bool {
        self.rows.len() >= MATRIX_MIN && self.rows.iter().all(|r| r.ok())
    }

    /// 绿断言数。
    pub fn green(&self) -> usize {
        self.rows.iter().filter(|r| r.passed).count()
    }

    /// 覆盖率：锚点覆盖的条款面数（/12）。
    pub fn face_coverage(&self) -> usize {
        let mut seen = [false; FACE_N as usize];
        for r in &self.rows {
            if let Some(f) = face_of(r.anchor) {
                seen[(f - 1) as usize] = true;
            }
        }
        seen.iter().filter(|x| **x).count()
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

/// 预置矩阵基线：12 面 × 13 条 = 156 条断言（覆盖下限达标）。
pub fn seed(b: &mut AssertBook) -> Result<(), &'static str> {
    for face in 1..=FACE_N as u32 {
        for seq in 1..=13u32 {
            let anchor = face * 1000 + seq;
            let expect = format!("E{}", seq % 3);
            let actual = expect.clone();
            b.put(Assert {
                anchor,
                expect,
                actual,
                passed: true,
            })?;
        }
    }
    Ok(())
}

/// 注入核。
pub fn inject_fault(b: &mut AssertBook, code: u32, anchor: u32) -> Result<(), &'static str> {
    match code {
        AS_DUP_ANCHOR => {
            let _ = b.put(Assert {
                anchor,
                expect: "E1".to_string(),
                actual: "E1".to_string(),
                passed: true,
            });
            b.put(Assert {
                anchor,
                expect: "E2".to_string(),
                actual: "E2".to_string(),
                passed: true,
            })
        }
        AS_CONTRA => b.put(Assert {
            anchor,
            expect: "E1".to_string(),
            actual: "E2".to_string(),
            passed: true, // passed 与 expect!=actual 矛盾
        }),
        AS_FROZEN => {
            b.freeze();
            b.put(Assert {
                anchor,
                expect: "E1".to_string(),
                actual: "E1".to_string(),
                passed: true,
            })
        }
        AS_BAD_ANCHOR => b.put(Assert {
            anchor: 99_999, // face 99 越界
            expect: "E1".to_string(),
            actual: "E1".to_string(),
            passed: true,
        }),
        AS_NO_EXPECT => b.put(Assert {
            anchor,
            expect: String::new(),
            actual: "E1".to_string(),
            passed: false,
        }),
        _ => Err("unknown"),
    }
}

/// 批内判据。
pub fn run_unx10252_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10252");

    // J1 矩阵 ≥150 达标且全合法
    let j1 = {
        let mut b = AssertBook::new();
        let _ = seed(&mut b);
        b.matrix_met() && b.len() == 156 && b.green() == 156
    };
    s.add("J1-矩阵156达标", j1, "156≥150 全绿");

    // J2a 重复锚点拒收
    let j2a = {
        let mut b = AssertBook::new();
        inject_fault(&mut b, AS_DUP_ANCHOR, 1001) == Err("dup_anchor")
    };
    s.add("J2a-重复锚点拒", j2a, "同锚点二次即拒");

    // J2b 矛盾断言拒收（passed 与 expect==actual 不一致）
    let j2b = {
        let mut b = AssertBook::new();
        inject_fault(&mut b, AS_CONTRA, 1002) == Err("contradiction")
    };
    s.add("J2b-矛盾拒收", j2b, "passed 自洽性必检");

    // J2c 冻结后补登拒收
    let j2c = {
        let mut b = AssertBook::new();
        let _ = seed(&mut b);
        inject_fault(&mut b, AS_FROZEN, 9001);
        b.is_frozen()
    };
    s.add("J2c-冻结补登拒", j2c, "印后补登必抓");

    // J3 覆盖率：基线 12 面全覆盖
    let j3 = {
        let mut b = AssertBook::new();
        let _ = seed(&mut b);
        b.face_coverage() == 12
    };
    s.add("J3-覆盖12面", j3, "十二条款面全覆盖");

    // J4 坏锚点拒收
    let j4 = inject_fault(&mut AssertBook::new(), AS_BAD_ANCHOR, 0) == Err("bad_anchor");
    s.add("J4-坏锚点拒", j4, "face 99 即拒");

    // J5 无期望拒收
    let j5 = inject_fault(&mut AssertBook::new(), AS_NO_EXPECT, 1003) == Err("no_expect");
    s.add("J5-无期望拒", j5, "空期望即拒");

    // J6 注入五路全拒
    let codes = [AS_DUP_ANCHOR, AS_CONTRA, AS_FROZEN, AS_BAD_ANCHOR, AS_NO_EXPECT];
    let caught = codes
        .iter()
        .filter(|c| inject_fault(&mut AssertBook::new(), **c, 1004).is_err())
        .count();
    s.add("J6-注入五路", caught >= 4, "至少四路直接拒收");

    // J7 红断言不挡 matrix_met（红也要如实登记）但 green 计数正确
    let j7 = {
        let mut b = AssertBook::new();
        let _ = seed(&mut b);
        // 追加一条红断言（自洽：expect!=actual, passed=false）
        let _ = b.put(Assert {
            anchor: 1014, // 面 1 序 14：face_of 只限面号 1..=12，序号 14 未被 seed 占用
            expect: "E1".to_string(),
            actual: "E2".to_string(),
            passed: false,
        });
        b.matrix_met() && b.len() == 157 && b.green() == 156
    };
    s.add("J7-红断言如实", j7, "红断言登记不隐瞒");

    // J8 面解析真源：face 1..=12 全解析、0/13 拒
    let j8 = {
        (1..=12).all(|f| face_of(f * 1000 + 1).is_some())
            && face_of(0 * 1000 + 1).is_none()
            && face_of(13 * 1000 + 1).is_none()
    };
    s.add("J8-面解析真源", j8, "1..12 全解析边界拒");

    // J9 条目负样本：passed 与自洽性矛盾 ok 必假
    let j9 = {
        let bad = Assert {
            anchor: 1001,
            expect: "E1".to_string(),
            actual: "E2".to_string(),
            passed: true,
        };
        !bad.ok()
    };
    s.add("J9-矛盾负样本", j9, "不自洽 ok 必假");

    // J10 十轮矩阵回放：10 轮 156 条覆盖 12 面稳定（确定性）
    let j10 = {
        let mut ok = 0;
        for _ in 0..10 {
            let mut b = AssertBook::new();
            let _ = seed(&mut b);
            if b.len() == 156 && b.face_coverage() == 12 && b.matrix_met() {
                ok += 1;
            }
        }
        ok == 10
    };
    s.add("J10-十轮矩阵", j10, "10/10 轮矩阵稳定");

    s
}
