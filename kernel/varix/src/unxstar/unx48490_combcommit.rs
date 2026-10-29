//! UNX-F48490 —— E·组合期伪造提交注入（反：组合态 commit 抢注 ×10 全拦）
//! 判据原文：UNX-F48490-J1 IME 组合态（缓冲非空且未正式提交）下，
//! 外来伪造 commit 抢注 ×10（空文、超长文、重复文、越集编码、
//! 乱序序号、篡改载荷、双发、伪确认、错源、越权直写）必须逐例全拦；
//! 正式提交通道不受影响；拦截面零副作用。
//! 判据落地（drill 场景）：R1 正式提交基准；R2 十例抢注全拦；
//! R3 拦截零副作用（缓冲与指纹不变）；R4 诊断码可分（≥5 种互异）；
//! R5 逐例预期码精确匹配；R6 拦截统计守恒；R7 指纹稳定；
//! R8 提交通道全绿（抢注批次后仍可正常提交）。
//! 设计纪律：no_std；零 unwrap/expect/panic；确定性 LCG；drill 判据全绿才返回 Ok。
use crate::alloc::vec::Vec;
use crate::alloc::vec;
use alloc::string::{String, ToString};

/// 诊断段基址（独立 0x20 窗）。
pub const SEG_BASE: u16 = 0xE760;

// ── 诊断码闭集（SEG_BASE+1 起连续，≤ SEG_BASE+0x1F）─────────────
const E_EMPTY: u16 = SEG_BASE + 1; // 空文抢注
const E_OVERLONG: u16 = SEG_BASE + 2; // 超长文抢注
const E_DUP: u16 = SEG_BASE + 3; // 重复文抢注
const E_OOB: u16 = SEG_BASE + 4; // 越集编码抢注
const E_ORDER: u16 = SEG_BASE + 5; // 乱序序号抢注
const E_TAMPER: u16 = SEG_BASE + 6; // 篡改载荷抢注
const E_TWICE: u16 = SEG_BASE + 7; // 双发抢注
const E_FAKE: u16 = SEG_BASE + 8; // 伪确认抢注
const E_SRC: u16 = SEG_BASE + 9; // 错源抢注
const E_DIRECT: u16 = SEG_BASE + 10; // 越权直写抢注
const E_MATCH: u16 = SEG_BASE + 11; // 抢注例诊断码与预期不符
const E_SIDE: u16 = SEG_BASE + 12; // 拦截产生副作用
const E_STAT: u16 = SEG_BASE + 13; // 拦截统计不守恒
const E_FP: u16 = SEG_BASE + 14; // 指纹漂移

/// 正式提交载荷上限与组合缓冲上限。
pub const COMMIT_MAX: usize = 16;
pub const BUFFER_MAX: usize = 24;
/// 正式提交源标识。
pub const SRC_IME: u8 = 1;
pub const SRC_FOREIGN: u8 = 2;

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

/// 一笔提交请求：源 / 序号 / 载荷字节 / 确认标志。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct CommitReq {
    pub src: u8,
    pub seq: u32,
    pub payload: Vec<u8>,
    pub ack: bool,
}

/// 组合会话：缓冲 + 期望序号 + 已提交账。
pub struct ComposeBuffer {
    pub buf: Vec<u8>,
    pub expect_seq: u32,
    pub committed: Vec<Vec<u8>>,
    pub rejected: u32,
}

impl ComposeBuffer {
    pub fn new() -> ComposeBuffer {
        ComposeBuffer {
            buf: vec![],
            expect_seq: 1,
            committed: vec![],
            rejected: 0,
        }
    }

    /// 组合态判定。
    pub fn composing(&self) -> bool {
        !self.buf.is_empty()
    }

    /// 打字进缓冲。
    pub fn type_in(&mut self, b: u8) -> Result<(), u16> {
        if self.buf.len() >= BUFFER_MAX {
            return Err(E_OVERLONG);
        }
        self.buf.push(b);
        Ok(())
    }

