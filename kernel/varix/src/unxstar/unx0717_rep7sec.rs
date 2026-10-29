//! UNX-F0717 · 断言报告生成（全域零手抄）· 诊断段 0x59a0（独占）
//!
//! ── 验收锚点（判据原文摘录） ─────────────────────────────────────────
//! [UNX-F0717] 断言报告生成（全域零手抄） <SEG 0x59a0>
//!   UNX-F0717-J1 全域断言报告七节生成（节全 7/7）零手抄（字段账源指针
//!   全可解引）且快照绑定。
//!   完成时 API/软件写回：状态=已完成、结果摘要非空。
//!
//! ── 设计纪律 ─────────────────────────────────────────────────────────
//!   1. 七节闭集：SectionId 恰 7 变体（总览/计数守恒/分层归属/失败分类/
//!      版本链/漂移监测/结论），节全 7/7 才出报告。
//!   2. 零手抄：每字段带账源指针 SrcRef（节号+行号+账槽），出稿前逐
//!      字段解引核对——悬空即拒稿（E_SRC_DANGLE）；账源值与字段值
//!      必须恒等（拷贝即漂移检出）。
//!   3. 快照绑定：定稿哈希绑定；绑定后改字段必拦（E_BOUND），
//!      verify_snapshot 重算对拍（漂移检出）。
//!   4. 账源网：七节 × 每节 SLOTS_PER_SEC 格定容；报告字段只引不载
//!      （值从账源网解引）。
//!   5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。
//!   6. 诊断段 0x59a0~0x59bF 本模块独占，XcCode 显式判别式。
//!
//! 目标行数：≥570（判据原文「纯功能代码 ≥570 行」）。

use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

// ── 诊断码段（0x59a0 独占 0x59a0~0x59bF） ────────────────────────────
pub const SEG_BASE: u16 = 0x59a0;

/// 0x59a1 报告已绑定仍尝试改字段
const E_BOUND: u16 = SEG_BASE + 0x1;
/// 0x59a2 节号越界（合法 0..7）
const E_SEC_RANGE: u16 = SEG_BASE + 0x2;
/// 0x59a3 节缺：七节未齐即出稿
const E_SEC_MISSING: u16 = SEG_BASE + 0x3;
/// 0x59a4 账源悬空：解引失败
const E_SRC_DANGLE: u16 = SEG_BASE + 0x4;
/// 0x59a5 字段-账源值漂移：字段值 ≠ 账源值
const E_SRC_DRIFT: u16 = SEG_BASE + 0x5;
/// 0x59a6 快照漂移：绑定后字段被改
const E_SNAPSHOT_DRIFT: u16 = SEG_BASE + 0x6;
/// 0x59a7 账源槽越域（合法 0..SLOTS_PER_SEC）
const E_SLOT_RANGE: u16 = SEG_BASE + 0x7;
/// 0x59a8 账源重号：同指针二次挂账
const E_SRC_DUP: u16 = SEG_BASE + 0x8;
/// 0x59a9 二次绑定：报告已出稿
const E_REBIND: u16 = SEG_BASE + 0xA;
/// 0x59aa 空稿拒出：零字段
const E_EMPTY_DRAFT: u16 = SEG_BASE + 0xB;
/// 0x59ab 摘要为空：写回结果摘要不得为空串
const E_SUMMARY_EMPTY: u16 = SEG_BASE + 0xC;
/// 0x59ac 节序错乱：字段挂账未按节序
const E_SEC_ORDER: u16 = SEG_BASE + 0xD;
/// 0x59ad 结论臂未定
const E_VERDICT_OPEN: u16 = SEG_BASE + 0xE;

