//! UNX-F11250 LD_DEBUG 诊断通道（0x1f320 段）
//!
//! 锚点：UNX-F11250-J1 LD_DEBUG=libs/files 输出与基准机关键行一致，诊断通道
//! 开关矩阵全过。
//!
//! 纯功能模型：LD_DEBUG 是一组**类别开关**，不是一个布尔量。环境串按逗号、
//! 冒号或空格切成 token，每个 token 打开一类诊断：libs（库查找与 init）、
//! files（文件打开与来源）、symbols、reloc、bindings、versions、statistics；
//! `all` 一次全开，`help` 只请求帮助文本。没打开的类别不产生任何行。
//!
//! 输出面：LD_DEBUG_OUTPUT 为空或未设时写 stderr；给了名字就写同名文件。
//! 目的地只能设一次，二次设置是硬错（真实实现里不会有第二次机会）。
//!
//! 安全面：AT_SECURE 置位时 glibc 整体关闭 LD_DEBUG。本模块同样把整份清单
//! 静默吞掉，只记 suppressed 条数；此后任何显式 `enable` 都会拿到 ESECURE。
//!
//! 判据核心：libs+files 两类的基准行序必须逐字一致——查找、打开、init 的
//! 相对顺序就是装载器的实际步序，不能被类别开关的排列顺序打乱。

use crate::alloc::format;
use crate::checks::CheckSet;

// ---------------------------------------------------------------- 错误码
const SEG_BASE: u32 = 0x1f320;
/// 阶段错（未解析即输出 / 重复解析 / 解析后设开关）
pub const DB_EORDER: u32 = SEG_BASE + 0x01;
/// 未知类别 token
pub const DB_ETOKEN: u32 = SEG_BASE + 0x02;
/// 输出名为空
pub const DB_ENAME: u32 = SEG_BASE + 0x03;
/// 事件下标越界
pub const DB_EEVENT: u32 = SEG_BASE + 0x04;
/// 行缓冲已满
pub const DB_EFULL: u32 = SEG_BASE + 0x05;
/// 空账或重放摘要不符
pub const DB_ESTATE: u32 = SEG_BASE + 0x06;
/// 冻结后改写
pub const DB_EFROZEN: u32 = SEG_BASE + 0x07;
/// 安全模式下显式开类别
pub const DB_ESECURE: u32 = SEG_BASE + 0x08;
/// 重复打开同一类别
pub const DB_EDUP: u32 = SEG_BASE + 0x09;
/// 输出目的地重复设置
pub const DB_EDEST: u32 = SEG_BASE + 0x0a;

// ---------------------------------------------------------------- 域常量
/// 类别位：libs
pub const CAT_LIBS: u8 = 0x01;
/// 类别位：files
pub const CAT_FILES: u8 = 0x02;
/// 类别位：symbols
pub const CAT_SYMBOLS: u8 = 0x04;
/// 类别位：reloc
pub const CAT_RELOC: u8 = 0x08;
/// 类别位：bindings
pub const CAT_BINDINGS: u8 = 0x10;
/// 类别位：versions
pub const CAT_VERSIONS: u8 = 0x20;
/// 类别位：statistics
pub const CAT_STATISTICS: u8 = 0x40;
/// 全类别掩码
pub const CAT_ALL: u8 = 0x7f;
/// 输出目的地：stderr
pub const DEST_STDERR: u8 = 0;
/// 输出目的地：文件
pub const DEST_FILE: u8 = 1;
/// 行缓冲容量
pub const LINE_CAP: usize = 8;
/// 单行容量
pub const LINE_MAX: usize = 64;
/// 事件表容量
pub const EVENT_CAP: usize = 10;
/// 状态：建场
pub const ST_BUILD: u8 = 0;
/// 状态：已解析
pub const ST_PARSED: u8 = 1;
/// 状态：冻结
pub const ST_FROZEN: u8 = 2;
const SINK_MAX: usize = 32;
const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x100_0000_01b3;

// ---------------------------------------------------------------- 事件表
/// 一条诊断事件：类别 + 键 + 值
#[derive(Clone, Copy)]
pub struct DiagEvent {
    cat: u8,
    key: &'static str,
    val: &'static str,
}

