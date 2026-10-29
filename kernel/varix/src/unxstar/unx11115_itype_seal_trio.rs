//! UNX-F11115 I 型面封存（场/账/协议三件）（0x1f595 段）
//!
//! I 型面由三件组成：场件、账件、协议件。三件必须逐件挂载、逐件核销，
//! 全部核销后方可封存；封存后移交注账必须写回，悬空注账禁止收官。

use crate::alloc::format;
use crate::alloc::string::String;
use crate::checks::CheckSet;
use alloc::vec::Vec;

/// 段基址：F11115 独占 0x1f595。
pub const SEG_BASE: u32 = 0x1f595;

/// 件类越界（合法 1..=3）。
pub const IS_KIND: u32 = SEG_BASE + 0x01;
/// 同一件类重复挂载。
pub const IS_DUP: u32 = SEG_BASE + 0x02;
/// 件未核销，不能封存。
pub const IS_UNVERIFIED: u32 = SEG_BASE + 0x03;
/// 面已封存，拒绝写入。
pub const IS_FROZEN: u32 = SEG_BASE + 0x04;
/// 注账号越界。
pub const IS_NOTE: u32 = SEG_BASE + 0x05;
/// 注账重复写回。
pub const IS_REWRITE: u32 = SEG_BASE + 0x06;
/// 仍有悬空注账。
pub const IS_DANGLING: u32 = SEG_BASE + 0x07;
/// 挂载容量溢出。
pub const IS_OVER: u32 = SEG_BASE + 0x08;

/// 场件类号。
pub const KIND_FIELD: u8 = 1;
/// 账件类号。
pub const KIND_LEDGER: u8 = 2;
/// 协议件类号。
pub const KIND_PROTO: u8 = 3;
/// 三件总数。
pub const PIECES: usize = 3;
/// 注账容量。
pub const NOTE_CAP: usize = 8;
/// 折叠种子。
const IS_TAG: u64 = 0x0000_1f5_9595;
/// FNV-1a 素数。
const FP_STEP: u64 = 0x100000001b3;
/// 件摘要基址。
const DIG_BASE: u64 = 0x4954_0000;

/// 封存件。
#[derive(Clone, Debug)]
pub struct SealPiece {
    /// 件类（1 场 / 2 账 / 3 协议）。
    pub kind: u8,
    /// 件名。
    pub name: String,
    /// 是否已核销。
    pub verified: bool,
    /// 件摘要。
    pub digest: u64,
}

impl SealPiece {
    /// 件渲染，形如 K1:FIELD:V。
    pub fn render(&self) -> String {
        let flag = if self.verified { "V" } else { "U" };
        format!("K{}:{}:{}", self.kind, self.name, flag)
    }
}

/// 移交注账。
#[derive(Clone, Debug)]
pub struct HandoffNote {
    /// 注账号（41 起）。
    pub id: usize,
    /// 接收方标签。
    pub target: String,
    /// 是否已写回。
    pub written: bool,
}

/// I 型面封存总账。
#[derive(Clone, Debug)]
pub struct ItypeSeal {
    pieces: Vec<SealPiece>,
    notes: Vec<HandoffNote>,
    sealed: bool,
    root: u64,
}

impl ItypeSeal {
    /// 建空面。
    pub fn new() -> Self {
        Self { pieces: Vec::new(), notes: Vec::new(), sealed: false, root: 0 }
    }

    /// 挂载一件：类域、重件、容量、冻结四道闸。
    pub fn mount(&mut self, kind: u8, name: &str) -> Result<(), u32> {
        if self.sealed {
            return Err(IS_FROZEN);
        }
        if kind < KIND_FIELD || kind > KIND_PROTO {
            return Err(IS_KIND);
        }
        if self.pieces.len() >= PIECES.saturating_add(1) {
            return Err(IS_OVER);
        }
        if self.has_kind(kind) {
            return Err(IS_DUP);
        }
        let digest = DIG_BASE.wrapping_add(kind as u64);
        let label = String::from(name);
        self.pieces.push(SealPiece { kind, name: label, verified: false, digest });
        Ok(())
    }

    /// 是否已挂该件类。
    pub fn has_kind(&self, kind: u8) -> bool {
        let mut k = 0usize;
        while k < self.pieces.len() {
            if self.pieces[k].kind == kind {
                return true;
            }
            k = k.wrapping_add(1);
        }
        false
    }

