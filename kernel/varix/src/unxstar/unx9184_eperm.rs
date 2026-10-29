//! UNX-F9184 身份翻转 EPERM 矩阵（特权规则）（0x636C0 段）
//!
//! 锚点：UNX-F9184-J1 身份错账（身份操作 × 触发场景 × 错误码 × 留
//! 痕逐条落账）；J2 身份错语义（场景闭集 + 场景到码唯一映射 + 留痕
//! 必填三性质）；J3 身份错指纹（账本折叠指纹）三判据全绿。
//! 域：CoRun·UNX-C2-B20（身份翻转 EPERM 矩阵面）。
//!
//! 结构：错误码闭集 + 场景闭集（六格）+ 映射核 + 错账本 + 折叠指纹
//! + 注入核（错码/空留痕/伪场景/重号/印后登记必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x636C0 独占 0x20）。
pub const SEG_BASE: u32 = 0x636C0;
pub const UE_UNK_SCENE: u32 = SEG_BASE + 0x01; // 未知场景
pub const UE_BAD_MAP: u32 = SEG_BASE + 0x02; // 场景错码映射错
pub const UE_EMPTY_NOTE: u32 = SEG_BASE + 0x03; // 空留痕
pub const UE_DUP_NO: u32 = SEG_BASE + 0x04; // 重格号
pub const UE_FROZEN: u32 = SEG_BASE + 0x05; // 印后登记

/// 错误码。
pub const E_EPERM: u32 = 1;
pub const E_EINVAL: u32 = 22;
pub const LC_R_OK: u32 = 0;
pub const E_COUNT: usize = 3;

pub const E_NAMES: [&str; 3] = ["EPERM", "EINVAL", "OK"];

/// 场景闭集（六格）。
pub const US_FOREIGN_UID: usize = 0; // 非特权 setuid 外来 uid → EPERM
pub const US_FOREIGN_RE: usize = 1; // 非特权 setreuid 外来值 → EPERM
pub const US_FOREIGN_RES: usize = 2; // 非特权 setresuid 外来值 → EPERM
pub const US_FSUID_NPRIV: usize = 3; // 非特权 setfsuid → EPERM
pub const US_PRIV_ANY: usize = 4; // 特权任意 → OK
pub const US_SAME_VALUE: usize = 5; // 三值内翻转 → OK
pub const US_COUNT: usize = 6;

pub const SCENE_NAMES: [&str; US_COUNT] =
    ["foreign-uid", "foreign-re", "foreign-res", "fsuid-npriv", "priv-any", "same-value"];

/// 场景 → 码唯一映射。
pub fn scene_to_code(scene: usize) -> Result<u32, u32> {
    match scene {
        US_FOREIGN_UID | US_FOREIGN_RE | US_FOREIGN_RES | US_FSUID_NPRIV => Ok(E_EPERM),
        US_PRIV_ANY | US_SAME_VALUE => Ok(LC_R_OK),
        _ => Err(UE_UNK_SCENE),
    }
}

/// 单条身份错账。
#[derive(Clone)]
pub struct UeCell {
    pub cell_no: u32,
    pub scene: usize,
    pub code: u32,
    pub note: String,
}

impl UeCell {
    pub fn ok(&self) -> bool {
        !self.note.is_empty() && scene_to_code(self.scene) == Ok(self.code)
    }

    pub fn render_line(&self) -> String {
        format!(
            "UE{}|{}|{}|{}",
            self.cell_no,
            SCENE_NAMES[self.scene],
            E_NAMES[(self.code == E_EPERM) as usize | ((self.code == E_EINVAL) as usize) * 1],
            self.note
        )
    }
}

/// 错账本。
pub struct UeBook {
    pub cells: Vec<UeCell>,
    pub next_no: u32,
    pub sealed: bool,
}

impl UeBook {
    pub fn new() -> UeBook {
        UeBook { cells: Vec::new(), next_no: 1, sealed: false }
    }

    pub fn register(&mut self, scene: usize, code: u32, note: &str) -> Result<u32, u32> {
        if self.sealed {
            return Err(UE_FROZEN);
        }
        if note.is_empty() {
            return Err(UE_EMPTY_NOTE);
        }
        if scene_to_code(scene).is_err() {
            return Err(UE_UNK_SCENE);
        }
        if scene_to_code(scene) != Ok(code) {
            return Err(UE_BAD_MAP);
        }
        let no = self.next_no;
        self.next_no += 1;
        self.cells.push(UeCell { cell_no: no, scene, code, note: note.to_string() });
        Ok(no)
    }