impl DiagEvent {
    /// 所属类别位
    pub fn cat(&self) -> u8 { self.cat }
    /// 键名
    pub fn key(&self) -> &'static str { self.key }
    /// 键值
    pub fn val(&self) -> &'static str { self.val }
}

/// 类别名（用于行首）
pub fn cat_name(cat: u8) -> &'static str {
    match cat {
        CAT_LIBS => "libs",
        CAT_FILES => "files",
        CAT_SYMBOLS => "symbols",
        CAT_RELOC => "reloc",
        CAT_BINDINGS => "bindings",
        CAT_VERSIONS => "versions",
        CAT_STATISTICS => "statistics",
        _ => "unknown",
    }
}

const EVENTS: [DiagEvent; EVENT_CAP] = [
    DiagEvent { cat: CAT_LIBS, key: "find library", val: "libhook.so" },
    DiagEvent { cat: CAT_FILES, key: "file", val: "libhook.so" },
    DiagEvent { cat: CAT_LIBS, key: "calling init", val: "libhook.so" },
    DiagEvent { cat: CAT_FILES, key: "file", val: "libc.so.6" },
    DiagEvent { cat: CAT_LIBS, key: "calling init", val: "libc.so.6" },
    DiagEvent { cat: CAT_SYMBOLS, key: "symbol", val: "malloc" },
    DiagEvent { cat: CAT_RELOC, key: "relocation", val: "R_X86_64_JUMP_SLOT" },
    DiagEvent { cat: CAT_BINDINGS, key: "binding", val: "printf" },
    DiagEvent { cat: CAT_VERSIONS, key: "version", val: "GLIBC_2.34" },
    DiagEvent { cat: CAT_STATISTICS, key: "total", val: "8" },
];

/// 第 i 条事件
pub fn event(i: usize) -> Option<DiagEvent> {
    match EVENTS.get(i) {
        Some(e) => Some(*e),
        None => None,
    }
}

/// libs+files 的基准行（与基准机逐字一致）
const BASELINE: [&str; 5] = [
    "libs: find library=libhook.so",
    "files: file=libhook.so",
    "libs: calling init=libhook.so",
    "files: file=libc.so.6",
    "libs: calling init=libc.so.6",
];

// ---------------------------------------------------------------- 诊断通道
/// LD_DEBUG 通道
pub struct DebugChannel {
    mask: u8,
    dest: u8,
    sink: [u8; SINK_MAX],
    sink_len: u8,
    dest_set: bool,
    help: bool,
    secure: bool,
    state: u8,
    root: Option<u64>,
    lines: [[u8; LINE_MAX]; LINE_CAP],
    line_len: [u8; LINE_CAP],
    line_count: usize,
    /// 解析到的 token 数
    pub tokens: u32,
    /// 实际产出的行数
    pub emitted: u32,
    /// 因类别未开而跳过的次数
    pub skipped: u32,
    /// 安全模式下被吞掉的 token 数
    pub suppressed: u32,
    /// 重复类别次数
    pub dupes: u32,
    /// 行缓冲满时的丢弃次数
    pub dropped: u32,
    /// 拒绝次数
    pub rejects: u32,
}

impl DebugChannel {
    /// 空通道：零类别、stderr、未解析
    pub fn new() -> Self {
        DebugChannel {
            mask: 0,
            dest: DEST_STDERR,
            sink: [0u8; SINK_MAX],
            sink_len: 0,
            dest_set: false,
            help: false,
            secure: false,
            state: ST_BUILD,
            root: None,
            lines: [[0u8; LINE_MAX]; LINE_CAP],
            line_len: [0u8; LINE_CAP],
            line_count: 0,
            tokens: 0,
            emitted: 0,
            skipped: 0,
            suppressed: 0,
            dupes: 0,
            dropped: 0,
            rejects: 0,
        }
    }

    fn bump(&mut self, code: u32) -> u32 {
        self.rejects = self.rejects.wrapping_add(1);
        code
    }