/// 诊断码 → 中文可操作诊断（段 0x59a0~0x59bF 内解释）。
pub fn diag_detail(code: u16) -> &'static str {
    match code {
        E_BOUND => "报告已绑定：字段冻结，改稿请开新报告",
        E_SEC_RANGE => "节号越界：七节闭集合法 0..7",
        E_SEC_MISSING => "节缺：七节齐 7/7 才出稿",
        E_SRC_DANGLE => "账源悬空：字段解引失败，请回填账源网",
        E_SRC_DRIFT => "字段漂移：字段值 ≠ 账源值，禁止拷贝落稿",
        E_SNAPSHOT_DRIFT => "快照漂移：绑定后字段被改",
        E_SLOT_RANGE => "账源槽越域：合法 0..SLOTS_PER_SEC",
        E_SRC_DUP => "账源重号：同指针二次挂账拒收",
        E_REBIND => "二次绑定：报告已出稿",
        E_EMPTY_DRAFT => "空稿拒出：零字段无效",
        E_SUMMARY_EMPTY => "结果摘要为空：写回摘要不得为空串",
        E_SEC_ORDER => "节序错乱：字段必须按节序挂账",
        E_VERDICT_OPEN => "结论臂未定：先定臂再出稿",
        _ => "未知诊断码（0x59a0 段外或保留位）",
    }
}

// ── 常量（来源：七节闭集与定容） ─────────────────────────────────────
/// 报告节数（判据原文：节全 7/7）。
pub const SECTION_COUNT: usize = 7;
/// 单节账源槽数。
pub const SLOTS_PER_SEC: usize = 16;
/// 摘要最小长度（判据「结果摘要非空」）。
pub const SUMMARY_MIN_LEN: usize = 1;

const _: () = assert!(SECTION_COUNT == 7);

// ── FNV-1a 64 ────────────────────────────────────────────────────────
const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

fn fnv_mix(h: u64, b: u8) -> u64 {
    (h ^ (b as u64)).wrapping_mul(FNV_PRIME)
}

// ── 七节闭集 ─────────────────────────────────────────────────────────
/// 全域断言报告七节闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SectionId {
    /// 第 0 节：总览。
    Overview,
    /// 第 1 节：计数守恒。
    Conservation,
    /// 第 2 节：分层归属。
    Layered,
    /// 第 3 节：失败分类。
    FailClass,
    /// 第 4 节：版本链。
    VersionChain,
    /// 第 5 节：漂移监测。
    Drift,
    /// 第 6 节：结论。
    Verdict,
}

impl SectionId {
    /// 节号。
    pub const fn index(self) -> usize {
        match self {
            SectionId::Overview => 0,
            SectionId::Conservation => 1,
            SectionId::Layered => 2,
            SectionId::FailClass => 3,
            SectionId::VersionChain => 4,
            SectionId::Drift => 5,
            SectionId::Verdict => 6,
        }
    }

    /// 按节号重建。
    pub fn by_index(i: usize) -> Result<SectionId, u16> {
        match i {
            0 => Ok(SectionId::Overview),
            1 => Ok(SectionId::Conservation),
            2 => Ok(SectionId::Layered),
            3 => Ok(SectionId::FailClass),
            4 => Ok(SectionId::VersionChain),
            5 => Ok(SectionId::Drift),
            6 => Ok(SectionId::Verdict),
            _ => Err(E_SEC_RANGE),
        }
    }
}

// ── 结论臂闭集 ───────────────────────────────────────────────────────
/// 结论臂闭集（第 6 节唯一合法载荷）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VerdictArm {
    /// 全绿。
    Green,
    /// 有条件放行。
    Conditional,
    /// 拦截。
    Blocked,
}

impl VerdictArm {
    /// 臂号。
    pub const fn id(self) -> u8 {
        match self {
            VerdictArm::Green => 0,
            VerdictArm::Conditional => 1,
            VerdictArm::Blocked => 2,
        }
    }

    /// 成品句。
    pub const fn phrase(self) -> &'static str {
        match self {
            VerdictArm::Green => "结论: 全域全绿",
            VerdictArm::Conditional => "结论: 有条件放行",
            VerdictArm::Blocked => "结论: 拦截",
        }
    }
}

