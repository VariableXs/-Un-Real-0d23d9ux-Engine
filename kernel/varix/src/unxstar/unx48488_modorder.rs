//! UNX-F48488 —— E·修饰键乱序注入（反：Shift/Caps 交错 ×10 状态机零错态）
//! 判据原文：UNX-F48488-J1 对 Shift/Caps 事件流做 10 例系统性交错注入
//! （乱序、重复按下、重复抬起、悬空抬起、交叉按抬、自反翻转等），
//! 状态机必须逐例零错态：终态与事件账一致、无非法转移、账目守恒。
//! 判据落地（drill 场景）：R1 状态机基准（正常按抬对零错态）；
//! R2 十例交错注入全零错态；R3 账目守恒（按下数-抬起数 = 按住数）；
//! R4 非法转移拦截（悬空抬起/重复按下必拒）；R5 交错终态逐例匹配预期；
//! R6 指纹稳定；R7 LCG 随机交错流零错态；R8 呈现行齐全。
//! 设计纪律：no_std；零 unwrap/expect/panic；确定性 LCG；drill 判据全绿才返回 Ok。
use crate::alloc::vec::Vec;
use crate::alloc::vec;
use alloc::string::{String, ToString};

/// 诊断段基址（独立 0x20 窗）。
pub const SEG_BASE: u16 = 0xE720;

// ── 诊断码闭集（SEG_BASE+1 起连续，≤ SEG_BASE+0x1F）─────────────
const E_KEY: u16 = SEG_BASE + 1; // 键位越出闭集
const E_HANG: u16 = SEG_BASE + 2; // 悬空抬起（无按下配对）
const E_DUPDOWN: u16 = SEG_BASE + 3; // 重复按下（已按住再按）
const E_STATE: u16 = SEG_BASE + 4; // 终态与事件账不一致（错态）
const E_CONSERV: u16 = SEG_BASE + 5; // 账目不守恒
const E_MATCH: u16 = SEG_BASE + 6; // 交错例终态与预期不符
const E_FP: u16 = SEG_BASE + 7; // 指纹漂移
const E_LINE: u16 = SEG_BASE + 8; // 呈现行缺段

/// 键位闭集：1 = Shift，2 = Caps。
pub const KEY_SHIFT: u8 = 1;
pub const KEY_CAPS: u8 = 2;

/// 事件类别：1 按下 / 2 抬起。
pub const EV_DOWN: u8 = 1;
pub const EV_UP: u8 = 2;

/// 单个修饰键事件。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ModEvent {
    pub key: u8,
    pub ev: u8,
    pub at: u32,
}

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

/// 双键状态：shift_held / caps_on（Caps 为翻转语义）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ModState {
    pub shift_held: bool,
    pub caps_on: bool,
    pub down_shift: u32,
    pub up_shift: u32,
    pub down_caps: u32,
    pub up_caps: u32,
}

impl ModState {
    pub fn new() -> ModState {
        ModState {
            shift_held: false,
            caps_on: false,
            down_shift: 0,
            up_shift: 0,
            down_caps: 0,
            up_caps: 0,
        }
    }

    /// 灌入一笔事件：Shift 按住语义（重复按下/悬空抬起必拒），
    /// Caps 翻转语义（按下即翻转，抬起只记账）。
    pub fn apply(&mut self, e: ModEvent) -> Result<(), u16> {
        if e.key != KEY_SHIFT && e.key != KEY_CAPS {
            return Err(E_KEY);
        }
        if e.ev != EV_DOWN && e.ev != EV_UP {
            return Err(E_KEY);
        }
        if e.key == KEY_SHIFT {
            if e.ev == EV_DOWN {
                if self.shift_held {
                    return Err(E_DUPDOWN);
                }
                self.shift_held = true;
                self.down_shift += 1;
            } else {
                if !self.shift_held {
                    return Err(E_HANG);
                }
                self.shift_held = false;
                self.up_shift += 1;
            }
        } else {
            if e.ev == EV_DOWN {
                self.caps_on = !self.caps_on;
                self.down_caps += 1;
            } else {
                self.up_caps += 1;
            }
        }
        Ok(())
    }

