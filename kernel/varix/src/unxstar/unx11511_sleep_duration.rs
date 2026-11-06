//! UNX-F11511 sleep 时长文法（整数/后缀/小数）（0xf100 段）
//!
//! `sleep` 的时长不是"一个数字"，而是一条小文法：十进制整数、可选的小数
//! 部分、可选的一个单位后缀，最后换成一个纳秒整数。本卡把这条文法写成
//! 逐字节的状态机，并把"多参数求和"与"换算调度节拍"接在后面。三段闸门
//! 依次是：**词法**（符号/数字/小数点/后缀各在什么位置合法）、**范围**
//! （秒数、纳秒、求和都不得越界）、**节拍**（向上取整，绝不早醒）。
//!
//! 词法规则：首字符必须是数字，`+`/`-` 一律拒绝——负时长没有语义，正号
//! 也不接受（`ESIGN`）；小数点前必须有数字（`.5` 是 `EDIGIT`），小数点后
//! 必须有 1..=9 位数字（`1.` 与十位小数都是 `EFRAC`）；后缀只认 `s/m/h/d`
//! 四个小写字母，缺省是秒，多一个字符或换一个字母都是 `ESUFFIX`。小数点
//! 最多一个，因为出现第二个时后缀位会读到 `.`，同样落到 `ESUFFIX`。
//!
//! 范围规则：整数秒部分最多 10^10 秒（约 317 年），乘单位、乘 10^9、加
//! 小数余量都用 `checked_*`，任一步越界即 `ERANGE`。多操作数逐个解析后
//! 相加，和同样受 10^19 纳秒的总上限约束；没有操作数是 `EOPERAND`。
//! 空操作数是 `EEMPTY`，两者语义不同：前者是"没给参数"，后者是"给了
//! 一个空串"。
//!
//! 节拍规则：内核睡眠必须**不早于**请求时长醒来，所以 `ticks` 用向上
//! 取整：`(ns + tick - 1) / tick`。tick 为 0 是 `ETICK`，加法的溢出是
//! `ERANGE`。这条"宁可多睡一拍，不可早醒一纳秒"的规则是本卡唯一与
//! 调度器相关的地方，其余部分都是纯文法。
//!
//! 账本十四格：八格成功（纯整数、分钟、小时、带小数、纯小数、两段求和、
//! 节拍换算、整天），六格拒绝（空串、带符号、点前无数字、未知后缀、
//! 点后无数字、零节拍）。审计逐格复算，封印折叠全账，冻结后拒绝登记。

use crate::alloc::format;
use crate::checks::CheckSet;

/// 段基址。
pub const SEG_BASE: u32 = 0xf100;

/// 空操作数。
pub const SL_EEMPTY: u32 = SEG_BASE + 0x01;
/// 带符号的时长。
pub const SL_ESIGN: u32 = SEG_BASE + 0x02;
/// 缺少数字。
pub const SL_EDIGIT: u32 = SEG_BASE + 0x03;
/// 后缀不认识或有多余字符。
pub const SL_ESUFFIX: u32 = SEG_BASE + 0x04;
/// 小数部分非法。
pub const SL_EFRAC: u32 = SEG_BASE + 0x05;
/// 数值越界。
pub const SL_ERANGE: u32 = SEG_BASE + 0x06;
/// 没有任何操作数。
pub const SL_EOPERAND: u32 = SEG_BASE + 0x07;
/// 节拍长度为零。
pub const SL_ETICK: u32 = SEG_BASE + 0x08;
/// 账本形状不符。
pub const SL_EBAL: u32 = SEG_BASE + 0x09;
/// 槽位乱序。
pub const SL_ESLOT: u32 = SEG_BASE + 0x0a;
/// 观测值与复算不符。
pub const SL_EVALUE: u32 = SEG_BASE + 0x0b;
/// 冻结后重放不一致。
pub const SL_ESEAL: u32 = SEG_BASE + 0x0c;
/// 观察下标越界。
pub const SL_ENOBS: u32 = SEG_BASE + 0x0d;

/// 一秒的纳秒数。
pub const UNIT_NS: u64 = 1_000_000_000;
/// 整数秒上限。
pub const MAX_SECS: u64 = 10_000_000_000;
/// 纳秒总上限。
pub const MAX_NS: u64 = MAX_SECS * UNIT_NS;
/// 小数位数上限。
pub const FRAC_MAX: u32 = 9;
/// 账本格数。
pub const SAMPLES: usize = 14;

