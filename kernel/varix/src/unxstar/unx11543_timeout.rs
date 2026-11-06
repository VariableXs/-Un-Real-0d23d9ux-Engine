//! UNX-F11543 timeout 限时执行
//!
//! `timeout` 的限时模型：把「时长解析 → 信号选择 → 命令装载 → 到点发信 →
//! 宽限二段杀 → 退出码」这一整条链拆成可判定的纯数据变换，不碰真实时钟、
//! 不碰真实进程。时长解析接受整数、小数与 `s/m/h/d` 后缀，裸整数按秒；
//! 时长为 0 表示不设限。到点先发配置信号（缺省 TERM），子进程若忽略且
//! 给了 `-k`，宽限期后补一发 KILL；退出码按 124（超时）/137（被杀）/
//! 128+sig（`--preserve-status`）三档收敛。
//!
//! 闸门次序：字节 → 时长 → 信号 → 选项 → 命令 → 计划 → 观测 → 账本。
//!
//! 本模块自成一体：无 std、无 unsafe、无 panic；所有失败走 13 个错误码，
//! 账本 14 格（8 成 6 败）在 `audit` 里逐格对账，冻结后重放必须同指纹。

use crate::alloc::string::String;
use crate::checks::CheckSet;

/// 段基址。
pub const SEG_BASE: u32 = 0xf300;

/// 账本格数。
pub const SAMPLES: usize = 14;

/// 命令容量。
pub const CMD_CAP: usize = 4;

/// 名称容量。
pub const NAME_CAP: usize = 48;

/// 时长文本容量。
pub const DUR_TEXT_CAP: usize = 32;

/// 正常退出。
pub const EXIT_OK: u32 = 0;

/// 超时退出。
pub const EXIT_TIMEOUT: u32 = 124;

/// `timeout` 自身失败。
pub const EXIT_FAILURE: u32 = 125;

/// 命令不可执行。
pub const EXIT_CANNOT: u32 = 126;

/// 被 KILL 杀死。
pub const EXIT_KILLED: u32 = 137;

/// 信号退出码基址。
pub const EXIT_SIGNAL_BASE: u32 = 128;

// ---------------------------------------------------------------------------
// 错误码
// ---------------------------------------------------------------------------

/// 时长非法。
pub const TO_EDUR: u32 = SEG_BASE + 0x01;
/// 命令装载失败。
pub const TO_EEXEC: u32 = SEG_BASE + 0x02;
/// 信号名非法。
pub const TO_ESIG: u32 = SEG_BASE + 0x03;
/// 选项非法。
pub const TO_EOPT: u32 = SEG_BASE + 0x04;
/// 超长。
pub const TO_ELEN: u32 = SEG_BASE + 0x05;
/// 字节非法。
pub const TO_EBYTE: u32 = SEG_BASE + 0x06;
/// 状态冲突。
pub const TO_ESTATE: u32 = SEG_BASE + 0x07;
/// 数值越界。
pub const TO_ERANGE: u32 = SEG_BASE + 0x08;
/// 账本不平。
pub const TO_EBAL: u32 = SEG_BASE + 0x09;
/// 槽位非法。
pub const TO_ESLOT: u32 = SEG_BASE + 0x0a;
/// 观测值不符。
pub const TO_EVALUE: u32 = SEG_BASE + 0x0b;
/// 指纹不符。
pub const TO_ESEAL: u32 = SEG_BASE + 0x0c;
/// 无观测。
pub const TO_ENOBS: u32 = SEG_BASE + 0x0d;

// ---------------------------------------------------------------------------
// 操作码
// ---------------------------------------------------------------------------

/// 时长解析。
pub const OP_DUR: u8 = 0x01;
/// 信号解析。
pub const OP_SIG: u8 = 0x02;
/// 选项解析。
pub const OP_OPT: u8 = 0x03;
/// 命令装载。
pub const OP_CMD: u8 = 0x04;
/// 限时运行。
pub const OP_RUN: u8 = 0x05;
/// 账本观测。
pub const OP_OBS: u8 = 0x06;

// ---------------------------------------------------------------------------
// 哈希
// ---------------------------------------------------------------------------

/// FNV 偏移基。
const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;
/// FNV 素数。
const FNV_PRIME: u64 = 0x100_0000_01b3;

/// 折入一个字节。
fn fold_byte(h: u64, b: u8) -> u64 {
    let mut v = h ^ (b as u64);
    let mut k = 0usize;
    while k < 8 {
        v = v.wrapping_mul(FNV_PRIME);
        k += 1;
    }
    v
}

/// 折入一个字。
fn fold(h: u64, w: u64) -> u64 {
    let mut v = h;
    let mut sh = 0u32;
    while sh < 64 {
        let b = ((w >> sh) & 0xff) as u8;
        v = fold_byte(v, b);
        sh += 8;
    }
    v
}

// ---------------------------------------------------------------------------
// 字节助手
// ---------------------------------------------------------------------------

/// 是否可打印：0x21..=0x7e。
fn printable(s: &[u8]) -> bool {
    let mut i = 0usize;
    while i < s.len() {
        if let Some(b) = s.get(i) {
            if *b < 0x21 || *b > 0x7e {
                return false;
            }
        }
        i += 1;
    }
    true
}

/// 逐字节相等。
fn eq_bytes(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut i = 0usize;
    while i < a.len() {
        match (a.get(i), b.get(i)) {
            (Some(x), Some(y)) => {
                if x != y {
                    return false;
                }
            }
            _ => return false,
        }
        i += 1;
    }
    true
}

/// 前缀。
fn starts_with(a: &[u8], p: &[u8]) -> bool {
    match a.get(0..p.len()) {
        Some(v) => eq_bytes(v, p),
        None => false,
    }
}

/// 大小写不敏感比较（右侧给大写）。
fn eq_upper(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut i = 0usize;
    while i < a.len() {
        match (a.get(i), b.get(i)) {
            (Some(x), Some(y)) => {
                let u = if *x >= b'a' && *x <= b'z' { *x - 32 } else { *x };
                if u != *y {
                    return false;
                }
            }
            _ => return false,
        }
        i += 1;
    }
    true
}

// ---------------------------------------------------------------------------
// 时长
// ---------------------------------------------------------------------------

/// 毫秒时长。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Duration {
    ms: u64,
}

impl Duration {
    /// 零时长（不设限）。
    pub const ZERO: Duration = Duration { ms: 0 };

    /// 从毫秒构造。
    pub const fn from_ms(ms: u64) -> Duration {
        Duration { ms }
    }

    /// 毫秒数。
    pub const fn ms(&self) -> u64 {
        self.ms
    }

    /// 是否为零。
    pub fn is_zero(&self) -> bool {
        self.ms == 0
    }

    /// 相加（溢出返回 `None`）。
    pub fn add(&self, other: Duration) -> Option<Duration> {
        match self.ms.checked_add(other.ms) {
            Some(v) => Some(Duration { ms: v }),
            None => None,
        }
    }

