//! UNX-F11495 五工具错误文案真值（0x20ec0 段）
//!
//! 错误文案是脚本判据的一部分：同一故障在不同工具、不同层级上的字节串必须逐字
//! 可复现。这里把 echo、dd、xargs、tr、wc 五个工具各取三层——用法错、运行错、
//! 致命错——共十五条真值挂在编号表上，编号即判据，工具与层级只是索引的两个维。
//!
//! 查表先验维：工具不在 1..=5 是 ETOOL，层级不在 1..=3 是 ETIER。归类按最长
//! 前缀：表项的字节串是输入的前缀即命中，多个命中取最长的那条，保证
//! `xargs: echo: ...` 不会被更短的 `xargs: ...` 抢走。精确判定则要求长度相等，
//! 输入比命中项长是 EPART（尾随垃圾），比所有项都短但仍是某条的前缀是 ESHORT，
//! 完全不沾边是 ENOMATCH，空串单独记 ENUL。
//!
//! 账本十四格：八格成功覆盖三层取文、前缀归类、精确命中、工具/层级反查与层计数；
//! 六格拒绝依次是工具越界、层级越界、无匹配、短前缀、长尾与空串。

use crate::alloc::format;
use crate::checks::CheckSet;

/// 段基址。
pub const SEG_BASE: u32 = 0x20ec0;

/// 编号非法。
pub const FE_EID: u32 = SEG_BASE + 0x01;
/// 登记顺序错误。
pub const FE_EORDER: u32 = SEG_BASE + 0x02;
/// 重复登记。
pub const FE_EDUP: u32 = SEG_BASE + 0x03;
/// 已冻结。
pub const FE_EFROZEN: u32 = SEG_BASE + 0x04;
/// 观测不存在的格。
pub const FE_ENOBS: u32 = SEG_BASE + 0x05;
/// 结论不符。
pub const FE_EMISMATCH: u32 = SEG_BASE + 0x06;
/// 账本失衡。
pub const FE_EBAL: u32 = SEG_BASE + 0x07;
/// 容量满。
pub const FE_EFULL: u32 = SEG_BASE + 0x08;
/// 票号不一致。
pub const FE_ESEAL: u32 = SEG_BASE + 0x09;
/// 工具越界。
pub const FE_ETOOL: u32 = SEG_BASE + 0x0a;
/// 层级越界。
pub const FE_ETIER: u32 = SEG_BASE + 0x0b;
/// 无任何表项命中。
pub const FE_ENOMATCH: u32 = SEG_BASE + 0x0c;
/// 输入是表项的短前缀。
pub const FE_ESHORT: u32 = SEG_BASE + 0x0d;
/// 命中项后仍有尾随字节。
pub const FE_EPART: u32 = SEG_BASE + 0x0e;
/// 空串输入。
pub const FE_ENUL: u32 = SEG_BASE + 0x0f;

/// 判决：成功。
pub const V_OK: u32 = 0;

/// 工具：echo。
pub const TOOL_ECHO: u8 = 1;
/// 工具：dd。
pub const TOOL_DD: u8 = 2;
/// 工具：xargs。
pub const TOOL_XARGS: u8 = 3;
/// 工具：tr。
pub const TOOL_TR: u8 = 4;
/// 工具：wc。
pub const TOOL_WC: u8 = 5;

/// 层级：用法错。
pub const TIER_USAGE: u8 = 1;
/// 层级：运行错。
pub const TIER_RUNTIME: u8 = 2;
/// 层级：致命错。
pub const TIER_FATAL: u8 = 3;

/// 操作：按工具与层级取文案。
pub const OP_MSG: u8 = 1;
/// 操作：前缀归类。
pub const OP_CLASS: u8 = 2;
/// 操作：精确判定。
pub const OP_EXACT: u8 = 3;
/// 操作：按编号反查工具。
pub const OP_TOOL: u8 = 4;
/// 操作：按编号反查层级。
pub const OP_TIER: u8 = 5;
/// 操作：层计数。
pub const OP_COUNT: u8 = 6;

