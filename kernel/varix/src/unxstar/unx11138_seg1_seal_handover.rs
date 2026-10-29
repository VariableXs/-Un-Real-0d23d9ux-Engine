//! UNX-F11138 清账段一封存（段一账册移交注账）（0x1f260 段）
//!
//! 纯功能模型：段一收口后要封存三件——账册、判据表、摘要。每件走同一条
//! 五步：立声明摘要 → 独立复算 → 核对（复算须等于声明，一字不合即拒）
//! → 封存 → 凭移交注记核销。核销不是盖章：必须挂一枚移交注记的号，
//! 无见证不得核销。
//!
//! 移交注账是封存之后的正本清源：段二的接收方按注记逐条承接账面行数，
//! 每条注记引用一件已核销的封存件、带非零注记号、带承接行数，开注后
//! 须由接收方回执（ack）才算落账。收口时悬空注记必须为零，承接行数
//! 合计必须等于段一行数 6400——少一行是丢账，多一行是重账。
//!
//! 拒止面：件号/注记号越界、重复登记、件序或注序倒挂、未立声明即复算、
//! 复算与声明不符、未封存即核销、核销缺见证、注记引用未核销件、注记号
//! 重复、承接行数合计不平、悬空注记未清、封盘后动账。

use crate::alloc::format;
use crate::checks::CheckSet;

// ---------------------------------------------------------------- 错误码
const SEG_BASE: u32 = 0x1f260;
/// 件号或注记号越界
pub const SF_NO: u32 = SEG_BASE + 0x01;
/// 重复登记
pub const SF_DUP: u32 = SEG_BASE + 0x02;
/// 件序或注序倒挂
pub const SF_ORDER: u32 = SEG_BASE + 0x03;
/// 未立声明即复算、未封存即核销
pub const SF_SHORT: u32 = SEG_BASE + 0x04;
/// 复算摘要与声明摘要不符
pub const SF_MISMATCH: u32 = SEG_BASE + 0x05;
/// 核销或开注缺见证号
pub const SF_WITNESS: u32 = SEG_BASE + 0x06;
/// 注记引用未核销的封存件
pub const SF_REF: u32 = SEG_BASE + 0x07;
/// 承接行数合计不平
pub const SF_SUM: u32 = SEG_BASE + 0x08;
/// 悬空注记未清
pub const SF_DANGLING: u32 = SEG_BASE + 0x09;
/// 封存账已封盘
pub const SF_FROZEN: u32 = SEG_BASE + 0x0a;

/// 封存件数。
pub const ARTIFACTS: usize = 3;
/// 移交注记条数。
pub const NOTES: usize = 6;
/// 件名。
pub const ART_NAMES: [&str; ARTIFACTS] = ["账册", "判据表", "摘要"];
/// 段一账面行数（与 11136 段账合计对齐）。
pub const LINES_TOTAL: u32 = 6400;
/// 每条注记承接行数。
pub const NOTE_LINES: [u32; NOTES] = [1200, 1200, 1000, 1000, 1000, 1000];
/// 注记号基底。
pub const NOTE_ID_BASE: u32 = 0x3b00;
/// 核销见证号基底。
pub const WITNESS_BASE: u32 = 0x3c00;
/// 注记引用件序（1..=3 循环）。
pub const NOTE_ART: [usize; NOTES] = [1, 2, 3, 1, 2, 3];
/// 逐件复算轮数（摘要复算不是一次读数是八轮折叠）。
pub const FOLD_ROUNDS: usize = 8;

const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x100_0000_01b3;

/// 件名解析；越界返回「越界」。
pub fn art_name(no: usize) -> &'static str {
    if no == 0 || no > ARTIFACTS {
        return "越界";
    }
    match ART_NAMES.get(no - 1) {
        Some(n) => n,
        None => "越界",
    }
}

