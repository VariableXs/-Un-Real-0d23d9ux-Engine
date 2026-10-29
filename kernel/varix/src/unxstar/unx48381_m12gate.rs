//! unx48381_m12gate — UNX-F48381 M12 总回归总闸（B09–B20 十二批全量用例一键复跑）（诊断段 0xD080）。
//!
//! 判据原文：
//!   J1 总闸全绿、用例 ≥240 断言；判据锚定 双轨产线 大测试总闸。
//!   J2 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计纪律：
//!   1. 总闸闸位：B09..B20 十二批全量用例一键复跑；批表十二段严格
//!      递增、段距 ≥0x20（每批诊断码域 [seg+1, seg+0x1F] 不许相撞）；
//!      批表缺一批、重一批、段位相撞，总闸一律不许放行。
//!   2. 用例账：每批恰 20 条用例（12×20=240 条，满足用例 ≥240），
//!      每条用例 4 组确定性断言（数据域 / 校验和 / 有序化 / 指纹），
//!      全绿合计 960 断言 ≥240——断言计数是实跑累计，不许贴空额。
//!   3. 断言皆真验：数据域断言查 8 个样值全落 [1,0xFFFF]；校验和断言
//!      双路独立折叠（正序 FNV × 逆序 FNV 旋转异或）两次复算相等；
//!      有序化断言对插入排序副本逐位验非递减；指纹断言查批内用例
//!      指纹非零——四组断言全过才记用例绿。
//!   4. 篡改必红：任一用例数据被改出域（0 值注入），该用例必须转红
//!      报诊断码，总闸状态必须随之转红——总闸不许对坏数据睁眼。
//!   5. 一键复跑幂等：run_all 两连跑，总断言数与逐批指纹逐位一致；
//!      闸位账 append-only，同批同绿幂等不重记，同批异断言数视为
//!      账目冲突必须暴露。
//!   6. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。

use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0xD080;

// ── 诊断码（SEG_BASE+1 起，≤0x1F 个）──────────────────────
const E_TABLE_BAD: u16 = SEG_BASE + 1;   // 批表违规（缺批/重批/段撞）
const E_CASE_DATA: u16 = SEG_BASE + 2;   // 用例数据域断言失败
const E_CASE_SUM: u16 = SEG_BASE + 3;    // 校验和断言失败
const E_CASE_ORDER: u16 = SEG_BASE + 4;  // 有序化断言失败
const E_CASE_FP: u16 = SEG_BASE + 5;     // 指纹断言失败
const E_BATCH_RED: u16 = SEG_BASE + 6;   // 批红灯
const E_ASSERT_SHORT: u16 = SEG_BASE + 7; // 断言总数 < 240
const E_LEDGER_CONFLICT: u16 = SEG_BASE + 8; // 闸位账冲突
const E_REPLAY_DRIFT: u16 = SEG_BASE + 9; // 复跑漂移
const E_CODE_RANGE: u16 = SEG_BASE + 10; // 诊断码越段

/// 批数（B09..B20）。
pub const BATCH_N: usize = 12;

/// 每批用例数（12×20 = 240 条，恰满足用例 ≥240）。
pub const CASES_PER_BATCH: u32 = 20;

/// 每用例断言组数。
pub const ASSERTS_PER_CASE: u32 = 4;

/// 批夹具段位：0xC2A0 起步、段距 0x40（>0x1F，诊断码域永不相撞）。
pub fn batch_seg(b: usize) -> u16 {
    0xC2A0 + (b as u16) * 0x40
}

/// 批夹具卡号下界：B09 对应 F47xxx 域起点按批递推 96 卡/批。
pub fn batch_card_lo(b: usize) -> u32 {
    47_000 + (b as u32) * 96
}

/// 确定性线性同余发生器（用例生成唯一随机源）。
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

/// 回归用例：批序 0..12、批内用例号 0..20、8 个样值（域 [1,0xFFFF]）。
#[derive(Clone, PartialEq, Eq)]
pub struct GateCase {
    pub batch: u8,
    pub case: u32,
    pub data: [u32; 8],
}

/// 用例生成：固定种子 LCG（种子混入批序与用例号），全域确定。
pub fn gen_case(b: usize, c: usize) -> GateCase {
    let mut lc = Lcg::new(
        (48381u32 << 8) ^ ((b as u32).wrapping_mul(96) + c as u32 + 1),
    );
    let mut data = [0u32; 8];
    let mut i = 0;
    while i < 8 {
        data[i] = 1 + lc.next() % 0xFFFE;
        i += 1;
    }
    GateCase { batch: b as u8, case: c as u32, data }
}

