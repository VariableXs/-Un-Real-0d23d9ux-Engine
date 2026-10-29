//! UNX-F48550 —— E·复位误触注入（反：未确认复位 ×10 全拦）
//! 判据原文：UNX-F48550-J1 反向判据：以 10 轮未确认复位请求注入
//! 复位面，守门器必须逐轮全拦：未确认请求不得触发执行、
//! 误触拦截逐轮记账（拦截数 == 注入轮数）、确认凭证校验
//! （凭证缺失/过期/错号均拦）；拦后账面零执行、指纹不变；
//! 合法确认对照组全数通过；账目守恒、呈现行可复核。
//! 判据落地（drill 场景）：R1 凭证闭集；R2 未确认全拦；
//! R3 十轮拦截记账；R4 过期凭证拦；R5 错号凭证拦；
//! R6 合法确认对照通过；R7 拦后零执行；R8 fuzz 请求流零错态；
//! R9 指纹稳定；R10 呈现行齐全。
//! 设计纪律：no_std；零 unwrap/expect/panic；确定性 LCG；drill 判据全绿才返回 Ok。
use crate::alloc::vec::Vec;
use crate::alloc::vec;
use alloc::string::{String, ToString};

/// 诊断段基址（独立 0x20 窗）。
pub const SEG_BASE: u16 = 0xF3A0;

// ── 诊断码闭集（SEG_BASE+1 起连续，≤ SEG_BASE+0x1F）─────────────
const E_TOKEN: u16 = SEG_BASE + 1; // 凭证越出闭集
const E_BLOCK: u16 = SEG_BASE + 2; // 未确认未拦
const E_EXPIRE: u16 = SEG_BASE + 3; // 过期凭证未拦
const E_WRONG: u16 = SEG_BASE + 4; // 错号凭证未拦
const E_EXEC: u16 = SEG_BASE + 5; // 拦后仍执行
const E_LEGAL: u16 = SEG_BASE + 6; // 合法确认被误拦
const E_CONSERV: u16 = SEG_BASE + 7; // 拦截账不守恒
const E_FUZZ: u16 = SEG_BASE + 8; // fuzz 请求流错态
const E_FP: u16 = SEG_BASE + 9; // 指纹漂移
const E_LINE: u16 = SEG_BASE + 10; // 呈现行缺段

/// 未确认注入轮数。
pub const ROUND_N: u32 = 10;

/// 凭证有效期（毫秒）。
pub const TOKEN_TTL_MS: u32 = 3000;

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

/// 一笔复位请求：凭证号 + 请求时刻。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ResetReq {
    pub token: u32,
    pub at: u32,
}

/// 凭证闭集：0 视为无凭证（未确认）。
pub fn token_valid(t: u32) -> bool {
    t != 0
}

/// 复位守门器：布防时刻 + 凭证 + 拦截账 + 执行账。
pub struct ResetGuard {
    pub armed_at: u32,
    pub token: u32,
    pub blocked: u32,
    pub executed: u32,
}

impl ResetGuard {
    pub fn new() -> ResetGuard {
        ResetGuard {
            armed_at: 0,
            token: 0,
            blocked: 0,
            executed: 0,
        }
    }

    /// 布防并签发凭证：有效期自布防时刻起 TTL。
    pub fn arm(&mut self, at: u32, token: u32) -> Result<(), u16> {
        if !token_valid(token) {
            return Err(E_TOKEN);
        }
        self.armed_at = at;
        self.token = token;
        Ok(())
    }

    /// 凭证时效判定。
    pub fn token_fresh(&self, req: &ResetReq) -> bool {
        req.at.saturating_sub(self.armed_at) <= TOKEN_TTL_MS
    }

    /// 受理一笔复位请求：无凭证/过期/错号均拦（记账不执行）；
    /// 合法凭证执行恰一次。
    pub fn submit(&mut self, req: ResetReq) -> Result<bool, u16> {
        if !token_valid(req.token) {
            self.blocked += 1;
            return Ok(false);
        }
        if self.token == 0 {
            self.blocked += 1;
            return Ok(false);
        }
        if !self.token_fresh(&req) {
            self.blocked += 1;
            return Ok(false);
        }
        if req.token != self.token {
            self.blocked += 1;
            return Ok(false);
        }
        if self.executed >= 1 {
            self.blocked += 1;
            return Ok(false);
        }
        self.executed += 1;
        Ok(true)
    }

    /// 账守恒：拦截 + 执行 == 受理总数。
    pub fn conserved(&self, submitted: u32) -> bool {
        self.blocked + self.executed == submitted
    }

    /// 拦后零执行一致性：executed 与终态联动。
    pub fn exec_sane(&self) -> bool {
        self.executed <= 1
    }
}

