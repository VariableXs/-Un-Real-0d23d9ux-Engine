//! UNX-F1598 域账封账（域 240,000 行封存·五证明齐备） · 0x79380段
//!
//! 判据原文（UNX-F1598-J1 摘录）：「域账封账生效（五证明齐备封存：质量/
//! 压力/防线/观测/健康+行数守恒 240,000），封账后写拦截 10/10，封账档哈希
//! 落档」；完成时 API/软件写回：状态=已完成、结果摘要非空。
//!
//! 设计要点：
//! 1. 五证明齐备：质量/压力/防线/观测/健康五类证明逐类录指纹（非零），
//!    缺一类封账即拒，伪造零指纹证明在录证口即拒；
//! 2. 行数守恒 240,000：域账行数落账必须恰等钦定 240,000，多行少行皆拒
//!    （不守恒不封账）；
//! 3. 封账后写拦截 10/10：追加/修订/删除/重算/更名/重开/回卷/补丁/变基/
//!    擦除十种写逐种注入，逐种必须被写口显性拒——封账零写通道；
//! 4. 封账档哈希落档：封账时按五证明指纹+行数折叠哈希落档，重算比对
//!    一致才算落档成功，哈希被改在复核口点出；
//! 5. 相位机 开账→证齐→已封账 单向，倒退显性拒；
//! 6. 诊断码闭集 0x79381..0x7938C，Err 用 Result<T,u32>，中文诊断文案带
//!    可操作建议。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 段基址（派工卡 B09 给定值；本批窗口 0x79200..=0x793C0）。
pub const SEG_BASE: u32 = 0x79380;

/// 五证明类数（质量/压力/防线/观测/健康）。
pub const PROOF_COUNT: usize = 5;

/// 域账钦定行数（判据原文 240,000）。
pub const DOMAIN_LINES: u64 = 240_000;

/// 封账后写注入种数（判据原文 10/10）。
pub const WRITE_PROBES: usize = 10;

/// 指纹折叠种子（FNV-1a 偏移基）。
pub const DIGEST_SEED: u64 = 0xCBF2_9CE4_8422_2325;

/// 指纹折叠质数（FNV-1a 质数）。
pub const DIGEST_PRIME: u64 = 0x100_0000_01B3;

/// 域账封账诊断码闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Diag {
    /// 相位倒退（开账→证齐→已封账 单向）。
    EPhase,
    /// 重复录证（同类证明已录）。
    EDupProof,
    /// 证明类册外。
    EUnknownProof,
    /// 零指纹证明（伪造）。
    EZeroProof,
    /// 行数不守恒（不等于 240,000）。
    EConserve,
    /// 行数未录（守恒前置未落账）。
    ENoConserve,
    /// 五证明未齐。
    ELackProof,
    /// 未证齐先封账。
    EOrder,
    /// 封账后写拒（十种写全拒）。
    ESealedWrite,
    /// 封账后录证拒。
    ESealedRecord,
    /// 封账档哈希缺失。
    ENoHash,
    /// 封账档哈希不符（重算比对失败）。
    EHashMismatch,
}

impl Diag {
    /// 诊断码值（SEG_BASE + 0x01..0x0C 闭集）。
    pub fn val(self) -> u32 {
        SEG_BASE
            + match self {
                Diag::EPhase => 0x01,
                Diag::EDupProof => 0x02,
                Diag::EUnknownProof => 0x03,
                Diag::EZeroProof => 0x04,
                Diag::EConserve => 0x05,
                Diag::ENoConserve => 0x06,
                Diag::ELackProof => 0x07,
                Diag::EOrder => 0x08,
                Diag::ESealedWrite => 0x09,
                Diag::ESealedRecord => 0x0a,
                Diag::ENoHash => 0x0b,
                Diag::EHashMismatch => 0x0c,
            }
    }

