//! UNX-F0749 · 移交包内容清单定版（六件套） · 诊断段 0x5ca0
//! 判据原文摘录：UNX-F0749-J1 清单六件齐（账册汇编/接口快照/ktest 封版/
//! 性能基线/域经卷/验收核对单）逐件内容描述 + 完备断言 6/6；
//! 完成时 API/软件写回：状态=已完成、结果摘要非空。纯功能代码 ≥570 行。
//!
//! 设计纪律：
//! 1. 六件套写死：账册汇编 / 接口快照 / ktest 封版 / 性能基线 / 域经卷 /
//!    验收核对单，件号 1..=6，序不可换，缺件即包不成立。
//! 2. 逐件内容描述：每件必挂内容描述（≥8 字节）与定容行数预算，
//!    描述缺失即件无效——清单不许有"无名件"。
//! 3. 件态机：Empty→Drafted→Verified 三态，合法边单向，Verified 不可回退。
//! 4. 完备断言 6/6：六件全部 Verified 才算清单齐备，缺一即拒绝封清单。
//! 5. 定容数组账本 + 守恒恒等式：drafted+verified+empty == 6。
//! 6. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。
//!    诊断段 0x5ca0 为本单独占。

use alloc::string::{String, ToString};
use alloc::vec::Vec;

pub const SEG_BASE: u16 = 0x5ca0;

// ── 诊断码闭集（SEG_BASE+1..+N）────────────────────────────────
const E_PIECE_BAD: u16 = SEG_BASE + 1; // 件号越界（1..=6）
const E_PIECE_TWICE: u16 = SEG_BASE + 2; // 件内容重复起草
const E_DESC_SHORT: u16 = SEG_BASE + 3; // 内容描述不足 8 字节
const E_DESC_TWICE: u16 = SEG_BASE + 4; // 描述改写（定版后禁改）
const E_BUDGET_ZERO: u16 = SEG_BASE + 5; // 行数预算为 0
const E_STATE_EDGE: u16 = SEG_BASE + 6; // 件态机非法边（回退或跳态）
const E_NOT_ALL: u16 = SEG_BASE + 7; // 六件未齐即封清单被拒
const E_VERIFY_LACK: u16 = SEG_BASE + 8; // 完备断言失败：未 Verified 件存在
const E_BAL: u16 = SEG_BASE + 9; // 守恒恒等式破裂（三态计数和≠6）
const E_SEALED: u16 = SEG_BASE + 10; // 清单已封，拒绝再写
const E_PIECE_NOTFOUND: u16 = SEG_BASE + 11; // 件不存在（未起草即核）
const E_ORDER_BAD: u16 = SEG_BASE + 12; // 件序错乱（核验必须按件号升序）
const E_HASH_ZERO: u16 = SEG_BASE + 13; // 件哈希为 0（核验未产出）
const E_TWICE_SEAL: u16 = SEG_BASE + 14; // 清单重复封

/// 六件套件号。
pub const P_CATALOG: u8 = 1; // 账册汇编
pub const P_API: u8 = 2; // 接口快照
pub const P_KTEST: u8 = 3; // ktest 封版
pub const P_PERF: u8 = 4; // 性能基线
pub const P_DOMEXP: u8 = 5; // 域经卷
pub const P_CHECK: u8 = 6; // 验收核对单
pub const PIECES: usize = 6;

/// 内容描述最小字节数。
pub const DESC_MIN: usize = 8;

// ── FNV-1a 64（件哈希与清单哈希口径）────────────────────────────
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
        E_PIECE_BAD => "件号越界：六件套 1..=6",
        E_PIECE_TWICE => "件内容重复起草：一件一份描述",
        E_DESC_SHORT => "内容描述不足 8 字节：写清楚件内容再登记",
        E_DESC_TWICE => "描述定版后禁改：改版须另立包版本",
        E_BUDGET_ZERO => "行数预算为 0：每件必须预留行数",
        E_STATE_EDGE => "件态机非法边：Empty→Drafted→Verified 单向",
        E_NOT_ALL => "六件未齐：六件套缺一不可",
        E_VERIFY_LACK => "存在未核验件：逐件核验后再封清单",
        E_BAL => "守恒破裂：三态计数之和必须为 6",
        E_SEALED => "清单已封：封账后只读",
        E_PIECE_NOTFOUND => "件未起草：先起草再核验",
        E_ORDER_BAD => "件序错乱：核验按件号 1..6 升序",
        E_HASH_ZERO => "件哈希为 0：核验必须产出哈希",
        E_TWICE_SEAL => "清单重复封",
        _ => "未知诊断码",
    }
}