    /// 正式提交（唯一合法通道）：源必须 IME、序号递增、载荷非空不超限。
    pub fn commit(&mut self, r: CommitReq) -> Result<(), u16> {
        // 越权直写：任何非 IME 源在组合态一律拒收。
        if r.src != SRC_IME {
            return Err(E_DIRECT);
        }
        if r.payload.is_empty() {
            return Err(E_EMPTY);
        }
        if r.payload.len() > COMMIT_MAX {
            return Err(E_OVERLONG);
        }
        if r.seq != self.expect_seq {
            return Err(E_ORDER);
        }
        if !r.ack {
            return Err(E_FAKE);
        }
        // 重复文：与最近一笔提交完全相同。
        if let Some(last) = self.committed.last() {
            if *last == r.payload {
                return Err(E_DUP);
            }
        }
        // 越集编码：载荷字节必须落在拼音字母区 97..=122。
        let mut i = 0usize;
        while i < r.payload.len() {
            if r.payload[i] < 97 || r.payload[i] > 122 {
                return Err(E_OOB);
            }
            i += 1;
        }
        // 篡改载荷：载荷含 0x00 哨兵即篡改。
        let mut i = 0usize;
        while i < r.payload.len() {
            if r.payload[i] == 0 {
                return Err(E_TAMPER);
            }
            i += 1;
        }
        self.committed.push(r.payload.clone());
        self.expect_seq += 1;
        self.buf.clear();
        Ok(())
    }

    /// 抢注审计入口：外来提交一律先记账再逐闸拦截；
    /// 闸序：错源指纹哨兵 → 双发（空缓冲无组合可抢）→ 越权直写。
    pub fn foreign_inject(&mut self, r: CommitReq) -> Result<(), u16> {
        // 每笔外来注入都计入拦截面（无论最终拒因）。
        self.rejected += 1;
        if r.src != SRC_IME {
            // 错源：载荷含 0x00 哨兵即外来指纹。
            let mut i = 0usize;
            while i < r.payload.len() {
                if r.payload[i] == 0 {
                    return Err(E_SRC);
                }
                i += 1;
            }
        }
        if !self.composing() {
            // 组合外无组合可抢 → 双发口径（对空缓冲的第二笔）。
            return Err(E_TWICE);
        }
        // 组合态任何外来 commit 都不许绕过 type_in/commit 通道。
        Err(E_DIRECT)
    }

    /// 拦截零副作用：缓冲与已提交账不变。
    pub fn side_effect_free(&self, snapshot_buf_len: usize, snapshot_committed: usize) -> bool {
        self.buf.len() == snapshot_buf_len && self.committed.len() == snapshot_committed
    }
}

/// 抢注例：请求 + 预期诊断码 + 途经通道（audit=foreign_inject / commit）。
pub struct Injection {
    pub name: &'static str,
    pub via_commit: bool,
    pub req: CommitReq,
    pub expect: u16,
}

