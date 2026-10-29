//! UNX-F11079 回归矩阵总扫与封存（矩阵面移交注账）（0x1e9e0 段）
//!
//! 矩阵被切成 10 个面，每面 4 个象限，共 40 个格子；每个格子带独立摘要，
//! 只有整面四格全部盖章后，才允许为该面写一条移交给下一所有者的注账。
//! 封存审计要求 10 面全封、10 条注账全部签收，且不存在指向未成面格子的悬空
//! 注账。不变式：注账数 = 已封存面数 = 已签收注账数。
//!
//! 结构：矩阵格（面/象限/摘要/封位）+ 移交注（目标/轮次/签收）+ 封存账
//! （面扫描、悬空扫描、fold 根、冻结）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 段基址：11079 号卡独占 0x20 步长窗口
const SEG_BASE: u32 = 0x1e9e0;

/// 空账 —— 封存时矩阵尚无任何格子或某面不足四格。
pub const MS_EMPTY: u32 = SEG_BASE + 0x01;
/// 范围 —— 面号不在 0..10、象限不在 0..4 或摘要为零。
pub const MS_RANGE: u32 = SEG_BASE + 0x02;
/// 重号 —— 同一面/象限重复刻格，或同一面重复写注账/重复签收。
pub const MS_DUP: u32 = SEG_BASE + 0x03;
/// 未封 —— 面内仍有格子未盖章，不能移交。
pub const MS_OPEN: u32 = SEG_BASE + 0x04;
/// 悬空 —— 注账指向根本不存在的矩阵面。
pub const MS_DANGLING: u32 = SEG_BASE + 0x05;
/// 封账后写 —— 矩阵已冻结仍尝试刻格、盖章或注账。
pub const MS_FROZEN: u32 = SEG_BASE + 0x06;

/// fold 域标（矩阵封存账）。
pub const MS_TAG: u64 = 0x1ea_79;

/// 矩阵面数。
pub const MS_FACES: u8 = 10;
/// 每面象限数。
pub const MS_QUADS: u8 = 4;

/// 一个矩阵格。
#[derive(Clone, Debug)]
pub struct MatrixCell {
    pub face: u8,
    pub quad: u8,
    pub digest: u32,
    pub sealed: bool,
}

impl MatrixCell {
    /// 格子键，供唯一性审计。
    pub fn key(&self) -> u16 {
        (self.face as u16).wrapping_mul(MS_QUADS as u16)
            .wrapping_add(self.quad as u16)
    }
}

/// 一条面移交注账。
#[derive(Clone, Debug)]
pub struct HandoffNote {
    pub face: u8,
    pub to_owner: u8,
    pub round: u16,
    pub acked: bool,
}

/// 矩阵封存账。
#[derive(Clone, Debug)]
pub struct SealMatrix {
    pub cells: Vec<MatrixCell>,
    pub notes: Vec<HandoffNote>,
    pub frozen: bool,
    pub root: Option<u64>,
}

impl SealMatrix {
    pub fn new() -> SealMatrix {
        SealMatrix { cells: Vec::new(), notes: Vec::new(), frozen: false, root: None }
    }

    fn cell_at(&self, face: u8, quad: u8) -> Option<usize> {
        let mut i = 0usize;
        while i < self.cells.len() {
            if self.cells[i].face == face && self.cells[i].quad == quad {
                return Some(i);
            }
            i += 1;
        }
        None
    }

    fn note_at(&self, face: u8) -> Option<usize> {
        let mut i = 0usize;
        while i < self.notes.len() {
            if self.notes[i].face == face {
                return Some(i);
            }
            i += 1;
        }
        None
    }

    /// 该面已刻格数。
    pub fn face_cells(&self, face: u8) -> u32 {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < self.cells.len() {
            if self.cells[i].face == face {
                n = n.wrapping_add(1);
            }
            i += 1;
        }
        n
    }

    /// 该面四格是否全部盖章。
    pub fn face_sealed(&self, face: u8) -> bool {
        if face >= MS_FACES || self.face_cells(face) != MS_QUADS as u32 {
            return false;
        }
        let mut q = 0u8;
        while q < MS_QUADS {
            match self.cell_at(face, q) {
                Some(i) => {
                    if !self.cells[i].sealed {
                        return false;
                    }
                }
                None => return false,
            }
            q = q.wrapping_add(1);
        }
        true
    }

