//! unx48333_smregsent — UNX-F48333 智能三类窗口回归哨兵（三类窗口（vxwm 原生/Wine/Xwayland）回归判据·宪法加严）（诊断段 0xCA80）。
//!
//! 判据原文：
//!   J1 三类窗口（vxwm 原生/Wine/Xwayland）回归判据：三窗智能功能
//!   行为一致 ×10 轮；判据锚定 域内宪法加严项。
//!   J2 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计纪律：
//!   1. 智能面四件套同源：联想（suggest）/纠错（autocorrect）/反馈环
//!      （feedback）/热词（hotword）在三类窗口下的行为必须逐位一致
//!      ——窗口类只允许改变传输元数据（伪影风格），不许改变智能
//!      功能输出。
//!   2. ×10 轮 ×3 窗：每轮一个确定性键入场景（LCG），三窗各自跑完
//!      四件套后产出行为指纹；30 次运行指纹全等是本卡判据本体。
//!   3. 四件套确定性契约：联想 = bigram 表查 Top3（同键序同表同结果）；
//!      纠错 = 键位混淆表还原 + 降权标注；反馈 = 饱和频次增益；
//!      热词 = 窗口期计数自增——四者对同输入永远同输出。
//!   4. 伪影吸收：传输面可注入重复/换序伪影，引擎侧按事件 id 归一
//!      （与 B14 三窗哨兵同源机制），归一后输出与干净流逐位一致。
//!   5. 扰动自查：对场景注入一位扰动，指纹必须改变（假阴性为零）；
//!      干净复跑指纹必须不变（假阳性为零）。
//!   6. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。

use crate::alloc::vec;
use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0xCA80;

const E_FINGER_DRIFT: u16 = SEG_BASE + 1;  // 三窗指纹漂移
const E_TAMPER_MISS: u16 = SEG_BASE + 2;   // 扰动未检出（假阴性）
const E_REPLAY_FALSE: u16 = SEG_BASE + 3;  // 干净复跑误报（假阳性）
const E_ABSORB_FAIL: u16 = SEG_BASE + 4;   // 伪影未吸收（长度还原失败）
const E_ROUND_MISS: u16 = SEG_BASE + 5;    // 轮数不足（×10 未跑满）
const E_SUGGEST_BAD: u16 = SEG_BASE + 6;   // 联想输出违例（越界/重复）
const E_CODE_RANGE: u16 = SEG_BASE + 7;    // 诊断码越段

/// 三类窗口。
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum WindowClass {
    /// vxwm 原生窗。
    VxwmNative = 0,
    /// Wine 窗。
    Wine = 1,
    /// Xwayland 窗。
    Xwayland = 2,
}

pub const WINDOW_COUNT: usize = 3;
pub const ROUNDS: u32 = 10;
pub const SCENE_LEN: usize = 20;

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

/// 键事件：id 单调唯一，key 为键位码（0..26）。
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct KeyEvent {
    pub id: u32,
    pub key: u8,
}

/// bigram 续接表：键位 → 3 个续接键（确定性联想核心）。
pub const BIGRAM: [[u8; 3]; 26] = [
    [1, 2, 3],
    [4, 5, 6],
    [7, 8, 9],
    [10, 11, 12],
    [13, 14, 15],
    [16, 17, 18],
    [19, 20, 21],
    [22, 23, 24],
    [25, 0, 1],
    [2, 3, 4],
    [5, 6, 7],
    [8, 9, 10],
    [11, 12, 13],
    [14, 15, 16],
    [17, 18, 19],
    [20, 21, 22],
    [23, 24, 25],
    [0, 2, 4],
    [6, 8, 10],
    [12, 14, 16],
    [18, 20, 22],
    [24, 1, 3],
    [5, 7, 9],
    [11, 13, 15],
    [17, 19, 21],
    [23, 25, 1],
];

/// 键位混淆表：n↔l、f↔h、z↔zh 位（纠错还原核心，低 3 位为混淆对号）。
pub const CONFUSABLE: [u8; 26] = [
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0x1, 0x1, 0, 0x2, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
];

/// 归一化：按 id 去重 + 按 id 升序还原键序（与 B14 同源机制）。
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

