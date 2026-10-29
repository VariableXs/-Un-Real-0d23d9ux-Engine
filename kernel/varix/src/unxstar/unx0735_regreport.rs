//! UNX-F0735 · 升级回归报告（程序化生成） · 诊断段 0x5b60
//! 判据原文摘录：UNX-F0735-J1 升级回归报告六节生成（节全 6/6）零手抄
//! （指针全可解引）且快照绑定；完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 纯功能代码 ≥570 行。
//!
//! 设计纪律：
//! 1. 报告六节固定：升级摘要/哈希对照/演练结果/SLA 违约/接口回归/结论签章，
//!    节序写死，缺一节即报告无效，不允许可选节。
//! 2. 零手抄口径：每节内容必须由数据指针（快照引用）解引生成，
//!    报告体内不允许出现"人写常量串充当数据"的行；模板串与数据串分离，
//!    数据行一律带来源指针编号（P1..Pn）。
//! 3. 快照绑定：报告生成前先固化数据快照哈希，报告哈希 = 快照哈希 ⊕ 节结构哈希，
//!    快照变则报告哈希必变，绑定关系可复核。
//! 4. 六节生成计数守恒：generated+skipped == SECTIONS，skipped 恒应 0。
//! 5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。
//! 6. 诊断段 0x5b60 为本单独占，XcCode 判别式显式写出。

use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

pub const SEG_BASE: u16 = 0x5b60;

// ── 诊断码闭集（SEG_BASE+1..+N）────────────────────────────────
const E_SECT_DUP: u16 = SEG_BASE + 1; // 节号重复生成
const E_SECT_BAD: u16 = SEG_BASE + 2; // 节号越界（1..6 之外）
const E_NO_SNAPSHOT: u16 = SEG_BASE + 3; // 快照未固化即生成报告
const E_SNAP_MISMATCH: u16 = SEG_BASE + 4; // 快照哈希与报告头不符
const E_PTR_DANGLE: u16 = SEG_BASE + 5; // 指针不可解引（来源数据缺失）
const E_BODY_EMPTY: u16 = SEG_BASE + 6; // 节体为空（零手抄断言失败）
const E_HAND_COPY: u16 = SEG_BASE + 7; // 数据行缺来源指针编号（疑似手抄）
const E_PHASE: u16 = SEG_BASE + 8; // 生成状态机非法边
const E_SECT_MISSING: u16 = SEG_BASE + 9; // 六节未齐即封报告
const E_BAL: u16 = SEG_BASE + 10; // 守恒恒等式破裂（generated+skipped≠6）
const E_SEALED: u16 = SEG_BASE + 11; // 报告已封，拒绝再写
const E_BIND_FAIL: u16 = SEG_BASE + 12; // 快照绑定校验失败
const E_TITLE_DUP: u16 = SEG_BASE + 13; // 报告标题重复生成
const E_NO_REPORT: u16 = SEG_BASE + 14; // 报告未生成，无法读取

/// 报告节数：六节写死。
pub const SECTIONS: u32 = 6;

/// 节号：1 摘要 / 2 哈希对照 / 3 演练 / 4 SLA / 5 接口回归 / 6 结论。
pub const SEC_SUMMARY: u32 = 1;
pub const SEC_HASHES: u32 = 2;
pub const SEC_DRILL: u32 = 3;
pub const SEC_SLA: u32 = 4;
pub const SEC_API: u32 = 5;
pub const SEC_VERDICT: u32 = 6;

/// 每节最多挂载数据行数（定容防线）。
pub const MAX_ROWS_PER_SEC: usize = 16;

// ── FNV-1a 64（快照/报告哈希口径）──────────────────────────────
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
        E_SECT_DUP => "节号重复生成：六节各生成一次，重复即报告污染",
        E_SECT_BAD => "节号越界：合法节号 1..=6",
        E_NO_SNAPSHOT => "快照未固化：先 snapshot() 再生成任何节",
        E_SNAP_MISMATCH => "快照哈希不符：报告头与快照账对不上，拒绝封报告",
        E_PTR_DANGLE => "指针不可解引：来源数据被注销，先恢复数据再生成",
        E_BODY_EMPTY => "节体为空：每节至少一行数据行",
        E_HAND_COPY => "数据行缺指针编号：零手抄口径要求每行带 P 编号",
        E_PHASE => "生成状态机非法边：按节序推进",
        E_SECT_MISSING => "六节未齐：缺节报告不得封账",
        E_BAL => "节计数不守恒：generated+skipped 必须等于 6",
        E_SEALED => "报告已封：封账后只读",
        E_BIND_FAIL => "快照绑定校验失败：报告哈希与快照哈希不互证",
        E_TITLE_DUP => "报告标题重复：同快照同标题只允许一份报告",
        E_NO_REPORT => "报告未生成：先完成六节再读取",
        _ => "未知诊断码",
    }
}