/// 件摘要的独立复算法：以段基址与件号为种子做八轮折叠。
///
/// 这是「独立复算」的最小可核形：封存时的声明摘要与核销时的复算摘要
/// 走同一纯函数，任何一位不同即拒。
pub fn artifact_digest(base: u32, no: usize) -> u64 {
    let mut h = FNV_OFF ^ (base as u64);
    h = h.wrapping_mul(FNV_PRIME);
    h ^= (no as u64) << 40;
    h = h.wrapping_mul(FNV_PRIME);
    let mut i = 0usize;
    while i < FOLD_ROUNDS {
        h ^= i.wrapping_add(no).wrapping_add(1) as u64;
        h = h.wrapping_mul(FNV_PRIME);
        i = i.wrapping_add(1);
    }
    h
}

/// 见证号是否合法。
pub fn witness_ok(w: u32) -> bool {
    w != 0
}

/// 一件封存物。
#[derive(Clone, Copy, Debug)]
pub struct Artifact {
    /// 件号（1..=3）。
    pub no: usize,
    declared: u64,
    recomputed: u64,
    has_decl: bool,
    has_recomp: bool,
    sealed: bool,
    off: bool,
    witness: u32,
}

impl Artifact {
    /// 空白件。
    pub const fn new(no: usize) -> Artifact {
        Artifact {
            no,
            declared: 0,
            recomputed: 0,
            has_decl: false,
            has_recomp: false,
            sealed: false,
            off: false,
            witness: 0,
        }
    }

    /// 立声明摘要。
    pub fn declare(&mut self, digest: u64) -> Result<(), u32> {
        if self.has_decl || self.sealed || self.off {
            return Err(SF_DUP);
        }
        self.declared = digest;
        self.has_decl = true;
        Ok(())
    }

    /// 独立复算摘要。
    pub fn recompute(&mut self, digest: u64) -> Result<(), u32> {
        if !self.has_decl {
            return Err(SF_SHORT);
        }
        if self.sealed || self.off {
            return Err(SF_ORDER);
        }
        if self.has_recomp {
            return Err(SF_DUP);
        }
        self.recomputed = digest;
        self.has_recomp = true;
        Ok(())
    }

    /// 封存：复算须等于声明。
    pub fn seal(&mut self) -> Result<(), u32> {
        if self.sealed || self.off {
            return Err(SF_DUP);
        }
        if !self.has_decl || !self.has_recomp {
            return Err(SF_SHORT);
        }
        if self.declared != self.recomputed {
            return Err(SF_MISMATCH);
        }
        self.sealed = true;
        Ok(())
    }

    /// 核销：须已封存，且挂见证号。
    pub fn write_off(&mut self, witness: u32) -> Result<(), u32> {
        if self.off {
            return Err(SF_DUP);
        }
        if !self.sealed {
            return Err(SF_SHORT);
        }
        if !witness_ok(witness) {
            return Err(SF_WITNESS);
        }
        self.witness = witness;
        self.off = true;
        Ok(())
    }

    /// 是否已立声明。
    pub fn declared(&self) -> bool {
        self.has_decl
    }

    /// 是否已复算。
    pub fn recomputed(&self) -> bool {
        self.has_recomp
    }

    /// 是否封存。
    pub fn sealed(&self) -> bool {
        self.sealed
    }

    /// 是否核销。
    pub fn off(&self) -> bool {
        self.off
    }

    /// 见证号。
    pub fn witness(&self) -> u32 {
        self.witness
    }

    /// 声明摘要。
    pub fn declared_digest(&self) -> u64 {
        self.declared
    }

    /// 复算摘要。
    pub fn recomputed_digest(&self) -> u64 {
        self.recomputed
    }

    /// 件指纹。
    pub fn digest(&self) -> u64 {
        let mut h = FNV_OFF ^ (self.no as u64);
        h = h.wrapping_mul(FNV_PRIME);
        h ^= self.declared;
        h = h.wrapping_mul(FNV_PRIME);
        h ^= self.recomputed;
        h = h.wrapping_mul(FNV_PRIME);
        h ^= self.witness as u64;
        h = h.wrapping_mul(FNV_PRIME);
        h ^= if self.sealed { 1 } else { 0 };
        h = h.wrapping_mul(FNV_PRIME);
        h ^= if self.off { 1 } else { 0 };
        h
    }
}