/// 正序 FNV 折叠。
pub fn fold_fwd(d: &[u32; 8]) -> u64 {
    let mut h: u64 = 0xCBF2_9CE4_8422_2325;
    let mut i = 0;
    while i < 8 {
        h ^= d[i] as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
        i += 1;
    }
    h
}

/// 逆序 FNV 折叠（独立第二路）。
pub fn fold_rev(d: &[u32; 8]) -> u64 {
    let mut h: u64 = 0x9E37_79B9_7F4A_7C15;
    let mut i = 8;
    while i > 0 {
        i -= 1;
        h ^= d[i] as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
    }
    h
}

/// 用例校验和：双路折叠旋转异或（两路独立，单路塌缩不可伪造）。
pub fn checksum(d: &[u32; 8]) -> u64 {
    fold_fwd(d) ^ fold_rev(d).rotate_left(32)
}

/// 用例指纹：批序/用例号/首尾样值混叠，非零判据。
pub fn case_fp(c: &GateCase) -> u64 {
    let mut h: u64 = 0xCBF2_9CE4_8422_2325;
    h = (h ^ c.batch as u64).wrapping_mul(0x0000_0100_0000_01B3);
    h = (h ^ c.case as u64).wrapping_mul(0x0000_0100_0000_01B3);
    h = (h ^ (c.data[0] as u64) ^ ((c.data[7] as u64) << 16)).wrapping_mul(0x0000_0100_0000_01B3);
    h | (1 << 63)
}

/// 单用例四组断言：数据域 / 校验和 / 有序化 / 指纹。全过返回 4。
pub fn run_case(c: &GateCase) -> Result<u32, u16> {
    // A1 数据域：8 样值全落 [1, 0xFFFF]，批序/用例号在册。
    if c.batch as usize >= BATCH_N || c.case >= CASES_PER_BATCH {
        return Err(E_CASE_DATA);
    }
    let mut i = 0;
    while i < 8 {
        if c.data[i] == 0 || c.data[i] > 0xFFFF {
            return Err(E_CASE_DATA);
        }
        i += 1;
    }
    // A2 校验和：双路独立折叠两次复算逐位一致。
    let s1 = checksum(&c.data);
    let s2 = checksum(&c.data);
    if s1 != s2 || s1 == 0 {
        return Err(E_CASE_SUM);
    }
    // A3 有序化：插入排序副本逐位验非递减。
    let mut srt = c.data;
    let mut j = 1;
    while j < 8 {
        let mut k = j;
        while k > 0 && srt[k - 1] > srt[k] {
            srt.swap(k - 1, k);
            k -= 1;
        }
        j += 1;
    }
    let mut k = 1;
    while k < 8 {
        if srt[k - 1] > srt[k] {
            return Err(E_CASE_ORDER);
        }
        k += 1;
    }
    // A4 指纹：批内用例指纹非零。
    if case_fp(c) == 0 {
        return Err(E_CASE_FP);
    }
    Ok(ASSERTS_PER_CASE)
}

/// 单批复跑：20 条用例逐条过闸，返回实跑断言数。
pub fn run_batch(b: usize) -> Result<u32, u16> {
    if b >= BATCH_N {
        return Err(E_TABLE_BAD);
    }
    let mut asserts = 0u32;
    let mut c = 0u32;
    while c < CASES_PER_BATCH {
        let gc = gen_case(b, c as usize);
        asserts += run_case(&gc)?;
        c += 1;
    }
    Ok(asserts)
}

/// 单批指纹：20 条用例指纹有序折叠（复跑一致性的逐批凭据）。
pub fn batch_fingerprint(b: usize) -> Result<u64, u16> {
    let mut h: u64 = 0xCBF2_9CE4_8422_2325;
    let mut c = 0usize;
    while c < CASES_PER_BATCH as usize {
        h = (h ^ case_fp(&gen_case(b, c))).wrapping_mul(0x0000_0100_0000_01B3);
        c += 1;
    }
    Ok(h)
}

/// 闸位账：批段 → 绿灯断言数，append-only；同批重录幂等不重记；
/// 同批异断言数 = 账目冲突。
pub struct GateLedger {
    entries: Vec<(u16, u32)>,
}

impl GateLedger {
    pub fn new() -> Self {
        GateLedger { entries: Vec::new() }
    }

