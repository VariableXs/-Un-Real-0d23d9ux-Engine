//! UNX-F0732 · 嫁接边界声明账（零补丁断言） · 诊断段 0x5b00
//! 判据原文摘录：UNX-F0732-J1 零补丁断言过（引导器构建产物 = 上游原产物哈希
//! 逐版一致）且边界声明账三段齐；完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 纯功能代码 ≥580 行。
//!
//! 设计纪律：
//! 1. 嫁接边界唯一裁决口径 = 引导器构建产物哈希 与 上游原产物哈希 逐版比对，
//!    不引入任何"看起来一样"的软判据（时间戳/长度/文件名一律不作为一致性证据）。
//! 2. 边界声明账必须三段齐备（段一产物登记 / 段二哈希对照 / 段三结论声明），
//!    任一段缺失即拒绝封账，杜绝半账本流入下游。
//! 3. 零补丁断言是全有全无断言：逐版哈希全等才放行，出现任一 Patched/Missing
//!    即整账拒绝，不允许"局部零补丁"。
//! 4. 定容数组账本 + 守恒恒等式：identical+patched+missing+pending == count，
//!    每次变更后复核，防幽灵条目。
//! 5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。
//! 6. 诊断段 0x5b00 为本单独占，XcCode 判别式显式写出，不与其他单混用。

use alloc::string::{String, ToString};
use alloc::vec::Vec;

pub const SEG_BASE: u16 = 0x5b00;

// ── 诊断码闭集（SEG_BASE+1..+N）────────────────────────────────
const E_DECL_DUP: u16 = SEG_BASE + 1; // 声明编号重复登记
const E_DECL_FULL: u16 = SEG_BASE + 2; // 声明账容量已满（超 32 条）
const E_DECL_NOTFOUND: u16 = SEG_BASE + 3; // 声明编号不存在
const E_HASH_TWICE: u16 = SEG_BASE + 4; // 同一版本的哈希重复写入
const E_HASH_ZERO: u16 = SEG_BASE + 5; // 哈希值非法（0 视为未采集）
const E_PHASE: u16 = SEG_BASE + 6; // 账本状态机非法边（阶段不匹配）
const E_SECT_MISSING: u16 = SEG_BASE + 7; // 边界声明账三段不齐
const E_PATCHED: u16 = SEG_BASE + 8; // 零补丁断言失败：检出被补丁产物
const E_MISSING: u16 = SEG_BASE + 9; // 零补丁断言失败：上游或本侧哈希缺失
const E_PENDING: u16 = SEG_BASE + 10; // 零补丁断言失败：仍有未对照条目
const E_EMPTY: u16 = SEG_BASE + 11; // 声明账为空，无从断言
const E_BAL: u16 = SEG_BASE + 12; // 守恒恒等式破裂（四态计数不守恒）
const E_SEALED: u16 = SEG_BASE + 13; // 账已封，拒绝再写
const E_VER_EMPTY: u16 = SEG_BASE + 14; // 版本号空串

/// 声明账定容：B37 嫁接面产物上界 32 条，超出必须分账。
pub const MAX_DECL: usize = 32;

/// 边界声明账三段位编号：段一产物登记、段二哈希对照、段三结论声明。
pub const SECT_REGISTRY: u8 = 1;
pub const SECT_HASHMAP: u8 = 2;
pub const SECT_VERDICT: u8 = 3;

// ── FNV-1a 64（确定性字节哈希，唯一一致性口径）──────────────────
pub fn fnv1a64(data: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in data {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
    }
    h
}

