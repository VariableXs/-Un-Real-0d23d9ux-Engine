//! UNX-F12770 域经十条撰写（域经验沉淀档） · 0x82A80段
//!
//! 判据原文（派工卡 12770 逐字锚定）：
//!   UNX-F12771-J1 四类资产 SHA-256 冻结在册，版本漂移演练 10/10 检出。
//!   ① 纯功能代码 ≥330 行；
//!   ② 完成时 API/软件写回：状态=已完成、结果摘要非空。
//!
//! 设计要点：
//! 1. 域经十条登记表（恰 10 行冻结闭集）：经号 1..=10 / 经验句代号 /
//!    经验句正文 / 适用面闭集位图 / 证据行引用双槽——行序冻结，越序、
//!    重复、超容、空样本一律显性拒，逐条真实语义句，不许灌水；
//! 2. 三列逐条可点验判定器 judge_wisdom(no) -> Result<WisdomRow, u32>：
//!    未登记 → ENOENT；适用面越闭集 → EINVAL；证据行引用悬空 → ESRCH；
//!    判定次序冻结为 未登记 → 适用面 → 证据（先账面、再语义、后引用）；
//! 3. 沉淀状态机 Draft→Reviewed→Sealed 单调只进，demote 一律显性拒
//!    （ERewind）；晋升封存瞬间四类资产（经验句文本/适用面清单/证据行
//!    全表/状态机档）SHA-256 现算落册冻结，冻结后再改写拒绝；
//! 4. 注入器三类（悬空证据/适用面越界/状态倒退）各 10 轮，逐轮以精确
//!    诊断码点验，逃脱即判据面失效；
//! 5. 版本漂移演练 10/10 检出：演练前先验冻结诚实性（四类资产原样重算
//!    必须逐字节等于在册摘要），再逐轮对一类资产翻一字节重算 SHA-256，
//!    与冻结摘要必须不同——不漂移即冻结面失效；
//! 6. no_std + alloc；零浮点、零 IO、零墙钟、零随机（LCG 种子写死 =
//!    SEG_BASE）；取值一律 match/if let/.get()/.get_mut()，生产区无
//!    unwrap/expect/panic/断言宏/裸下标/浮点。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 段基址（VX9 派工公式：0x80000 + 0x40 × (12770 − 12600) = 0x82A80）。
pub const SEG_BASE: u32 = 0x82A80;

// ── 域经沉淀诊断码闭集 ───────────────────────────────────────────
/// 诊断码闭集（段内 0x01..0x0C 偏移，越出闭集的码在查表处回落"未知"）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WisDiag {
    /// 经号未登记（ENOENT）：登记表里查不到这一经号。
    ENoEntry,
    /// 适用面越闭集（EINVAL）：空面或位图越过 10 面闭集。
    EScopeOut,
    /// 证据行引用悬空（ESRCH）：引用下标不在证据行冻结表内。
    EDangling,
    /// 经号越序：登记必须按 1..=10 逐行推进。
    EOrder,
    /// 经号重复：同一经号只许落一行。
    EDup,
    /// 容量耗尽：十行闭集已满，第 11 行拒绝。
    ECapFull,
    /// 非草稿状态写入：评审/封存后登记闸关闭。
    EStateWrite,
    /// 十行未齐禁止评审晋升。
    ENotFull,
    /// 状态机倒退：Draft→Reviewed→Sealed 只进不退。
    ERewind,
    /// 冻结面改写：四类摘要已落册，再冻结一律拒绝。
    EFrozen,
    /// 漂移逃脱：翻字节后摘要未变或未在册，冻结面失效。
    EDrift,
    /// 空样本：经验句代号或正文为空，不构成经验行。
    EEmpty,
}

impl WisDiag {
    /// 诊断码值（SEG_BASE + 0x01..0x0C 闭集）。
    pub fn val(self) -> u32 {
        SEG_BASE
            + match self {
                WisDiag::ENoEntry => 0x01,
                WisDiag::EScopeOut => 0x02,
                WisDiag::EDangling => 0x03,
                WisDiag::EOrder => 0x04,
                WisDiag::EDup => 0x05,
                WisDiag::ECapFull => 0x06,
                WisDiag::EStateWrite => 0x07,
                WisDiag::ENotFull => 0x08,
                WisDiag::ERewind => 0x09,
                WisDiag::EFrozen => 0x0a,
                WisDiag::EDrift => 0x0b,
                WisDiag::EEmpty => 0x0c,
            }
    }

    /// 中文诊断文案（含可操作建议）。
    pub fn detail(self) -> &'static str {
        match self {
            WisDiag::ENoEntry => "经号未登记：先对域经十条闭集 1..=10 确认行号，未入册的经验先走登记闸",
            WisDiag::EScopeOut => "适用面越闭集：位图只许落在 10 面闭集内且非空，先收窄适用面再入册",
            WisDiag::EDangling => "证据行引用悬空：引用下标须落在证据行冻结表内，先补证据行再点验",
            WisDiag::EOrder => "经号越序：按 1..=10 逐行登记，跳行属乱序，回补上一行后再续",
            WisDiag::EDup => "经号重复：每号只落一行，重复登记先查已有行",
            WisDiag::ECapFull => "容量耗尽：十行闭集已满，新经验走下一档沉淀档滚动，不挤占本册",
            WisDiag::EStateWrite => "非草稿状态写入：评审/封存后登记闸关闭，改动走修订档另立新册",
            WisDiag::ENotFull => "十行未齐禁止晋升：先补齐缺行再走评审/封存口",
            WisDiag::ERewind => "状态机倒退：Draft→Reviewed→Sealed 只进不退，倒退即沉淀档可疑",
            WisDiag::EFrozen => "冻结面改写：四类 SHA-256 已落册只许一次，改写先解冻审计再立新册",
            WisDiag::EDrift => "漂移逃脱：翻字节后摘要未检出漂移，SHA-256 冻结面失效须整表复核",
            WisDiag::EEmpty => "空样本：经验句代号或正文为空，先写实语义句再入册，不许灌水",
        }
    }
}

/// 全部诊断码（闭集数组）。
pub const WIS_ALL: &[WisDiag] = &[
    WisDiag::ENoEntry,
    WisDiag::EScopeOut,
    WisDiag::EDangling,
    WisDiag::EOrder,
    WisDiag::EDup,
    WisDiag::ECapFull,
    WisDiag::EStateWrite,
    WisDiag::ENotFull,
    WisDiag::ERewind,
    WisDiag::EFrozen,
    WisDiag::EDrift,
    WisDiag::EEmpty,
];

/// 诊断码查表（闭集返回，未知码回落提示文案）。
pub fn detail(code: u32) -> &'static str {
    for d in WIS_ALL.iter() {
        if d.val() == code {
            return d.detail();
        }
    }
    "未知诊断码：不在 0x82A81..0x82A8C 闭集内"
}

// ── 适用面闭集（10 面位图，位 0..10 有效） ───────────────────────
/// 适用面闭集名词表（位序即面序，冻结不许重排）。
pub const SCOPE_FACETS: &[&str] = &[
    "调度", "内存", "持久", "驱动", "安全", "测试", "工具", "文档", "审计", "回滚",
];

/// 全闭集位图（10 面全 1）。
pub const SCOPE_FULL: u16 = 0x03FF;

/// 适用面越闭集判定：空面与高位越面同归违约（EINVAL 语义面）。
pub fn scope_violated(mask: u16) -> bool {
    if mask == 0 {
        return true;
    }
    if mask & !SCOPE_FULL != 0 {
        return true;
    }
    false
}

/// 适用面中文名摘要（诊断文本用）。
pub fn scope_names(mask: u16) -> String {
    let mut out = String::new();
    for (i, name) in SCOPE_FACETS.iter().enumerate() {
        if mask & (1u16 << i) != 0 {
            if !out.is_empty() {
                out.push('|');
            }
            out.push_str(name);
        }
    }
    if out.is_empty() {
        out.push_str("空面");
    }
    out
}