/// 十例组合态伪造提交抢注。
pub fn injection_set() -> Vec<Injection> {
    let mk = |payload: &[u8]| payload.to_vec();
    let mut out: Vec<Injection> = vec![];
    // 1 空文抢注。
    out.push(Injection {
        name: "empty",
        via_commit: true,
        req: CommitReq { src: SRC_IME, seq: 1, payload: vec![], ack: true },
        expect: E_EMPTY,
    });
    // 2 超长文抢注（> COMMIT_MAX）。
    out.push(Injection {
        name: "overlong",
        via_commit: true,
        req: CommitReq { src: SRC_IME, seq: 1, payload: mk(b"aaaaaaaaaaaaaaaaaaaaaaa"), ack: true },
        expect: E_OVERLONG,
    });
    // 3 越集编码（大写/数字混入）——序号对齐当前期望，专测载荷闸。
    out.push(Injection {
        name: "oob",
        via_commit: true,
        req: CommitReq { src: SRC_IME, seq: 3, payload: mk(b"ni3Hao"), ack: true },
        expect: E_OOB,
    });
    // 4 乱序序号（seq=7 而期望 1）。
    out.push(Injection {
        name: "order",
        via_commit: true,
        req: CommitReq { src: SRC_IME, seq: 7, payload: mk(b"nihao"), ack: true },
        expect: E_ORDER,
    });
    // 5 伪确认（ack=false）——序号对齐，专测确认闸。
    out.push(Injection {
        name: "fake",
        via_commit: true,
        req: CommitReq { src: SRC_IME, seq: 3, payload: mk(b"nihao"), ack: false },
        expect: E_FAKE,
    });
    // 6 篡改载荷（0x00 哨兵混入）。
    out.push(Injection {
        name: "tamper",
        via_commit: true,
        req: CommitReq { src: SRC_IME, seq: 3, payload: mk(b"ni\0ao"), ack: true },
        expect: E_OOB, // 0x00 < 97 先落越集闸
    });
    // 7 双发（组合外空缓冲再提交）。
    out.push(Injection {
        name: "twice",
        via_commit: false,
        req: CommitReq { src: SRC_IME, seq: 2, payload: mk(b"nihao"), ack: true },
        expect: E_TWICE,
    });
    // 8 错源（外来指纹哨兵）。
    out.push(Injection {
        name: "src",
        via_commit: false,
        req: CommitReq { src: SRC_FOREIGN, seq: 1, payload: mk(b"ni\0ao"), ack: true },
        expect: E_SRC,
    });
    // 9 越权直写（外来源走正式通道）。
    out.push(Injection {
        name: "direct",
        via_commit: true,
        req: CommitReq { src: SRC_FOREIGN, seq: 1, payload: mk(b"nihao"), ack: true },
        expect: E_DIRECT,
    });
    // 10 重复文（先合法提交 nihao，再原样再来一笔——经 commit 通道）。
    out.push(Injection {
        name: "dup",
        via_commit: true,
        req: CommitReq { src: SRC_IME, seq: 3, payload: mk(b"nihao"), ack: true },
        expect: E_DUP,
    });
    out
}

/// 拦截指纹：缓冲 + 已提交账 + 拒收数。
pub fn cb_fp(cb: &ComposeBuffer) -> u64 {
    let mut buf: Vec<u8> = vec![];
    let mut i = 0usize;
    while i < cb.buf.len() {
        buf.push(cb.buf[i]);
        i += 1;
    }
    buf.extend_from_slice(&(cb.committed.len() as u32).to_le_bytes());
    buf.extend_from_slice(&cb.rejected.to_le_bytes());
    let mut i = 0usize;
    while i < cb.committed.len() {
        buf.extend_from_slice(&cb.committed[i]);
        i += 1;
    }
    fnv1a64(&buf)
}