/// 一条移交注记。
#[derive(Clone, Copy, Debug)]
pub struct Handover {
    /// 注记序号（1..=6）。
    pub no: usize,
    /// 引用件号（1..=3）。
    pub artifact: usize,
    /// 注记号（非零）。
    pub note_id: u32,
    /// 承接行数。
    pub lines: u32,
    opened: bool,
    acked: bool,
}

impl Handover {
    /// 空白注记。
    pub const fn new(no: usize) -> Handover {
        Handover {
            no,
            artifact: 0,
            note_id: 0,
            lines: 0,
            opened: false,
            acked: false,
        }
    }

    /// 开注：件号、注记号、行数三项自检。
    pub fn open(&mut self, artifact: usize, note_id: u32, lines: u32) -> Result<(), u32> {
        if self.opened || self.acked {
            return Err(SF_DUP);
        }
        if artifact == 0 || artifact > ARTIFACTS {
            return Err(SF_NO);
        }
        if !witness_ok(note_id) {
            return Err(SF_WITNESS);
        }
        if lines == 0 {
            return Err(SF_SUM);
        }
        self.artifact = artifact;
        self.note_id = note_id;
        self.lines = lines;
        self.opened = true;
        Ok(())
    }

    /// 接收方回执。
    pub fn ack(&mut self) -> Result<(), u32> {
        if !self.opened {
            return Err(SF_SHORT);
        }
        if self.acked {
            return Err(SF_DUP);
        }
        self.acked = true;
        Ok(())
    }

    /// 是否已开注。
    pub fn opened(&self) -> bool {
        self.opened
    }

    /// 是否已回执。
    pub fn acked(&self) -> bool {
        self.acked
    }

    /// 是否悬空（开而未回执）。
    pub fn dangling(&self) -> bool {
        self.opened && !self.acked
    }

    /// 注记指纹。
    pub fn digest(&self) -> u64 {
        let mut h = FNV_OFF ^ (self.no as u64);
        h = h.wrapping_mul(FNV_PRIME);
        h ^= self.note_id as u64;
        h = h.wrapping_mul(FNV_PRIME);
        h ^= self.lines as u64;
        h = h.wrapping_mul(FNV_PRIME);
        h ^= ((self.artifact as u64) << 16) | if self.acked { 1 } else { 0 };
        h
    }
}

/// 封存账：三件 + 六注。
#[derive(Clone, Debug)]
pub struct SealLedger {
    /// 段内基址。
    pub base: u32,
    arts: [Artifact; ARTIFACTS],
    notes: [Handover; NOTES],
    note_cursor: usize,
    rejects: u32,
    frozen: bool,
}

impl SealLedger {
    /// 新建三件六注的空账。
    pub fn new(base: u32) -> SealLedger {
        let mut arts = [Artifact::new(1); ARTIFACTS];
        let mut i = 0usize;
        while i < ARTIFACTS {
            if let Some(a) = arts.get_mut(i) {
                *a = Artifact::new(i.wrapping_add(1));
            }
            i = i.wrapping_add(1);
        }
        let mut notes = [Handover::new(1); NOTES];
        let mut k = 0usize;
        while k < NOTES {
            if let Some(n) = notes.get_mut(k) {
                *n = Handover::new(k.wrapping_add(1));
            }
            k = k.wrapping_add(1);
        }
        SealLedger {
            base,
            arts,
            notes,
            note_cursor: 0,
            rejects: 0,
            frozen: false,
        }
    }

    fn fail(&mut self, code: u32) -> u32 {
        self.rejects = self.rejects.wrapping_add(1);
        code
    }

    /// 件只读。
    pub fn art(&self, no: usize) -> Option<Artifact> {
        if no == 0 || no > ARTIFACTS {
            return None;
        }
        self.arts.get(no - 1).copied()
    }

    /// 注记只读。
    pub fn note(&self, no: usize) -> Option<Handover> {
        if no == 0 || no > NOTES {
            return None;
        }
        self.notes.get(no - 1).copied()
    }

    /// 立声明摘要。
    pub fn declare(&mut self, no: usize, digest: u64) -> Result<(), u32> {
        if self.frozen {
            return Err(self.fail(SF_FROZEN));
        }
        if no == 0 || no > ARTIFACTS {
            return Err(self.fail(SF_NO));
        }
        let r = match self.arts.get_mut(no - 1) {
            Some(a) => a.declare(digest),
            None => Err(SF_NO),
        };
        match r {
            Ok(()) => Ok(()),
            Err(e) => Err(self.fail(e)),
        }
    }