// ── 证据行冻结表（12 行，闭集，常量来源：沉淀规程台账） ──────────
/// 证据行冻结表：每行一条可追溯台账条目，引用下标 0..12。
pub const EVIDENCE_LINES: &[&str] = &[
    "EV-01 中断下半部派发例程与调度记录",
    "EV-02 数据结构先行评审决议",
    "EV-03 中断延迟预算实测台账",
    "EV-04 错误路径诊断文案清单（detail 建议全覆盖）",
    "EV-05 冻结表容量与扩容决议记录",
    "EV-06 容量记账回归测试记录",
    "EV-07 固定种子 LCG 规约条目（零随机红线）",
    "EV-08 边界测例清单（恰满/超一/欠一）",
    "EV-09 状态机单调性决议（倒退显性拒）",
    "EV-10 双跑一致性核验记录（指纹比对）",
    "EV-11 注入演练台账（三类注入全抓记录）",
    "EV-12 经验沉淀规程（证据行引用强制条款）",
];

/// 证据行引用悬空判定：下标不在冻结表内即悬空（ESRCH 语义面）。
pub fn ev_dangling(ref_no: u32) -> bool {
    EVIDENCE_LINES.get(ref_no as usize).is_none()
}

// ── 域经行数据模型 ───────────────────────────────────────────────
/// 登记表容量：十行齐即满（闭集，不许扩）。
pub const WISDOM_CAP: usize = 10;

/// 域经单行：经号 / 经验句代号 / 经验句正文 / 适用面位图 / 证据行引用双槽。
/// 三列可点验列 = 经验句（正文）· 适用面（位图）· 证据行（双槽引用）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WisdomRow {
    /// 经号（1..=10 闭集）。
    pub no: u32,
    /// 经验句代号（JX-01..JX-10，冻结）。
    pub code: &'static str,
    /// 经验句正文（一句话真实语义经验）。
    pub sentence: &'static str,
    /// 适用面闭集位图（SCOPE_FULL 子集且非空）。
    pub scope: u16,
    /// 证据行引用甲（证据行冻结表下标）。
    pub ev_a: u32,
    /// 证据行引用乙（证据行冻结表下标）。
    pub ev_b: u32,
}

/// 行摘要（诊断文本用）。
pub fn wisdom_brief(row: &WisdomRow) -> String {
    format!(
        "经{}({}) 面[{}] 证{},{}",
        row.no,
        row.code,
        scope_names(row.scope),
        row.ev_a,
        row.ev_b
    )
}

/// 域经十条冻结闭集（判据主体：每条一句话经验 + 适用面 + 证据引用，
/// 行序即登记序，冻结不许重排）。
pub const WISDOM_TEN: &[WisdomRow] = &[
    WisdomRow {
        no: 1,
        code: "JX-01",
        sentence: "中断上半部只做登记与应答，重活必须下沉到下半部线程，否则延迟预算必被击穿。",
        scope: 0x0009,
        ev_a: 0,
        ev_b: 2,
    },
    WisdomRow {
        no: 2,
        code: "JX-02",
        sentence: "先定数据结构再定控制流，结构错了控制流救不回来，返工成本随行数指数涨。",
        scope: 0x0042,
        ev_a: 1,
        ev_b: 11,
    },
    WisdomRow {
        no: 3,
        code: "JX-03",
        sentence: "每条错误路径必须携带可操作的中文建议，裸错误码等于诊断盲区。",
        scope: 0x0110,
        ev_a: 3,
        ev_b: 11,
    },
    WisdomRow {
        no: 4,
        code: "JX-04",
        sentence: "冻结表先定容量再定内容，扩容纪律是不够即扩并显性记账，不许静默吞。",
        scope: 0x00C0,
        ev_a: 4,
        ev_b: 5,
    },
    WisdomRow {
        no: 5,
        code: "JX-05",
        sentence: "确定性优先：同输入必同输出，一切随机源换固定种子线性同余，双跑才可复算。",
        scope: 0x0120,
        ev_a: 6,
        ev_b: 9,
    },
    WisdomRow {
        no: 6,
        code: "JX-06",
        sentence: "边界值单独成测例：恰满、超一、欠一各钉一条，顺路测例盖不住边界。",
        scope: 0x0020,
        ev_a: 7,
        ev_b: 11,
    },
    WisdomRow {
        no: 7,
        code: "JX-07",
        sentence: "状态机只进不退，倒退一律显性拒绝并留诊断码，静默回卷等于账本可疑。",
        scope: 0x0300,
        ev_a: 8,
        ev_b: 11,
    },
    WisdomRow {
        no: 8,
        code: "JX-08",
        sentence: "双跑一致是指纹底线：两次全流程逐字节相同才叫可复算，否则先查隐式状态。",
        scope: 0x0060,
        ev_a: 9,
        ev_b: 10,
    },
    WisdomRow {
        no: 9,
        code: "JX-09",
        sentence: "注入必抓：人为构造的违约若逃过判定器，判据面整体失效，必须整表复核。",
        scope: 0x0030,
        ev_a: 10,
        ev_b: 11,
    },
    WisdomRow {
        no: 10,
        code: "JX-10",
        sentence: "经验句入册必须带证据行引用，无证据沉淀的经验是传闻，不许进冻结闭集。",
        scope: 0x0180,
        ev_a: 11,
        ev_b: 3,
    },
];

// ── 沉淀状态机：Draft→Reviewed→Sealed 单调 ──────────────────────
/// 沉淀状态机（单调只进，倒退显性拒）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WisState {
    /// 草稿：允许逐行登记。
    Draft,
    /// 评审：十行齐，等待封存。
    Reviewed,
    /// 封存：四类摘要冻结在册，只读。
    Sealed,
}

impl WisState {
    /// 状态秩（只许前进）。
    pub fn rank(self) -> u8 {
        match self {
            WisState::Draft => 0,
            WisState::Reviewed => 1,
            WisState::Sealed => 2,
        }
    }

    /// 状态中文名。
    pub fn name(self) -> &'static str {
        match self {
            WisState::Draft => "草稿",
            WisState::Reviewed => "评审",
            WisState::Sealed => "封存",
        }
    }
}

// ── SHA-256（FIPS 180-4，纯整数实现，零依赖） ────────────────────
/// SHA-256 轮常数 K（FIPS 180-4 §4.2.3，64 词闭集）。
const SHA_K: [u32; 64] = [
    0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4,
    0xab1c5ed5, 0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe,
    0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f,
    0x4a7484aa, 0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7,
    0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc,
    0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b,
    0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070, 0x19a4c116,
    0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
    0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7,
    0xc67178f2,
];

/// SHA-256 初始链接变量（FIPS 180-4 §5.3.3）。
const SHA_H0: [u32; 8] = [
    0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab,
    0x5be0cd19,
];

/// 消息字取值（防裸下标：越界回落零）。
fn w_at(w: &[u32; 64], i: usize) -> u32 {
    w.get(i).copied().unwrap_or(0)
}

/// 大端写入一个 32 位字到摘要缓冲（get_mut 防裸下标）。
fn put32(buf: &mut [u8], word_idx: usize, v: u32) {
    if let Some(s) = buf.get_mut(word_idx * 4) {
        *s = (v >> 24) as u8;
    }
    if let Some(s) = buf.get_mut(word_idx * 4 + 1) {
        *s = (v >> 16) as u8;
    }
    if let Some(s) = buf.get_mut(word_idx * 4 + 2) {
        *s = (v >> 8) as u8;
    }
    if let Some(s) = buf.get_mut(word_idx * 4 + 3) {
        *s = v as u8;
    }
}