/// 操作码：解析一段。
pub const OP_PARSE: u8 = 1;
/// 操作码：多段求和。
pub const OP_SUM: u8 = 2;
/// 操作码：换算节拍。
pub const OP_TICKS: u8 = 3;

/// FNV 偏移基。
pub const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;
/// FNV 质数。
pub const FNV_PRIME: u64 = 0x100_0000_01b3;

/// 折入一个 64 位字。
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

/// 后缀 → 秒倍数。
fn suffix_mult(b: u8) -> Result<u64, u32> {
    match b {
        b's' => Ok(1),
        b'm' => Ok(60),
        b'h' => Ok(3600),
        b'd' => Ok(86_400),
        _ => Err(SL_ESUFFIX),
    }
}

/// 解析一段时长，返回纳秒。
pub fn parse(text: &[u8]) -> Result<u64, u32> {
    if text.is_empty() {
        return Err(SL_EEMPTY);
    }
    match text.get(0).copied() {
        Some(b'+') | Some(b'-') => return Err(SL_ESIGN),
        Some(b) if b.is_ascii_digit() => {}
        Some(_) => return Err(SL_EDIGIT),
        None => return Err(SL_EEMPTY),
    }
    let mut i = 0usize;
    let mut secs = 0u64;
    while let Some(b) = text.get(i).copied() {
        if !b.is_ascii_digit() {
            break;
        }
        secs = secs.checked_mul(10).ok_or(SL_ERANGE)?;
        secs = secs.checked_add((b - b'0') as u64).ok_or(SL_ERANGE)?;
        if secs > MAX_SECS {
            return Err(SL_ERANGE);
        }
        i += 1;
    }
    let mut frac = 0u64;
    if text.get(i).copied() == Some(b'.') {
        i += 1;
        let mut fd = 0u32;
        let mut scale = 100_000_000u64;
        while let Some(b) = text.get(i).copied() {
            if !b.is_ascii_digit() {
                break;
            }
            if fd >= FRAC_MAX {
                return Err(SL_EFRAC);
            }
            frac += (b - b'0') as u64 * scale;
            scale /= 10;
            fd += 1;
            i += 1;
        }
        if fd == 0 {
            return Err(SL_EFRAC);
        }
    }
    let mult = match text.get(i).copied() {
        Some(b) => {
            i += 1;
            suffix_mult(b)?
        }
        None => 1,
    };
    if i < text.len() {
        return Err(SL_ESUFFIX);
    }
    let whole = secs.checked_mul(mult).ok_or(SL_ERANGE)?;
    if whole > MAX_SECS {
        return Err(SL_ERANGE);
    }
    let base = whole.checked_mul(UNIT_NS).ok_or(SL_ERANGE)?;
    let extra = frac.checked_mul(mult).ok_or(SL_ERANGE)?;
    let total = base.checked_add(extra).ok_or(SL_ERANGE)?;
    if total > MAX_NS {
        return Err(SL_ERANGE);
    }
    Ok(total)
}

/// 多段时长求和。
pub fn parse_all(args: &[&[u8]]) -> Result<u64, u32> {
    if args.is_empty() {
        return Err(SL_EOPERAND);
    }
    let mut total = 0u64;
    let mut i = 0usize;
    while i < args.len() {
        let a: &[u8] = match args.get(i) {
            Some(v) => *v,
            None => break,
        };
        let d = parse(a)?;
        total = total.checked_add(d).ok_or(SL_ERANGE)?;
        if total > MAX_NS {
            return Err(SL_ERANGE);
        }
        i += 1;
    }
    Ok(total)
}

/// 纳秒换算成节拍，向上取整。
pub fn ticks(ns: u64, tick_ns: u64) -> Result<u64, u32> {
    if tick_ns == 0 {
        return Err(SL_ETICK);
    }
    if ns == 0 {
        return Ok(0);
    }
    let bias = match ns.checked_add(tick_ns - 1) {
        Some(v) => v,
        None => return Err(SL_ERANGE),
    };
    Ok(bias / tick_ns)
}

/// 纳秒 → 毫秒（账本观测用）。
fn as_ms(ns: u64) -> u32 {
    (ns / 1_000_000) as u32
}

/// 一个账本样本。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Sample {
    /// 槽位。
    pub slot: u8,
    /// 操作码。
    pub op: u8,
    /// 是否成功。
    pub ok: bool,
    /// 错误码。
    pub code: u32,
    /// 观测值。
    pub value: u32,
}