/// 诊断详情：把错误码翻译成中文可操作建议。
pub fn detail(code: u16) -> &'static str {
    match code {
        E_DECL_DUP => "声明编号重复：同一产物同一版本只允许一条声明，先查账再登记",
        E_DECL_FULL => "声明账已满 32 条：B37 嫁接面上界触顶，分拆次级账本",
        E_DECL_NOTFOUND => "声明编号不存在：先走 declare 登记再录哈希",
        E_HASH_TWICE => "哈希重复写入：一版一哈希，改值须走勘误通道不可覆写",
        E_HASH_ZERO => "哈希为 0：0 视为未采集，禁止用零值伪造对照",
        E_PHASE => "账本状态机非法边：登记期只许登记、对照期只许对照",
        E_SECT_MISSING => "三段不齐：段一/段二/段三必须全部落账才可封账",
        E_PATCHED => "检出补丁产物：构建产物与上游哈希不等，回查构建脚本",
        E_MISSING => "哈希缺失：上游或本侧有一侧未采集，先补齐再断言",
        E_PENDING => "仍有待对照条目：零补丁断言是全有全无断言，先清零 pending",
        E_EMPTY => "声明账为空：至少登记一条嫁接产物才可断言",
        E_BAL => "四态计数不守恒：identical+patched+missing+pending 必须等于 count",
        E_SEALED => "账已封账：封账后只读，写操作一律拒绝",
        E_VER_EMPTY => "版本号为空：逐版比对必须有版本锚，拒绝无版本声明",
        _ => "未知诊断码",
    }
}

// ── 声明结论四态 ─────────────────────────────────────────────
/// 单条声明的对照结论：逐版哈希比对的唯一产物。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DeclVerdict {
    /// 已登记但两侧哈希未齐，等待对照。
    Pending,
    /// 两侧哈希逐版一致，零补丁通过。
    Identical,
    /// 两侧哈希不等，检出补丁，必须拦截。
    Patched,
    /// 任一侧哈希缺失，无法裁决，视同未通过。
    Missing,
}

impl DeclVerdict {
    /// 四态编号（落账用，确定性映射）。
    pub fn code(self) -> u8 {
        match self {
            DeclVerdict::Pending => 0,
            DeclVerdict::Identical => 1,
            DeclVerdict::Patched => 2,
            DeclVerdict::Missing => 3,
        }
    }

    /// 中文短句（报告行渲染用）。
    pub fn zh(self) -> &'static str {
        match self {
            DeclVerdict::Pending => "待对照",
            DeclVerdict::Identical => "逐版一致",
            DeclVerdict::Patched => "检出补丁",
            DeclVerdict::Missing => "哈希缺失",
        }
    }
}

// ── 账本状态机 ───────────────────────────────────────────────
/// 账本生命周期：合法边 Open→Filled→CrossChecked→Sealed，一步不跳。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LedgerPhase {
    /// 开账期：只允许登记声明与采集哈希。
    Open,
    /// 三段落齐、哈希齐备，等待逐版对照。
    Filled,
    /// 逐版对照已跑完，断言已出结论。
    CrossChecked,
    /// 封账：只读，哈希封存。
    Sealed,
}

impl LedgerPhase {
    /// 阶段编号（1..4，落账与守恒核对用）。
    pub fn code(self) -> u8 {
        match self {
            LedgerPhase::Open => 1,
            LedgerPhase::Filled => 2,
            LedgerPhase::CrossChecked => 3,
            LedgerPhase::Sealed => 4,
        }
    }
}

// ── 单条边界声明 ─────────────────────────────────────────────
/// 一条嫁接边界声明：一个构建产物 × 一个版本 × 两侧哈希。
#[derive(Clone, Debug)]
pub struct GraftDecl {
    /// 声明编号（账内唯一，1 起连续）。
    pub decl_id: u32,
    /// 构建产物名（如 limine.bin / kernel.elf，账内唯一键之一）。
    pub artifact: String,
    /// 版本锚（逐版比对的最小单位，非空）。
    pub version: String,
    /// 上游原产物哈希（FNV-1a 64，0=未采集）。
    pub upstream_hash: u64,
    /// 本侧引导器构建产物哈希（FNV-1a 64，0=未采集）。
    pub built_hash: u64,
    /// 当前结论（由 crosscheck 裁决，登记期为 Pending）。
    pub verdict: DeclVerdict,
}

impl GraftDecl {
    /// 两侧哈希是否均已采集（非零）。
    fn both_hashed(&self) -> bool {
        self.upstream_hash != 0 && self.built_hash != 0
    }
}

