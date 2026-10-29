//! UNX-F11257 环境变量族语义总账（LD_*）（0x1f390 段）
//!
//! 模型吃一张固定的环境条目表，每条是“名字=值”形式的定长字节串。
//! 解析器只认五个消费点：LD_PRELOAD 的预加载对象数、LD_LIBRARY_PATH
//! 的非空目录数、LD_DEBUG 是否开启、LD_BIND_NOW 是否立即绑定、
//! LD_TRACE_LOADED_OBJECTS 是否只列依赖。其余 LD_ 开头的名字计入
//! 未知族，不带 LD_ 前缀的条目计入非装载环境，两者都只记账不报错。
//! 同名变量再次出现时后者覆盖前者，并累加一次“覆盖”计数。
//! 安全模式（AT_SECURE）下 PRELOAD/LIBRARY_PATH/DEBUG/TRACE 四项被
//! 整份忽略，但 BIND_NOW 仍然生效，这一例外由 active 系列接口验证。
//! 拒绝条件：条目表为空、条目数越界、条目缺少等号或名字为空。
//! 账本登记十二个样本，冻结前重放全部计数与生效位恒等式。

use crate::checks::CheckSet;

/// 段基址。
pub const SEG_BASE: u32 = 0x1f390;

/// 编号非法。
pub const EV_EID: u32 = SEG_BASE + 0x01;
/// 状态顺序错误。
pub const EV_EORDER: u32 = SEG_BASE + 0x02;
/// 重复登记。
pub const EV_EDUP: u32 = SEG_BASE + 0x03;
/// 已冻结。
pub const EV_EFROZEN: u32 = SEG_BASE + 0x04;
/// 观测不存在的格。
pub const EV_ENOBS: u32 = SEG_BASE + 0x05;
/// 观测结果不符。
pub const EV_EMISMATCH: u32 = SEG_BASE + 0x06;
/// 账本失衡。
pub const EV_EBAL: u32 = SEG_BASE + 0x07;
/// 条目表为空。
pub const EV_EEMPTY: u32 = SEG_BASE + 0x08;
/// 条目数越界。
pub const EV_ETRUNC: u32 = SEG_BASE + 0x09;
/// 条目格式非法。
pub const EV_EFORMAT: u32 = SEG_BASE + 0x0a;

/// 条目容量。
pub const ENTRY_MAX: usize = 40;
/// 条目个数上限。
pub const ENV_MAX: usize = 6;
/// 样本个数。
pub const SAMPLES: usize = 12;

/// PRELOAD 名字。
pub const NAME_PRELOAD: &[u8] = b"LD_PRELOAD";
/// LIBRARY_PATH 名字。
pub const NAME_LIBPATH: &[u8] = b"LD_LIBRARY_PATH";
/// DEBUG 名字。
pub const NAME_DEBUG: &[u8] = b"LD_DEBUG";
/// BIND_NOW 名字。
pub const NAME_BIND_NOW: &[u8] = b"LD_BIND_NOW";
/// TRACE 名字。
pub const NAME_TRACE: &[u8] = b"LD_TRACE_LOADED_OBJECTS";

/// FNV 偏移基。
pub const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;
/// FNV 质数。
pub const FNV_PRIME: u64 = 0x100_0000_01b3;

/// 一张环境条目表。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EnvCase {
    pub buf: [[u8; ENTRY_MAX]; ENV_MAX],
    pub len: [u8; ENV_MAX],
    pub n: u8,
    pub secure: bool,
}

impl EnvCase {
    /// 空表。
    pub const fn blank() -> EnvCase {
        EnvCase {
            buf: [[0; ENTRY_MAX]; ENV_MAX],
            len: [0; ENV_MAX],
            n: 0,
            secure: false,
        }
    }
}

/// 写入一条条目，返回是否写入成功。
pub fn set_entry(c: &mut EnvCase, s: &str) -> bool {
    let at = c.n as usize;
    if at >= ENV_MAX {
        return false;
    }
    let bytes = s.as_bytes();
    if bytes.len() > ENTRY_MAX {
        return false;
    }
    if let Some(row) = c.buf.get_mut(at) {
        let mut i = 0usize;
        while i < bytes.len() {
            if let Some(b) = row.get_mut(i) {
                *b = match bytes.get(i) {
                    Some(v) => *v,
                    None => 0,
                };
            }
            i += 1;
        }
    }
    if let Some(l) = c.len.get_mut(at) {
        *l = bytes.len() as u8;
    }
    c.n = c.n.wrapping_add(1);
    true
}

/// 取第 i 条条目的字节切片。
fn entry<'a>(c: &'a EnvCase, i: usize) -> Option<&'a [u8]> {
    let n = *c.len.get(i)? as usize;
    let row = c.buf.get(i)?;
    row.get(0..n)
}

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