    /// 刻一个矩阵格。
    pub fn mint(&mut self, face: u8, quad: u8, digest: u32) -> Result<(), u32> {
        if self.frozen {
            return Err(MS_FROZEN);
        }
        if face >= MS_FACES || quad >= MS_QUADS || digest == 0 {
            return Err(MS_RANGE);
        }
        if self.cell_at(face, quad).is_some() {
            return Err(MS_DUP);
        }
        self.cells.push(MatrixCell { face, quad, digest, sealed: false });
        Ok(())
    }

    /// 整面盖章；四格不齐时返回 MS_EMPTY，不产生半封局面。
    pub fn seal_face(&mut self, face: u8) -> Result<u32, u32> {
        if self.frozen {
            return Err(MS_FROZEN);
        }
        if face >= MS_FACES {
            return Err(MS_RANGE);
        }
        if self.face_cells(face) != MS_QUADS as u32 {
            return Err(MS_EMPTY);
        }
        let mut stamped = 0u32;
        let mut i = 0usize;
        while i < self.cells.len() {
            if self.cells[i].face == face {
                self.cells[i].sealed = true;
                stamped = stamped.wrapping_add(1);
            }
            i += 1;
        }
        Ok(stamped)
    }

    /// 写一条面移交注账：面必须四格成面且全部盖章。
    pub fn note_handoff(&mut self, face: u8, to_owner: u8, round: u16) -> Result<(), u32> {
        if self.frozen {
            return Err(MS_FROZEN);
        }
        if face >= MS_FACES || to_owner == 0 || round == 0 {
            return Err(MS_RANGE);
        }
        if self.face_cells(face) == 0 {
            return Err(MS_DANGLING);
        }
        if !self.face_sealed(face) {
            return Err(MS_OPEN);
        }
        if self.note_at(face).is_some() {
            return Err(MS_DUP);
        }
        self.notes.push(HandoffNote { face, to_owner, round, acked: false });
        Ok(())
    }

    /// 签收一条移交注账。
    pub fn ack_handoff(&mut self, face: u8) -> Result<(), u32> {
        if self.frozen {
            return Err(MS_FROZEN);
        }
        let ni = match self.note_at(face) {
            Some(i) => i,
            None => return Err(MS_DANGLING),
        };
        if self.notes[ni].acked {
            return Err(MS_DUP);
        }
        self.notes[ni].acked = true;
        Ok(())
    }

    /// 10 个面全部封存。
    pub fn all_faces_sealed(&self) -> bool {
        if self.cells.len() != (MS_FACES as usize).wrapping_mul(MS_QUADS as usize) {
            return false;
        }
        let mut f = 0u8;
        while f < MS_FACES {
            if !self.face_sealed(f) {
                return false;
            }
            f = f.wrapping_add(1);
        }
        true
    }

    /// 悬空注账计数：注账指向的面不足四格。
    pub fn dangling_notes(&self) -> u32 {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < self.notes.len() {
            if self.face_cells(self.notes[i].face) != MS_QUADS as u32 {
                n = n.wrapping_add(1);
            }
            i += 1;
        }
        n
    }

    /// 移交面 10/10：十条注账全部签收且零悬空。
    pub fn handoff_clean(&self) -> bool {
        if self.notes.len() != MS_FACES as usize || self.dangling_notes() != 0 {
            return false;
        }
        let mut i = 0usize;
        while i < self.notes.len() {
            if !self.notes[i].acked || !self.face_sealed(self.notes[i].face) {
                return false;
            }
            i += 1;
        }
        true
    }

    /// 总扫封存；不满足全封/全签则拒绝并给出首个缺口类别。
    pub fn seal(&self) -> Result<u64, u32> {
        if self.cells.is_empty() {
            return Err(MS_EMPTY);
        }
        if self.cells.len() != (MS_FACES as usize).wrapping_mul(MS_QUADS as usize) {
            return Err(MS_EMPTY);
        }
        if !self.all_faces_sealed() {
            return Err(MS_OPEN);
        }
        if self.notes.len() != MS_FACES as usize {
            return Err(MS_DANGLING);
        }
        if !self.handoff_clean() {
            return Err(MS_DANGLING);
        }
        Ok(self.fold())
    }