    /// 当前状态
    pub fn stage(&self) -> u8 { self.state }
    /// 类别掩码
    pub fn mask(&self) -> u8 { self.mask }
    /// 是否真的会产线
    pub fn active(&self) -> bool { !self.secure && self.mask != 0 }
    /// 是否请求了 help
    pub fn help_requested(&self) -> bool { self.help }
    /// 是否处于安全模式
    pub fn secure_mode(&self) -> bool { self.secure }
    /// 输出目的地
    pub fn destination(&self) -> u8 { self.dest }
    /// 文件目的地名（stderr 时为空串）
    pub fn sink(&self) -> &str { str_at(&self.sink, self.sink_len) }
    /// 已产出行数
    pub fn line_count(&self) -> usize { self.line_count }
    /// 第 i 行
    pub fn line(&self, i: usize) -> Result<&str, u32> {
        if i >= self.line_count { return Err(DB_ESTATE); }
        match self.lines.get(i) {
            Some(buf) => Ok(str_at(buf, match self.line_len.get(i) {
                Some(n) => *n,
                None => 0,
            })),
            None => Err(DB_ESTATE),
        }
    }

    /// 置安全模式：整份清单将被吞掉
    pub fn set_secure(&mut self, on: bool) -> Result<u32, u32> {
        if self.state == ST_FROZEN { return Err(self.bump(DB_EFROZEN)); }
        if self.state != ST_BUILD { return Err(self.bump(DB_EORDER)); }
        self.secure = on;
        Ok(if on { 1 } else { 0 })
    }

    /// 设输出目的地：只能设一次；空名拒
    pub fn set_output(&mut self, path: Option<&str>) -> Result<u32, u32> {
        if self.state == ST_FROZEN { return Err(self.bump(DB_EFROZEN)); }
        if self.state != ST_BUILD { return Err(self.bump(DB_EORDER)); }
        if self.dest_set { return Err(self.bump(DB_EDEST)); }
        match path {
            None => {
                self.dest = DEST_STDERR;
                self.sink_len = 0;
                self.dest_set = true;
                Ok(0)
            }
            Some(p) => {
                if p.as_bytes().is_empty() { return Err(self.bump(DB_ENAME)); }
                let n = match put(&mut self.sink, p) {
                    Some(n) => n,
                    None => return Err(self.bump(DB_ENAME)),
                };
                self.sink_len = n;
                self.dest = DEST_FILE;
                self.dest_set = true;
                Ok(1)
            }
        }
    }

    /// 显式开一个类别；安全模式下必拒
    pub fn enable(&mut self, cat: u8) -> Result<u32, u32> {
        if self.state == ST_FROZEN { return Err(self.bump(DB_EFROZEN)); }
        if self.secure { return Err(self.bump(DB_ESECURE)); }
        if self.state != ST_BUILD { return Err(self.bump(DB_EORDER)); }
        if cat == 0 || cat & (cat - 1) != 0 || cat & CAT_ALL != cat {
            return Err(self.bump(DB_EEVENT));
        }
        if self.mask & cat != 0 { return Err(self.bump(DB_EDUP)); }
        self.mask |= cat;
        Ok(self.mask as u32)
    }

    /// 解析环境串；未知 token 拒，重复类别只记账
    pub fn parse(&mut self, spec: &str) -> Result<u32, u32> {
        if self.state == ST_FROZEN { return Err(self.bump(DB_EFROZEN)); }
        if self.state != ST_BUILD { return Err(self.bump(DB_EORDER)); }
        let bytes = spec.as_bytes();
        let mut local: u8 = 0;
        let mut toks: u32 = 0;
        let mut dups: u32 = 0;
        let mut help = false;
        let mut start = 0usize;
        while start <= bytes.len() {
            let mut end = start;
            while end < bytes.len() && bytes[end] != b',' && bytes[end] != b':' && bytes[end] != b' ' {
                end += 1;
            }
            if end > start {
                let tok = match spec.get(start..end) {
                    Some(t) => t,
                    None => "",
                };
                if !tok.is_empty() {
                    toks += 1;
                    if tok == "all" {
                        if (self.mask | local) & CAT_ALL == CAT_ALL {
                            dups += 1;
                        } else {
                            local |= CAT_ALL;
                        }
                    } else if tok == "help" {
                        help = true;
                    } else {
                        match cat_bit(tok) {
                            Some(bit) => {
                                if (self.mask | local) & bit != 0 {
                                    dups += 1;
                                } else {
                                    local |= bit;
                                }
                            }
                            None => return Err(self.bump(DB_ETOKEN)),
                        }
                    }
                }
            }
            start = end + 1;
        }
        if self.secure {
            self.suppressed = self.suppressed.wrapping_add(toks);
        } else {
            self.mask |= local;
        }
        self.help = help;
        self.tokens = toks;
        self.dupes = dups;
        self.state = ST_PARSED;
        Ok(self.mask as u32)
    }