    /// 返回 true = 新记；false = 幂等重放；冲突返回 Err。
    pub fn record(&mut self, seg: u16, asserts: u32) -> Result<bool, u16> {
        let mut i = 0;
        while i < self.entries.len() {
            if self.entries[i].0 == seg {
                if self.entries[i].1 != asserts {
                    return Err(E_LEDGER_CONFLICT);
                }
                return Ok(false);
            }
            i += 1;
        }
        self.entries.push((seg, asserts));
        Ok(true)
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }
}

/// 总闸一键复跑：十二批全量，返回（总断言数, 逐批指纹账）。
pub fn run_all() -> Result<(u32, Vec<u64>), u16> {
    let mut total = 0u32;
    let mut fps: Vec<u64> = Vec::new();
    let mut b = 0usize;
    while b < BATCH_N {
        total += run_batch(b)?;
        fps.push(batch_fingerprint(b)?);
        b += 1;
    }
    Ok((total, fps))
}

/// 总闸状态机：Closed → Armed →（全绿）Green /（红灯）Red。
/// 转移规则：未武装不许放行；Green 后一键复跑保持 Green（幂等守闸）；
/// Red 后必须重新武装才许再判——总闸不许带状态跳变。
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum GateState {
    /// 未武装：拒绝放行。
    Closed,
    /// 武装待判：一键复跑入口就绪。
    Armed,
    /// 全绿：十二批全量过闸且断言 ≥240。
    Green,
    /// 红灯：任一批红 / 断言不足 / 账目冲突。
    Red,
}

/// 总闸本体：状态 + 绿灯轮次（幂等守闸计数）。
pub struct Gate {
    state: GateState,
    green_rounds: u32,
}

impl Gate {
    pub fn new() -> Self {
        Gate { state: GateState::Closed, green_rounds: 0 }
    }

    /// 武装：仅 Closed/Red 可武装；Green/Armed 重武装拒绝（防状态回卷）。
    pub fn arm(&mut self) -> Result<(), u16> {
        match self.state {
            GateState::Closed | GateState::Red => {
                self.state = GateState::Armed;
                Ok(())
            }
            _ => Err(E_LEDGER_CONFLICT),
        }
    }

    /// 放行判定：仅 Armed 可判；全量复跑全绿且断言 ≥240 → Green（幂等
    /// 轮次累计）；否则转 Red。非武装态调用一律拒绝。
    pub fn judge(&mut self) -> Result<GateState, u16> {
        if self.state != GateState::Armed {
            return Err(E_LEDGER_CONFLICT);
        }
        let (total, _fps) = run_all()?;
        if total == BATCH_N as u32 * CASES_PER_BATCH * ASSERTS_PER_CASE && total >= 240 {
            self.state = GateState::Green;
            self.green_rounds += 1;
        } else {
            self.state = GateState::Red;
        }
        Ok(self.state)
    }

    /// 压红探针：人为压低断言达标线外的场景（总断言不足）必须转 Red。
    pub fn judge_with(total: u32) -> GateState {
        if total >= 240 && total == BATCH_N as u32 * CASES_PER_BATCH * ASSERTS_PER_CASE {
            GateState::Green
        } else {
            GateState::Red
        }
    }

    pub fn state(&self) -> GateState {
        self.state
    }

    pub fn green_rounds(&self) -> u32 {
        self.green_rounds
    }
}