// ── 件态机 ───────────────────────────────────────────────────
/// 件三态：Empty→Drafted→Verified，单向合法边。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PieceState {
    /// 空件。
    Empty,
    /// 已起草（有描述与预算）。
    Drafted,
    /// 已核验（哈希产出）。
    Verified,
}

impl PieceState {
    /// 编号（落账用）。
    pub fn code(self) -> u8 {
        match self {
            PieceState::Empty => 0,
            PieceState::Drafted => 1,
            PieceState::Verified => 2,
        }
    }

    /// 中文名。
    pub fn zh(self) -> &'static str {
        match self {
            PieceState::Empty => "空件",
            PieceState::Drafted => "已起草",
            PieceState::Verified => "已核验",
        }
    }
}

// ── 件行 ─────────────────────────────────────────────────────
/// 清单中的一件：件号 + 件名 + 内容描述 + 行数预算 + 件态。
#[derive(Clone, Debug)]
pub struct ManifestPiece {
    /// 件号（1..=6）。
    pub piece_no: u8,
    /// 件名。
    pub piece_name: String,
    /// 内容描述（≥8 字节）。
    pub desc: String,
    /// 行数预算（>0）。
    pub budget: u32,
    /// 件态。
    pub state: PieceState,
    /// 核验哈希（Verified 后非零）。
    pub verify_hash: u64,
}

impl ManifestPiece {
    /// 件有效性：描述齐 + 预算正。
    pub fn drafted_ok(&self) -> bool {
        self.desc.as_bytes().len() >= DESC_MIN && self.budget > 0
    }
}

// ── 移交包清单（主体）────────────────────────────────────────
/// 移交包内容清单：六件套逐件登记 → 逐件核验 → 完备断言 → 封清单。
pub struct PackageManifest {
    pieces: [Option<ManifestPiece>; PIECES],
    /// 三态计数。
    n_empty: u32,
    n_drafted: u32,
    n_verified: u32,
    /// 核验进度（下一件待核件号，升序推进）。
    verify_cursor: u8,
    /// 是否封清单。
    sealed: bool,
    /// 清单哈希（封后非零）。
    sealed_hash: u64,
}

impl PackageManifest {
    /// 开清单：六件全空。
    pub fn new() -> PackageManifest {
        PackageManifest {
            pieces: core::array::from_fn(|_| None),
            n_empty: PIECES as u32,
            n_drafted: 0,
            n_verified: 0,
            verify_cursor: 1,
            sealed: false,
            sealed_hash: 0,
        }
    }

    /// 起草一件：件号合法 + 描述达标 + 预算正 + 未起草过。
    pub fn draft(&mut self, piece_no: u8, name: &str, desc: &str, budget: u32) -> Result<(), u16> {
        if self.sealed {
            return Err(E_SEALED);
        }
        if piece_no == 0 || piece_no as usize > PIECES {
            return Err(E_PIECE_BAD);
        }
        if name.is_empty() {
            return Err(E_PIECE_BAD);
        }
        if desc.as_bytes().len() < DESC_MIN {
            return Err(E_DESC_SHORT);
        }
        if budget == 0 {
            return Err(E_BUDGET_ZERO);
        }
        let idx = (piece_no - 1) as usize;
        if self.pieces[idx].is_some() {
            return Err(E_PIECE_TWICE);
        }
        self.pieces[idx] = Some(ManifestPiece {
            piece_no,
            piece_name: name.to_string(),
            desc: desc.to_string(),
            budget,
            state: PieceState::Drafted,
            verify_hash: 0,
        });
        self.n_empty -= 1;
        self.n_drafted += 1;
        Ok(())
    }

    /// 描述定版改写检测：Drafted/Verified 件的描述内容必须与登记一致。
    pub fn desc_unchanged(&self, piece_no: u8, desc: &str) -> Result<(), u16> {
        if piece_no == 0 || piece_no as usize > PIECES {
            return Err(E_PIECE_BAD);
        }
        match &self.pieces[(piece_no - 1) as usize] {
            Some(p) => {
                if p.desc != desc {
                    return Err(E_DESC_TWICE);
                }
                Ok(())
            }
            None => Err(E_PIECE_NOTFOUND),
        }
    }