/// SHA-256 摘要（输入字节切片 → 32 字节摘要）。
pub fn sha256(data: &[u8]) -> [u8; 32] {
    let mut st: [u32; 8] = SHA_H0;
    // 填充：原文 + 0x80 + 零填充至 56 mod 64 + 64 位大端位长。
    let bit_len = (data.len() as u64).wrapping_mul(8);
    let mut padded: Vec<u8> = Vec::new();
    for b in data.iter() {
        padded.push(*b);
    }
    padded.push(0x80u8);
    while padded.len() % 64 != 56 {
        padded.push(0u8);
    }
    let mut k = 0u32;
    while k < 8 {
        padded.push(((bit_len >> (56 - k * 8)) & 0xFF) as u8);
        k += 1;
    }
    // 逐 64 字节块压缩。
    for chunk in padded.chunks(64) {
        let mut w = [0u32; 64];
        let mut t = 0usize;
        while t < 16 {
            let b0 = chunk.get(t * 4).copied().unwrap_or(0) as u32;
            let b1 = chunk.get(t * 4 + 1).copied().unwrap_or(0) as u32;
            let b2 = chunk.get(t * 4 + 2).copied().unwrap_or(0) as u32;
            let b3 = chunk.get(t * 4 + 3).copied().unwrap_or(0) as u32;
            if let Some(slot) = w.get_mut(t) {
                *slot = (b0 << 24) | (b1 << 16) | (b2 << 8) | b3;
            }
            t += 1;
        }
        let mut t = 16usize;
        while t < 64 {
            let w15 = w_at(&w, t - 15);
            let w2 = w_at(&w, t - 2);
            let s0 = w15.rotate_right(7) ^ w15.rotate_right(18) ^ (w15 >> 3);
            let s1 = w2.rotate_right(17) ^ w2.rotate_right(19) ^ (w2 >> 10);
            let w16 = w_at(&w, t - 16);
            let w7 = w_at(&w, t - 7);
            if let Some(slot) = w.get_mut(t) {
                *slot = w16.wrapping_add(s0).wrapping_add(w7).wrapping_add(s1);
            }
            t += 1;
        }
        let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut hh] = st;
        let mut t = 0usize;
        while t < 64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ ((!e) & g);
            let temp1 = hh
                .wrapping_add(s1)
                .wrapping_add(ch)
                .wrapping_add(SHA_K.get(t).copied().unwrap_or(0))
                .wrapping_add(w_at(&w, t));
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let temp2 = s0.wrapping_add(maj);
            hh = g;
            g = f;
            f = e;
            e = d.wrapping_add(temp1);
            d = c;
            c = b;
            b = a;
            a = temp1.wrapping_add(temp2);
            t += 1;
        }
        let [q0, q1, q2, q3, q4, q5, q6, q7] = st;
        st = [
            q0.wrapping_add(a),
            q1.wrapping_add(b),
            q2.wrapping_add(c),
            q3.wrapping_add(d),
            q4.wrapping_add(e),
            q5.wrapping_add(f),
            q6.wrapping_add(g),
            q7.wrapping_add(hh),
        ];
    }
    let mut out = [0u8; 32];
    let mut i = 0usize;
    while i < 8 {
        let v = st.get(i).copied().unwrap_or(0);
        put32(&mut out, i, v);
        i += 1;
    }
    out
}

/// 32 字节摘要的小写十六进制文本（诊断与已知向量比对用）。
pub fn hex32(d: &[u8; 32]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::new();
    for b in d.iter() {
        let hi = (*b >> 4) as usize;
        let lo = (*b & 0x0F) as usize;
        if let Some(c) = HEX.get(hi) {
            out.push(*c as char);
        }
        if let Some(c) = HEX.get(lo) {
            out.push(*c as char);
        }
    }
    out
}

// ── 确定性 LCG（种子 = SEG_BASE，零随机红线合规） ────────────────
/// 线性同余发生器：种子写死，序列确定，双跑一致。
pub struct Lcg {
    state: u64,
}

impl Lcg {
    /// 以固定种子构造。
    pub fn new(seed: u64) -> Lcg {
        Lcg { state: seed }
    }

    /// 下一伪随机数（确定性，Knuth MMIX 31 位推荐常数）。
    pub fn next_u32(&mut self) -> u32 {
        self.state = self
            .state
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        (self.state >> 33) as u32
    }

    /// 0..bound 内取值（bound 为零返回零）。
    pub fn below(&mut self, bound: u32) -> u32 {
        if bound == 0 {
            return 0;
        }
        self.next_u32() % bound
    }
}

// ── 域经登记表（沉淀档账本） ─────────────────────────────────────
/// 域经十条登记表：草稿登记 → 评审 → 封存冻结四类资产摘要。
#[derive(Clone, Debug)]
pub struct WisdomRegistry {
    /// 已入账行（按登记序，经号 1..=10）。
    rows: Vec<WisdomRow>,
    /// 沉淀状态。
    state: WisState,
    /// 四类资产 SHA-256 冻结摘要（封存瞬间落册）。
    frozens: [[u8; 32]; 4],
    /// 冻结在册标记。
    frozen_ready: bool,
}

impl WisdomRegistry {
    /// 开一本新沉淀档（草稿态、空表、未冻结）。
    pub fn new() -> WisdomRegistry {
        WisdomRegistry {
            rows: Vec::new(),
            state: WisState::Draft,
            frozens: [[0u8; 32]; 4],
            frozen_ready: false,
        }
    }

    /// 注入面构造：绕过登记闸构造违约样本册（仅供注入演练与判定器点验，
    /// 正常流一律走 register 闸）。
    pub fn from_raw(rows: Vec<WisdomRow>) -> WisdomRegistry {
        WisdomRegistry {
            rows,
            state: WisState::Draft,
            frozens: [[0u8; 32]; 4],
            frozen_ready: false,
        }
    }

    /// 当前沉淀状态。
    pub fn state(&self) -> WisState {
        self.state
    }

    /// 已登记行数。
    pub fn row_count(&self) -> usize {
        self.rows.len()
    }

    /// 是否已冻结在册。
    pub fn is_frozen(&self) -> bool {
        self.frozen_ready
    }

    /// 取第 idx 行（越界 None）。
    pub fn row_at(&self, idx: usize) -> Option<WisdomRow> {
        self.rows.get(idx).copied()
    }

    /// 按经号查行。
    pub fn row_of(&self, no: u32) -> Option<WisdomRow> {
        for r in self.rows.iter() {
            if r.no == no {
                return Some(*r);
            }
        }
        None
    }

    /// 取第 class 类资产冻结摘要（未冻结返回 None）。
    pub fn frozen_of(&self, class: usize) -> Option<[u8; 32]> {
        if !self.frozen_ready {
            return None;
        }
        self.frozens.get(class).copied()
    }

    /// 登记一行域经（状态→容量→越序/重复→空样本→适用面→证据 六道闸）。
    pub fn register(&mut self, row: WisdomRow) -> Result<(), u32> {
        // 状态闸：只有草稿态允许登记。
        if self.state != WisState::Draft {
            return Err(WisDiag::EStateWrite.val());
        }
        // 容量闸：十行闭集已满。
        if self.rows.len() >= WISDOM_CAP {
            return Err(WisDiag::ECapFull.val());
        }
        // 越序/重复闸：按 1..=10 逐行推进。
        let expect = self.rows.len() as u32 + 1;
        if row.no != expect {
            if self.row_of(row.no).is_some() {
                return Err(WisDiag::EDup.val());
            }
            return Err(WisDiag::EOrder.val());
        }
        // 空样本闸：代号与正文必须非空。
        if row.code.is_empty() || row.sentence.is_empty() {
            return Err(WisDiag::EEmpty.val());
        }
        // 适用面闸：位图必须落在 10 面闭集内且非空。
        if scope_violated(row.scope) {
            return Err(WisDiag::EScopeOut.val());
        }
        // 证据闸：双槽引用必须落在证据行冻结表内。
        if ev_dangling(row.ev_a) || ev_dangling(row.ev_b) {
            return Err(WisDiag::EDangling.val());
        }
        self.rows.push(row);
        Ok(())
    }

    /// 晋升：草稿→评审（十行齐）→封存（四类摘要落册冻结）。
    pub fn promote(&mut self) -> Result<(), u32> {
        match self.state {
            WisState::Draft => {
                if self.rows.len() != WISDOM_CAP {
                    return Err(WisDiag::ENotFull.val());
                }
                self.state = WisState::Reviewed;
                Ok(())
            }
            WisState::Reviewed => {
                // 封存瞬间：先落封存态（状态机档资产含状态秩，摘要必须
                // 按终态计算），再四类资产 SHA-256 现算落册（只许一次）。
                self.state = WisState::Sealed;
                self.frozens = self.compute_frozens();
                self.frozen_ready = true;
                Ok(())
            }
            WisState::Sealed => Err(WisDiag::ERewind.val()),
        }
    }

    /// 倒退：一律显性拒（沉淀档只进不退）。
    pub fn demote(&mut self) -> Result<(), u32> {
        Err(WisDiag::ERewind.val())
    }

    /// 冻结面改写：已冻结后再落摘要一律拒绝（冻结在册只许一次）。
    pub fn refreeze(&mut self) -> Result<(), u32> {
        if !self.frozen_ready {
            return Err(WisDiag::ENotFull.val());
        }
        Err(WisDiag::EFrozen.val())
    }