/// 真值条数。
pub const MESSAGES: usize = 15;
/// 样本格数。
pub const SAMPLES: usize = 14;

/// 一条错误文案。
#[derive(Clone, Copy, Debug)]
pub struct Msg {
    pub id: u8,
    pub tool: u8,
    pub tier: u8,
    pub text: &'static [u8],
}

/// 十五条真值，编号即判据。
pub const TABLE: [Msg; MESSAGES] = [
    Msg { id: 1, tool: TOOL_ECHO, tier: TIER_USAGE, text: b"echo: invalid option -- 'z'" },
    Msg { id: 2, tool: TOOL_ECHO, tier: TIER_RUNTIME, text: b"echo: write error: No space left on device" },
    Msg { id: 3, tool: TOOL_ECHO, tier: TIER_FATAL, text: b"echo: write error: Broken pipe" },
    Msg { id: 4, tool: TOOL_DD, tier: TIER_USAGE, text: b"dd: invalid option -- 'z'" },
    Msg { id: 5, tool: TOOL_DD, tier: TIER_RUNTIME, text: b"dd: failed to open 'in': No such file or directory" },
    Msg { id: 6, tool: TOOL_DD, tier: TIER_FATAL, text: b"dd: error writing 'out': Input/output error" },
    Msg { id: 7, tool: TOOL_XARGS, tier: TIER_USAGE, text: b"xargs: invalid option -- 'z'" },
    Msg { id: 8, tool: TOOL_XARGS, tier: TIER_RUNTIME, text: b"xargs: echo: No such file or directory" },
    Msg { id: 9, tool: TOOL_XARGS, tier: TIER_FATAL, text: b"xargs: command terminated by signal 9" },
    Msg { id: 10, tool: TOOL_TR, tier: TIER_USAGE, text: b"tr: missing operand" },
    Msg { id: 11, tool: TOOL_TR, tier: TIER_RUNTIME, text: b"tr: write error: No space left on device" },
    Msg { id: 12, tool: TOOL_TR, tier: TIER_FATAL, text: b"tr: read error: Input/output error" },
    Msg { id: 13, tool: TOOL_WC, tier: TIER_USAGE, text: b"wc: invalid option -- 'z'" },
    Msg { id: 14, tool: TOOL_WC, tier: TIER_RUNTIME, text: b"wc: stdin: No such file or directory" },
    Msg { id: 15, tool: TOOL_WC, tier: TIER_FATAL, text: b"wc: write error: Broken pipe" },
];

/// FNV 偏移基。
pub const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;
/// FNV 质数。
pub const FNV_PRIME: u64 = 0x100_0000_01b3;

/// 折入。
pub fn fold(h: u64, v: u64) -> u64 {
    let mut x = h;
    let mut n = v;
    let mut i = 0u32;
    while i < 8 {
        x ^= n & 0xff;
        x = x.wrapping_mul(FNV_PRIME);
        n >>= 8;
        i += 1;
    }
    x
}

fn valid_tool(tool: u8) -> bool {
    tool >= TOOL_ECHO && tool <= TOOL_WC
}

fn valid_tier(tier: u8) -> bool {
    tier >= TIER_USAGE && tier <= TIER_FATAL
}

/// 前缀判定：pre 是否完整出现在 text 的头部。
pub fn has_prefix(text: &[u8], pre: &[u8]) -> bool {
    if pre.len() > text.len() {
        return false;
    }
    let mut i = 0usize;
    while i < pre.len() {
        match (text.get(i), pre.get(i)) {
            (Some(a), Some(b)) => {
                if a != b {
                    return false;
                }
            }
            _ => return false,
        }
        i += 1;
    }
    true
}

fn find(tool: u8, tier: u8) -> Option<&'static Msg> {
    let mut i = 0usize;
    while i < MESSAGES {
        if let Some(m) = TABLE.get(i) {
            if m.tool == tool && m.tier == tier {
                return Some(m);
            }
        }
        i += 1;
    }
    None
}