    /// FNV-1a：格子键/摘要/封位与注账面/目标/轮次/签收全部入链。
    pub fn fold(&self) -> u64 {
        let mut h: u64 = MS_TAG;
        let mut i = 0usize;
        while i < self.cells.len() {
            let c = &self.cells[i];
            h = (h ^ c.key() as u64).wrapping_mul(0x100000001b3);
            h = (h ^ c.digest as u64).wrapping_mul(0x100000001b3);
            h = (h ^ if c.sealed { 1 } else { 0 }).wrapping_mul(0x100000001b3);
            i += 1;
        }
        let mut k = 0usize;
        while k < self.notes.len() {
            let n = &self.notes[k];
            h = (h ^ n.face as u64).wrapping_mul(0x100000001b3);
            h = (h ^ n.to_owner as u64).wrapping_mul(0x100000001b3);
            h = (h ^ n.round as u64).wrapping_mul(0x100000001b3);
            h = (h ^ if n.acked { 1 } else { 0 }).wrapping_mul(0x100000001b3);
            k += 1;
        }
        h
    }

    /// 冻结矩阵并锁定封存根。
    pub fn freeze(&mut self) -> u64 {
        self.frozen = true;
        let r = self.fold();
        self.root = Some(r);
        r
    }

    /// 汇总文本。
    pub fn summary(&self) -> String {
        let label = "matrix".to_string();
        format!("{}|cells={}|notes={}|dangling={}|root={}", label, self.cells.len(),
            self.notes.len(), self.dangling_notes(), self.root.unwrap_or(0))
    }
}

/// 四路坏样本：面号越界、格重号、悬空注账、冻结后写。
pub fn inject_four() -> [bool; 4] {
    let mut got = [false; 4];

    let mut a = SealMatrix::new();
    got[0] = a.mint(MS_FACES, 0, 7).err() == Some(MS_RANGE);

    let mut b = SealMatrix::new();
    let _ = b.mint(2, 1, 11);
    got[1] = b.mint(2, 1, 12).err() == Some(MS_DUP);

    let mut c = SealMatrix::new();
    got[2] = c.note_handoff(7, 3, 1).err() == Some(MS_DANGLING);

    let _ = c.mint(0, 0, 99);
    let _ = c.freeze();
    got[3] = c.mint(0, 1, 100).err() == Some(MS_FROZEN);
    got
}