impl Sample {
    /// 空白格。
    pub const fn blank() -> Sample {
        Sample { slot: 0, op: 0, ok: false, code: 0, value: 0 }
    }
}

/// `Result<u32, u32>` → 样本。
fn word_cell(slot: u8, op: u8, r: Result<u32, u32>) -> Sample {
    match r {
        Ok(v) => Sample { slot, op, ok: true, code: 0, value: v },
        Err(c) => Sample { slot, op, ok: false, code: c, value: 0 },
    }
}

/// 样本账本。
#[derive(Clone, Copy, Debug)]
pub struct Ledger {
    rows: [Sample; SAMPLES],
    len: usize,
    root: u64,
    frozen: bool,
    rejects: u32,
}

impl Ledger {
    /// 空账本。
    pub const fn new() -> Ledger {
        Ledger { rows: [Sample::blank(); SAMPLES], len: 0, root: 0, frozen: false, rejects: 0 }
    }

    /// 登记一格。
    pub fn register(&mut self, s: Sample) -> Result<(), u32> {
        if self.frozen || (s.slot as usize) != self.len || (s.slot as usize) >= SAMPLES {
            self.rejects += 1;
            return Err(SL_ESLOT);
        }
        if let Some(dst) = self.rows.get_mut(self.len) {
            *dst = s;
        }
        self.len += 1;
        Ok(())
    }

    /// 格数。
    pub fn len(&self) -> usize {
        self.len
    }

    /// 是否空账。
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// 第 i 格。
    pub fn sample(&self, i: usize) -> Option<Sample> {
        if i >= self.len {
            return None;
        }
        self.rows.get(i).copied()
    }

    /// 成功格数。
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

    /// 失败格数。
    pub fn err_count(&self) -> usize {
        self.len - self.ok_count()
    }

    /// 对账。
    pub fn audit(&self) -> Result<(), u32> {
        if self.len != SAMPLES || self.ok_count() != 8 || self.err_count() != 6 {
            return Err(SL_EBAL);
        }
        const VALS: [u32; 8] = [5000, 120_000, 3_600_000, 1500, 250, 5000, 150, 86_400_000];
        const CODES: [u32; 6] =
            [SL_EEMPTY, SL_ESIGN, SL_EDIGIT, SL_ESUFFIX, SL_EFRAC, SL_ETICK];
        let mut vi = 0usize;
        let mut ci = 0usize;
        let mut i = 0usize;
        while i < self.len {
            let s = match self.rows.get(i) {
                Some(v) => *v,
                None => return Err(SL_EBAL),
            };
            if s.slot as usize != i {
                return Err(SL_ESLOT);
            }
            if s.op == 0 {
                return Err(SL_EVALUE);
            }
            if s.ok {
                if s.code != 0 {
                    return Err(SL_EVALUE);
                }
                match VALS.get(vi) {
                    Some(w) => {
                        if s.value != *w {
                            return Err(SL_EVALUE);
                        }
                    }
                    None => return Err(SL_EBAL),
                }
                vi += 1;
            } else {
                if s.value != 0 {
                    return Err(SL_EVALUE);
                }
                match CODES.get(ci) {
                    Some(w) => {
                        if s.code != *w {
                            return Err(SL_EVALUE);
                        }
                    }
                    None => return Err(SL_EBAL),
                }
                ci += 1;
            }
            i += 1;
        }
        if vi != 8 || ci != 6 {
            return Err(SL_EBAL);
        }
        Ok(())
    }

    /// 指纹。
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
        fold(h, self.rejects as u64)
    }

    /// 冻结。
    pub fn freeze(&mut self) -> Result<u64, u32> {
        if self.frozen {
            self.rejects += 1;
            return Err(SL_EBAL);
        }
        self.root = self.seal();
        self.frozen = true;
        Ok(self.root)
    }

    /// 重放。
    pub fn replay(&self) -> Result<u64, u32> {
        let fresh = build_ledger();
        if fresh.seal() == self.seal() {
            Ok(fresh.seal())
        } else {
            Err(SL_ESEAL)
        }
    }

    /// 冻结指纹。
    pub fn root(&self) -> u64 {
        self.root
    }

    /// 是否冻结。
    pub fn is_frozen(&self) -> bool {
        self.frozen
    }

    /// 拒绝计数。
    pub fn rejects(&self) -> u32 {
        self.rejects
    }
}