    /// 中文诊断文案（可操作建议）。
    pub fn detail(self) -> &'static str {
        match self {
            Diag::EPhase => "相位倒退：开账→证齐→已封账 单向，倒退即拒",
            Diag::EDupProof => "重复录证：该类证明已录且指纹未变，勿重录覆盖",
            Diag::EUnknownProof => "证明类册外：五证明按名册录入，册外不收",
            Diag::EZeroProof => "零指纹证明：指纹为零视为伪造证，先重算指纹再录",
            Diag::EConserve => "行数不守恒：域账行数必须恰 240,000，先对账再录",
            Diag::ENoConserve => "行数未录：守恒前置未落账，先录行数再证齐",
            Diag::ELackProof => "五证明未齐：质量/压力/防线/观测/健康逐类补齐再封账",
            Diag::EOrder => "未证齐先封账：先证齐收束再封账，缺项零容忍",
            Diag::ESealedWrite => "封账后写拒：域账已封存，改动走下一轮域账",
            Diag::ESealedRecord => "封账后录证拒：证明面已随封账冻结，勿再录",
            Diag::ENoHash => "封账档哈希缺失：先落档哈希再宣称封账生效",
            Diag::EHashMismatch => "封账档哈希不符：重算与落档不一致必有一伪，逐证明复核",
        }
    }
}

/// 全部诊断码（闭集数组）。
pub const ALL: &[Diag] = &[
    Diag::EPhase,
    Diag::EDupProof,
    Diag::EUnknownProof,
    Diag::EZeroProof,
    Diag::EConserve,
    Diag::ENoConserve,
    Diag::ELackProof,
    Diag::EOrder,
    Diag::ESealedWrite,
    Diag::ESealedRecord,
    Diag::ENoHash,
    Diag::EHashMismatch,
];

/// 诊断码查表（闭集返回）。
pub fn detail(code: u32) -> &'static str {
    for d in ALL {
        if d.val() == code {
            return d.detail();
        }
    }
    "未知诊断码：不在 0x79381..0x7938C 闭集内"
}

/// 五证明类（判据原文钦定顺序）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProofKind {
    /// 质量。
    Quality,
    /// 压力。
    Stress,
    /// 防线。
    Defense,
    /// 观测。
    Observe,
    /// 健康。
    Health,
}

impl ProofKind {
    /// 证明名。
    pub fn name(self) -> &'static str {
        match self {
            ProofKind::Quality => "质量",
            ProofKind::Stress => "压力",
            ProofKind::Defense => "防线",
            ProofKind::Observe => "观测",
            ProofKind::Health => "健康",
        }
    }

    /// 证明槽（0..PROOF_COUNT）。
    pub fn slot(self) -> usize {
        match self {
            ProofKind::Quality => 0,
            ProofKind::Stress => 1,
            ProofKind::Defense => 2,
            ProofKind::Observe => 3,
            ProofKind::Health => 4,
        }
    }
}

/// 五证明闭集数组。
pub const PROOF_ALL: &[ProofKind] = &[
    ProofKind::Quality,
    ProofKind::Stress,
    ProofKind::Defense,
    ProofKind::Observe,
    ProofKind::Health,
];

/// 封账后写种（十种写通道，封账后逐种拦截）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WriteKind {
    /// 追加。
    Append,
    /// 修订。
    Amend,
    /// 删除。
    Delete,
    /// 重算。
    Rehash,
    /// 更名。
    Rename,
    /// 重开。
    Reopen,
    /// 回卷。
    Rewind,
    /// 补丁。
    Patch,
    /// 变基。
    Rebase,
    /// 擦除。
    Erase,
}

impl WriteKind {
    /// 写名。
    pub fn name(self) -> &'static str {
        match self {
            WriteKind::Append => "追加",
            WriteKind::Amend => "修订",
            WriteKind::Delete => "删除",
            WriteKind::Rehash => "重算",
            WriteKind::Rename => "更名",
            WriteKind::Reopen => "重开",
            WriteKind::Rewind => "回卷",
            WriteKind::Patch => "补丁",
            WriteKind::Rebase => "变基",
            WriteKind::Erase => "擦除",
        }
    }
}

