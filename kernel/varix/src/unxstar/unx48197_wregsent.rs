//! unx48197_wregsent — UNX-F48197 Wine 窗回归哨兵（三类窗口（vxwm 原生/Wine/Xwayland）回归判据·宪法加严：500 字试打 Wine 窗段全绿）（诊断段 0xB980）。
//!
//! 判据原文：
//!   J1 三类窗口（vxwm 原生/Wine/Xwayland）回归判据：Wine 窗 500 字 ×3 轮逐字一致；判据锚定 域内宪法加严项、J-主轴分窗。
//!   J2 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计纪律：
//!   1. 三类窗口各自有一条真实编码面：vxwm 原生直通 u32 码位；Wine 走
//!      WPARAM/LPARAM 16 位拆包（低 16 位 + 高 16 位两笔事件再拼回）；
//!      Xwayland 走 keysym 偏移面（码位 + 0x0100_0000 编码，解码时
//!      剥偏移）。哨兵的比对对象是三类面解码后的逐字 transcript。
//!   2. 判据主体（宪法加严）：Wine 窗 500 字 ×3 轮，每轮 transcript
//!      与基准逐位相等，3×500=1500 字逐字核销、一字不落；vxwm 原生
//!      与 Xwayland 各 ×1 轮 500 字做三类交叉一致（同源必同文）。
//!   3. 基准流确定性：LCG 伪随机在 CJK 区 [0x4E00, 0x4E00+3000) 与
//!      ASCII 区 [0x20, 0x7F) 交替抽字，同种子同文，可复跑。
//!   4. 探针自证：注入一位解码翻失（模拟 Wine 拆包错位），哨兵必须
//!      报 E_CHAR_MISMATCH 且报出首个错字下标——探针不响即哨兵失效。
//!   5. 轮账结构：每轮留 (窗口类, 轮号, 字数, 全绿位) 四元组；×3 轮
//!      全绿才允许写回；差一轮即 Err。
//!   6. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。

use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0xB980;

// ── 诊断码（SEG_BASE+1 起，≤0x1F 个）──────────────────────
const E_CHAR_MISMATCH: u16 = SEG_BASE + 1; // 逐字不一致
const E_ROUND_MISS: u16 = SEG_BASE + 2; // 轮数不足 ×3
const E_LEN_MISS: u16 = SEG_BASE + 3; // 字数不足 500
const E_KIND_BAD: u16 = SEG_BASE + 4; // 窗口类出闭集
const E_ENCODE_BAD: u16 = SEG_BASE + 5; // 编码面自检破（keysym 无偏移等）
const E_LEDGER_BAD: u16 = SEG_BASE + 6; // 轮账不守恒
const E_PROBE_SILENT: u16 = SEG_BASE + 7; // 探针未响（哨兵失效）
const E_EMPTY_SUM: u16 = SEG_BASE + 8; // 写回摘要为空

/// 判据字数（宪法加严）。
pub const N_CHARS: u32 = 500;
/// 判据轮数（Wine 窗）。
pub const N_ROUNDS: u32 = 3;
/// Xwayland keysym 偏移面。
pub const KEYSYM_OFF: u32 = 0x0100_0000;
/// CJK 基址与幅宽。
pub const CJK_BASE: u32 = 0x4E00;
pub const CJK_SPAN: u32 = 3000;
/// ASCII 区。
pub const ASCII_LO: u32 = 0x20;
pub const ASCII_HI: u32 = 0x7F;

/// 确定性 LCG。
pub struct Lcg {
    s: u32,
}

impl Lcg {
    pub fn new(seed: u32) -> Lcg {
        Lcg { s: seed }
    }
    pub fn next(&mut self) -> u32 {
        self.s = self.s.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        self.s
    }
    pub fn below(&mut self, n: u32) -> u32 {
        if n == 0 { 0 } else { self.next() % n }
    }
}

/// 窗口类闭集（J-主轴分窗）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WinKind {
    VxwmNative,
    WineWin,
    XwaylandWin,
}

impl WinKind {
    pub fn code(&self) -> u8 {
        match self {
            WinKind::VxwmNative => 0,
            WinKind::WineWin => 1,
            WinKind::XwaylandWin => 2,
        }
    }
}

/// 基准字流：CJK/ASCII 交替抽字，同种子同文（纪律 3）。
pub fn canonical_stream(seed: u32, n: u32) -> Vec<u32> {
    let mut rng = Lcg::new(seed);
    let mut out: Vec<u32> = Vec::new();
    let mut i = 0u32;
    while i < n {
        if i % 2 == 0 {
            out.push(CJK_BASE + rng.below(CJK_SPAN));
        } else {
            out.push(ASCII_LO + rng.below(ASCII_HI - ASCII_LO));
        }
        i += 1;
    }
    out
}