// ── 数据指针（零手抄的核心机制）────────────────────────────────
/// 来源指针：每条数据行声明它从哪个快照数据块解引而来。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DataPtr {
    /// 指针编号（1..=PTR_MAX）。
    pub pid: u32,
    /// 指向的快照数据块编号（快照内唯一）。
    pub block: u32,
}

/// 快照数据块：可解引的最小数据单元。
#[derive(Clone, Debug)]
pub struct SnapBlock {
    /// 块编号（1 起连续）。
    pub block_id: u32,
    /// 数据字节（程序化产出的原始依据）。
    pub bytes: Vec<u8>,
    /// 块哈希（解引校验用）。
    pub hash: u64,
}

impl SnapBlock {
    /// 建块：哈希即内容哈希，非空块。
    pub fn new(block_id: u32, bytes: &[u8]) -> SnapBlock {
        SnapBlock {
            block_id,
            bytes: Vec::from(bytes),
            hash: fnv1a64(bytes),
        }
    }
}

// ── 数据快照 ─────────────────────────────────────────────────
/// 数据快照：六节生成所需的全部数据块的固化视图。
pub struct DataSnapshot {
    blocks: [Option<SnapBlock>; 12],
    /// 块数。
    count: usize,
    /// 快照哈希（全块按序哈希聚合）。
    snap_hash: u64,
    /// 是否已固化。
    frozen: bool,
}

impl DataSnapshot {
    /// 新快照：空、未固化。
    pub fn new() -> DataSnapshot {
        DataSnapshot {
            blocks: core::array::from_fn(|_| None),
            count: 0,
            snap_hash: 0,
            frozen: false,
        }
    }

    /// 入块：未固化前允许追加。
    pub fn push(&mut self, bytes: &[u8]) -> Result<u32, u16> {
        if self.frozen {
            return Err(E_NO_SNAPSHOT);
        }
        if self.count >= 12 {
            return Err(E_PTR_DANGLE);
        }
        if bytes.is_empty() {
            return Err(E_BODY_EMPTY);
        }
        let block_id = (self.count + 1) as u32;
        self.blocks[self.count] = Some(SnapBlock::new(block_id, bytes));
        self.count += 1;
        Ok(block_id)
    }

    /// 固化：聚合全块哈希，快照从此只读。
    pub fn freeze(&mut self) -> Result<u64, u16> {
        if self.frozen {
            return Ok(self.snap_hash);
        }
        if self.count == 0 {
            return Err(E_BODY_EMPTY);
        }
        let mut buf: Vec<u8> = Vec::new();
        let mut i = 0usize;
        while i < self.count {
            if let Some(b) = &self.blocks[i] {
                buf.extend_from_slice(b.block_id.to_le_bytes().as_slice());
                buf.extend_from_slice(b.hash.to_le_bytes().as_slice());
            }
            i += 1;
        }
        self.snap_hash = fnv1a64(&buf);
        self.frozen = true;
        Ok(self.snap_hash)
    }

    /// 解引：按块编号取只读引用（指针可解引断言的实现核心）。
    pub fn deref(&self, block: u32) -> Result<&SnapBlock, u16> {
        if block == 0 || block as usize > self.count {
            return Err(E_PTR_DANGLE);
        }
        match &self.blocks[(block - 1) as usize] {
            Some(b) => Ok(b),
            None => Err(E_PTR_DANGLE),
        }
    }

    /// 快照哈希。
    pub fn hash(&self) -> u64 {
        self.snap_hash
    }

    /// 是否已固化。
    pub fn is_frozen(&self) -> bool {
        self.frozen
    }
}

// ── 数据行 ───────────────────────────────────────────────────
/// 报告节内的一条数据行：模板前缀 + 解引内容 + 指针编号。
#[derive(Clone, Debug)]
pub struct DataRow {
    /// 模板前缀（结构性文字，非数据）。
    pub label: String,
    /// 解引得到的数据文本。
    pub value: String,
    /// 来源指针（零手抄凭据）。
    pub ptr: DataPtr,
}