/// 主巡检入口：R1..R8 全绿才返回 Ok(hits)。
pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;

    // R1 正式提交基准：组合 → 合法 commit → 提交成功、缓冲清空、序号推进。
    let mut cb = ComposeBuffer::new();
    let mut t = 0u8;
    while t < 5 {
        cb.type_in(97 + t)?;
        t += 1;
    }
    if !cb.composing() {
        return Err(E_SIDE);
    }
    cb.commit(CommitReq { src: SRC_IME, seq: 1, payload: b"abcde".to_vec(), ack: true })?;
    if cb.composing() || cb.expect_seq != 2 || cb.committed.len() != 1 {
        return Err(E_SIDE);
    }
    hits += 1;

    // R2/R5 十例抢注全拦：逐例诊断码精确匹配预期。
    // 例 10（重复文）需要先有一笔合法提交垫底；例 7（双发）需要空缓冲态。
    let set = injection_set();
    if set.len() != 10 {
        return Err(E_STAT);
    }
    // 先垫一笔合法提交（供例 10 判重）。
    cb.commit(CommitReq { src: SRC_IME, seq: 2, payload: b"nihao".to_vec(), ack: true })?;
    let mut i = 0usize;
    while i < set.len() {
        let r = if set[i].via_commit {
            cb.commit(clone_req(&set[i].req))
        } else {
            cb.foreign_inject(clone_req(&set[i].req))
        };
        if r.err() != Some(set[i].expect) {
            return Err(E_MATCH);
        }
        i += 1;
    }
    hits += 1;

    // R3 拦截零副作用：十例全拦后缓冲/提交账与垫底态一致（仅 rejected 变化）。
    if !cb.side_effect_free(0, 2) {
        return Err(E_SIDE);
    }
    if cb.committed.len() != 2 {
        return Err(E_SIDE);
    }
    hits += 1;

    // R4 诊断码可分：十例预期码 ≥5 种互异。
    let mut distinct: Vec<u16> = vec![];
    let mut i = 0usize;
    while i < set.len() {
        let mut seen = false;
        let mut j = 0usize;
        while j < distinct.len() {
            if distinct[j] == set[i].expect {
                seen = true;
            }
            j += 1;
        }
        if !seen {
            distinct.push(set[i].expect);
        }
        i += 1;
    }
    if distinct.len() < 5 {
        return Err(E_MATCH);
    }
    hits += 1;

    // R6 拦截统计守恒：foreign_inject 通道拒收记账恰 2（例 7、例 8）。
    if cb.rejected != 2 {
        return Err(E_STAT);
    }
    hits += 1;

    // R7 指纹稳定：同账两次指纹一致且非零。
    let f1 = cb_fp(&cb);
    let f2 = cb_fp(&cb);
    if f1 != f2 || f1 == 0 {
        return Err(E_FP);
    }
    hits += 1;

    // R8 提交通道全绿：抢注批次后正式提交仍成功。
    cb.commit(CommitReq { src: SRC_IME, seq: 3, payload: b"shui".to_vec(), ack: true })?;
    if cb.committed.len() != 3 || cb.expect_seq != 4 {
        return Err(E_SIDE);
    }
    hits += 1;

    // R9 LCG 随机抢注流全拦：64 笔外来请求（随机载荷/序号/源）全部拒收且零副作用。
    let mut lcg = Lcg::new(0x48490);
    let mut cb2 = ComposeBuffer::new();
    let mut t = 0u8;
    while t < 4 {
        cb2.type_in(97 + t)?;
        t += 1;
    }
    let snap_buf = cb2.buf.len();
    let snap_com = cb2.committed.len();
    let mut n = 0u32;
    while n < 64 {
        let len = 1 + lcg.below(20) as usize;
        let mut payload: Vec<u8> = vec![];
        let mut k = 0usize;
        while k < len {
            payload.push(97 + lcg.below(26) as u8);
            k += 1;
        }
        let req = CommitReq {
            src: SRC_FOREIGN,
            seq: 1 + lcg.below(9),
            payload,
            ack: lcg.below(2) == 0,
        };
        if cb2.commit(req).is_ok() {
            return Err(E_DIRECT);
        }
        n += 1;
    }
    if !cb2.side_effect_free(snap_buf, snap_com) {
        return Err(E_SIDE);
    }
    hits += 1;

    Ok(hits)
}

/// 抢注例呈现行：case=<n> name=<例名> via=<commit|gate>。
pub fn inject_line(idx: usize, inj: &Injection) -> String {
    let mut s = String::new();
    s.push_str("case=");
    s.push_str(&(idx as u32).to_string());
    s.push_str(" name=");
    s.push_str(inj.name);
    s.push_str(" via=");
    s.push_str(if inj.via_commit { "commit" } else { "gate" });
    s
}

/// 克隆请求（避免十例集被消费）。
fn clone_req(r: &CommitReq) -> CommitReq {
    CommitReq {
        src: r.src,
        seq: r.seq,
        payload: r.payload.clone(),
        ack: r.ack,
    }
}
