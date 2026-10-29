//! UNX-F48484 —— E·长按伪装单击注入（反：时序伪造 ×10 识别正确）
//! 判据原文：UNX-F48484-J1 对 Shift 时序做 10 例系统性伪装（时长裁剪、时戳回拨、
//! 分段拼接、抖动掩蔽、事件乱序、幽灵抬键及其贴边变体），识别器必须
//! 逐例判为伪造且诊断码可分；正常样本（含容忍窗内抖动）零误报。
//! 判据落地（drill 场景）：R1 识别器基准（正常样本放行）；R2 十例伪造全识破；
//! R3 诊断码可分（≥5 种互异）；R4 逐例预期码精确匹配；R5 正常批零误报；
//! R6 伪造统计守恒；R7 指纹稳定；R8 容忍窗边界零误报。
//! 设计纪律：no_std；零 unwrap/expect/panic；确定性 LCG；drill 判据全绿才返回 Ok。
use crate::alloc::vec::Vec;
use crate::alloc::vec;
use alloc::string::{String, ToString};

/// 诊断段基址（独立 0x20 窗）。
pub const SEG_BASE: u16 = 0xE6A0;

// ── 诊断码闭集（SEG_BASE+1 起连续，≤ SEG_BASE+0x1F）─────────────
const E_CROP: u16 = SEG_BASE + 1; // 时长裁剪（声明长按、观测单击侧）
const E_ROLLBACK: u16 = SEG_BASE + 2; // 时戳回拨（up 早于 down）
const E_STITCH: u16 = SEG_BASE + 3; // 分段拼接（短按多段贴缝拼成长按）
const E_JITTER: u16 = SEG_BASE + 4; // 抖动掩蔽（观测偏离声明超容忍窗）
const E_REORDER: u16 = SEG_BASE + 5; // 事件乱序（down 早于前一对 up）
const E_GHOST: u16 = SEG_BASE + 6; // 幽灵抬键（同刻双 up）
const E_MATCH: u16 = SEG_BASE + 7; // 伪造例诊断码与预期不符
const E_FALSEPOS: u16 = SEG_BASE + 8; // 正常样本被误报
const E_STAT: u16 = SEG_BASE + 9; // 伪造统计不守恒
const E_FP: u16 = SEG_BASE + 10; // 指纹漂移

/// 长按判定阈值与抖动容忍窗（毫秒）。
pub const LONG_MS: u32 = 300;
pub const JITTER_MS: u32 = 20;

/// 成对按键事件：down/up 时戳 + 驱动层声明时长。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct KeyPair {
    pub at_down: u32,
    pub at_up: u32,
    pub declared: u32,
}

/// 识别结论：ok = 正常放行；否则 code 为伪造诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Verdict {
    pub ok: bool,
    pub code: u16,
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

/// 时序识别器。
pub struct Inspector;

impl Inspector {
    /// 对级审查：回拨 → 裁剪 → 抖动，逐条自洽。
    pub fn inspect(p: &KeyPair) -> Verdict {
        if p.at_up < p.at_down {
            return Verdict { ok: false, code: E_ROLLBACK };
        }
        let observed = p.at_up - p.at_down;
        let drift = if observed > p.declared {
            observed - p.declared
        } else {
            p.declared - observed
        };
        if drift > JITTER_MS {
            // 声明长按、观测被裁到单击侧 → 裁剪伪装。
            if p.declared >= LONG_MS && observed < LONG_MS {
                return Verdict { ok: false, code: E_CROP };
            }
            return Verdict { ok: false, code: E_JITTER };
        }
        Verdict { ok: true, code: 0 }
    }

    /// 流级审查：乱序 / 幽灵双 up / 拼接贴缝，再逐对审查。
    pub fn inspect_stream(ps: &[KeyPair]) -> Verdict {
        let mut i = 1usize;
        while i < ps.len() {
            // 乱序：本对 down 早于前一对 up。
            if ps[i].at_down < ps[i - 1].at_up {
                return Verdict { ok: false, code: E_REORDER };
            }
            // 幽灵：本对 up 与前一对 up 同刻。
            if ps[i].at_up == ps[i - 1].at_up {
                return Verdict { ok: false, code: E_GHOST };
            }
            // 拼接：两对均观测短按、均声明长按、且贴缝间隔小于容忍窗。
            let gap = ps[i].at_down.saturating_sub(ps[i - 1].at_up);
            let d0 = ps[i - 1].at_up.saturating_sub(ps[i - 1].at_down);
            let d1 = ps[i].at_up.saturating_sub(ps[i].at_down);
            if gap < JITTER_MS && d0 < LONG_MS && d1 < LONG_MS && ps[i - 1].declared >= LONG_MS && ps[i].declared >= LONG_MS {
                return Verdict { ok: false, code: E_STITCH };
            }
            i += 1;
        }
        let mut i = 0usize;
        while i < ps.len() {
            let v = Inspector::inspect(&ps[i]);
            if !v.ok {
                return v;
            }
            i += 1;
        }
        Verdict { ok: true, code: 0 }
    }
}