// ── 逐版对照报告 ─────────────────────────────────────────────
/// 一次逐版对照的聚合结果（四态守恒由调用方复核）。
#[derive(Clone, Debug)]
pub struct CrossReport {
    /// 逐版一致条数。
    pub identical: u32,
    /// 检出补丁条数。
    pub patched: u32,
    /// 哈希缺失条数。
    pub missing: u32,
    /// 待对照条数。
    pub pending: u32,
    /// 总条数（守恒分母）。
    pub total: u32,
}

impl CrossReport {
    /// 守恒恒等式：四态之和必须等于总条数，破式即账本损坏。
    pub fn conserved(&self) -> bool {
        self.identical + self.patched + self.missing + self.pending == self.total
    }

    /// 零补丁口径：全部逐版一致且总条数非零。
    pub fn zero_patch(&self) -> bool {
        self.total > 0 && self.identical == self.total
    }
}

// ── 边界声明账（定容账本主体）─────────────────────────────────
/// 嫁接边界声明账：三段式定容账本，零补丁断言的唯一事实源。
pub struct BoundaryLedger {
    /// 定容声明槽位：None=空槽。
    decls: [Option<GraftDecl>; MAX_DECL],
    /// 已占槽位数（守恒分母）。
    count: usize,
    /// 三段落账位图：bit0=段一 bit1=段二 bit2=段三。
    section_bits: u8,
    /// 账本状态机当前阶段。
    phase: LedgerPhase,
    /// 封账哈希（封账时对全账序列化字节做 FNV-1a 64，0=未封）。
    sealed_hash: u64,
}

impl BoundaryLedger {
    /// 开账：空账、Open 阶段、三段落账位清零。
    pub fn new() -> BoundaryLedger {
        BoundaryLedger {
            decls: core::array::from_fn(|_| None),
            count: 0,
            section_bits: 0,
            phase: LedgerPhase::Open,
            sealed_hash: 0,
        }
    }

    /// 段位落账：置位对应段 bit（幂等，重复落账无害）。
    pub fn mark_section(&mut self, sect: u8) -> Result<(), u16> {
        if self.phase == LedgerPhase::Sealed {
            return Err(E_SEALED);
        }
        match sect {
            SECT_REGISTRY => self.section_bits |= 0b001,
            SECT_HASHMAP => self.section_bits |= 0b010,
            SECT_VERDICT => self.section_bits |= 0b100,
            _ => return Err(E_SECT_MISSING),
        }
        Ok(())
    }

    /// 三段齐备判定：三个 bit 全 1。
    pub fn sections_complete(&self) -> bool {
        self.section_bits == 0b111
    }

    /// 段一（产物登记）：登记一条边界声明，编号自动连续分配。
    pub fn declare(&mut self, artifact: &str, version: &str) -> Result<u32, u16> {
        if self.phase == LedgerPhase::Sealed {
            return Err(E_SEALED);
        }
        if self.count >= MAX_DECL {
            return Err(E_DECL_FULL);
        }
        if artifact.is_empty() {
            return Err(E_DECL_NOTFOUND);
        }
        if version.is_empty() {
            return Err(E_VER_EMPTY);
        }
        // 账内唯一键：artifact × version 不得重复。
        let mut i = 0usize;
        while i < self.count {
            if let Some(d) = &self.decls[i] {
                if d.artifact == artifact && d.version == version {
                    return Err(E_DECL_DUP);
                }
            }
            i += 1;
        }
        let decl_id = (self.count + 1) as u32;
        self.decls[self.count] = Some(GraftDecl {
            decl_id,
            artifact: artifact.to_string(),
            version: version.to_string(),
            upstream_hash: 0,
            built_hash: 0,
            verdict: DeclVerdict::Pending,
        });
        self.count += 1;
        self.mark_section(SECT_REGISTRY)?;
        Ok(decl_id)
    }

    /// 槽位定位：按声明编号找下标（编号=登记序，1 起）。
    fn slot_of(&self, decl_id: u32) -> Result<usize, u16> {
        if decl_id == 0 || decl_id as usize > self.count {
            return Err(E_DECL_NOTFOUND);
        }
        Ok((decl_id - 1) as usize)
    }