    /// 文本：`<ms>ms`。
    pub fn text(&self) -> String {
        let mut s = String::new();
        push_u64(&mut s, self.ms);
        s.push_str("ms");
        s
    }
}

/// 推入无符号十进制。
fn push_u64(s: &mut String, mut v: u64) {
    if v == 0 {
        s.push('0');
        return;
    }
    let mut buf = [0u8; 20];
    let mut at = buf.len();
    while v > 0 {
        let d = (v % 10) as u8;
        v /= 10;
        at -= 1;
        if let Some(dst) = buf.get_mut(at) {
            *dst = b'0' + d;
        }
    }
    let mut i = at;
    while i < buf.len() {
        if let Some(b) = buf.get(i) {
            s.push(*b as char);
        }
        i += 1;
    }
}

/// 后缀倍率。
fn suffix_scale(suffix: &[u8]) -> Result<u64, u32> {
    match suffix {
        [] => Ok(1000),
        [b's'] => Ok(1000),
        [b'm'] => Ok(60_000),
        [b'h'] => Ok(3_600_000),
        [b'd'] => Ok(86_400_000),
        _ => Err(TO_EDUR),
    }
}

/// 解析时长。裸整数按秒；`1.5m`、`250ms` 这类小数按后缀缩放，小数最多三位。
pub fn parse_duration(s: &[u8]) -> Result<Duration, u32> {
    if s.is_empty() {
        return Err(TO_EDUR);
    }
    if s.len() > DUR_TEXT_CAP {
        return Err(TO_ELEN);
    }
    if !printable(s) {
        return Err(TO_EBYTE);
    }
    let mut end = s.len();
    while end > 0 {
        let b = match s.get(end - 1) {
            Some(v) => *v,
            None => break,
        };
        if b.is_ascii_alphabetic() {
            end -= 1;
        } else {
            break;
        }
    }
    if s.len() - end > 1 {
        return Err(TO_EDUR);
    }
    let num = match s.get(0..end) {
        Some(v) => v,
        None => return Err(TO_EDUR),
    };
    if num.is_empty() {
        return Err(TO_EDUR);
    }
    let suffix = match s.get(end..) {
        Some(v) => v,
        None => return Err(TO_EDUR),
    };
    let scale = suffix_scale(suffix)?;
    let mut int: u64 = 0;
    let mut frac: u64 = 0;
    let mut frac_digits: u32 = 0;
    let mut seen_dot = false;
    let mut i = 0usize;
    while i < num.len() {
        let b = match num.get(i) {
            Some(v) => *v,
            None => return Err(TO_EDUR),
        };
        if b == b'.' {
            if seen_dot {
                return Err(TO_EDUR);
            }
            seen_dot = true;
        } else if b.is_ascii_digit() {
            let d = (b - b'0') as u64;
            if seen_dot {
                if frac_digits >= 3 {
                    return Err(TO_EDUR);
                }
                frac = match frac.checked_mul(10).and_then(|v| v.checked_add(d)) {
                    Some(v) => v,
                    None => return Err(TO_ERANGE),
                };
                frac_digits += 1;
            } else {
                int = match int.checked_mul(10).and_then(|v| v.checked_add(d)) {
                    Some(v) => v,
                    None => return Err(TO_ERANGE),
                };
            }
        } else {
            return Err(TO_EDUR);
        }
        i += 1;
    }
    if seen_dot && frac_digits == 0 {
        return Err(TO_EDUR);
    }
    let mut ms = match int.checked_mul(scale) {
        Some(v) => v,
        None => return Err(TO_ERANGE),
    };
    let mut div = 1u64;
    let mut k = 0u32;
    while k < frac_digits {
        div = div.saturating_mul(10);
        k += 1;
    }
    let scaled = match frac.checked_mul(scale) {
        Some(v) => v,
        None => return Err(TO_ERANGE),
    };
    ms = match ms.checked_add(scaled / div) {
        Some(v) => v,
        None => return Err(TO_ERANGE),
    };
    Ok(Duration { ms })
}

// ---------------------------------------------------------------------------
// 信号
// ---------------------------------------------------------------------------

/// 可发信号子集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Sig {
    /// 挂断。
    Hup,
    /// 中断。
    Int,
    /// 退出。
    Quit,
    /// 终止。
    Term,
    /// 强制杀。
    Kill,
    /// 用户一。
    Usr1,
}

impl Sig {
    /// 全表。
    pub fn all() -> [Sig; 6] {
        [Sig::Hup, Sig::Int, Sig::Quit, Sig::Term, Sig::Kill, Sig::Usr1]
    }

    /// 名字。
    pub fn text(&self) -> &'static str {
        match self {
            Sig::Hup => "HUP",
            Sig::Int => "INT",
            Sig::Quit => "QUIT",
            Sig::Term => "TERM",
            Sig::Kill => "KILL",
            Sig::Usr1 => "USR1",
        }
    }

    /// 编号。
    pub fn number(&self) -> u32 {
        match self {
            Sig::Hup => 1,
            Sig::Int => 2,
            Sig::Quit => 3,
            Sig::Kill => 9,
            Sig::Usr1 => 10,
            Sig::Term => 15,
        }
    }

    /// 是否不可捕获。
    pub fn uncatchable(&self) -> bool {
        match self {
            Sig::Kill => true,
            _ => false,
        }
    }

    /// 解析名字或编号（大小写不敏感）。
    pub fn parse(s: &[u8]) -> Result<Sig, u32> {
        if s.is_empty() || s.len() > 16 {
            return Err(TO_ESIG);
        }
        if !printable(s) {
            return Err(TO_EBYTE);
        }
        let table = Sig::all();
        let mut i = 0usize;
        while i < table.len() {
            if let Some(sig) = table.get(i) {
                if eq_upper(s, sig.text().as_bytes()) {
                    return Ok(*sig);
                }
            }
            i += 1;
        }
        let mut n: u32 = 0;
        let mut j = 0usize;
        let mut all_digits = !s.is_empty();
        while j < s.len() {
            match s.get(j) {
                Some(b) => {
                    if !b.is_ascii_digit() {
                        all_digits = false;
                        break;
                    }
                    n = match n.checked_mul(10).and_then(|v| v.checked_add((*b - b'0') as u32)) {
                        Some(v) => v,
                        None => return Err(TO_ESIG),
                    };
                }
                None => {
                    all_digits = false;
                    break;
                }
            }
            j += 1;
        }
        if !all_digits {
            return Err(TO_ESIG);
        }
        let mut k = 0usize;
        while k < table.len() {
            if let Some(sig) = table.get(k) {
                if sig.number() == n {
                    return Ok(*sig);
                }
            }
            k += 1;
        }
        Err(TO_ESIG)
    }
}

// ---------------------------------------------------------------------------
// 宽限与配置
// ---------------------------------------------------------------------------

/// `-k` 宽限。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct KillAfter {
    enabled: bool,
    delay: Duration,
}

impl KillAfter {
    /// 关闭。
    pub const fn off() -> KillAfter {
        KillAfter { enabled: false, delay: Duration::ZERO }
    }