fn starts_with(hay: &[u8], head: &[u8]) -> bool {
    if hay.len() < head.len() {
        return false;
    }
    match hay.get(0..head.len()) {
        Some(v) => eq_bytes(v, head),
        None => false,
    }
}

/// 按分隔符统计非空片段数。
pub fn count_tokens(v: &[u8], sep: u8) -> u32 {
    let mut n = 0u32;
    let mut in_tok = false;
    let mut i = 0usize;
    while i < v.len() {
        match v.get(i) {
            Some(b) if *b == sep => in_tok = false,
            Some(_) => {
                if !in_tok {
                    n = n.wrapping_add(1);
                    in_tok = true;
                }
            }
            None => break,
        }
        i += 1;
    }
    n
}

/// 按冒号或空格统计列表项数量。
pub fn count_list(v: &[u8]) -> u32 {
    let mut n = 0u32;
    let mut in_tok = false;
    let mut i = 0usize;
    while i < v.len() {
        match v.get(i) {
            Some(b) if *b == b':' || *b == b' ' => in_tok = false,
            Some(_) => {
                if !in_tok {
                    n = n.wrapping_add(1);
                    in_tok = true;
                }
            }
            None => break,
        }
        i += 1;
    }
    n
}

/// 解析后的环境语义摘要。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EnvInfo {
    pub preload: u32,
    pub libpath: u32,
    pub overrides: u32,
    pub unknown: u32,
    pub non_ld: u32,
    pub secure_ignored: u32,
    pub debug: bool,
    pub bind_now: bool,
    pub trace: bool,
    pub secure: bool,
}

impl EnvInfo {
    /// 空摘要。
    pub const fn none() -> EnvInfo {
        EnvInfo {
            preload: 0,
            libpath: 0,
            overrides: 0,
            unknown: 0,
            non_ld: 0,
            secure_ignored: 0,
            debug: false,
            bind_now: false,
            trace: false,
            secure: false,
        }
    }

    /// 预加载是否真的生效。
    pub fn preload_active(&self) -> bool {
        !self.secure && self.preload > 0
    }

    /// 库路径是否真的生效。
    pub fn libpath_active(&self) -> bool {
        !self.secure && self.libpath > 0
    }

    /// 调试输出是否真的生效。
    pub fn debug_active(&self) -> bool {
        !self.secure && self.debug
    }

    /// 只列依赖模式是否真的生效。
    pub fn trace_active(&self) -> bool {
        !self.secure && self.trace
    }

    /// 立即绑定在安全模式下依然生效。
    pub fn bind_active(&self) -> bool {
        self.bind_now
    }
}

/// 解析环境条目表。
pub fn parse(c: &EnvCase) -> Result<EnvInfo, u32> {
    let n = c.n as usize;
    if n == 0 {
        return Err(EV_EEMPTY);
    }
    if n > ENV_MAX {
        return Err(EV_ETRUNC);
    }
    let mut info = EnvInfo::none();
    info.secure = c.secure;
    let mut seen = [false; 5];
    let mut i = 0usize;
    while i < n {
        let e = match entry(c, i) {
            Some(v) => v,
            None => return Err(EV_ETRUNC),
        };
        let mut eq = 0usize;
        let mut found = false;
        while eq < e.len() {
            if let Some(b) = e.get(eq) {
                if *b == b'=' {
                    found = true;
                    break;
                }
            }
            eq += 1;
        }
        if !found {
            return Err(EV_EFORMAT);
        }
        let name = match e.get(0..eq) {
            Some(v) => v,
            None => return Err(EV_EFORMAT),
        };
        if name.is_empty() {
            return Err(EV_EFORMAT);
        }
        let val = match e.get(eq + 1..) {
            Some(v) => v,
            None => &[],
        };
        if !starts_with(name, b"LD_") {
            info.non_ld = info.non_ld.wrapping_add(1);
            i += 1;
            continue;
        }
        if eq_bytes(name, NAME_PRELOAD) {
            if seen[0] {
                info.overrides = info.overrides.wrapping_add(1);
            }
            seen[0] = true;
            info.preload = count_list(val);
            if info.secure {
                info.secure_ignored = info.secure_ignored.wrapping_add(1);
            }
        } else if eq_bytes(name, NAME_LIBPATH) {
            if seen[1] {
                info.overrides = info.overrides.wrapping_add(1);
            }
            seen[1] = true;
            info.libpath = count_tokens(val, b':');
            if info.secure {
                info.secure_ignored = info.secure_ignored.wrapping_add(1);
            }
        } else if eq_bytes(name, NAME_DEBUG) {
            if seen[2] {
                info.overrides = info.overrides.wrapping_add(1);
            }
            seen[2] = true;
            info.debug = !val.is_empty();
            if info.secure {
                info.secure_ignored = info.secure_ignored.wrapping_add(1);
            }
        } else if eq_bytes(name, NAME_BIND_NOW) {
            if seen[3] {
                info.overrides = info.overrides.wrapping_add(1);
            }
            seen[3] = true;
            info.bind_now = !val.is_empty();
        } else if eq_bytes(name, NAME_TRACE) {
            if seen[4] {
                info.overrides = info.overrides.wrapping_add(1);
            }
            seen[4] = true;
            info.trace = !val.is_empty();
            if info.secure {
                info.secure_ignored = info.secure_ignored.wrapping_add(1);
            }
        } else {
            info.unknown = info.unknown.wrapping_add(1);
        }
        i += 1;
    }
    Ok(info)
}

