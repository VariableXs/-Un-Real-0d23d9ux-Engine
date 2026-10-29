//! UNX-F9167 限额错误矩阵（EINVAL/EPERM/ENOMEM）（0x634A0 段）
//!
//! 锚点：UNX-F9167-J1 限额错账（限额操作 × 触发场景 × 错误码 × 留
//! 痕逐条落账）；J2 限额错语义（场景闭集 + 场景到码唯一映射 + 留痕
//! 必填三性质）；J3 限额错指纹（账本折叠指纹）三判据全绿。
//! 域：CoRun·UNX-C2-B19（限额错误矩阵面）。
//!
//! 结构：错误码闭集 + 触发场景闭集（六格）+ 映射核 + 限额错账本 +
//! 折叠指纹 + 注入核（错码/空留痕/伪场景/重号/印后登记必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x634A0 独占 0x20）。
pub const SEG_BASE: u32 = 0x634A0;
pub const LE_UNK_SCENE: u32 = SEG_BASE + 0x01; // 未知场景
pub const LE_BAD_MAP: u32 = SEG_BASE + 0x02; // 场景错码映射错
pub const LE_EMPTY_NOTE: u32 = SEG_BASE + 0x03; // 空留痕
pub const LE_DUP_NO: u32 = SEG_BASE + 0x04; // 重格号
pub const LE_FROZEN: u32 = SEG_BASE + 0x05; // 印后登记

/// 错误码。
pub const E_EINVAL: u32 = 22;
pub const E_EPERM: u32 = 1;
pub const E_ENOMEM: u32 = 12;
pub const E_COUNT: usize = 3;

pub const E_NAMES: [&str; E_COUNT] = ["EINVAL", "EPERM", "ENOMEM"];

/// 触发场景闭集（六格）。
pub const LC_ORDER: usize = 0; // 软>硬 → EINVAL
pub const LC_BAD_RES: usize = 1; // 伪资源号 → EINVAL
pub const LC_RAISE_HARD: usize = 2; // 非特权提硬限 → EPERM
pub const LC_CROSS_PID: usize = 3; // 无特权跨进程 → EPERM
pub const LC_NOMEM_SEED: usize = 4; // 限额册扩席无内存 → ENOMEM
pub const LC_RTPRIO: usize = 5; // 非特权提实时级 → EPERM
pub const LC_COUNT: usize = 6;

pub const SCENE_NAMES: [&str; LC_COUNT] =
    ["order", "bad-res", "raise-hard", "cross-pid", "nomem-seed", "rtprio"];

/// 场景 → 码唯一映射。
pub fn scene_to_code(scene: usize) -> Result<u32, u32> {
    match scene {
        LC_ORDER | LC_BAD_RES => Ok(E_EINVAL),
        LC_RAISE_HARD | LC_CROSS_PID | LC_RTPRIO => Ok(E_EPERM),
        LC_NOMEM_SEED => Ok(E_ENOMEM),
        _ => Err(LE_UNK_SCENE),
    }
}

/// 单条限额错账。
#[derive(Clone)]
pub struct LimitErr {
    pub cell_no: u32,
    pub scene: usize,
    pub code: u32,
    pub note: String,
}

impl LimitErr {
    pub fn ok(&self) -> bool {
        !self.note.is_empty() && scene_to_code(self.scene) == Ok(self.code)
    }

    pub fn render_line(&self) -> String {
        format!(
            "LE{}|{}|{}|{}",
            self.cell_no,
            SCENE_NAMES[self.scene],
            E_NAMES[(self.code == E_EPERM) as usize
                | ((self.code == E_ENOMEM) as usize) * 2],
            self.note
        )
    }
}

/// 限额错账本。
pub struct LeBook {
    pub cells: Vec<LimitErr>,
    pub next_no: u32,
    pub sealed: bool,
}

impl LeBook {
    pub fn new() -> LeBook {
        LeBook { cells: Vec::new(), next_no: 1, sealed: false }
    }

    pub fn register(&mut self, scene: usize, code: u32, note: &str) -> Result<u32, u32> {
        if self.sealed {
            return Err(LE_FROZEN);
        }
        if note.is_empty() {
            return Err(LE_EMPTY_NOTE);
        }
        if scene_to_code(scene).is_err() {
            return Err(LE_UNK_SCENE);
        }
        if scene_to_code(scene) != Ok(code) {
            return Err(LE_BAD_MAP);
        }
        let no = self.next_no;
        self.next_no += 1;
        self.cells.push(LimitErr { cell_no: no, scene, code, note: note.to_string() });
        Ok(no)
    }