    /// 逐件核验：按件号升序推进（乱序拒绝），产出件哈希。
    pub fn verify_next(&mut self, payload: &[u8]) -> Result<u64, u16> {
        if self.sealed {
            return Err(E_SEALED);
        }
        if payload.is_empty() {
            return Err(E_HASH_ZERO);
        }
        if self.verify_cursor as usize > PIECES {
            return Err(E_STATE_EDGE);
        }
        let idx = (self.verify_cursor - 1) as usize;
        let p = match &mut self.pieces[idx] {
            Some(p) => p,
            None => return Err(E_PIECE_NOTFOUND),
        };
        if p.state != PieceState::Drafted {
            return Err(E_STATE_EDGE);
        }
        if !p.drafted_ok() {
            return Err(E_DESC_SHORT);
        }
        let mut buf: Vec<u8> = Vec::new();
        buf.extend_from_slice(p.piece_no.to_le_bytes().as_slice());
        buf.extend_from_slice(p.piece_name.as_bytes());
        buf.extend_from_slice(p.desc.as_bytes());
        buf.extend_from_slice(payload);
        let h = fnv1a64(&buf);
        if h == 0 {
            return Err(E_HASH_ZERO);
        }
        p.state = PieceState::Verified;
        p.verify_hash = h;
        self.n_drafted -= 1;
        self.n_verified += 1;
        self.verify_cursor += 1;
        Ok(h)
    }

    /// 件只读快照。
    pub fn get(&self, piece_no: u8) -> Result<(String, PieceState, u64), u16> {
        if piece_no == 0 || piece_no as usize > PIECES {
            return Err(E_PIECE_BAD);
        }
        match &self.pieces[(piece_no - 1) as usize] {
            Some(p) => Ok((p.piece_name.clone(), p.state, p.verify_hash)),
            None => Err(E_PIECE_NOTFOUND),
        }
    }

    /// 守恒恒等式：三态计数和恒等于 6。
    pub fn conserved(&self) -> bool {
        self.n_empty + self.n_drafted + self.n_verified == PIECES as u32
    }

    /// 完备断言 6/6：六件全 Verified 且守恒。
    pub fn completeness_assert(&self) -> Result<(), u16> {
        if !self.conserved() {
            return Err(E_BAL);
        }
        if self.n_verified != PIECES as u32 {
            return Err(E_VERIFY_LACK);
        }
        let mut i = 0usize;
        while i < PIECES {
            match &self.pieces[i] {
                Some(p) => {
                    if p.state != PieceState::Verified || p.verify_hash == 0 {
                        return Err(E_VERIFY_LACK);
                    }
                }
                None => return Err(E_VERIFY_LACK),
            }
            i += 1;
        }
        Ok(())
    }

    /// 封清单：完备断言过 → 清单哈希定版。
    pub fn seal(&mut self) -> Result<u64, u16> {
        if self.sealed {
            return Err(E_TWICE_SEAL);
        }
        self.completeness_assert()?;
        let mut buf: Vec<u8> = Vec::new();
        let mut i = 0usize;
        while i < PIECES {
            match &self.pieces[i] {
                Some(p) => {
                    buf.push(p.piece_no);
                    buf.extend_from_slice(p.piece_name.as_bytes());
                    buf.push(0x1f);
                    buf.extend_from_slice(p.verify_hash.to_le_bytes().as_slice());
                }
                None => return Err(E_NOT_ALL),
            }
            i += 1;
        }
        let h = fnv1a64(&buf);
        self.sealed_hash = h;
        self.sealed = true;
        Ok(h)
    }

    /// 清单哈希。
    pub fn sealed_hash(&self) -> u64 {
        self.sealed_hash
    }

    /// 是否封清单。
    pub fn is_sealed(&self) -> bool {
        self.sealed
    }

    /// 三态分布读取。
    pub fn state_tally(&self) -> (u32, u32, u32) {
        (self.n_empty, self.n_drafted, self.n_verified)
    }