/// 按编号反查表项。
pub fn by_id(id: u8) -> Option<&'static Msg> {
    let mut i = 0usize;
    while i < MESSAGES {
        if let Some(m) = TABLE.get(i) {
            if m.id == id {
                return Some(m);
            }
        }
        i += 1;
    }
    None
}

/// 取文案：维先验，再查表。
pub fn msg(tool: u8, tier: u8) -> Result<&'static [u8], u32> {
    if !valid_tool(tool) {
        return Err(FE_ETOOL);
    }
    if !valid_tier(tier) {
        return Err(FE_ETIER);
    }
    match find(tool, tier) {
        Some(m) => Ok(m.text),
        None => Err(FE_ENOMATCH),
    }
}

/// 取编号。
pub fn msg_id(tool: u8, tier: u8) -> Result<u32, u32> {
    match msg(tool, tier) {
        Ok(_) => match find(tool, tier) {
            Some(m) => Ok(m.id as u32),
            None => Err(FE_ENOMATCH),
        },
        Err(c) => Err(c),
    }
}

/// 最长前缀归类。
pub fn classify(text: &[u8]) -> Result<u8, u32> {
    let mut best: Option<&'static Msg> = None;
    let mut i = 0usize;
    while i < MESSAGES {
        if let Some(m) = TABLE.get(i) {
            if has_prefix(text, m.text) {
                let better = match best {
                    Some(b) => m.text.len() > b.text.len(),
                    None => true,
                };
                if better {
                    best = Some(m);
                }
            }
        }
        i += 1;
    }
    match best {
        Some(m) => Ok(m.id),
        None => Err(FE_ENOMATCH),
    }
}

/// 精确判定：长度必须相等。
pub fn exact(text: &[u8]) -> Result<u8, u32> {
    if text.is_empty() {
        return Err(FE_ENUL);
    }
    match classify(text) {
        Ok(id) => match by_id(id) {
            Some(m) => {
                if m.text.len() == text.len() {
                    Ok(id)
                } else {
                    Err(FE_EPART)
                }
            }
            None => Err(FE_ENOMATCH),
        },
        Err(_) => {
            let mut short = false;
            let mut i = 0usize;
            while i < MESSAGES {
                if let Some(m) = TABLE.get(i) {
                    if m.text.len() > text.len() && has_prefix(m.text, text) {
                        short = true;
                    }
                }
                i += 1;
            }
            if short {
                Err(FE_ESHORT)
            } else {
                Err(FE_ENOMATCH)
            }
        }
    }
}

/// 按编号反查工具。
pub fn tool_of(id: u8) -> Result<u8, u32> {
    match by_id(id) {
        Some(m) => Ok(m.tool),
        None => Err(FE_ENOMATCH),
    }
}

/// 按编号反查层级。
pub fn tier_of(id: u8) -> Result<u8, u32> {
    match by_id(id) {
        Some(m) => Ok(m.tier),
        None => Err(FE_ENOMATCH),
    }
}

/// 层计数。
pub fn count_tier(tier: u8) -> Result<usize, u32> {
    if !valid_tier(tier) {
        return Err(FE_ETIER);
    }
    let mut n = 0usize;
    let mut i = 0usize;
    while i < MESSAGES {
        if let Some(m) = TABLE.get(i) {
            if m.tier == tier {
                n += 1;
            }
        }
        i += 1;
    }
    Ok(n)
}

/// 一个账本样本。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Sample {
    pub slot: u8,
    pub op: u8,
    pub ok: bool,
    pub code: u32,
    pub value: u32,
}

impl Sample {
    /// 空白格。
    pub const fn blank() -> Sample {
        Sample {
            slot: 0,
            op: 0,
            ok: false,
            code: 0,
            value: 0,
        }
    }
}

/// 样本账本。
#[derive(Clone, Copy, Debug)]
pub struct Ledger {
    rows: [Sample; SAMPLES],
    len: usize,
    cap: usize,
    seal: u64,
    frozen: bool,
    rejects: u32,
}

impl Ledger {
    /// 空账本。
    pub const fn new() -> Ledger {
        Ledger {
            rows: [Sample::blank(); SAMPLES],
            len: 0,
            cap: SAMPLES,
            seal: 0,
            frozen: false,
            rejects: 0,
        }
    }