    /// 三列逐条可点验判定器：未登记 → ENOENT；适用面越闭集 → EINVAL；
    /// 证据行引用悬空 → ESRCH。判定次序冻结：先账面、再语义、后引用。
    pub fn judge_wisdom(&self, no: u32) -> Result<WisdomRow, u32> {
        // 第一闸（账面）：未登记。
        let row = match self.row_of(no) {
            Some(r) => r,
            None => return Err(WisDiag::ENoEntry.val()),
        };
        // 第二闸（语义）：适用面越闭集。
        if scope_violated(row.scope) {
            return Err(WisDiag::EScopeOut.val());
        }
        // 第三闸（引用）：证据行引用悬空（双槽逐槽点验）。
        if ev_dangling(row.ev_a) || ev_dangling(row.ev_b) {
            return Err(WisDiag::EDangling.val());
        }
        Ok(row)
    }

    /// 第 class 类资产字节流（0=经验句文本 1=适用面清单 2=证据行全表
    /// 3=状态机档；越界类回落状态机档）。
    pub fn asset_bytes(&self, class: usize) -> Vec<u8> {
        let mut out: Vec<u8> = Vec::new();
        match class {
            0 => {
                // 资产一：经验句文本（行序拼接，行间换行分隔）。
                for row in self.rows.iter() {
                    for b in row.sentence.as_bytes().iter() {
                        out.push(*b);
                    }
                    out.push(b'\n');
                }
            }
            1 => {
                // 资产二：适用面清单（每行位图 2 字节小端，行序冻结）。
                for row in self.rows.iter() {
                    out.push((row.scope & 0xFF) as u8);
                    out.push((row.scope >> 8) as u8);
                }
            }
            2 => {
                // 资产三：证据行全表（闭集 12 行逐行拼接）。
                for line in EVIDENCE_LINES.iter() {
                    for b in line.as_bytes().iter() {
                        out.push(*b);
                    }
                    out.push(b'\n');
                }
            }
            _ => {
                // 资产四：状态机档（行数 8B 小端 + 状态秩 + 容量 + 证据行数 + 段基 4B 小端）。
                let n = self.rows.len() as u64;
                let mut k = 0u32;
                while k < 8 {
                    out.push(((n >> (k * 8)) & 0xFF) as u8);
                    k += 1;
                }
                out.push(self.state.rank());
                out.push(WISDOM_CAP as u8);
                out.push(EVIDENCE_LINES.len() as u8);
                out.push((SEG_BASE & 0xFF) as u8);
                out.push(((SEG_BASE >> 8) & 0xFF) as u8);
                out.push(((SEG_BASE >> 16) & 0xFF) as u8);
                out.push(((SEG_BASE >> 24) & 0xFF) as u8);
            }
        }
        out
    }

    /// 四类资产 SHA-256 现算（封存时落册）。
    fn compute_frozens(&self) -> [[u8; 32]; 4] {
        [
            sha256(&self.asset_bytes(0)),
            sha256(&self.asset_bytes(1)),
            sha256(&self.asset_bytes(2)),
            sha256(&self.asset_bytes(3)),
        ]
    }

    /// 沉淀档指纹：四类冻结摘要的 FNV-1a 64 位折合（未冻结回落零）。
    pub fn fingerprint(&self) -> u64 {
        if !self.frozen_ready {
            return 0;
        }
        let mut fp: u64 = 0xcbf29ce4_8422_2325;
        let mut cls = 0usize;
        while cls < 4 {
            if let Some(d) = self.frozens.get(cls) {
                for b in d.iter() {
                    fp ^= *b as u64;
                    fp = fp.wrapping_mul(0x100000001b3);
                }
            }
            cls += 1;
        }
        fp
    }

    /// 版本漂移演练：先验冻结诚实性（原样重算逐字节等于在册摘要），
    /// 再 10 轮逐轮翻一类资产一字节重算 SHA-256，必须检出漂移。
    /// 返回 (检出轮数, 总轮数, 首个逃脱描述)。
    pub fn drift_sweep(&self) -> (usize, usize, String) {
        let mut caught = 0usize;
        let mut total = 0usize;
        let mut escape = String::new();
        // 冻结诚实性：四类原样重算必须逐字节等于在册摘要。
        let mut cls = 0usize;
        while cls < 4 {
            match self.frozen_of(cls) {
                Some(f) => {
                    if sha256(&self.asset_bytes(cls)) != f && escape.is_empty() {
                        escape = format!("冻结不诚实：资产类{} 原样重算不等于在册摘要", cls);
                    }
                }
                None => {
                    if escape.is_empty() {
                        escape = format!("资产类{} 未冻结在册", cls);
                    }
                }
            }
            cls += 1;
        }
        // 10 轮漂移：轮 r 翻资产类 r%4 的一个字节（位置 LCG 抖动）。
        let mut rng = Lcg::new(SEG_BASE.wrapping_add(0x3E) as u64);
        let mut r = 0u32;
        while r < 10 {
            let cls = (r % 4) as usize;
            let mut bytes = self.asset_bytes(cls);
            let pos = rng.below(bytes.len() as u32) as usize;
            let flipped = match bytes.get_mut(pos) {
                Some(slot) => {
                    *slot = slot.wrapping_add(1);
                    true
                }
                None => false,
            };
            let drifted = sha256(&bytes);
            let detected = match self.frozen_of(cls) {
                Some(f) => flipped && drifted != f,
                None => false,
            };
            total += 1;
            if detected {
                caught += 1;
            } else if escape.is_empty() {
                escape = format!(
                    "漂移逃脱：资产类{} 字节{} 翻转后未检出（{}）",
                    cls,
                    pos,
                    WisDiag::EDrift.detail()
                );
            }
            r += 1;
        }
        (caught, total, escape)
    }
}

/// 构建评审态沉淀档（十行齐 + 晋升一次）。
pub fn build_reviewed_registry() -> WisdomRegistry {
    let mut reg = WisdomRegistry::new();
    for row in WISDOM_TEN.iter() {
        let _ = reg.register(*row);
    }
    let _ = reg.promote();
    reg
}

/// 构建封存态沉淀档（十行齐 + 两次晋升，四类摘要冻结在册）。
pub fn build_full_registry() -> WisdomRegistry {
    let mut reg = build_reviewed_registry();
    let _ = reg.promote();
    reg
}

// ── 注入面：三类注入器，各 10 轮，逐轮精确诊断码点验 ─────────────
/// 注入一：悬空证据注入 10 轮——每轮一行证据引用越过冻结表，
/// 判定器必须逐轮以精确 ESRCH 码拒绝。
pub fn inject_dangling_sweep() -> (usize, usize, String) {
    let mut rng = Lcg::new(SEG_BASE.wrapping_add(0x1C) as u64);
    let mut caught = 0usize;
    let mut total = 0usize;
    let mut escape = String::new();
    let ev_len = EVIDENCE_LINES.len() as u32;
    let mut k = 0u32;
    while k < 10 {
        let off = 1u32 + rng.below(8);
        // 偶数轮悬空甲槽，奇数轮悬空乙槽（双槽逐槽点验覆盖）。
        let (a, b) = if k % 2 == 0 {
            (ev_len + off, 0)
        } else {
            (0, ev_len + off)
        };
        let mut rows = Vec::new();
        rows.push(WisdomRow {
            no: k + 1,
            code: "JX-ZD",
            sentence: "注入样本：证据行引用悬空。",
            scope: 0x0001,
            ev_a: a,
            ev_b: b,
        });
        let reg = WisdomRegistry::from_raw(rows);
        total += 1;
        if reg.judge_wisdom(k + 1) == Err(WisDiag::EDangling.val()) {
            caught += 1;
        } else if escape.is_empty() {
            escape = format!("第{}轮悬空证据注入逃脱", k + 1);
        }
        k += 1;
    }
    (caught, total, escape)
}