/// 样本规格。
pub fn spec_of(id: usize) -> EnvCase {
    let mut c = EnvCase::blank();
    match id {
        0 => {
            set_entry(&mut c, "LD_PRELOAD=liba.so:libb.so");
        }
        1 => {
            set_entry(&mut c, "LD_PRELOAD=liba.so libb.so libc.so");
        }
        2 => {
            set_entry(&mut c, "LD_LIBRARY_PATH=/a:/b::/c");
        }
        3 => {
            set_entry(&mut c, "LD_DEBUG=libs files");
        }
        4 => {
            set_entry(&mut c, "LD_BIND_NOW=1");
        }
        5 => {
            set_entry(&mut c, "LD_TRACE_LOADED_OBJECTS=1");
        }
        6 => {
            set_entry(&mut c, "LD_PRELOAD=libx.so");
            set_entry(&mut c, "LD_LIBRARY_PATH=/l1:/l2");
            set_entry(&mut c, "LD_DEBUG=all");
            set_entry(&mut c, "LD_BIND_NOW=1");
            set_entry(&mut c, "LD_TRACE_LOADED_OBJECTS=1");
            set_entry(&mut c, "LD_FOO=1");
        }
        7 => {
            set_entry(&mut c, "LD_DEBUG=libs");
            set_entry(&mut c, "LD_DEBUG=files");
        }
        8 => {
            set_entry(&mut c, "PATH=/bin");
            set_entry(&mut c, "LD_PRELOAD=libp.so");
        }
        9 => {
            set_entry(&mut c, "LD_PRELOAD");
        }
        10 => {}
        11 => {
            c.secure = true;
            set_entry(&mut c, "LD_PRELOAD=libs.so");
            set_entry(&mut c, "LD_LIBRARY_PATH=/s");
            set_entry(&mut c, "LD_DEBUG=libs");
            set_entry(&mut c, "LD_TRACE_LOADED_OBJECTS=1");
            set_entry(&mut c, "LD_BIND_NOW=1");
        }
        _ => {}
    }
    c
}

/// 样本期望：(码, 预加载数, 目录数, 覆盖数, 未知数, 非LD数, 安全忽略数,
/// debug, bind, trace)。
pub fn expected_of(id: usize) -> (u32, u32, u32, u32, u32, u32, u32, bool, bool, bool) {
    match id {
        0 => (0, 2, 0, 0, 0, 0, 0, false, false, false),
        1 => (0, 3, 0, 0, 0, 0, 0, false, false, false),
        2 => (0, 0, 3, 0, 0, 0, 0, false, false, false),
        3 => (0, 0, 0, 0, 0, 0, 0, true, false, false),
        4 => (0, 0, 0, 0, 0, 0, 0, false, true, false),
        5 => (0, 0, 0, 0, 0, 0, 0, false, false, true),
        6 => (0, 1, 2, 0, 1, 0, 0, true, true, true),
        7 => (0, 0, 0, 1, 0, 0, 0, true, false, false),
        8 => (0, 1, 0, 0, 0, 1, 0, false, false, false),
        9 => (EV_EFORMAT, 0, 0, 0, 0, 0, 0, false, false, false),
        10 => (EV_EEMPTY, 0, 0, 0, 0, 0, 0, false, false, false),
        11 => (0, 1, 1, 0, 0, 0, 4, true, true, true),
        _ => (EV_EID, 0, 0, 0, 0, 0, 0, false, false, false),
    }
}

/// 账本中的一格。
#[derive(Clone, Copy)]
struct EnvSlot {
    code: u8,
    preload: u8,
    libpath: u8,
    overrides: u8,
    unknown: u8,
    non_ld: u8,
    secure_ignored: u8,
    bits: u8,
    seen: bool,
    spec: EnvCase,
}

impl EnvSlot {
    const fn zero() -> EnvSlot {
        EnvSlot {
            code: 0,
            preload: 0,
            libpath: 0,
            overrides: 0,
            unknown: 0,
            non_ld: 0,
            secure_ignored: 0,
            bits: 0,
            seen: false,
            spec: EnvCase::blank(),
        }
    }
}