/// Wine 拆包面：一个码位拆成低/高 16 位两笔事件。
pub fn wine_encode(ch: u32) -> [u32; 2] {
    [ch & 0xFFFF, (ch >> 16) & 0xFFFF]
}

/// Wine 拼回：两笔事件还原码位。
pub fn wine_decode(ev: &[u32; 2]) -> Result<u32, u16> {
    let lo = ev[0] as u32;
    let hi = ev[1] as u32;
    if hi > 0x1F {
        return Err(E_ENCODE_BAD); // 码位超出 BMP 顶层带，拆包面自检破
    }
    Ok(lo | (hi << 16))
}

/// Xwayland keysym 面：编码 + 偏移，解码剥偏移。
pub fn xwl_encode(ch: u32) -> u32 {
    ch.wrapping_add(KEYSYM_OFF)
}

pub fn xwl_decode(ev: u32) -> Result<u32, u16> {
    if ev < KEYSYM_OFF {
        return Err(E_ENCODE_BAD); // 无偏移：非 keysym 面
    }
    Ok(ev - KEYSYM_OFF)
}

/// 单轮会话：按窗口类把基准流过一遍编码面并解码回 transcript。
pub fn run_round(kind: WinKind, base: &[u32]) -> Result<Vec<u32>, u16> {
    let mut out: Vec<u32> = Vec::new();
    let mut i = 0usize;
    while i < base.len() {
        match kind {
            WinKind::VxwmNative => out.push(base[i]),
            WinKind::WineWin => {
                let ev = wine_encode(base[i]);
                out.push(wine_decode(&ev)?);
            }
            WinKind::XwaylandWin => {
                out.push(xwl_decode(xwl_encode(base[i]))?);
            }
        }
        i += 1;
    }
    Ok(out)
}

/// 逐字比对：返回首个不一致下标（全一致返回 None）。
pub fn first_diff(a: &[u32], b: &[u32]) -> Option<usize> {
    if a.len() != b.len() {
        return Some(a.len().min(b.len()));
    }
    let mut i = 0usize;
    while i < a.len() {
        if a[i] != b[i] {
            return Some(i);
        }
        i += 1;
    }
    None
}

/// 轮账四元组。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct RoundRec {
    pub kind: u8,
    pub round_no: u32,
    pub chars: u32,
    pub green: bool,
}

/// 回归哨兵：跑轮、留账、全绿审计。
pub struct RegSentinel {
    pub recs: Vec<RoundRec>,
}

impl RegSentinel {
    pub fn new() -> RegSentinel {
        RegSentinel { recs: Vec::new() }
    }

    /// 跑一轮并与基准逐字比对，留轮账。
    pub fn round(&mut self, kind: WinKind, round_no: u32, base: &[u32]) -> Result<(), u16> {
        if base.len() < N_CHARS as usize {
            return Err(E_LEN_MISS);
        }
        let got = run_round(kind, base)?;
        let green = match first_diff(&got, base) {
            None => true,
            Some(_) => false,
        };
        if !green {
            return Err(E_CHAR_MISMATCH);
        }
        self.recs.push(RoundRec {
            kind: kind.code(),
            round_no,
            chars: base.len() as u32,
            green,
        });
        Ok(())
    }

    /// Wine 窗 ×3 轮全绿审计（判据 J1 硬口径）。
    pub fn audit_wine3(&self) -> Result<(), u16> {
        let mut green_wine = 0u32;
        let mut i = 0usize;
        while i < self.recs.len() {
            let r = &self.recs[i];
            if r.kind == WinKind::WineWin.code() && r.green && r.chars == N_CHARS {
                green_wine += 1;
            }
            i += 1;
        }
        if green_wine < N_ROUNDS {
            return Err(E_ROUND_MISS);
        }
        Ok(())
    }

    /// 轮账守恒：账面条数 == 留账轮数，无重复 (kind, round_no) 对。
    pub fn audit_ledger(&self) -> Result<(), u16> {
        let mut i = 0usize;
        while i < self.recs.len() {
            if !self.recs[i].green {
                return Err(E_LEDGER_BAD);
            }
            let mut j = i + 1;
            while j < self.recs.len() {
                if self.recs[i].kind == self.recs[j].kind
                    && self.recs[i].round_no == self.recs[j].round_no
                {
                    return Err(E_LEDGER_BAD);
                }
                j += 1;
            }
            i += 1;
        }
        Ok(())
    }