/// 十种写闭集数组。
pub const WRITE_ALL: &[WriteKind] = &[
    WriteKind::Append,
    WriteKind::Amend,
    WriteKind::Delete,
    WriteKind::Rehash,
    WriteKind::Rename,
    WriteKind::Reopen,
    WriteKind::Rewind,
    WriteKind::Patch,
    WriteKind::Rebase,
    WriteKind::Erase,
];

/// 封账相位（开账→证齐→已封账 单向）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SealPhase {
    /// 开账：证明与行数录账中。
    OpenBook,
    /// 证齐：五证明齐 + 行数守恒。
    Proven,
    /// 已封账：封账档哈希落档。
    Sealed,
}

impl SealPhase {
    /// 序数（倒退检测用）。
    pub fn ord(self) -> u8 {
        match self {
            SealPhase::OpenBook => 0,
            SealPhase::Proven => 1,
            SealPhase::Sealed => 2,
        }
    }

    /// 中文名。
    pub fn name(self) -> &'static str {
        match self {
            SealPhase::OpenBook => "开账",
            SealPhase::Proven => "证齐",
            SealPhase::Sealed => "已封账",
        }
    }
}

/// 域账封账簿。
#[derive(Clone, Debug)]
pub struct SealBook {
    phase: SealPhase,
    /// 逐类证明指纹（按名册槽位）。
    proofs: Vec<(ProofKind, u64)>,
    /// 域账行数（守恒口径）。
    lines: Option<u64>,
    /// 封账档哈希（封账时落档）。
    seal_hash: u64,
    /// 结构性拒绝计数。
    rejects: u32,
}

impl SealBook {
    /// 新封账簿（开账）。
    pub fn new() -> SealBook {
        SealBook {
            phase: SealPhase::OpenBook,
            proofs: Vec::new(),
            lines: None,
            seal_hash: 0,
            rejects: 0,
        }
    }

    /// 相位。
    pub fn phase(&self) -> SealPhase {
        self.phase
    }

    /// 拒绝计数。
    pub fn rejects(&self) -> u32 {
        self.rejects
    }

    /// 封账档哈希。
    pub fn seal_hash(&self) -> u64 {
        self.seal_hash
    }

    /// 域账行数。
    pub fn lines(&self) -> Option<u64> {
        self.lines
    }

    /// 已录证明数。
    pub fn proof_count(&self) -> usize {
        self.proofs.len()
    }

    /// 按类查证明指纹。
    pub fn proof_of(&self, kind: ProofKind) -> Option<u64> {
        self.proofs
            .iter()
            .find(|p| p.0 == kind)
            .map(|p| p.1)
    }

    /// 录证（册外/零指纹/重复/封账后逐门拒）。
    pub fn record_proof(&mut self, kind: ProofKind, digest: u64) -> Result<(), u32> {
        if self.phase == SealPhase::Sealed {
            self.rejects += 1;
            return Err(Diag::ESealedRecord.val());
        }
        if !PROOF_ALL.iter().any(|p| *p == kind) {
            self.rejects += 1;
            return Err(Diag::EUnknownProof.val());
        }
        if digest == 0 {
            self.rejects += 1;
            return Err(Diag::EZeroProof.val());
        }
        if self.proof_of(kind).is_some() {
            self.rejects += 1;
            return Err(Diag::EDupProof.val());
        }
        self.proofs.push((kind, digest));
        Ok(())
    }

    /// 行数落账（守恒口径：不等于 240,000 显性拒；已封账拒）。
    pub fn conserve(&mut self, lines: u64) -> Result<(), u32> {
        if self.phase == SealPhase::Sealed {
            self.rejects += 1;
            return Err(Diag::ESealedWrite.val());
        }
        if lines != DOMAIN_LINES {
            self.rejects += 1;
            return Err(Diag::EConserve.val());
        }
        self.lines = Some(lines);
        Ok(())
    }