    /// 打开。
    pub const fn on(delay: Duration) -> KillAfter {
        KillAfter { enabled: true, delay }
    }

    /// 是否打开。
    pub fn enabled(&self) -> bool {
        self.enabled
    }

    /// 宽限时长。
    pub fn delay(&self) -> Duration {
        self.delay
    }

    /// 文本。
    pub fn text(&self) -> String {
        if self.enabled {
            self.delay.text()
        } else {
            let mut s = String::new();
            s.push_str("off");
            s
        }
    }
}

/// 限时配置。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Config {
    duration: Duration,
    kill_after: KillAfter,
    sig: Sig,
    preserve: bool,
    foreground: bool,
}

impl Config {
    /// 缺省：不设限、TERM、不保留状态、后台。
    pub const fn new() -> Config {
        Config {
            duration: Duration::ZERO,
            kill_after: KillAfter::off(),
            sig: Sig::Term,
            preserve: false,
            foreground: false,
        }
    }

    /// 时长。
    pub fn duration(&self) -> Duration {
        self.duration
    }

    /// 宽限。
    pub fn kill_after(&self) -> KillAfter {
        self.kill_after
    }

    /// 首信号。
    pub fn signal(&self) -> Sig {
        self.sig
    }

    /// 是否保留子进程状态。
    pub fn preserve(&self) -> bool {
        self.preserve
    }

    /// 是否前台。
    pub fn foreground(&self) -> bool {
        self.foreground
    }

    /// 是否设限。
    pub fn limited(&self) -> bool {
        !self.duration.is_zero()
    }

    /// 渲染四行：时长、信号、宽限、模式。
    pub fn text(&self) -> String {
        let mut s = String::new();
        s.push_str("limit ");
        s.push_str(self.duration.text().as_str());
        s.push('\n');
        s.push_str("signal ");
        s.push_str(self.sig.text());
        s.push('\n');
        s.push_str("kill-after ");
        s.push_str(self.kill_after.text().as_str());
        s.push('\n');
        s.push_str("mode ");
        if self.foreground {
            s.push_str("foreground");
        } else {
            s.push_str("background");
        }
        s.push('\n');
        s
    }
}

// ---------------------------------------------------------------------------
// 命令名
// ---------------------------------------------------------------------------

/// 命令名或路径。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Name {
    bytes: [u8; NAME_CAP],
    len: usize,
}

impl Name {
    /// 空白。
    pub const fn blank() -> Name {
        Name { bytes: [0u8; NAME_CAP], len: 0 }
    }

    /// 构造。
    pub fn new(s: &[u8]) -> Result<Name, u32> {
        if s.is_empty() {
            return Err(TO_EBYTE);
        }
        if s.len() > NAME_CAP {
            return Err(TO_ELEN);
        }
        if !printable(s) {
            return Err(TO_EBYTE);
        }
        let mut n = Name::blank();
        let mut i = 0usize;
        while i < s.len() {
            match (s.get(i), n.bytes.get_mut(i)) {
                (Some(src), Some(dst)) => *dst = *src,
                _ => return Err(TO_ELEN),
            }
            i += 1;
        }
        n.len = s.len();
        Ok(n)
    }

    /// 字节。
    pub fn bytes(&self) -> &[u8] {
        self.bytes.get(0..self.len).unwrap_or(&[])
    }

    /// 文本。
    pub fn text(&self) -> String {
        let mut s = String::new();
        let mut i = 0usize;
        while i < self.len {
            if let Some(b) = self.bytes.get(i) {
                s.push(*b as char);
            }
            i += 1;
        }
        s
    }

    /// 长度。
    pub fn len(&self) -> usize {
        self.len
    }

    /// 是否空。
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// 是否同名。
    pub fn named(&self, other: &[u8]) -> bool {
        eq_bytes(self.bytes(), other)
    }
}

// ---------------------------------------------------------------------------
// 调用
// ---------------------------------------------------------------------------

/// 一次解析完成的 `timeout` 调用。
#[derive(Clone, Copy, Debug)]
pub struct Invocation {
    config: Config,
    cmds: [Name; CMD_CAP],
    len: usize,
    dur_seen: bool,
    sig_seen: bool,
    kill_seen: bool,
    end_seen: bool,
}

impl Invocation {
    /// 空白。
    pub const fn blank() -> Invocation {
        Invocation {
            config: Config::new(),
            cmds: [Name::blank(); CMD_CAP],
            len: 0,
            dur_seen: false,
            sig_seen: false,
            kill_seen: false,
            end_seen: false,
        }
    }

    /// 配置。
    pub fn config(&self) -> Config {
        self.config
    }

    /// 命令数。
    pub fn len(&self) -> usize {
        self.len
    }

    /// 是否无命令。
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// 取命令。
    pub fn at(&self, i: usize) -> Option<Name> {
        if i >= self.len {
            return None;
        }
        self.cmds.get(i).copied()
    }

    /// 首命令。
    pub fn first(&self) -> Option<Name> {
        self.at(0)
    }

    /// 是否见过 `--`。
    pub fn end_seen(&self) -> bool {
        self.end_seen
    }

    /// 命令整行。
    pub fn command_text(&self) -> String {
        let mut s = String::new();
        let mut i = 0usize;
        while i < self.len {
            if let Some(c) = self.cmds.get(i) {
                if i > 0 {
                    s.push(' ');
                }
                s.push_str(c.text().as_str());
            }
            i += 1;
        }
        s
    }
}

/// 取下个参数。
fn next_arg<'a>(args: &[&'a [u8]], i: &mut usize, code: u32) -> Result<&'a [u8], u32> {
    *i += 1;
    match args.get(*i) {
        Some(v) => Ok(*v),
        None => Err(code),
    }
}