/// 单例伪装：事件流 + 预期诊断码。
pub struct Forge {
    pub name: String,
    pub pairs: Vec<KeyPair>,
    pub expect: u16,
}

/// 十例系统性时序伪装（长按伪装单击）。
pub fn forge_set() -> Vec<Forge> {
    let mut out: Vec<Forge> = vec![];
    // 1 时长裁剪：声明 500ms 长按被裁到 80ms。
    out.push(Forge {
        name: "crop".to_string(),
        pairs: vec![KeyPair { at_down: 0, at_up: 80, declared: 500 }],
        expect: E_CROP,
    });
    // 2 时戳回拨：up 早于 down。
    out.push(Forge {
        name: "rollback".to_string(),
        pairs: vec![KeyPair { at_down: 400, at_up: 100, declared: 300 }],
        expect: E_ROLLBACK,
    });
    // 3 分段拼接：两次 150ms 短按贴缝 10ms，声明两次 600ms 长按。
    out.push(Forge {
        name: "stitch".to_string(),
        pairs: vec![
            KeyPair { at_down: 0, at_up: 150, declared: 600 },
            KeyPair { at_down: 160, at_up: 320, declared: 600 },
        ],
        expect: E_STITCH,
    });
    // 4 抖动掩蔽：观测 345 对声明 300（超窗 25ms）。
    out.push(Forge {
        name: "jitter".to_string(),
        pairs: vec![KeyPair { at_down: 0, at_up: 345, declared: 300 }],
        expect: E_JITTER,
    });
    // 5 裁剪贴边：声明 420 被裁到 160。
    out.push(Forge {
        name: "crop-edge".to_string(),
        pairs: vec![KeyPair { at_down: 1000, at_up: 1160, declared: 420 }],
        expect: E_CROP,
    });
    // 6 事件乱序：第二对 down 早于第一对 up。
    out.push(Forge {
        name: "reorder".to_string(),
        pairs: vec![
            KeyPair { at_down: 0, at_up: 100, declared: 100 },
            KeyPair { at_down: 50, at_up: 200, declared: 150 },
        ],
        expect: E_REORDER,
    });
    // 7 幽灵抬键：两对 up 同刻。
    out.push(Forge {
        name: "ghost".to_string(),
        pairs: vec![
            KeyPair { at_down: 0, at_up: 100, declared: 100 },
            KeyPair { at_down: 150, at_up: 100, declared: 0 },
        ],
        expect: E_GHOST,
    });
    // 8 单击侧抖动：观测 260 对声明 200（超窗 60ms）。
    out.push(Forge {
        name: "jitter-click".to_string(),
        pairs: vec![KeyPair { at_down: 3000, at_up: 3260, declared: 200 }],
        expect: E_JITTER,
    });
    // 9 深裁剪：声明 350 被裁到 100。
    out.push(Forge {
        name: "crop-deep".to_string(),
        pairs: vec![KeyPair { at_down: 4000, at_up: 4100, declared: 350 }],
        expect: E_CROP,
    });
    // 10 拼接贴边：贴缝 5ms，两段 140ms。
    out.push(Forge {
        name: "stitch-edge".to_string(),
        pairs: vec![
            KeyPair { at_down: 6000, at_up: 6140, declared: 450 },
            KeyPair { at_down: 6145, at_up: 6285, declared: 450 },
        ],
        expect: E_STITCH,
    });
    out
}

/// 伪造集指纹：例名 + 事件对三元组串联。
pub fn forge_fp(fs: &[Forge]) -> u64 {
    let mut buf: Vec<u8> = vec![];
    let mut i = 0usize;
    while i < fs.len() {
        buf.extend_from_slice(fs[i].name.as_bytes());
        buf.push(0x1F);
        let mut j = 0usize;
        while j < fs[i].pairs.len() {
            buf.extend_from_slice(&fs[i].pairs[j].at_down.to_le_bytes());
            buf.extend_from_slice(&fs[i].pairs[j].at_up.to_le_bytes());
            buf.extend_from_slice(&fs[i].pairs[j].declared.to_le_bytes());
            j += 1;
        }
        buf.push(0x1E);
        i += 1;
    }
    fnv1a64(&buf)
}