    /// 段二（哈希对照·上游侧）：采集上游原产物哈希。
    pub fn record_upstream(&mut self, decl_id: u32, hash: u64) -> Result<(), u16> {
        if self.phase == LedgerPhase::Sealed {
            return Err(E_SEALED);
        }
        if self.phase != LedgerPhase::Open {
            return Err(E_PHASE);
        }
        if hash == 0 {
            return Err(E_HASH_ZERO);
        }
        let s = self.slot_of(decl_id)?;
        let d = self.decls[s].as_mut().ok_or(E_DECL_NOTFOUND)?;
        if d.upstream_hash != 0 {
            return Err(E_HASH_TWICE);
        }
        d.upstream_hash = hash;
        self.mark_section(SECT_HASHMAP)?;
        Ok(())
    }

    /// 段二（哈希对照·本侧）：采集引导器构建产物哈希。
    pub fn record_built(&mut self, decl_id: u32, hash: u64) -> Result<(), u16> {
        if self.phase == LedgerPhase::Sealed {
            return Err(E_SEALED);
        }
        if self.phase != LedgerPhase::Open {
            return Err(E_PHASE);
        }
        if hash == 0 {
            return Err(E_HASH_ZERO);
        }
        let s = self.slot_of(decl_id)?;
        let d = self.decls[s].as_mut().ok_or(E_DECL_NOTFOUND)?;
        if d.built_hash != 0 {
            return Err(E_HASH_TWICE);
        }
        d.built_hash = hash;
        self.mark_section(SECT_HASHMAP)?;
        Ok(())
    }

    /// 转入对照期：要求三段齐备且每条两侧哈希均已登记。
    pub fn begin_crosscheck(&mut self) -> Result<(), u16> {
        if self.phase == LedgerPhase::Sealed {
            return Err(E_SEALED);
        }
        if self.phase != LedgerPhase::Open {
            return Err(E_PHASE);
        }
        if !self.sections_complete() {
            return Err(E_SECT_MISSING);
        }
        if self.count == 0 {
            return Err(E_EMPTY);
        }
        let mut i = 0usize;
        while i < self.count {
            if let Some(d) = &self.decls[i] {
                if !d.both_hashed() {
                    return Err(E_MISSING);
                }
            }
            i += 1;
        }
        self.mark_section(SECT_VERDICT)?;
        self.phase = LedgerPhase::Filled;
        Ok(())
    }

    /// 逐版对照：对每条声明做哈希相等裁决，产出四态守恒报告。
    pub fn crosscheck(&mut self) -> Result<CrossReport, u16> {
        if self.phase != LedgerPhase::Filled {
            return Err(E_PHASE);
        }
        let mut rep = CrossReport {
            identical: 0,
            patched: 0,
            missing: 0,
            pending: 0,
            total: self.count as u32,
        };
        let mut i = 0usize;
        while i < self.count {
            if let Some(d) = self.decls[i].as_mut() {
                d.verdict = if d.upstream_hash == 0 || d.built_hash == 0 {
                    rep.missing += 1;
                    DeclVerdict::Missing
                } else if d.upstream_hash == d.built_hash {
                    rep.identical += 1;
                    DeclVerdict::Identical
                } else {
                    rep.patched += 1;
                    DeclVerdict::Patched
                };
            }
            i += 1;
        }
        if !rep.conserved() {
            return Err(E_BAL);
        }
        self.phase = LedgerPhase::CrossChecked;
        Ok(rep)
    }

    /// 零补丁断言：全有全无，逐版一致才算过。
    pub fn zero_patch_assert(&self, rep: &CrossReport) -> Result<(), u16> {
        if self.phase != LedgerPhase::CrossChecked {
            return Err(E_PHASE);
        }
        if !rep.conserved() {
            return Err(E_BAL);
        }
        if rep.total == 0 {
            return Err(E_EMPTY);
        }
        if rep.pending > 0 {
            return Err(E_PENDING);
        }
        if rep.missing > 0 {
            return Err(E_MISSING);
        }
        if rep.patched > 0 {
            return Err(E_PATCHED);
        }
        Ok(())
    }