    /// 件数。
    pub fn piece_count(&self) -> usize {
        self.pieces.len()
    }

    /// 核销一件：件必须在册且未封存。
    pub fn verify(&mut self, kind: u8) -> Result<(), u32> {
        if self.sealed {
            return Err(IS_FROZEN);
        }
        let mut idx = usize::MAX;
        let mut k = 0usize;
        while k < self.pieces.len() {
            if self.pieces[k].kind == kind {
                idx = k;
            }
            k = k.wrapping_add(1);
        }
        if idx == usize::MAX {
            return Err(IS_KIND);
        }
        if let Some(p) = self.pieces.get_mut(idx) {
            p.verified = true;
            return Ok(());
        }
        Err(IS_KIND)
    }

    /// 已核销件数。
    pub fn verified_count(&self) -> usize {
        let mut n = 0usize;
        let mut k = 0usize;
        while k < self.pieces.len() {
            if self.pieces[k].verified {
                n = n.wrapping_add(1);
            }
            k = k.wrapping_add(1);
        }
        n
    }

    /// 三件是否齐备。
    pub fn all_mounted(&self) -> bool {
        self.has_kind(KIND_FIELD) && self.has_kind(KIND_LEDGER) && self.has_kind(KIND_PROTO)
    }

    /// 三件是否全部核销。
    pub fn all_verified(&self) -> bool {
        self.all_mounted() && self.verified_count() == PIECES
    }

    /// 试封存：三件未全核销则拒绝。
    pub fn try_seal(&mut self) -> Result<u64, u32> {
        if self.sealed {
            return Err(IS_FROZEN);
        }
        if !self.all_verified() {
            return Err(IS_UNVERIFIED);
        }
        self.sealed = true;
        self.root = self.fold();
        Ok(self.root)
    }

    /// 已封存否。
    pub fn is_sealed(&self) -> bool {
        self.sealed
    }

    /// 登记移交注账。
    pub fn note(&mut self, id: usize, target: &str) -> Result<(), u32> {
        if self.sealed {
            return Err(IS_FROZEN);
        }
        if id < 41 {
            return Err(IS_NOTE);
        }
        if self.notes.len() >= NOTE_CAP {
            return Err(IS_OVER);
        }
        let mut k = 0usize;
        while k < self.notes.len() {
            if self.notes[k].id == id {
                return Err(IS_DUP);
            }
            k = k.wrapping_add(1);
        }
        let label = String::from(target);
        self.notes.push(HandoffNote { id, target: label, written: false });
        Ok(())
    }

    /// 注账写回。
    pub fn write_off(&mut self, id: usize) -> Result<(), u32> {
        let mut idx = usize::MAX;
        let mut k = 0usize;
        while k < self.notes.len() {
            if self.notes[k].id == id {
                idx = k;
            }
            k = k.wrapping_add(1);
        }
        if idx == usize::MAX {
            return Err(IS_NOTE);
        }
        if let Some(nt) = self.notes.get_mut(idx) {
            if nt.written {
                return Err(IS_REWRITE);
            }
            nt.written = true;
            return Ok(());
        }
        Err(IS_NOTE)
    }

    /// 注账数。
    pub fn note_count(&self) -> usize {
        self.notes.len()
    }

    /// 悬空注账号清单。
    pub fn dangling(&self) -> Vec<usize> {
        let mut out: Vec<usize> = Vec::new();
        let mut k = 0usize;
        while k < self.notes.len() {
            if !self.notes[k].written && out.len() < 16 {
                out.push(self.notes[k].id);
            }
            k = k.wrapping_add(1);
        }
        out.sort_by_key(|n| *n);
        out
    }

    /// 面收官：三件核销、注账非空且零悬空。
    pub fn closeout(&self) -> Result<(), u32> {
        if !self.all_verified() {
            return Err(IS_UNVERIFIED);
        }
        if self.notes.is_empty() {
            return Err(IS_NOTE);
        }
        if !self.dangling().is_empty() {
            return Err(IS_DANGLING);
        }
        Ok(())
    }