/// 注入二：适用面越界注入 10 轮——每轮位图带一个 10 面外的高位，
/// 判定器必须逐轮以精确 EINVAL 码拒绝。
pub fn inject_scope_sweep() -> (usize, usize, String) {
    let mut rng = Lcg::new(SEG_BASE.wrapping_add(0x2A) as u64);
    let mut caught = 0usize;
    let mut total = 0usize;
    let mut escape = String::new();
    let mut k = 0u32;
    while k < 10 {
        // 高位越面：位 10..15 轮转（LCG 抖动合法低位，保持面非空）。
        let low = rng.below(SCOPE_FULL as u32 + 1) as u16;
        let mask = (0x0001u16 | low) | (1u16 << (10 + (k % 6)));
        let mut rows = Vec::new();
        rows.push(WisdomRow {
            no: k + 1,
            code: "JX-ZS",
            sentence: "注入样本：适用面越闭集。",
            scope: mask,
            ev_a: 0,
            ev_b: 1,
        });
        let reg = WisdomRegistry::from_raw(rows);
        total += 1;
        if reg.judge_wisdom(k + 1) == Err(WisDiag::EScopeOut.val()) {
            caught += 1;
        } else if escape.is_empty() {
            escape = format!("第{}轮适用面越界注入逃脱", k + 1);
        }
        k += 1;
    }
    (caught, total, escape)
}

/// 注入三：状态倒退注入 10 轮——每轮在草稿/评审/封存三态轮转尝试
/// 倒退与非草稿写入，必须逐轮以精确诊断码拒绝。
pub fn inject_rewind_sweep() -> (usize, usize, String) {
    let mut caught = 0usize;
    let mut total = 0usize;
    let mut escape = String::new();
    let mut k = 0u32;
    while k < 10 {
        let mut ok = true;
        match k % 3 {
            0 => {
                // 封存态：demote 与再 promote 都必须拒。
                let mut reg = build_full_registry();
                if reg.demote() != Err(WisDiag::ERewind.val()) {
                    ok = false;
                }
                if reg.promote() != Err(WisDiag::ERewind.val()) {
                    ok = false;
                }
            }
            1 => {
                // 评审态：demote 拒；评审后登记闸关闭。
                let mut reg = build_reviewed_registry();
                if reg.demote() != Err(WisDiag::ERewind.val()) {
                    ok = false;
                }
                let intr = WisdomRow {
                    no: 11,
                    code: "JX-ZR",
                    sentence: "注入样本：评审后写入。",
                    scope: 0x0001,
                    ev_a: 0,
                    ev_b: 1,
                };
                if reg.register(intr) != Err(WisDiag::EStateWrite.val()) {
                    ok = false;
                }
            }
            _ => {
                // 草稿态：demote 拒。
                let mut reg = WisdomRegistry::new();
                if reg.demote() != Err(WisDiag::ERewind.val()) {
                    ok = false;
                }
            }
        }
        total += 1;
        if ok {
            caught += 1;
        } else if escape.is_empty() {
            escape = format!("第{}轮状态倒退注入逃脱", k + 1);
        }
        k += 1;
    }
    (caught, total, escape)
}

// ── 判据谓词（私有，断言方向：cond 为违约条件） ─────────────────
/// 段位守卫：SEG 对齐 0x40 且落在 VX9 段窗 [0x80000, 0x831C0]。
fn seg_guard_ok() -> bool {
    if SEG_BASE % 0x40 != 0 {
        return false;
    }
    if SEG_BASE < 0x80000 || SEG_BASE > 0x831C0 {
        return false;
    }
    true
}

/// 诊断码闭集守卫：全部落窗口 + 查表命中 + 未知码回落。
fn diag_closed_ok() -> bool {
    for d in WIS_ALL.iter() {
        let v = d.val();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
    }
    if detail(WisDiag::ENoEntry.val()) != WisDiag::ENoEntry.detail() {
        return false;
    }
    if detail(SEG_BASE + 0x3F) == WisDiag::ENoEntry.detail() {
        return false;
    }
    true
}

/// 域经十行冻结闭集守卫：行数、经号、代号、正文、适用面、证据全检。
fn ten_rows_ok() -> bool {
    if WISDOM_TEN.len() != WISDOM_CAP {
        return false;
    }
    for (i, row) in WISDOM_TEN.iter().enumerate() {
        if row.no != (i as u32) + 1 {
            return false;
        }
        if row.code.is_empty() || row.sentence.is_empty() {
            return false;
        }
        if scope_violated(row.scope) {
            return false;
        }
        if ev_dangling(row.ev_a) || ev_dangling(row.ev_b) {
            return false;
        }
    }
    true
}

/// 适用面闭集与证据行冻结表守卫。
fn tables_ok() -> bool {
    if SCOPE_FACETS.len() != 10 {
        return false;
    }
    if SCOPE_FULL != 0x03FF {
        return false;
    }
    if EVIDENCE_LINES.len() != 12 {
        return false;
    }
    for line in EVIDENCE_LINES.iter() {
        if line.is_empty() {
            return false;
        }
    }
    true
}

/// SHA-256 已知向量守卫（FIPS 180-4 附录 B 标准向量）。
fn sha_vectors_ok() -> bool {
    if hex32(&sha256(b""))
        != "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
    {
        return false;
    }
    if hex32(&sha256(b"abc"))
        != "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    {
        return false;
    }
    true
}

/// 正常流：登记十行 → 评审 → 封存冻结 → 逐条点验全通。
fn happy_path_ok() -> bool {
    let mut reg = WisdomRegistry::new();
    for row in WISDOM_TEN.iter() {
        if reg.register(*row).is_err() {
            return false;
        }
    }
    if reg.state() != WisState::Draft {
        return false;
    }
    if reg.promote().is_err() {
        return false;
    }
    if reg.state() != WisState::Reviewed {
        return false;
    }
    if reg.promote().is_err() {
        return false;
    }
    if reg.state() != WisState::Sealed || !reg.is_frozen() {
        return false;
    }
    // 封存后再冻结：拒绝（冻结只许一次）。
    if reg.refreeze() != Err(WisDiag::EFrozen.val()) {
        return false;
    }
    // 十行逐条点验全通。
    let mut no = 1u32;
    while no <= 10 {
        match reg.judge_wisdom(no) {
            Ok(r) => {
                if r.no != no {
                    return false;
                }
            }
            Err(_) => return false,
        }
        no += 1;
    }
    // 四类摘要全在册且非全零。
    let mut cls = 0usize;
    while cls < 4 {
        match reg.frozen_of(cls) {
            Some(d) => {
                if d.iter().all(|b| *b == 0) {
                    return false;
                }
            }
            None => return false,
        }
        cls += 1;
    }
    true
}

/// 判定器三闸逐条命中：ENOENT / ESRCH / EINVAL 各一路精确码。
fn judge_gates_ok() -> bool {
    // 未登记：空册查任何经号。
    let empty = WisdomRegistry::new();
    if empty.judge_wisdom(1) != Err(WisDiag::ENoEntry.val()) {
        return false;
    }
    // 证据悬空：越表引用。
    let mut rows_d = Vec::new();
    rows_d.push(WisdomRow {
        no: 1,
        code: "JX-Z1",
        sentence: "注入样本：证据行引用悬空。",
        scope: 0x0001,
        ev_a: 999,
        ev_b: 0,
    });
    let raw_d = WisdomRegistry::from_raw(rows_d);
    if raw_d.judge_wisdom(1) != Err(WisDiag::EDangling.val()) {
        return false;
    }
    // 适用面越界：位 10 高位。
    let mut rows_s = Vec::new();
    rows_s.push(WisdomRow {
        no: 1,
        code: "JX-Z2",
        sentence: "注入样本：适用面越闭集。",
        scope: SCOPE_FULL | 0x0400,
        ev_a: 0,
        ev_b: 1,
    });
    let raw_s = WisdomRegistry::from_raw(rows_s);
    if raw_s.judge_wisdom(1) != Err(WisDiag::EScopeOut.val()) {
        return false;
    }
    // 正常命中：封存档逐条 Ok。
    let full = build_full_registry();
    match full.judge_wisdom(5) {
        Ok(r) => {
            if r.no != 5 || r.code.is_empty() || r.sentence.is_empty() {
                return false;
            }
        }
        Err(_) => return false,
    }
    true
}