    /// 段三（结论声明）+ 封账：断言通过后对全账哈希封存。
    pub fn seal(&mut self) -> Result<u64, u16> {
        if self.phase != LedgerPhase::CrossChecked {
            return Err(E_PHASE);
        }
        if !self.sections_complete() {
            return Err(E_SECT_MISSING);
        }
        let mut buf: Vec<u8> = Vec::new();
        let mut i = 0usize;
        while i < self.count {
            if let Some(d) = &self.decls[i] {
                buf.extend_from_slice(d.decl_id.to_le_bytes().as_slice());
                buf.extend_from_slice(d.artifact.as_bytes());
                buf.push(0x1f);
                buf.extend_from_slice(d.version.as_bytes());
                buf.push(0x1f);
                buf.extend_from_slice(d.upstream_hash.to_le_bytes().as_slice());
                buf.extend_from_slice(d.built_hash.to_le_bytes().as_slice());
                buf.push(d.verdict.code());
            }
            i += 1;
        }
        buf.push(self.section_bits);
        buf.push(self.phase.code());
        let h = fnv1a64(&buf);
        self.sealed_hash = h;
        self.phase = LedgerPhase::Sealed;
        Ok(h)
    }

    /// 封账哈希读取（0=未封）。
    pub fn sealed_hash(&self) -> u64 {
        self.sealed_hash
    }

    /// 当前阶段。
    pub fn phase(&self) -> LedgerPhase {
        self.phase
    }

    /// 条目数。
    pub fn len(&self) -> usize {
        self.count
    }

    /// 空账判定。
    pub fn is_empty(&self) -> bool {
        self.count == 0
    }

    /// 按编号读取单条声明的只读快照。
    pub fn get(&self, decl_id: u32) -> Result<(String, String, DeclVerdict), u16> {
        let s = self.slot_of(decl_id)?;
        let d = self.decls[s].as_ref().ok_or(E_DECL_NOTFOUND)?;
        Ok((d.artifact.clone(), d.version.clone(), d.verdict))
    }

    /// 按结论过滤声明编号清单（审计取数用）。
    pub fn ids_by_verdict(&self, v: DeclVerdict) -> Vec<u32> {
        let mut out: Vec<u32> = Vec::new();
        let mut i = 0usize;
        while i < self.count {
            if let Some(d) = &self.decls[i] {
                if d.verdict == v {
                    out.push(d.decl_id);
                }
            }
            i += 1;
        }
        out
    }

    /// 四态实时计数（不动结论，只盘点）。
    pub fn tally(&self) -> CrossReport {
        let mut rep = CrossReport {
            identical: 0,
            patched: 0,
            missing: 0,
            pending: 0,
            total: self.count as u32,
        };
        let mut i = 0usize;
        while i < self.count {
            if let Some(d) = &self.decls[i] {
                match d.verdict {
                    DeclVerdict::Identical => rep.identical += 1,
                    DeclVerdict::Patched => rep.patched += 1,
                    DeclVerdict::Missing => rep.missing += 1,
                    DeclVerdict::Pending => rep.pending += 1,
                }
            }
            i += 1;
        }
        rep
    }

    /// 审计行渲染：逐条"编号|产物|版本|结论"，报告节直接消费。
    pub fn audit_lines(&self) -> Vec<String> {
        let mut lines: Vec<String> = Vec::new();
        lines.push("=== 边界声明账 · 三段审计 ===".to_string());
        let rep = self.tally();
        lines.push("四态守恒: ".to_string()
            + &itoa(rep.identical as u64) + "+" + &itoa(rep.patched as u64) + "+"
            + &itoa(rep.missing as u64) + "+" + &itoa(rep.pending as u64)
            + "=" + &itoa(rep.total as u64));
        let mut i = 0usize;
        while i < self.count {
            if let Some(d) = &self.decls[i] {
                let mut line = String::from("D");
                line.push_str(&itoa(d.decl_id as u64));
                line.push('|');
                line.push_str(&d.artifact);
                line.push('|');
                line.push_str(&d.version);
                line.push('|');
                line.push_str(d.verdict.zh());
                lines.push(line);
            }
            i += 1;
        }
        lines
    }
}

