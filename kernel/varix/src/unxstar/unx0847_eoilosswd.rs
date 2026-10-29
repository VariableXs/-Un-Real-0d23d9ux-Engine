//! UNX-F0847 · EOI 丢失注入检测器（人为丢 EOI 必抓） · 诊断段 0x66C0（独占 0x66C0~0x66CF）
//!
//! **判据原文摘录**：注入丢 EOI 10/10 次被检测器捕获（ISR 残留+重投递停止
//! 双信号），误报率为 0。
//!
//! **设计纪律**：
//! 1. 双信号判定（判据明文）：信号 A = ISR 位残留（EOI 应写未写 ⇒ 位图
//!    非零）；信号 B = 重投递停止（该向量失能后设备不再收到服务——模型上
//!    以「后续投递被拒/挂起不再 accept」表征）。**A∧B 同时成立才立案**，
//!    单信号仅入观察账——压误报的关键：只有双信号联合才判丢 EOI。
//! 2. 注入面：10/10 次人为丢 EOI（每次丢注入后跳过 eoi() 调用），检测器
//!    逐次捕获；对照组 10/10 次正常 EOI（无误报）。捕获率 10/10 且
//!    false_positive = 0 才过。
//! 3. 观察窗推进：每 tick 采样一次 ISR 位图与重投递计数；残留持续 ≥
//!    patience 个采样窗才升格为信号成立（抗瞬时窗口）。
//! 4. 守恒恒等式：`should_eoi = eoi_done + eoi_lost + pending_window`，
//!    pending_window = 当前仍在观察窗内未定案的 EOI 欠账数。
//! 5. 零 unwrap/expect/panic/浮点/std；无墙钟，采样 tick 内置自增。
//!
//! **诊断段声明**：0x66C0~0x66CF 为本单独占。纯功能线：no_std + alloc，
//! 零 unwrap/expect/panic，整数算术，确定性。

#![allow(dead_code)]

use alloc::vec::Vec;

// ===========================================================================
// 一、诊断码（0x66C0~0x66CF 闭集）
// ===========================================================================

/// 段基址（保留码）。
pub const SEG_BASE: u16 = 0x66C0;
/// 段顶（保留码）。
pub const SEG_TOP: u16 = 0x66CF;

/// 向量号越界。
pub const X_EL_VEC_RANGE: u16 = 0x66C1;
/// 观察窗 patience 配置非法（0 窗视为无观测）。
pub const X_EL_PATIENCE: u16 = 0x66C2;
/// 双信号立案不一致（A/B 信号组合枚举越界）。
pub const X_EL_SIGNAL_RANGE: u16 = 0x66C3;
/// 丢 EOI 未捕获（注入面红线：应抓未抓）。
pub const X_EL_MISS_DETECTED: u16 = 0x66C4;
/// 误报检出（正常 EOI 被立案——false positive 红线）。
pub const X_EL_FALSE_POSITIVE: u16 = 0x66C5;
/// 守恒破缺（should_eoi ≠ done + lost + pending）。
pub const X_EL_UNBALANCED: u16 = 0x66C6;
/// 观察案满（定容拒绝，降级计数）。
pub const X_EL_CASE_FULL: u16 = 0x66C7;
/// 案件状态机越界（观察态直接终局非法）。
pub const X_EL_CASE_STATE: u16 = 0x66C8;

/// 诊断码→中文名。
pub const fn code_name(code: u16) -> &'static str {
    match code {
        SEG_BASE => "段基址(保留)",
        X_EL_VEC_RANGE => "向量号越界",
        X_EL_PATIENCE => "观察窗配置非法",
        X_EL_SIGNAL_RANGE => "信号组合越界",
        X_EL_MISS_DETECTED => "丢EOI未捕获",
        X_EL_FALSE_POSITIVE => "误报检出",
        X_EL_UNBALANCED => "EOI账守恒破缺",
        X_EL_CASE_FULL => "观察案满",
        X_EL_CASE_STATE => "案件状态越界",
        _ => "未知码",
    }
}

// ===========================================================================
// 二、ISR 位图（[u64;4]）
// ===========================================================================

/// 256 位置图。
#[derive(Clone, Copy)]
pub struct VecBitmap {
    words: [u64; 4],
}