    /// 独立复算。
    pub fn recompute(&mut self, no: usize, digest: u64) -> Result<(), u32> {
        if self.frozen {
            return Err(self.fail(SF_FROZEN));
        }
        if no == 0 || no > ARTIFACTS {
            return Err(self.fail(SF_NO));
        }
        let r = match self.arts.get_mut(no - 1) {
            Some(a) => a.recompute(digest),
            None => Err(SF_NO),
        };
        match r {
            Ok(()) => Ok(()),
            Err(e) => Err(self.fail(e)),
        }
    }

    /// 封存一件。
    pub fn seal(&mut self, no: usize) -> Result<(), u32> {
        if self.frozen {
            return Err(self.fail(SF_FROZEN));
        }
        if no == 0 || no > ARTIFACTS {
            return Err(self.fail(SF_NO));
        }
        let r = match self.arts.get_mut(no - 1) {
            Some(a) => a.seal(),
            None => Err(SF_NO),
        };
        match r {
            Ok(()) => Ok(()),
            Err(e) => Err(self.fail(e)),
        }
    }

    /// 核销一件。
    pub fn write_off(&mut self, no: usize, witness: u32) -> Result<(), u32> {
        if self.frozen {
            return Err(self.fail(SF_FROZEN));
        }
        if no == 0 || no > ARTIFACTS {
            return Err(self.fail(SF_NO));
        }
        let r = match self.arts.get_mut(no - 1) {
            Some(a) => a.write_off(witness),
            None => Err(SF_NO),
        };
        match r {
            Ok(()) => Ok(()),
            Err(e) => Err(self.fail(e)),
        }
    }

    /// 开注（注序须紧接游标）。
    pub fn open_note(&mut self, no: usize, artifact: usize, note_id: u32, lines: u32) -> Result<(), u32> {
        if self.frozen {
            return Err(self.fail(SF_FROZEN));
        }
        if no == 0 || no > NOTES {
            return Err(self.fail(SF_NO));
        }
        if no != self.note_cursor.wrapping_add(1) {
            return Err(self.fail(SF_ORDER));
        }
        // 引用件必须已封存且已核销——未核销件不得被移交。
        let off = match self.arts.get(artifact.wrapping_sub(1)) {
            Some(a) => a.off(),
            None => false,
        };
        if !off {
            return Err(self.fail(SF_REF));
        }
        let r = match self.notes.get_mut(no - 1) {
            Some(n) => n.open(artifact, note_id, lines),
            None => Err(SF_NO),
        };
        match r {
            Ok(()) => {
                self.note_cursor = no;
                Ok(())
            }
            Err(e) => Err(self.fail(e)),
        }
    }

    /// 回执一条注记。
    pub fn ack_note(&mut self, no: usize) -> Result<(), u32> {
        if self.frozen {
            return Err(self.fail(SF_FROZEN));
        }
        if no == 0 || no > NOTES {
            return Err(self.fail(SF_NO));
        }
        let r = match self.notes.get_mut(no - 1) {
            Some(n) => n.ack(),
            None => Err(SF_NO),
        };
        match r {
            Ok(()) => Ok(()),
            Err(e) => Err(self.fail(e)),
        }
    }
}