    /// FNV-1a 折叠。
    pub fn fold(&self) -> u64 {
        let mut h = IS_TAG;
        let mut k = 0usize;
        while k < self.pieces.len() {
            let p = &self.pieces[k];
            h = (h ^ p.kind as u64).wrapping_mul(FP_STEP);
            h = (h ^ p.digest).wrapping_mul(FP_STEP);
            let b = if p.verified { 1u64 } else { 2u64 };
            h = (h ^ b).wrapping_mul(FP_STEP);
            k = k.wrapping_add(1);
        }
        let mut j = 0usize;
        while j < self.notes.len() {
            let n = &self.notes[j];
            h = (h ^ n.id as u64).wrapping_mul(FP_STEP);
            let b = if n.written { 3u64 } else { 4u64 };
            h = (h ^ b).wrapping_mul(FP_STEP);
            j = j.wrapping_add(1);
        }
        h
    }
}

/// 标准三件：场/账/协议依次挂载并逐件核销。
pub fn build_seal() -> ItypeSeal {
    let mut s = ItypeSeal::new();
    let _ = s.mount(KIND_FIELD, "FIELD");
    let _ = s.mount(KIND_LEDGER, "LEDGER");
    let _ = s.mount(KIND_PROTO, "PROTO");
    let _ = s.verify(KIND_FIELD);
    let _ = s.verify(KIND_LEDGER);
    let _ = s.verify(KIND_PROTO);
    let _ = s.note(41, "B37");
    let _ = s.note(42, "B38");
    let _ = s.note(43, "B39");
    let _ = s.note(44, "B40");
    let _ = s.write_off(41);
    let _ = s.write_off(42);
    let _ = s.write_off(43);
    let _ = s.write_off(44);
    let _ = s.try_seal();
    s
}

/// 四路对抗：件类越界、重件、未核销封存、悬空注账。
pub fn inject_four() -> [bool; 4] {
    let mut a_face = ItypeSeal::new();
    let a = a_face.mount(4, "BAD") == Err(IS_KIND) && !a_face.has_kind(4);
    let mut b_face = ItypeSeal::new();
    let _ = b_face.mount(KIND_FIELD, "FIELD");
    let b = b_face.mount(KIND_FIELD, "FIELD") == Err(IS_DUP) && b_face.piece_count() == 1;
    let mut c_face = ItypeSeal::new();
    let _ = c_face.mount(KIND_FIELD, "FIELD");
    let _ = c_face.mount(KIND_LEDGER, "LEDGER");
    let _ = c_face.verify(KIND_FIELD);
    let c = c_face.try_seal() == Err(IS_UNVERIFIED) && !c_face.is_sealed();
    let mut d_face = ItypeSeal::new();
    let _ = d_face.mount(KIND_FIELD, "FIELD");
    let _ = d_face.mount(KIND_LEDGER, "LEDGER");
    let _ = d_face.mount(KIND_PROTO, "PROTO");
    let _ = d_face.verify(KIND_FIELD);
    let _ = d_face.verify(KIND_LEDGER);
    let _ = d_face.verify(KIND_PROTO);
    let _ = d_face.note(41, "B37");
    let _ = d_face.note(42, "B38");
    let _ = d_face.note(43, "B39");
    let _ = d_face.write_off(41);
    let _ = d_face.write_off(42);
    let d = d_face.closeout() == Err(IS_DANGLING) && d_face.dangling() == [43];
    [a, b, c, d]
}