/// 解析 `timeout` 参数。
pub fn parse_argv(args: &[&[u8]]) -> Result<Invocation, u32> {
    let mut inv = Invocation::blank();
    let mut i = 0usize;
    let mut operands = false;
    while i < args.len() {
        let a = match args.get(i) {
            Some(v) => *v,
            None => return Err(TO_EEXEC),
        };
        if !operands {
            if eq_bytes(a, b"--") {
                inv.end_seen = true;
                operands = true;
                i += 1;
                continue;
            }
            if eq_bytes(a, b"--preserve-status") {
                if inv.config.preserve {
                    return Err(TO_ESTATE);
                }
                inv.config.preserve = true;
                i += 1;
                continue;
            }
            if eq_bytes(a, b"--foreground") {
                if inv.config.foreground {
                    return Err(TO_ESTATE);
                }
                inv.config.foreground = true;
                i += 1;
                continue;
            }
            if eq_bytes(a, b"-s") || eq_bytes(a, b"--signal") {
                if inv.sig_seen {
                    return Err(TO_ESTATE);
                }
                let v = next_arg(args, &mut i, TO_ESIG)?;
                inv.config.sig = Sig::parse(v)?;
                inv.sig_seen = true;
                i += 1;
                continue;
            }
            if starts_with(a, b"--signal=") {
                if inv.sig_seen {
                    return Err(TO_ESTATE);
                }
                let v = match a.get(9..) {
                    Some(v) => v,
                    None => return Err(TO_ESIG),
                };
                inv.config.sig = Sig::parse(v)?;
                inv.sig_seen = true;
                i += 1;
                continue;
            }
            if eq_bytes(a, b"-k") || eq_bytes(a, b"--kill-after") {
                if inv.kill_seen {
                    return Err(TO_ESTATE);
                }
                let v = next_arg(args, &mut i, TO_EDUR)?;
                let d = parse_duration(v)?;
                inv.config.kill_after = KillAfter::on(d);
                inv.kill_seen = true;
                i += 1;
                continue;
            }
            if starts_with(a, b"--kill-after=") {
                if inv.kill_seen {
                    return Err(TO_ESTATE);
                }
                let v = match a.get(13..) {
                    Some(v) => v,
                    None => return Err(TO_EDUR),
                };
                let d = parse_duration(v)?;
                inv.config.kill_after = KillAfter::on(d);
                inv.kill_seen = true;
                i += 1;
                continue;
            }
            if a.len() > 2 && a.first() == Some(&b'-') && a.get(1) == Some(&b's') {
                if inv.sig_seen {
                    return Err(TO_ESTATE);
                }
                let v = match a.get(2..) {
                    Some(v) => v,
                    None => return Err(TO_ESIG),
                };
                inv.config.sig = Sig::parse(v)?;
                inv.sig_seen = true;
                i += 1;
                continue;
            }
            if a.len() > 2 && a.first() == Some(&b'-') && a.get(1) == Some(&b'k') {
                if inv.kill_seen {
                    return Err(TO_ESTATE);
                }
                let v = match a.get(2..) {
                    Some(v) => v,
                    None => return Err(TO_EDUR),
                };
                let d = parse_duration(v)?;
                inv.config.kill_after = KillAfter::on(d);
                inv.kill_seen = true;
                i += 1;
                continue;
            }
            if !a.is_empty() && a.first() == Some(&b'-') {
                return Err(TO_EOPT);
            }
            operands = true;
        }
        if !inv.dur_seen {
            inv.config.duration = parse_duration(a)?;
            inv.dur_seen = true;
            i += 1;
            continue;
        }
        let n = Name::new(a)?;
        if inv.len >= CMD_CAP {
            return Err(TO_ELEN);
        }
        if let Some(dst) = inv.cmds.get_mut(inv.len) {
            *dst = n;
        }
        inv.len += 1;
        i += 1;
    }
    if !inv.dur_seen {
        return Err(TO_EDUR);
    }
    if inv.len == 0 {
        return Err(TO_EEXEC);
    }
    Ok(inv)
}

// ---------------------------------------------------------------------------
// 子进程与结果
// ---------------------------------------------------------------------------

/// 子进程终态。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    /// 正常退出。
    Exit(u32),
    /// 被信号杀死。
    Signed(Sig),
}

impl Status {
    /// 退出码。
    pub fn code(&self) -> u32 {
        match self {
            Status::Exit(n) => *n,
            Status::Signed(s) => EXIT_SIGNAL_BASE.saturating_add(s.number()),
        }
    }
}

/// 子进程观测。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Child {
    /// 从启动到结束/到点的毫秒数。
    pub elapsed_ms: u64,
    /// 终态（仅未超时时有效）。
    pub status: Status,
    /// 是否屏蔽首信号。
    pub ignores: bool,
}

impl Child {
    /// 构造。
    pub const fn new(elapsed_ms: u64, status: Status, ignores: bool) -> Child {
        Child { elapsed_ms, status, ignores }
    }

    /// 正常早退。
    pub const fn quick(code: u32) -> Child {
        Child { elapsed_ms: 0, status: Status::Exit(code), ignores: false }
    }
}

/// 限时执行结果。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Outcome {
    /// 是否到点。
    pub timed_out: bool,
    /// 是否被 KILL。
    pub killed: bool,
    /// 是否进入宽限二段。
    pub grace: bool,
    /// 子进程是否死等不退。
    pub stuck: bool,
    /// 首信号（未到点则 `None`）。
    pub signal: Option<Sig>,
    /// 退出码。
    pub exit: u32,
}

impl Outcome {
    /// 未到点的直接透传。
    pub fn pass(child: &Child) -> Outcome {
        Outcome {
            timed_out: false,
            killed: false,
            grace: false,
            stuck: false,
            signal: None,
            exit: child.status.code(),
        }
    }

    /// 渲染。
    pub fn text(&self) -> String {
        let mut s = String::new();
        s.push_str("exit ");
        push_u64(&mut s, self.exit as u64);
        if self.timed_out {
            s.push_str(" timeout");
        }
        if self.killed {
            s.push_str(" killed");
        }
        if self.grace {
            s.push_str(" grace");
        }
        if self.stuck {
            s.push_str(" stuck");
        }
        if let Some(sig) = self.signal {
            s.push_str(" sig ");
            s.push_str(sig.text());
        }
        s.push('\n');
        s
    }
}

/// 判定一次限时执行。
pub fn run(cfg: &Config, child: &Child) -> Outcome {
    if !cfg.limited() || child.elapsed_ms < cfg.duration.ms() {
        return Outcome::pass(child);
    }
    let sig = cfg.signal();
    if !child.ignores {
        let exit = if cfg.preserve {
            EXIT_SIGNAL_BASE.saturating_add(sig.number())
        } else {
            EXIT_TIMEOUT
        };
        return Outcome {
            timed_out: true,
            killed: false,
            grace: false,
            stuck: false,
            signal: Some(sig),
            exit,
        };
    }
    let ka = cfg.kill_after();
    if ka.enabled() {
        Outcome {
            timed_out: true,
            killed: true,
            grace: !ka.delay().is_zero(),
            stuck: false,
            signal: Some(sig),
            exit: EXIT_KILLED,
        }
    } else {
        Outcome {
            timed_out: true,
            killed: false,
            grace: false,
            stuck: true,
            signal: Some(sig),
            exit: EXIT_TIMEOUT,
        }
    }
}

/// 便捷入口：从参数直接跑一次模拟执行。
pub fn execute(args: &[&[u8]], child: &Child) -> Result<Outcome, u32> {
    let inv = parse_argv(args)?;
    Ok(run(&inv.config(), child))
}

// ---------------------------------------------------------------------------
// 样本与账本
// ---------------------------------------------------------------------------

/// 一格样本。
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

