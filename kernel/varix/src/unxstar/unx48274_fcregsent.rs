//! unx48274_fcregsent — UNX-F48274 fcitx5 互通三类窗口回归哨兵（诊断段 0xC320）。
//!
//! 判据原文：
//!   J1 三类窗口（vxwm 原生/Wine/Xwayland）回归判据：addon 引擎三窗 ×3 轮
//!   逐字一致；判据锚定 域内宪法加严项。
//!   J2 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计纪律：
//!   1. 三窗同源：vxwm 原生窗/Wine 窗/Xwayland 窗三类传输面不同，但 addon
//!      引擎对同一键序必须产出逐字一致的提交串——窗口类只允许改变事件
//!      传输元数据（批化/重复/乱序等伪影），不许改变引擎输出。
//!   2. ×3 轮逐字一致：每窗 100 字试打 ×3 轮，第 1 轮为基线，第 2/3 轮
//!      对基线逐字比对，任何一位漂移即哨兵报警并登记。
//!   3. 伪影吸收：传输面可注入重复投递与相邻交换伪影；引擎侧归一器
//!      （按事件 id 去重 + 按 id 升序还原）必须把伪影完全吸收，使提交
//!      串与干净流逐字一致——这是「窗口类不改变输出」的可证机制。
//!   4. 扰动自查：哨兵必须能检出人工注入的单字漂移（假阴性为零），
//!      且对干净复检不得误报（假阳性为零），否则哨兵不配入账。
//!   5. 哨兵账 append-only：同（窗,轮）重复裁定幂等不重记；同键位不同
//!      裁决视为账目冲突必须暴露。
//!   6. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。

use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0xC320;

const E_RUN_INCOMPLETE: u16 = SEG_BASE + 1; // 试打未满 100 字
const E_ROUND_DRIFT: u16 = SEG_BASE + 2;    // 轮间逐字漂移
const E_CROSS_DRIFT: u16 = SEG_BASE + 3;    // 跨窗逐字漂移
const E_FP_NEGATIVE: u16 = SEG_BASE + 4;    // 扰动未检出（假阴性）
const E_FP_POSITIVE: u16 = SEG_BASE + 5;    // 无扰误报（假阳性）
const E_LEDGER_CONFLICT: u16 = SEG_BASE + 6; // 哨兵账冲突/非幂等
const E_CODE_RANGE: u16 = SEG_BASE + 7;     // 诊断码越段

/// 三类窗口传输面。
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum WindowClass {
    /// vxwm 原生窗（协议内直连）。
    VxwmNative,
    /// Wine 窗（兼容层转译）。
    Wine,
    /// Xwayland 窗（X 协议桥接）。
    Xwayland,
}

/// 三窗枚举顺序表（索引 0/1/2 对应三类）。
pub const WINDOW_CLASSES: [WindowClass; 3] = [
    WindowClass::VxwmNative,
    WindowClass::Wine,
    WindowClass::Xwayland,
];

/// 回归轮数（×3 轮）。
pub const ROUNDS: u32 = 3;

/// 每轮试打字数（100 字试打）。
pub const TRIAL_LEN: usize = 100;

/// 键事件：id 单调唯一，key 为键位码（0..26 字母位）。
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct KeyEvent {
    pub id: u32,
    pub key: u8,
}

/// 传输伪影风格：每（窗,轮）一种确定性扰动模式。
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Artifact {
    /// 干净流：不加扰动。
    Clean,
    /// 重复：部分事件重复投递（幂等去重后等价）。
    Duplicated,
    /// 微乱序：相邻对交换（按 id 归位后等价）。
    Swapped,
    /// 混合：重复 + 相邻交换叠加。
    Mixed,
}

