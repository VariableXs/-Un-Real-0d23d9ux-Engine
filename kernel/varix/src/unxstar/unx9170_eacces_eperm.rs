//! UNX-F9170 优先级 EACCES/EPERM 矩阵（特权降级规则）（0x63500 段）
//!
//! 锚点：UNX-F9170-J1 优先级错账（优先级操作 × 触发场景 × 错误码 ×
//! 留痕逐条落账）；J2 优先级错语义（场景闭集 + 场景到码唯一映射 +
//! 留痕必填三性质）；J3 优先级错指纹（账本折叠指纹）三判据全绿。
//! 域：CoRun·UNX-C2-B19（优先级 EACCES/EPERM 矩阵面）。
//!
//! 结构：错误码闭集（EACCES=13/EPERM=1/EINVAL=22/ESRCH=3）+ 场景闭
//! 集（六格）+ 映射核 + 错账本 + 折叠指纹 + 注入核（错码/空留痕/
//! 伪场景/重号/印后登记必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x63500 独占 0x20）。
pub const SEG_BASE: u32 = 0x63500;
pub const PE_UNK_SCENE: u32 = SEG_BASE + 0x01; // 未知场景
pub const PE_BAD_MAP: u32 = SEG_BASE + 0x02; // 场景错码映射错
pub const PE_EMPTY_NOTE: u32 = SEG_BASE + 0x03; // 空留痕
pub const PE_DUP_NO: u32 = SEG_BASE + 0x04; // 重格号
pub const PE_FROZEN: u32 = SEG_BASE + 0x05; // 印后登记

/// 错误码。
pub const E_EACCES: u32 = 13;
pub const E_EPERM: u32 = 1;
pub const E_EINVAL: u32 = 22;
pub const E_ESRCH: u32 = 3;
pub const E_COUNT: usize = 4;

pub const E_NAMES: [&str; E_COUNT] = ["EACCES", "EPERM", "EINVAL", "ESRCH"];

/// 触发场景闭集（六格）。
pub const PC_RAISE_NPRIV: usize = 0; // 非特权提级 → EACCES
pub const PC_FOREIGN_TGT: usize = 1; // 目标非自身非同 uid → EPERM
pub const PC_BAD_WHICH: usize = 2; // which 出闭集 → EINVAL
pub const PC_UNK_WHO: usize = 3; // who 无对应进程 → ESRCH
pub const PC_RAISE_ROOT: usize = 4; // 特权提级 → OK（反例格：0）
pub const PC_LOWER_FREE: usize = 5; // 降级自由 → OK（反例格：0）
pub const LC_R_OK: u32 = 0; // 反例格成功码
pub const PC_COUNT: usize = 6;

pub const SCENE_NAMES: [&str; PC_COUNT] =
    ["raise-npriv", "foreign-tgt", "bad-which", "unk-who", "raise-root", "lower-free"];

/// 场景 → 码唯一映射。
pub fn scene_to_code(scene: usize) -> Result<u32, u32> {
    match scene {
        PC_RAISE_NPRIV => Ok(E_EACCES),
        PC_FOREIGN_TGT => Ok(E_EPERM),
        PC_BAD_WHICH => Ok(E_EINVAL),
        PC_UNK_WHO => Ok(E_ESRCH),
        PC_RAISE_ROOT | PC_LOWER_FREE => Ok(LC_R_OK),
        _ => Err(PE_UNK_SCENE),
    }
}

/// 单条优先级错账。
#[derive(Clone)]
pub struct PriErr {
    pub cell_no: u32,
    pub scene: usize,
    pub code: u32,
    pub note: String,
}

impl PriErr {
    pub fn ok(&self) -> bool {
        !self.note.is_empty() && scene_to_code(self.scene) == Ok(self.code)
    }

    pub fn render_line(&self) -> String {
        format!(
            "PE{}|{}|{}|{}",
            self.cell_no,
            SCENE_NAMES[self.scene],
            E_NAMES[(self.code == E_EPERM) as usize
                | ((self.code == E_EINVAL) as usize) * 1
                | ((self.code == E_ESRCH) as usize) * 2
                | ((self.code == E_EACCES) as usize) * 3],
            self.note
        )
    }
}

/// 错账本。
pub struct PeBook {
    pub cells: Vec<PriErr>,
    pub next_no: u32,
    pub sealed: bool,
}

impl PeBook {
    pub fn new() -> PeBook {
        PeBook { cells: Vec::new(), next_no: 1, sealed: false }
    }

    pub fn register(&mut self, scene: usize, code: u32, note: &str) -> Result<u32, u32> {
        if self.sealed {
            return Err(PE_FROZEN);
        }
        if note.is_empty() {
            return Err(PE_EMPTY_NOTE);
        }
        if scene_to_code(scene).is_err() {
            return Err(PE_UNK_SCENE);
        }
        if scene_to_code(scene) != Ok(code) {
            return Err(PE_BAD_MAP);
        }
        let no = self.next_no;
        self.next_no += 1;
        self.cells.push(PriErr { cell_no: no, scene, code, note: note.to_string() });
        Ok(no)
    }