/// F11115 全部检查：三件逐件核销与注账零悬空。
pub fn run_unx11115_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F11115 I 型面封存");
    let mut face = build_seal();
    let four = inject_four();

    // J1 三件齐备。
    s.add(
        "J1-三件齐备",
        face.piece_count() == PIECES
            && face.all_mounted()
            && face.has_kind(KIND_FIELD)
            && face.has_kind(KIND_LEDGER)
            && face.has_kind(KIND_PROTO),
        "场件、账件、协议件三类全部在册，件数为 3",
    );
    // J2 逐件核销。
    s.add(
        "J2-逐件核销",
        face.verified_count() == PIECES && face.all_verified(),
        "三件逐个核销，核销计数达到 3",
    );
    // J3 封存根。
    s.add(
        "J3-封存根",
        face.is_sealed() && face.fold() != 0 && face.try_seal() == Err(IS_FROZEN),
        "封存后根非零，重复试封存报 IS_FROZEN",
    );
    // J4 件类闸。
    let mut kind_probe = ItypeSeal::new();
    s.add(
        "J4-件类闸",
        kind_probe.mount(0, "ZERO") == Err(IS_KIND)
            && kind_probe.mount(4, "FOUR") == Err(IS_KIND)
            && kind_probe.piece_count() == 0,
        "件类 0 与 4 越出 1..=3，挂载报 IS_KIND 且不入册",
    );
    // J5 重件闸。
    let mut dup_probe = ItypeSeal::new();
    let _ = dup_probe.mount(KIND_LEDGER, "LEDGER");
    s.add(
        "J5-重件闸",
        dup_probe.mount(KIND_LEDGER, "LEDGER") == Err(IS_DUP) && dup_probe.piece_count() == 1,
        "同类件二次挂载报 IS_DUP，在册件数保持 1",
    );
    // J6 未核销不封存。
    let mut half = ItypeSeal::new();
    let _ = half.mount(KIND_FIELD, "FIELD");
    let _ = half.mount(KIND_LEDGER, "LEDGER");
    let _ = half.verify(KIND_FIELD);
    s.add(
        "J6-核销闸",
        half.try_seal() == Err(IS_UNVERIFIED) && !half.is_sealed(),
        "仅核销 1 件时试封存报 IS_UNVERIFIED，面保持未封存",
    );
    // J7 封存后拒挂载。
    let mut frozen_probe = build_seal();
    s.add(
        "J7-封存拒写",
        frozen_probe.mount(KIND_PROTO, "PROTO2") == Err(IS_FROZEN)
            && frozen_probe.note(45, "B41") == Err(IS_FROZEN),
        "封存后挂载与注账均报 IS_FROZEN",
    );
    // J8 注账四件。
    s.add(
        "J8-注账四件",
        face.note_count() == 4,
        "移交注账 41 至 44 共 4 件，覆盖 B37 至 B40",
    );
    // J9 零悬空收官。
    s.add(
        "J9-零悬空",
        face.dangling().is_empty() && face.closeout().is_ok(),
        "四件注账全部写回，悬空清单为空且收官通过",
    );
    // J10 收官需三件核销。
    let mut unverified = ItypeSeal::new();
    let _ = unverified.mount(KIND_FIELD, "FIELD");
    let _ = unverified.note(41, "B37");
    let _ = unverified.write_off(41);
    s.add(
        "J10-收官闸",
        unverified.closeout() == Err(IS_UNVERIFIED),
        "件未核销时收官报 IS_UNVERIFIED",
    );
    // J11 注账号闸与去重。
    let mut note_probe = ItypeSeal::new();
    let early = note_probe.note(40, "B36") == Err(IS_NOTE);
    let first = note_probe.note(41, "B37").is_ok();
    let twice = note_probe.note(41, "B38") == Err(IS_DUP);
    s.add("J11-注账闸", early && first && twice, "注账号 40 越界被拒，41 重号二次登记被拒");
    // J12 重写闸。
    let mut rewrite = build_seal();
    s.add(
        "J12-重写闸",
        rewrite.write_off(41) == Err(IS_REWRITE) && rewrite.write_off(99) == Err(IS_NOTE),
        "已写回注账再次写回报 IS_REWRITE，未在册注账报 IS_NOTE",
    );
    // J13 四路对抗齐备。
    s.add(
        "J13-四路对抗",
        four[0] && four[1] && four[2] && four[3],
        "件类越界、重件、未核销封存、悬空注账四路全部被拒",
    );
    // J14 件渲染。
    let piece = SealPiece {
        kind: KIND_FIELD,
        name: String::from("FIELD"),
        verified: true,
        digest: DIG_BASE.wrapping_add(1),
    };
    s.add(
        "J14-件渲染",
        piece.render() == "K1:FIELD:V",
        "核销后的场件渲染为 K1:FIELD:V",
    );
    // J15 动态明细。
    s.add_dyn(
        "J15-封存明细",
        face.verified_count() == 3 && face.note_count() == 4,
        &format!(
            "件={} 核销={} 注账={} 悬空={}",
            face.piece_count(),
            face.verified_count(),
            face.note_count(),
            face.dangling().len()
        ),
    );
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx11115_seal_flow() {
        let mut face = build_seal();
        assert!(face.all_verified());
        assert!(face.is_sealed());
        assert_eq!(face.piece_count(), PIECES);
        assert_eq!(face.note_count(), 4);
        assert!(face.dangling().is_empty());
        assert!(face.closeout().is_ok());
        assert_eq!(face.try_seal(), Err(IS_FROZEN));
    }

    #[test]
    fn unx11115_checks_green() {
        let s = run_unx11115_checks();
        assert!(s.all_passed(), "{}", s.red_items_text());
    }
}