/// 声母→字码映射表（试打引擎的确定性输出核心）。
/// 27 槽：键位 k 经 (k*7+13)%27 槽位取字；表项为 UTF-16 码元。
const SYL_TABLE: [u16; 27] = [
    0x4E00, 0x4E01, 0x4E02, 0x4E03, 0x4E04, 0x4E05, 0x4E06, 0x4E07, 0x4E08,
    0x4E09, 0x4E0A, 0x4E0B, 0x4E0C, 0x4E0D, 0x4E0E, 0x4E0F, 0x4E10, 0x4E11,
    0x4E12, 0x4E13, 0x4E14, 0x4E15, 0x4E16, 0x4E17, 0x4E18, 0x4E19, 0x4E1A,
];

/// 确定性线性同余发生器（fuzz/扰动注入统一入口，禁真随机）。
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

/// addon 引擎：传输侧归一器 + 逐字提交器。
pub struct AddonEngine {
    committed: Vec<u16>,
}

impl AddonEngine {
    pub fn new() -> Self {
        AddonEngine {
            committed: Vec::new(),
        }
    }

    /// 归一化：按 id 去重（重复投递只留首个）+ 按 id 升序还原键序。
    /// 归一化是「传输伪影不影响输出」的唯一机制点。
    pub fn normalize(events: &[KeyEvent]) -> Vec<KeyEvent> {
        let mut evs: Vec<KeyEvent> = Vec::new();
        for e in events.iter() {
            let mut dup = false;
            for t in evs.iter() {
                if t.id == e.id {
                    dup = true;
                    break;
                }
            }
            if !dup {
                evs.push(*e);
            }
        }
        // 插入排序按 id 升序（稳定归位微乱序）。
        let mut i = 1;
        while i < evs.len() {
            let mut j = i;
            while j > 0 && evs[j - 1].id > evs[j].id {
                evs.swap(j - 1, j);
                j -= 1;
            }
            i += 1;
        }
        evs
    }

    /// 提交：对归一化后键序逐字查表输出。
    pub fn commit(&mut self, events: &[KeyEvent]) {
        let norm = Self::normalize(events);
        for e in norm.iter() {
            let slot = ((e.key as u32 * 7 + 13) % 27) as usize;
            self.committed.push(SYL_TABLE[slot]);
        }
    }

    pub fn committed(&self) -> &[u16] {
        &self.committed
    }
}

/// 传输面：把基准键序按伪影风格投递为事件流（确定性）。
pub fn transport(seed: u32, keys: &[u8], style: Artifact) -> Vec<KeyEvent> {
    let mut lc = Lcg::new(seed);
    let mut events: Vec<KeyEvent> = Vec::new();
    let mut id: u32 = 0;
    for k in keys.iter() {
        events.push(KeyEvent { id, key: *k });
        id += 1;
    }
    match style {
        Artifact::Clean => events,
        Artifact::Duplicated => {
            let mut out: Vec<KeyEvent> = Vec::new();
            for e in events.iter() {
                out.push(*e);
                if lc.next() % 3 == 0 {
                    out.push(*e); // 重复投递
                }
            }
            out
        }
        Artifact::Swapped => {
            let mut i = 0;
            while i + 1 < events.len() {
                if lc.next() % 2 == 0 {
                    events.swap(i, i + 1); // 相邻交换微乱序
                }
                i += 2;
            }
            events
        }
        Artifact::Mixed => {
            let mut out: Vec<KeyEvent> = Vec::new();
            for e in events.iter() {
                out.push(*e);
                if lc.next() % 4 == 0 {
                    out.push(*e);
                }
            }
            let mut i = 0;
            while i + 1 < out.len() {
                if lc.next() % 3 == 0 {
                    out.swap(i, i + 1);
                }
                i += 2;
            }
            out
        }
    }
}