    /// 指定容量。
    pub const fn with_cap(cap: usize) -> Ledger {
        Ledger {
            rows: [Sample::blank(); SAMPLES],
            len: 0,
            cap,
            seal: 0,
            frozen: false,
            rejects: 0,
        }
    }

    /// 登记一格：冻结 → 编号 → 重复 → 顺序 → 容量。
    pub fn register(&mut self, row: Sample) -> Result<(), u32> {
        if self.frozen {
            self.rejects += 1;
            return Err(FE_EFROZEN);
        }
        if (row.slot as usize) >= SAMPLES {
            return Err(FE_EID);
        }
        if (row.slot as usize) < self.len {
            return Err(FE_EDUP);
        }
        if (row.slot as usize) != self.len {
            return Err(FE_EORDER);
        }
        if self.len >= self.cap {
            return Err(FE_EFULL);
        }
        match self.rows.get_mut(self.len) {
            Some(x) => *x = row,
            None => return Err(FE_EID),
        }
        self.len += 1;
        Ok(())
    }

    /// 格数。
    pub fn len(&self) -> usize {
        self.len
    }

    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// 取样本。
    pub fn sample(&self, i: usize) -> Option<Sample> {
        if i >= self.len {
            return None;
        }
        self.rows.get(i).copied()
    }

    /// 成功计数。
    pub fn ok_count(&self) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < self.len {
            if let Some(s) = self.rows.get(i) {
                if s.ok {
                    n += 1;
                }
            }
            i += 1;
        }
        n
    }

    /// 拒绝计数。
    pub fn err_count(&self) -> usize {
        self.len - self.ok_count()
    }

    /// 对账。
    pub fn audit(&self) -> Result<(), u32> {
        if self.len != SAMPLES {
            return Err(FE_EBAL);
        }
        if self.ok_count() != 8 || self.err_count() != 6 {
            return Err(FE_EBAL);
        }
        let mut i = 0usize;
        while i < self.len {
            match self.rows.get(i) {
                Some(s) => {
                    if s.slot as usize != i {
                        return Err(FE_EORDER);
                    }
                    if s.ok {
                        if s.code != V_OK {
                            return Err(FE_EMISMATCH);
                        }
                    } else if s.code == V_OK || s.value != 0 {
                        return Err(FE_EMISMATCH);
                    }
                }
                None => return Err(FE_ENOBS),
            }
            i += 1;
        }
        let vals = [2u32, 15, 10, 8, 4, 4, 3, 5];
        let mut k = 0usize;
        while k < vals.len() {
            match (self.sample(k), vals.get(k)) {
                (Some(s), Some(w)) => {
                    if !s.ok || s.value != *w {
                        return Err(FE_EMISMATCH);
                    }
                }
                _ => return Err(FE_EBAL),
            }
            k += 1;
        }
        let codes = [FE_ETOOL, FE_ETIER, FE_ENOMATCH, FE_ESHORT, FE_EPART, FE_ENUL];
        let mut c = 0usize;
        let mut j = 8usize;
        while j < self.len {
            match self.rows.get(j) {
                Some(s) => match codes.get(c) {
                    Some(w) => {
                        if s.code != *w {
                            return Err(FE_EMISMATCH);
                        }
                    }
                    None => return Err(FE_EBAL),
                },
                None => return Err(FE_ENOBS),
            }
            c += 1;
            j += 1;
        }
        if c != codes.len() {
            return Err(FE_EBAL);
        }
        Ok(())
    }

    /// 熔断票号。
    pub fn seal(&self) -> u64 {
        let mut h = FNV_OFF;
        let mut i = 0usize;
        while i < self.len {
            if let Some(s) = self.rows.get(i) {
                h = fold(h, s.slot as u64);
                h = fold(h, s.op as u64);
                h = fold(h, s.ok as u64);
                h = fold(h, s.code as u64);
                h = fold(h, s.value as u64);
            }
            i += 1;
        }
        h
    }

    /// 冻结。
    pub fn freeze(&mut self) -> Result<u64, u32> {
        if self.frozen {
            self.rejects += 1;
            return Err(FE_EFROZEN);
        }
        let s = self.seal();
        self.seal = s;
        self.frozen = true;
        Ok(s)
    }

    /// 重放。
    pub fn replay(&self) -> Result<u64, u32> {
        let fresh = build_ledger();
        if fresh.len != self.len {
            return Err(FE_EBAL);
        }
        let h = fresh.seal();
        if self.frozen && h != self.seal {
            return Err(FE_ESEAL);
        }
        Ok(h)
    }

    /// 封存票号。
    pub fn root(&self) -> u64 {
        self.seal
    }

    /// 二次冻结次数。
    pub fn rejects(&self) -> u32 {
        self.rejects
    }

    /// 是否已冻结。
    pub fn is_frozen(&self) -> bool {
        self.frozen
    }
}