impl SealLedger {
    /// 已封存件数。
    pub fn sealed_count(&self) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < ARTIFACTS {
            if let Some(a) = self.arts.get(i) {
                if a.sealed() {
                    n = n.wrapping_add(1);
                }
            }
            i = i.wrapping_add(1);
        }
        n
    }

    /// 已核销件数。
    pub fn off_count(&self) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < ARTIFACTS {
            if let Some(a) = self.arts.get(i) {
                if a.off() {
                    n = n.wrapping_add(1);
                }
            }
            i = i.wrapping_add(1);
        }
        n
    }

    /// 悬空注记数。
    pub fn dangling(&self) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < NOTES {
            if let Some(x) = self.notes.get(i) {
                if x.dangling() {
                    n = n.wrapping_add(1);
                }
            }
            i = i.wrapping_add(1);
        }
        n
    }

    /// 已回执注记数。
    pub fn acked_notes(&self) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < NOTES {
            if let Some(x) = self.notes.get(i) {
                if x.acked() {
                    n = n.wrapping_add(1);
                }
            }
            i = i.wrapping_add(1);
        }
        n
    }

    /// 承接行数合计。
    pub fn carry_sum(&self) -> u32 {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < NOTES {
            if let Some(x) = self.notes.get(i) {
                if x.opened() {
                    n = n.wrapping_add(x.lines);
                }
            }
            i = i.wrapping_add(1);
        }
        n
    }

    /// 注记号是否互异。
    pub fn note_ids_unique(&self) -> bool {
        let mut i = 0usize;
        while i < NOTES {
            let a = match self.notes.get(i) {
                Some(x) => *x,
                None => return false,
            };
            if !a.opened() {
                i = i.wrapping_add(1);
                continue;
            }
            let mut j = i.wrapping_add(1);
            while j < NOTES {
                if let Some(b) = self.notes.get(j) {
                    if b.opened() && b.note_id == a.note_id {
                        return false;
                    }
                }
                j = j.wrapping_add(1);
            }
            i = i.wrapping_add(1);
        }
        true
    }

    /// 见证号是否互异。
    pub fn witnesses_unique(&self) -> bool {
        let mut i = 0usize;
        while i < ARTIFACTS {
            let a = match self.arts.get(i) {
                Some(x) => *x,
                None => return false,
            };
            if !a.off() {
                i = i.wrapping_add(1);
                continue;
            }
            let mut j = i.wrapping_add(1);
            while j < ARTIFACTS {
                if let Some(b) = self.arts.get(j) {
                    if b.off() && b.witness() == a.witness() {
                        return false;
                    }
                }
                j = j.wrapping_add(1);
            }
            i = i.wrapping_add(1);
        }
        true
    }

    /// 拒止计数。
    pub fn rejects(&self) -> u32 {
        self.rejects
    }

    /// 是否封盘。
    pub fn frozen(&self) -> bool {
        self.frozen
    }

    /// 总账指纹。
    pub fn fold(&self) -> u64 {
        let mut h = FNV_OFF;
        for b in self.base.to_le_bytes() {
            h ^= b as u64;
            h = h.wrapping_mul(FNV_PRIME);
        }
        h ^= if self.frozen { 1 } else { 0 };
        h = h.wrapping_mul(FNV_PRIME);
        h ^= self.rejects as u64;
        h = h.wrapping_mul(FNV_PRIME);
        let mut i = 0usize;
        while i < ARTIFACTS {
            if let Some(a) = self.arts.get(i) {
                h ^= a.digest();
                h = h.wrapping_mul(FNV_PRIME);
            }
            i = i.wrapping_add(1);
        }
        i = 0usize;
        while i < NOTES {
            if let Some(n) = self.notes.get(i) {
                h ^= n.digest();
                h = h.wrapping_mul(FNV_PRIME);
            }
            i = i.wrapping_add(1);
        }
        h
    }

    /// 封盘，幂等。
    pub fn freeze(&mut self) -> Result<u64, u32> {
        if self.frozen {
            return Ok(self.fold());
        }
        self.frozen = true;
        Ok(self.fold())
    }

    /// 终重放：三件逐件核销、六注零悬空、承接行数对平、号不重，然后封盘。
    pub fn replay(&mut self) -> Result<u64, u32> {
        if self.sealed_count() != ARTIFACTS || self.off_count() != ARTIFACTS {
            return Err(self.fail(SF_SHORT));
        }
        if self.dangling() != 0 {
            return Err(self.fail(SF_DANGLING));
        }
        if self.carry_sum() != LINES_TOTAL {
            return Err(self.fail(SF_SUM));
        }
        if !self.note_ids_unique() || !self.witnesses_unique() {
            return Err(self.fail(SF_DUP));
        }
        self.freeze()
    }
}