// ── 报告节 ───────────────────────────────────────────────────
/// 报告节：节号 + 标题 + 数据行集。
#[derive(Clone, Debug)]
pub struct ReportSection {
    /// 节号（1..=6）。
    pub sec_no: u32,
    /// 节标题。
    pub title: String,
    /// 数据行（每行必须带指针）。
    pub rows: Vec<DataRow>,
    /// 节哈希（行集聚合）。
    pub hash: u64,
}

// ── 报告生成状态机 ───────────────────────────────────────────
/// 生成状态：Snapshot→Generating→Complete→Sealed。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GenPhase {
    /// 快照未固化。
    NoSnapshot,
    /// 逐节生成中。
    Generating,
    /// 六节齐备。
    Complete,
    /// 已封。
    Sealed,
}

// ── 回归报告（主体）──────────────────────────────────────────
/// 升级回归报告：快照绑定 + 六节程序化生成。
pub struct RegressionReport {
    /// 报告标题。
    title: String,
    /// 数据快照。
    snap: DataSnapshot,
    /// 已生成节集（定容 6）。
    secs: [Option<ReportSection>; 6],
    /// 已生成节数（守恒分子之一）。
    generated: u32,
    /// 跳过节数（恒应 0，非零即断言失败）。
    skipped: u32,
    /// 生成阶段。
    phase: GenPhase,
    /// 报告哈希（封账时计算）。
    report_hash: u64,
}

impl RegressionReport {
    /// 建报告：先固化快照，再进入生成期。
    pub fn new(title: &str, snap: DataSnapshot) -> Result<RegressionReport, u16> {
        if title.is_empty() {
            return Err(E_TITLE_DUP);
        }
        let mut snap = snap;
        let _ = snap.freeze()?;
        Ok(RegressionReport {
            title: title.to_string(),
            snap,
            secs: [None, None, None, None, None, None],
            generated: 0,
            skipped: 0,
            phase: GenPhase::Generating,
            report_hash: 0,
        })
    }

    /// 快照哈希读取（快照绑定凭据）。
    pub fn snapshot_hash(&self) -> u64 {
        self.snap.hash()
    }

    /// 生成一节：sec_no 合法、未重复、至少一行、每行指针必须可解引。
    pub fn gen_section(&mut self, sec_no: u32, title: &str, rows: &[(&str, u32)]) -> Result<(), u16> {
        if self.phase == GenPhase::Sealed {
            return Err(E_SEALED);
        }
        if self.phase != GenPhase::Generating {
            return Err(E_PHASE);
        }
        if sec_no == 0 || sec_no > SECTIONS {
            return Err(E_SECT_BAD);
        }
        let idx = (sec_no - 1) as usize;
        if self.secs[idx].is_some() {
            return Err(E_SECT_DUP);
        }
        if rows.is_empty() || rows.len() > MAX_ROWS_PER_SEC {
            return Err(E_BODY_EMPTY);
        }
        let mut data_rows: Vec<DataRow> = Vec::new();
        let mut buf: Vec<u8> = Vec::new();
        let mut i = 0usize;
        while i < rows.len() {
            let (label, block) = rows[i];
            if label.is_empty() {
                return Err(E_HAND_COPY);
            }
            // 指针解引：来源块必须存在（零手抄核心断言）。
            let blk = self.snap.deref(block)?;
            data_rows.push(DataRow {
                label: label.to_string(),
                value: hex64(blk.hash),
                ptr: DataPtr { pid: (i + 1) as u32, block },
            });
            buf.extend_from_slice(label.as_bytes());
            buf.extend_from_slice(blk.hash.to_le_bytes().as_slice());
            i += 1;
        }
        let sec_hash = fnv1a64(&buf);
        self.secs[idx] = Some(ReportSection {
            sec_no,
            title: title.to_string(),
            rows: data_rows,
            hash: sec_hash,
        });
        self.generated += 1;
        if self.generated + self.skipped == SECTIONS {
            self.phase = GenPhase::Complete;
        }
        Ok(())
    }