fn cell(slot: u8, op: u8, r: Result<u32, u32>) -> Sample {
    match r {
        Ok(v) => Sample {
            slot,
            op,
            ok: true,
            code: V_OK,
            value: v,
        },
        Err(c) => Sample {
            slot,
            op,
            ok: false,
            code: c,
            value: 0,
        },
    }
}

/// 十四格账本。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();

    let _ = l.register(cell(0, OP_MSG, msg_id(TOOL_ECHO, TIER_RUNTIME)));
    let _ = l.register(cell(1, OP_MSG, msg_id(TOOL_WC, TIER_FATAL)));
    let _ = l.register(cell(2, OP_CLASS, classify(b"tr: missing operand").map(u32::from)));
    let _ = l.register(cell(3, OP_CLASS, classify(b"xargs: echo: No such file or directory\n").map(u32::from)));
    let _ = l.register(cell(4, OP_EXACT, exact(b"dd: invalid option -- 'z'").map(u32::from)));
    let _ = l.register(cell(5, OP_TOOL, tool_of(11).map(u32::from)));
    let _ = l.register(cell(6, OP_TIER, tier_of(9).map(u32::from)));
    let _ = l.register(cell(7, OP_COUNT, count_tier(TIER_USAGE).map(|n| n as u32)));

    let _ = l.register(cell(8, OP_MSG, msg_id(TOOL_WC + 1, TIER_USAGE)));
    let _ = l.register(cell(9, OP_MSG, msg_id(TOOL_ECHO, TIER_FATAL + 1)));
    let _ = l.register(cell(10, OP_CLASS, classify(b"cat: nope").map(u32::from)));
    let _ = l.register(cell(11, OP_EXACT, exact(b"tr: missing").map(u32::from)));
    let _ = l.register(cell(12, OP_EXACT, exact(b"tr: missing operand plus").map(u32::from)));
    let _ = l.register(cell(13, OP_EXACT, exact(b"").map(u32::from)));

    l
}

/// 四路注入：账本自洽、取文与反查、归类与精确、拒绝后恢复。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];

    let mut l = build_ledger();
    let base = l.len() == SAMPLES && l.audit().is_ok();
    let rep = l.replay();
    let root = match l.freeze() {
        Ok(v) => v,
        Err(_) => 0,
    };
    let again = l.freeze() == Err(FE_EFROZEN);
    out[0] = base && root != 0 && rep == Ok(root) && again && l.rejects() == 1;

    out[1] = msg_id(TOOL_ECHO, TIER_RUNTIME) == Ok(2)
        && msg_id(TOOL_WC, TIER_FATAL) == Ok(15)
        && tool_of(11) == Ok(TOOL_TR)
        && tier_of(9) == Ok(TIER_FATAL)
        && msg(TOOL_DD, TIER_USAGE).is_ok();

    out[2] = classify(b"tr: missing operand") == Ok(10)
        && classify(b"xargs: echo: No such file or directory\n") == Ok(8)
        && exact(b"tr: missing operand") == Ok(10)
        && exact(b"tr: missing operand ") == Err(FE_EPART)
        && exact(b"tr: missing") == Err(FE_ESHORT);

    let tool = msg_id(0, TIER_USAGE) == Err(FE_ETOOL);
    let tier = msg_id(TOOL_ECHO, 0) == Err(FE_ETIER);
    let none = exact(b"zzz") == Err(FE_ENOMATCH);
    let recovered = msg_id(TOOL_ECHO, TIER_USAGE) == Ok(1);
    out[3] = tool && tier && none && recovered;
    out
}