/// 矩阵封存与移交判据。
pub fn run_unx11079_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11079");

    let mut b = SealMatrix::new();
    let mut face = 0u8;
    while face < MS_FACES {
        let mut q = 0u8;
        while q < MS_QUADS {
            let digest = 0x7900u32
                .wrapping_add((face as u32).wrapping_mul(0x11))
                .wrapping_add(q as u32 + 1);
            let _ = b.mint(face, q, digest);
            q = q.wrapping_add(1);
        }
        let _ = b.seal_face(face);
        let _ = b.note_handoff(face, face.wrapping_add(1), 100u16.wrapping_add(face as u16));
        let _ = b.ack_handoff(face);
        face = face.wrapping_add(1);
    }

    // J1 两半：矩阵面逐件核销 10/10，移交注账零悬空。
    let j1a = b.all_faces_sealed() && b.cells.len() == 40;
    let j1b = b.handoff_clean() && b.dangling_notes() == 0;
    s.add("J1-矩阵面封存", j1a, "10 面 40 格全部盖章");
    s.add("J1-移交零悬空", j1b, "10 条移交注账全部签收");

    // J2 守恒：注账数 = 封存面数 = 签收数。
    let mut acked = 0u32;
    let mut i = 0usize;
    while i < b.notes.len() {
        if b.notes[i].acked {
            acked = acked.wrapping_add(1);
        }
        i += 1;
    }
    let j2 = b.notes.len() == 10 && acked == 10 && b.all_faces_sealed();
    s.add("J2-移交守恒", j2, "注账、封存面、签收三者对平");

    // J3 注入四路。
    let inj = inject_four();
    s.add("J3-越界注入", inj[0], "面号越界刻格必拒");
    s.add("J3-重号注入", inj[1], "同象限二次刻格必拒");
    s.add("J3-悬空注入", inj[2], "无格面注账必拒");
    s.add("J3-冻结注入", inj[3], "封账后刻格必拒");

    // J4 冻结：根稳定且三条写路径关闭。
    let mut f = b.clone();
    let root = f.freeze();
    let j4 = f.root == Some(root) && f.fold() == root
        && f.mint(0, 0, 5).err() == Some(MS_FROZEN)
        && f.seal_face(0).err() == Some(MS_FROZEN)
        && f.ack_handoff(0).err() == Some(MS_FROZEN);
    s.add("J4-封存不可变", j4, "冻结后刻格/盖章/签收全拒");

    // J5 指纹敏感：改一个格摘要即改根。
    let mut c = b.clone();
    c.cells[17].digest ^= 0x00ff_00ff;
    let j5 = c.fold() != b.fold();
    s.add("J5-指纹敏感", j5, "单格摘要变化即改变封存根");

    // J6 空账/范围/未封。
    let mut e = SealMatrix::new();
    let empty = e.seal();
    let range = e.mint(3, MS_QUADS, 1);
    let _ = e.mint(1, 0, 44);
    let open = e.note_handoff(1, 2, 1);
    let j6 = empty.err() == Some(MS_EMPTY)
        && range.err() == Some(MS_RANGE)
        && open.err() == Some(MS_OPEN);
    s.add("J6-空账未封", j6, "空账、越界象限、未封注账均拒");

    // J7 唯一有序：40 个格子键互异，10 条注账面号互异。
    let mut unique = true;
    let mut x = 0usize;
    while x < b.cells.len() {
        let mut y = x + 1;
        while y < b.cells.len() {
            if b.cells[x].key() == b.cells[y].key() {
                unique = false;
            }
            y += 1;
        }
        x += 1;
    }
    let mut face_uniq = true;
    let mut n = 0usize;
    while n < b.notes.len() {
        let mut m = n + 1;
        while m < b.notes.len() {
            if b.notes[n].face == b.notes[m].face {
                face_uniq = false;
            }
            m += 1;
        }
        n += 1;
    }
    s.add("J7-键值唯一", unique && face_uniq, "格子键与注账面号均不重复");

    // J8 负语义：九面成账时总扫必须失败。
    let mut partial = SealMatrix::new();
    let mut pf = 0u8;
    while pf < 9 {
        let mut q = 0u8;
        while q < MS_QUADS {
            let _ = partial.mint(pf, q, 0x9000 + pf as u32);
            q = q.wrapping_add(1);
        }
        let _ = partial.seal_face(pf);
        let _ = partial.note_handoff(pf, pf + 1, 1);
        let _ = partial.ack_handoff(pf);
        pf = pf.wrapping_add(1);
    }
    let j8 = !partial.all_faces_sealed() && partial.seal().is_err()
        && partial.notes.len() == 9;
    s.add("J8-九面非绿", j8, "九面成账不得通过总扫");

    // J9 动态面账。
    s.add_dyn("J9-面账动态", b.all_faces_sealed(),
        &format!("faces={} cells={} notes={} dangling={}", 10, b.cells.len(),
            b.notes.len(), b.dangling_notes()));

    // J10 签收路径与重复签收。
    let mut d = SealMatrix::new();
    let mut q = 0u8;
    while q < MS_QUADS {
        let _ = d.mint(0, q, 0x5000 + q as u32);
        q = q.wrapping_add(1);
    }
    let _ = d.seal_face(0);
    let _ = d.note_handoff(0, 1, 1);
    let first = d.ack_handoff(0);
    let dup = d.ack_handoff(0);
    s.add("J10-签收唯一", first.is_ok() && dup.err() == Some(MS_DUP),
        "首次签收成功且二次签收必拒");

    // J11 总扫根与冻结根一致。
    let seal_root = b.seal();
    let j11 = seal_root.ok() == Some(b.fold()) && b.fold() != 0;
    s.add("J11-总扫根", j11, "总扫返回根与 fold 一致");

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx11079_flow() {
        let mut b = SealMatrix::new();
        assert!(b.seal().err() == Some(MS_EMPTY));
        assert!(b.mint(0, 0, 1).is_ok());
        assert!(b.mint(0, 0, 2).err() == Some(MS_DUP));
        assert!(b.note_handoff(0, 1, 1).err() == Some(MS_OPEN));
        assert!(b.mint(0, 1, 2).is_ok());
        assert!(b.mint(0, 2, 3).is_ok());
        assert!(b.mint(0, 3, 4).is_ok());
        assert!(b.seal_face(0).is_ok());
        assert!(b.note_handoff(0, 1, 1).is_ok());
        assert!(b.ack_handoff(0).is_ok());
        let fp = b.fold();
        let root = b.freeze();
        assert!(fp == root && b.seal_face(0).err() == Some(MS_FROZEN));
        for hit in inject_four() {
            assert!(hit, "注入路径必须全部拒止");
        }
    }

    #[test]
    fn unx11079_checks_green() {
        assert!(run_unx11079_checks().all_passed(), "UNX-F11079 判据未全绿");
    }
}