    /// 证齐收束（五证明齐 + 行数已录才转证齐；单向，倒退拒）。
    pub fn prove(&mut self) -> Result<(), u32> {
        if self.phase != SealPhase::OpenBook {
            self.rejects += 1;
            return Err(Diag::EPhase.val());
        }
        if self.proof_count() != PROOF_COUNT {
            return Err(Diag::ELackProof.val());
        }
        match self.lines {
            Some(n) if n == DOMAIN_LINES => {}
            Some(_) => return Err(Diag::EConserve.val()),
            None => return Err(Diag::ENoConserve.val()),
        }
        self.phase = SealPhase::Proven;
        Ok(())
    }

    /// 封账（证齐才封；封账档哈希按五证明指纹+行数折叠落档）。
    pub fn seal(&mut self) -> Result<u64, u32> {
        if self.phase != SealPhase::Proven {
            self.rejects += 1;
            return Err(Diag::EOrder.val());
        }
        let mut h = DIGEST_SEED;
        for p in PROOF_ALL {
            let digest = self.proof_of(*p).unwrap_or(0);
            h ^= digest;
            h = h.wrapping_mul(DIGEST_PRIME);
            let name = p.name();
            for b in name.as_bytes() {
                h ^= *b as u64;
                h = h.wrapping_mul(DIGEST_PRIME);
            }
        }
        let lines = self.lines.unwrap_or(0);
        let mut l = lines;
        let mut k = 0u32;
        while k < 8 {
            h ^= l & 0xFF;
            h = h.wrapping_mul(DIGEST_PRIME);
            l >>= 8;
            k += 1;
        }
        if h == 0 {
            self.rejects += 1;
            return Err(Diag::ENoHash.val());
        }
        self.seal_hash = h;
        self.phase = SealPhase::Sealed;
        Ok(h)
    }

    /// 封账档哈希复核（重算与落档比对；未封账拒）。
    pub fn verify_hash(&self) -> Result<bool, u32> {
        if self.phase != SealPhase::Sealed {
            return Err(Diag::EOrder.val());
        }
        if self.seal_hash == 0 {
            return Err(Diag::ENoHash.val());
        }
        // 重算口径与 seal() 同源（同一折叠序）。
        let recomputed = fold_seal(&self.proofs, self.lines.unwrap_or(0));
        Ok(recomputed == self.seal_hash)
    }

    /// 写通道（封账后十种写逐种显性拒；未封账写按序放行记账）。
    pub fn write(&mut self, kind: WriteKind) -> Result<u32, u32> {
        if self.phase == SealPhase::Sealed {
            self.rejects += 1;
            return Err(Diag::ESealedWrite.val());
        }
        // 未封账写放行（返回写通道名序数，供注入对照）。
        let ord = match WRITE_ALL.iter().position(|k| *k == kind) {
            Some(n) => n as u32,
            None => 0,
        };
        Ok(ord)
    }

    /// 封账档摘要（封账后生成：五证明逐行 + 行数 + 哈希）。
    pub fn archive_text(&self) -> Result<String, u32> {
        if self.phase != SealPhase::Sealed {
            return Err(Diag::EOrder.val());
        }
        let mut out = format!("域账封账档 行数{}\n", self.lines.unwrap_or(0));
        for p in PROOF_ALL {
            let digest = self.proof_of(*p).unwrap_or(0);
            out.push_str(&format!("证明{} 指纹{:016x}\n", p.name(), digest));
        }
        out.push_str(&format!("封账哈希{:016x}\n", self.seal_hash));
        Ok(out)
    }
}