/// 十三判据。
pub fn run_unx11495_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11495.errtext");
    let l = build_ledger();

    s.add(
        "FE1-十四格齐备",
        l.len() == SAMPLES && !l.is_empty() && l.sample(0).map(|x| x.op) == Some(OP_MSG),
        "static",
    );
    s.add_dyn(
        "FE2-八允六拒分布",
        l.ok_count() == 8 && l.err_count() == 6 && l.audit().is_ok(),
        &format!("ok={} err={} audit={:?}", l.ok_count(), l.err_count(), l.audit()),
    );
    s.add(
        "FE3-三层各五条",
        TABLE.len() == MESSAGES
            && count_tier(TIER_USAGE) == Ok(5)
            && count_tier(TIER_RUNTIME) == Ok(5)
            && count_tier(TIER_FATAL) == Ok(5),
        "static",
    );
    s.add(
        "FE4-三层取文",
        msg_id(TOOL_ECHO, TIER_USAGE) == Ok(1)
            && msg_id(TOOL_ECHO, TIER_RUNTIME) == Ok(2)
            && msg_id(TOOL_WC, TIER_FATAL) == Ok(15)
            && l.sample(0).map(|x| x.value) == Some(2)
            && l.sample(1).map(|x| x.value) == Some(15),
        "static",
    );
    s.add(
        "FE5-最长前缀归类",
        classify(b"tr: missing operand") == Ok(10)
            && classify(b"xargs: echo: No such file or directory\n") == Ok(8)
            && classify(b"xargs: echo: No such file or directory") == Ok(8)
            && l.sample(3).map(|x| x.value) == Some(8),
        "static",
    );
    s.add(
        "FE6-精确命中",
        exact(b"dd: invalid option -- 'z'") == Ok(4)
            && exact(b"echo: invalid option -- 'z'") == Ok(1)
            && exact(b"tr: missing operand") == Ok(10)
            && l.sample(4).map(|x| x.value) == Some(4),
        "static",
    );
    s.add(
        "FE7-工具反查",
        tool_of(11) == Ok(TOOL_TR)
            && tool_of(2) == Ok(TOOL_ECHO)
            && tool_of(15) == Ok(TOOL_WC)
            && l.sample(5).map(|x| x.value) == Some(4),
        "static",
    );
    s.add(
        "FE8-层级反查与计数",
        tier_of(9) == Ok(TIER_FATAL)
            && tier_of(1) == Ok(TIER_USAGE)
            && count_tier(TIER_USAGE) == Ok(5)
            && l.sample(6).map(|x| x.value) == Some(3)
            && l.sample(7).map(|x| x.value) == Some(5),
        "static",
    );
    s.add(
        "FE9-维越界",
        msg_id(0, TIER_USAGE) == Err(FE_ETOOL)
            && msg_id(TOOL_WC + 1, TIER_USAGE) == Err(FE_ETOOL)
            && msg_id(TOOL_ECHO, 0) == Err(FE_ETIER)
            && msg_id(TOOL_ECHO, TIER_FATAL + 1) == Err(FE_ETIER)
            && count_tier(4) == Err(FE_ETIER)
            && l.sample(8).map(|x| x.code) == Some(FE_ETOOL)
            && l.sample(9).map(|x| x.code) == Some(FE_ETIER),
        "static",
    );
    s.add(
        "FE10-无匹配与短前缀",
        exact(b"cat: nope") == Err(FE_ENOMATCH)
            && classify(b"cat: nope") == Err(FE_ENOMATCH)
            && exact(b"tr: missing") == Err(FE_ESHORT)
            && exact(b"dd: failed to open") == Err(FE_ESHORT)
            && l.sample(10).map(|x| x.code) == Some(FE_ENOMATCH)
            && l.sample(11).map(|x| x.code) == Some(FE_ESHORT),
        "static"
    );
    s.add(
        "FE11-长尾与空串",
        exact(b"tr: missing operand plus") == Err(FE_EPART)
            && exact(b"wc: write error: Broken pipe!") == Err(FE_EPART)
            && exact(b"") == Err(FE_ENUL)
            && classify(b"") == Err(FE_ENOMATCH)
            && l.sample(12).map(|x| x.code) == Some(FE_EPART)
            && l.sample(13).map(|x| x.code) == Some(FE_ENUL),
        "static",
    );
    s.add(
        "FE12-精确与长尾互斥",
        exact(b"xargs: command terminated by signal 9") == Ok(9)
            && exact(b"xargs: command terminated by signal 9x") == Err(FE_EPART)
            && exact(b"xargs: command terminated by signal") == Err(FE_ESHORT)
            && classify(b"xargs: command terminated by signal 9x") == Ok(9),
        "static",
    );
    let mut f = build_ledger();
    let rep = f.replay();
    let root = match f.freeze() {
        Ok(v) => v,
        Err(_) => 0,
    };
    let again = f.freeze() == Err(FE_EFROZEN);
    let inj = inject_four();
    s.add_dyn(
        "FE13-冻结重放与四路",
        root != 0 && rep == Ok(root) && again && f.root() == root && inj == [true; 4],
        &format!("root={:016x} inj={:?}", root, inj),
    );
    s
}

