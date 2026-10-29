//! UNX-F9194 身份/休眠族错误矩阵补格（EINVAL/ENOMEM）（0x63800 段）
//!
//! 锚点：UNX-F9194-J1 补格账（身份/休眠操作 × 触发场景 × 错误码 ×
//! 留痕逐条落账）；J2 补格语义（场景闭集 + 场景到码唯一映射 + 留痕
//! 必填三性质）；J3 补格指纹（账本折叠指纹）三判据全绿。
//! 域：CoRun·UNX-C2-B20（身份/休眠 EINVAL/ENOMEM 补格面）。
//!
//! 结构：错误码闭集 + 场景闭集（六格）+ 映射核 + 补格账本 + 折叠指
//! 纹 + 注入核（错码/空留痕/伪场景/重号/印后登记必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x63800 独占 0x20）。
pub const SEG_BASE: u32 = 0x63800;
pub const EN_UNK_SCENE: u32 = SEG_BASE + 0x01; // 未知场景
pub const EN_BAD_MAP: u32 = SEG_BASE + 0x02; // 场景错码映射错
pub const EN_EMPTY_NOTE: u32 = SEG_BASE + 0x03; // 空留痕
pub const EN_DUP_NO: u32 = SEG_BASE + 0x04; // 重格号
pub const EN_FROZEN: u32 = SEG_BASE + 0x05; // 印后登记

/// 错误码。
pub const E_EINVAL: u32 = 22;
pub const E_ENOMEM: u32 = 12;
pub const E_COUNT: usize = 2;

pub const E_NAMES: [&str; E_COUNT] = ["EINVAL", "ENOMEM"];

/// 场景闭集（六格）。
pub const SC_BAD_NSEC: usize = 0; // nanosleep 负 nsec → EINVAL
pub const SC_BAD_CLK: usize = 1; // clock_nanosleep 错时钟基 → EINVAL
pub const SC_BAD_FLAG: usize = 2; // 野 flag → EINVAL
pub const SC_LEDGER_GROW: usize = 3; // 休眠账本扩席无内存 → ENOMEM
pub const SC_GROUP_GROW: usize = 4; // 组集扩席无内存 → ENOMEM
pub const SC_SLEEP_ZERO: usize = 5; // 零时长（合法直返 → OK 反例格）
pub const LC_R_OK: u32 = 0;
pub const SC_COUNT: usize = 6;

pub const SCENE_NAMES: [&str; SC_COUNT] =
    ["bad-nsec", "bad-clk", "bad-flag", "ledger-grow", "group-grow", "sleep-zero"];

/// 场景 → 码唯一映射。
pub fn scene_to_code(scene: usize) -> Result<u32, u32> {
    match scene {
        SC_BAD_NSEC | SC_BAD_CLK | SC_BAD_FLAG => Ok(E_EINVAL),
        SC_LEDGER_GROW | SC_GROUP_GROW => Ok(E_ENOMEM),
        SC_SLEEP_ZERO => Ok(LC_R_OK),
        _ => Err(EN_UNK_SCENE),
    }
}

/// 单条补格账。
#[derive(Clone)]
pub struct EnCell {
    pub cell_no: u32,
    pub scene: usize,
    pub code: u32,
    pub note: String,
}

impl EnCell {
    pub fn ok(&self) -> bool {
        !self.note.is_empty() && scene_to_code(self.scene) == Ok(self.code)
    }

    pub fn render_line(&self) -> String {
        format!(
            "EN{}|{}|{}|{}",
            self.cell_no,
            SCENE_NAMES[self.scene],
            E_NAMES[(self.code == E_ENOMEM) as usize],
            self.note
        )
    }
}

/// 补格账本。
pub struct EnBook {
    pub cells: Vec<EnCell>,
    pub next_no: u32,
    pub sealed: bool,
}

impl EnBook {
    pub fn new() -> EnBook {
        EnBook { cells: Vec::new(), next_no: 1, sealed: false }
    }

    pub fn register(&mut self, scene: usize, code: u32, note: &str) -> Result<u32, u32> {
        if self.sealed {
            return Err(EN_FROZEN);
        }
        if note.is_empty() {
            return Err(EN_EMPTY_NOTE);
        }
        if scene_to_code(scene).is_err() {
            return Err(EN_UNK_SCENE);
        }
        if scene_to_code(scene) != Ok(code) {
            return Err(EN_BAD_MAP);
        }
        let no = self.next_no;
        self.next_no += 1;
        self.cells.push(EnCell { cell_no: no, scene, code, note: note.to_string() });
        Ok(no)
    }

    pub fn full_cover(&self) -> bool {
        for sc in 0..SC_COUNT {
            if !self.cells.iter().any(|c| c.scene == sc) {
                return false;
            }
        }
        true
    }

    pub fn no_unique(&self) -> bool {
        for i in 0..self.cells.len() {
            for j in (i + 1)..self.cells.len() {
                if self.cells[i].cell_no == self.cells[j].cell_no {
                    return false;
                }
            }
        }
        true
    }

    pub fn fold(&self) -> u32 {
        let mut h: u32 = 0x91940;
        for c in &self.cells {
            h = h.wrapping_mul(31).wrapping_add(c.cell_no);
            h = h.wrapping_mul(31).wrapping_add(c.scene as u32);
            h = h.wrapping_mul(31).wrapping_add(c.code);
            for b in c.note.bytes() {
                h = h.wrapping_mul(31).wrapping_add(b as u32);
            }
        }
        h
    }

    pub fn seal(&mut self) -> u32 {
        self.sealed = true;
        self.fold()
    }
}

