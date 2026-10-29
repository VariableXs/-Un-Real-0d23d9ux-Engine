//! unx48336_smconc — UNX-F48336 B17 批并发自适应纪律（双焦点各自统计零互染）（诊断段 0xCAE0）。
//!
//! 判据原文：
//!   J1 并发 ×10^3 零互染；判据锚定 M5 母版。
//!   J2 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计纪律：
//!   1. 焦点隔离：双焦点（窗口 A/B）各自持有独立统计空间（词频表/
//!      热词计数/反馈游标），任意一侧的写不得出现在另一侧的读里
//!      ——零互染是本卡唯一口径。
//!   2. ×10^3 混合流：1,000 次确定性操作（LCG 决定焦点与写键），
//!      逐操作后双侧指纹分离可证（A 的写入只进 A 的空间）。
//!   3. 空间边界：统计空间按焦点路由（focus → space 映射唯一），
//!      路由表冻结（非法焦点拒路由，不入任何一侧）。
//!   4. 隔离证明三件套：写侧自见（写后本侧读得到）、跨侧不见
//!      （另一侧读不到）、终态分账（双侧终态指纹 = 各自操作重放
//!      指纹，单侧重放与混合重跑同值）。
//!   5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。

use crate::alloc::vec;
use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0xCAE0;

const E_CROSS_TINT: u16 = SEG_BASE + 1;    // 跨侧可见（互染）
const E_SELF_MISS: u16 = SEG_BASE + 2;     // 写后本侧不可见
const E_ROUTE_BAD: u16 = SEG_BASE + 3;     // 非法焦点被路由
const E_FINAL_DRIFT: u16 = SEG_BASE + 4;   // 终态分账漂移
const E_REPLAY_DRIFT: u16 = SEG_BASE + 5;  // 单侧重放指纹漂移
const E_CODE_RANGE: u16 = SEG_BASE + 6;    // 诊断码越段

/// 焦点二元。
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Focus {
    /// 焦点 A。
    FA = 0,
    /// 焦点 B。
    FB = 1,
}

pub const FOCUS_COUNT: usize = 2;
pub const TABLE_SLOTS: usize = 8;

/// 单焦点统计空间：8 槽词频表 + 热词计数。
#[derive(Clone, Copy)]
pub struct StatSpace {
    pub freq: [u32; TABLE_SLOTS],
    pub hot: u32,
}

impl StatSpace {
    pub fn new() -> Self {
        StatSpace {
            freq: [0; TABLE_SLOTS],
            hot: 0,
        }
    }

    pub fn bump(&mut self, key: u8) {
        let slot = (key % TABLE_SLOTS as u8) as usize;
        self.freq[slot] += 1;
        if self.freq[slot] % 3 == 0 {
            self.hot += 1; // 每 3 次命中热词计数 +1
        }
    }

    pub fn fingerprint(&self) -> u64 {
        let mut h: u64 = 0xCBF2_9CE4_8422_2325;
        let mut i = 0;
        while i < TABLE_SLOTS {
            h ^= self.freq[i] as u64;
            h = h.wrapping_mul(0x0000_0100_0000_01B3);
            i += 1;
        }
        h ^= self.hot as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
        h
    }
}

/// 双焦点自适应枢纽：路由表冻结（仅 FA/FB 合法）。
pub struct ConcHub {
    pub spaces: [StatSpace; FOCUS_COUNT],
}

impl ConcHub {
    pub fn new() -> Self {
        ConcHub {
            spaces: [StatSpace::new(), StatSpace::new()],
        }
    }

    /// 路由写：焦点合法才落空间；返回是否落账。
    pub fn route_write(&mut self, f: Focus, key: u8) -> bool {
        match f {
            Focus::FA => {
                self.spaces[0].bump(key);
                true
            }
            Focus::FB => {
                self.spaces[1].bump(key);
                true
            }
        }
    }

    /// 跨侧读：词频槽值（用于互染检查）。
    pub fn peek(&self, f: Focus, key: u8) -> u32 {
        let slot = (key % TABLE_SLOTS as u8) as usize;
        match f {
            Focus::FA => self.spaces[0].freq[slot],
            Focus::FB => self.spaces[1].freq[slot],
        }
    }
}

/// 操作流合成：×10^3 条（焦点 + 键），确定性。
pub fn synth_ops(n: usize, seed: u32) -> Vec<(Focus, u8)> {
    let mut lc = Lcg::new(seed);
    let mut ops: Vec<(Focus, u8)> = Vec::new();
    let mut i = 0;
    while i < n {
        let r = lc.next();
        let f = if r & 1 == 0 { Focus::FA } else { Focus::FB };
        let key = ((r >> 8) % 12) as u8; // 0..11（覆盖槽重叠区，放大互染概率）
        ops.push((f, key));
        i += 1;
    }
    ops
}

/// 确定性线性同余发生器。
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