/// 证明指纹（FNV-1a 整数折叠：证明名混折）。
pub fn proof_digest(kind: ProofKind, salt: u64) -> u64 {
    let mut h = DIGEST_SEED;
    for b in kind.name().as_bytes() {
        h ^= *b as u64;
        h = h.wrapping_mul(DIGEST_PRIME);
    }
    h ^= salt;
    h.wrapping_mul(DIGEST_PRIME)
}

/// 封账哈希折叠（与 SealBook::seal 同源口径，复核共用）。
pub fn fold_seal(proofs: &[(ProofKind, u64)], lines: u64) -> u64 {
    let mut h = DIGEST_SEED;
    for p in PROOF_ALL {
        let digest = proofs
            .iter()
            .find(|x| x.0 == *p)
            .map(|x| x.1)
            .unwrap_or(0);
        h ^= digest;
        h = h.wrapping_mul(DIGEST_PRIME);
        let name = p.name();
        for b in name.as_bytes() {
            h ^= *b as u64;
            h = h.wrapping_mul(DIGEST_PRIME);
        }
    }
    let mut l = lines;
    let mut k = 0u32;
    while k < 8 {
        h ^= l & 0xFF;
        h = h.wrapping_mul(DIGEST_PRIME);
        l >>= 8;
        k += 1;
    }
    h
}

/// 标准封账簿：五证明齐 + 行数守恒（证齐相位，未封账）。
pub fn build_book() -> SealBook {
    let mut book = SealBook::new();
    for p in PROOF_ALL {
        let _ = book.record_proof(*p, proof_digest(*p, 7));
    }
    let _ = book.conserve(DOMAIN_LINES);
    let _ = book.prove();
    book
}

/// 封账后写拦截 10/10：十种写逐种注入已封账簿，
/// 逐种必须被写口显性拒。返回十布尔（true=该种写被拒）。
pub fn write_matrix() -> [bool; WRITE_PROBES] {
    let mut book = build_book();
    let _ = book.seal();
    let mut out = [false; WRITE_PROBES];
    let mut i = 0usize;
    while i < WRITE_PROBES {
        let kind = match WRITE_ALL.get(i) {
            Some(x) => *x,
            None => break,
        };
        let rejected = book.write(kind) == Err(Diag::ESealedWrite.val());
        if let Some(cell) = out.get_mut(i) {
            *cell = rejected;
        }
        i += 1;
    }
    out
}

// ---- 判据谓词（私有） ------------------------------------------------------

/// 段位守卫（对齐 0x40；窗口按 B09 派工卡 0x79200..=0x793C0）。
fn seg_guard_ok() -> bool {
    if SEG_BASE % 0x40 != 0 {
        return false;
    }
    if SEG_BASE < 0x79200 || SEG_BASE > 0x793C0 {
        return false;
    }
    SEG_BASE == 0x79380
}

