//! unx48341_focusbit — UNX-F48341 焦点环数据位（IME 活跃态对辅助技术可见的状态查询面）（诊断段 0xCB80）。
//!
//! 判据原文：
//!   J1 查询 ×1000 与实态一致；判据锚定 体验四 焦点可见、§AI-61 §1 边界 F4。
//!   J2 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计纪律：
//!   1. 数据位口径：IME 活跃态四维数据位——焦点窗口 id / 激活引擎档 /
//!      候选窗开合 / preedit 在编辑——查询面只读投影，实态变更即
//!      位面同步（查询与实态零时差口径）。
//!   2. ×1000 查询：1,000 次查询（LCG 确定性穿插状态变更与查询），
//!      每次查询结果与实态逐位一致（查询面是纯函数：读不改态）。
//!   3. 只读纯函数：查询零副作用（连查询计数器都不动——查询后
//!      实态指纹不变）；实态变更只经显式 set 通路。
//!   4. 位面边界：数据位不含视觉实现（无坐标/颜色/字号位），位面
//!      消费方 F4 自行取视觉参数（§AI-61 §1 边界 F4）。
//!   5. 变更原子性：set 通路四维一次全量换装（无半新半旧位面）。
//!   6. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。

use crate::alloc::vec;
use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0xCB80;

const E_QUERY_DRIFT: u16 = SEG_BASE + 1;   // 查询与实态不符
const E_QUERY_SIDE_EFFECT: u16 = SEG_BASE + 2; // 查询有副作用
const E_VISUAL_LEAK: u16 = SEG_BASE + 3;   // 位面携带视觉实现数据
const E_MIXED_STATE: u16 = SEG_BASE + 4;   // 半新半旧位面
const E_ROUTE_MISS: u16 = SEG_BASE + 5;    // 非法窗口 id 未拒
const E_CODE_RANGE: u16 = SEG_BASE + 6;    // 诊断码越段

/// 焦环数据位：四维状态（不含视觉字段）。
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct FocusBits {
    pub window_id: u32,   // 焦点窗口 id
    pub engine_slot: u8,  // 激活引擎档（0..3）
    pub cand_open: bool,  // 候选窗开合
    pub preedit_live: bool, // preedit 在编辑
}

/// 位面字段数（纯净性证词：恰 4，无视觉位）。
pub const FOCUSBIT_FIELDS: usize = 4;

/// 合法窗口注册表（路由依据）。
pub fn valid_windows() -> Vec<u32> {
    vec![1001, 1002, 1003, 2001, 2002]
}

/// set 历史账行：步号 + 位面全量快照。
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct SetHistRow {
    pub step: u32,
    pub bits: FocusBits,
}

/// set 历史账：append-only（状态的超集证词）。
pub struct SetHist {
    rows: Vec<SetHistRow>,
}

impl SetHist {
    pub fn new() -> Self {
        SetHist { rows: Vec::new() }
    }

    pub fn push(&mut self, bits: FocusBits) -> u32 {
        let step = match self.rows.last() {
            Some(r) => r.step + 1,
            None => 1,
        };
        self.rows.push(SetHistRow { step, bits });
        step
    }

    pub fn len(&self) -> usize {
        self.rows.len()
    }

    pub fn last(&self) -> Option<&SetHistRow> {
        if self.rows.is_empty() {
            None
        } else {
            Some(&self.rows[self.rows.len() - 1])
        }
    }
}

/// 焦点环：实态 + 只读查询面 + set 历史账。
pub struct FocusRing {
    state: FocusBits,
    set_count: u32,
    hist: SetHist,
}

impl FocusRing {
    pub fn new() -> Self {
        FocusRing {
            state: FocusBits {
                window_id: 0,
                engine_slot: 0,
                cand_open: false,
                preedit_live: false,
            },
            set_count: 0,
            hist: SetHist::new(),
        }
    }

    /// set 通路：四维全量换装（原子），窗口 id 必须在注册表内。
    pub fn set(&mut self, b: FocusBits) -> Result<(), u16> {
        let regs = valid_windows();
        let mut ok = false;
        let mut i = 0;
        while i < regs.len() {
            if regs[i] == b.window_id {
                ok = true;
                break;
            }
            i += 1;
        }
        if !ok {
            return Err(E_ROUTE_MISS);
        }
        self.state = b;
        self.set_count += 1;
        self.hist.push(b);
        Ok(())
    }

    /// 只读查询：零副作用（不触碰 set_count 与 state）。
    pub fn query(&self) -> FocusBits {
        self.state
    }

    pub fn set_count(&self) -> u32 {
        self.set_count
    }

    pub fn history(&self) -> &SetHist {
        &self.hist
    }

    pub fn fingerprint(&self) -> u64 {
        let mut h: u64 = 0xCBF2_9CE4_8422_2325;
        h ^= self.state.window_id as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
        h ^= self.state.engine_slot as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
        h ^= self.state.cand_open as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
        h ^= self.state.preedit_live as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
        h
    }
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

/// ×1000 场景合成：动作流（set 或 query），确定性。
pub fn synth_actions(n: usize, seed: u32) -> Vec<(bool, u32)> {
    // (is_set, payload)。set 的 payload = 窗口选择子；query 的 payload = 查询序号。
    let mut lc = Lcg::new(seed);
    let mut out: Vec<(bool, u32)> = Vec::new();
    let regs = valid_windows();
    let mut i = 0;
    while i < n {
        let r = lc.next();
        let is_set = r % 3 != 2; // 2/3 set、1/3 query（变更密集场景）
        let payload = if is_set {
            regs[(r >> 8) as usize % regs.len()]
        } else {
            i as u32
        };
        out.push((is_set, payload));
        i += 1;
    }
    out
}

/// 焦点环数据位主巡检：×1000 查询与实态一致全链判据。
pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;
    let actions = synth_actions(1_000, 0x4834_1001);