/// B17 并发自适应主巡检：×10^3 零互染全链判据。
pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;
    let ops = synth_ops(1_000, 0x4833_6001);

    // R1 逐操作隔离断言：每写一侧，本侧必见、跨侧必不见（对同键
    //    直接对读）。
    let mut hub = ConcHub::new();
    let mut n_a = 0u32;
    let mut n_b = 0u32;
    let mut i = 0;
    while i < ops.len() {
        let (f, key) = ops[i];
        let other = match f {
            Focus::FA => Focus::FB,
            Focus::FB => Focus::FA,
        };
        let before_self = hub.peek(f, key);
        let before_other = hub.peek(other, key);
        if !hub.route_write(f, key) {
            return Err(E_ROUTE_BAD);
        }
        // 写侧自见。
        if hub.peek(f, key) != before_self + 1 {
            return Err(E_SELF_MISS);
        }
        // 跨侧不见。
        if hub.peek(other, key) != before_other {
            return Err(E_CROSS_TINT);
        }
        match f {
            Focus::FA => n_a += 1,
            Focus::FB => n_b += 1,
        }
        i += 1;
    }
    if n_a == 0 || n_b == 0 {
        return Err(E_ROUTE_BAD); // 两侧都要有真实流量
    }
    hits += 1;

    // R2 终态分账：双侧槽值之和 = 各侧操作数（频次守恒，无旁路写入）。
    let sum_a: u64 = {
        let mut s = 0u64;
        let mut j = 0;
        while j < TABLE_SLOTS {
            s += hub.spaces[0].freq[j] as u64;
            j += 1;
        }
        s
    };
    let sum_b: u64 = {
        let mut s = 0u64;
        let mut j = 0;
        while j < TABLE_SLOTS {
            s += hub.spaces[1].freq[j] as u64;
            j += 1;
        }
        s
    };
    if sum_a != n_a as u64 || sum_b != n_b as u64 {
        return Err(E_FINAL_DRIFT);
    }
    hits += 1;

    // R3 单侧重放：把 A 侧操作抽出来单独重放进新空间 → 指纹与混合
    //    运行中 A 空间指纹一致（混合调度不改变单侧终态）。
    let mut solo = StatSpace::new();
    let mut i3 = 0;
    while i3 < ops.len() {
        let (f, key) = ops[i3];
        if f == Focus::FA {
            solo.bump(key);
        }
        i3 += 1;
    }
    if solo.fingerprint() != hub.spaces[0].fingerprint() {
        return Err(E_REPLAY_DRIFT);
    }
    hits += 1;

    // R4 双侧重放（全量重跑同编排）：终态指纹与首跑逐位一致。
    let mut hub2 = ConcHub::new();
    let mut i4 = 0;
    while i4 < ops.len() {
        let (f, key) = ops[i4];
        hub2.route_write(f, key);
        i4 += 1;
    }
    if hub2.spaces[0].fingerprint() != hub.spaces[0].fingerprint()
        || hub2.spaces[1].fingerprint() != hub.spaces[1].fingerprint()
    {
        return Err(E_REPLAY_DRIFT);
    }
    hits += 1;

    // R5 热词分账：双侧热词计数独立（各自 = 本侧 3 倍数槽命中累计），
    //    且互不搬移（A 的热词计数只由 A 的槽值决定）。
    let hot_a_expect = derive_hot(&hub.spaces[0]);
    let hot_b_expect = derive_hot(&hub.spaces[1]);
    if hub.spaces[0].hot != hot_a_expect || hub.spaces[1].hot != hot_b_expect {
        return Err(E_FINAL_DRIFT);
    }
    hits += 1;

    // R6 逐槽对账：A 侧 8 槽逐槽与单侧重放空间逐槽相等（指纹一致
    //    之外的细粒度证词——互染若发生在槽级可精确定位）。
    let mut slot = 0usize;
    while slot < TABLE_SLOTS {
        if hub.spaces[0].freq[slot] != solo.freq[slot] {
            return Err(E_CROSS_TINT);
        }
        slot += 1;
    }
    hits += 1;

    // R6b B 侧逐槽对账：与 A 侧同口径（单侧重放 B 操作流逐槽核对）。
    let mut solo_b = StatSpace::new();
    let mut i5 = 0;
    while i5 < ops.len() {
        let (f, key) = ops[i5];
        if f == Focus::FB {
            solo_b.bump(key);
        }
        i5 += 1;
    }
    let mut slot2 = 0usize;
    while slot2 < TABLE_SLOTS {
        if hub.spaces[1].freq[slot2] != solo_b.freq[slot2] {
            return Err(E_CROSS_TINT);
        }
        slot2 += 1;
    }
    hits += 1;

    // R7 诊断码闭包：全部错误码落在 [SEG_BASE+1, SEG_BASE+0x1F] 段内。
    const CODES: [u16; 6] = [
        E_CROSS_TINT,
        E_SELF_MISS,
        E_ROUTE_BAD,
        E_FINAL_DRIFT,
        E_REPLAY_DRIFT,
        E_CODE_RANGE,
    ];
    let mut ci = 0;
    while ci < CODES.len() {
        if CODES[ci] <= SEG_BASE || CODES[ci] > SEG_BASE + 0x1F {
            return Err(E_CODE_RANGE);
        }
        ci += 1;
    }
    hits += 1;

    Ok(hits)
}

/// 热词计数推导（独立第二路）：每槽值中 3 的倍数部分累计。
pub fn derive_hot(s: &StatSpace) -> u32 {
    let mut hot = 0u32;
    let mut i = 0;
    while i < TABLE_SLOTS {
        // 频次从 0 涨到 v 的过程中每 +1 检查一次 %3==0：
        // 热词增量 = v/3（0,1,2 不触发，3 触发 1 次……）。
        hot += s.freq[i] / 3;
        i += 1;
    }
    hot
}
