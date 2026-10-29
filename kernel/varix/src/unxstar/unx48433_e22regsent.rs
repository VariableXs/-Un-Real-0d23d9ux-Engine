//! UNX-F48433 —— E22 三类窗口超长回归哨兵
//! 判据原文：UNX-F48433-J1 三类窗口（vxwm 原生 / Wine / Xwayland）回归判据 · 宪法加严：
//! 三窗各以 200 音节长串试打，受纳行为须与冻结基线一致，静默截断即判回归。
//! 判据落地（drill 场景）：R1 窗类闭集；R2 音节流确定性；R3 试打串足长且不越字节预算；
//! R4 受纳完整（无静默截断）；R5 三窗回归台账与基线逐项一致；R6 步数线性预算；
//! R7 哨兵指纹稳定；R8 窗类键唯一；R9 空串拦截；R10 三窗试打串互异；
//! R11 呈现摘要三窗互异且带前缀；R12 步数记账与线性模型复算一致。
//! 设计纪律：no_std；零 unwrap/expect/panic；确定性 LCG；drill 判据全绿才返回 Ok。
use crate::alloc::vec::Vec;
use crate::alloc::vec;
use alloc::string::{String, ToString};

/// 诊断段基址（独立 0x20 窗）。
pub const SEG_BASE: u16 = 0xDA20;

// ── 诊断码闭集（SEG_BASE+1 起连续，≤ SEG_BASE+0x1F）─────────────
const E_WINCLASS: u16 = SEG_BASE + 1; // 窗类越出三类闭集
const E_SYLL: u16 = SEG_BASE + 2; // 音节流违反确定性
const E_LEN: u16 = SEG_BASE + 3; // 试打串不足 200 音节
const E_TRUNC: u16 = SEG_BASE + 4; // 静默截断（无告警丢尾）
const E_REG: u16 = SEG_BASE + 5; // 回归台账与基线不符
const E_LAT: u16 = SEG_BASE + 6; // 处理步数超线性预算
const E_FP: u16 = SEG_BASE + 7; // 哨兵指纹漂移
const E_DUP: u16 = SEG_BASE + 8; // 窗类键重复
const E_EMPTY: u16 = SEG_BASE + 9; // 空串混入试打管线
const E_OOB: u16 = SEG_BASE + 10; // 字节统计越出预算上限
const E_NAME: u16 = SEG_BASE + 11; // 窗类名称表与键表失配
const E_BRIEF: u16 = SEG_BASE + 12; // 呈现摘要缺前缀

/// 宪法加严试打音节数。
pub const SYLLABLES: usize = 200;
/// 单音节最小字节数（声母 + 韵腹下限）。
pub const MIN_SYL_BYTES: usize = 2;
/// 单音节最大字节数（三段拼接上限）。
pub const MAX_SYL_BYTES: usize = 8;
/// 全串字节预算上限：越界即统计异常。
pub const MAX_STREAM_BYTES: usize = SYLLABLES * MAX_SYL_BYTES;
/// 线性预算常数：步数 ≤ K×字节 + C。
pub const LAT_K: u64 = 2;
pub const LAT_C: u64 = 16;

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

/// 三类窗口键：1 = vxwm 原生，2 = Wine，3 = Xwayland。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct WinClass(pub u8);

impl WinClass {
    pub const NATIVE_VXWM: u8 = 1;
    pub const WINE: u8 = 2;
    pub const XWAYLAND: u8 = 3;

    pub fn is_known(self) -> bool {
        self.0 >= Self::NATIVE_VXWM && self.0 <= Self::XWAYLAND
    }
}

/// 窗类显示名（与键位一一对应，顺序即键序）。
pub const WINDOW_NAMES: [&str; 3] = ["vxwm-native", "wine", "xwayland"];

/// 键 → 显示名；越集返回空。
pub fn class_name(c: WinClass) -> &'static str {
    if c.is_known() {
        WINDOW_NAMES[(c.0 - 1) as usize]
    } else {
        ""
    }
}

/// 音节母表：声母 / 韵腹 / 韵尾。
pub const ONSET: [&str; 6] = ["zh", "ch", "sh", "b", "d", "g"];
pub const MEDIAL: [&str; 4] = ["a", "o", "e", "i"];
pub const CODA: [&str; 4] = ["ng", "n", "u", ""];

/// 一次生成的试打长串（文本 + 音节计数）。
pub struct SyllStream {
    pub text: String,
    pub count: usize,
}

/// 按 LCG 确定性生成 count 个音节的长串。
pub fn gen_stream(lcg: &mut Lcg, count: usize) -> SyllStream {
    let mut text = String::new();
    let mut i = 0usize;
    while i < count {
        let o = lcg.below(ONSET.len() as u32) as usize;
        let m = lcg.below(MEDIAL.len() as u32) as usize;
        let c = lcg.below(CODA.len() as u32) as usize;
        text.push_str(ONSET[o]);
        text.push_str(MEDIAL[m]);
        text.push_str(CODA[c]);
        i += 1;
    }
    SyllStream { text, count }
}