/// 判定次序冻结：未登记先于适用面、适用面先于证据。
fn judge_order_ok() -> bool {
    // 双违约样本：适用面越界 + 证据悬空 → 先 EINVAL。
    let mut rows = Vec::new();
    rows.push(WisdomRow {
        no: 1,
        code: "JX-Z3",
        sentence: "双违约样本。",
        scope: 0x0400,
        ev_a: 999,
        ev_b: 888,
    });
    let raw = WisdomRegistry::from_raw(rows);
    if raw.judge_wisdom(1) != Err(WisDiag::EScopeOut.val()) {
        return false;
    }
    // 未登记优先于一切：册内有违约行，查未登记经号仍 ENOENT。
    if raw.judge_wisdom(2) != Err(WisDiag::ENoEntry.val()) {
        return false;
    }
    if raw.judge_wisdom(0) != Err(WisDiag::ENoEntry.val()) {
        return false;
    }
    if raw.judge_wisdom(11) != Err(WisDiag::ENoEntry.val()) {
        return false;
    }
    true
}

/// 登记闸全拒：越序/重复/空样本/空面/悬空/超容/非草稿写入。
fn register_gates_ok() -> bool {
    let mut reg = WisdomRegistry::new();
    // 越序：首行必须经号 1。
    let jump = WisdomRow {
        no: 3,
        code: "JX-X",
        sentence: "越序样本。",
        scope: 0x0001,
        ev_a: 0,
        ev_b: 1,
    };
    if reg.register(jump) != Err(WisDiag::EOrder.val()) {
        return false;
    }
    let first = match WISDOM_TEN.get(0) {
        Some(r) => *r,
        None => return false,
    };
    if reg.register(first).is_err() {
        return false;
    }
    // 重复：再登记经号 1。
    if reg.register(first) != Err(WisDiag::EDup.val()) {
        return false;
    }
    // 空样本：正文为空。
    let blank = WisdomRow {
        no: 2,
        code: "JX-X",
        sentence: "",
        scope: 0x0001,
        ev_a: 0,
        ev_b: 1,
    };
    if reg.register(blank) != Err(WisDiag::EEmpty.val()) {
        return false;
    }
    // 空面：位图为零。
    let zero_scope = WisdomRow {
        no: 2,
        code: "JX-X",
        sentence: "空面样本。",
        scope: 0,
        ev_a: 0,
        ev_b: 1,
    };
    if reg.register(zero_scope) != Err(WisDiag::EScopeOut.val()) {
        return false;
    }
    // 悬空：证据引用越表。
    let dang = WisdomRow {
        no: 2,
        code: "JX-X",
        sentence: "悬空样本。",
        scope: 0x0001,
        ev_a: 99,
        ev_b: 1,
    };
    if reg.register(dang) != Err(WisDiag::EDangling.val()) {
        return false;
    }
    // 补齐到十行。
    let mut i = 1usize;
    while i < WISDOM_CAP {
        let row = match WISDOM_TEN.get(i) {
            Some(r) => *r,
            None => return false,
        };
        if reg.register(row).is_err() {
            return false;
        }
        i += 1;
    }
    // 超容：第 11 行。
    let over = WisdomRow {
        no: 11,
        code: "JX-X",
        sentence: "超容样本。",
        scope: 0x0001,
        ev_a: 0,
        ev_b: 1,
    };
    if reg.register(over) != Err(WisDiag::ECapFull.val()) {
        return false;
    }
    // 评审后写入：登记闸关闭。
    if reg.promote().is_err() {
        return false;
    }
    if reg.register(over) != Err(WisDiag::EStateWrite.val()) {
        return false;
    }
    true
}

/// 四类边界：经号边界 / 适用面边界 / 证据引用边界 / 状态机边界。
fn boundary_ok() -> bool {
    // 边界一：经号 0/11 未登记，经号 1/10 在册可点验。
    let full = build_full_registry();
    if full.judge_wisdom(0) != Err(WisDiag::ENoEntry.val()) {
        return false;
    }
    if full.judge_wisdom(11) != Err(WisDiag::ENoEntry.val()) {
        return false;
    }
    if full.judge_wisdom(1).is_err() || full.judge_wisdom(10).is_err() {
        return false;
    }
    // 边界二：适用面 低位单面/全闭集 合法；位 10/位 15/空面 非法。
    let mut r2 = Vec::new();
    r2.push(WisdomRow {
        no: 1,
        code: "JX-B",
        sentence: "低位单面。",
        scope: 0x0001,
        ev_a: 0,
        ev_b: 0,
    });
    r2.push(WisdomRow {
        no: 2,
        code: "JX-B",
        sentence: "全闭集。",
        scope: SCOPE_FULL,
        ev_a: 0,
        ev_b: 0,
    });
    r2.push(WisdomRow {
        no: 3,
        code: "JX-B",
        sentence: "位10越面。",
        scope: 0x0400,
        ev_a: 0,
        ev_b: 0,
    });
    r2.push(WisdomRow {
        no: 4,
        code: "JX-B",
        sentence: "位15越面。",
        scope: 0x8000,
        ev_a: 0,
        ev_b: 0,
    });
    r2.push(WisdomRow {
        no: 5,
        code: "JX-B",
        sentence: "空面。",
        scope: 0,
        ev_a: 0,
        ev_b: 0,
    });
    let reg2 = WisdomRegistry::from_raw(r2);
    if reg2.judge_wisdom(1).is_err() || reg2.judge_wisdom(2).is_err() {
        return false;
    }
    if reg2.judge_wisdom(3) != Err(WisDiag::EScopeOut.val()) {
        return false;
    }
    if reg2.judge_wisdom(4) != Err(WisDiag::EScopeOut.val()) {
        return false;
    }
    if reg2.judge_wisdom(5) != Err(WisDiag::EScopeOut.val()) {
        return false;
    }
    // 边界三：证据引用 下界 0 与上界 len-1 合法；len 与 u32::MAX 悬空。
    let last = (EVIDENCE_LINES.len() as u32).saturating_sub(1);
    let mut r3 = Vec::new();
    r3.push(WisdomRow {
        no: 1,
        code: "JX-B",
        sentence: "证据下界。",
        scope: 0x0001,
        ev_a: 0,
        ev_b: 0,
    });
    r3.push(WisdomRow {
        no: 2,
        code: "JX-B",
        sentence: "证据上界。",
        scope: 0x0001,
        ev_a: last,
        ev_b: 0,
    });
    r3.push(WisdomRow {
        no: 3,
        code: "JX-B",
        sentence: "证据越界。",
        scope: 0x0001,
        ev_a: EVIDENCE_LINES.len() as u32,
        ev_b: 0,
    });
    r3.push(WisdomRow {
        no: 4,
        code: "JX-B",
        sentence: "证据极大。",
        scope: 0x0001,
        ev_a: u32::MAX,
        ev_b: 0,
    });
    let reg3 = WisdomRegistry::from_raw(r3);
    if reg3.judge_wisdom(1).is_err() || reg3.judge_wisdom(2).is_err() {
        return false;
    }
    if reg3.judge_wisdom(3) != Err(WisDiag::EDangling.val()) {
        return false;
    }
    if reg3.judge_wisdom(4) != Err(WisDiag::EDangling.val()) {
        return false;
    }
    // 边界四：空册晋升拒；封存档 demote/再 promote 均拒。
    let mut empty = WisdomRegistry::new();
    if empty.promote() != Err(WisDiag::ENotFull.val()) {
        return false;
    }
    let mut sealed = build_full_registry();
    if sealed.demote() != Err(WisDiag::ERewind.val()) {
        return false;
    }
    if sealed.promote() != Err(WisDiag::ERewind.val()) {
        return false;
    }
    true
}

/// 注入一必抓：悬空证据 10/10 逐轮精确 ESRCH 命中，无逃脱。
fn inject_dangling_ok() -> bool {
    let (caught, total, escape) = inject_dangling_sweep();
    total == 10 && caught == 10 && escape.is_empty()
}

/// 注入二必抓：适用面越界 10/10 逐轮精确 EINVAL 命中，无逃脱。
fn inject_scope_ok() -> bool {
    let (caught, total, escape) = inject_scope_sweep();
    total == 10 && caught == 10 && escape.is_empty()
}

/// 注入三必抓：状态倒退 10/10 逐轮精确 ERewind 命中，无逃脱。
fn inject_rewind_ok() -> bool {
    let (caught, total, escape) = inject_rewind_sweep();
    total == 10 && caught == 10 && escape.is_empty()
}