/// 十四格账本：八个成功观测 + 六类拒绝。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();

    let _ = l.register(word_cell(0, OP_PARSE, parse(b"5").map(as_ms)));
    let _ = l.register(word_cell(1, OP_PARSE, parse(b"2m").map(as_ms)));
    let _ = l.register(word_cell(2, OP_PARSE, parse(b"1h").map(as_ms)));
    let _ = l.register(word_cell(3, OP_PARSE, parse(b"1.5s").map(as_ms)));
    let _ = l.register(word_cell(4, OP_PARSE, parse(b"0.25").map(as_ms)));
    let _ = l.register(word_cell(5, OP_SUM, parse_all(&[b"2", b"3"]).map(as_ms)));
    let _ = l.register(word_cell(
        6,
        OP_TICKS,
        ticks(1_500_000_000, 10_000_000).map(|v| v as u32),
    ));
    let _ = l.register(word_cell(7, OP_PARSE, parse(b"1d").map(as_ms)));

    let _ = l.register(word_cell(8, OP_PARSE, parse(b"").map(as_ms)));
    let _ = l.register(word_cell(9, OP_PARSE, parse(b"-1").map(as_ms)));
    let _ = l.register(word_cell(10, OP_PARSE, parse(b".5").map(as_ms)));
    let _ = l.register(word_cell(11, OP_PARSE, parse(b"5x").map(as_ms)));
    let _ = l.register(word_cell(12, OP_PARSE, parse(b"1.").map(as_ms)));
    let _ = l.register(word_cell(13, OP_TICKS, ticks(1, 0).map(|v| v as u32)));

    l
}

/// 四路注入。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];

    let l = build_ledger();
    let rep = l.replay();
    let root = l.seal();
    out[0] = l.len() == SAMPLES && l.audit().is_ok() && rep == Ok(root) && root != 0;

    // 一路：后缀换算。
    out[1] = parse(b"5") == Ok(5 * UNIT_NS)
        && parse(b"5s") == Ok(5 * UNIT_NS)
        && parse(b"2m") == Ok(120 * UNIT_NS)
        && parse(b"1h") == Ok(3600 * UNIT_NS)
        && parse(b"1d") == Ok(86_400 * UNIT_NS)
        && parse(b"0") == Ok(0);

    // 二路：小数精度与求和。
    out[2] = parse(b"1.5") == Ok(1_500_000_000)
        && parse(b"0.25") == Ok(250_000_000)
        && parse(b"0.000000001") == Ok(1)
        && parse(b"1.000000001") == Ok(1_000_000_001)
        && parse_all(&[b"1", b"2"]) == Ok(3 * UNIT_NS)
        && parse_all(&[b"1.5", b"0.5"]) == Ok(2 * UNIT_NS)
        && parse_all(&[b"1m", b"60"]) == parse(b"2m");

    // 三路：节拍向上取整与各类拒绝。
    out[3] = ticks(0, 10) == Ok(0)
        && ticks(1, 10) == Ok(1)
        && ticks(10, 10) == Ok(1)
        && ticks(11, 10) == Ok(2)
        && parse(b"") == Err(SL_EEMPTY)
        && parse(b"-1") == Err(SL_ESIGN)
        && parse(b"+1") == Err(SL_ESIGN)
        && parse(b".5") == Err(SL_EDIGIT)
        && parse(b"5x") == Err(SL_ESUFFIX)
        && parse(b"1.") == Err(SL_EFRAC)
        && parse(b"1.1234567890") == Err(SL_EFRAC)
        && parse(b"10000000001") == Err(SL_ERANGE)
        && parse_all(&[]) == Err(SL_EOPERAND)
        && ticks(1, 0) == Err(SL_ETICK);

    out
}

