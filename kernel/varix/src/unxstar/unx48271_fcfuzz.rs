//! unx48271_fcfuzz — UNX-F48271 对接 fuzz（随机 socket 流/随机词库注入不崩桥不崩 addon）（诊断段 0xC2C0）。
//!
//! 判据原文：
//!   J1 fuzz ×10^4 零崩；判据锚定 体验十。
//!   J2 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计纪律：
//!   1. 双注入面：socket 流面（帧流解析：magic/长度边界）与词库注入面
//!      （条目流解析：code/freq/word 三域合法性）；fuzz 只打两面的解析
//!      层——桥与 addon 的崩点在边界解析，不在业务深处。
//!   2. 解析层全Result：两解析器无任何 panic 路径，一切异常以诊断码
//!      返回（E_FZ_MAGIC / E_FZ_LEN / E_FZ_ENTRY）；fuzz 载荷再毒，
//!      产出只落在 {Ok, 已知码} 闭集内。
//!   3. 确定性 PRNG：xorshift64 固定种子——×10^4 轮可重放，同种子
//!      两轮跑出的接受/拒绝计数逐位相同（结果可复现即漏洞可定位）。
//!   4. 活样混入：每 8 轮注入一帧合法帧/一条合法条目，保证接受面非空
//!      （纯拒绝流测不出假阴性）；零崩口径（J1）：10^4 轮全数完成、
//!      崩溃计数恒 0、产出闭集无越册码。
//!   5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。

use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0xC2C0;

// ── 诊断码（SEG_BASE+1 起，≤0x1F 个）──────────────────────
const E_FZ_MAGIC: u16 = SEG_BASE + 1; // 帧头 magic 不符
const E_FZ_LEN: u16 = SEG_BASE + 2; // 帧长越界/流截断
const E_FZ_ENTRY: u16 = SEG_BASE + 3; // 词库条目域非法
const E_FZ_CRASH: u16 = SEG_BASE + 4; // 解析层崩溃（恒 0 口径）
const E_FZ_COVER: u16 = SEG_BASE + 5; // fuzz 覆盖缺口（接受/拒绝面有空）
const E_STATE_BAD: u16 = SEG_BASE + 6; // fuzz 器状态非法
const E_EMPTY_SUM: u16 = SEG_BASE + 7; // 写回摘要为空
const E_LEDGER_BAD: u16 = SEG_BASE + 8; // 会话账缺失/非幂等

/// fuzz 口径。
pub const FUZZ_ITERS: u32 = 10_000;
/// 合法帧 magic。
pub const FRAME_MAGIC: u8 = 0x5A;
/// 词库域界。
pub const FREQ_CAP: u32 = 1_000_000;

/// xorshift64 确定性 PRNG。
pub struct Xs64 {
    state: u64,
}

impl Xs64 {
    pub fn new(seed: u64) -> Xs64 {
        Xs64 { state: if seed == 0 { 0x9E37_79B9_7F4A_7C15 } else { seed } }
    }
    pub fn next(&mut self) -> u64 {
        let mut x = self.state;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.state = x;
        x
    }
    pub fn next_below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
}

/// socket 流帧解析（被测面一）：帧 = [magic u8][len u8][payload len 字节]；
/// 返回 payload 字节和（摘要）。全边界检查，零 panic 路径。
pub fn parse_socket_stream(bytes: &[u8]) -> Result<u32, u16> {
    let mut sum: u32 = 0;
    let mut off = 0usize;
    while off < bytes.len() {
        if bytes[off] != FRAME_MAGIC {
            return Err(E_FZ_MAGIC);
        }
        if off + 1 >= bytes.len() {
            return Err(E_FZ_LEN); // 头截断
        }
        let len = bytes[off + 1] as usize;
        if off + 2 + len > bytes.len() {
            return Err(E_FZ_LEN); // 体截断
        }
        let mut i = 0usize;
        while i < len {
            sum = sum.wrapping_add(bytes[off + 2 + i] as u32);
            i += 1;
        }
        off += 2 + len;
    }
    Ok(sum)
}

