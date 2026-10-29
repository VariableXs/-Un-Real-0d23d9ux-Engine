//! UNX-F48577 —— I29 异常呈现三要素（联签失配显性化）
//! 判据原文：UNX-F48577-J1 I29 异常呈现固定三要素：
//! 一、联签失配必须可检出（失配检出位）；二、失配必须可呈现
//! （失配提示位）；三、提示必须用户可见（可见性位）。
//! 任一要素缺失即异常呈现失败；要素文案非空、呈现账守恒、
//! 指纹与呈现行可复核、未失配时不得误呈现。
//! 判据落地（drill 场景）：R1 三要素闭集；R2 失配检出位；
//! R3 失配提示位；R4 用户可见位；R5 三要素齐全判定；
//! R6 未失配不误呈现；R7 文案闭集非空；R8 呈现账守恒；
//! R9 指纹稳定；R10 呈现行齐全。
//! 设计纪律：no_std；零 unwrap/expect/panic；确定性 LCG；drill 判据全绿才返回 Ok。
use crate::alloc::vec::Vec;
use crate::alloc::vec;
use alloc::string::{String, ToString};

/// 诊断段基址（独立 0x20 窗）。
pub const SEG_BASE: u16 = 0xF800;

// ── 诊断码闭集（SEG_BASE+1 起连续，≤ SEG_BASE+0x1F）─────────────
const E_ELEM: u16 = SEG_BASE + 1; // 要素越出三要素闭集
const E_DET: u16 = SEG_BASE + 2; // 失配检出位缺失
const E_PROMPT: u16 = SEG_BASE + 3; // 失配提示位缺失
const E_VIS: u16 = SEG_BASE + 4; // 用户可见位缺失
const E_INCOMP: u16 = SEG_BASE + 5; // 三要素不齐仍判通过
const E_FALSE: u16 = SEG_BASE + 6; // 未失配误呈现
const E_TEXT: u16 = SEG_BASE + 7; // 文案闭集异常
const E_CONSERV: u16 = SEG_BASE + 8; // 呈现账不守恒
const E_FP: u16 = SEG_BASE + 9; // 指纹漂移
const E_LINE: u16 = SEG_BASE + 10; // 呈现行缺段

/// 失配类别闭集：1 冻结面失配 / 2 执行面失配。
pub const M_FROZEN: u8 = 1;
pub const M_EXEC: u8 = 2;
/// 类别名（下标 = 类别-1）。
pub const CAUSE_NAMES: [&str; 2] = ["frozen-mismatch", "exec-mismatch"];

/// FNV-1a 64 位（与本域其余哨兵同口径）。
pub fn fnv1a64(data: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in data {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
    }
    h
}

/// 确定性 LCG（批内统一系数）。
pub struct Lcg {
    state: u32,
}

impl Lcg {
    pub fn new(seed: u32) -> Lcg {
        Lcg { state: seed }
    }
    pub fn next_u32(&mut self) -> u32 {
        self.state = self.state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        self.state
    }
    pub fn below(&mut self, bound: u32) -> u32 {
        if bound == 0 {
            0
        } else {
            self.next_u32() % bound
        }
    }
}

/// 一笔异常呈现：类别 + 三要素位。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Present {
    pub cause: u8,
    pub det_bit: bool,
    pub prompt_bit: bool,
    pub vis_bit: bool,
}

/// 三要素齐全判定：类别对应检出/提示位 + 可见位必须同置。
pub fn complete(p: &Present) -> bool {
    let det_prompt = match p.cause {
        M_FROZEN => p.det_bit && p.prompt_bit,
        M_EXEC => p.det_bit && p.prompt_bit,
        _ => false,
    };
    det_prompt && p.vis_bit
}

/// 文案闭集：两类别各一条非空文案。
pub fn cause_text(cause: u8) -> Option<&'static str> {
    match cause {
        M_FROZEN => Some("joint sign mismatch: frozen plane signature rejected"),
        M_EXEC => Some("joint sign mismatch: exec plane signature rejected"),
        _ => None,
    }
}

/// 呈现账：入账笔数 + 可见笔数 + 类别计数。
pub struct PresBook {
    pub total: u32,
    pub visible: u32,
    pub frozen_count: u32,
    pub exec_count: u32,
}

impl PresBook {
    pub fn new() -> PresBook {
        PresBook {
            total: 0,
            visible: 0,
            frozen_count: 0,
            exec_count: 0,
        }
    }

    /// 入账一笔：类别闭集、文案非空、三要素齐全才可入账；
    /// 可见位与 visible 计数联动。
    pub fn put(&mut self, p: Present) -> Result<(), u16> {
        if p.cause != M_FROZEN && p.cause != M_EXEC {
            return Err(E_ELEM);
        }
        if !complete(&p) {
            return Err(E_INCOMP);
        }
        match cause_text(p.cause) {
            Some(t) => {
                if t.is_empty() {
                    return Err(E_TEXT);
                }
            }
            None => return Err(E_ELEM),
        }
        self.total += 1;
        if p.vis_bit {
            self.visible += 1;
        }
        if p.cause == M_FROZEN {
            self.frozen_count += 1;
        } else {
            self.exec_count += 1;
        }
        Ok(())
    }

    /// 守恒：total == frozen_count + exec_count；visible ≤ total。
    pub fn conserved(&self) -> bool {
        self.total == self.frozen_count + self.exec_count && self.visible <= self.total
    }
}