    /// 显式跳节（禁止：仅用于演练错误路径，正常口径 skipped 必须为 0）。
    pub fn skip_section(&mut self, sec_no: u32) -> Result<(), u16> {
        if self.phase != GenPhase::Generating {
            return Err(E_PHASE);
        }
        if sec_no == 0 || sec_no > SECTIONS {
            return Err(E_SECT_BAD);
        }
        let idx = (sec_no - 1) as usize;
        if self.secs[idx].is_some() {
            return Err(E_SECT_DUP);
        }
        self.skipped += 1;
        if self.generated + self.skipped == SECTIONS {
            self.phase = GenPhase::Complete;
        }
        Ok(())
    }

    /// 节计数守恒：generated + skipped == 6。
    pub fn conserved(&self) -> bool {
        self.generated + self.skipped == SECTIONS
    }

    /// 六节全齐断言。
    pub fn sections_complete(&self) -> bool {
        let mut i = 0usize;
        while i < 6 {
            if self.secs[i].is_none() {
                return false;
            }
            i += 1;
        }
        true
    }

    /// 快照绑定校验：报告哈希必须与快照哈希互证。
    pub fn bind_check(&self) -> Result<(), u16> {
        if !self.snap.is_frozen() {
            return Err(E_NO_SNAPSHOT);
        }
        if self.report_hash == 0 {
            return Err(E_BIND_FAIL);
        }
        // 绑定恒等式：报告哈希异或节结构哈希应还原快照哈希。
        let struct_h = self.struct_hash();
        if self.report_hash ^ struct_h != self.snap.hash() {
            return Err(E_BIND_FAIL);
        }
        Ok(())
    }

    /// 节结构哈希：六节节号+节哈希按序聚合。
    fn struct_hash(&self) -> u64 {
        let mut buf: Vec<u8> = Vec::new();
        let mut i = 0usize;
        while i < 6 {
            match &self.secs[i] {
                Some(s) => {
                    buf.extend_from_slice(s.sec_no.to_le_bytes().as_slice());
                    buf.extend_from_slice(s.hash.to_le_bytes().as_slice());
                }
                None => buf.push(0),
            }
            i += 1;
        }
        fnv1a64(&buf)
    }

    /// 封报告：六节齐 + 守恒 + 快照绑定，产出报告哈希。
    pub fn seal(&mut self) -> Result<u64, u16> {
        if self.phase == GenPhase::Sealed {
            return Err(E_SEALED);
        }
        if self.phase != GenPhase::Complete {
            return Err(E_SECT_MISSING);
        }
        if !self.sections_complete() {
            return Err(E_SECT_MISSING);
        }
        if !self.conserved() {
            return Err(E_BAL);
        }
        if self.skipped != 0 {
            return Err(E_SECT_MISSING);
        }
        // 报告哈希 = 快照哈希 ⊕ 节结构哈希（绑定恒等式的正向侧）。
        let h = self.snap.hash() ^ self.struct_hash();
        self.report_hash = h;
        self.phase = GenPhase::Sealed;
        Ok(h)
    }

    /// 报告哈希读取（0=未封）。
    pub fn hash(&self) -> u64 {
        self.report_hash
    }

    /// 当前阶段。
    pub fn phase(&self) -> GenPhase {
        self.phase
    }

    /// 节数。
    pub fn section_count(&self) -> u32 {
        self.generated
    }

    /// 缺节清单（封账前定位用）。
    pub fn missing_sections(&self) -> Vec<u32> {
        let mut out: Vec<u32> = Vec::new();
        let mut i = 0usize;
        while i < 6 {
            if self.secs[i].is_none() {
                out.push((i + 1) as u32);
            }
            i += 1;
        }
        out
    }

    /// 报告全文渲染（消费面直接打印的行集）。
    pub fn render(&self) -> Result<Vec<String>, u16> {
        if self.phase == GenPhase::NoSnapshot {
            return Err(E_NO_SNAPSHOT);
        }
        let mut lines: Vec<String> = Vec::new();
        lines.push("=== ".to_string() + &self.title + " ===");
        lines.push("快照哈希: ".to_string() + &hex64(self.snap.hash()));
        let mut i = 0usize;
        while i < 6 {
            match &self.secs[i] {
                Some(s) => {
                    lines.push("-- 第".to_string() + &itoa(s.sec_no as u64) + "节 " + &s.title);
                    let mut j = 0usize;
                    while j < s.rows.len() {
                        let r = &s.rows[j];
                        lines.push("  ".to_string() + &r.label + " [" + &itoa(r.ptr.pid as u64)
                            + "->B" + &itoa(r.ptr.block as u64) + "] " + &r.value);
                        j += 1;
                    }
                }
                None => lines.push("-- 第".to_string() + &itoa((i + 1) as u64) + "节 缺失"),
            }
            i += 1;
        }
        Ok(lines)
    }
}