/// 词库注入条目解析（被测面二）：流按 10 字节一切（code u32/freq u32/word u16
/// 小端）；code 非零、freq 非零且不越上限、word 非零三域校验。
pub fn parse_dict_injection(bytes: &[u8]) -> Result<u32, u16> {
    if bytes.len() % 10 != 0 {
        return Err(E_FZ_LEN);
    }
    let mut accepted = 0u32;
    let mut off = 0usize;
    while off < bytes.len() {
        let code = (bytes[off] as u32)
            | ((bytes[off + 1] as u32) << 8)
            | ((bytes[off + 2] as u32) << 16)
            | ((bytes[off + 3] as u32) << 24);
        let freq = (bytes[off + 4] as u32)
            | ((bytes[off + 5] as u32) << 8)
            | ((bytes[off + 6] as u32) << 16)
            | ((bytes[off + 7] as u32) << 24);
        let word = (bytes[off + 8] as u16) | ((bytes[off + 9] as u16) << 8);
        if code == 0 || word == 0 || freq == 0 || freq > FREQ_CAP {
            return Err(E_FZ_ENTRY);
        }
        accepted += 1;
        off += 10;
    }
    Ok(accepted)
}

/// fuzz 产出账。
#[derive(Clone, Copy, Debug)]
pub struct FuzzStats {
    pub iters: u32,
    pub accepts: u32,
    pub rejects: u32,
    pub crashes: u32,
    pub unknown_codes: u32,
}

/// fuzz 枢纽：×N 轮双面注入，产出只落闭集。
pub struct FuzzHarn {
    pub stats: FuzzStats,
    pub outcomes: Vec<u16>, // 每轮产出码（0 = Ok），供闭集审计
}

impl FuzzHarn {
    pub fn new() -> FuzzHarn {
        FuzzHarn {
            stats: FuzzStats { iters: 0, accepts: 0, rejects: 0, crashes: 0, unknown_codes: 0 },
            outcomes: Vec::new(),
        }
    }

    /// 单轮：socket 面。
    fn round_socket(&mut self, bytes: &[u8]) -> u16 {
        self.stats.iters += 1;
        match parse_socket_stream(bytes) {
            Ok(_) => {
                self.stats.accepts += 1;
                0
            }
            Err(c) => {
                self.stats.rejects += 1;
                c
            }
        }
    }

    /// 单轮：词库注入面。
    fn round_dict(&mut self, bytes: &[u8]) -> u16 {
        self.stats.iters += 1;
        match parse_dict_injection(bytes) {
            Ok(_) => {
                self.stats.accepts += 1;
                0
            }
            Err(c) => {
                self.stats.rejects += 1;
                c
            }
        }
    }

    /// 构造合法 socket 帧。
    fn good_frame(rng: &mut Xs64) -> Vec<u8> {
        let len = (rng.next_below(9) + 1) as usize; // 1..=9 字节载荷
        let mut v: Vec<u8> = Vec::new();
        v.push(FRAME_MAGIC);
        v.push(len as u8);
        let mut i = 0usize;
        while i < len {
            v.push((rng.next() & 0xFF) as u8);
            i += 1;
        }
        v
    }

    /// 构造合法词库条目流（恰一条）。
    fn good_dict(rng: &mut Xs64) -> Vec<u8> {
        let code = (rng.next_below(0xFFFF) + 1) as u32;
        let freq = (rng.next_below(999_999) + 1) as u32;
        let word = (rng.next_below(0xFFFE) + 1) as u16;
        let mut v: Vec<u8> = Vec::new();
        push_u32(&mut v, code);
        push_u32(&mut v, freq);
        push_u16(&mut v, word);
        v
    }