/// LD_* 语义账本。
pub struct EnvLedger {
    slots: [EnvSlot; SAMPLES],
    n: usize,
    accepted: u32,
    rejected: u32,
    observed: u32,
    matched: u32,
    frozen: bool,
}

impl EnvLedger {
    /// 空账本。
    pub const fn new() -> EnvLedger {
        EnvLedger {
            slots: [EnvSlot::zero(); SAMPLES],
            n: 0,
            accepted: 0,
            rejected: 0,
            observed: 0,
            matched: 0,
            frozen: false,
        }
    }

    fn fail(&mut self, code: u32) -> u32 {
        self.rejected = self.rejected.wrapping_add(1);
        code
    }

    fn duplicate(&self, c: &EnvCase) -> bool {
        let mut i = 0usize;
        while i < self.n {
            if let Some(s) = self.slots.get(i) {
                if s.spec == *c {
                    return true;
                }
            }
            i += 1;
        }
        false
    }
}

impl EnvLedger {
    fn commit(&mut self, c: &EnvCase, code: u32, info: &EnvInfo) -> Result<u64, u32> {
        let at = self.n;
        if at >= SAMPLES {
            return Err(self.fail(EV_EID));
        }
        if let Some(slot) = self.slots.get_mut(at) {
            slot.code = if code == 0 { 0 } else { code.wrapping_sub(SEG_BASE) as u8 };
            let ok = code == 0;
            slot.preload = if ok { info.preload as u8 } else { 0 };
            slot.libpath = if ok { info.libpath as u8 } else { 0 };
            slot.overrides = if ok { info.overrides as u8 } else { 0 };
            slot.unknown = if ok { info.unknown as u8 } else { 0 };
            slot.non_ld = if ok { info.non_ld as u8 } else { 0 };
            slot.secure_ignored = if ok { info.secure_ignored as u8 } else { 0 };
            let mut bits = 0u8;
            if ok {
                if info.debug {
                    bits |= 1;
                }
                if info.bind_now {
                    bits |= 2;
                }
                if info.trace {
                    bits |= 4;
                }
                if info.secure {
                    bits |= 8;
                }
            }
            slot.bits = bits;
            slot.seen = false;
            slot.spec = *c;
        }
        self.n = at + 1;
        self.accepted = self.accepted.wrapping_add(1);
        Ok(self.fold())
    }

    /// 登记一张环境表。
    pub fn register(&mut self, c: &EnvCase) -> Result<u64, u32> {
        if self.frozen {
            return Err(self.fail(EV_EFROZEN));
        }
        if self.duplicate(c) {
            return Err(self.fail(EV_EDUP));
        }
        match parse(c) {
            Ok(info) => self.commit(c, 0, &info),
            Err(code) => Err(self.fail(code)),
        }
    }

    /// 登记并核对期望码。
    pub fn register_ruled(&mut self, c: &EnvCase, want: u32) -> Result<u64, u32> {
        if self.frozen {
            return Err(self.fail(EV_EFROZEN));
        }
        if self.duplicate(c) {
            return Err(self.fail(EV_EDUP));
        }
        match parse(c) {
            Ok(info) => {
                if want != 0 {
                    return Err(self.fail(EV_EMISMATCH));
                }
                self.commit(c, 0, &info)
            }
            Err(code) => {
                if want == 0 || want != code {
                    return Err(self.fail(EV_EMISMATCH));
                }
                self.commit(c, code, &EnvInfo::none())
            }
        }
    }
}

impl EnvLedger {
    /// 观测一格。
    pub fn observe(&mut self, id: usize) -> Result<u64, u32> {
        if self.frozen {
            return Err(self.fail(EV_EFROZEN));
        }
        if id >= self.n {
            return Err(self.fail(EV_ENOBS));
        }
        let got = match self.slots.get(id) {
            Some(s) => *s,
            None => return Err(self.fail(EV_ENOBS)),
        };
        if got.seen {
            return Err(self.fail(EV_EDUP));
        }
        let (wc, wp, wl, wo, wu, wn, wg, wd, wb, wt) = expected_of(id);
        self.observed = self.observed.wrapping_add(1);
        if let Some(slot) = self.slots.get_mut(id) {
            slot.seen = true;
        }
        let code_ok = if wc == 0 {
            got.code == 0
        } else {
            got.code == (wc.wrapping_sub(SEG_BASE)) as u8
        };
        let d = got.bits & 1 != 0;
        let b = got.bits & 2 != 0;
        let t = got.bits & 4 != 0;
        if !code_ok
            || got.preload as u32 != wp
            || got.libpath as u32 != wl
            || got.overrides as u32 != wo
            || got.unknown as u32 != wu
            || got.non_ld as u32 != wn
            || got.secure_ignored as u32 != wg
            || d != wd
            || b != wb
            || t != wt
        {
            return Err(self.fail(EV_EMISMATCH));
        }
        self.matched = self.matched.wrapping_add(1);
        Ok(self.fold())
    }