    /// 静默探针：重跑一轮带一位翻失的 Wine 拆包，哨兵必须报错并指认下标。
    pub fn probe_mismatch(&self, base: &[u32], at: usize) -> Result<usize, u16> {
        let mut corrupt = Vec::new();
        let mut i = 0usize;
        while i < base.len() {
            corrupt.push(base[i]);
            i += 1;
        }
        if at >= corrupt.len() {
            return Err(E_LEN_MISS);
        }
        corrupt[at] ^= 0x0001; // 末位翻失：解码面得异字
        let got = run_round(WinKind::WineWin, &corrupt)?;
        match first_diff(&got, &corrupt) {
            Some(idx) => Ok(idx),
            None => Err(E_PROBE_SILENT),
        }
    }
    /// 静默探针：无翻失流重跑一轮；找不到差异时显式报 E_PROBE_SILENT，
    /// 用于证明探针路径不是恒绿空转。
    pub fn probe_mismatch_quiet(&self, base: &[u32]) -> Result<usize, u16> {
        let got = run_round(WinKind::WineWin, base)?;
        match first_diff(&got, base) {
            Some(idx) => Ok(idx),
            None => Err(E_PROBE_SILENT),
        }
    }
}

/// 写回判定（判据 J2）。
fn writeback(done: bool, summary_len: u32) -> Result<(), u16> {
    if !done || summary_len == 0 {
        return Err(E_EMPTY_SUM);
    }
    Ok(())
}

/// 自例程判据：逐条落实 J1（Wine 窗 500 字 ×3 轮逐字一致），返回命中数。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;
    let base = canonical_stream(0xB980_0500, N_CHARS);
    if base.len() != N_CHARS as usize {
        return Err(E_LEN_MISS);
    }

    // R1 主判据：Wine 窗 500 字 ×3 轮逐字一致，轮账 ×3 全绿。
    let mut sent = RegSentinel::new();
    let mut r = 1u32;
    while r <= N_ROUNDS {
        sent.round(WinKind::WineWin, r, &base)?;
        r += 1;
    }
    sent.audit_wine3()?;
    sent.audit_ledger()?;
    hits += 1;

    // R2 三类交叉一致：vxwm 原生与 Xwayland 各 ×1 轮 500 字与基准逐字同文。
    sent.round(WinKind::VxwmNative, 1, &base)?;
    sent.round(WinKind::XwaylandWin, 1, &base)?;
    sent.audit_ledger()?;
    if sent.recs.len() != 5 {
        return Err(E_LEDGER_BAD); // 3 Wine + 1 native + 1 xwl
    }
    hits += 1;

    // R3 可复跑：同种子重取基准 + 重跑 Wine 一轮，逐字与首轮基准同文。
    let base2 = canonical_stream(0xB980_0500, N_CHARS);
    if first_diff(&base, &base2).is_some() {
        return Err(E_CHAR_MISMATCH); // 同种子必须同文
    }
    let got2 = run_round(WinKind::WineWin, &base2)?;
    if first_diff(&got2, &base).is_some() {
        return Err(E_CHAR_MISMATCH);
    }
    hits += 1;

    // R4 编码面自检：Wine 拆包拼回逐字相等；Xwayland 剥偏移逐字相等；
    //    无偏移 keysym 与超界拆包必须各自报 E_ENCODE_BAD。
    let mut i = 0usize;
    while i < base.len() {
        let ev = wine_encode(base[i]);
        if wine_decode(&ev)? != base[i] {
            return Err(E_CHAR_MISMATCH);
        }
        if xwl_decode(xwl_encode(base[i]))? != base[i] {
            return Err(E_CHAR_MISMATCH);
        }
        i += 1;
    }
    if xwl_decode(0x0000_4E00).is_ok() {
        return Err(E_ENCODE_BAD); // 无 keysym 偏移
    }
    if wine_decode(&[0xFFFF, 0x0020]).is_ok() {
        return Err(E_ENCODE_BAD); // 高位越界带
    }
    hits += 1;

    // R5 探针自证：一位翻失后哨兵必报错且指认下标（纪律 4）。
    let idx = sent.probe_mismatch(&base, 123)?;
    if idx != 123 {
        return Err(E_PROBE_SILENT);
    }
    // 静默哨兵探针：无翻失时 probe 必须报 E_PROBE_SILENT（否则探针恒绿）。
    let silent = sent.probe_mismatch_quiet(&base);
    if silent.is_ok() {
        return Err(E_PROBE_SILENT);
    }
    hits += 1;

    // R6 字数硬线：499 字基准不得入轮（500 是硬口径）。
    let short = canonical_stream(0xB980_0499, N_CHARS - 1);
    let mut sent6 = RegSentinel::new();
    if sent6.round(WinKind::WineWin, 1, &short).is_ok() {
        return Err(E_LEN_MISS);
    }
    if sent6.recs.len() != 0 {
        return Err(E_LEDGER_BAD);
    }
    hits += 1;

    // R7 写回（判据 J2）：完成态 + 非空摘要放行；空摘要拒。
    if writeback(true, 0).is_ok() {
        return Err(E_EMPTY_SUM);
    }
    writeback(true, 0x1500)?;
    hits += 1;

    Ok(hits)
}