    /// 发一条事件：类别未开则静默跳过，缓冲满则拒
    pub fn emit(&mut self, idx: usize) -> Result<u32, u32> {
        if self.state == ST_FROZEN { return Err(self.bump(DB_EFROZEN)); }
        if self.state != ST_PARSED { return Err(self.bump(DB_EORDER)); }
        let ev = match event(idx) {
            Some(e) => e,
            None => return Err(self.bump(DB_EEVENT)),
        };
        if !self.active() || ev.cat() & self.mask == 0 {
            self.skipped = self.skipped.wrapping_add(1);
            return Ok(0);
        }
        if self.line_count >= LINE_CAP {
            self.dropped = self.dropped.wrapping_add(1);
            return Err(self.bump(DB_EFULL));
        }
        let text = format!("{}: {}={}", cat_name(ev.cat()), ev.key(), ev.val());
        let at = self.line_count;
        let n = match self.lines.get_mut(at) {
            Some(buf) => match put(buf, &text) {
                Some(n) => n,
                None => return Err(self.bump(DB_EFULL)),
            },
            None => return Err(self.bump(DB_EFULL)),
        };
        if let Some(slot) = self.line_len.get_mut(at) {
            *slot = n;
        }
        self.line_count = at + 1;
        self.emitted = self.emitted.wrapping_add(1);
        Ok(1)
    }

    /// 按表序发完全部事件，返回实际产出行数
    pub fn feed_all(&mut self) -> u32 {
        let mut n = 0u32;
        let mut i = 0;
        while i < EVENT_CAP {
            if self.emit(i) == Ok(1) { n = n.wrapping_add(1); }
            i += 1;
        }
        n
    }

    /// 基准行逐字比对（libs+files 两类，行数必须恰好五条）
    pub fn baseline_ok(&self) -> bool {
        if self.line_count != BASELINE.len() { return false; }
        let mut i = 0;
        while i < BASELINE.len() {
            match self.line(i) {
                Ok(s) => if s != BASELINE[i] { return false; },
                Err(_) => return false,
            }
            i += 1;
        }
        true
    }
}

fn cat_bit(tok: &str) -> Option<u8> {
    match tok {
        "libs" => Some(CAT_LIBS),
        "files" => Some(CAT_FILES),
        "symbols" => Some(CAT_SYMBOLS),
        "reloc" => Some(CAT_RELOC),
        "bindings" => Some(CAT_BINDINGS),
        "versions" => Some(CAT_VERSIONS),
        "statistics" => Some(CAT_STATISTICS),
        _ => None,
    }
}

fn put(dst: &mut [u8], s: &str) -> Option<u8> {
    let b = s.as_bytes();
    if b.len() > dst.len() { return None; }
    let mut i = 0;
    while i < b.len() { dst[i] = b[i]; i += 1; }
    Some(b.len() as u8)
}

fn str_at(buf: &[u8], len: u8) -> &str {
    let n = len as usize;
    if n > buf.len() { return ""; }
    match core::str::from_utf8(&buf[..n]) {
        Ok(s) => s,
        Err(_) => "",
    }
}

fn mix(h: u64, v: u64) -> u64 {
    (h ^ v).wrapping_mul(FNV_PRIME)
}

fn mix_str(h: u64, s: &str) -> u64 {
    let b = s.as_bytes();
    let mut x = h;
    let mut i = 0;
    while i < b.len() {
        x = mix(x, b[i] as u64);
        i += 1;
    }
    x
}