    /// 清单行渲染：逐件"件号|件名|态"。
    pub fn manifest_lines(&self) -> Vec<String> {
        let mut lines: Vec<String> = Vec::new();
        lines.push("=== 移交包内容清单 · 六件套 ===".to_string());
        let mut i = 0usize;
        while i < PIECES {
            match &self.pieces[i] {
                Some(p) => {
                    let mut line = String::from("件");
                    line.push_str(&itoa(p.piece_no as u64));
                    line.push('|');
                    line.push_str(&p.piece_name);
                    line.push('|');
                    line.push_str(p.state.zh());
                    line.push_str("|预算");
                    line.push_str(&itoa(p.budget as u64));
                    lines.push(line);
                }
                None => {
                    let mut line = String::from("件");
                    line.push_str(&itoa((i + 1) as u64));
                    line.push_str("|未登记|空件");
                    lines.push(line);
                }
            }
            i += 1;
        }
        lines
    }

    /// 行数预算总和（清单规模取数口）。
    pub fn budget_total(&self) -> u32 {
        let mut s = 0u32;
        let mut i = 0usize;
        while i < PIECES {
            if let Some(p) = &self.pieces[i] {
                s += p.budget;
            }
            i += 1;
        }
        s
    }

    /// 六件哈希链核验：chain[i] = fnv(chain[i-1] ‖ verify_hash[i])，
    /// 清单封后用于逐环复算，任一件核验哈希被篡改即断链。
    pub fn hash_chain_verify(&self) -> Result<u64, u16> {
        if self.n_verified != PIECES as u32 {
            return Err(E_VERIFY_LACK);
        }
        let mut prev: u64 = 0;
        let mut i = 0usize;
        while i < PIECES {
            let p = self.pieces[i].as_ref().ok_or(E_PIECE_NOTFOUND)?;
            let mut ib: Vec<u8> = Vec::new();
            ib.extend_from_slice(prev.to_le_bytes().as_slice());
            ib.extend_from_slice(p.verify_hash.to_le_bytes().as_slice());
            prev = fnv1a64(&ib);
            i += 1;
        }
        Ok(prev)
    }

    /// 核验进度：已完成核验件数（升序口径）。
    pub fn verified_count(&self) -> u32 {
        self.n_verified
    }

    /// 件描述只读快照（消费方核对内容用）。
    pub fn desc_of(&self, piece_no: u8) -> Result<String, u16> {
        if piece_no == 0 || piece_no as usize > PIECES {
            return Err(E_PIECE_BAD);
        }
        match &self.pieces[(piece_no - 1) as usize] {
            Some(p) => Ok(p.desc.clone()),
            None => Err(E_PIECE_NOTFOUND),
        }
    }

    /// 描述长度直方图：len/8 桶计数（描述质量分布取数口）。
    pub fn desc_len_tally(&self) -> [u32; 8] {
        let mut tally = [0u32; 8];
        let mut i = 0usize;
        while i < PIECES {
            if let Some(p) = &self.pieces[i] {
                let l = p.desc.as_bytes().len();
                let bucket = l / 8;
                if bucket < 8 {
                    tally[bucket] += 1;
                }
            }
            i += 1;
        }
        tally
    }

    /// 描述字节总量（六件描述规模合计）。
    pub fn desc_bytes_total(&self) -> u32 {
        let mut s = 0u32;
        let mut i = 0usize;
        while i < PIECES {
            if let Some(p) = &self.pieces[i] {
                s += p.desc.as_bytes().len() as u32;
            }
            i += 1;
        }
        s
    }