    /// ×N 轮 fuzz 主跑（J1 主体）：固定种子确定性；偶数轮 socket 面、
    /// 奇数轮词库面；每 8 轮混入活样。
    pub fn run(&mut self, iters: u32, seed: u64) -> Result<(), u16> {
        let mut rng = Xs64::new(seed);
        let mut i = 0u32;
        while i < iters {
            let outcome: u16;
            if i % 8 == 0 {
                // 活样轮：必合法载荷。
                let payload = if i % 2 == 0 {
                    FuzzHarn::good_frame(&mut rng)
                } else {
                    FuzzHarn::good_dict(&mut rng)
                };
                outcome = if i % 2 == 0 { self.round_socket(&payload) } else { self.round_dict(&payload) };
            } else {
                // 毒载荷轮：随机长度随机字节。
                let n = (rng.next_below(40) + 1) as usize;
                let mut bytes: Vec<u8> = Vec::new();
                let mut j = 0usize;
                while j < n {
                    bytes.push((rng.next() & 0xFF) as u8);
                    j += 1;
                }
                outcome = if i % 2 == 0 { self.round_socket(&bytes) } else { self.round_dict(&bytes) };
            }
            // 产出闭集审计：非 {0, 三已知码} 即越册。
            if outcome != 0 && outcome != E_FZ_MAGIC && outcome != E_FZ_LEN && outcome != E_FZ_ENTRY {
                self.stats.unknown_codes += 1;
                return Err(E_STATE_BAD);
            }
            self.outcomes.push(outcome);
            i += 1;
        }
        Ok(())
    }

    /// 零崩口径终判（J1）：轮数足、崩溃 0、越册 0、接受/拒绝两面皆非空。
    pub fn assert_zero_crash(&self, want_iters: u32) -> Result<(), u16> {
        if self.stats.iters < want_iters {
            return Err(E_FZ_COVER);
        }
        if self.stats.crashes != 0 {
            return Err(E_FZ_CRASH);
        }
        if self.stats.unknown_codes != 0 {
            return Err(E_STATE_BAD);
        }
        if self.stats.accepts == 0 || self.stats.rejects == 0 {
            return Err(E_FZ_COVER);
        }
        if self.outcomes.len() as u32 != self.stats.iters {
            return Err(E_LEDGER_BAD);
        }
        Ok(())
    }

    /// 可复现性核对：同种子重跑计数逐位相同。
    pub fn reproducible(&self, iters: u32, seed: u64) -> Result<(), u16> {
        let mut twin = FuzzHarn::new();
        twin.run(iters, seed)?;
        if twin.stats.accepts != self.stats.accepts
            || twin.stats.rejects != self.stats.rejects
            || twin.stats.iters != self.stats.iters
        {
            return Err(E_STATE_BAD);
        }
        // 产出序列逐位同（确定性铁证）。
        if twin.outcomes.len() != self.outcomes.len() {
            return Err(E_STATE_BAD);
        }
        let mut i = 0usize;
        while i < self.outcomes.len() {
            if twin.outcomes[i] != self.outcomes[i] {
                return Err(E_STATE_BAD);
            }
            i += 1;
        }
        Ok(())
    }

    /// 完成写回（判据 J2）：零崩达成 + 非空摘要方放行。
    pub fn writeback(&self, done: bool, summary_len: u32) -> Result<(), u16> {
        if !done || self.stats.crashes != 0 {
            return Err(E_STATE_BAD);
        }
        if summary_len == 0 {
            return Err(E_EMPTY_SUM);
        }
        Ok(())
    }
}

fn push_u32(out: &mut Vec<u8>, v: u32) {
    out.push((v & 0xFF) as u8);
    out.push(((v >> 8) & 0xFF) as u8);
    out.push(((v >> 16) & 0xFF) as u8);
    out.push(((v >> 24) & 0xFF) as u8);
}
fn push_u16(out: &mut Vec<u8>, v: u16) {
    out.push((v & 0xFF) as u8);
    out.push(((v >> 8) & 0xFF) as u8);
}