/// 主巡检入口：R1..R8 全绿才返回 Ok(hits)。
pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;

    // R1 识别器基准：正常单击/长按/窗内抖动/正常流全部放行。
    let normals: [KeyPair; 4] = [
        KeyPair { at_down: 0, at_up: 120, declared: 120 },
        KeyPair { at_down: 500, at_up: 900, declared: 400 },
        KeyPair { at_down: 2000, at_up: 2318, declared: 320 }, // drift 2ms
        KeyPair { at_down: 4000, at_up: 4285, declared: 300 }, // drift 15ms
    ];
    let mut i = 0usize;
    while i < normals.len() {
        let v = Inspector::inspect(&normals[i]);
        if !v.ok {
            return Err(E_FALSEPOS);
        }
        i += 1;
    }
    let ok_stream: Vec<KeyPair> = vec![
        KeyPair { at_down: 0, at_up: 100, declared: 100 },
        KeyPair { at_down: 200, at_up: 550, declared: 350 },
        KeyPair { at_down: 700, at_up: 800, declared: 100 },
    ];
    let v = Inspector::inspect_stream(&ok_stream);
    if !v.ok {
        return Err(E_FALSEPOS);
    }
    hits += 1;

    // R2/R4 十例伪造全识破：流级口径逐例诊断码精确匹配预期。
    let fs = forge_set();
    if fs.len() != 10 {
        return Err(E_STAT);
    }
    let mut i = 0usize;
    while i < fs.len() {
        let v = Inspector::inspect_stream(&fs[i].pairs);
        if v.ok || v.code != fs[i].expect {
            return Err(E_MATCH);
        }
        i += 1;
    }
    hits += 1;

    // R3 诊断码可分：十例预期码至少 5 种互异。
    let mut distinct: Vec<u16> = vec![];
    let mut i = 0usize;
    while i < fs.len() {
        let mut seen = false;
        let mut j = 0usize;
        while j < distinct.len() {
            if distinct[j] == fs[i].expect {
                seen = true;
            }
            j += 1;
        }
        if !seen {
            distinct.push(fs[i].expect);
        }
        i += 1;
    }
    if distinct.len() < 5 {
        return Err(E_MATCH);
    }
    hits += 1;

    // R5 正常批零误报：LCG 采样 24 对正常样本（声明 = 观测 ± 窗内抖动）全放行。
    let mut lcg = Lcg::new(0x48484);
    let mut t = 0u32;
    while t < 24 {
        let base = t * 800 + 100;
        let dur = 100 + lcg.below(500);
        let drift = lcg.below(JITTER_MS + 1);
        // 声明在观测 ± 窗内随机偏移，保证 |obs - declared| ≤ 窗。
        let declared = if lcg.below(2) == 0 {
            dur + drift
        } else {
            dur - drift
        };
        let pair = KeyPair {
            at_down: base,
            at_up: base + dur,
            declared,
        };
        let v = Inspector::inspect(&pair);
        if !v.ok {
            return Err(E_FALSEPOS);
        }
        t += 1;
    }
    hits += 1;

    // R6 伪造统计守恒：十例全判伪，计数恰 10。
    let mut rejected = 0u32;
    let mut i = 0usize;
    while i < fs.len() {
        if !Inspector::inspect_stream(&fs[i].pairs).ok {
            rejected += 1;
        }
        i += 1;
    }
    if rejected != 10 {
        return Err(E_STAT);
    }
    hits += 1;

    // R7 指纹稳定：伪造集指纹两次一致且非零。
    let f1 = forge_fp(&fs);
    let f2 = forge_fp(&fs);
    if f1 != f2 || f1 == 0 {
        return Err(E_FP);
    }
    hits += 1;

    // R8 容忍窗边界：drift 恰等于窗（20ms）放行；超 1ms 即拒（E_JITTER）。
    let edge_ok = KeyPair { at_down: 9000, at_up: 9320, declared: 300 };
    let v = Inspector::inspect(&edge_ok);
    if !v.ok {
        return Err(E_FALSEPOS);
    }
    let edge_bad = KeyPair { at_down: 9500, at_up: 9821, declared: 300 };
    let v = Inspector::inspect(&edge_bad);
    if v.ok || v.code != E_JITTER {
        return Err(E_JITTER);
    }
    hits += 1;

    // 呈现行烟测：伪造就绪摘要含例数与首例名（防构造器空转）。
    let mut line = String::new();
    line.push_str("forges=");
    line.push_str(&(fs.len() as u32).to_string());
    line.push_str(" first=");
    line.push_str(&fs[0].name);
    if !has_kw(line.as_bytes(), b"forges=10") || !has_kw(line.as_bytes(), b"first=crop") {
        return Err(E_FP);
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
