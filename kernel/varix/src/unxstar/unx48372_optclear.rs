//! unx48372_optclear — UNX-F48372 B19 批可关闭与残留清零（用户关日志后零写入）（诊断段 0xCF60）。
//!
//! 判据原文：
//!   J1 关闭后 ×10^3 事件零落盘；判据锚定 体验十二 诚实透明。
//!   J2 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计纪律：
//!   1. 关闭即零写入：用户关闭体验日志（B19 批开关）后，任何事件都不得
//!      落盘——不是延迟写，不是脱敏写，是存储侧一个字节都不动；落盘长
//!      度与存储指纹在关闭前后必须完全一致。
//!   2. 诚实透明：被拒收的事件必须逐条计数（抑制计数），关着开关丢了
//!      多少事件要能对账；抑制计数 ≠ 偷偷缓存，被抑制事件不进任何待写
//!      缓冲。
//!   3. 残留清零：关闭时刻批量缓冲里可能还有未冲刷的残留批次；显式
//!      clear_residue() 必须把残留丢弃到零且不产生任何落盘（用户已说
//!      不记，残留就是不该写的账）。
//!   4. 重开即恢复：开关重新打开后写入必须恢复正常，且新批次从空缓冲
//!      起步（旧残留已被清零，不复活）；重复关闭幂等（不产生第二次清
//!      状态副作用）。
//!   5. ×10^3 压力关门：关闭后灌入 1000 条事件，存储零增长、指纹不变、
//!      抑制计数恰 +1000——三项同时成立才算关门干净。
//!   6. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。

use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0xCF60;

// ── 诊断码（SEG_BASE+1 起，≤0x1F 个）──────────────────────
const E_CLOSED_WRITE: u16 = SEG_BASE + 1;  // 关闭后仍发生落盘
const E_SUPPRESS_FAULT: u16 = SEG_BASE + 2; // 抑制计数与拒收数不符
const E_RESIDUE_SURVIVED: u16 = SEG_BASE + 3; // 残留未被清零
const E_RESIDUE_WROTE: u16 = SEG_BASE + 4; // 残留清理过程产生落盘
const E_RESUME_FAULT: u16 = SEG_BASE + 5;  // 重开后写入未恢复
const E_DOUBLE_OFF_FAULT: u16 = SEG_BASE + 6; // 重复关闭产生副作用
const E_DIGEST_FAULT: u16 = SEG_BASE + 7;  // 存储指纹守恒被破坏
const E_CODE_RANGE: u16 = SEG_BASE + 8;    // 诊断码越段

/// B19 批开关状态。
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum OptState {
    On,
    Off,
}

/// 批量缓冲大小（残留检查的单位）。
pub const BATCH: usize = 16;

/// 可关闭体验日志存储：开关 + 批量缓冲 + 落盘账。
pub struct OptStore {
    opt: OptState,
    /// 已落盘记录。
    disk: Vec<u32>,
    /// 存储指纹：FNV-1a 折叠全部落盘记录。
    digest: u64,
    /// 批量缓冲（未冲刷的待写批次）。
    pending: Vec<u32>,
    /// 关闭后被抑制（零落盘拒收）的事件数。
    pub suppressed: u32,
    /// 残留清理丢弃的事件数。
    pub residue_cleared: u64,
}

impl OptStore {
    pub fn new() -> Self {
        OptStore {
            opt: OptState::On,
            disk: Vec::new(),
            digest: 0xCBF2_9CE4_8422_2325,
            pending: Vec::new(),
            suppressed: 0,
            residue_cleared: 0,
        }
    }

    /// 写入一个事件：开关关着 = 零落盘零缓存，只做抑制计数。
    /// 返回 true = 已进入批量缓冲；false = 被抑制。
    pub fn write_event(&mut self, payload: u32) -> bool {
        if self.opt == OptState::Off {
            self.suppressed += 1;
            return false;
        }
        self.pending.push(payload);
        if self.pending.len() >= BATCH {
            self.flush();
        }
        true
    }

    /// 批量冲刷：缓冲整体落盘并折叠指纹。
    fn flush(&mut self) {
        let mut i = 0usize;
        while i < self.pending.len() {
            let v = self.pending[i];
            self.disk.push(v);
            self.digest ^= v as u64;
            self.digest = self.digest.wrapping_mul(0x0000_0100_0000_01B3);
            i += 1;
        }
        self.pending.clear();
    }

    /// 收尾强制冲刷：无视批量门槛把缓冲全部落盘（仅在开关开启时有效）。
    pub fn flush_force(&mut self) {
        if self.opt == OptState::On {
            self.flush();
        }
    }

    /// 残留清零：丢弃批量缓冲中的未冲刷残留，绝不落盘。
    pub fn clear_residue(&mut self) {
        self.residue_cleared += self.pending.len() as u64;
        self.pending.clear();
    }

    pub fn set_opt(&mut self, s: OptState) {
        self.opt = s;
    }

    pub fn opt(&self) -> OptState {
        self.opt
    }

    pub fn disk_len(&self) -> usize {
        self.disk.len()
    }

    pub fn pending_len(&self) -> usize {
        self.pending.len()
    }

    pub fn digest(&self) -> u64 {
        self.digest
    }
}

/// 确定性线性同余发生器（压测剧本统一入口，禁真随机）。
pub struct Lcg(u32);

impl Lcg {
    pub fn new(seed: u32) -> Self {
        Lcg(seed | 1)
    }
    pub fn next(&mut self) -> u32 {
        self.0 = self.0.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        self.0
    }
}