/// 搭建封存账：三件封存核销、六注开注回执、终重放。
pub fn build_seal(base: u32) -> (SealLedger, Result<u64, u32>) {
    let mut g = SealLedger::new(base);
    let mut no = 1usize;
    while no <= ARTIFACTS {
        let d = artifact_digest(base, no);
        if g.declare(no, d).is_err() {
            return (g, Err(SF_SHORT));
        }
        if g.recompute(no, d).is_err() {
            return (g, Err(SF_MISMATCH));
        }
        if g.seal(no).is_err() {
            return (g, Err(SF_MISMATCH));
        }
        if g.write_off(no, WITNESS_BASE.wrapping_add(no as u32)).is_err() {
            return (g, Err(SF_WITNESS));
        }
        no = no.wrapping_add(1);
    }
    let mut k = 1usize;
    while k <= NOTES {
        let art = match NOTE_ART.get(k.wrapping_sub(1)) {
            Some(v) => *v,
            None => 1,
        };
        let lines = match NOTE_LINES.get(k.wrapping_sub(1)) {
            Some(v) => *v,
            None => 0,
        };
        let nid = NOTE_ID_BASE.wrapping_add(k as u32);
        if g.open_note(k, art, nid, lines).is_err() {
            return (g, Err(SF_REF));
        }
        if g.ack_note(k).is_err() {
            return (g, Err(SF_DANGLING));
        }
        k = k.wrapping_add(1);
    }
    let out = g.replay();
    (g, out)
}

/// 四路注入：复算不符、未封存即核销、注记引用未核销件、核销缺见证。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];

    // ① 复算摘要被改一位，封存即拒。
    let mut a = SealLedger::new(0x3a00);
    let d = artifact_digest(0x3a00, 1);
    let ok1 = a.declare(1, d).is_ok() && a.recompute(1, d ^ 1).is_ok();
    out[0] = ok1 && a.seal(1).err() == Some(SF_MISMATCH) && a.rejects() == 1;

    // ② 只复算不封存就核销。
    let mut b = SealLedger::new(0x3a01);
    let d2 = artifact_digest(0x3a01, 1);
    let ok2 = b.declare(1, d2).is_ok() && b.recompute(1, d2).is_ok();
    out[1] = ok2 && b.write_off(1, WITNESS_BASE).err() == Some(SF_SHORT) && b.rejects() == 1;

    // ③ 件未核销，注记不得引用。
    let mut c = SealLedger::new(0x3a02);
    let d3 = artifact_digest(0x3a02, 1);
    let ok3 = c.declare(1, d3).is_ok() && c.recompute(1, d3).is_ok() && c.seal(1).is_ok();
    out[2] = ok3 && c.open_note(1, 1, NOTE_ID_BASE, 100).err() == Some(SF_REF) && c.rejects() == 1;

    // ④ 核销未见证号。
    let mut e = SealLedger::new(0x3a03);
    let d4 = artifact_digest(0x3a03, 2);
    let mut ok4 = e.declare(2, d4).is_ok() && e.recompute(2, d4).is_ok();
    ok4 = ok4 && e.seal(2).is_ok();
    out[3] = ok4 && e.write_off(2, 0).err() == Some(SF_WITNESS) && e.rejects() == 1;

    out
}