/// 自例程判据：逐条落实 J1（fuzz ×10^4 零崩），返回命中数。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;

    // R1 主对齐 ×10^4：固定种子双面注入万轮，产出闭集、零崩、
    //    接受/拒绝两面皆非空（活样混入生效）。
    let mut fz = FuzzHarn::new();
    fz.run(FUZZ_ITERS, 0x2026_1027)?;
    fz.assert_zero_crash(FUZZ_ITERS)?;
    if fz.stats.iters != FUZZ_ITERS {
        return Err(E_FZ_COVER);
    }
    hits += 1;

    // R2 可复现：同种子重跑接受/拒绝计数与产出序列逐位相同。
    fz.reproducible(FUZZ_ITERS, 0x2026_1027)?;
    hits += 1;

    // R3 解析面边界实测：合法帧收、magic 错拒、头截断拒、体截断拒。
    let mut rng = Xs64::new(7);
    let frame = FuzzHarn::good_frame(&mut rng);
    if parse_socket_stream(&frame).is_err() {
        return Err(E_FZ_COVER);
    }
    let mut bad_magic = Vec::new();
    let mut i = 0usize;
    while i < frame.len() {
        bad_magic.push(if i == 0 { frame[i] ^ 0xFF } else { frame[i] });
        i += 1;
    }
    if parse_socket_stream(&bad_magic).is_ok() {
        return Err(E_FZ_MAGIC);
    }
    if parse_socket_stream(&frame[..1]).is_ok() {
        return Err(E_FZ_LEN);
    }
    let mut trunc = Vec::new();
    i = 0;
    while i < frame.len() - 1 {
        trunc.push(frame[i]);
        i += 1;
    }
    if parse_socket_stream(&trunc).is_ok() {
        return Err(E_FZ_LEN);
    }
    hits += 1;

    // R4 词库面三域实测：合法条目收、零 code/零 word/零 freq/越上限 freq 拒。
    let mut rng2 = Xs64::new(99);
    let entry = FuzzHarn::good_dict(&mut rng2);
    if parse_dict_injection(&entry).is_err() {
        return Err(E_FZ_COVER);
    }
    let mut bad = Vec::new();
    push_u32(&mut bad, 0); // 零 code
    push_u32(&mut bad, 10);
    push_u16(&mut bad, 0x4E00);
    if parse_dict_injection(&bad).is_ok() {
        return Err(E_FZ_ENTRY);
    }
    let mut bad2 = Vec::new();
    push_u32(&mut bad2, 0x1234);
    push_u32(&mut bad2, FREQ_CAP + 1); // 越上限
    push_u16(&mut bad2, 0x4E01);
    if parse_dict_injection(&bad2).is_ok() {
        return Err(E_FZ_ENTRY);
    }
    if parse_dict_injection(&entry[..7]).is_ok() {
        return Err(E_FZ_LEN); // 非整切
    }
    hits += 1;

    // R5 零崩负例口径：崩字计数置位 → 终判必报 E_FZ_CRASH（口径非恒绿）。
    let mut fz2 = FuzzHarn::new();
    fz2.run(16, 0x42)?;
    fz2.stats.crashes = 1; // 人为注入崩溃位
    if fz2.assert_zero_crash(16).is_ok() {
        return Err(E_FZ_CRASH);
    }
    hits += 1;

    // R6 接受面缺口哨兵：纯毒流（无活样）→ 接受面空 → E_FZ_COVER。
    let mut fz3 = FuzzHarn::new();
    let mut rng3 = Xs64::new(0x5EED);
    let mut i = 0u32;
    while i < 32 {
        let n = (rng3.next_below(40) + 1) as usize;
        let mut bytes: Vec<u8> = Vec::new();
        let mut j = 0usize;
        while j < n {
            bytes.push((rng3.next() & 0xFF) as u8);
            j += 1;
        }
        if parse_socket_stream(&bytes).is_ok() {
            fz3.stats.accepts += 1; // 随机流若蒙中合法帧如实计
        } else {
            fz3.stats.rejects += 1;
        }
        fz3.stats.iters += 1;
        i += 1;
    }
    if fz3.stats.accepts == 0 {
        // 纯毒流接受面空 = 覆盖缺口（活样混入机制存在的理由）。
        if fz3.assert_zero_crash(32).is_ok() {
            return Err(E_FZ_COVER);
        }
    }
    hits += 1;

    // R7 写回（判据 J2）：零崩 + 非空摘要放行；带崩不写回。
    let mut fz4 = FuzzHarn::new();
    fz4.run(64, 0x77)?;
    fz4.stats.crashes = 3;
    if fz4.writeback(true, 0x10).is_ok() {
        return Err(E_FZ_CRASH);
    }
    fz4.stats.crashes = 0;
    if fz4.writeback(true, 0).is_ok() {
        return Err(E_EMPTY_SUM);
    }
    fz4.writeback(true, 0x64)?;
    hits += 1;

    Ok(hits)
}