    /// 已登记格数。
    pub fn len(&self) -> usize {
        self.n
    }

    /// 账本是否为空。
    pub fn is_empty(&self) -> bool {
        self.n == 0
    }

    /// 成功登记次数。
    pub fn accepted(&self) -> u32 {
        self.accepted
    }

    /// 拒绝次数。
    pub fn rejected(&self) -> u32 {
        self.rejected
    }

    /// 观测次数。
    pub fn observed(&self) -> u32 {
        self.observed
    }

    /// 观测通过次数。
    pub fn matched(&self) -> u32 {
        self.matched
    }

    /// 冻结标志。
    pub fn is_frozen(&self) -> bool {
        self.frozen
    }

    /// 记录到非零错误码的格数。
    pub fn errors(&self) -> u32 {
        let mut c = 0u32;
        let mut i = 0usize;
        while i < self.n {
            if let Some(s) = self.slots.get(i) {
                if s.code != 0 {
                    c = c.wrapping_add(1);
                }
            }
            i += 1;
        }
        c
    }
}

impl EnvLedger {
    fn sum_field<F: Fn(&EnvSlot) -> u32>(&self, f: F) -> u32 {
        let mut t = 0u32;
        let mut i = 0usize;
        while i < self.n {
            if let Some(s) = self.slots.get(i) {
                if s.code == 0 {
                    t = t.wrapping_add(f(s));
                }
            }
            i += 1;
        }
        t
    }

    fn count_field<F: Fn(&EnvSlot) -> bool>(&self, f: F) -> u32 {
        let mut t = 0u32;
        let mut i = 0usize;
        while i < self.n {
            if let Some(s) = self.slots.get(i) {
                if s.code == 0 && f(s) {
                    t = t.wrapping_add(1);
                }
            }
            i += 1;
        }
        t
    }

    /// 成功样本预加载对象总数。
    pub fn preload_total(&self) -> u32 {
        self.sum_field(|s| s.preload as u32)
    }

    /// 成功样本库目录总数。
    pub fn libpath_total(&self) -> u32 {
        self.sum_field(|s| s.libpath as u32)
    }

    /// 覆盖次数总和。
    pub fn overrides_total(&self) -> u32 {
        self.sum_field(|s| s.overrides as u32)
    }

    /// 未知 LD_ 变量总数。
    pub fn unknown_total(&self) -> u32 {
        self.sum_field(|s| s.unknown as u32)
    }

    /// 非 LD_ 条目总数。
    pub fn non_ld_total(&self) -> u32 {
        self.sum_field(|s| s.non_ld as u32)
    }

    /// 安全模式忽略次数总和。
    pub fn secure_ignored_total(&self) -> u32 {
        self.sum_field(|s| s.secure_ignored as u32)
    }

    /// 开启调试的样本数。
    pub fn debug_count(&self) -> u32 {
        self.count_field(|s| s.bits & 1 != 0)
    }

    /// 立即绑定的样本数。
    pub fn bind_count(&self) -> u32 {
        self.count_field(|s| s.bits & 2 != 0)
    }

    /// 只列依赖的样本数。
    pub fn trace_count(&self) -> u32 {
        self.count_field(|s| s.bits & 4 != 0)
    }

    /// 预加载真正生效的样本数。
    pub fn preload_active(&self) -> u32 {
        self.count_field(|s| s.bits & 8 == 0 && s.preload > 0)
    }

    /// 库路径真正生效的样本数。
    pub fn libpath_active(&self) -> u32 {
        self.count_field(|s| s.bits & 8 == 0 && s.libpath > 0)
    }

    /// 调试真正生效的样本数。
    pub fn debug_active(&self) -> u32 {
        self.count_field(|s| s.bits & 8 == 0 && s.bits & 1 != 0)
    }

    /// 只列依赖真正生效的样本数。
    pub fn trace_active(&self) -> u32 {
        self.count_field(|s| s.bits & 8 == 0 && s.bits & 4 != 0)
    }

    /// 立即绑定生效的样本数（安全模式也生效）。
    pub fn bind_active(&self) -> u32 {
        self.count_field(|s| s.bits & 2 != 0)
    }