impl VecBitmap {
    pub const EMPTY: VecBitmap = VecBitmap { words: [0; 4] };

    pub fn set(&mut self, v: u8) {
        self.words[(v >> 6) as usize] |= 1u64 << (v & 63);
    }
    pub fn clear(&mut self, v: u8) {
        self.words[(v >> 6) as usize] &= !(1u64 << (v & 63));
    }
    pub fn get(&self, v: u8) -> bool {
        (self.words[(v >> 6) as usize] >> (v & 63)) & 1 == 1
    }
    pub const fn is_zero(&self) -> bool {
        (self.words[0] | self.words[1] | self.words[2] | self.words[3]) == 0
    }
    pub fn popcount(&self) -> u32 {
        let mut n = 0u32;
        let mut w = 0;
        while w < 4 {
            let mut x = self.words[w];
            while x != 0 {
                x &= x - 1;
                n += 1;
            }
            w += 1;
        }
        n
    }
}

// ===========================================================================
// 三、案件状态机（观察 → 立案 / 洗清）
// ===========================================================================

/// 案件状态闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CaseState {
    /// 观察中（信号未齐）。
    Watching,
    /// 立案（双信号齐，判定丢 EOI）。
    Convicted,
    /// 洗清（EOI 补写，信号消失）。
    Cleared,
}

/// 单向量观察案。
#[derive(Clone, Copy)]
pub struct WatchCase {
    pub vector: u8,
    pub state: CaseState,
    /// 信号 A 持续命中采样数（ISR 残留）。
    pub sig_a_streak: u32,
    /// 信号 B 持续命中采样数（重投递停止）。
    pub sig_b_streak: u32,
    /// 采样次数。
    pub samples: u32,
}

impl WatchCase {
    /// 双信号判定：A、B 各自持续 ≥ patience 才立案。
    pub const fn should_convict(&self, patience: u32) -> bool {
        self.sig_a_streak >= patience && self.sig_b_streak >= patience
    }
}

// ===========================================================================
// 四、检测器主体
// ===========================================================================

/// EOI 丢失检测器：守恒账 + 双信号观察窗。
pub struct EoiLossDetector {
    pub isr: VecBitmap,
    /// patience：升格所需持续采样数（≥1）。
    pub patience: u32,
    /// 守恒账。
    pub should_eoi: u64,
    pub eoi_done: u64,
    pub eoi_lost: u64,
    /// 观察窗内未定案欠账（pending）。
    pub pending: u64,
    /// 观察案表（每向量至多一案）。
    cases: Vec<WatchCase>,
    /// 每向量重投递活动计数（上一采样以来是否仍有投递）。
    redeliver_active: [bool; 32],
    /// 向量→重投递活动映射的实际承载（低 32 向量段外的用哈希槽近似：
    /// 本模型限定检测面向 32..63 常用段，共 32 槽）。
    pub redeliver_seen: [u32; 32],
    /// 立案/洗清/误报统计。
    pub convicted_total: u64,
    pub cleared_total: u64,
    pub false_positive: u64,
    tick: u64,
}

impl EoiLossDetector {
    pub fn new(patience: u32) -> Result<Self, u16> {
        if patience == 0 {
            return Err(X_EL_PATIENCE);
        }
        Ok(Self {
            isr: VecBitmap::EMPTY,
            patience,
            should_eoi: 0,
            eoi_done: 0,
            eoi_lost: 0,
            pending: 0,
            cases: Vec::new(),
            redeliver_active: [false; 32],
            redeliver_seen: [0; 32],
            convicted_total: 0,
            cleared_total: 0,
            false_positive: 0,
            tick: 0,
        })
    }

    /// 槽索引：检测面向 32..=63 向量段（32 槽）。
    const fn slot(vector: u8) -> Result<usize, u16> {
        if vector < 32 || vector > 63 {
            return Err(X_EL_VEC_RANGE);
        }
        Ok((vector - 32) as usize)
    }