/// 版本漂移演练 10/10 检出（含冻结诚实性前置）。
fn drift_ok() -> bool {
    let reg = build_full_registry();
    let (caught, total, escape) = reg.drift_sweep();
    total == 10 && caught == 10 && escape.is_empty()
}

/// 双跑一致：两次全流程行、状态、指纹、漂移演练逐项相同。
fn dual_run_ok() -> bool {
    let a = build_full_registry();
    let b = build_full_registry();
    if a.row_count() != b.row_count() || a.state() != b.state() {
        return false;
    }
    if a.fingerprint() != b.fingerprint() || a.fingerprint() == 0 {
        return false;
    }
    let mut i = 0usize;
    while i < a.row_count() {
        match (a.row_at(i), b.row_at(i)) {
            (Some(x), Some(y)) => {
                if x != y {
                    return false;
                }
            }
            _ => return false,
        }
        i += 1;
    }
    let s1 = a.drift_sweep();
    let s2 = b.drift_sweep();
    s1 == s2
}

// ── 检查入口 ─────────────────────────────────────────────────────
/// UNX-F12770 自检（判据 12770 卡：域经十条齐 + 四类资产 SHA-256 冻结
/// 在册 + 版本漂移演练 10/10 检出）。
pub fn run_unx12770_checks() -> CheckSet {
    let mut s = CheckSet::new("unx12770");
    let reg = build_full_registry();

    s.add_dyn(
        "F12770-01 段位守卫",
        seg_guard_ok(),
        &format!("seg=0x{:05x} align40={}", SEG_BASE, SEG_BASE % 0x40 == 0),
    );

    s.add_dyn(
        "F12770-02 诊断码闭集",
        diag_closed_ok(),
        &format!(
            "n={} first=0x{:05x}",
            WIS_ALL.len(),
            WIS_ALL.first().map(|d| d.val()).unwrap_or(0)
        ),
    );

    s.add_dyn(
        "F12770-03 域经十行冻结闭集",
        ten_rows_ok(),
        &format!("rows={} cap={}", WISDOM_TEN.len(), WISDOM_CAP),
    );

    s.add_dyn(
        "F12770-04 适用面闭集与证据行表",
        tables_ok(),
        &format!(
            "facets={} evs={} full=0x{:04x}",
            SCOPE_FACETS.len(),
            EVIDENCE_LINES.len(),
            SCOPE_FULL
        ),
    );

    s.add_dyn(
        "F12770-05 SHA-256 已知向量",
        sha_vectors_ok(),
        &format!("fpc={:016x}", reg.fingerprint()),
    );

    s.add_dyn(
        "F12770-06 正常流登记评审封存",
        happy_path_ok(),
        &format!(
            "rows={} state={} frozen={}",
            reg.row_count(),
            reg.state().name(),
            reg.is_frozen()
        ),
    );

    let brief5 = match reg.judge_wisdom(5) {
        Ok(r) => wisdom_brief(&r),
        Err(_) => String::new(),
    };
    s.add_dyn(
        "F12770-07 判定器三闸逐条命中",
        judge_gates_ok(),
        &brief5,
    );

    s.add_dyn(
        "F12770-08 判定次序冻结",
        judge_order_ok(),
        "未登记→适用面→证据 三级依次短路",
    );

    s.add_dyn(
        "F12770-09 登记闸全拒",
        register_gates_ok(),
        "越序/重复/空样本/空面/悬空/超容/非草稿 七闸",
    );

    let (c1, t1, e1) = inject_dangling_sweep();
    s.add_dyn(
        "F12770-10 注入一悬空证据10/10",
        inject_dangling_ok(),
        &format!("抓 {}/{} {}", c1, t1, if e1.is_empty() { "无逃脱" } else { &e1 }),
    );

    let (c2, t2, e2) = inject_scope_sweep();
    s.add_dyn(
        "F12770-11 注入二适用面越界10/10",
        inject_scope_ok(),
        &format!("抓 {}/{} {}", c2, t2, if e2.is_empty() { "无逃脱" } else { &e2 }),
    );

    let (c3, t3, e3) = inject_rewind_sweep();
    s.add_dyn(
        "F12770-12 注入三状态倒退10/10",
        inject_rewind_ok(),
        &format!("抓 {}/{} {}", c3, t3, if e3.is_empty() { "无逃脱" } else { &e3 }),
    );

    let (cd, td, ed) = reg.drift_sweep();
    s.add_dyn(
        "F12770-13 版本漂移演练10/10",
        drift_ok(),
        &format!(
            "检出 {}/{} {}",
            cd,
            td,
            if ed.is_empty() { "无逃脱" } else { &ed }
        ),
    );

    s.add_dyn(
        "F12770-14 双跑一致指纹",
        dual_run_ok(),
        &format!("fp={:016x}", reg.fingerprint()),
    );

    s.add_dyn(
        "F12770-15 四类边界",
        boundary_ok(),
        "经号0/11·适用面位10/15/空面·证据下上界/越界·空册晋升与封存倒退",
    );

    // 收口：判据条数声明（15 条判据 + 1 条收口 = 16）。
    s.add_dyn(
        "F12770-16 收口-判据条数声明",
        s.len() == 15 && s.all_passed(),
        "判据锚 UNX-F12771-J1：四类资产SHA-256冻结在册，漂移演练10/10",
    );

    s
}