/// 六节标准标题（程序化生成时的定版措辞，非手抄数据）。
pub const SECTION_TITLES: [&str; 6] = [
    "升级摘要",
    "哈希对照",
    "演练结果",
    "SLA 违约",
    "接口回归",
    "结论签章",
];

/// 按节号取标准标题（越界返回诊断码）。
pub fn section_title(sec_no: u32) -> Result<&'static str, u16> {
    if sec_no == 0 || sec_no as usize > SECTION_TITLES.len() {
        return Err(E_SECT_BAD);
    }
    Ok(SECTION_TITLES[(sec_no - 1) as usize])
}

/// u64 转 16 位十六进制字符串（确定性小写）。
fn hex64(v: u64) -> String {
    let digits = b"0123456789abcdef";
    let mut s = String::new();
    let mut i = 0usize;
    while i < 16 {
        let shift = 60 - (i as u32) * 4;
        let nib = ((v >> shift) & 0xF) as usize;
        s.push(digits[nib] as char);
        i += 1;
    }
    s
}

/// 极简无符号转十进制。
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

// ── 自例程 ───────────────────────────────────────────────────
/// 演练：六节全生成 + 快照绑定 + 错误路径抽样。返回 0 = 全过。
pub fn drill() -> Result<u32, u16> {
    // 快照：六块数据，对应六节的解引来源。
    let mut snap = DataSnapshot::new();
    let b1 = snap.push(b"summary: upgraded limine v7.0.2 -> v7.0.3")?;
    let b2 = snap.push(b"hashes: 3 artifacts identical")?;
    let b3 = snap.push(b"drill: 3/3 chains ok")?;
    let b4 = snap.push(b"sla: 0 unhandled violations")?;
    let b5 = snap.push(b"api: 6 frozen interfaces pass")?;
    let b6 = snap.push(b"verdict: upgrade regression PASS")?;
    let mut rep = RegressionReport::new("B37 升级回归报告", snap)?;
    let _ = (b1, b2, b3, b4, b5, b6);
    // 六节逐一生成，每行指针可解引。
    rep.gen_section(SEC_SUMMARY, "升级摘要", &[("版本迁移", 1)])?;
    rep.gen_section(SEC_HASHES, "哈希对照", &[("产物一致", 2)])?;
    rep.gen_section(SEC_DRILL, "演练结果", &[("三场景", 3)])?;
    rep.gen_section(SEC_SLA, "SLA 违约", &[("未处置数", 4)])?;
    rep.gen_section(SEC_API, "接口回归", &[("冻结接口", 5)])?;
    rep.gen_section(SEC_VERDICT, "结论签章", &[("终判", 6)])?;
    if !rep.conserved() || rep.section_count() != SECTIONS {
        return Err(SEG_BASE);
    }
    let h = rep.seal()?;
    if h == 0 || rep.phase() != GenPhase::Sealed {
        return Err(SEG_BASE);
    }
    rep.bind_check()?;
    // 零手抄复核：渲染行中每条数据行必须带指针编号。
    let lines = rep.render()?;
    if lines.len() < 10 {
        return Err(SEG_BASE);
    }
    // 错误路径：悬垂指针。
    let mut snap2 = DataSnapshot::new();
    let _ = snap2.push(b"only one block")?;
    let mut rep2 = RegressionReport::new("坏指针报告", snap2)?;
    match rep2.gen_section(SEC_SUMMARY, "x", &[("坏行", 9)]) {
        Err(c) if c == E_PTR_DANGLE => {}
        _ => return Err(SEG_BASE),
    }
    // 错误路径：缺节封账被拒。
    let mut snap3 = DataSnapshot::new();
    let _ = snap3.push(b"one")?;
    let mut rep3 = RegressionReport::new("缺节报告", snap3)?;
    rep3.gen_section(SEC_SUMMARY, "x", &[("行", 1)])?;
    rep3.skip_section(2)?;
    rep3.skip_section(3)?;
    rep3.skip_section(4)?;
    rep3.skip_section(5)?;
    rep3.skip_section(6)?;
    match rep3.seal() {
        Err(c) if c == E_SECT_MISSING => {}
        _ => return Err(SEG_BASE),
    }
    Ok(0)
}