    pub fn full_cover(&self) -> bool {
        for sc in 0..LC_COUNT {
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
        let mut h: u32 = 0x91670;
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
    let bad = LimitErr {
        cell_no: 1,
        scene: LC_ORDER,
        code: E_EPERM, // order 应为 EINVAL
        note: "inj-map".to_string(),
    };
    got[0] = !bad.ok();
    // 2) 空留痕
    let bad2 = LimitErr {
        cell_no: 2,
        scene: LC_ORDER,
        code: E_EINVAL,
        note: String::new(),
    };
    got[1] = !bad2.ok();
    // 3) 伪场景
    let mut b = LeBook::new();
    got[2] = b.register(99, E_EINVAL, "inj-scene").err() == Some(LE_UNK_SCENE);
    // 4) 重格号
    let _ = b.register(LC_ORDER, E_EINVAL, "real");
    b.cells.push(LimitErr {
        cell_no: 1,
        scene: LC_RAISE_HARD,
        code: E_EPERM,
        note: "inj-dup".to_string(),
    });
    got[3] = !b.no_unique();
    // 5) 印后登记必抓
    let mut b2 = LeBook::new();
    b2.seal();
    got[4] = b2.register(LC_ORDER, E_EINVAL, "inj-frozen").err() == Some(LE_FROZEN);
    got
}

/// 判据主体。
pub fn run_unx9167_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F9167");
    let mut b = LeBook::new();

    // J1 限额错账非空
    let _ = b.register(LC_ORDER, E_EINVAL, "软>硬");
    let j1 = !b.cells.is_empty();
    s.add("J1-限额错账非空", j1, "至少一格在册");

    // J2 账字段齐
    let j2 = b.cells.iter().all(|c| c.ok());
    s.add("J2-账字段齐备", j2, "留痕非空且映射一致");

    // J3 六格映射正确
    let j3 = {
        let mut all = true;
        for sc in 0..LC_COUNT {
            let code = scene_to_code(sc).unwrap_or(0);
            let expect = match sc {
                LC_ORDER | LC_BAD_RES => E_EINVAL,
                LC_RAISE_HARD | LC_CROSS_PID | LC_RTPRIO => E_EPERM,
                _ => E_ENOMEM,
            };
            if code != expect {
                all = false;
            }
        }
        all
    };
    s.add("J3-六格映射", j3, "EINVAL×2 / EPERM×3 / ENOMEM×1");

    // J4 映射唯一闸
    let j4 = b.register(LC_ORDER, E_EPERM, "错码").err() == Some(LE_BAD_MAP);
    s.add("J4-映射唯一闸", j4, "场景错码映射错必拒");

    // J5 全覆盖
    let j5 = {
        let mut b2 = LeBook::new();
        let _ = b2.register(LC_ORDER, E_EINVAL, "a");
        let _ = b2.register(LC_BAD_RES, E_EINVAL, "b");
        let _ = b2.register(LC_RAISE_HARD, E_EPERM, "c");
        let _ = b2.register(LC_CROSS_PID, E_EPERM, "d");
        let _ = b2.register(LC_NOMEM_SEED, E_ENOMEM, "e");
        let _ = b2.register(LC_RTPRIO, E_EPERM, "f");
        b2.full_cover()
    };
    s.add("J5-六格全覆盖", j5, "六场景全在册");

    // J6 伪场景拒
    let j6 = b.register(42, E_EINVAL, "伪").err() == Some(LE_UNK_SCENE);
    s.add("J6-伪场景拒", j6, "场景出闭集必拒");

    // J7 码值正确
    let j7 = E_EINVAL == 22 && E_EPERM == 1 && E_ENOMEM == 12;
    s.add("J7-码值正确", j7, "EINVAL=22 EPERM=1 ENOMEM=12");

    // J8 格号递增
    let j8 = {
        let a = b.register(LC_BAD_RES, E_EINVAL, "g1");
        let bb = b.register(LC_RAISE_HARD, E_EPERM, "g2");
        a.is_ok() && bb.is_ok() && bb.unwrap() > a.unwrap()
    };
    s.add("J8-格号递增", j8, "格号严格递增");

    // J9 格号唯一
    let j9 = b.no_unique();
    s.add("J9-格号唯一", j9, "账本格号不重");

    // J10 空留痕拒
    let j10 = b.register(LC_CROSS_PID, E_EPERM, "").err() == Some(LE_EMPTY_NOTE);
    s.add("J10-空留痕拒", j10, "空留痕必拒");

    // J11 指纹稳定
    let j11 = b.fold() == b.fold();
    s.add("J11-指纹稳定", j11, "折叠指纹两次一致");

    // J12 拒绝不落账
    let j12 = {
        let before = b.cells.len();
        let _ = b.register(LC_ORDER, E_EPERM, "不落");
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
    fn unx9167_map() {
        assert_eq!(scene_to_code(LC_ORDER), Ok(E_EINVAL));
        assert_eq!(scene_to_code(LC_RAISE_HARD), Ok(E_EPERM));
        assert_eq!(scene_to_code(LC_NOMEM_SEED), Ok(E_ENOMEM));
        assert!(scene_to_code(99).is_err());
        let mut b = LeBook::new();
        assert!(b.register(LC_ORDER, E_EINVAL, "a").is_ok());
        assert_eq!(b.register(LC_ORDER, E_EPERM, "b"), Err(LE_BAD_MAP));
        assert_eq!(b.register(99, E_EINVAL, "c"), Err(LE_UNK_SCENE));
    }

    #[test]
    fn unx9167_checks_green() {
        let s = run_unx9167_checks();
        assert!(s.all_passed(), "UNX-F9167 判据未全绿");
    }
}