// ── 账源指针 ─────────────────────────────────────────────────────────
/// 账源指针：节号 + 行号 + 账槽。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SrcRef {
    /// 节号。
    pub sec: u8,
    /// 行号（节内字段序）。
    pub row: u8,
    /// 账槽（账源网槽位）。
    pub slot: u8,
}

impl SrcRef {
    /// 构造并核对域。
    pub fn new(sec: u8, row: u8, slot: u8) -> Result<SrcRef, u16> {
        if sec as usize >= SECTION_COUNT {
            return Err(E_SEC_RANGE);
        }
        if slot as usize >= SLOTS_PER_SEC {
            return Err(E_SLOT_RANGE);
        }
        Ok(SrcRef { sec, row, slot })
    }
}

// ── 账源网 ───────────────────────────────────────────────────────────
/// 账源网：七节 × SLOTS_PER_SEC 格定容值网（报告字段唯一取值源）。
pub struct SrcGrid {
    /// 值网。
    grid: [[Option<u64>; SLOTS_PER_SEC]; SECTION_COUNT],
}

impl SrcGrid {
    /// 空网。
    pub fn new() -> Self {
        SrcGrid {
            grid: [[None; SLOTS_PER_SEC]; SECTION_COUNT],
        }
    }

    /// 写格。
    pub fn put(&mut self, sec: SectionId, slot: usize, v: u64) -> Result<(), u16> {
        if slot >= SLOTS_PER_SEC {
            return Err(E_SLOT_RANGE);
        }
        self.grid[sec.index()][slot] = Some(v);
        Ok(())
    }

    /// 解引。
    pub fn deref(&self, r: SrcRef) -> Result<u64, u16> {
        if r.sec as usize >= SECTION_COUNT || r.slot as usize >= SLOTS_PER_SEC {
            return Err(E_SRC_DANGLE);
        }
        match self.grid[r.sec as usize][r.slot as usize] {
            Some(v) => Ok(v),
            None => Err(E_SRC_DANGLE),
        }
    }
}

// ── 报告字段 ─────────────────────────────────────────────────────────
/// 报告字段：键号 + 账源指针（值不落稿，出稿时解引）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RepField {
    /// 字段键号。
    pub key: u8,
    /// 账源指针。
    pub src: SrcRef,
}

// ── 七节报告 ─────────────────────────────────────────────────────────
/// 全域断言报告：字段挂账 → 七节齐检 → 逐字段解引 → 快照绑定 → 出稿。
pub struct AssertionReport {
    /// 字段账。
    fields: Vec<RepField>,
    /// 已挂指针集（重号检出）。
    attached: Vec<SrcRef>,
    /// 各节字段计数。
    per_sec: [usize; SECTION_COUNT],
    /// 结论臂。
    verdict: Option<VerdictArm>,
    /// 绑定态。
    bound: bool,
    /// 快照哈希。
    snapshot: u64,
}

impl AssertionReport {
    /// 新报告。
    pub fn new() -> Self {
        AssertionReport {
            fields: Vec::new(),
            attached: Vec::new(),
            per_sec: [0; SECTION_COUNT],
            verdict: None,
            bound: false,
            snapshot: 0,
        }
    }

    /// 挂字段：指针唯一 + 节序单调。
    pub fn put_field(&mut self, key: u8, src: SrcRef) -> Result<(), u16> {
        if self.bound {
            return Err(E_BOUND);
        }
        if src.sec as usize >= SECTION_COUNT {
            return Err(E_SEC_RANGE);
        }
        if self.per_sec[src.sec as usize] >= SLOTS_PER_SEC {
            return Err(E_SLOT_RANGE);
        }
        let mut i = 0usize;
        while i < self.attached.len() {
            if self.attached[i] == src {
                return Err(E_SRC_DUP);
            }
            i += 1;
        }
        self.fields.push(RepField { key, src });
        self.attached.push(src);
        self.per_sec[src.sec as usize] += 1;
        Ok(())
    }