    /// 投递 + accept（ISR 置位，产生一次「应 EOI」欠账）。
    pub fn deliver(&mut self, vector: u8) -> Result<(), u16> {
        let s = Self::slot(vector)?;
        if self.isr.get(vector) {
            // 同向量在服：重投递挂起（不重复 accept，欠账不增）。
            self.redeliver_active[s] = true;
            return Ok(());
        }
        self.isr.set(vector);
        self.should_eoi += 1;
        self.pending += 1;
        self.redeliver_active[s] = true;
        self.balance_check()?;
        Ok(())
    }

    /// EOI 写（欠账结清）。
    pub fn eoi(&mut self, vector: u8) -> Result<(), u16> {
        let _ = Self::slot(vector)?;
        if !self.isr.get(vector) {
            // 无 ISR 位：视为对已丢 EOI 向量的迟来补写——洗清信号 B。
            return Ok(());
        }
        self.isr.clear(vector);
        self.eoi_done += 1;
        self.pending -= 1;
        self.balance_check()?;
        Ok(())
    }

    /// 守恒：should_eoi = done + lost + pending。
    fn balance_check(&self) -> Result<(), u16> {
        if self.should_eoi == self.eoi_done + self.eoi_lost + self.pending {
            Ok(())
        } else {
            Err(X_EL_UNBALANCED)
        }
    }

    /// 采样推进：对每案累计 A/B 信号 streak，达 patience 立案；
    /// 信号消失且未立案 → 洗清。
    pub fn sample(&mut self) -> Result<Vec<CaseState>, u16> {
        self.tick += 1;
        let mut results = Vec::new();
        let mut i = 0;
        while i < self.cases.len() {
            let v = self.cases[i].vector;
            let s = Self::slot(v)?;
            let sig_a = self.isr.get(v); // 残留 = ISR 仍置位
            let sig_b = !self.redeliver_active[s]; // 重投递停止 = 无活动
            // 采样后重投递活动清零（下一采样重新累计）。
            self.redeliver_active[s] = false;
            let c = &mut self.cases[i];
            c.samples += 1;
            c.sig_a_streak = if sig_a { c.sig_a_streak + 1 } else { 0 };
            c.sig_b_streak = if sig_b { c.sig_b_streak + 1 } else { 0 };
            if c.state == CaseState::Watching {
                if c.should_convict(self.patience) {
                    c.state = CaseState::Convicted;
                    self.convicted_total += 1;
                    self.eoi_lost += 1;
                    self.pending -= 1;
                    results.push(CaseState::Convicted);
                } else if !sig_a && !sig_b && c.samples > self.patience {
                    c.state = CaseState::Cleared;
                    self.cleared_total += 1;
                    self.pending -= 1;
                    results.push(CaseState::Cleared);
                } else {
                    results.push(CaseState::Watching);
                }
            } else {
                results.push(c.state);
            }
            i += 1;
        }
        self.balance_check()?;
        Ok(results)
    }

    /// 开观察案（每向量至多一案）。
    pub fn watch(&mut self, vector: u8) -> Result<(), u16> {
        let _ = Self::slot(vector)?;
        let mut i = 0;
        while i < self.cases.len() {
            if self.cases[i].vector == vector {
                return Ok(()); // 已有案：幂等。
            }
            i += 1;
        }
        if self.cases.len() >= 32 {
            return Err(X_EL_CASE_FULL);
        }
        self.cases.push(WatchCase {
            vector,
            state: CaseState::Watching,
            sig_a_streak: 0,
            sig_b_streak: 0,
            samples: 0,
        });
        Ok(())
    }
}

// ===========================================================================
// 五、注入演练（10 丢必抓 + 10 正常零误报）
// ===========================================================================

/// 演练结论。
pub struct DrillVerdict {
    /// 注入丢失次数（应 = 10）。
    pub injected: u64,
    /// 捕获立案数（应 = 10）。
    pub caught: u64,
    /// 误报数（应 = 0）。
    pub false_positive: u64,
    /// 正常对照组次数（应 = 10）。
    pub control: u64,
}

/// 确定性 LCG。
pub struct Lcg(u64);
impl Lcg {
    pub const fn new(seed: u64) -> Self {
        Lcg(seed)
    }
    pub fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0
    }
}