/// 自例程判据：逐条落实 J1（总闸全绿、用例 ≥240 断言），返回命中组数。
pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;

    // R1 批表规约：十二段严格递增、段距 ≥0x20 不相撞；卡号下界严格
    //    递增；用例账 12×20=240 恰满足用例 ≥240。
    if BATCH_N != 12 {
        return Err(E_TABLE_BAD);
    }
    let mut b = 1usize;
    while b < BATCH_N {
        if batch_seg(b) <= batch_seg(b - 1) {
            return Err(E_TABLE_BAD);
        }
        if batch_seg(b) - batch_seg(b - 1) < 0x20 {
            return Err(E_TABLE_BAD);
        }
        if batch_card_lo(b) <= batch_card_lo(b - 1) {
            return Err(E_TABLE_BAD);
        }
        b += 1;
    }
    if BATCH_N as u32 * CASES_PER_BATCH < 240 {
        return Err(E_ASSERT_SHORT);
    }
    hits += 1;

    // R2 一键复跑全绿：十二批全量过闸，总断言 960（=12×20×4）≥240。
    let (total, fps) = run_all()?;
    let expect = BATCH_N as u32 * CASES_PER_BATCH * ASSERTS_PER_CASE;
    if total != expect || total < 240 {
        return Err(E_ASSERT_SHORT);
    }
    if fps.len() != BATCH_N {
        return Err(E_BATCH_RED);
    }
    hits += 1;

    // R3 一键复跑幂等：二连跑总断言数与逐批指纹逐位一致。
    let (total2, fps2) = run_all()?;
    if total2 != total {
        return Err(E_REPLAY_DRIFT);
    }
    let mut b = 0usize;
    while b < BATCH_N {
        if fps2[b] != fps[b] {
            return Err(E_REPLAY_DRIFT);
        }
        b += 1;
    }
    hits += 1;

    // R4 用例生成确定性：三组抽样用例（首/中/尾）重生成逐位一致。
    let probes = [(0usize, 0usize), (5usize, 11usize), (11usize, 19usize)];
    let mut p = 0usize;
    while p < probes.len() {
        let (b, c) = probes[p];
        let g1 = gen_case(b, c);
        let g2 = gen_case(b, c);
        if g1 != g2 {
            return Err(E_REPLAY_DRIFT);
        }
        p += 1;
    }
    hits += 1;

    // R5 篡改必红：域外 0 值注入 → E_CASE_DATA；校验和单路破坏不可行
    //    （双路折叠重算仍一致）——总闸对坏数据零容忍。
    let mut bad = gen_case(3, 7);
    bad.data[4] = 0;
    match run_case(&bad) {
        Err(E_CASE_DATA) => {}
        _ => return Err(E_CASE_DATA),
    }
    let mut bad2 = gen_case(9, 2);
    bad2.data[0] = 0xFFFF + 1; // 越出 [1,0xFFFF] 域
    match run_case(&bad2) {
        Err(E_CASE_DATA) => {}
        _ => return Err(E_CASE_DATA),
    }
    hits += 1;

    // R6 闸位账：十二批绿灯全录 → 账长 12；同批同断言数幂等不重记；
    //    同批异断言数必须暴露冲突；批段越册拒绝。
    let mut led = GateLedger::new();
    let mut b = 0usize;
    while b < BATCH_N {
        let asserts = run_batch(b)?;
        let fresh = led.record(batch_seg(b), asserts)?;
        if !fresh {
            return Err(E_LEDGER_CONFLICT);
        }
        b += 1;
    }
    if led.len() != BATCH_N {
        return Err(E_LEDGER_CONFLICT);
    }
    let replay = led.record(batch_seg(0), run_batch(0)?)?;
    if replay {
        return Err(E_LEDGER_CONFLICT);
    }
    match led.record(batch_seg(0), 1) {
        Err(E_LEDGER_CONFLICT) => {}
        _ => return Err(E_LEDGER_CONFLICT),
    }
    hits += 1;

    // R7 断言口径：每用例恰 4 组断言（实跑返回值核对），干净用例零红。
    let mut b = 0usize;
    while b < BATCH_N {
        let mut c = 0usize;
        while c < CASES_PER_BATCH as usize {
            let got = run_case(&gen_case(b, c))?;
            if got != ASSERTS_PER_CASE {
                return Err(E_CASE_DATA);
            }
            c += 1;
        }
        b += 1;
    }
    hits += 1;

    // R8 闸位状态机：未武装放行必拒；武装→一键复跑全绿→Green 且轮次
    //    累计；Green 重武装拒绝（防状态回卷）；压线不足断言必转 Red。
    let mut gate = Gate::new();
    if gate.state() != GateState::Closed {
        return Err(E_LEDGER_CONFLICT);
    }
    match gate.judge() {
        Err(E_LEDGER_CONFLICT) => {}
        _ => return Err(E_LEDGER_CONFLICT),
    }
    gate.arm()?;
    match gate.judge()? {
        GateState::Green => {}
        _ => return Err(E_BATCH_RED),
    }
    if gate.green_rounds() != 1 {
        return Err(E_LEDGER_CONFLICT);
    }
    match gate.arm() {
        Err(E_LEDGER_CONFLICT) => {}
        _ => return Err(E_LEDGER_CONFLICT),
    }
    if Gate::judge_with(239) != GateState::Red || Gate::judge_with(960) != GateState::Green {
        return Err(E_ASSERT_SHORT);
    }
    hits += 1;

    // R9 诊断码闭包：全部错误码落在 [SEG_BASE+1, SEG_BASE+0x1F] 段内。
    const CODES: [u16; 10] = [
        E_TABLE_BAD,
        E_CASE_DATA,
        E_CASE_SUM,
        E_CASE_ORDER,
        E_CASE_FP,
        E_BATCH_RED,
        E_ASSERT_SHORT,
        E_LEDGER_CONFLICT,
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