/// 自例程判据：逐条落实 J1 的全部断言（关闭后 ×10^3 事件零落盘）。
pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;
    let mut lc = Lcg::new(0x4837_2001);

    // R1 关门前基线：开关开启，50 条事件正常写入；缓冲留有 2 条残留
    // （50 % 16 = 2），落盘 48 条、指纹非初值。
    let mut st = OptStore::new();
    let mut i = 0u32;
    while i < 50 {
        if !st.write_event(lc.next()) {
            return Err(E_RESUME_FAULT);
        }
        i += 1;
    }
    if st.disk_len() != 48 || st.pending_len() != 2 {
        return Err(E_RESIDUE_SURVIVED);
    }
    if st.digest() == 0xCBF2_9CE4_8422_2325 {
        return Err(E_DIGEST_FAULT);
    }
    hits += 1;

    // R2 关门 ×10^3 零落盘：关闭开关，灌入 1000 条事件，落盘长度与指纹
    // 完全不变。
    let before_len = st.disk_len();
    let before_digest = st.digest();
    st.set_opt(OptState::Off);
    let mut i = 0u32;
    while i < 1000 {
        if st.write_event(lc.next()) {
            return Err(E_CLOSED_WRITE);
        }
        i += 1;
    }
    if st.disk_len() != before_len || st.digest() != before_digest {
        return Err(E_CLOSED_WRITE);
    }
    hits += 1;

    // R3 诚实对账：抑制计数恰好 +1000，且关闭期间缓冲零增长（不偷偷
    // 缓存）。
    if st.suppressed != 1000 || st.pending_len() != 2 {
        return Err(E_SUPPRESS_FAULT);
    }
    hits += 1;

    // R4 残留清零：关门状态下显式清残留，缓冲归零、零落盘、指纹不变。
    st.clear_residue();
    if st.pending_len() != 0 || st.residue_cleared != 2 {
        return Err(E_RESIDUE_SURVIVED);
    }
    if st.disk_len() != before_len || st.digest() != before_digest {
        return Err(E_RESIDUE_WROTE);
    }
    hits += 1;

    // R5 重复关闭幂等：再次置 Off 再灌 100 条，只多抑制不落盘，清残留
    // 计数不再增长（无第二次清状态副作用）。
    st.set_opt(OptState::Off);
    let mut i = 0u32;
    while i < 100 {
        if st.write_event(lc.next()) {
            return Err(E_DOUBLE_OFF_FAULT);
        }
        i += 1;
    }
    if st.suppressed != 1100 || st.residue_cleared != 2 || st.disk_len() != before_len {
        return Err(E_DOUBLE_OFF_FAULT);
    }
    hits += 1;

    // R6 重开即恢复：重开后 3 批 + 4 条写入，落盘恢复增长且从空缓冲
    // 起步；缓冲恰好是新写的 4 条（旧残留已在 R4 清零，不复活）。
    st.set_opt(OptState::On);
    let mut i = 0u32;
    while i < 3 * BATCH as u32 + 4 {
        if !st.write_event(lc.next()) {
            return Err(E_RESUME_FAULT);
        }
        i += 1;
    }
    if st.disk_len() != before_len + 3 * BATCH || st.pending_len() != 4 {
        return Err(E_RESUME_FAULT);
    }
    if st.digest() == before_digest {
        return Err(E_DIGEST_FAULT);
    }
    hits += 1;

    // R7 重开后收尾冲刷：强制冲刷缓冲，落盘数与指纹与逐条手工折叠
    // 完全一致（写序保真）。
    let mut verify = OptStore::new();
    // 重放全程：基线 50（含 2 残留被清）→ 重开 52 条 → 收尾冲刷。
    let mut lc2 = Lcg::new(0x4837_2001);
    let mut i = 0u32;
    let mut payload_log: Vec<u32> = Vec::new();
    while i < 50 {
        payload_log.push(lc2.next());
        i += 1;
    }
    let mut i = 0u32;
    while i < 3 * BATCH as u32 + 4 {
        payload_log.push(lc2.next());
        i += 1;
    }
    let mut i = 0usize;
    while i < 50 {
        let _ = verify.write_event(payload_log[i]);
        i += 1;
    }
    verify.clear_residue();
    let mut i = 50usize;
    while i < payload_log.len() {
        let _ = verify.write_event(payload_log[i]);
        i += 1;
    }
    // 手工折叠基线 48 条（50 条中前 48 条满批落盘）+ 重开后全部 52 条
    // （收尾强制冲刷把缓冲 4 条一并落盘）。
    let mut h: u64 = 0xCBF2_9CE4_8422_2325;
    let mut i = 0usize;
    while i < 48 {
        h ^= payload_log[i] as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
        i += 1;
    }
    let mut i = 50usize;
    while i < 102 {
        h ^= payload_log[i] as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
        i += 1;
    }
    verify.flush_force();
    if verify.digest() != h || verify.disk_len() != 100 || verify.pending_len() != 0 {
        return Err(E_DIGEST_FAULT);
    }
    hits += 1;

    // R8 诊断码闭包：全部错误码落在 [SEG_BASE+1, SEG_BASE+0x1F] 段内。
    const CODES: [u16; 8] = [
        E_CLOSED_WRITE,
        E_SUPPRESS_FAULT,
        E_RESIDUE_SURVIVED,
        E_RESIDUE_WROTE,
        E_RESUME_FAULT,
        E_DOUBLE_OFF_FAULT,
        E_DIGEST_FAULT,
        E_CODE_RANGE,
    ];
    let mut ci = 0usize;
    while ci < CODES.len() {
        if CODES[ci] <= SEG_BASE || CODES[ci] > SEG_BASE + 0x1F {
            return Err(E_CODE_RANGE);
        }
        ci += 1;
    }
    hits += 1;

    Ok(hits)
}