    /// 逐格重解析并核对登记值。
    pub fn audit_all(&self) -> bool {
        let mut i = 0usize;
        while i < self.n {
            if let Some(s) = self.slots.get(i) {
                if s.code == 0 {
                    let info = match parse(&s.spec) {
                        Ok(v) => v,
                        Err(_) => return false,
                    };
                    if info.preload != s.preload as u32
                        || info.libpath != s.libpath as u32
                        || info.overrides != s.overrides as u32
                        || info.unknown != s.unknown as u32
                        || info.non_ld != s.non_ld as u32
                        || info.secure_ignored != s.secure_ignored as u32
                        || info.debug != (s.bits & 1 != 0)
                        || info.bind_now != (s.bits & 2 != 0)
                        || info.trace != (s.bits & 4 != 0)
                        || info.secure != (s.bits & 8 != 0)
                    {
                        return false;
                    }
                }
            }
            i += 1;
        }
        true
    }
}

impl EnvLedger {
    /// 冻结核对：全登记、全观测、全匹配、族计数与生效位。
    pub fn replay(&mut self) -> Result<u64, u32> {
        if self.n != SAMPLES || self.accepted != SAMPLES as u32 {
            return Err(self.fail(EV_ENOBS));
        }
        if self.observed != SAMPLES as u32 || self.matched != SAMPLES as u32 {
            return Err(self.fail(EV_EMISMATCH));
        }
        if !self.audit_all() {
            return Err(self.fail(EV_EBAL));
        }
        if self.errors() != 2
            || self.preload_total() != 8
            || self.libpath_total() != 6
            || self.overrides_total() != 1
            || self.unknown_total() != 1
            || self.non_ld_total() != 1
            || self.secure_ignored_total() != 4
            || self.debug_count() != 4
            || self.bind_count() != 3
            || self.trace_count() != 3
            || self.preload_active() != 4
            || self.libpath_active() != 2
            || self.debug_active() != 3
            || self.trace_active() != 2
            || self.bind_active() != 3
        {
            return Err(self.fail(EV_EBAL));
        }
        Ok(self.fold())
    }

    /// 冻结：先重放再上锁。
    pub fn freeze(&mut self) -> Result<u64, u32> {
        if self.frozen {
            return Err(self.fail(EV_EFROZEN));
        }
        self.replay()?;
        self.frozen = true;
        Ok(self.fold())
    }

    /// 对全部字段做 FNV-1a 折叠。
    pub fn fold(&self) -> u64 {
        let mut h = FNV_OFF;
        h = mix(h, self.n as u64);
        h = mix(h, self.accepted as u64);
        h = mix(h, self.rejected as u64);
        h = mix(h, self.observed as u64);
        h = mix(h, self.matched as u64);
        h = mix(h, if self.frozen { 1 } else { 0 });
        let mut i = 0usize;
        while i < SAMPLES {
            if let Some(s) = self.slots.get(i) {
                h = mix(h, s.code as u64);
                h = mix(h, s.preload as u64);
                h = mix(h, s.libpath as u64);
                h = mix(h, s.overrides as u64);
                h = mix(h, s.unknown as u64);
                h = mix(h, s.non_ld as u64);
                h = mix(h, s.secure_ignored as u64);
                h = mix(h, s.bits as u64);
                h = mix(h, if s.seen { 1 } else { 0 });
                let mut r = 0usize;
                while r < ENV_MAX {
                    if let Some(l) = s.spec.len.get(r) {
                        h = mix(h, *l as u64);
                        let mut k = 0usize;
                        while k < ENTRY_MAX {
                            if let Some(row) = s.spec.buf.get(r) {
                                if let Some(v) = row.get(k) {
                                    h = mix(h, *v as u64);
                                }
                            }
                            k += 1;
                        }
                    }
                    r += 1;
                }
                h = mix(h, s.spec.n as u64);
                h = mix(h, if s.spec.secure { 1 } else { 0 });
            }
            i += 1;
        }
        h
    }
}

fn mix(mut h: u64, v: u64) -> u64 {
    let mut i = 0u32;
    while i < 8 {
        h ^= (v >> (i * 8)) & 0xff;
        h = h.wrapping_mul(FNV_PRIME);
        i += 1;
    }
    h
}

/// 构建基准账本。
pub fn build_ledger() -> Result<EnvLedger, u32> {
    let mut l = EnvLedger::new();
    let mut i = 0usize;
    while i < SAMPLES {
        let (want, _, _, _, _, _, _, _, _, _) = expected_of(i);
        l.register_ruled(&spec_of(i), want)?;
        i += 1;
    }
    let mut j = 0usize;
    while j < SAMPLES {
        l.observe(j)?;
        j += 1;
    }
    Ok(l)
}