/// 十三项判据。
pub fn run_unx11511_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11511.sleep_duration");
    let l = build_ledger();

    s.add_dyn(
        "SL01-十四格八允六拒",
        l.len() == SAMPLES && l.ok_count() == 8 && l.err_count() == 6 && l.audit().is_ok(),
        &format!("n={} ok={} err={}", l.len(), l.ok_count(), l.err_count()),
    );

    s.add(
        "SL02-整数秒",
        parse(b"5") == Ok(5 * UNIT_NS)
            && parse(b"0") == Ok(0)
            && parse(b"007") == Ok(7 * UNIT_NS)
            && parse(b"10") == Ok(10 * UNIT_NS)
            && l.sample(0).map(|x| x.value) == Some(5000),
        "static",
    );

    s.add(
        "SL03-后缀族",
        parse(b"1s") == Ok(UNIT_NS)
            && parse(b"1m") == Ok(60 * UNIT_NS)
            && parse(b"1h") == Ok(3600 * UNIT_NS)
            && parse(b"1d") == Ok(86_400 * UNIT_NS)
            && parse(b"2m") == parse(b"120")
            && parse(b"1h") == parse(b"60m")
            && parse(b"1d") == parse(b"24h")
            && l.sample(1).map(|x| x.value) == Some(120_000)
            && l.sample(2).map(|x| x.value) == Some(3_600_000)
            && l.sample(7).map(|x| x.value) == Some(86_400_000),
        "static",
    );

    s.add(
        "SL04-小数解析",
        parse(b"1.5") == Ok(1_500_000_000)
            && parse(b"1.5s") == parse(b"1.5")
            && parse(b"0.25") == Ok(250_000_000)
            && parse(b"0.001") == Ok(1_000_000)
            && parse(b"1.000000001") == Ok(1_000_000_001)
            && parse(b"0.000000001") == Ok(1)
            && l.sample(3).map(|x| x.value) == Some(1500)
            && l.sample(4).map(|x| x.value) == Some(250),
        "static",
    );

    s.add(
        "SL05-小数位闸门",
        parse(b".5") == Err(SL_EDIGIT)
            && parse(b"1.") == Err(SL_EFRAC)
            && parse(b"1.123456789") == Ok(1_123_456_789)
            && parse(b"1.1234567890") == Err(SL_EFRAC)
            && parse(b"0.0000000001") == Err(SL_EFRAC)
            && l.sample(12).map(|x| x.code) == Some(SL_EFRAC),
        "static",
    );

    s.add(
        "SL06-多段求和",
        parse_all(&[b"1", b"2"]) == Ok(3 * UNIT_NS)
            && parse_all(&[b"1.5", b"0.5"]) == Ok(2 * UNIT_NS)
            && parse_all(&[b"1m", b"60"]) == parse(b"2m")
            && parse_all(&[b"7"]) == parse(b"7")
            && parse_all(&[]) == Err(SL_EOPERAND)
            && l.sample(5).map(|x| x.value) == Some(5000),
        "static",
    );

    s.add(
        "SL07-求和上限",
        parse_all(&[b"10000000000", b"1"]) == Err(SL_ERANGE)
            && parse_all(&[b"10000000000"]) == Ok(MAX_NS)
            && parse_all(&[b"5000000000", b"5000000000"]) == Ok(MAX_NS)
            && parse_all(&[b"5000000000", b"5000000000", b"1"]) == Err(SL_ERANGE),
        "static",
    );

    s.add(
        "SL08-数值上限",
        parse(b"10000000000") == Ok(MAX_NS)
            && parse(b"10000000001") == Err(SL_ERANGE)
            && parse(b"999999999999999999999") == Err(SL_ERANGE)
            && parse(b"18446744074") == Err(SL_ERANGE)
            && l.sample(11).map(|x| x.code) == Some(SL_ESUFFIX),
        "static",
    );

    s.add(
        "SL09-符号与首字符",
        parse(b"") == Err(SL_EEMPTY)
            && parse(b"-1") == Err(SL_ESIGN)
            && parse(b"+1") == Err(SL_ESIGN)
            && parse(b"abc") == Err(SL_EDIGIT)
            && parse(b"x5") == Err(SL_EDIGIT)
            && parse(b" 5") == Err(SL_EDIGIT)
            && l.sample(8).map(|x| x.code) == Some(SL_EEMPTY)
            && l.sample(9).map(|x| x.code) == Some(SL_ESIGN)
            && l.sample(10).map(|x| x.code) == Some(SL_EDIGIT),
        "static",
    );

    s.add(
        "SL10-后缀闸门",
        parse(b"5x") == Err(SL_ESUFFIX)
            && parse(b"5S") == Err(SL_ESUFFIX)
            && parse(b"5ss") == Err(SL_ESUFFIX)
            && parse(b"5m5") == Err(SL_ESUFFIX)
            && parse(b"5 ") == Err(SL_ESUFFIX)
            && parse(b"5.5.5") == Err(SL_ESUFFIX)
            && l.sample(11).map(|x| x.code) == Some(SL_ESUFFIX),
        "static",
    );

    s.add(
        "SL11-节拍向上取整",
        ticks(0, 10) == Ok(0)
            && ticks(1, 10) == Ok(1)
            && ticks(9, 10) == Ok(1)
            && ticks(10, 10) == Ok(1)
            && ticks(11, 10) == Ok(2)
            && ticks(15, 10) == Ok(2)
            && ticks(1_500_000_000, 10_000_000) == Ok(150)
            && ticks(1, 0) == Err(SL_ETICK)
            && l.sample(6).map(|x| x.value) == Some(150)
            && l.sample(13).map(|x| x.code) == Some(SL_ETICK),
        "static",
    );

    s.add(
        "SL12-等价写法",
        parse(b"1.5s") == parse(b"1.5")
            && parse(b"60s") == parse(b"1m")
            && parse(b"1440m") == parse(b"1d")
            && parse(b"1d") == parse(b"24h")
            && parse(b"0.5m") == Ok(30 * UNIT_NS)
            && parse_all(&[b"0.5m", b"0.5m"]) == parse(b"1m"),
        "static",
    );

    s.add(
        "SL13-冻结重放四注入",
        {
            let mut f = build_ledger();
            let rep = f.replay();
            let one = f.freeze();
            let two = f.freeze();
            let inj = inject_four();
            one.is_ok()
                && one == rep
                && two.is_err()
                && f.is_frozen()
                && f.root() != 0
                && f.rejects() == 1
                && inj == [true; 4]
        },
        "static",
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ledger_shape() {
        let l = build_ledger();
        assert_eq!(l.len(), SAMPLES);
        assert_eq!(l.ok_count(), 8);
        assert_eq!(l.err_count(), 6);
        assert_eq!(l.rejects(), 0);
        assert!(l.audit().is_ok());
        let mut i = 0usize;
        while i < l.len() {
            assert_eq!(l.sample(i).map(|x| x.slot as usize), Some(i));
            i += 1;
        }
    }

    #[test]
    fn checks_pass() {
        let s = run_unx11511_checks();
        assert_eq!(s.len(), 13);
        assert!(s.all_passed());
    }

    #[test]
    fn whole_seconds() {
        assert_eq!(parse(b"5"), Ok(5 * UNIT_NS));
        assert_eq!(parse(b"0"), Ok(0));
        assert_eq!(parse(b"007"), Ok(7 * UNIT_NS));
        assert_eq!(parse(b"1s"), Ok(UNIT_NS));
    }

    #[test]
    fn suffixes() {
        assert_eq!(parse(b"1m"), Ok(60 * UNIT_NS));
        assert_eq!(parse(b"1h"), Ok(3600 * UNIT_NS));
        assert_eq!(parse(b"1d"), Ok(86_400 * UNIT_NS));
        assert_eq!(parse(b"1d"), parse(b"24h"));
        assert_eq!(parse(b"60s"), parse(b"1m"));
    }

    #[test]
    fn fractions() {
        assert_eq!(parse(b"1.5"), Ok(1_500_000_000));
        assert_eq!(parse(b"0.25"), Ok(250_000_000));
        assert_eq!(parse(b"0.000000001"), Ok(1));
        assert_eq!(parse(b".5"), Err(SL_EDIGIT));
        assert_eq!(parse(b"1."), Err(SL_EFRAC));
        assert_eq!(parse(b"1.1234567890"), Err(SL_EFRAC));
    }

    #[test]
    fn sums() {
        assert_eq!(parse_all(&[b"1", b"2"]), Ok(3 * UNIT_NS));
        assert_eq!(parse_all(&[b"1.5", b"0.5"]), Ok(2 * UNIT_NS));
        assert_eq!(parse_all(&[]), Err(SL_EOPERAND));
        assert_eq!(parse_all(&[b"10000000000", b"1"]), Err(SL_ERANGE));
    }

    #[test]
    fn tick_rounding() {
        assert_eq!(ticks(0, 10), Ok(0));
        assert_eq!(ticks(1, 10), Ok(1));
        assert_eq!(ticks(10, 10), Ok(1));
        assert_eq!(ticks(11, 10), Ok(2));
        assert_eq!(ticks(1, 0), Err(SL_ETICK));
    }

    #[test]
    fn grammar_gates() {
        assert_eq!(parse(b""), Err(SL_EEMPTY));
        assert_eq!(parse(b"-1"), Err(SL_ESIGN));
        assert_eq!(parse(b"abc"), Err(SL_EDIGIT));
        assert_eq!(parse(b"5x"), Err(SL_ESUFFIX));
        assert_eq!(parse(b"10000000001"), Err(SL_ERANGE));
        assert_eq!(parse_all(&[]), Err(SL_EOPERAND));
    }

    #[test]
    fn inject_four_holds() {
        assert_eq!(inject_four(), [true; 4]);
    }
}