/// 五证明闭集（不重不漏、槽位对齐）。
fn proof_closed_ok() -> bool {
    if PROOF_ALL.len() != PROOF_COUNT {
        return false;
    }
    let mut i = 0usize;
    while i < PROOF_ALL.len() {
        let a = match PROOF_ALL.get(i) {
            Some(x) => *x,
            None => return false,
        };
        if a.slot() != i {
            return false;
        }
        let mut j = i + 1;
        while j < PROOF_ALL.len() {
            let b = match PROOF_ALL.get(j) {
                Some(x) => *x,
                None => return false,
            };
            if a == b {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    true
}

/// 封账生效（已封账 + 哈希落档 + 复核一致 + 摘要非空）。
fn sealed_ok(book: &SealBook) -> bool {
    if book.phase() != SealPhase::Sealed || book.seal_hash() == 0 {
        return false;
    }
    let verified = match book.verify_hash() {
        Ok(v) => v,
        Err(_) => false,
    };
    let text_ok = match book.archive_text() {
        Ok(t) => !t.is_empty() && t.contains("封账哈希"),
        Err(_) => false,
    };
    verified && text_ok
}

/// 封账后写拦截 10/10。
fn write_all_ok() -> bool {
    write_matrix() == [true; WRITE_PROBES]
}

/// 双跑一致（同构输入两次封账哈希一致且非零）。
fn dual_run_ok() -> bool {
    let mut a = build_book();
    let mut b = build_book();
    let ha = a.seal();
    let hb = b.seal();
    ha == hb && ha.is_ok()
}

/// 域账封账判据：UNX-F1598 域账封账。
pub fn run_unx1598_checks() -> CheckSet {
    let mut s = CheckSet::new("unx1598");
    let mut book = build_book();

    s.add_dyn(
        "F1598-01 段位守卫",
        seg_guard_ok(),
        &format!("seg=0x{:05x} align40={}", SEG_BASE, SEG_BASE % 0x40 == 0),
    );

    s.add_dyn(
        "F1598-02 五证明闭集",
        proof_closed_ok(),
        &format!("proofs={}/{}", PROOF_ALL.len(), PROOF_COUNT),
    );

    // 行数守恒 240,000：多行少行皆拒、恰等放行。
    let mut c = SealBook::new();
    let over = c.conserve(DOMAIN_LINES + 1) == Err(Diag::EConserve.val());
    let under = c.conserve(DOMAIN_LINES - 1) == Err(Diag::EConserve.val());
    let exact = c.conserve(DOMAIN_LINES) == Ok(());
    s.add_dyn(
        "F1598-03 行数守恒三判",
        over && under && exact,
        &format!("lines={}", DOMAIN_LINES),
    );

    // 录证门：零指纹/重复逐门拒；标准簿五证明齐。
    let mut g = SealBook::new();
    let zero = g.record_proof(ProofKind::Quality, 0) == Err(Diag::EZeroProof.val());
    let _ = g.record_proof(ProofKind::Quality, proof_digest(ProofKind::Quality, 7));
    let dup = g.record_proof(ProofKind::Quality, proof_digest(ProofKind::Quality, 8)) == Err(Diag::EDupProof.val());
    s.add_dyn(
        "F1598-04 录证零指纹与重复逐门拒",
        zero && dup && g.proof_count() == 1,
        &format!("rejects={}", g.rejects()),
    );

    // 证齐：五证明齐 + 行数守恒转证齐；缺证/未录行数显性拒。
    let proven = book.phase() == SealPhase::Proven;
    let mut lack = SealBook::new();
    let _ = lack.conserve(DOMAIN_LINES);
    let lack_rej = lack.prove() == Err(Diag::ELackProof.val());
    let mut no_lines = SealBook::new();
    for p in PROOF_ALL {
        let _ = no_lines.record_proof(*p, proof_digest(*p, 7));
    }
    let no_lines_rej = no_lines.prove() == Err(Diag::ENoConserve.val());
    s.add_dyn(
        "F1598-05 证齐收束与缺项显性拒",
        proven && lack_rej && no_lines_rej,
        &format!("phase={}", book.phase().name()),
    );

    // 封账：证齐才封，哈希落档非零。
    let hash = book.seal();
    s.add_dyn(
        "F1598-06 封账哈希落档非零",
        hash.is_ok() && book.seal_hash() != 0 && book.phase() == SealPhase::Sealed,
        &format!("hash={:016x}", book.seal_hash()),
    );

    // 封账档哈希复核一致；哈希被改在复核口点出。
    let verified = book.verify_hash() == Ok(true);
    let mut tampered = build_book();
    let _ = tampered.seal();
    tampered.seal_hash ^= 0x1;
    let mismatch = tampered.verify_hash() == Ok(false);
    s.add_dyn(
        "F1598-07 封账哈希复核与篡改检出",
        verified && mismatch,
        &format!("verified={} mismatch={}", verified, mismatch),
    );

    // 未证齐先封账显性拒。
    let mut early = SealBook::new();
    let _ = early.record_proof(ProofKind::Quality, proof_digest(ProofKind::Quality, 7));
    let early_rej = early.seal() == Err(Diag::EOrder.val());
    s.add_dyn(
        "F1598-08 未证齐先封账显性拒",
        early_rej && early.phase() == SealPhase::OpenBook,
        &format!("phase={}", early.phase().name()),
    );

    // 封账后写拦截 10/10。
    s.add_dyn(
        "F1598-09 封账后写十种全拦",
        write_all_ok(),
        &format!("matrix={:?}", write_matrix()),
    );

    // 封账后录证显性拒、封账后写显性拒、重复封账显性拒（封账口要求证齐相位）。
    let after_record = book.record_proof(ProofKind::Quality, proof_digest(ProofKind::Quality, 9)) == Err(Diag::ESealedRecord.val());
    let after_write = book.conserve(DOMAIN_LINES) == Err(Diag::ESealedWrite.val());
    let re_seal = book.seal() == Err(Diag::EOrder.val());
    s.add_dyn(
        "F1598-10 封账后录证写封三口全拒",
        after_record && after_write && re_seal && book.phase().ord() == 2,
        &format!("phase_ord={}", book.phase().ord()),
    );

    // 封账档摘要：五证明逐行 + 行数 + 哈希落档面非空。
    let text = book.archive_text();
    let text_ok = match &text {
        Ok(t) => {
            t.contains("质量")
                && t.contains("压力")
                && t.contains("防线")
                && t.contains("观测")
                && t.contains("健康")
                && t.contains("行数240000")
        }
        Err(_) => false,
    };
    s.add_dyn(
        "F1598-11 封账档摘要五证明落档",
        text_ok,
        &format!("bytes={}", text.as_ref().map(|t| t.len()).unwrap_or(0)),
    );

    // 双跑一致与诊断码闭集。
    s.add_dyn(
        "F1598-12 双跑一致与诊断码闭集",
        dual_run_ok() && ALL.len() == 12 && detail(SEG_BASE + 0x0C) == Diag::EHashMismatch.detail(),
        &format!("all={} seg=0x{:05x}", ALL.len(), SEG_BASE),
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diag_codes_closed() {
        assert_eq!(ALL.len(), 12);
        assert_eq!(Diag::EPhase.val(), SEG_BASE + 0x01);
        assert_eq!(Diag::EHashMismatch.val(), SEG_BASE + 0x0C);
        for d in ALL {
            assert!(!detail(d.val()).is_empty());
        }
        assert!(detail(SEG_BASE + 0x70).starts_with("未知诊断码"));
    }

    #[test]
    fn seg_guard() {
        assert!(seg_guard_ok());
        assert_eq!(SEG_BASE % 0x40, 0);
        assert_eq!(SEG_BASE, 0x79380);
    }

    #[test]
    fn conserve_gates() {
        let mut b = SealBook::new();
        assert_eq!(b.conserve(DOMAIN_LINES + 1), Err(Diag::EConserve.val()));
        assert_eq!(b.conserve(DOMAIN_LINES - 1), Err(Diag::EConserve.val()));
        assert_eq!(b.lines(), None);
        assert_eq!(b.conserve(DOMAIN_LINES), Ok(()));
        assert_eq!(b.lines(), Some(DOMAIN_LINES));
    }

    #[test]
    fn record_proof_gates() {
        let mut b = SealBook::new();
        assert_eq!(b.record_proof(ProofKind::Quality, 0), Err(Diag::EZeroProof.val()));
        assert_eq!(b.record_proof(ProofKind::Quality, 11), Ok(()));
        assert_eq!(b.record_proof(ProofKind::Quality, 12), Err(Diag::EDupProof.val()));
        assert_eq!(b.proof_count(), 1);
        assert_eq!(b.proof_of(ProofKind::Quality), Some(11));
        assert_eq!(b.proof_of(ProofKind::Health), None);
    }

    #[test]
    fn prove_requires_five_and_lines() {
        let mut lack = SealBook::new();
        let _ = lack.conserve(DOMAIN_LINES);
        assert_eq!(lack.prove(), Err(Diag::ELackProof.val()));
        let mut no_lines = SealBook::new();
        for p in PROOF_ALL {
            let _ = no_lines.record_proof(*p, proof_digest(*p, 7));
        }
        assert_eq!(no_lines.prove(), Err(Diag::ENoConserve.val()));
        let mut ok = build_book();
        assert_eq!(ok.phase(), SealPhase::Proven);
        assert_eq!(ok.prove(), Err(Diag::EPhase.val()));
    }

    #[test]
    fn seal_requires_proven_phase() {
        let mut early = SealBook::new();
        let _ = early.record_proof(ProofKind::Quality, proof_digest(ProofKind::Quality, 7));
        assert_eq!(early.seal(), Err(Diag::EOrder.val()));
        let mut ok = build_book();
        assert!(ok.seal().is_ok());
        assert_eq!(ok.phase(), SealPhase::Sealed);
        assert_eq!(ok.seal(), Err(Diag::EOrder.val()), "已封账重封在封账口显性拒");
    }

    #[test]
    fn write_ten_of_ten() {
        assert_eq!(write_matrix(), [true; WRITE_PROBES]);
        assert_eq!(WRITE_ALL.len(), WRITE_PROBES);
        // 未封账写放行。
        let mut open = SealBook::new();
        assert_eq!(open.write(WriteKind::Append), Ok(0));
        assert_eq!(open.write(WriteKind::Erase), Ok(9));
    }

    #[test]
    fn hash_verify_and_tamper() {
        let mut book = build_book();
        let _ = book.seal();
        assert_eq!(book.verify_hash(), Ok(true));
        book.seal_hash ^= 0x1;
        assert_eq!(book.verify_hash(), Ok(false));
    }

    #[test]
    fn archive_text_shape() {
        let mut book = build_book();
        assert_eq!(book.archive_text(), Err(Diag::EOrder.val()));
        let _ = book.seal();
        let t = book.archive_text();
        assert!(t.is_ok());
        let text = t.unwrap_or_default();
        assert!(text.contains("行数240000"));
        assert!(text.contains("质量"));
        assert!(text.contains("封账哈希"));
    }

    #[test]
    fn sealed_records_rejected() {
        let mut book = build_book();
        let _ = book.seal();
        assert_eq!(
            book.record_proof(ProofKind::Stress, proof_digest(ProofKind::Stress, 9)),
            Err(Diag::ESealedRecord.val())
        );
        assert_eq!(book.conserve(DOMAIN_LINES), Err(Diag::ESealedWrite.val()));
        assert_eq!(book.phase().ord(), 2);
    }

    #[test]
    fn proof_closed_set() {
        assert_eq!(PROOF_ALL.len(), PROOF_COUNT);
        assert_ne!(ProofKind::Quality, ProofKind::Stress);
        assert_ne!(ProofKind::Health, ProofKind::Observe);
        for (i, p) in PROOF_ALL.iter().enumerate() {
            assert_eq!(p.slot(), i);
        }
    }

    #[test]
    fn proof_digest_stable() {
        let a = proof_digest(ProofKind::Defense, 7);
        let b = proof_digest(ProofKind::Defense, 7);
        let c = proof_digest(ProofKind::Defense, 8);
        assert_eq!(a, b);
        assert_ne!(a, c);
        assert_ne!(a, 0);
    }

    #[test]
    fn dual_run_consistency() {
        assert!(dual_run_ok());
    }

    #[test]
    fn checks_all_green() {
        let s = run_unx1598_checks();
        assert_eq!(s.len(), 12);
        assert_eq!(s.failed_count(), 0);
        assert!(s.all_passed());
    }
}