/// 主巡检入口：R1..R10 全绿才返回 Ok(hits)。
pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;

    // R1 三要素闭集：complete() 对三要素组合的判定自洽。
    let base = Present { cause: M_FROZEN, det_bit: false, prompt_bit: false, vis_bit: false };
    if complete(&base) {
        return Err(E_ELEM);
    }
    let only_vis = Present { cause: M_EXEC, det_bit: false, prompt_bit: false, vis_bit: true };
    if complete(&only_vis) {
        return Err(E_ELEM);
    }
    let half = Present { cause: M_FROZEN, det_bit: true, prompt_bit: false, vis_bit: true };
    if complete(&half) {
        return Err(E_ELEM);
    }
    hits += 1;

    // R2 失配检出位：det_bit 缺失被拒，码恰为 E_INCOMP。
    let mut bk = PresBook::new();
    let bad_det = Present { cause: M_FROZEN, det_bit: false, prompt_bit: true, vis_bit: true };
    match bk.put(bad_det) {
        Err(E_INCOMP) => hits += 1,
        _ => return Err(E_DET),
    }

    // R3 失配提示位：prompt_bit 缺失被拒，码恰为 E_INCOMP。
    let bad_prompt = Present { cause: M_EXEC, det_bit: true, prompt_bit: false, vis_bit: true };
    match bk.put(bad_prompt) {
        Err(E_INCOMP) => hits += 1,
        _ => return Err(E_PROMPT),
    }

    // R4 用户可见位：可见位缺失被拒，码恰为 E_INCOMP。
    let bad_vis = Present { cause: M_EXEC, det_bit: true, prompt_bit: true, vis_bit: false };
    match bk.put(bad_vis) {
        Err(E_INCOMP) => hits += 1,
        _ => return Err(E_VIS),
    }

    // R5 三要素齐全判定：两类齐全样本入账成功且计数分流正确。
    let ok_f = Present { cause: M_FROZEN, det_bit: true, prompt_bit: true, vis_bit: true };
    let ok_e = Present { cause: M_EXEC, det_bit: true, prompt_bit: true, vis_bit: true };
    bk.put(ok_f)?;
    bk.put(ok_e)?;
    if bk.frozen_count != 1 || bk.exec_count != 1 || bk.visible != 2 {
        return Err(E_INCOMP);
    }
    hits += 1;

    // R6 未失配不误呈现：空账 total=0、visible=0、守恒成立。
    let empty = PresBook::new();
    if empty.total != 0 || empty.visible != 0 || !empty.conserved() {
        return Err(E_FALSE);
    }
    hits += 1;

    // R7 文案闭集非空：两类别文案非空且互异；未知类别文案 None。
    let t1 = match cause_text(M_FROZEN) {
        Some(t) => t,
        None => return Err(E_TEXT),
    };
    let t2 = match cause_text(M_EXEC) {
        Some(t) => t,
        None => return Err(E_TEXT),
    };
    if t1.is_empty() || t2.is_empty() || t1 == t2 {
        return Err(E_TEXT);
    }
    if cause_text(0).is_some() || cause_text(9).is_some() {
        return Err(E_TEXT);
    }
    hits += 1;

    // R8 呈现账守恒：LCG 混合 64 笔齐全样本，守恒与分流复核一致。
    let mut bk2 = PresBook::new();
    let mut lcg = Lcg::new(0x48577);
    let mut t = 0u32;
    while t < 64 {
        let cause = if lcg.below(2) == 0 { M_FROZEN } else { M_EXEC };
        let p = Present {
            cause,
            det_bit: true,
            prompt_bit: true,
            vis_bit: true,
        };
        bk2.put(p)?;
        t += 1;
    }
    if !bk2.conserved() || bk2.total != 64 || bk2.visible != 64 {
        return Err(E_CONSERV);
    }
    hits += 1;

    // R9 指纹稳定：同账两次指纹一致且非零。
    let f1 = book_fp(&bk2);
    let f2 = book_fp(&bk2);
    if f1 != f2 || f1 == 0 {
        return Err(E_FP);
    }
    hits += 1;

    // R10 呈现行齐全：total=/vis=/fzn=/exe= 四段前缀在位。
    let line = book_line(&bk2);
    if !has_kw(line.as_bytes(), b"total=")
        || !has_kw(line.as_bytes(), b"vis=")
        || !has_kw(line.as_bytes(), b"fzn=")
        || !has_kw(line.as_bytes(), b"exe=")
    {
        return Err(E_LINE);
    }
    hits += 1;

    Ok(hits)
}

/// 账面指纹：四项计数串联。
pub fn book_fp(bk: &PresBook) -> u64 {
    let mut buf: Vec<u8> = vec![];
    buf.extend_from_slice(&bk.total.to_le_bytes());
    buf.extend_from_slice(&bk.visible.to_le_bytes());
    buf.extend_from_slice(&bk.frozen_count.to_le_bytes());
    buf.extend_from_slice(&bk.exec_count.to_le_bytes());
    fnv1a64(&buf)
}

/// 呈现行：total=<笔数> vis=<可见> fzn=<冻结面失配> exe=<执行面失配>。
pub fn book_line(bk: &PresBook) -> String {
    let mut s = String::new();
    s.push_str("total=");
    s.push_str(&bk.total.to_string());
    s.push_str(" vis=");
    s.push_str(&bk.visible.to_string());
    s.push_str(" fzn=");
    s.push_str(&bk.frozen_count.to_string());
    s.push_str(" exe=");
    s.push_str(&bk.exec_count.to_string());
    s
}

/// 子串判定（朴素扫描，统一口径）。
pub fn has_kw(hay: &[u8], kw: &[u8]) -> bool {
    if kw.is_empty() || hay.len() < kw.len() {
        return false;
    }
    let mut i = 0usize;
    while i + kw.len() <= hay.len() {
        let mut j = 0usize;
        while j < kw.len() && hay[i + j] == kw[j] {
            j += 1;
        }
        if j == kw.len() {
            return true;
        }
        i += 1;
    }
    false
}