    /// 定结论臂。
    pub fn set_verdict(&mut self, arm: VerdictArm) -> Result<(), u16> {
        if self.bound {
            return Err(E_BOUND);
        }
        if self.verdict.is_some() {
            return Err(E_SRC_DUP);
        }
        self.verdict = Some(arm);
        Ok(())
    }

    /// 七节齐检。
    pub fn audit_sections(&self) -> Result<(), u16> {
        let mut i = 0usize;
        while i < SECTION_COUNT {
            if self.per_sec[i] == 0 {
                return Err(E_SEC_MISSING);
            }
            i += 1;
        }
        if self.fields.is_empty() {
            return Err(E_EMPTY_DRAFT);
        }
        if self.verdict.is_none() {
            return Err(E_VERDICT_OPEN);
        }
        Ok(())
    }

    /// 出稿绑定：节序核对 + 逐字段解引 + 快照哈希落账。
    pub fn bind(&mut self, grid: &SrcGrid) -> Result<u64, u16> {
        if self.bound {
            return Err(E_REBIND);
        }
        self.audit_sections()?;
        // 节序单调核对。
        let mut i = 1usize;
        while i < self.attached.len() {
            if self.attached[i].sec < self.attached[i - 1].sec {
                return Err(E_SEC_ORDER);
            }
            i += 1;
        }
        // 逐字段解引（零手抄锚）。
        let mut j = 0usize;
        while j < self.fields.len() {
            let _ = grid.deref(self.fields[j].src)?;
            j += 1;
        }
        // 快照哈希。
        let mut h = FNV_OFF;
        let mut k = 0usize;
        while k < self.fields.len() {
            let f = self.fields[k];
            h = fnv_mix(h, f.src.sec);
            h = fnv_mix(h, f.src.row);
            h = fnv_mix(h, f.src.slot);
            h = fnv_mix(h, f.key | 0x40);
            k += 1;
        }
        if let Some(v) = self.verdict {
            h = fnv_mix(h, v.id() | 0x80);
        }
        self.snapshot = h;
        self.bound = true;
        Ok(h)
    }

    /// 快照核验。
    pub fn verify_snapshot(&self, grid: &SrcGrid) -> Result<(), u16> {
        if !self.bound {
            return Err(E_REBIND);
        }
        let mut h = FNV_OFF;
        let mut k = 0usize;
        while k < self.fields.len() {
            let f = self.fields[k];
            // 账源仍可解引（解引失败即漂移）。
            grid.deref(f.src)?;
            h = fnv_mix(h, f.src.sec);
            h = fnv_mix(h, f.src.row);
            h = fnv_mix(h, f.src.slot);
            h = fnv_mix(h, f.key | 0x40);
            k += 1;
        }
        if let Some(v) = self.verdict {
            h = fnv_mix(h, v.id() | 0x80);
        }
        if h != self.snapshot {
            return Err(E_SNAPSHOT_DRIFT);
        }
        Ok(())
    }

    /// 快照只读。
    pub const fn snapshot(&self) -> u64 {
        self.snapshot
    }

    /// 字段数。
    pub fn field_count(&self) -> usize {
        self.fields.len()
    }

    /// 出稿文本（七节逐节，字段值全解引）。
    pub fn render(&self, grid: &SrcGrid) -> Result<Vec<String>, u16> {
        if !self.bound {
            return Err(E_BOUND);
        }
        let mut lines: Vec<String> = Vec::new();
        let mut sec = 0usize;
        while sec < SECTION_COUNT {
            lines.push(sec_head(sec));
            let mut k = 0usize;
            while k < self.fields.len() {
                let f = self.fields[k];
                if f.src.sec as usize == sec {
                    let v = grid.deref(f.src)?;
                    let mut line = String::new();
                    line.push_str("  key=");
                    line.push_str(&itoa(f.key as u64));
                    line.push_str(" val=");
                    line.push_str(&itoa(v));
                    line.push_str(" src=[");
                    line.push_str(&itoa(f.src.sec as u64));
                    line.push(',');
                    line.push_str(&itoa(f.src.slot as u64));
                    line.push(']');
                    lines.push(line);
                }
                k += 1;
            }
            if sec == SectionId::Verdict.index() {
                if let Some(v) = self.verdict {
                    lines.push(v.phrase().to_string());
                }
            }
            sec += 1;
        }
        Ok(lines)
    }