// ── 测试区 ───────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seg_guard_aligned_and_in_window() {
        // 段位守卫：对齐 0x40 且落在 VX9 段窗，且与派工公式一致。
        assert!(seg_guard_ok());
        assert_eq!(SEG_BASE % 0x40, 0);
        assert!(SEG_BASE >= 0x80000 && SEG_BASE <= 0x831C0);
        assert_eq!(SEG_BASE, 0x80000 + 0x40 * (12770 - 12600));
    }

    #[test]
    fn sha256_known_vectors() {
        // FIPS 180-4 标准向量：空串与 "abc"。
        assert_eq!(
            hex32(&sha256(b"")),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert_eq!(
            hex32(&sha256(b"abc")),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        // 56 字节与 64 字节边界（触发 0x80 落点分支）。
        let d56 = [0x61u8; 56];
        let d64 = [0x61u8; 64];
        assert_ne!(hex32(&sha256(&d56)), hex32(&sha256(&d64)));
        assert_eq!(hex32(&sha256(&d56)).len(), 64);
    }

    #[test]
    fn happy_path_seals_and_freezes() {
        // 正常流：登记十行 → 评审 → 封存冻结 → 逐条点验。
        let mut reg = WisdomRegistry::new();
        for row in WISDOM_TEN.iter() {
            assert!(reg.register(*row).is_ok(), "经{}登记被拒", row.no);
        }
        assert_eq!(reg.row_count(), 10);
        assert_eq!(reg.state(), WisState::Draft);
        assert!(reg.promote().is_ok());
        assert_eq!(reg.state(), WisState::Reviewed);
        assert!(reg.promote().is_ok());
        assert_eq!(reg.state(), WisState::Sealed);
        assert!(reg.is_frozen());
        // 封存后再冻结拒绝。
        assert_eq!(reg.refreeze(), Err(WisDiag::EFrozen.val()));
        // 十行逐条点验全通且内容与冻结表一致。
        let mut no = 1u32;
        while no <= 10 {
            let expect = WISDOM_TEN
                .get((no - 1) as usize)
                .copied()
                .unwrap_or(WisdomRow {
                    no: 0,
                    code: "",
                    sentence: "",
                    scope: 0,
                    ev_a: 0,
                    ev_b: 0,
                });
            assert_eq!(reg.judge_wisdom(no), Ok(expect));
            no += 1;
        }
        // 四类摘要全在册且指纹非零。
        assert!(reg.fingerprint() != 0);
        let mut cls = 0usize;
        while cls < 4 {
            assert!(reg.frozen_of(cls).is_some());
            cls += 1;
        }
    }

    #[test]
    fn judge_gates_and_order() {
        // 三闸逐条命中：ENOENT / EINVAL / ESRCH 精确码。
        let empty = WisdomRegistry::new();
        assert_eq!(empty.judge_wisdom(1), Err(WisDiag::ENoEntry.val()));
        let mut rows = Vec::new();
        rows.push(WisdomRow {
            no: 1,
            code: "JX-Z1",
            sentence: "注入样本：证据行引用悬空。",
            scope: 0x0001,
            ev_a: 999,
            ev_b: 0,
        });
        let raw_d = WisdomRegistry::from_raw(rows);
        assert_eq!(raw_d.judge_wisdom(1), Err(WisDiag::EDangling.val()));
        let mut rows2 = Vec::new();
        rows2.push(WisdomRow {
            no: 1,
            code: "JX-Z2",
            sentence: "注入样本：适用面越闭集。",
            scope: SCOPE_FULL | 0x0400,
            ev_a: 0,
            ev_b: 1,
        });
        let raw_s = WisdomRegistry::from_raw(rows2);
        assert_eq!(raw_s.judge_wisdom(1), Err(WisDiag::EScopeOut.val()));
        // 判定次序：双违约先 EINVAL；未登记先于一切。
        let mut both = Vec::new();
        both.push(WisdomRow {
            no: 1,
            code: "JX-Z3",
            sentence: "双违约样本。",
            scope: 0x0400,
            ev_a: 999,
            ev_b: 888,
        });
        let raw_b = WisdomRegistry::from_raw(both);
        assert_eq!(raw_b.judge_wisdom(1), Err(WisDiag::EScopeOut.val()));
        assert_eq!(raw_b.judge_wisdom(2), Err(WisDiag::ENoEntry.val()));
        assert_eq!(raw_b.judge_wisdom(0), Err(WisDiag::ENoEntry.val()));
        assert_eq!(raw_b.judge_wisdom(11), Err(WisDiag::ENoEntry.val()));
    }

    #[test]
    fn three_injections_all_caught() {
        // 三类注入各 10 轮，逐轮以精确诊断码命中，无逃脱。
        let (c1, t1, e1) = inject_dangling_sweep();
        assert_eq!(t1, 10);
        assert_eq!(c1, 10, "悬空证据逃脱：{}", e1);
        assert!(e1.is_empty());
        let (c2, t2, e2) = inject_scope_sweep();
        assert_eq!(t2, 10);
        assert_eq!(c2, 10, "适用面越界逃脱：{}", e2);
        assert!(e2.is_empty());
        let (c3, t3, e3) = inject_rewind_sweep();
        assert_eq!(t3, 10);
        assert_eq!(c3, 10, "状态倒退逃脱：{}", e3);
        assert!(e3.is_empty());
        assert!(inject_dangling_ok() && inject_scope_ok() && inject_rewind_ok());
    }

    #[test]
    fn drift_detected_10_of_10() {
        // 版本漂移演练：冻结诚实性前置 + 10/10 检出。
        let reg = build_full_registry();
        let (caught, total, escape) = reg.drift_sweep();
        assert_eq!(total, 10);
        assert_eq!(caught, 10, "漂移逃脱：{}", escape);
        assert!(escape.is_empty());
        // 诚实性单验：四类原样重算逐字节等于在册摘要。
        let mut cls = 0usize;
        while cls < 4 {
            let f = reg.frozen_of(cls).unwrap_or([0u8; 32]);
            assert_eq!(sha256(&reg.asset_bytes(cls)), f);
            cls += 1;
        }
        assert!(drift_ok());
    }

    #[test]
    fn dual_run_fingerprint_identical() {
        // 双跑一致：行、状态、指纹、漂移演练逐项相同。
        let a = build_full_registry();
        let b = build_full_registry();
        assert_eq!(a.row_count(), b.row_count());
        assert_eq!(a.state(), b.state());
        assert_eq!(a.fingerprint(), b.fingerprint());
        assert!(a.fingerprint() != 0);
        let mut i = 0usize;
        while i < a.row_count() {
            assert_eq!(a.row_at(i), b.row_at(i));
            i += 1;
        }
        assert_eq!(a.drift_sweep(), b.drift_sweep());
        assert!(dual_run_ok());
    }

    #[test]
    fn capacity_exhaustion_at_ten() {
        // 容量耗尽：第 11 行拒绝；评审后登记闸关闭。
        let mut reg = WisdomRegistry::new();
        for row in WISDOM_TEN.iter() {
            assert!(reg.register(*row).is_ok());
        }
        let over = WisdomRow {
            no: 11,
            code: "JX-X",
            sentence: "超容样本。",
            scope: 0x0001,
            ev_a: 0,
            ev_b: 1,
        };
        assert_eq!(reg.register(over), Err(WisDiag::ECapFull.val()));
        assert_eq!(reg.row_count(), 10);
        assert!(reg.promote().is_ok());
        assert_eq!(reg.register(over), Err(WisDiag::EStateWrite.val()));
    }

    #[test]
    fn register_gates_reject_all() {
        // 登记闸全拒：越序/重复/空样本/空面/悬空/超容/非草稿写入。
        assert!(register_gates_ok());
        // 越序后可回补：首行经号 3 被拒后登记经号 1 应成功。
        let mut reg = WisdomRegistry::new();
        let jump = WisdomRow {
            no: 3,
            code: "JX-X",
            sentence: "越序样本。",
            scope: 0x0001,
            ev_a: 0,
            ev_b: 1,
        };
        assert_eq!(reg.register(jump), Err(WisDiag::EOrder.val()));
        assert_eq!(reg.row_count(), 0);
        let first = WISDOM_TEN.get(0).copied().unwrap_or(jump);
        assert!(reg.register(first).is_ok());
    }

    #[test]
    fn state_machine_never_rewinds() {
        // 状态机单调：Draft→Reviewed→Sealed 只进不退。
        assert!(WisState::Draft.rank() < WisState::Reviewed.rank());
        assert!(WisState::Reviewed.rank() < WisState::Sealed.rank());
        let mut reg = build_reviewed_registry();
        assert_eq!(reg.state(), WisState::Reviewed);
        assert_eq!(reg.demote(), Err(WisDiag::ERewind.val()));
        assert_eq!(reg.state(), WisState::Reviewed);
        assert!(reg.promote().is_ok());
        assert_eq!(reg.state(), WisState::Sealed);
        assert_eq!(reg.demote(), Err(WisDiag::ERewind.val()));
        assert_eq!(reg.promote(), Err(WisDiag::ERewind.val()));
        assert_eq!(reg.state(), WisState::Sealed);
        // 注入三整体回归。
        let (c3, t3, e3) = inject_rewind_sweep();
        assert_eq!(t3, 10);
        assert_eq!(c3, 10, "倒退逃脱：{}", e3);
    }

    #[test]
    fn boundary_four_classes() {
        // 四类边界：经号 / 适用面 / 证据引用 / 状态机。
        assert!(boundary_ok());
        // 经号边界补钉：0 与 11 未登记，1 与 10 在册。
        let full = build_full_registry();
        assert_eq!(full.judge_wisdom(0), Err(WisDiag::ENoEntry.val()));
        assert_eq!(full.judge_wisdom(11), Err(WisDiag::ENoEntry.val()));
        assert!(full.judge_wisdom(1).is_ok());
        assert!(full.judge_wisdom(10).is_ok());
    }

    #[test]
    fn diag_codes_are_closed_set() {
        // 闭集：12 个码互异、全落窗口、查表命中、未知码回落。
        assert_eq!(WIS_ALL.len(), 12);
        let mut i = 0usize;
        while i < WIS_ALL.len() {
            let vi = WIS_ALL.get(i).map(|d| d.val()).unwrap_or(0);
            assert!(vi >= SEG_BASE && vi < SEG_BASE + 0x40);
            let mut j = i + 1;
            while j < WIS_ALL.len() {
                let vj = WIS_ALL.get(j).map(|d| d.val()).unwrap_or(0);
                assert_ne!(vi, vj, "诊断码重复 0x{:05x}", vi);
                j += 1;
            }
            assert!(!detail(vi).is_empty());
            i += 1;
        }
        assert!(detail(SEG_BASE + 0x30).contains("未知"));
        assert_eq!(
            detail(WisDiag::EDangling.val()),
            WisDiag::EDangling.detail()
        );
    }

    #[test]
    fn checks_entry_all_green() {
        // 检查入口全绿且覆盖 ≥12 条（本节 16 = 15 判据 + 1 收口）。
        let s = run_unx12770_checks();
        assert!(s.all_passed(), "红项: {}", s.red_items_text());
        assert!(s.len() >= 12);
        assert_eq!(s.len(), 16);
        assert_eq!(s.tally().1, 0);
    }
}