impl DebugChannel {
    /// FNV-1a 摘要：行缓冲、掩码、目的地与账本入账
    pub fn fold(&self) -> u64 {
        let mut h = FNV_OFF;
        let mut i = 0;
        while i < self.line_count {
            if let Some(buf) = self.lines.get(i) {
                let n = match self.line_len.get(i) {
                    Some(v) => *v,
                    None => 0,
                };
                h = mix_str(h, str_at(buf, n));
            }
            i += 1;
        }
        h = mix(h, self.mask as u64);
        h = mix(h, self.dest as u64);
        h = mix_str(h, self.sink());
        h = mix(h, if self.help { 1 } else { 0 });
        h = mix(h, if self.secure { 1 } else { 0 });
        h = mix(h, self.tokens as u64);
        h = mix(h, self.emitted as u64);
        h = mix(h, self.skipped as u64);
        h = mix(h, self.suppressed as u64);
        h = mix(h, self.dupes as u64);
        h = mix(h, self.dropped as u64);
        h = mix(h, self.rejects as u64);
        h
    }

    /// 冻结：只在已解析态成立，幂等
    pub fn freeze(&mut self) -> Result<u64, u32> {
        if self.state == ST_FROZEN {
            if let Some(r) = self.root { return Ok(r); }
        }
        if self.state != ST_PARSED { return Err(self.bump(DB_EORDER)); }
        self.state = ST_FROZEN;
        let v = self.fold();
        self.root = Some(v);
        Ok(v)
    }

    /// 重放：摘要必须与冻结时一致
    pub fn replay(&mut self) -> Result<u64, u32> {
        if self.state != ST_FROZEN { return Err(self.bump(DB_EORDER)); }
        let v = self.fold();
        match self.root {
            Some(r) if r == v => Ok(v),
            _ => Err(self.bump(DB_ESTATE)),
        }
    }
}

/// libs+files 基准样例：五条关键行
pub fn build_libs_files() -> DebugChannel {
    let mut d = DebugChannel::new();
    let _ = d.set_output(None);
    let _ = d.parse("libs,files");
    let _ = d.feed_all();
    d
}

/// 安全模式样例：清单被整份吞掉
pub fn build_secure() -> DebugChannel {
    let mut d = DebugChannel::new();
    let _ = d.set_secure(true);
    let _ = d.parse("libs files symbols");
    let _ = d.emit(0);
    d
}

/// 四路独立注入：每条都在新实例上证明一条拒绝路径会触发
pub fn inject_four() -> [bool; 4] {
    let mut ok = [false; 4];

    let mut a = DebugChannel::new();
    match a.parse("libs,bogus") {
        Err(code) if code == DB_ETOKEN => { ok[0] = a.rejects == 1 && a.stage() == ST_BUILD; }
        _ => {}
    }

    let mut b = DebugChannel::new();
    match b.emit(0) {
        Err(code) if code == DB_EORDER => { ok[1] = b.rejects == 1 && b.emitted == 0; }
        _ => {}
    }

    let mut c = build_secure();
    match c.enable(CAT_FILES) {
        Err(code) if code == DB_ESECURE => { ok[2] = c.rejects == 1 && c.suppressed == 3; }
        _ => {}
    }

    let mut d = DebugChannel::new();
    let _ = d.parse("all");
    let mut i = 0;
    while i < 9 {
        let _ = d.emit(i);
        i += 1;
    }
    ok[3] = d.line_count() == LINE_CAP && d.dropped == 1 && d.rejects == 1;

    ok
}