    /// 账目守恒：Shift 按下数 == 抬起数 + 按住数（0/1）。
    pub fn conserved(&self) -> bool {
        self.down_shift == self.up_shift + (self.shift_held as u32)
    }

    /// 终态一致：Shift 账偶数笔 ⇒ 未按住；Caps 账与 caps_on 奇偶一致。
    pub fn state_consistent(&self) -> bool {
        if self.shift_held && self.down_shift != self.up_shift + 1 {
            return false;
        }
        if (self.down_caps % 2 == 1) != self.caps_on {
            return false;
        }
        true
    }
}

/// 流级应用：逐笔传播首个错误。
pub fn apply_stream(st: &mut ModState, es: &[ModEvent]) -> Result<(), u16> {
    let mut i = 0usize;
    while i < es.len() {
        st.apply(es[i])?;
        i += 1;
    }
    Ok(())
}

/// 单例交错注入：事件流 + 预期终态（shift_held, caps_on）+ 是否允许出账错误。
pub struct Interleave {
    pub name: &'static str,
    pub events: Vec<ModEvent>,
    pub expect_shift: bool,
    pub expect_caps: bool,
    pub expect_err: Option<u16>,
}

/// 十例系统性 Shift/Caps 交错注入。
pub fn interleave_set() -> Vec<Interleave> {
    let mut out: Vec<Interleave> = vec![];
    // 1 顺序正常：Shift 按抬、Caps 按抬。
    out.push(Interleave {
        name: "plain",
        events: vec![
            ModEvent { key: KEY_SHIFT, ev: EV_DOWN, at: 0 },
            ModEvent { key: KEY_SHIFT, ev: EV_UP, at: 10 },
            ModEvent { key: KEY_CAPS, ev: EV_DOWN, at: 20 },
            ModEvent { key: KEY_CAPS, ev: EV_UP, at: 30 },
        ],
        expect_shift: false,
        expect_caps: true,
        expect_err: None,
    });
    // 2 交错按抬：Shift 按住期间 Caps 按抬。
    out.push(Interleave {
        name: "cross",
        events: vec![
            ModEvent { key: KEY_SHIFT, ev: EV_DOWN, at: 0 },
            ModEvent { key: KEY_CAPS, ev: EV_DOWN, at: 10 },
            ModEvent { key: KEY_CAPS, ev: EV_UP, at: 20 },
            ModEvent { key: KEY_SHIFT, ev: EV_UP, at: 30 },
        ],
        expect_shift: false,
        expect_caps: true,
        expect_err: None,
    });
    // 3 双按双抬：Shift/Caps 同时按住后同时抬起。
    out.push(Interleave {
        name: "double",
        events: vec![
            ModEvent { key: KEY_SHIFT, ev: EV_DOWN, at: 0 },
            ModEvent { key: KEY_CAPS, ev: EV_DOWN, at: 5 },
            ModEvent { key: KEY_SHIFT, ev: EV_UP, at: 50 },
            ModEvent { key: KEY_CAPS, ev: EV_UP, at: 55 },
        ],
        expect_shift: false,
        expect_caps: true,
        expect_err: None,
    });
    // 4 Caps 自反翻转：按三次 → caps_on 回 true（奇数翻）。
    out.push(Interleave {
        name: "caps-odd",
        events: vec![
            ModEvent { key: KEY_CAPS, ev: EV_DOWN, at: 0 },
            ModEvent { key: KEY_CAPS, ev: EV_DOWN, at: 10 },
            ModEvent { key: KEY_CAPS, ev: EV_DOWN, at: 20 },
        ],
        expect_shift: false,
        expect_caps: true,
        expect_err: None,
    });
    // 5 悬空抬起：未按住 Shift 直接抬起 → E_HANG。
    out.push(Interleave {
        name: "hang",
        events: vec![ModEvent { key: KEY_SHIFT, ev: EV_UP, at: 0 }],
        expect_shift: false,
        expect_caps: false,
        expect_err: Some(E_HANG),
    });
    // 6 重复按下：Shift 已按住再按 → E_DUPDOWN。
    out.push(Interleave {
        name: "dupdown",
        events: vec![
            ModEvent { key: KEY_SHIFT, ev: EV_DOWN, at: 0 },
            ModEvent { key: KEY_SHIFT, ev: EV_DOWN, at: 10 },
        ],
        expect_shift: true,
        expect_caps: false,
        expect_err: Some(E_DUPDOWN),
    });
    // 7 交错后悬空：Shift 按住 → Caps 抬起（Caps 抬起不悬空，只记账）→ 正常。
    out.push(Interleave {
        name: "caps-up-only",
        events: vec![
            ModEvent { key: KEY_SHIFT, ev: EV_DOWN, at: 0 },
            ModEvent { key: KEY_CAPS, ev: EV_UP, at: 10 },
            ModEvent { key: KEY_SHIFT, ev: EV_UP, at: 20 },
        ],
        expect_shift: false,
        expect_caps: false,
        expect_err: None,
    });
    // 8 长交错链：Shift 按住期间 Caps 按五次（奇数翻）。
    out.push(Interleave {
        name: "long-cross",
        events: vec![
            ModEvent { key: KEY_SHIFT, ev: EV_DOWN, at: 0 },
            ModEvent { key: KEY_CAPS, ev: EV_DOWN, at: 10 },
            ModEvent { key: KEY_CAPS, ev: EV_DOWN, at: 20 },
            ModEvent { key: KEY_CAPS, ev: EV_DOWN, at: 30 },
            ModEvent { key: KEY_CAPS, ev: EV_DOWN, at: 40 },
            ModEvent { key: KEY_CAPS, ev: EV_DOWN, at: 50 },
            ModEvent { key: KEY_SHIFT, ev: EV_UP, at: 60 },
        ],
        expect_shift: false,
        expect_caps: true,
        expect_err: None,
    });
    // 9 越集键位：键 3 按下 → E_KEY。
    out.push(Interleave {
        name: "badkey",
        events: vec![ModEvent { key: 3, ev: EV_DOWN, at: 0 }],
        expect_shift: false,
        expect_caps: false,
        expect_err: Some(E_KEY),
    });
    // 10 越集事件：Caps 事件类别 5 → E_KEY。
    out.push(Interleave {
        name: "badev",
        events: vec![ModEvent { key: KEY_CAPS, ev: 5, at: 0 }],
        expect_shift: false,
        expect_caps: false,
        expect_err: Some(E_KEY),
    });
    out
}