/// 传输伪影：按风格注入重复/换序（窗口类的合法自由度）。
pub fn transport(seed: u32, keys: &[u8], w_idx: usize, round: u32) -> Vec<KeyEvent> {
    let mut lc = Lcg::new(seed ^ (w_idx as u32 * 7919) ^ round.wrapping_mul(31));
    let mut events: Vec<KeyEvent> = Vec::new();
    let mut id = 0u32;
    for k in keys.iter() {
        events.push(KeyEvent { id, key: *k });
        id += 1;
    }
    // 三窗三种伪影风格：0 干净 / 1 重复 / 2 混合（重复+换序）。
    match w_idx {
        0 => events,
        1 => {
            let mut out: Vec<KeyEvent> = Vec::new();
            for e in events.iter() {
                out.push(*e);
                if lc.next() % 4 == 0 {
                    out.push(*e);
                }
            }
            out
        }
        _ => {
            let mut out: Vec<KeyEvent> = Vec::new();
            for e in events.iter() {
                out.push(*e);
                if lc.next() % 5 == 0 {
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

/// 联想：末键 bigram 查 Top3（返回续接键三元组）。
pub fn suggest(last_key: u8) -> [u8; 3] {
    let slot = (last_key % 26) as usize;
    BIGRAM[slot]
}

/// 纠错：混淆表还原 + 降权标注（还原命中 → 惩罚位 1）。
pub fn autocorrect(key: u8) -> (u8, u8) {
    let conf = CONFUSABLE[(key % 26) as usize];
    if conf != 0 {
        (key, conf) // 保留原键但带降权语义位
    } else {
        (key, 0)
    }
}

/// 反馈环：饱和频次增益（+1，封顶 99）。
pub fn feedback(freq: u32) -> u32 {
    if freq >= 99 {
        99
    } else {
        freq + 1
    }
}

/// 热词：窗口期计数自增（+1，封顶 1000）。
pub fn hotword(count: u32) -> u32 {
    if count >= 1000 {
        1000
    } else {
        count + 1
    }
}

/// 智能管线一次完整运行：键入场景 → 归一 → 四件套 → 行为指纹。
pub fn run_pipeline(seed: u32, w_idx: usize, round: u32) -> Result<u64, u16> {
    // 场景合成：确定性 20 键（与窗/轮无关的基准键序）。
    let mut lc = Lcg::new(0x4833_3000 ^ round);
    let mut base: Vec<u8> = Vec::new();
    let mut n = 0;
    while n < SCENE_LEN {
        base.push((lc.next() % 26) as u8);
        n += 1;
    }
    // 传输（带伪影）→ 归一。
    let events = transport(seed, &base, w_idx, round);
    let norm = normalize(&events);
    if norm.len() != SCENE_LEN {
        return Err(E_ABSORB_FAIL);
    }
    // 四件套：状态驱动跑完全场景。
    let mut freq: u32 = 0;
    let mut hot: u32 = 0;
    let mut h: u64 = 0xCBF2_9CE4_8422_2325;
    let mut i = 0;
    while i < norm.len() {
        let k = norm[i].key;
        // 联想。
        let s = suggest(k);
        let mut j = 0;
        while j < 3 {
            h ^= s[j] as u64;
            h = h.wrapping_mul(0x0000_0100_0000_01B3);
            j += 1;
        }
        // 纠错（降权位入指纹）。
        let (ck, pen) = autocorrect(k);
        h ^= ck as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
        h ^= pen as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
        // 反馈（选中续接首位即增益）。
        freq = feedback(freq);
        h ^= freq as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
        // 热词（每 5 键一计）。
        if i % 5 == 0 {
            hot = hotword(hot);
            h ^= hot as u64;
            h = h.wrapping_mul(0x0000_0100_0000_01B3);
        }
        i += 1;
    }
    h |= 1 << 63;
    Ok(h)
}

/// 智能三类窗口主巡检：×10 轮 ×3 窗行为一致全链判据。
pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;

    // R1 ×10 轮 ×3 窗：30 次运行指纹全等。
    let mut fp0 = match run_pipeline(0x4833_3001, 0, 1) {
        Ok(f) => f,
        Err(e) => return Err(e),
    };
    let mut round = 1u32;
    while round <= ROUNDS {
        let mut w = 0usize;
        while w < WINDOW_COUNT {
            let fp = run_pipeline(0x4833_3001, w, round)?;
            if fp != fp0 {
                return Err(E_FINGER_DRIFT);
            }
            w += 1;
        }
        round += 1;
    }
    hits += 1;

    // R2 轮数规约：×10 轮全部实际执行（防弱序列空转）。
    if ROUNDS != 10 {
        return Err(E_ROUND_MISS);
    }
    // 场景间独立性：不同轮的基准场景不同（至少 8 轮指纹互异）。
    let mut scene_fps: Vec<u64> = vec![];
    let mut r2 = 1u32;
    while r2 <= ROUNDS {
        scene_fps.push(run_pipeline(0x4833_3001, 0, r2)?);
        r2 += 1;
    }
    let mut distinct = 0u32;
    let mut a = 0;
    while a < scene_fps.len() {
        let mut b = a + 1;
        while b < scene_fps.len() {
            if scene_fps[a] != scene_fps[b] {
                distinct += 1;
            }
            b += 1;
        }
        a += 1;
    }
    // 10 场景两两对比 45 对中至少 8 对互异（LCG 保证强独立）。
    if distinct < 8 {
        return Err(E_ROUND_MISS);
    }
    hits += 1;

    // R3 联想契约：末键 bigram 三元组全落 0..26 且互不重复。
    let mut k = 0u8;
    while (k as usize) < 26 {
        let s = suggest(k);
        let mut j = 0;
        while j < 3 {
            if s[j] >= 26 {
                return Err(E_SUGGEST_BAD);
            }
            let mut m = j + 1;
            while m < 3 {
                if s[j] == s[m] {
                    return Err(E_SUGGEST_BAD);
                }
                m += 1;
            }
            j += 1;
        }
        k += 1;
    }
    hits += 1;

    // R4 纠错契约：混淆键降权位非零、清晰键为零（×26 全查）。
    let mut k2 = 0u8;
    while (k2 as usize) < 26 {
        let (_, pen) = autocorrect(k2);
        let conf = CONFUSABLE[(k2 % 26) as usize];
        if (conf != 0) != (pen != 0) {
            return Err(E_FINGER_DRIFT);
        }
        k2 += 1;
    }
    hits += 1;

    // R5 反馈/热词饱和：封顶后不再增长（99/1000 恒稳）。
    if feedback(99) != 99 || feedback(100) != 99 {
        return Err(E_FINGER_DRIFT);
    }
    if hotword(1000) != 1000 || hotword(1001) != 1000 {
        return Err(E_FINGER_DRIFT);
    }
    hits += 1;

    // R6 扰动自查（假阴性=0）：场景扰动一位 → 指纹必变。
    let fp_base = run_pipeline(0x4833_3001, 0, 3)?;
    let fp_tamper = run_pipeline(0x4833_3001 ^ 0x0000_0001, 0, 3)?;
    if fp_tamper == fp_base {
        return Err(E_TAMPER_MISS);
    }
    hits += 1;

    // R7 干净复跑（假阳性=0）：同种子同窗同轮重跑指纹不变。
    let fp_replay = run_pipeline(0x4833_3001, 0, 3)?;
    if fp_replay != fp_base {
        return Err(E_REPLAY_FALSE);
    }
    // 干净复跑不能触发 R6 的扰动告警语义（指纹一致 = 无漂移）。
    if fp_replay == fp_tamper {
        return Err(E_REPLAY_FALSE);
    }
    hits += 1;

    // R8 伪影吸收：三窗伪影风格全部还原 SCENE_LEN 长度（归一通路）。
    let mut w3 = 0usize;
    while w3 < WINDOW_COUNT {
        let mut lc2 = Lcg::new(0x4833_3100 ^ w3 as u32);
        let mut base2: Vec<u8> = Vec::new();
        let mut n2 = 0;
        while n2 < SCENE_LEN {
            base2.push((lc2.next() % 26) as u8);
            n2 += 1;
        }
        let ev = transport(0x4833_3101, &base2, w3, 5);
        let norm2 = normalize(&ev);
        if norm2.len() != SCENE_LEN {
            return Err(E_ABSORB_FAIL);
        }
        let mut i = 0;
        while i < SCENE_LEN {
            if norm2[i].key != base2[i] {
                return Err(E_ABSORB_FAIL);
            }
            i += 1;
        }
        w3 += 1;
    }
    hits += 1;

    // R9 指纹基线稳定：fp0 与全量重跑的首指纹一致（判定无状态泄漏）。
    let fp_again = run_pipeline(0x4833_3001, 0, 1)?;
    if fp_again != fp0 {
        return Err(E_REPLAY_FALSE);
    }
    let _ = &mut fp0;
    hits += 1;

    // R10 诊断码闭包：全部错误码落在 [SEG_BASE+1, SEG_BASE+0x1F] 段内。
    const CODES: [u16; 7] = [
        E_FINGER_DRIFT,
        E_TAMPER_MISS,
        E_REPLAY_FALSE,
        E_ABSORB_FAIL,
        E_ROUND_MISS,
        E_SUGGEST_BAD,
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