/// 四路独立注入：越序、重复、越界、冻结。
pub fn inject_four() -> [bool; 4] {
    let mut ok = [false; 4];
    let c0 = spec_of(0);

    let mut a = EnvLedger::new();
    match a.observe(0) {
        Err(code) if code == EV_ENOBS => ok[0] = a.rejected() == 1 && a.len() == 0,
        _ => {}
    }

    let mut b = EnvLedger::new();
    let _ = b.register(&c0);
    match b.register(&c0) {
        Err(code) if code == EV_EDUP => ok[1] = b.rejected() == 1 && b.len() == 1,
        _ => {}
    }

    let mut d = EnvLedger::new();
    let mut i = 0usize;
    while i < SAMPLES {
        let (want, _, _, _, _, _, _, _, _, _) = expected_of(i);
        let _ = d.register_ruled(&spec_of(i), want);
        i += 1;
    }
    let mut extra = EnvCase::blank();
    set_entry(&mut extra, "LD_OTHER=1");
    match d.register(&extra) {
        Err(code) if code == EV_EID => ok[2] = d.rejected() == 1 && d.len() == SAMPLES,
        _ => {}
    }

    if let Ok(mut l) = build_ledger() {
        if l.freeze().is_ok() {
            match l.register(&c0) {
                Err(code) if code == EV_EFROZEN => ok[3] = l.is_frozen() && l.rejected() == 1,
                _ => {}
            }
        }
    }
    ok
}