/// 摘要。
pub fn summary(l: &Ledger) -> alloc::string::String {
    format!("rows={} ok={} err={}", l.len(), l.ok_count(), l.err_count())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn table_is_complete() {
        assert_eq!(TABLE.len(), MESSAGES);
        assert_eq!(count_tier(TIER_USAGE), Ok(5));
        assert_eq!(count_tier(TIER_RUNTIME), Ok(5));
        assert_eq!(count_tier(TIER_FATAL), Ok(5));
        assert_eq!(msg_id(TOOL_ECHO, TIER_USAGE), Ok(1));
        assert_eq!(msg_id(TOOL_WC, TIER_FATAL), Ok(15));
    }

    #[test]
    fn dimensions_are_checked() {
        assert_eq!(msg_id(0, TIER_USAGE), Err(FE_ETOOL));
        assert_eq!(msg_id(TOOL_ECHO, 0), Err(FE_ETIER));
        assert_eq!(msg(TOOL_DD, TIER_USAGE), Ok(&b"dd: invalid option -- 'z'"[..]));
    }

    #[test]
    fn classify_takes_longest_prefix() {
        assert_eq!(classify(b"tr: missing operand"), Ok(10));
        assert_eq!(classify(b"xargs: echo: No such file or directory\n"), Ok(8));
        assert_eq!(classify(b"cat: nope"), Err(FE_ENOMATCH));
    }

    #[test]
    fn exact_distinguishes_tails() {
        assert_eq!(exact(b"dd: invalid option -- 'z'"), Ok(4));
        assert_eq!(exact(b"tr: missing operand plus"), Err(FE_EPART));
        assert_eq!(exact(b"tr: missing"), Err(FE_ESHORT));
        assert_eq!(exact(b""), Err(FE_ENUL));
    }

    #[test]
    fn reverse_lookup() {
        assert_eq!(tool_of(11), Ok(TOOL_TR));
        assert_eq!(tier_of(9), Ok(TIER_FATAL));
        assert_eq!(tool_of(99), Err(FE_ENOMATCH));
    }

    #[test]
    fn ledger_and_checks() {
        let l = build_ledger();
        assert_eq!(l.len(), SAMPLES);
        assert_eq!(l.ok_count(), 8);
        assert!(l.audit().is_ok());
        let s = run_unx11495_checks();
        assert_eq!(s.len(), 13);
        assert!(s.all_passed(), "{}", s.red_items_text());
        assert_eq!(inject_four(), [true; 4]);
    }
}