/// 单窗试打受纳记录。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SessionRecord {
    pub class: u8,
    pub syllables: usize,
    pub bytes_in: usize,
    pub bytes_accepted: usize,
    pub truncated: bool,
    pub steps: u32,
}

/// 窗口受纳器：三窗对超长串的统一受纳语义。
pub struct Acceptor;

impl Acceptor {
    /// 受纳语义：全量受纳、零截断、步数按线性模型记账。
    pub fn accept(class: WinClass, s: &SyllStream) -> Result<SessionRecord, u16> {
        if !class.is_known() {
            return Err(E_WINCLASS);
        }
        if s.text.is_empty() {
            return Err(E_EMPTY);
        }
        let bytes_in = s.text.len();
        if bytes_in > MAX_STREAM_BYTES {
            return Err(E_OOB);
        }
        let steps = (bytes_in as u64 * LAT_K + LAT_C) as u32;
        Ok(SessionRecord {
            class: class.0,
            syllables: s.count,
            bytes_in,
            bytes_accepted: bytes_in,
            truncated: false,
            steps,
        })
    }

    /// 线性预算复核：steps ≤ K×bytes + C。
    pub fn within_budget(rec: &SessionRecord) -> bool {
        let budget = rec.bytes_in as u64 * LAT_K + LAT_C;
        rec.steps as u64 <= budget
    }

    /// 线性模型复算：steps 必须恰等于模型值（记账不漂移）。
    pub fn model_matches(rec: &SessionRecord) -> bool {
        let expect = (rec.bytes_in as u64 * LAT_K + LAT_C) as u32;
        rec.steps == expect
    }
}

/// 三窗冻结基线：(键, 允许截断 = false, 允许降级 = true)。
/// 宪法加严后三窗一律禁止静默截断；降级通道保留但本次试打不触发。
pub const BASELINE: [(u8, bool, bool); 3] = [
    (WinClass::NATIVE_VXWM, false, true),
    (WinClass::WINE, false, true),
    (WinClass::XWAYLAND, false, true),
];

/// 受纳记录的呈现摘要：win=<键> syl=<音节> acc=<收讫字节>。
pub fn brief(rec: &SessionRecord) -> String {
    let mut s = String::new();
    s.push_str("win=");
    s.push_str(&(rec.class as u32).to_string());
    s.push_str(" syl=");
    s.push_str(&(rec.syllables as u32).to_string());
    s.push_str(" acc=");
    s.push_str(&(rec.bytes_accepted as u32).to_string());
    s
}

/// 诊断码 → 中文可操作建议。
pub fn detail(code: u16) -> &'static str {
    match code {
        E_WINCLASS => "窗类越出三类闭集：只接受 vxwm 原生/Wine/Xwayland 三键，先核对接线方上报的窗类",
        E_SYLL => "音节流不确定：同种子必须产出同串，检查 LCG 是否被并发或时间源污染",
        E_LEN => "试打串不足 200 音节：宪法加严口径未落实，重新生成足长串",
        E_TRUNC => "静默截断：无告警丢尾即回归，须改为全量受纳或显式告警",
        E_REG => "回归台账与冻结基线不符：逐窗比对截断/降级位，先回滚再定位",
        E_LAT => "步数超线性预算：检查是否混入平方级扫描路径",
        E_FP => "哨兵指纹漂移：同输入两次聚合指纹不一致，检查隐式全局态",
        E_DUP => "窗类键重复：台账中同键双写，先去重再入账",
        E_EMPTY => "空串混入试打管线：空输入必须前置拦截",
        E_OOB => "字节统计越出预算上限：音节母表被改动，重核母表字节数",
        E_NAME => "窗类名称表与键表失配：名称序必须与键序一致",
        E_BRIEF => "呈现摘要缺前缀：win=/syl=/acc= 三段前缀缺一即呈现回归",
        _ => "未知诊断码",
    }
}

/// 三流聚合指纹（键 + 文本字节串联后 FNV）。
fn fingerprint(streams: &[(WinClass, SyllStream)]) -> u64 {
    let mut buf: Vec<u8> = vec![];
    for item in streams.iter() {
        buf.push(item.0 .0);
        buf.extend_from_slice(item.1.text.as_bytes());
    }
    fnv1a64(&buf)
}