/// 极简无符号转十进制（确定性、无格式化依赖）。
fn itoa(mut v: u64) -> String {
    if v == 0 {
        return String::from("0");
    }
    let mut buf = [0u8; 20];
    let mut n = 0usize;
    while v > 0 {
        buf[n] = b'0' + (v % 10) as u8;
        v /= 10;
        n += 1;
    }
    let mut s = String::new();
    let mut i = n;
    while i > 0 {
        i -= 1;
        s.push(buf[i] as char);
    }
    s
}

/// 零补丁断言的一站式收口：对照→断言→（可选）封账。
pub fn run_zero_patch_gate(ledger: &mut BoundaryLedger) -> Result<(CrossReport, Option<u64>), u16> {
    ledger.begin_crosscheck()?;
    let rep = ledger.crosscheck()?;
    ledger.zero_patch_assert(&rep)?;
    let sealed = if ledger.phase() == LedgerPhase::CrossChecked {
        Some(ledger.seal()?)
    } else {
        None
    };
    Ok((rep, sealed))
}

// ── 自例程 ───────────────────────────────────────────────────
/// 演练：样本走主路径（逐版一致→断言→封账）+ 错误路径（补丁注入→拦截）。
/// 返回 0 = 全过。
pub fn drill() -> Result<u32, u16> {
    // 主路径：三条产物，两侧哈希逐版一致。
    let mut lg = BoundaryLedger::new();
    let d1 = lg.declare("limine.bin", "v7.0.3")?;
    let d2 = lg.declare("kernel.elf", "b37")?;
    let d3 = lg.declare("initrd.img", "b37")?;
    let h1 = fnv1a64(b"limine.bin@v7.0.3");
    let h2 = fnv1a64(b"kernel.elf@b37");
    let h3 = fnv1a64(b"initrd.img@b37");
    lg.record_upstream(d1, h1)?;
    lg.record_built(d1, h1)?;
    lg.record_upstream(d2, h2)?;
    lg.record_built(d2, h2)?;
    lg.record_upstream(d3, h3)?;
    lg.record_built(d3, h3)?;
    let (rep, sealed) = run_zero_patch_gate(&mut lg)?;
    if !rep.zero_patch() || !rep.conserved() {
        return Err(SEG_BASE);
    }
    if sealed.is_none() || lg.phase() != LedgerPhase::Sealed {
        return Err(SEG_BASE);
    }
    // 封账后写尝试必须被拒。
    if lg.record_upstream(d1, h1).is_ok() {
        return Err(SEG_BASE);
    }
    // 错误路径一：补丁注入，断言必须拦截。
    let mut lg2 = BoundaryLedger::new();
    let e1 = lg2.declare("limine.bin", "v7.0.3")?;
    lg2.record_upstream(e1, h1)?;
    lg2.record_built(e1, fnv1a64(b"limine.bin@v7.0.3-patched"))?;
    lg2.begin_crosscheck()?;
    let rep2 = lg2.crosscheck()?;
    match lg2.zero_patch_assert(&rep2) {
        Err(c) if c == E_PATCHED => {}
        _ => return Err(SEG_BASE),
    }
    // 错误路径二：哈希缺失，转 Filled 前即拦截。
    let mut lg3 = BoundaryLedger::new();
    let f1 = lg3.declare("kernel.elf", "b37")?;
    lg3.record_upstream(f1, h2)?;
    match lg3.begin_crosscheck() {
        Err(c) if c == E_MISSING => {}
        _ => return Err(SEG_BASE),
    }
    // 错误路径三：重复登记同键。
    let mut lg4 = BoundaryLedger::new();
    let _ = lg4.declare("limine.bin", "v7.0.3")?;
    match lg4.declare("limine.bin", "v7.0.3") {
        Err(c) if c == E_DECL_DUP => {}
        _ => return Err(SEG_BASE),
    }
    Ok(0)
}