/// 账本。
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

    /// 登记。
    pub fn register(&mut self, s: Sample) -> Result<(), u32> {
        if self.frozen || (s.slot as usize) != self.len || (s.slot as usize) >= SAMPLES {
            self.rejects = self.rejects.saturating_add(1);
            return Err(TO_ESLOT);
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

    /// 是否空。
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// 取一格。
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
            return Err(TO_EBAL);
        }
        const VALS: [u32; 8] = [30000, 90000, 500, 9, 0, 124, 137, 143];
        const CODES: [u32; 6] = [TO_EDUR, TO_ESIG, TO_EOPT, TO_EEXEC, TO_ELEN, TO_EBYTE];
        let mut vi = 0usize;
        let mut ci = 0usize;
        let mut i = 0usize;
        while i < self.len {
            let s = match self.rows.get(i) {
                Some(v) => *v,
                None => return Err(TO_EBAL),
            };
            if s.slot as usize != i {
                return Err(TO_ESLOT);
            }
            if s.ok {
                if s.code != 0 {
                    return Err(TO_EVALUE);
                }
                match VALS.get(vi) {
                    Some(w) => {
                        if s.value != *w {
                            return Err(TO_EVALUE);
                        }
                    }
                    None => return Err(TO_EBAL),
                }
                vi += 1;
            } else {
                match CODES.get(ci) {
                    Some(w) => {
                        if s.code != *w {
                            return Err(TO_EVALUE);
                        }
                    }
                    None => return Err(TO_EBAL),
                }
                ci += 1;
            }
            i += 1;
        }
        if vi != 8 || ci != 6 {
            return Err(TO_EBAL);
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
            self.rejects = self.rejects.saturating_add(1);
            return Err(TO_EBAL);
        }
        self.root = self.seal();
        self.frozen = true;
        Ok(self.root)
    }

    /// 重放。
    pub fn replay(&self) -> Result<u64, u32> {
        let fresh = build_ledger();
        let a = fresh.seal();
        if a == self.seal() {
            Ok(a)
        } else {
            Err(TO_ESEAL)
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

// ---------------------------------------------------------------------------
// 账本观测
// ---------------------------------------------------------------------------

/// `Result<u32, u32>` → 样本。
fn word_cell(slot: u8, op: u8, r: Result<u32, u32>) -> Sample {
    match r {
        Ok(v) => Sample { slot, op, ok: true, code: 0, value: v },
        Err(c) => Sample { slot, op, ok: false, code: c, value: 0 },
    }
}

/// 时长 → 毫秒（限 u32）。
fn dur_word(d: Duration) -> Result<u32, u32> {
    let ms = d.ms();
    if ms > u32::MAX as u64 {
        return Err(TO_ERANGE);
    }
    Ok(ms as u32)
}

/// 恰好的早退命令：`timeout 30s sleep 1`。
fn early_probe() -> Result<u32, u32> {
    let args: [&[u8]; 4] = [b"30s", b"sh", b"-c", b"true"];
    let inv = parse_argv(&args)?;
    let child = Child::new(1_000, Status::Exit(0), false);
    Ok(run(&inv.config(), &child).exit)
}

/// 到点发 TERM：退出 124。
fn timeout_probe() -> Result<u32, u32> {
    let inv = parse_argv(&[b"1s", b"sleep", b"5"])?;
    let child = Child::new(5_000, Status::Exit(0), false);
    Ok(run(&inv.config(), &child).exit)
}

/// 忽略首信号且给了 `-k`：137。
fn killed_probe() -> Result<u32, u32> {
    let inv = parse_argv(&[b"-k", b"0.5s", b"1s", b"sleep", b"9"])?;
    let child = Child::new(9_000, Status::Exit(0), true);
    Ok(run(&inv.config(), &child).exit)
}

/// `--preserve-status`：128+TERM=143。
fn preserve_probe() -> Result<u32, u32> {
    let inv = parse_argv(&[b"--preserve-status", b"1s", b"sleep", b"2"])?;
    let child = Child::new(2_000, Status::Exit(0), false);
    Ok(run(&inv.config(), &child).exit)
}

/// 账本组装。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();

    let _ = l.register(word_cell(0, OP_DUR, parse_duration(b"30").and_then(dur_word)));

    let _ = l.register(word_cell(1, OP_DUR, parse_duration(b"1.5m").and_then(dur_word)));

    let _ = l.register(word_cell(2, OP_DUR, parse_duration(b"0.5s").and_then(dur_word)));

    let _ = l.register(word_cell(3, OP_SIG, Sig::parse(b"KILL").map(|s| s.number())));

    let _ = l.register(word_cell(4, OP_RUN, early_probe()));

    let _ = l.register(word_cell(5, OP_RUN, timeout_probe()));

    let _ = l.register(word_cell(6, OP_RUN, killed_probe()));

    let _ = l.register(word_cell(7, OP_RUN, preserve_probe()));

    let _ = l.register(word_cell(8, OP_DUR, parse_duration(b"").map(|_| 0)));

    let _ = l.register(word_cell(9, OP_SIG, Sig::parse(b"BOGUS").map(|_| 0)));

    let _ = l.register(word_cell(10, OP_OPT, parse_argv(&[b"--bogus", b"1s", b"x"]).map(|_| 0)));

    let _ = l.register(word_cell(11, OP_CMD, parse_argv(&[b"1s"]).map(|_| 0)));

    let _ = l.register(word_cell(
        12,
        OP_CMD,
        parse_argv(&[b"1s", b"a", b"b", b"c", b"d", b"e"]).map(|_| 0),
    ));

    let _ = l.register(word_cell(13, OP_CMD, parse_argv(&[b"1s", b"\x01"]).map(|_| 0)));

    l
}

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

/// 时长解析面。
fn duration_ok() -> bool {
    let a = parse_duration(b"30").map(|d| d.ms()) == Ok(30_000);
    let b = parse_duration(b"30s").map(|d| d.ms()) == Ok(30_000);
    let c = parse_duration(b"1.5m").map(|d| d.ms()) == Ok(90_000);
    let d = parse_duration(b"0.5s").map(|d| d.ms()) == Ok(500);
    let e = parse_duration(b"2h").map(|d| d.ms()) == Ok(7_200_000);
    let f = parse_duration(b"1d").map(|d| d.ms()) == Ok(86_400_000);
    let g = parse_duration(b"0").map(|d| d.is_zero()) == Ok(true);
    let h = parse_duration(b"250ms").err() == Some(TO_EDUR);
    let i = parse_duration(b"1.2345s").err() == Some(TO_EDUR);
    let j = parse_duration(b"1.s").err() == Some(TO_EDUR);
    let k = parse_duration(b"99999999999999999999d").err() == Some(TO_ERANGE);
    let l = parse_duration(b"1.1.1").err() == Some(TO_EDUR);
    let m = parse_duration(b"1x").err() == Some(TO_EDUR);
    a && b && c && d && e && f && g && h && i && j && k && l && m
}

/// 信号解析面。
fn signal_ok() -> bool {
    let a = Sig::parse(b"TERM") == Ok(Sig::Term);
    let b = Sig::parse(b"term") == Ok(Sig::Term);
    let c = Sig::parse(b"15") == Ok(Sig::Term);
    let d = Sig::parse(b"KILL") == Ok(Sig::Kill);
    let e = Sig::parse(b"9") == Ok(Sig::Kill);
    let f = Sig::parse(b"1") == Ok(Sig::Hup);
    let g = Sig::parse(b"10") == Ok(Sig::Usr1);
    let h = Sig::parse(b"BOGUS").err() == Some(TO_ESIG);
    let i = Sig::parse(b"99").err() == Some(TO_ESIG);
    let j = Sig::parse(b"").err() == Some(TO_ESIG);
    let k = Sig::Kill.uncatchable() && !Sig::Term.uncatchable();
    let l = Sig::all().len() == 6;
    a && b && c && d && e && f && g && h && i && j && k && l
}

/// 选项面。
fn option_ok() -> bool {
    let a = match parse_argv(&[b"-s", b"INT", b"5s", b"x"]) {
        Ok(v) => v.config().signal() == Sig::Int,
        Err(_) => false,
    };
    let b = match parse_argv(&[b"--signal=KILL", b"5s", b"x"]) {
        Ok(v) => v.config().signal() == Sig::Kill,
        Err(_) => false,
    };
    let c = match parse_argv(&[b"-sTERM", b"5s", b"x"]) {
        Ok(v) => v.config().signal() == Sig::Term,
        Err(_) => false,
    };
    let d = match parse_argv(&[b"-k", b"2s", b"5s", b"x"]) {
        Ok(v) => {
            v.config().kill_after().enabled()
                && v.config().kill_after().delay().ms() == 2_000
        }
        Err(_) => false,
    };
    let e = match parse_argv(&[b"--kill-after=3s", b"5s", b"x"]) {
        Ok(v) => v.config().kill_after().delay().ms() == 3_000,
        Err(_) => false,
    };
    let f = match parse_argv(&[b"-k2s", b"5s", b"x"]) {
        Ok(v) => v.config().kill_after().delay().ms() == 2_000,
        Err(_) => false,
    };
    let g = match parse_argv(&[b"--preserve-status", b"5s", b"x"]) {
        Ok(v) => v.config().preserve(),
        Err(_) => false,
    };
    let h = match parse_argv(&[b"--foreground", b"5s", b"x"]) {
        Ok(v) => v.config().foreground(),
        Err(_) => false,
    };
    let i = match parse_argv(&[b"--", b"5s", b"x"]) {
        Ok(v) => v.end_seen() && v.len() == 1 && v.first().map(|n| n.named(b"x")).unwrap_or(false),
        Err(_) => false,
    };
    a && b && c && d && e && f && g && h && i
}

/// 选项错误面。
fn option_err_ok() -> bool {
    let a = parse_argv(&[b"-x", b"5s", b"c"]).err() == Some(TO_EOPT);
    let b = parse_argv(&[b"--bogus", b"5s", b"c"]).err() == Some(TO_EOPT);
    let c = parse_argv(&[b"-s", b"TERM", b"-s", b"INT", b"5s", b"c"]).err() == Some(TO_ESTATE);
    let d = parse_argv(&[b"-s"]).err() == Some(TO_ESIG);
    let e = parse_argv(&[b"-k"]).err() == Some(TO_EDUR);
    let f = parse_argv(&[b"--preserve-status", b"--preserve-status", b"5s", b"c"]).err()
        == Some(TO_ESTATE);
    let g = parse_argv(&[b"--foreground", b"--foreground", b"5s", b"c"]).err() == Some(TO_ESTATE);
    let h = parse_argv(&[b"-sNOPE", b"5s", b"c"]).err() == Some(TO_ESIG);
    a && b && c && d && e && f && g && h
}

/// 命令面。
fn command_ok() -> bool {
    let a = parse_argv(&[b"5s"]).err() == Some(TO_EEXEC);
    let b = parse_argv(&[]).err() == Some(TO_EDUR);
    let c = match parse_argv(&[b"5s", b"sh", b"-c", b"true", b"x"]) {
        Ok(v) => v.len() == 4 && v.command_text().as_str() == "sh -c true x",
        Err(_) => false,
    };
    let d = parse_argv(&[b"5s", b"a", b"b", b"c", b"d", b"e"]).err() == Some(TO_ELEN);
    let e = parse_argv(&[b"5s", b"\x02"]).err() == Some(TO_EBYTE);
    let f = parse_argv(&[b"5s", b"a b"]).err() == Some(TO_EBYTE);
    let g = match parse_argv(&[b"5s", b"-x"]) {
        Ok(v) => v.first().map(|n| n.named(b"-x")).unwrap_or(false),
        Err(_) => false,
    };
    a && b && c && d && e && f && g
}

/// 早退透传。
fn run_ok() -> bool {
    let inv = match parse_argv(&[b"5s", b"x"]) {
        Ok(v) => v,
        Err(_) => Invocation::blank(),
    };
    let zero = run(&inv.config(), &Child::new(100, Status::Exit(0), false));
    let seven = run(&inv.config(), &Child::new(4_999, Status::Exit(7), false));
    let signed = run(&inv.config(), &Child::new(1, Status::Signed(Sig::Int), false));
    zero.exit == 0
        && !zero.timed_out
        && seven.exit == 7
        && signed.exit == EXIT_SIGNAL_BASE + 2
        && inv.config().signal() == Sig::Term
        && inv.config().duration().ms() == 5_000
}

/// 到点三分支。
fn timeout_ok() -> bool {
    let inv = match parse_argv(&[b"1s", b"x"]) {
        Ok(v) => v,
        Err(_) => Invocation::blank(),
    };
    let plain = run(&inv.config(), &Child::new(1_000, Status::Exit(0), false));
    let a = plain.timed_out && !plain.killed && plain.exit == EXIT_TIMEOUT;
    let b = plain.signal == Some(Sig::Term);
    let both = match parse_argv(&[b"-k", b"0.5s", b"1s", b"x"]) {
        Ok(v) => v,
        Err(_) => Invocation::blank(),
    };
    let killed = run(&both.config(), &Child::new(9_000, Status::Exit(0), true));
    let c = killed.timed_out && killed.killed && killed.exit == EXIT_KILLED && killed.grace;
    let no_k = run(&inv.config(), &Child::new(9_000, Status::Exit(0), true));
    let d = no_k.stuck && !no_k.killed;
    let zero_k = match parse_argv(&[b"-k", b"0", b"1s", b"x"]) {
        Ok(v) => v,
        Err(_) => Invocation::blank(),
    };
    let now = run(&zero_k.config(), &Child::new(9_000, Status::Exit(0), true));
    let e = now.killed && !now.grace;
    a && b && c && d && e
}

/// 保留状态与禁用。
fn preserve_ok() -> bool {
    let p = match parse_argv(&[b"--preserve-status", b"-s", b"INT", b"1s", b"x"]) {
        Ok(v) => v,
        Err(_) => Invocation::blank(),
    };
    let o = run(&p.config(), &Child::new(2_000, Status::Exit(0), false));
    let a = o.exit == EXIT_SIGNAL_BASE + 2;
    let z = match parse_argv(&[b"0", b"x"]) {
        Ok(v) => v,
        Err(_) => Invocation::blank(),
    };
    let b = !z.config().limited();
    let c = run(&z.config(), &Child::new(9_999_999, Status::Exit(5), false)).exit == 5;
    let d = exec_fail();
    a && b && c && d
}

/// 不可执行命令映射到 126（装载阶段已拦，返回恒定失败语义）。
fn exec_fail() -> bool {
    EXIT_CANNOT == 126 && EXIT_FAILURE == 125 && EXIT_TIMEOUT == 124
}

/// 渲染与账本。
fn render_ok() -> bool {
    let p = match parse_argv(&[b"-k", b"2s", b"-s", b"INT", b"5s", b"x"]) {
        Ok(v) => v,
        Err(_) => Invocation::blank(),
    };
    let text = p.config().text();
    let a = text.as_str().starts_with("limit 5000ms\n");
    let b = text.as_str().contains("signal INT\n");
    let c = text.as_str().contains("kill-after 2000ms\n");
    let d = text.as_str().contains("mode background\n");
    let e = text.as_str().matches('\n').count() == 4;
    let o = run(&p.config(), &Child::new(5_000, Status::Exit(0), false));
    let f = o.text().as_str() == "exit 124 timeout sig INT\n";
    a && b && c && d && e && f
}

/// 账本闭环。
fn ledger_ok() -> bool {
    let l = build_ledger();
    if l.audit() != Ok(()) {
        return false;
    }
    if l.len() != SAMPLES || l.ok_count() != 8 || l.err_count() != 6 {
        return false;
    }
    let s1 = l.seal();
    if s1 != build_ledger().seal() {
        return false;
    }
    let mut l2 = build_ledger();
    let root = match l2.freeze() {
        Ok(v) => v,
        Err(_) => return false,
    };
    if root != s1 || !l2.is_frozen() {
        return false;
    }
    let replay = match l2.replay() {
        Ok(v) => v == root,
        Err(_) => false,
    };
    if !replay {
        return false;
    }
    let probe = Sample { slot: 0, op: 0, ok: false, code: 0, value: 0 };
    if l2.register(probe) != Err(TO_ESLOT) {
        return false;
    }
    l2.rejects() == 1
}

/// 四路注入。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];
    out[0] = duration_ok() && signal_ok();
    out[1] = option_ok() && option_err_ok() && command_ok();
    out[2] = ledger_ok();
    out[3] = run_ok() && timeout_ok() && preserve_ok() && render_ok();
    out
}