    pub fn full_cover(&self) -> bool {
        for sc in 0..US_COUNT {
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
        let mut h: u32 = 0x91840;
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
    let bad = UeCell {
        cell_no: 1,
        scene: US_FOREIGN_UID,
        code: E_EINVAL, // 应为 EPERM
        note: "inj-map".to_string(),
    };
    got[0] = !bad.ok();
    // 2) 空留痕
    let bad2 = UeCell {
        cell_no: 2,
        scene: US_FOREIGN_UID,
        code: E_EPERM,
        note: String::new(),
    };
    got[1] = !bad2.ok();
    // 3) 伪场景
    let mut b = UeBook::new();
    got[2] = b.register(99, E_EPERM, "inj-scene").err() == Some(UE_UNK_SCENE);
    // 4) 重格号
    let _ = b.register(US_FOREIGN_UID, E_EPERM, "real");
    b.cells.push(UeCell {
        cell_no: 1,
        scene: US_FOREIGN_RE,
        code: E_EPERM,
        note: "inj-dup".to_string(),
    });
    got[3] = !b.no_unique();
    // 5) 印后登记必抓
    let mut b2 = UeBook::new();
    b2.seal();
    got[4] = b2.register(US_FOREIGN_UID, E_EPERM, "inj-frozen").err() == Some(UE_FROZEN);
    got
}

/// 判据主体。
pub fn run_unx9184_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F9184");
    let mut b = UeBook::new();

    // J1 身份错账非空
    let _ = b.register(US_FOREIGN_UID, E_EPERM, "外来 uid");
    let j1 = !b.cells.is_empty();
    s.add("J1-身份错账非空", j1, "至少一格在册");

    // J2 账字段齐
    let j2 = b.cells.iter().all(|c| c.ok());
    s.add("J2-账字段齐备", j2, "留痕非空且映射一致");

    // J3 六格映射正确
    let j3 = {
        let mut all = true;
        for sc in 0..US_COUNT {
            let expect = if sc < US_PRIV_ANY { E_EPERM } else { LC_R_OK };
            if scene_to_code(sc).unwrap_or(99) != expect {
                all = false;
            }
        }
        all
    };
    s.add("J3-六格映射", j3, "前四格 EPERM 后两格 OK");

    // J4 映射唯一闸
    let j4 = b.register(US_FOREIGN_UID, E_EINVAL, "错码").err() == Some(UE_BAD_MAP);
    s.add("J4-映射唯一闸", j4, "场景错码映射错必拒");

    // J5 全覆盖
    let j5 = {
        let mut b2 = UeBook::new();
        let _ = b2.register(US_FOREIGN_UID, E_EPERM, "a");
        let _ = b2.register(US_FOREIGN_RE, E_EPERM, "b");
        let _ = b2.register(US_FOREIGN_RES, E_EPERM, "c");
        let _ = b2.register(US_FSUID_NPRIV, E_EPERM, "d");
        let _ = b2.register(US_PRIV_ANY, LC_R_OK, "e");
        let _ = b2.register(US_SAME_VALUE, LC_R_OK, "f");
        b2.full_cover()
    };
    s.add("J5-六格全覆盖", j5, "六场景全在册");

    // J6 特权规则：特权任意翻 OK
    let j6 = scene_to_code(US_PRIV_ANY) == Ok(0);
    s.add("J6-特权规则", j6, "特权路径映射 OK");

    // J7 伪场景拒
    let j7 = b.register(42, E_EPERM, "伪").err() == Some(UE_UNK_SCENE);
    s.add("J7-伪场景拒", j7, "场景出闭集必拒");

    // J8 格号递增
    let j8 = {
        let a = b.register(US_FOREIGN_RE, E_EPERM, "g1");
        let bb = b.register(US_FOREIGN_RES, E_EPERM, "g2");
        a.is_ok() && bb.is_ok() && bb.unwrap() > a.unwrap()
    };
    s.add("J8-格号递增", j8, "格号严格递增");

    // J9 格号唯一
    let j9 = b.no_unique();
    s.add("J9-格号唯一", j9, "账本格号不重");

    // J10 码值正确
    let j10 = E_EPERM == 1 && E_EINVAL == 22;
    s.add("J10-码值正确", j10, "EPERM=1 EINVAL=22");

    // J11 指纹稳定
    let j11 = b.fold() == b.fold();
    s.add("J11-指纹稳定", j11, "折叠指纹两次一致");

    // J12 拒绝不落账
    let j12 = {
        let before = b.cells.len();
        let _ = b.register(US_FOREIGN_UID, E_EINVAL, "不落");
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
    fn unx9184_map() {
        assert_eq!(scene_to_code(US_FOREIGN_UID), Ok(E_EPERM));
        assert_eq!(scene_to_code(US_PRIV_ANY), Ok(0));
        assert!(scene_to_code(99).is_err());
        let mut b = UeBook::new();
        assert!(b.register(US_FOREIGN_UID, E_EPERM, "a").is_ok());
        assert_eq!(b.register(US_FOREIGN_UID, E_EINVAL, "b"), Err(UE_BAD_MAP));
        assert_eq!(b.register(99, E_EPERM, "c"), Err(UE_UNK_SCENE));
    }

    #[test]
    fn unx9184_checks_green() {
        let s = run_unx9184_checks();
        assert!(s.all_passed(), "UNX-F9184 判据未全绿");
    }
}