    /// 六件套件名表只读快照。
    pub fn piece_names(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        let mut i = 0usize;
        while i < PIECES {
            match &self.pieces[i] {
                Some(p) => out.push(p.piece_name.clone()),
                None => out.push(String::new()),
            }
            i += 1;
        }
        out
    }
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

// ── 标准六件定版描述（判据口径的登记样本）─────────────────────
/// 六件套标准件名。
pub const STANDARD_NAMES: [&str; 6] = [
    "账册汇编",
    "接口快照",
    "ktest 封版",
    "性能基线",
    "域经卷",
    "验收核对单",
];

/// 六件套标准描述（定版内容描述）。
pub const STANDARD_DESCS: [&str; 6] = [
    "批册40+深化册40逐册指针与域账累计链终点240000",
    "六冻结接口终版字段表与F0736回归账一致快照",
    "全域断言清单快照300条与批断言集映射齐",
    "T0T3四锚点三机终版与阈值3000ms判定凭证",
    "高中档条目卷与登记簿一致且低档零混入",
    "六验收判据逐项凭证指针与核对结论三列",
];

/// 六件套标准行数预算（定版）。
pub const STANDARD_BUDGETS: [u32; 6] = [280, 260, 300, 240, 260, 240];

/// 标准六件一键起草（定版名/描述/预算）。
pub fn enroll_standard(mf: &mut PackageManifest) -> Result<(), u16> {
    let mut i = 0usize;
    while i < PIECES {
        mf.draft(
            (i + 1) as u8,
            STANDARD_NAMES[i],
            STANDARD_DESCS[i],
            STANDARD_BUDGETS[i],
        )?;
        i += 1;
    }
    Ok(())
}

/// 逐件核验至六件全 Verified（标准载荷 = 件号派生字节）。
pub fn verify_all_standard(mf: &mut PackageManifest) -> Result<[u64; PIECES], u16> {
    let mut hashes = [0u64; PIECES];
    let mut i = 0usize;
    while i < PIECES {
        let payload = [b'S', (i + 1) as u8, 0xAB];
        hashes[i] = mf.verify_next(&payload)?;
        i += 1;
    }
    Ok(hashes)
}

// ── 自例程 ───────────────────────────────────────────────────
/// 演练：六件齐 6/6 + 守恒 + 封清单 + 错误路径。返回 0 = 全过。
pub fn drill() -> Result<u32, u16> {
    let mut mf = PackageManifest::new();
    // 一键起草六件（标准定版）。
    enroll_standard(&mut mf)?;
    // 预算总和：240+260+300+240+260+240 的定版合计核对。
    if mf.budget_total() != 1580 {
        return Err(SEG_BASE);
    }
    let (e0, d0, v0) = mf.state_tally();
    if e0 != 0 || d0 != 6 || v0 != 0 {
        return Err(SEG_BASE);
    }
    if !mf.conserved() {
        return Err(SEG_BASE);
    }
    // 描述防改核。
    mf.desc_unchanged(1, STANDARD_DESCS[0])?;
    match mf.desc_unchanged(1, "被篡改的描述内容") {
        Err(c) if c == E_DESC_TWICE => {}
        _ => return Err(SEG_BASE),
    }
    // 逐件核验（升序，标准载荷）。
    let hashes = verify_all_standard(&mut mf)?;
    let mut i = 0usize;
    while i < PIECES {
        if hashes[i] == 0 {
            return Err(SEG_BASE);
        }
        i += 1;
    }
    mf.completeness_assert()?;
    let chain = mf.hash_chain_verify()?;
    if chain == 0 {
        return Err(SEG_BASE);
    }
    // 描述取数口：字节总量与件名表。
    if mf.desc_bytes_total() == 0 {
        return Err(SEG_BASE);
    }
    let names = mf.piece_names();
    if names.len() != 6 || names[0] != STANDARD_NAMES[0] {
        return Err(SEG_BASE);
    }
    let d1 = mf.desc_of(1)?;
    if d1 != STANDARD_DESCS[0] {
        return Err(SEG_BASE);
    }
    let sealed = mf.seal()?;
    if sealed == 0 || !mf.is_sealed() {
        return Err(SEG_BASE);
    }
    // 封清单后写拒绝。
    match mf.draft(1, "再写", "超出六件套定版范围", 10) {
        Err(c) if c == E_SEALED => {}
        _ => return Err(SEG_BASE),
    }
    // 错误路径：缺件核验（未起草即核）。
    let mut mf2 = PackageManifest::new();
    match mf2.verify_next(b"x") {
        Err(c) if c == E_PIECE_NOTFOUND => {}
        _ => return Err(SEG_BASE),
    }
    // 错误路径：描述过短。
    match mf2.draft(1, "短描述件", "太短", 10) {
        Err(c) if c == E_DESC_SHORT => {}
        _ => return Err(SEG_BASE),
    }
    // 错误路径：未齐即封。
    let _ = mf2.draft(1, STANDARD_NAMES[0], STANDARD_DESCS[0], 100)?;
    match mf2.seal() {
        Err(c) if c == E_VERIFY_LACK || c == E_BAL => {}
        _ => return Err(SEG_BASE),
    }
    Ok(0)
}