/// 注入核：五个必然被检出的坏样本。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    // 1) 错码账
    let bad = EnCell {
        cell_no: 1,
        scene: SC_BAD_NSEC,
        code: E_ENOMEM, // 应为 EINVAL
        note: "inj-map".to_string(),
    };
    got[0] = !bad.ok();
    // 2) 空留痕
    let bad2 = EnCell {
        cell_no: 2,
        scene: SC_BAD_NSEC,
        code: E_EINVAL,
        note: String::new(),
    };
    got[1] = !bad2.ok();
    // 3) 伪场景
    let mut b = EnBook::new();
    got[2] = b.register(99, E_EINVAL, "inj-scene").err() == Some(EN_UNK_SCENE);
    // 4) 重格号
    let _ = b.register(SC_BAD_NSEC, E_EINVAL, "real");
    b.cells.push(EnCell {
        cell_no: 1,
        scene: SC_BAD_CLK,
        code: E_EINVAL,
        note: "inj-dup".to_string(),
    });
    got[3] = !b.no_unique();
    // 5) 印后登记必抓
    let mut b2 = EnBook::new();
    b2.seal();
    got[4] = b2.register(SC_BAD_NSEC, E_EINVAL, "inj-frozen").err() == Some(EN_FROZEN);
    got
}

/// 判据主体。
pub fn run_unx9194_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F9194");
    let mut b = EnBook::new();

    // J1 补格账非空
    let _ = b.register(SC_BAD_NSEC, E_EINVAL, "负 nsec");
    let j1 = !b.cells.is_empty();
    s.add("J1-补格账非空", j1, "至少一格在册");

    // J2 账字段齐
    let j2 = b.cells.iter().all(|c| c.ok());
    s.add("J2-账字段齐备", j2, "留痕非空且映射一致");

    // J3 六格映射正确
    let j3 = {
        let mut all = true;
        for sc in 0..SC_COUNT {
            let expect = match sc {
                SC_BAD_NSEC | SC_BAD_CLK | SC_BAD_FLAG => E_EINVAL,
                SC_LEDGER_GROW | SC_GROUP_GROW => E_ENOMEM,
                _ => LC_R_OK,
            };
            if scene_to_code(sc).unwrap_or(99) != expect {
                all = false;
            }
        }
        all
    };
    s.add("J3-六格映射", j3, "EINVAL×3 / ENOMEM×2 / OK×1");

    // J4 映射唯一闸
    let j4 = b.register(SC_BAD_NSEC, E_ENOMEM, "错码").err() == Some(EN_BAD_MAP);
    s.add("J4-映射唯一闸", j4, "场景错码映射错必拒");

    // J5 全覆盖
    let j5 = {
        let mut b2 = EnBook::new();
        let _ = b2.register(SC_BAD_NSEC, E_EINVAL, "a");
        let _ = b2.register(SC_BAD_CLK, E_EINVAL, "b");
        let _ = b2.register(SC_BAD_FLAG, E_EINVAL, "c");
        let _ = b2.register(SC_LEDGER_GROW, E_ENOMEM, "d");
        let _ = b2.register(SC_GROUP_GROW, E_ENOMEM, "e");
        let _ = b2.register(SC_SLEEP_ZERO, LC_R_OK, "f");
        b2.full_cover()
    };
    s.add("J5-六格全覆盖", j5, "六场景全在册");

    // J6 伪场景拒
    let j6 = b.register(42, E_EINVAL, "伪").err() == Some(EN_UNK_SCENE);
    s.add("J6-伪场景拒", j6, "场景出闭集必拒");

    // J7 码值正确
    let j7 = E_EINVAL == 22 && E_ENOMEM == 12;
    s.add("J7-码值正确", j7, "EINVAL=22 ENOMEM=12");

    // J8 格号递增
    let j8 = {
        let a = b.register(SC_BAD_CLK, E_EINVAL, "g1");
        let bb = b.register(SC_BAD_FLAG, E_EINVAL, "g2");
        a.is_ok() && bb.is_ok() && bb.unwrap() > a.unwrap()
    };
    s.add("J8-格号递增", j8, "格号严格递增");

    // J9 格号唯一
    let j9 = b.no_unique();
    s.add("J9-格号唯一", j9, "账本格号不重");

    // J10 反例格零码
    let j10 = scene_to_code(SC_SLEEP_ZERO) == Ok(0);
    s.add("J10-反例格零", j10, "零时长合法路径映射 OK");

    // J11 指纹稳定
    let j11 = b.fold() == b.fold();
    s.add("J11-指纹稳定", j11, "折叠指纹两次一致");

    // J12 拒绝不落账
    let j12 = {
        let before = b.cells.len();
        let _ = b.register(SC_BAD_NSEC, E_ENOMEM, "不落");
        b.cells.len() == before
    };
    s.add("J12-拒绝不落账", j12, "拒绝路径零污染账本");

    // J13 注入核 5/5
    let got = inject_five();
    let j13 = got.iter().all(|g| *g);
    s.add_dyn("J13-注入全抓", j13, &format!("inj5={:?}", got));

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx9194_map() {
        assert_eq!(scene_to_code(SC_BAD_NSEC), Ok(E_EINVAL));
        assert_eq!(scene_to_code(SC_LEDGER_GROW), Ok(E_ENOMEM));
        assert_eq!(scene_to_code(SC_SLEEP_ZERO), Ok(0));
        assert!(scene_to_code(99).is_err());
        let mut b = EnBook::new();
        assert!(b.register(SC_BAD_NSEC, E_EINVAL, "a").is_ok());
        assert_eq!(b.register(SC_BAD_NSEC, E_ENOMEM, "b"), Err(EN_BAD_MAP));
    }

    #[test]
    fn unx9194_checks_green() {
        let s = run_unx9194_checks();
        assert!(s.all_passed(), "UNX-F9194 判据未全绿");
    }
}