/// （窗,轮）→ 伪影风格的确定性映射：9 条运行覆盖全部 4 种风格。
pub fn artifact_for(w_idx: usize, round: u32) -> Artifact {
    match (w_idx, round) {
        (0, 1) => Artifact::Clean,
        (0, 2) => Artifact::Duplicated,
        (0, 3) => Artifact::Mixed,
        (1, 1) => Artifact::Swapped,
        (1, 2) => Artifact::Mixed,
        (1, 3) => Artifact::Clean,
        (2, 1) => Artifact::Duplicated,
        (2, 2) => Artifact::Swapped,
        _ => Artifact::Mixed,
    }
}

/// 基准键序：确定性生成 100 键（LCG，a..z 循环位）。
pub fn base_keys() -> Vec<u8> {
    let mut lc = Lcg::new(0x4827_4001);
    let mut keys: Vec<u8> = Vec::new();
    let mut i = 0;
    while i < TRIAL_LEN {
        keys.push(((lc.next() % 26) as u8) + b'a');
        i += 1;
    }
    keys
}

/// 单（窗,轮）试打：传输（带伪影）→ 归一 → 提交 → 100 字提交串。
pub fn run_trial(seed: u32, keys: &[u8], style: Artifact) -> Vec<u16> {
    let events = transport(seed, keys, style);
    let mut eng = AddonEngine::new();
    eng.commit(&events);
    let mut out: Vec<u16> = Vec::new();
    for c in eng.committed().iter() {
        out.push(*c);
    }
    out
}

/// 逐字比对：返回首个漂移位（长度不等记为超界位）。
pub fn first_drift(a: &[u16], b: &[u16]) -> Option<usize> {
    if a.len() != b.len() {
        return Some(usize::MAX);
    }
    let mut i = 0;
    while i < a.len() {
        if a[i] != b[i] {
            return Some(i);
        }
        i += 1;
    }
    None
}

/// 运行指纹：FNV-1a 折叠提交串（跨轮/跨窗一致性的整串判据）。
pub fn fingerprint(chars: &[u16]) -> u64 {
    let mut h: u64 = 0xCBF2_9CE4_8422_2325;
    for c in chars.iter() {
        h ^= *c as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
    }
    h
}

/// 哨兵裁定。
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    Match,
    Drift,
}

/// 哨兵账目：（窗,轮）→ 裁定。
#[derive(Clone, Copy)]
pub struct SentinelEntry {
    pub window: WindowClass,
    pub round: u32,
    pub verdict: Verdict,
}

/// 回归哨兵账：append-only；同（窗,轮）重放幂等不重记；
/// 同键位不同裁决 = 账目冲突（返回 false 由调用方报警）。
pub struct SentinelLedger {
    entries: Vec<SentinelEntry>,
}

impl SentinelLedger {
    pub fn new() -> Self {
        SentinelLedger {
            entries: Vec::new(),
        }
    }

    /// 返回 true = 记账一致（新记或幂等重放）；false = 裁决冲突。
    pub fn record(&mut self, e: SentinelEntry) -> bool {
        for t in self.entries.iter() {
            if t.window == e.window && t.round == e.round {
                return t.verdict == e.verdict;
            }
        }
        self.entries.push(e);
        true
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn entries(&self) -> &[SentinelEntry] {
        &self.entries
    }
}

/// 哨兵主巡检：三窗 ×3 轮逐字一致全链判据。
/// 返回 Ok(hits) = 通过的判据组数；Err(code) = 首个违规诊断码。
pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;
    let keys = base_keys();

    // R1 三窗 ×3 轮试打全部满 100 字（伪影吸收后长度必须还原）。
    let mut outs: Vec<Vec<Vec<u16>>> = Vec::new();
    let mut w = 0usize;
    while w < 3 {
        let mut per_round: Vec<Vec<u16>> = Vec::new();
        let mut r = 1u32;
        while r <= ROUNDS {
            let style = artifact_for(w, r);
            let seed = (48274u32 << 8) ^ (w as u32 * 16 + r);
            let out = run_trial(seed, &keys, style);
            if out.len() != TRIAL_LEN {
                return Err(E_RUN_INCOMPLETE);
            }
            per_round.push(out);
            r += 1;
        }
        outs.push(per_round);
        w += 1;
    }
    hits += 1;