/// 十三项自检。
pub fn run_unx11257_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F11257");

    let fresh = EnvLedger::new();
    s.add(
        "EV1-建场静默",
        fresh.len() == 0
            && fresh.is_empty()
            && fresh.rejected() == 0
            && fresh.observed() == 0
            && !fresh.is_frozen(),
        "新账本零格零拒绝",
    );

    let e2 = match (parse(&spec_of(0)), parse(&spec_of(1))) {
        (Ok(a), Ok(b)) => a.preload == 2 && a.preload_active() && b.preload == 3,
        _ => false,
    };
    s.add("EV2-PRELOAD列表", e2, "冒号与空格两种分隔都计数");

    let e3 = match parse(&spec_of(2)) {
        Ok(a) => a.libpath == 3 && a.libpath_active(),
        Err(_) => false,
    };
    let mut dense = EnvCase::blank();
    set_entry(&mut dense, "LD_LIBRARY_PATH=/a::/b");
    let dense_ok = match parse(&dense) {
        Ok(a) => a.libpath == 2,
        Err(_) => false,
    };
    s.add("EV3-LIBPATH目录数", e3 && dense_ok, "连续冒号不产生空目录");

    let e4 = match parse(&spec_of(3)) {
        Ok(a) => a.debug && a.debug_active(),
        Err(_) => false,
    };
    let mut blank_dbg = EnvCase::blank();
    set_entry(&mut blank_dbg, "LD_DEBUG=");
    let blank_ok = match parse(&blank_dbg) {
        Ok(a) => !a.debug,
        Err(_) => false,
    };
    s.add("EV4-DEBUG开关", e4 && blank_ok, "空值视为关闭");

    let e5 = match parse(&spec_of(4)) {
        Ok(a) => a.bind_now && a.bind_active(),
        Err(_) => false,
    };
    s.add("EV5-BIND_NOW开关", e5, "非空值开启立即绑定");

    let e6 = match parse(&spec_of(5)) {
        Ok(a) => a.trace && a.trace_active(),
        Err(_) => false,
    };
    s.add("EV6-TRACE开关", e6, "非空值进入只列依赖模式");

    let e7 = match (parse(&spec_of(6)), parse(&spec_of(7)), parse(&spec_of(8))) {
        (Ok(a), Ok(b), Ok(c)) => {
            a.unknown == 1
                && a.preload == 1
                && a.libpath == 2
                && b.overrides == 1
                && b.debug
                && c.non_ld == 1
                && c.preload == 1
        }
        _ => false,
    };
    s.add("EV7-覆盖与未知族", e7, "同名后者覆盖 未知LD_与非LD分别记账");

    let e8 = match parse(&spec_of(11)) {
        Ok(a) => {
            a.secure
                && a.secure_ignored == 4
                && !a.preload_active()
                && !a.libpath_active()
                && !a.debug_active()
                && !a.trace_active()
                && a.bind_active()
        }
        Err(_) => false,
    };
    s.add("EV8-安全模式四忽略一保留", e8, "PRELOAD/LIBPATH/DEBUG/TRACE忽略 BIND_NOW保留");

    let mut noname = EnvCase::blank();
    set_entry(&mut noname, "=x");
    let e9 = parse(&spec_of(9)) == Err(EV_EFORMAT) && parse(&noname) == Err(EV_EFORMAT);
    s.add("EV9-格式拒绝", e9, "缺等号或名字为空均拒绝");

    s.add(
        "EV10-空表拒绝",
        parse(&spec_of(10)) == Err(EV_EEMPTY),
        "零条目视为空环境",
    );

    let built = build_ledger();
    let (e11, e12) = match &built {
        Ok(l) => {
            let r = l.len() == SAMPLES
                && l.accepted() == SAMPLES as u32
                && l.rejected() == 0
                && l.errors() == 2
                && l.preload_total() == 8
                && l.libpath_total() == 6
                && l.overrides_total() == 1
                && l.unknown_total() == 1
                && l.non_ld_total() == 1
                && l.secure_ignored_total() == 4
                && l.debug_count() == 4
                && l.bind_count() == 3
                && l.trace_count() == 3
                && l.preload_active() == 4
                && l.libpath_active() == 2
                && l.debug_active() == 3
                && l.trace_active() == 2
                && l.bind_active() == 3
                && l.audit_all();
            let mut m = match build_ledger() {
                Ok(v) => v,
                Err(_) => EnvLedger::new(),
            };
            let f = match m.freeze() {
                Ok(_) => m.is_frozen() && m.freeze() == Err(EV_EFROZEN) && m.rejected() == 1,
                Err(_) => false,
            };
            (r, f)
        }
        Err(_) => (false, false),
    };
    s.add("EV11-族计数与生效位", e11, "十二格原始计数与安全模式生效位全部吻合");
    s.add("EV12-冻结后二次拒绝", e12, "冻结成功且再次冻结返回错误");

    s.add(
        "EV13-四路注入",
        inject_four() == [true, true, true, true],
        "越序 重复 越界 冻结四条拒绝路径均触发",
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preload_and_libpath_lists() {
        let a = match parse(&spec_of(0)) {
            Ok(v) => v,
            Err(_) => EnvInfo::none(),
        };
        assert_eq!(a.preload, 2);
        assert!(a.preload_active());
        let b = match parse(&spec_of(2)) {
            Ok(v) => v,
            Err(_) => EnvInfo::none(),
        };
        assert_eq!(b.libpath, 3);
        assert!(b.libpath_active());
        let mut t = EnvCase::blank();
        set_entry(&mut t, "LD_PRELOAD=:a::b ");
        let c = match parse(&t) {
            Ok(v) => v,
            Err(_) => EnvInfo::none(),
        };
        assert_eq!(c.preload, 2);
    }

    #[test]
    fn secure_mode_exception() {
        let a = match parse(&spec_of(11)) {
            Ok(v) => v,
            Err(_) => EnvInfo::none(),
        };
        assert!(a.secure);
        assert_eq!(a.secure_ignored, 4);
        assert!(!a.preload_active());
        assert!(!a.libpath_active());
        assert!(!a.debug_active());
        assert!(!a.trace_active());
        assert!(a.bind_active());
        assert_eq!(a.preload, 1);
    }

    #[test]
    fn duplicate_debug_last_wins() {
        let a = match parse(&spec_of(7)) {
            Ok(v) => v,
            Err(_) => EnvInfo::none(),
        };
        assert_eq!(a.overrides, 1);
        assert!(a.debug);
        assert_eq!(a.preload, 0);
    }

    #[test]
    fn format_refusals() {
        assert_eq!(parse(&spec_of(9)), Err(EV_EFORMAT));
        assert_eq!(parse(&spec_of(10)), Err(EV_EEMPTY));
        let mut noname = EnvCase::blank();
        set_entry(&mut noname, "=x");
        assert_eq!(parse(&noname), Err(EV_EFORMAT));
        let mut big = EnvCase::blank();
        big.n = (ENV_MAX + 1) as u8;
        assert_eq!(parse(&big), Err(EV_ETRUNC));
    }

    #[test]
    fn ledger_totals_and_freeze() {
        let mut l = match build_ledger() {
            Ok(v) => v,
            Err(_) => EnvLedger::new(),
        };
        assert_eq!(l.len(), SAMPLES);
        assert_eq!(l.errors(), 2);
        assert_eq!(l.preload_total(), 8);
        assert_eq!(l.libpath_total(), 6);
        assert_eq!(l.debug_count(), 4);
        assert_eq!(l.secure_ignored_total(), 4);
        assert_eq!(l.preload_active(), 4);
        assert_eq!(l.bind_active(), 3);
        assert!(l.audit_all());
        let h = match l.freeze() {
            Ok(v) => v,
            Err(_) => 0,
        };
        assert_eq!(l.replay(), Ok(h));
        assert_eq!(l.freeze(), Err(EV_EFROZEN));
    }

    #[test]
    fn injections_and_duplicates() {
        assert_eq!(inject_four(), [true, true, true, true]);
        let mut l = EnvLedger::new();
        let c = spec_of(3);
        assert!(l.register(&c).is_ok());
        assert_eq!(l.register(&c), Err(EV_EDUP));
        assert_eq!(l.observe(1), Err(EV_ENOBS));
        assert_eq!(l.rejected(), 2);
    }
}