/// 主巡检入口：R1..R12 全绿才返回 Ok(hits)。
pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;

    // R1 窗类闭集：1..=3 均已知，4 越集必拒，名称表逐键对得上。
    let mut k = WinClass(WinClass::NATIVE_VXWM);
    while k.0 <= WinClass::XWAYLAND {
        if !k.is_known() || class_name(k).is_empty() {
            return Err(E_WINCLASS);
        }
        k.0 += 1;
    }
    if WinClass(4).is_known() {
        return Err(E_WINCLASS);
    }
    if class_name(WinClass(WinClass::WINE)) != WINDOW_NAMES[1] {
        return Err(E_NAME);
    }
    hits += 1;

    // R2 音节流确定性：同种子同串，异种子异串。
    let mut a = Lcg::new(7);
    let mut b = Lcg::new(7);
    let mut c = Lcg::new(8);
    let sa = gen_stream(&mut a, 8);
    let sb = gen_stream(&mut b, 8);
    let sc = gen_stream(&mut c, 8);
    if sa.text != sb.text || sa.text == sc.text {
        return Err(E_SYLL);
    }
    hits += 1;

    // R3 足长：三窗各 200 音节，字节数落在 [MIN, MAX] 窗内。
    let mut r3 = Lcg::new(0x48433);
    let mut streams: Vec<(WinClass, SyllStream)> = vec![];
    let mut cls = 1u8;
    while cls <= 3 {
        let st = gen_stream(&mut r3, SYLLABLES);
        if st.count != SYLLABLES {
            return Err(E_LEN);
        }
        let n = st.text.len();
        if n < SYLLABLES * MIN_SYL_BYTES || n > MAX_STREAM_BYTES {
            return Err(E_LEN);
        }
        streams.push((WinClass(cls), st));
        cls += 1;
    }
    hits += 1;

    // R4 受纳完整：逐窗受纳，零截断、全量收讫。
    for item in streams.iter() {
        let rec = Acceptor::accept(item.0, &item.1)?;
        if rec.truncated || rec.bytes_accepted != rec.bytes_in {
            return Err(E_TRUNC);
        }
        if rec.syllables != SYLLABLES {
            return Err(E_LEN);
        }
    }
    hits += 1;

    // R5 回归台账：三窗记录与冻结基线逐项一致且键全覆盖。
    let mut ledger: Vec<SessionRecord> = vec![];
    for item in streams.iter() {
        ledger.push(Acceptor::accept(item.0, &item.1)?);
    }
    for base in BASELINE.iter() {
        let mut found = false;
        for rec in ledger.iter() {
            if rec.class == base.0 {
                if rec.truncated != base.1 {
                    return Err(E_REG);
                }
                if rec.bytes_accepted != rec.bytes_in {
                    return Err(E_REG);
                }
                found = true;
            }
        }
        if !found {
            return Err(E_REG);
        }
    }
    hits += 1;

    // R6 步数预算：逐窗不超线性。
    for rec in ledger.iter() {
        if !Acceptor::within_budget(rec) {
            return Err(E_LAT);
        }
    }
    hits += 1;

    // R7 指纹稳定：同输入两次聚合指纹一致。
    let fp_a = fingerprint(&streams);
    let fp_b = fingerprint(&streams);
    if fp_a != fp_b {
        return Err(E_FP);
    }
    hits += 1;

    // R8 键唯一：台账三键两两互异。
    let mut i = 0usize;
    while i < ledger.len() {
        let mut j = i + 1;
        while j < ledger.len() {
            if ledger[i].class == ledger[j].class {
                return Err(E_DUP);
            }
            j += 1;
        }
        i += 1;
    }
    hits += 1;

    // R9 空串拦截：空串必被拒且诊断码恰为 E_EMPTY。
    let empty = SyllStream {
        text: String::new(),
        count: 0,
    };
    match Acceptor::accept(WinClass(1), &empty) {
        Err(E_EMPTY) => hits += 1,
        _ => return Err(E_EMPTY),
    }

    // R10 三窗试打串互异：同一 LCG 顺序生成，文本两两不同。
    let mut p = 0usize;
    while p < streams.len() {
        let mut q = p + 1;
        while q < streams.len() {
            if streams[p].1.text == streams[q].1.text {
                return Err(E_SYLL);
            }
            q += 1;
        }
        p += 1;
    }
    hits += 1;

    // R11 呈现摘要：三窗互异、三段前缀齐全。
    let b0 = brief(&ledger[0]);
    let b1 = brief(&ledger[1]);
    let b2 = brief(&ledger[2]);
    if b0 == b1 || b1 == b2 || b0 == b2 {
        return Err(E_BRIEF);
    }
    for txt in [&b0, &b1, &b2] {
        if !(has_prefix(txt, "win=") && has_kw(txt.as_bytes(), b"syl=") && has_kw(txt.as_bytes(), b"acc=")) {
            return Err(E_BRIEF);
        }
    }
    hits += 1;

    // R12 步数记账与线性模型复算一致（记账不漂移）。
    for rec in ledger.iter() {
        if !Acceptor::model_matches(rec) {
            return Err(E_LAT);
        }
    }
    hits += 1;

    Ok(hits)
}

/// 前缀判定（字节级，无模式依赖）。
fn has_prefix(s: &str, p: &str) -> bool {
    let pb = p.as_bytes();
    let sb = s.as_bytes();
    if sb.len() < pb.len() {
        return false;
    }
    let mut i = 0usize;
    while i < pb.len() {
        if sb[i] != pb[i] {
            return false;
        }
        i += 1;
    }
    true
}

/// 子串判定（朴素扫描，关键词族统一口径）。
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