/// 判据集：E1~E13。
pub fn run_unx11138_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11138.seg1_seal_handover");
    let (g, r) = build_seal(0x1138);

    // E1 常量面。
    let mut lsum = 0u32;
    let mut i1 = 0usize;
    while i1 < NOTES {
        if let Some(v) = NOTE_LINES.get(i1) {
            lsum = lsum.wrapping_add(*v);
        }
        i1 = i1.wrapping_add(1);
    }
    let c1 = SEG_BASE == 0x1f260
        && ARTIFACTS == 3
        && NOTES == 6
        && LINES_TOTAL == 6400
        && lsum == LINES_TOTAL
        && ART_NAMES.len() == ARTIFACTS
        && NOTE_LINES.len() == NOTES
        && NOTE_ART.len() == NOTES;
    s.add("E1-常量面", c1, "三件六注，承接行数 6400 与段一对齐");

    // E2 三件逐件封存核销。
    s.add_dyn(
        "E2-三件核销",
        g.sealed_count() == ARTIFACTS && g.off_count() == ARTIFACTS,
        &format!(
            "sealed={}/{} off={}/{}",
            g.sealed_count(),
            ARTIFACTS,
            g.off_count(),
            ARTIFACTS
        ),
    );

    // E3 六注开齐、回执齐、零悬空。
    s.add_dyn(
        "E3-注账零悬空",
        g.acked_notes() == NOTES && g.dangling() == 0,
        &format!("acked={}/{} dangling={}", g.acked_notes(), NOTES, g.dangling()),
    );

    // E4 承接行数合计对平。
    s.add_dyn(
        "E4-承接对平",
        g.carry_sum() == LINES_TOTAL,
        &format!("carry={}/{}", g.carry_sum(), LINES_TOTAL),
    );

    // E5 注记号与见证号互异。
    s.add(
        "E5-号不重",
        g.note_ids_unique() && g.witnesses_unique(),
        "六注号互异、三见证号互异",
    );

    // E6 复算摘要逐件等于声明，且等于独立复算。
    let mut c6 = 0usize;
    let mut i6 = 1usize;
    while i6 <= ARTIFACTS {
        if let Some(a) = g.art(i6) {
            let want = artifact_digest(0x1138, i6);
            if a.declared_digest() == a.recomputed_digest() && a.recomputed_digest() == want {
                c6 = c6.wrapping_add(1);
            }
        }
        i6 = i6.wrapping_add(1);
    }
    s.add_dyn("E6-复算对平", c6 == ARTIFACTS, &format!("match={}/{}", c6, ARTIFACTS));

    // E7 见证号逐件非零。
    let mut c7 = 0usize;
    let mut i7 = 1usize;
    while i7 <= ARTIFACTS {
        if let Some(a) = g.art(i7) {
            if a.off() && witness_ok(a.witness()) {
                c7 = c7.wrapping_add(1);
            }
        }
        i7 = i7.wrapping_add(1);
    }
    s.add_dyn("E7-见证齐", c7 == ARTIFACTS, &format!("witness={}/{}", c7, ARTIFACTS));

    // E8 终重放成立并封盘。
    s.add("E8-封存封盘", r.is_ok() && g.frozen(), "终重放通过并封盘");

    // E9 重放与封盘幂等。
    let mut a9 = g.clone();
    let before = a9.fold();
    let again = a9.replay();
    let mut b9 = SealLedger::new(0x1138);
    let f1 = b9.freeze();
    let f2 = b9.freeze();
    s.add(
        "E9-幂等",
        again.is_ok() && a9.fold() == before && f1.is_ok() && f1 == f2,
        "重放二次同指纹；封盘二次同指纹",
    );

    // E10 拒止优先序：封盘先于件号。
    let mut c10 = g.clone();
    let z1 = c10.declare(ARTIFACTS + 1, 7).err();
    let mut c10b = SealLedger::new(0x1139);
    let z2 = c10b.declare(ARTIFACTS + 1, 7).err();
    s.add(
        "E10-拒止优先序",
        z1 == Some(SF_FROZEN) && z2 == Some(SF_NO),
        "封盘优先于件号；未封盘则 SF_NO",
    );

    // E11 四路注入。
    let inj = inject_four();
    s.add(
        "E11-四路注入",
        inj[0] && inj[1] && inj[2] && inj[3],
        "复算不符/未封即销/引用未销/缺见证四路各自发火",
    );

    // E12 指纹灵敏度。
    let (g2, _) = build_seal(0x113a);
    let (g3, _) = build_seal(0x1138);
    s.add(
        "E12-指纹灵敏度",
        g2.fold() != g.fold() && g3.fold() == g.fold(),
        "异基址相异、同基址相同",
    );

    // E13 逐例归零。
    s.add(
        "E13-逐例归零",
        g.rejects() == 0
            && g.off_count() == ARTIFACTS
            && g.dangling() == 0
            && g.carry_sum() == LINES_TOTAL,
        "rejects=0，三件核销、零悬空、行数对平",
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f11138_flow() {
        let (g, r) = build_seal(0x7000);
        assert!(r.is_ok());
        assert!(g.frozen());
        assert_eq!(g.sealed_count(), ARTIFACTS);
        assert_eq!(g.off_count(), ARTIFACTS);
        assert_eq!(g.acked_notes(), NOTES);
        assert_eq!(g.dangling(), 0);
        assert_eq!(g.carry_sum(), LINES_TOTAL);
        assert_eq!(g.rejects(), 0);
        assert!(g.note_ids_unique());
        assert!(g.witnesses_unique());
        // 三件摘要逐件可复算。
        let mut no = 1usize;
        while no <= ARTIFACTS {
            let want = artifact_digest(0x7000, no);
            let a = match g.art(no) {
                Some(a) => a,
                None => return,
            };
            assert!(a.off());
            assert_eq!(a.declared_digest(), want);
            assert_eq!(a.recomputed_digest(), want);
            no += 1;
        }
    }

    #[test]
    fn f11138_gates() {
        let mut g = SealLedger::new(0x7001);
        let d = artifact_digest(0x7001, 1);
        assert_eq!(g.write_off(1, 5).err(), Some(SF_SHORT));
        assert!(g.declare(1, d).is_ok());
        assert!(g.recompute(1, d ^ 2).is_ok());
        assert_eq!(g.seal(1).err(), Some(SF_MISMATCH));
        assert_eq!(g.declare(1, d).err(), Some(SF_DUP));
        assert_eq!(g.recompute(1, d).err(), Some(SF_DUP));
        assert_eq!(g.write_off(1, 9).err(), Some(SF_SHORT));
        let d2 = artifact_digest(0x7001, 2);
        assert!(g.declare(2, d2).is_ok());
        assert!(g.recompute(2, d2).is_ok());
        assert!(g.seal(2).is_ok());
        assert_eq!(g.write_off(2, 0).err(), Some(SF_WITNESS));
        assert_eq!(g.open_note(2, 2, NOTE_ID_BASE, 100).err(), Some(SF_ORDER));
        assert_eq!(g.open_note(1, 1, NOTE_ID_BASE, 100).err(), Some(SF_REF));
        assert_eq!(g.ack_note(1).err(), Some(SF_SHORT));
        assert_eq!(g.rejects(), 9);
        assert!(g.freeze().is_ok());
        assert_eq!(g.declare(3, 7).err(), Some(SF_FROZEN));
        assert_eq!(g.rejects(), 10);
    }

    #[test]
    fn f11138_rules() {
        let d = artifact_digest(0x7002, 1);
        assert_eq!(d, artifact_digest(0x7002, 1));
        assert_ne!(d, artifact_digest(0x7002, 2));
        assert_ne!(d, artifact_digest(0x7003, 1));
        assert!(witness_ok(1));
        assert!(!witness_ok(0));
        assert_eq!(art_name(1), "账册");
        assert_eq!(art_name(3), "摘要");
        assert_eq!(art_name(0), "越界");
        assert_eq!(art_name(4), "越界");
        // 注记状态机：未开注即回执拒，开而未回执为悬空。
        let mut h = Handover::new(1);
        assert_eq!(h.ack().err(), Some(SF_SHORT));
        assert_eq!(h.open(0, NOTE_ID_BASE, 10).err(), Some(SF_NO));
        assert_eq!(h.open(1, 0, 10).err(), Some(SF_WITNESS));
        assert_eq!(h.open(1, NOTE_ID_BASE, 0).err(), Some(SF_SUM));
        assert!(h.open(1, NOTE_ID_BASE, 10).is_ok());
        assert!(h.dangling());
        assert!(h.ack().is_ok());
        assert!(!h.dangling());
        assert_eq!(h.ack().err(), Some(SF_DUP));
        // 件状态机：未立声明即复算。
        let mut a = Artifact::new(1);
        assert_eq!(a.recompute(1).err(), Some(SF_SHORT));
        assert_eq!(a.seal().err(), Some(SF_SHORT));
    }

    #[test]
    fn f11138_injection() {
        assert_eq!(inject_four(), [true, true, true, true]);
    }

    #[test]
    fn f11138_checks_green() {
        let cs = run_unx11138_checks();
        assert!(cs.all_passed(), "checks red in unx11138");
    }
}