    pub fn full_cover(&self) -> bool {
        for sc in 0..PC_COUNT {
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
        let mut h: u32 = 0x91700;
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
    let bad = PriErr {
        cell_no: 1,
        scene: PC_RAISE_NPRIV,
        code: E_EPERM, // 应为 EACCES
        note: "inj-map".to_string(),
    };
    got[0] = !bad.ok();
    // 2) 空留痕
    let bad2 = PriErr {
        cell_no: 2,
        scene: PC_RAISE_NPRIV,
        code: E_EACCES,
        note: String::new(),
    };
    got[1] = !bad2.ok();
    // 3) 伪场景
    let mut b = PeBook::new();
    got[2] = b.register(99, E_EACCES, "inj-scene").err() == Some(PE_UNK_SCENE);
    // 4) 重格号
    let _ = b.register(PC_RAISE_NPRIV, E_EACCES, "real");
    b.cells.push(PriErr {
        cell_no: 1,
        scene: PC_FOREIGN_TGT,
        code: E_EPERM,
        note: "inj-dup".to_string(),
    });
    got[3] = !b.no_unique();
    // 5) 印后登记必抓
    let mut b2 = PeBook::new();
    b2.seal();
    got[4] = b2.register(PC_RAISE_NPRIV, E_EACCES, "inj-frozen").err() == Some(PE_FROZEN);
    got
}

/// 判据主体。
pub fn run_unx9170_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F9170");
    let mut b = PeBook::new();

    // J1 错账非空
    let _ = b.register(PC_RAISE_NPRIV, E_EACCES, "非特权提级");
    let j1 = !b.cells.is_empty();
    s.add("J1-错账非空", j1, "至少一格在册");

    // J2 账字段齐
    let j2 = b.cells.iter().all(|c| c.ok());
    s.add("J2-账字段齐备", j2, "留痕非空且映射一致");

    // J3 六格映射正确
    let j3 = {
        let mut all = true;
        for sc in 0..PC_COUNT {
            let expect = match sc {
                PC_RAISE_NPRIV => E_EACCES,
                PC_FOREIGN_TGT => E_EPERM,
                PC_BAD_WHICH => E_EINVAL,
                PC_UNK_WHO => E_ESRCH,
                _ => LC_R_OK,
            };
            if scene_to_code(sc).unwrap_or(99) != expect {
                all = false;
            }
        }
        all
    };
    s.add("J3-六格映射", j3, "EACCES/EPERM/EINVAL/ESRCH/OK×2");

    // J4 映射唯一闸
    let j4 = b.register(PC_RAISE_NPRIV, E_EPERM, "错码").err() == Some(PE_BAD_MAP);
    s.add("J4-映射唯一闸", j4, "场景错码映射错必拒");

    // J5 全覆盖
    let j5 = {
        let mut b2 = PeBook::new();
        let _ = b2.register(PC_RAISE_NPRIV, E_EACCES, "a");
        let _ = b2.register(PC_FOREIGN_TGT, E_EPERM, "b");
        let _ = b2.register(PC_BAD_WHICH, E_EINVAL, "c");
        let _ = b2.register(PC_UNK_WHO, E_ESRCH, "d");
        let _ = b2.register(PC_RAISE_ROOT, LC_R_OK, "e");
        let _ = b2.register(PC_LOWER_FREE, LC_R_OK, "f");
        b2.full_cover()
    };
    s.add("J5-六格全覆盖", j5, "六场景全在册");

    // J6 码值正确
    let j6 = E_EACCES == 13 && E_EPERM == 1 && E_EINVAL == 22 && E_ESRCH == 3;
    s.add("J6-码值正确", j6, "EACCES=13 EPERM=1 EINVAL=22 ESRCH=3");

    // J7 伪场景拒
    let j7 = b.register(42, E_EACCES, "伪").err() == Some(PE_UNK_SCENE);
    s.add("J7-伪场景拒", j7, "场景出闭集必拒");

    // J8 反例格：特权提级与降级自由映射 OK
    let j8 = scene_to_code(PC_RAISE_ROOT) == Ok(0) && scene_to_code(PC_LOWER_FREE) == Ok(0);
    s.add("J8-反例格零", j8, "合法路径映射码 0");

    // J9 格号唯一
    let j9 = b.no_unique();
    s.add("J9-格号唯一", j9, "账本格号不重");

    // J10 空留痕拒
    let j10 = b.register(PC_FOREIGN_TGT, E_EPERM, "").err() == Some(PE_EMPTY_NOTE);
    s.add("J10-空留痕拒", j10, "空留痕必拒");

    // J11 指纹稳定
    let j11 = b.fold() == b.fold();
    s.add("J11-指纹稳定", j11, "折叠指纹两次一致");

    // J12 拒绝不落账
    let j12 = {
        let before = b.cells.len();
        let _ = b.register(PC_RAISE_NPRIV, E_EPERM, "不落");
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
    fn unx9170_map() {
        assert_eq!(scene_to_code(PC_RAISE_NPRIV), Ok(E_EACCES));
        assert_eq!(scene_to_code(PC_FOREIGN_TGT), Ok(E_EPERM));
        assert_eq!(scene_to_code(PC_RAISE_ROOT), Ok(0));
        assert!(scene_to_code(99).is_err());
        let mut b = PeBook::new();
        assert!(b.register(PC_RAISE_NPRIV, E_EACCES, "a").is_ok());
        assert_eq!(b.register(PC_RAISE_NPRIV, E_EPERM, "b"), Err(PE_BAD_MAP));
    }

    #[test]
    fn unx9170_checks_green() {
        let s = run_unx9170_checks();
        assert!(s.all_passed(), "UNX-F9170 判据未全绿");
    }
}