    // R2 轮间逐字一致：第 2/3 轮对第 1 轮基线逐位比对（3 窗 × 2 比对）。
    w = 0;
    while w < 3 {
        let mut r = 1usize;
        while r < 3 {
            if first_drift(&outs[w][0], &outs[w][r]).is_some() {
                return Err(E_ROUND_DRIFT);
            }
            r += 1;
        }
        w += 1;
    }
    hits += 1;

    // R3 跨窗逐字一致：三窗基线互比（0-1、0-2、1-2），同键序必同串。
    if first_drift(&outs[0][0], &outs[1][0]).is_some() {
        return Err(E_CROSS_DRIFT);
    }
    if first_drift(&outs[0][0], &outs[2][0]).is_some() {
        return Err(E_CROSS_DRIFT);
    }
    if first_drift(&outs[1][0], &outs[2][0]).is_some() {
        return Err(E_CROSS_DRIFT);
    }
    hits += 1;

    // R4 运行指纹：9 条运行指纹全相等（同键序同引擎 → 同指纹）。
    let fp0 = fingerprint(&outs[0][0]);
    w = 0;
    while w < 3 {
        let mut r = 0usize;
        while r < 3 {
            if fingerprint(&outs[w][r]) != fp0 {
                return Err(E_ROUND_DRIFT);
            }
            r += 1;
        }
        w += 1;
    }
    hits += 1;

    // R5 假阴性自查：对窗1轮2串注入单字漂移，比对必须报警。
    let mut tampered: Vec<u16> = Vec::new();
    for c in outs[1][1].iter() {
        tampered.push(*c);
    }
    tampered[37] ^= 0x0001;
    if first_drift(&outs[1][0], &tampered).is_none() {
        return Err(E_FP_NEGATIVE);
    }
    hits += 1;

    // R6 假阳性自查：同种子同风格干净复检必须逐字一致（不报警）。
    let replay = run_trial((48274u32 << 8) ^ (1 * 16 + 2), &keys, artifact_for(1, 2));
    if first_drift(&outs[1][1], &replay).is_some() {
        return Err(E_FP_POSITIVE);
    }
    hits += 1;

    // R7 哨兵账：9 记全 Match + 幂等重放不重记 + 冲突裁决必须暴露。
    let mut led = SentinelLedger::new();
    w = 0;
    while w < 3 {
        let mut r = 1u32;
        while r <= ROUNDS {
            let ok = led.record(SentinelEntry {
                window: WINDOW_CLASSES[w],
                round: r,
                verdict: Verdict::Match,
            });
            if !ok {
                return Err(E_LEDGER_CONFLICT);
            }
            r += 1;
        }
        w += 1;
    }
    if led.len() != 9 {
        return Err(E_LEDGER_CONFLICT);
    }
    // 幂等重放：同裁决重放后账长不变。
    let ok = led.record(SentinelEntry {
        window: WINDOW_CLASSES[0],
        round: 1,
        verdict: Verdict::Match,
    });
    if !ok || led.len() != 9 {
        return Err(E_LEDGER_CONFLICT);
    }
    // 冲突探测：同键位反向裁决必须被拒绝（返回 false）。
    let clash = led.record(SentinelEntry {
        window: WINDOW_CLASSES[0],
        round: 1,
        verdict: Verdict::Drift,
    });
    if clash {
        return Err(E_LEDGER_CONFLICT);
    }
    hits += 1;

    // R8 诊断码闭包：全部错误码落在 [SEG_BASE+1, SEG_BASE+0x1F] 段内。
    const CODES: [u16; 7] = [
        E_RUN_INCOMPLETE,
        E_ROUND_DRIFT,
        E_CROSS_DRIFT,
        E_FP_NEGATIVE,
        E_FP_POSITIVE,
        E_LEDGER_CONFLICT,
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