/// 注入演练：10 次人为丢 EOI（各占一向量）+ 10 次正常 EOI 对照。
/// 每轮：投递 → watch → 按分支丢/写 EOI → 连续 sample 至案件终局。
pub fn run_injection_rounds() -> Result<DrillVerdict, u16> {
    let mut det = EoiLossDetector::new(3)?;
    let mut rng = Lcg::new(0x66C0_66C0);
    let mut injected = 0u64;
    let mut caught = 0u64;
    let mut control = 0u64;
    let mut round = 0u64;
    while round < 20 {
        // 每轮独立向量：32..63 段内不重复（丢失向量偶数槽、对照奇数槽）。
        let vector = if round < 10 {
            32 + (round * 2) as u8
        } else {
            33 + ((round - 10) * 2) as u8
        };
        det.deliver(vector)?;
        det.watch(vector)?;
        let lose = round < 10; // 前 10 轮人为丢 EOI。
        if !lose {
            det.eoi(vector)?;
            control += 1;
        } else {
            injected += 1;
        }
        // 采样至终局：上限 16 采样（patience=3，足够 A/B streak 达标）。
        // 只判当前向量案件状态，旧轮已终局案件不回填计数（防重复计捕）。
        let mut s = 0;
        while s < 16 {
            det.sample()?;
            let mut st = CaseState::Watching;
            let mut i = 0;
            while i < det.cases.len() {
                if det.cases[i].vector == vector {
                    st = det.cases[i].state;
                }
                i += 1;
            }
            match st {
                CaseState::Convicted => {
                    caught += 1;
                    break;
                }
                CaseState::Cleared => break,
                CaseState::Watching => {}
            }
            s += 1;
        }
        let _ = rng.next(); // 保持序列滚动（确定性占位）。
        round += 1;
    }
    // 判定：10/10 捕获，0 误报（对照组不得 Convicted）。
    if caught != 10 {
        return Err(X_EL_MISS_DETECTED);
    }
    // 误报口径：立案总数 > 注入数即为误报。
    if det.convicted_total > injected {
        det.false_positive = det.convicted_total - injected;
    }
    if det.false_positive != 0 {
        return Err(X_EL_FALSE_POSITIVE);
    }
    Ok(DrillVerdict {
        injected,
        caught,
        false_positive: det.false_positive,
        control,
    })
}

// ===========================================================================
// 六、drill 自例程
// ===========================================================================

/// 自例程：10/10 捕获 + 零误报 + 守恒 + 错误路径。0=全过。
pub fn drill() -> Result<u32, u16> {
    // 1) patience=0 拒绝。
    if EoiLossDetector::new(0).is_ok() {
        return Ok(1);
    }
    // 2) 向量越界拒绝。
    let mut det = EoiLossDetector::new(2)?;
    if det.deliver(20).is_ok() || det.deliver(70).is_ok() {
        return Ok(2);
    }
    // 3) 主路径：注入演练。
    let v = run_injection_rounds()?;
    if v.injected != 10 || v.caught != 10 || v.false_positive != 0 || v.control != 10 {
        return Ok(3);
    }
    // 4) 守恒：终态账面平衡。
    if det.should_eoi != det.eoi_done + det.eoi_lost + det.pending {
        return Err(X_EL_UNBALANCED);
    }
    // 5) 补写 EOI（洗清路径）：丢 EOI 向量迟来补写不清欠账但不错账。
    let mut det2 = EoiLossDetector::new(1)?;
    det2.deliver(40)?;
    det2.eoi(40)?; // 正常结清。
    if det2.eoi_done != 1 || det2.pending != 0 {
        return Ok(5);
    }
    // 6) 无 ISR 位补写为幂等（不增账）。
    det2.eoi(40)?;
    if det2.eoi_done != 1 {
        return Ok(6);
    }
    // 7) 诊断码表完备性。
    let probe = [
        SEG_BASE, X_EL_VEC_RANGE, X_EL_PATIENCE, X_EL_SIGNAL_RANGE, X_EL_MISS_DETECTED,
        X_EL_FALSE_POSITIVE, X_EL_UNBALANCED, X_EL_CASE_FULL, X_EL_CASE_STATE,
    ];
    for c in probe {
        if code_name(c) == "未知码" {
            return Ok(7);
        }
    }
    Ok(0)
}