/// 状态指纹。
pub fn state_fp(st: &ModState) -> u64 {
    let mut buf: Vec<u8> = vec![];
    buf.push(st.shift_held as u8);
    buf.push(st.caps_on as u8);
    buf.extend_from_slice(&st.down_shift.to_le_bytes());
    buf.extend_from_slice(&st.up_shift.to_le_bytes());
    buf.extend_from_slice(&st.down_caps.to_le_bytes());
    buf.extend_from_slice(&st.up_caps.to_le_bytes());
    fnv1a64(&buf)
}

/// 状态行（呈现面）：shift=<0/1> caps=<0/1> down_s=<n> up_s=<n>。
pub fn state_line(st: &ModState) -> String {
    let mut s = String::new();
    s.push_str("shift=");
    s.push_str(&(st.shift_held as u32).to_string());
    s.push_str(" caps=");
    s.push_str(&(st.caps_on as u32).to_string());
    s.push_str(" down_s=");
    s.push_str(&st.down_shift.to_string());
    s.push_str(" up_s=");
    s.push_str(&st.up_shift.to_string());
    s
}

/// 主巡检入口：R1..R8 全绿才返回 Ok(hits)。
pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;

    // R1 状态机基准：正常按抬对流终态零错态且账目守恒。
    let base: Vec<ModEvent> = vec![
        ModEvent { key: KEY_SHIFT, ev: EV_DOWN, at: 0 },
        ModEvent { key: KEY_CAPS, ev: EV_DOWN, at: 10 },
        ModEvent { key: KEY_SHIFT, ev: EV_UP, at: 20 },
        ModEvent { key: KEY_CAPS, ev: EV_UP, at: 30 },
        ModEvent { key: KEY_CAPS, ev: EV_DOWN, at: 40 },
        ModEvent { key: KEY_CAPS, ev: EV_UP, at: 50 },
    ];
    let mut st = ModState::new();
    apply_stream(&mut st, &base)?;
    // Caps 两按两抬：翻转偶数次 → 终态 false；Shift 按抬配对 → 未按住。
    if st.shift_held || st.caps_on {
        return Err(E_STATE);
    }
    if !st.conserved() || !st.state_consistent() {
        return Err(E_CONSERV);
    }
    hits += 1;

    // R2/R3/R5 十例交错注入：终态逐例匹配预期或错误码精确匹配。
    let set = interleave_set();
    if set.len() != 10 {
        return Err(E_MATCH);
    }
    let mut i = 0usize;
    while i < set.len() {
        let mut st = ModState::new();
        let res = apply_stream(&mut st, &set[i].events);
        match set[i].expect_err {
            Some(code) => {
                if res.err() != Some(code) {
                    return Err(E_MATCH);
                }
            }
            None => {
                if res.is_err() {
                    return Err(E_MATCH);
                }
                if st.shift_held != set[i].expect_shift || st.caps_on != set[i].expect_caps {
                    return Err(E_MATCH);
                }
                if !st.conserved() || !st.state_consistent() {
                    return Err(E_STATE);
                }
            }
        }
        i += 1;
    }
    hits += 1;

    // R3' 账目守恒独立复核：对全部无错例（partially applied 除外）做守恒断言。
    let mut i = 0usize;
    while i < set.len() {
        if set[i].expect_err.is_none() {
            let mut st = ModState::new();
            apply_stream(&mut st, &set[i].events)?;
            if !st.conserved() {
                return Err(E_CONSERV);
            }
        }
        i += 1;
    }
    hits += 1;

    // R4 非法转移拦截：悬空抬起与重复按下必拒（独立于十例集再验一遍）。
    let mut st = ModState::new();
    match st.apply(ModEvent { key: KEY_SHIFT, ev: EV_UP, at: 0 }) {
        Err(E_HANG) => {}
        _ => return Err(E_STATE),
    }
    st.apply(ModEvent { key: KEY_SHIFT, ev: EV_DOWN, at: 10 })?;
    match st.apply(ModEvent { key: KEY_SHIFT, ev: EV_DOWN, at: 20 }) {
        Err(E_DUPDOWN) => {}
        _ => return Err(E_STATE),
    }
    hits += 1;

    // R5' 终态逐例匹配已在 R2 内；此处复核长交错链例（例 8）单独重放。
    let mut st = ModState::new();
    apply_stream(&mut st, &set[7].events)?;
    if st.shift_held || !st.caps_on {
        return Err(E_MATCH);
    }
    if st.down_caps != 5 {
        return Err(E_MATCH);
    }
    hits += 1;

    // R6 指纹稳定：基准终态两次指纹一致且非零。
    let f1 = state_fp(&st);
    let f2 = state_fp(&st);
    if f1 != f2 || f1 == 0 {
        return Err(E_FP);
    }
    hits += 1;

    // R7 LCG 随机交错流零错态：128 笔合法事件（Shift 按抬配对 + Caps 乱按）
    // 终态账目守恒、状态一致。
    let mut lcg = Lcg::new(0x48488);
    let mut st = ModState::new();
    let mut n = 0u32;
    while n < 128 {
        let key = if lcg.below(2) == 0 { KEY_SHIFT } else { KEY_CAPS };
        let ev = if key == KEY_SHIFT {
            // Shift 只在合法方向发事件：按住时只能抬，未按时只能按。
            if st.shift_held {
                EV_UP
            } else {
                EV_DOWN
            }
        } else if lcg.below(4) == 0 {
            EV_UP
        } else {
            EV_DOWN
        };
        st.apply(ModEvent { key, ev, at: n * 7 })?;
        if !st.state_consistent() {
            return Err(E_STATE);
        }
        n += 1;
    }
    if !st.conserved() {
        return Err(E_CONSERV);
    }
    hits += 1;

    // R8 呈现行齐全：shift=/caps=/down_s=/up_s= 四段在位。
    let line = state_line(&st);
    if !has_kw(line.as_bytes(), b"shift=")
        || !has_kw(line.as_bytes(), b"caps=")
        || !has_kw(line.as_bytes(), b"down_s=")
        || !has_kw(line.as_bytes(), b"up_s=")
    {
        return Err(E_LINE);
    }
    hits += 1;

    Ok(hits)
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