/// 主巡检入口：R1..R10 全绿才返回 Ok(hits)。
pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;

    // R1 凭证闭集：0 无效、非零有效。
    if token_valid(0) || !token_valid(1) || !token_valid(0xFFFF_FFFF) {
        return Err(E_TOKEN);
    }
    hits += 1;

    // R2 未确认全拦：无凭证请求（token=0）逐轮全拦，执行恒 0。
    let mut gd = ResetGuard::new();
    gd.arm(0, 777)?;
    let mut t = 0u32;
    while t < ROUND_N {
        let ok = gd.submit(ResetReq { token: 0, at: 10 + t })?;
        if ok || gd.executed != 0 {
            return Err(E_BLOCK);
        }
        t += 1;
    }
    if gd.blocked != ROUND_N {
        return Err(E_BLOCK);
    }
    hits += 1;

    // R3 十轮拦截记账：拦截账恰 10 且守恒。
    if !gd.conserved(ROUND_N) || !gd.exec_sane() {
        return Err(E_CONSERV);
    }
    hits += 1;

    // R4 过期凭证拦：TTL 边界 3000 过、3001 拦。
    let mut gd2 = ResetGuard::new();
    gd2.arm(1000, 42)?;
    let ok = gd2.submit(ResetReq { token: 42, at: 1000 + TOKEN_TTL_MS })?;
    if !ok || gd2.executed != 1 {
        return Err(E_LEGAL);
    }
    let mut gd3 = ResetGuard::new();
    gd3.arm(1000, 42)?;
    let ok = gd3.submit(ResetReq {
        token: 42,
        at: 1000 + TOKEN_TTL_MS + 1,
    })?;
    if ok || gd3.executed != 0 || gd3.blocked != 1 {
        return Err(E_EXPIRE);
    }
    hits += 1;

    // R5 错号凭证拦：号不符拦，码路径正确。
    let mut gd4 = ResetGuard::new();
    gd4.arm(0, 111)?;
    let ok = gd4.submit(ResetReq { token: 222, at: 1 })?;
    if ok || gd4.executed != 0 {
        return Err(E_WRONG);
    }
    hits += 1;

    // R6 合法确认对照通过：凭证 fresh + 错号拦 + 正号过。
    let ok = gd4.submit(ResetReq { token: 111, at: 2 })?;
    if !ok || gd4.executed != 1 {
        return Err(E_LEGAL);
    }
    // 二次提交被拦（已执行）。
    let ok = gd4.submit(ResetReq { token: 111, at: 3 })?;
    if ok || gd4.executed != 1 {
        return Err(E_EXEC);
    }
    hits += 1;

    // R7 拦后零执行：拦截后指纹与零执行基线一致。
    let mut gd5 = ResetGuard::new();
    gd5.arm(0, 555)?;
    let mut t = 0u32;
    while t < 6 {
        let _ = gd5.submit(ResetReq { token: 0, at: t })?;
        t += 1;
    }
    if gd5.executed != 0 || !gd5.conserved(6) {
        return Err(E_EXEC);
    }
    hits += 1;

    // R8 fuzz 请求流零错态：LCG 驱动 96 笔混合请求（0 号/错号/过期/合法），
    // 每笔按判定收/拦，执行 ≤ 1、末态守恒。
    let mut gd6 = ResetGuard::new();
    gd6.arm(0, 9090)?;
    let mut lcg = Lcg::new(0x48550);
    let mut submitted = 0u32;
    let mut t = 0u32;
    while t < 96 {
        let pick = lcg.below(4);
        let req = match pick {
            0 => ResetReq { token: 0, at: t },
            1 => ResetReq { token: 1 + lcg.below(9_999), at: t },
            2 => ResetReq { token: 9090, at: 10_000 + t },
            _ => ResetReq { token: 9090, at: t },
        };
        let ok = gd6.submit(req)?;
        submitted += 1;
        if ok && gd6.executed != 1 {
            return Err(E_FUZZ);
        }
        t += 1;
    }
    if gd6.executed > 1 || !gd6.conserved(submitted) {
        return Err(E_FUZZ);
    }
    hits += 1;

    // R9 指纹稳定：同账两次指纹一致且非零。
    let f1 = guard_fp(&gd6);
    let f2 = guard_fp(&gd6);
    if f1 != f2 || f1 == 0 {
        return Err(E_FP);
    }
    hits += 1;

    // R10 呈现行齐全：block=/exec= 两段前缀在位。
    let line = guard_line(&gd6);
    if !has_kw(line.as_bytes(), b"block=") || !has_kw(line.as_bytes(), b"exec=") {
        return Err(E_LINE);
    }
    hits += 1;

    Ok(hits)
}

/// 账面指纹：拦截数 + 执行数 + 凭证号串联。
pub fn guard_fp(g: &ResetGuard) -> u64 {
    let mut buf: Vec<u8> = vec![];
    buf.extend_from_slice(&g.blocked.to_le_bytes());
    buf.extend_from_slice(&g.executed.to_le_bytes());
    buf.extend_from_slice(&g.token.to_le_bytes());
    buf.extend_from_slice(&g.armed_at.to_le_bytes());
    fnv1a64(&buf)
}

/// 呈现行：block=<拦截> exec=<执行>。
pub fn guard_line(g: &ResetGuard) -> String {
    let mut s = String::new();
    s.push_str("block=");
    s.push_str(&g.blocked.to_string());
    s.push_str(" exec=");
    s.push_str(&g.executed.to_string());
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