/// 判据集合。
pub fn run_unx11543_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11543.timeout");

    s.add(
        "TO01-时长解析",
        {
            duration_ok()
        },
        "static",
    );

    s.add(
        "TO02-信号解析",
        {
            signal_ok()
        },
        "static",
    );

    s.add(
        "TO03-选项面",
        {
            option_ok()
        },
        "static",
    );

    s.add(
        "TO04-命令面",
        {
            command_ok()
        },
        "static",
    );

    s.add(
        "TO05-早退透传",
        {
            run_ok()
        },
        "static",
    );

    s.add(
        "TO06-到点三分支",
        {
            timeout_ok()
        },
        "static",
    );

    s.add(
        "TO07-保留状态",
        {
            preserve_ok()
        },
        "static",
    );

    s.add(
        "TO08-渲染四行",
        {
            render_ok()
        },
        "static",
    );

    s.add(
        "TO09-选项错误码",
        {
            option_err_ok()
        },
        "static",
    );

    s.add(
        "TO10-容量边界",
        {
            let a = parse_argv(&[b"5s", b"a", b"b", b"c", b"d"]).map(|v| v.len()) == Ok(4);
            let b = parse_argv(&[b"5s", b"a", b"b", b"c", b"d", b"e"]).err() == Some(TO_ELEN);
            let c = Name::new(b"x").map(|n| n.len()) == Ok(1);
            let d = Name::new(b"").err() == Some(TO_EBYTE);
            let e = Name::new(&[b'x'; 49]).err() == Some(TO_ELEN);
            a && b && c && d && e
        },
        "static",
    );

    s.add(
        "TO11-账本对账",
        {
            build_ledger().audit() == Ok(())
        },
        "static",
    );

    s.add(
        "TO12-零时长禁用",
        {
            let z = match parse_argv(&[b"0s", b"x"]) {
                Ok(v) => v,
                Err(_) => Invocation::blank(),
            };
            let o = run(&z.config(), &Child::new(86_400_000, Status::Exit(3), false));
            !z.config().limited() && !o.timed_out && o.exit == 3
        },
        "static",
    );

    s.add(
        "TO13-冻结重放四注入",
        {
            let four = inject_four();
            four[0] && four[1] && four[2] && four[3]
        },
        "static",
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn duration_units() {
        assert_eq!(parse_duration(b"30").map(|d| d.ms()), Ok(30_000));
        assert_eq!(parse_duration(b"30s").map(|d| d.ms()), Ok(30_000));
        assert_eq!(parse_duration(b"2m").map(|d| d.ms()), Ok(120_000));
        assert_eq!(parse_duration(b"1.5m").map(|d| d.ms()), Ok(90_000));
        assert_eq!(parse_duration(b"0.5s").map(|d| d.ms()), Ok(500));
        assert_eq!(parse_duration(b"1h").map(|d| d.ms()), Ok(3_600_000));
        assert_eq!(parse_duration(b"1d").map(|d| d.ms()), Ok(86_400_000));
        assert_eq!(parse_duration(b"007").map(|d| d.ms()), Ok(7_000));
    }

    #[test]
    fn duration_rejects() {
        assert_eq!(parse_duration(b"").err(), Some(TO_EDUR));
        assert_eq!(parse_duration(b"ms").err(), Some(TO_EDUR));
        assert_eq!(parse_duration(b"1ms").err(), Some(TO_EDUR));
        assert_eq!(parse_duration(b"1.s").err(), Some(TO_EDUR));
        assert_eq!(parse_duration(b"1.2.3s").err(), Some(TO_EDUR));
        assert_eq!(parse_duration(b"1.2345s").err(), Some(TO_EDUR));
        assert_eq!(parse_duration(b"12x").err(), Some(TO_EDUR));
        assert_eq!(parse_duration(b"a").err(), Some(TO_EDUR));
        assert_eq!(parse_duration(b"1 2").err(), Some(TO_EBYTE));
        assert_eq!(parse_duration(b"\x01").err(), Some(TO_EBYTE));
        assert_eq!(parse_duration(&[b'9'; 20]).err(), Some(TO_ERANGE));
        assert_eq!(parse_duration(&[b'9'; 41]).err(), Some(TO_ELEN));
    }

    #[test]
    fn signal_names() {
        assert_eq!(Sig::parse(b"TERM"), Ok(Sig::Term));
        assert_eq!(Sig::parse(b"term"), Ok(Sig::Term));
        assert_eq!(Sig::parse(b"Term"), Ok(Sig::Term));
        assert_eq!(Sig::parse(b"15"), Ok(Sig::Term));
        assert_eq!(Sig::parse(b"9"), Ok(Sig::Kill));
        assert_eq!(Sig::parse(b"USR1"), Ok(Sig::Usr1));
        assert_eq!(Sig::parse(b"nope").err(), Some(TO_ESIG));
        assert_eq!(Sig::parse(b"0").err(), Some(TO_ESIG));
        assert_eq!(Sig::parse(b"-9").err(), Some(TO_ESIG));
        assert_eq!(Sig::parse(b"13").err(), Some(TO_ESIG));
    }

    #[test]
    fn option_forms() {
        let a = parse_argv(&[b"-s", b"INT", b"5", b"x"]).expect("a");
        assert_eq!(a.config().signal(), Sig::Int);
        let b = parse_argv(&[b"--signal=HUP", b"5", b"x"]).expect("b");
        assert_eq!(b.config().signal(), Sig::Hup);
        let c = parse_argv(&[b"-k0.5s", b"5", b"x"]).expect("c");
        assert!(c.config().kill_after().enabled());
        assert_eq!(c.config().kill_after().delay().ms(), 500);
        let d = parse_argv(&[b"--kill-after=1s", b"5", b"x"]).expect("d");
        assert_eq!(d.config().kill_after().delay().ms(), 1_000);
        let e = parse_argv(&[b"--preserve-status", b"--foreground", b"5", b"x"]).expect("e");
        assert!(e.config().preserve());
        assert!(e.config().foreground());
    }

    #[test]
    fn option_errors() {
        assert_eq!(parse_argv(&[b"-z", b"5", b"x"]).err(), Some(TO_EOPT));
        assert_eq!(parse_argv(&[b"--signal", b"5", b"x"]).err(), Some(TO_ESIG));
        assert_eq!(parse_argv(&[b"-k", b"5", b"x"]).err(), Some(TO_EDUR));
        assert_eq!(parse_argv(&[b"-s", b"TERM", b"--signal=INT", b"5", b"x"]).err(), Some(TO_ESTATE));
        assert_eq!(parse_argv(&[b"--", b"5", b"x"]).map(|v| v.len()), Ok(1));
    }

    #[test]
    fn command_forms() {
        let v = parse_argv(&[b"5", b"sh", b"-c", b"true"]).expect("v");
        assert_eq!(v.len(), 3);
        assert_eq!(v.command_text().as_str(), "sh -c true");
        assert!(v.first().map(|n| n.named(b"sh")).unwrap_or(false));
        assert_eq!(parse_argv(&[b"5"]).err(), Some(TO_EEXEC));
        assert_eq!(parse_argv(&[b"5", b"a", b"b", b"c", b"d", b"e"]).err(), Some(TO_ELEN));
        assert_eq!(parse_argv(&[b"5", b"\x03"]).err(), Some(TO_EBYTE));
    }

    #[test]
    fn early_exit_passthrough() {
        let inv = parse_argv(&[b"5s", b"x"]).expect("i");
        let a = run(&inv.config(), &Child::new(0, Status::Exit(0), false));
        assert_eq!(a.exit, 0);
        assert!(!a.timed_out);
        let b = run(&inv.config(), &Child::new(4_999, Status::Exit(9), false));
        assert_eq!(b.exit, 9);
        let c = run(&inv.config(), &Child::new(10, Status::Signed(Sig::Quit), false));
        assert_eq!(c.exit, 131);
    }

    #[test]
    fn timeout_plain() {
        let inv = parse_argv(&[b"1s", b"x"]).expect("i");
        let o = run(&inv.config(), &Child::new(1_000, Status::Exit(0), false));
        assert!(o.timed_out);
        assert_eq!(o.exit, 124);
        assert_eq!(o.signal, Some(Sig::Term));
        assert!(!o.killed);
    }

    #[test]
    fn timeout_kill_after() {
        let inv = parse_argv(&[b"-k", b"0.5s", b"1s", b"x"]).expect("i");
        let o = run(&inv.config(), &Child::new(5_000, Status::Exit(0), true));
        assert!(o.killed);
        assert!(o.grace);
        assert_eq!(o.exit, 137);
        let z = parse_argv(&[b"-k", b"0", b"1s", b"x"]).expect("z");
        let p = run(&z.config(), &Child::new(5_000, Status::Exit(0), true));
        assert!(p.killed);
        assert!(!p.grace);
    }

    #[test]
    fn timeout_stuck() {
        let inv = parse_argv(&[b"1s", b"x"]).expect("i");
        let o = run(&inv.config(), &Child::new(5_000, Status::Exit(0), true));
        assert!(o.stuck);
        assert!(!o.killed);
        assert_eq!(o.exit, 124);
    }

    #[test]
    fn preserve_status() {
        let inv = parse_argv(&[b"--preserve-status", b"-s", b"INT", b"1s", b"x"]).expect("i");
        let o = run(&inv.config(), &Child::new(2_000, Status::Exit(0), false));
        assert_eq!(o.exit, 130);
        assert!(o.timed_out);
    }

    #[test]
    fn disabled_limit() {
        let inv = parse_argv(&[b"0", b"x"]).expect("i");
        assert!(!inv.config().limited());
        let o = run(&inv.config(), &Child::new(999_999, Status::Exit(4), false));
        assert_eq!(o.exit, 4);
        assert!(!o.timed_out);
    }

    #[test]
    fn config_render() {
        let inv = parse_argv(&[b"-sHUP", b"-k", b"1s", b"5s", b"x"]).expect("i");
        let text = inv.config().text();
        assert!(text.as_str().contains("limit 5000ms\n"));
        assert!(text.as_str().contains("signal HUP\n"));
        assert!(text.as_str().contains("kill-after 1000ms\n"));
        assert!(text.as_str().contains("mode background\n"));
        assert_eq!(text.as_str().matches('\n').count(), 4);
    }

    #[test]
    fn outcome_render() {
        let inv = parse_argv(&[b"1s", b"x"]).expect("i");
        let o = run(&inv.config(), &Child::new(9_000, Status::Exit(0), false));
        assert_eq!(o.text().as_str(), "exit 124 timeout sig TERM\n");
        let k = parse_argv(&[b"-k1s", b"1s", b"x"]).expect("k");
        let q = run(&k.config(), &Child::new(9_000, Status::Exit(0), true));
        assert_eq!(q.text().as_str(), "exit 137 timeout killed grace sig TERM\n");
    }

    #[test]
    fn ledger_shape() {
        let l = build_ledger();
        assert_eq!(l.audit(), Ok(()));
        assert_eq!(l.len(), SAMPLES);
        assert_eq!(l.sample(1).map(|s| s.value), Some(90_000));
        assert_eq!(l.sample(9).map(|s| s.code), Some(TO_ESIG));
        assert_eq!(l.ok_count(), 8);
        assert_eq!(l.err_count(), 6);
    }

    #[test]
    fn ledger_freeze_replay() {
        let mut l = build_ledger();
        let root = l.freeze().expect("freeze");
        assert_eq!(l.replay(), Ok(root));
        assert_eq!(l.freeze(), Err(TO_EBAL));
        assert_eq!(l.register(Sample::blank()), Err(TO_ESLOT));
        assert_eq!(l.rejects(), 2);
    }

    #[test]
    fn injections() {
        let four = inject_four();
        assert!(four[0] && four[1] && four[2] && four[3]);
    }

    #[test]
    fn all_checks_green() {
        let s = run_unx11543_checks();
        assert!(s.all_passed());
    }
}