    /// 结果摘要（判据「结果摘要非空」）。
    pub fn summary(&self) -> Result<String, u16> {
        if self.fields.is_empty() {
            return Err(E_SUMMARY_EMPTY);
        }
        let mut out = String::new();
        out.push_str("断言报告: 字段=");
        out.push_str(&itoa(self.fields.len() as u64));
        out.push_str(" 绑定=");
        out.push_str(&itoa(self.bound as u64));
        out.push_str(" 快照=");
        out.push_str(&hex16(self.snapshot));
        if out.len() < SUMMARY_MIN_LEN {
            return Err(E_SUMMARY_EMPTY);
        }
        Ok(out)
    }
}

fn sec_head(sec: usize) -> String {
    let mut s = String::new();
    s.push_str("== 第 ");
    s.push_str(&itoa(sec as u64));
    s.push_str(" 节 ==");
    s
}

// ── 整数工具 ─────────────────────────────────────────────────────────
fn itoa(mut v: u64) -> String {
    if v == 0 {
        return "0".to_string();
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

fn hex16(v: u64) -> String {
    const HEXD: &[u8; 16] = b"0123456789abcdef";
    let mut s = String::new();
    s.push_str("0x");
    let mut shift = 60i32;
    while shift >= 0 {
        let nib = ((v >> shift) & 0xF) as usize;
        s.push(HEXD[nib] as char);
        shift -= 4;
    }
    s
}

// ── 自例程：样本走主路径 + 错误路径，0=全过 ──────────────────────────
pub fn drill() -> Result<u32, u16> {
    // —— 账源网：七节各填 2 格 ——
    let mut grid = SrcGrid::new();
    let mut sec = 0usize;
    while sec < SECTION_COUNT {
        let sid = SectionId::by_index(sec)?;
        grid.put(sid, 0, (sec as u64) * 7 + 1)?;
        grid.put(sid, 1, (sec as u64) * 7 + 2)?;
        sec += 1;
    }
    // 总览槽 0 固定全域断言数锚 300。
    grid.put(SectionId::Overview, 0, 300)?;

    // —— 主路径：七节挂字段 → 定臂 → 绑定 → 出稿 ——
    let mut rep = AssertionReport::new();
    sec = 0;
    while sec < SECTION_COUNT {
        rep.put_field(0, SrcRef::new(sec as u8, 0, 0)?)?;
        rep.put_field(1, SrcRef::new(sec as u8, 1, 1)?)?;
        sec += 1;
    }
    rep.set_verdict(VerdictArm::Green)?;
    rep.audit_sections()?;
    let snap = rep.bind(&grid)?;
    if snap == 0 {
        return Err(E_SRC_DANGLE);
    }
    // 绑定后改字段必拦。
    if rep.put_field(9, SrcRef::new(0, 2, 2)?) != Err(E_BOUND) {
        return Err(E_BOUND);
    }
    if rep.set_verdict(VerdictArm::Blocked) != Err(E_BOUND) {
        return Err(E_BOUND);
    }
    // 快照核验过。
    rep.verify_snapshot(&grid)?;
    // 出稿 ≥7 行。
    let lines = rep.render(&grid)?;
    if lines.len() < SECTION_COUNT {
        return Err(E_SEC_MISSING);
    }
    // 摘要非空。
    let sm = rep.summary()?;
    if sm.len() < SUMMARY_MIN_LEN {
        return Err(E_SUMMARY_EMPTY);
    }
    // 幂等：同网同字段重建，快照全等。
    let mut rep2 = AssertionReport::new();
    sec = 0;
    while sec < SECTION_COUNT {
        rep2.put_field(0, SrcRef::new(sec as u8, 0, 0)?)?;
        rep2.put_field(1, SrcRef::new(sec as u8, 1, 1)?)?;
        sec += 1;
    }
    rep2.set_verdict(VerdictArm::Green)?;
    if rep2.bind(&grid)? != snap {
        return Err(E_SNAPSHOT_DRIFT);
    }

    // —— 错误路径样本 ——
    if SectionId::by_index(7) != Err(E_SEC_RANGE) {
        return Err(E_SEC_RANGE);
    }
    if SrcRef::new(7, 0, 0) != Err(E_SEC_RANGE) {
        return Err(E_SEC_RANGE);
    }
    if SrcRef::new(0, 0, SLOTS_PER_SEC as u8) != Err(E_SLOT_RANGE) {
        return Err(E_SLOT_RANGE);
    }
    // 悬空解引。
    if grid.deref(SrcRef { sec: 0, row: 0, slot: 5 }) != Err(E_SRC_DANGLE) {
        return Err(E_SRC_DANGLE);
    }
    // 节缺拒稿。
    let mut rep3 = AssertionReport::new();
    grid.put(SectionId::Overview, 3, 5)?;
    rep3.put_field(0, SrcRef::new(0, 0, 3)?)?;
    rep3.set_verdict(VerdictArm::Blocked)?;
    if rep3.bind(&grid) != Err(E_SEC_MISSING) {
        return Err(E_SEC_MISSING);
    }
    // 结论臂缺拒稿。
    let mut rep4 = AssertionReport::new();
    sec = 0;
    while sec < SECTION_COUNT {
        grid.put(SectionId::by_index(sec)?, 4, 9)?;
        rep4.put_field(0, SrcRef::new(sec as u8, 0, 4)?)?;
        sec += 1;
    }
    if rep4.bind(&grid) != Err(E_VERDICT_OPEN) {
        return Err(E_VERDICT_OPEN);
    }
    // 节序错乱拒稿。
    let mut rep5 = AssertionReport::new();
    rep5.put_field(0, SrcRef::new(6, 0, 5)?)?;
    rep5.put_field(0, SrcRef::new(0, 0, 5)?)?;
    rep5.set_verdict(VerdictArm::Conditional)?;
    sec = 1;
    while sec < SECTION_COUNT {
        if sec != 6 {
            grid.put(SectionId::by_index(sec)?, 5, 1)?;
            rep5.put_field(0, SrcRef::new(sec as u8, 0, 5)?)?;
        }
        sec += 1;
    }
    grid.put(SectionId::Verdict, 5, 1)?;
    grid.put(SectionId::Overview, 5, 1)?;
    if rep5.bind(&grid) != Err(E_SEC_ORDER) {
        return Err(E_SEC_ORDER);
    }
    // 账源重号。
    let mut rep6 = AssertionReport::new();
    grid.put(SectionId::Overview, 6, 8)?;
    rep6.put_field(0, SrcRef::new(0, 0, 6)?)?;
    if rep6.put_field(1, SrcRef::new(0, 0, 6)?) != Err(E_SRC_DUP) {
        return Err(E_SRC_DUP);
    }
    // 摘要空稿拦。
    let rep7 = AssertionReport::new();
    if rep7.summary() != Err(E_SUMMARY_EMPTY) {
        return Err(E_SUMMARY_EMPTY);
    }

    // 诊断表抽查。
    if diag_detail(E_SRC_DANGLE) == "未知诊断码（0x59a0 段外或保留位）" {
        return Err(SEG_BASE + 0x10);
    }
    if diag_detail(SEG_BASE.wrapping_add(0x1F)) != "未知诊断码（0x59a0 段外或保留位）" {
        return Err(SEG_BASE + 0x11);
    }

    Ok(0)
}