/// 十三项静态/动态判据
pub fn run_unx11250_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11250.lddebug");

    let fresh = DebugChannel::new();
    s.add(
        "DB1-建场静默",
        fresh.stage() == ST_BUILD
            && fresh.mask() == 0
            && !fresh.active()
            && fresh.destination() == DEST_STDERR
            && fresh.line_count() == 0,
        "static",
    );

    let mut a = DebugChannel::new();
    s.add(
        "DB2-libs与files两类",
        a.parse("libs,files") == Ok(3)
            && a.mask() == (CAT_LIBS | CAT_FILES)
            && a.tokens == 2
            && a.active(),
        "static",
    );

    let mut b = DebugChannel::new();
    s.add(
        "DB3-未知类别拒",
        b.parse("libs,bogus") == Err(DB_ETOKEN) && b.stage() == ST_BUILD && b.rejects == 1,
        "static",
    );

    let mut c = DebugChannel::new();
    let _ = c.parse("all");
    s.add(
        "DB4-all一次全开",
        c.mask() == CAT_ALL
            && c.emit(5) == Ok(1)
            && c.emit(8) == Ok(1)
            && c.emit(9) == Ok(1)
            && c.line_count() == 3,
        "static",
    );

    let mut d = DebugChannel::new();
    let _ = d.parse("libs,libs,files,help");
    s.add(
        "DB5-重复类别只记账",
        d.dupes == 1 && d.tokens == 4 && d.mask() == (CAT_LIBS | CAT_FILES) && d.rejects == 0,
        "static",
    );

    let base = build_libs_files();
    s.add_dyn(
        "DB6-基准行逐字一致",
        base.baseline_ok()
            && base.line(0) == Ok("libs: find library=libhook.so")
            && base.line(2) == Ok("libs: calling init=libhook.so"),
        &format!("lines={} first={:?}", base.line_count(), base.line(0)),
    );

    let mut e = DebugChannel::new();
    let _ = e.parse("libs");
    s.add(
        "DB7-未开类别静默跳过",
        e.emit(1) == Ok(0) && e.skipped == 1 && e.emit(0) == Ok(1) && e.line_count() == 1,
        "static",
    );

    let sec = build_secure();
    let mut sec2 = build_secure();
    s.add(
        "DB8-安全模式整份吞掉",
        sec.suppressed == 3
            && !sec.active()
            && sec.mask() == 0
            && sec.emitted == 0
            && sec2.enable(CAT_LIBS) == Err(DB_ESECURE),
        "static",
    );

    let mut f = DebugChannel::new();
    let file = f.set_output(Some("diag.log")) == Ok(1);
    let again = f.set_output(None) == Err(DB_EDEST);
    let mut g = DebugChannel::new();
    let blank = g.set_output(Some("")) == Err(DB_ENAME);
    s.add(
        "DB9-目的地只设一次",
        file
            && again
            && f.destination() == DEST_FILE
            && f.sink() == "diag.log"
            && blank
            && g.destination() == DEST_STDERR,
        "static",
    );

    let mut h = DebugChannel::new();
    let _ = h.parse("all");
    let mut i = 0;
    while i < 9 {
        let _ = h.emit(i);
        i += 1;
    }
    s.add_dyn(
        "DB10-行缓冲满即拒",
        h.line_count() == LINE_CAP && h.dropped == 1 && h.rejects == 1 && h.emitted == 8,
        &format!("lines={} dropped={}", h.line_count(), h.dropped),
    );

    let mut k = DebugChannel::new();
    let _ = k.parse("help");
    s.add(
        "DB11-help只请求帮助",
        k.help_requested() && k.mask() == 0 && !k.active() && k.emit(0) == Ok(0),
        "static",
    );

    let mut z = build_libs_files();
    let f1 = z.freeze();
    let f2 = z.freeze();
    let rp = z.replay();
    let locked_emit = z.emit(0) == Err(DB_EFROZEN);
    let drift = z.replay() == Err(DB_ESTATE);
    let rootv = match f1 {
        Ok(v) => v,
        Err(_) => 0,
    };
    s.add_dyn(
        "DB12-冻结重放幂等",
        f1.is_ok() && f1 == f2 && rp == f1 && locked_emit && drift && z.rejects == 2,
        &format!("root={:x}", rootv),
    );

    let inj = inject_four();
    s.add(
        "DB13-四路注入",
        inj[0] && inj[1] && inj[2] && inj[3],
        "static",
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flow_libs_files_baseline() {
        let base = build_libs_files();
        assert_eq!(base.stage(), ST_PARSED);
        assert_eq!(base.line_count(), 5);
        assert!(base.baseline_ok());
        assert_eq!(base.emitted, 5);
        assert_eq!(base.skipped, 5);
        assert!(base.active());
        assert_eq!(base.line(0), Ok("libs: find library=libhook.so"));
        assert_eq!(base.line(1), Ok("files: file=libhook.so"));
        assert_eq!(base.line(4), Ok("libs: calling init=libc.so.6"));
        assert_eq!(base.line(5), Err(DB_ESTATE));
    }

    #[test]
    fn gates_exact_rejects() {
        let mut d = DebugChannel::new();
        assert_eq!(d.emit(0), Err(DB_EORDER));
        assert_eq!(d.parse("libs bogus"), Err(DB_ETOKEN));
        assert_eq!(d.parse("libs,files"), Ok(3));
        assert_eq!(d.parse("all"), Err(DB_EORDER));
        assert_eq!(d.set_secure(true), Err(DB_EORDER));
        assert_eq!(d.enable(CAT_SYMBOLS), Err(DB_EORDER));
        assert_eq!(d.emit(0), Ok(1));
        assert_eq!(d.emit(1), Ok(1));
        assert_eq!(d.emit(2), Ok(1));
        assert_eq!(d.emit(3), Ok(1));
        assert_eq!(d.emit(4), Ok(1));
        assert_eq!(d.emit(99), Err(DB_EEVENT));
        assert_eq!(d.emit(5), Ok(0));
        assert_eq!(d.set_output(None), Err(DB_EORDER));
        assert!(d.freeze().is_ok());
        assert!(d.replay().is_ok());
        assert_eq!(d.emit(0), Err(DB_EFROZEN));
        assert_eq!(d.parse("libs"), Err(DB_EFROZEN));
        assert_eq!(d.replay(), Err(DB_ESTATE));
        assert_eq!(d.rejects, 10);
        assert_eq!(d.emitted, 5);
        assert_eq!(d.skipped, 1);
        assert_eq!(d.line_count(), 5);
        assert_eq!(d.tokens, 2);
    }

    #[test]
    fn rules_categories_and_dest() {
        let cats = [
            CAT_LIBS, CAT_FILES, CAT_SYMBOLS, CAT_RELOC,
            CAT_BINDINGS, CAT_VERSIONS, CAT_STATISTICS,
        ];
        let mut or = 0u8;
        let mut i = 0;
        while i < cats.len() {
            assert_eq!(cats[i] & (cats[i] - 1), 0);
            or |= cats[i];
            i += 1;
        }
        assert_eq!(or, CAT_ALL);

        let mut d = DebugChannel::new();
        assert_eq!(d.parse("all,libs"), Ok(CAT_ALL as u32));
        assert_eq!(d.dupes, 1);
        assert_eq!(d.tokens, 2);

        let mut h = DebugChannel::new();
        assert_eq!(h.parse("help"), Ok(0));
        assert!(h.help_requested());
        assert!(!h.active());

        let sec = build_secure();
        assert_eq!(sec.suppressed, 3);
        assert_eq!(sec.mask(), 0);
        assert_eq!(sec.emitted, 0);
        assert!(!sec.active());

        let mut f = DebugChannel::new();
        assert_eq!(f.set_output(Some("")), Err(DB_ENAME));
        assert_eq!(f.destination(), DEST_STDERR);
        assert_eq!(f.set_output(None), Ok(0));
        assert_eq!(f.set_output(Some("x")), Err(DB_EDEST));

        let codes = [
            DB_EORDER, DB_ETOKEN, DB_ENAME, DB_EEVENT, DB_EFULL,
            DB_ESTATE, DB_EFROZEN, DB_ESECURE, DB_EDUP, DB_EDEST,
        ];
        let mut k = 0;
        while k < codes.len() {
            assert_eq!(codes[k], SEG_BASE + (k as u32) + 1);
            let mut j = k + 1;
            while j < codes.len() {
                assert_ne!(codes[k], codes[j]);
                j += 1;
            }
            k += 1;
        }
        assert_eq!(SEG_BASE, 0x1f320);
    }

    #[test]
    fn injection_paths_fire() {
        let inj = inject_four();
        assert!(inj[0], "unknown category must be rejected");
        assert!(inj[1], "emit before parse must be rejected");
        assert!(inj[2], "secure enable must be rejected");
        assert!(inj[3], "line buffer overflow must be rejected");
    }

    #[test]
    fn checks_green() {
        let cs = run_unx11250_checks();
        assert_eq!(cs.len(), 13);
        assert!(cs.all_passed(), "{}", cs.red_items_text());
    }
}