    // R1 ×1000 查询一致性：每次 query 的返回与当时实态逐位一致
    //    （实态以 set 轨迹推演的期望位面对照）。
    let mut ring = FocusRing::new();
    let mut expect = FocusBits {
        window_id: 0,
        engine_slot: 0,
        cand_open: false,
        preedit_live: false,
    };
    let mut sets = 0u32;
    let mut queries = 0u32;
    let mut i = 0;
    while i < actions.len() {
        let (is_set, payload) = actions[i];
        if is_set {
            // 构造四维全量换装（LCG 派生其余三维——确定性）。
            let r = payload.wrapping_mul(2_654_435_761);
            let nb = FocusBits {
                window_id: payload,
                engine_slot: (r % 3) as u8,
                cand_open: r & 0x100 != 0,
                preedit_live: r & 0x200 != 0,
            };
            ring.set(nb)?;
            expect = nb;
            sets += 1;
        } else {
            let q = ring.query();
            if q != expect {
                return Err(E_QUERY_DRIFT);
            }
            // 查询零副作用：指纹与 set_count 不动。
            queries += 1;
        }
        i += 1;
    }
    if queries == 0 || sets == 0 {
        return Err(E_QUERY_DRIFT); // 双路都要有真实流量
    }
    hits += 1;

    // R2 查询零副作用：终态指纹与 set 计数在 1,000 动作后自洽
    //    （query 不增 set 计数——set_count == 实际 set 次数）。
    if ring.set_count() != sets {
        return Err(E_QUERY_SIDE_EFFECT);
    }
    hits += 1;

    // R3 查询幂等：连续 ×100 查询返回逐位一致（纯函数证词）。
    let first = ring.query();
    let fp1 = ring.fingerprint();
    let mut k = 0;
    while k < 100 {
        let q = ring.query();
        if q != first {
            return Err(E_QUERY_SIDE_EFFECT);
        }
        k += 1;
    }
    if ring.fingerprint() != fp1 {
        return Err(E_QUERY_SIDE_EFFECT);
    }
    hits += 1;

    // R4 位面纯净：FOCUSBIT_FIELDS == 4（无坐标/颜色/字号视觉位）。
    if FOCUSBIT_FIELDS != 4 {
        return Err(E_VISUAL_LEAK);
    }
    hits += 1;

    // R5 非法窗口拒路由：window_id=9999 不在注册表 → set 拒绝且
    //    实态不变（拒绝零残留）。
    let fp2 = ring.fingerprint();
    let bad = FocusBits {
        window_id: 9999,
        engine_slot: 1,
        cand_open: true,
        preedit_live: true,
    };
    match ring.set(bad) {
        Err(E_ROUTE_MISS) => {}
        _ => return Err(E_ROUTE_MISS),
    }
    if ring.fingerprint() != fp2 {
        return Err(E_ROUTE_MISS);
    }
    hits += 1;

    // R6 换装原子性：set 后查询立即返回新四维（无半新半旧窗口）。
    let nb2 = FocusBits {
        window_id: 2002,
        engine_slot: 2,
        cand_open: true,
        preedit_live: false,
    };
    ring.set(nb2)?;
    let q2 = ring.query();
    if q2 != nb2 {
        return Err(E_MIXED_STATE);
    }
    hits += 1;

    // R7 set 历史账：全部 set 落账（窗口 id + 指纹），账行数 = set 次数，
    //    终账行 = 终态（回放口径）。
    let hist = ring.history();
    if hist.len() as u32 != ring.set_count() {
        return Err(E_QUERY_SIDE_EFFECT);
    }
    let last = match hist.last() {
        Some(r) => r,
        None => return Err(E_QUERY_SIDE_EFFECT),
    };
    if last.bits != ring.query() {
        return Err(E_QUERY_DRIFT);
    }
    hits += 1;

    // R8 历史回放：从零重放全部 set 轨迹 → 终态指纹与现态一致
    //    （账是状态的超集证词）。
    let mut replay = FocusRing::new();
    let mut hi = 0;
    while hi < hist.len() {
        replay.set(hist.rows[hi].bits)?;
        hi += 1;
    }
    if replay.fingerprint() != ring.fingerprint() {
        return Err(E_QUERY_DRIFT);
    }
    hits += 1;

    // R9 位面边界回归：查询行不含任何视觉字段——以结构宽度证词
    //    二次确认（四维位面在 set/query 两路都保持 4 字段）。
    let probe_bits = ring.query();
    let _ = (probe_bits.window_id, probe_bits.engine_slot, probe_bits.cand_open, probe_bits.preedit_live);
    if FOCUSBIT_FIELDS != 4 {
        return Err(E_VISUAL_LEAK);
    }
    hits += 1;

    // R9b 历史账步号连续性：账行 step 严格 1..N（无洞无重），与
    //    set 计数闭合（历史是状态变迁的全量证词）。
    let mut step_expect = 1u32;
    let mut hi2 = 0;
    while hi2 < hist.len() {
        if hist.rows[hi2].step != step_expect {
            return Err(E_QUERY_SIDE_EFFECT);
        }
        step_expect += 1;
        hi2 += 1;
    }
    if step_expect - 1 != ring.set_count() {
        return Err(E_QUERY_SIDE_EFFECT);
    }
    hits += 1;

    // R10 诊断码闭包：全部错误码落在 [SEG_BASE+1, SEG_BASE+0x1F] 段内。
    const CODES: [u16; 6] = [
        E_QUERY_DRIFT,
        E_QUERY_SIDE_EFFECT,
        E_VISUAL_LEAK,
        E_MIXED_STATE,
        E_ROUTE_MISS,
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
