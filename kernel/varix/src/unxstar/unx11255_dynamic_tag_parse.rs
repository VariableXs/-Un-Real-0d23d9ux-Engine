//! UNX-F11255 ELF 动态段解析（DT_ 项族）（0x1f360 段）
//!
//! 模型直接吃一串 (d_tag, d_un) 对以及一张字符串表，不依赖真实 ELF 文件。
//! 已知项只处理装载器真正消费的几位：NEEDED 可以重复并累加计数；
//! SONAME、RPATH、RUNPATH 是单例，第二次出现视为重复并拒绝；FLAGS_1
//! 只记录 NOW 与 PIE 两个位，其余位计入未知位而不报错。未知的 d_tag
//! 一律忽略，但要在账本里留下计数，便于证明“忽略语义一致”。
//! DT_NULL 必须出现，NULL 之后再出现的对全部记为尾部忽略项；缺少
//! 终止符则整段拒绝。字符串偏移必须落在表内且能找到 NUL 结尾。
//! 动态搜索时 RUNPATH 一旦存在就压过 RPATH，二者并存不算错误，但
//! 生效路径只取 RUNPATH，这一点由 effective 接口单独验证。
//! 账本登记十二个样本并观测，冻结前重放全部计数恒等式。

use crate::checks::CheckSet;

/// 段基址。
pub const SEG_BASE: u32 = 0x1f360;

/// 编号非法。
pub const DY_EID: u32 = SEG_BASE + 0x01;
/// 状态顺序错误。
pub const DY_EORDER: u32 = SEG_BASE + 0x02;
/// 重复登记。
pub const DY_EDUP: u32 = SEG_BASE + 0x03;
/// 已冻结。
pub const DY_EFROZEN: u32 = SEG_BASE + 0x04;
/// 观测不存在的格。
pub const DY_ENOBS: u32 = SEG_BASE + 0x05;
/// 观测结果不符。
pub const DY_EMISMATCH: u32 = SEG_BASE + 0x06;
/// 账本失衡。
pub const DY_EBAL: u32 = SEG_BASE + 0x07;
/// 空动态段。
pub const DY_EEMPTY: u32 = SEG_BASE + 0x08;
/// 对数越界。
pub const DY_ETRUNC: u32 = SEG_BASE + 0x09;
/// 缺少 DT_NULL。
pub const DY_ENOTERM: u32 = SEG_BASE + 0x0a;
/// 字符串偏移非法。
pub const DY_EOFF: u32 = SEG_BASE + 0x0b;
/// 单例标签重复。
pub const DY_ESONAME: u32 = SEG_BASE + 0x0c;

/// DT_NULL 终止符。
pub const DT_NULL: u32 = 0;
/// 依赖项。
pub const DT_NEEDED: u32 = 1;
/// 共享对象名。
pub const DT_SONAME: u32 = 14;
/// 旧式搜索路径。
pub const DT_RPATH: u32 = 15;
/// 新式搜索路径。
pub const DT_RUNPATH: u32 = 29;
/// 标志位一。
pub const DT_FLAGS_1: u32 = 0x6fff_fffb;

/// DF_1_NOW。
pub const DF_1_NOW: u32 = 0x0000_0001;
/// DF_1_NODELETE。
pub const DF_1_NODELETE: u32 = 0x0000_0008;
/// DF_1_PIE。
pub const DF_1_PIE: u32 = 0x0800_0000;
/// 已知 FLAGS_1 位。
pub const DF_1_KNOWN: u32 = DF_1_NOW | DF_1_NODELETE | DF_1_PIE;

/// 单段最多标签对。
pub const MAX_PAIRS: usize = 12;
/// 字符串表容量。
pub const STR_MAX: usize = 48;
/// 样本个数。
pub const SAMPLES: usize = 12;

/// FNV 偏移基。
pub const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;
/// FNV 质数。
pub const FNV_PRIME: u64 = 0x100_0000_01b3;

/// 一个动态段样本。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DynCase {
    pub tag: [u32; MAX_PAIRS],
    pub val: [u64; MAX_PAIRS],
    pub n: u8,
    pub strtab: [u8; STR_MAX],
    pub slen: u8,
}

impl DynCase {
    /// 空样本。
    pub const fn blank() -> DynCase {
        DynCase {
            tag: [0; MAX_PAIRS],
            val: [0; MAX_PAIRS],
            n: 0,
            strtab: [0; STR_MAX],
            slen: 0,
        }
    }
}

/// 追加一对标签；越界时静默丢弃，由解析器按 n 判断截断。
pub fn push_pair(c: &mut DynCase, tag: u32, val: u64) {
    let at = c.n as usize;
    if at < MAX_PAIRS {
        if let Some(t) = c.tag.get_mut(at) {
            *t = tag;
        }
        if let Some(v) = c.val.get_mut(at) {
            *v = val;
        }
        c.n = c.n.wrapping_add(1);
    }
}

/// 把字符串写入表内偏移处，末尾补 NUL。
pub fn set_str(c: &mut DynCase, off: usize, s: &str) {
    let bytes = s.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        let at = off.wrapping_add(i);
        if at >= STR_MAX {
            return;
        }
        if let Some(b) = c.strtab.get_mut(at) {
            *b = match bytes.get(i) {
                Some(v) => *v,
                None => 0,
            };
        }
        i += 1;
    }
    let nul = off.wrapping_add(bytes.len());
    if nul < STR_MAX {
        if let Some(b) = c.strtab.get_mut(nul) {
            *b = 0;
        }
    }
    let end = nul.wrapping_add(1);
    if end > STR_MAX {
        c.slen = STR_MAX as u8;
    } else if end as u8 > c.slen {
        c.slen = end as u8;
    }
}

/// 解析后的动态段摘要。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DynInfo {
    pub needed: u32,
    pub soname_off: u32,
    pub soname_len: u32,
    pub rpath_off: u32,
    pub rpath_len: u32,
    pub runpath_off: u32,
    pub runpath_len: u32,
    pub flags1: u32,
    pub unknown: u32,
    pub ignored: u32,
    pub has_soname: bool,
    pub has_rpath: bool,
    pub has_runpath: bool,
    pub pie: bool,
    pub now: bool,
    pub unknown_flags: u32,
}

impl DynInfo {
    /// 空摘要。
    pub const fn none() -> DynInfo {
        DynInfo {
            needed: 0,
            soname_off: 0,
            soname_len: 0,
            rpath_off: 0,
            rpath_len: 0,
            runpath_off: 0,
            runpath_len: 0,
            flags1: 0,
            unknown: 0,
            ignored: 0,
            has_soname: false,
            has_rpath: false,
            has_runpath: false,
            pie: false,
            now: false,
            unknown_flags: 0,
        }
    }
}

/// 从字符串表取以 NUL 结尾的字节串。
pub fn cstr(c: &DynCase, off: u64) -> Result<&[u8], u32> {
    let at = off as usize;
    if at >= c.slen as usize {
        return Err(DY_EOFF);
    }
    let mut end = at;
    while end < c.slen as usize {
        match c.strtab.get(end) {
            Some(0) => {
                return match c.strtab.get(at..end) {
                    Some(v) => Ok(v),
                    None => Err(DY_EOFF),
                }
            }
            Some(_) => end += 1,
            None => return Err(DY_EOFF),
        }
    }
    Err(DY_EOFF)
}

/// 解析动态段。
pub fn parse(c: &DynCase) -> Result<DynInfo, u32> {
    let n = c.n as usize;
    if n == 0 {
        return Err(DY_EEMPTY);
    }
    if n > MAX_PAIRS {
        return Err(DY_ETRUNC);
    }
    let mut info = DynInfo::none();
    let mut i = 0usize;
    let mut terminated = false;
    while i < n {
        let tag = match c.tag.get(i) {
            Some(v) => *v,
            None => return Err(DY_ETRUNC),
        };
        let val = match c.val.get(i) {
            Some(v) => *v,
            None => return Err(DY_ETRUNC),
        };
        if tag == DT_NULL {
            terminated = true;
            i += 1;
            break;
        }
        match tag {
            DT_NEEDED => {
                cstr(c, val)?;
                info.needed = info.needed.wrapping_add(1);
            }
            DT_SONAME => {
                if info.has_soname {
                    return Err(DY_ESONAME);
                }
                let s = cstr(c, val)?;
                info.soname_off = val as u32;
                info.soname_len = s.len() as u32;
                info.has_soname = true;
            }
            DT_RPATH => {
                if info.has_rpath {
                    return Err(DY_ESONAME);
                }
                let s = cstr(c, val)?;
                info.rpath_off = val as u32;
                info.rpath_len = s.len() as u32;
                info.has_rpath = true;
            }
            DT_RUNPATH => {
                if info.has_runpath {
                    return Err(DY_ESONAME);
                }
                let s = cstr(c, val)?;
                info.runpath_off = val as u32;
                info.runpath_len = s.len() as u32;
                info.has_runpath = true;
            }
            DT_FLAGS_1 => {
                info.flags1 = val as u32;
            }
            _ => {
                info.unknown = info.unknown.wrapping_add(1);
            }
        }
        i += 1;
    }
    if !terminated {
        return Err(DY_ENOTERM);
    }
    info.ignored = (n - i) as u32;
    info.pie = info.flags1 & DF_1_PIE != 0;
    info.now = info.flags1 & DF_1_NOW != 0;
    info.unknown_flags = info.flags1 & !DF_1_KNOWN;
    Ok(info)
}

/// 生效的搜索路径：RUNPATH 优先于 RPATH，都没有则空串。
pub fn effective(c: &DynCase) -> Result<&[u8], u32> {
    let info = parse(c)?;
    if info.has_runpath {
        if let Some(v) = c.strtab.get(info.runpath_off as usize..(info.runpath_off + info.runpath_len) as usize) {
            return Ok(v);
        }
        return Err(DY_EOFF);
    }
    if info.has_rpath {
        if let Some(v) = c.strtab.get(info.rpath_off as usize..(info.rpath_off + info.rpath_len) as usize) {
            return Ok(v);
        }
        return Err(DY_EOFF);
    }
    Ok(&[])
}

/// 样本规格。字符串按固定偏移摆放，避免互相覆盖。
pub fn spec_of(id: usize) -> DynCase {
    let mut c = DynCase::blank();
    match id {
        0 => {
            set_str(&mut c, 0, "libc.so.6");
            set_str(&mut c, 12, "libfoo.so");
            push_pair(&mut c, DT_NEEDED, 0);
            push_pair(&mut c, DT_SONAME, 12);
            push_pair(&mut c, DT_NULL, 0);
        }
        1 => {
            set_str(&mut c, 0, "liba.so");
            set_str(&mut c, 10, "libb.so");
            push_pair(&mut c, DT_NEEDED, 0);
            push_pair(&mut c, DT_NEEDED, 10);
            push_pair(&mut c, DT_NULL, 0);
        }
        2 => {
            set_str(&mut c, 0, "/opt/lib");
            push_pair(&mut c, DT_RPATH, 0);
            push_pair(&mut c, DT_NULL, 0);
        }
        3 => {
            set_str(&mut c, 0, "/opt/lib");
            set_str(&mut c, 10, "/run/lib");
            push_pair(&mut c, DT_RPATH, 0);
            push_pair(&mut c, DT_RUNPATH, 10);
            push_pair(&mut c, DT_NULL, 0);
        }
        4 => {
            push_pair(&mut c, DT_FLAGS_1, (DF_1_NOW | DF_1_PIE) as u64);
            push_pair(&mut c, DT_NULL, 0);
        }
        5 => {
            push_pair(&mut c, 0x1234_5678, 0x99);
            push_pair(&mut c, DT_NULL, 0);
        }
        6 => {
            set_str(&mut c, 0, "libc.so.6");
            push_pair(&mut c, DT_NEEDED, 0);
        }
        7 => {
            set_str(&mut c, 0, "libc.so.6");
            push_pair(&mut c, DT_NEEDED, 40);
            push_pair(&mut c, DT_NULL, 0);
        }
        8 => {
            set_str(&mut c, 0, "a.so");
            set_str(&mut c, 10, "b.so");
            push_pair(&mut c, DT_SONAME, 0);
            push_pair(&mut c, DT_SONAME, 10);
            push_pair(&mut c, DT_NULL, 0);
        }
        9 => {
            push_pair(&mut c, DT_NULL, 0);
            push_pair(&mut c, DT_NEEDED, 0);
            push_pair(&mut c, DT_FLAGS_1, 1);
            push_pair(&mut c, 0x1234, 0);
        }
        10 => {}
        11 => {
            push_pair(&mut c, DT_FLAGS_1, DF_1_NOW as u64);
            push_pair(&mut c, DT_NULL, 0);
        }
        _ => {}
    }
    c
}

/// 样本期望：(错误码, NEEDED数, SONAME长度, RPATH长度, RUNPATH长度,
/// FLAGS_1, 未知标签数, NULL后忽略数)。
pub fn expected_of(id: usize) -> (u32, u32, u32, u32, u32, u32, u32, u32) {
    match id {
        0 => (0, 1, 9, 0, 0, 0, 0, 0),
        1 => (0, 2, 0, 0, 0, 0, 0, 0),
        2 => (0, 0, 0, 8, 0, 0, 0, 0),
        3 => (0, 0, 0, 8, 8, 0, 0, 0),
        4 => (0, 0, 0, 0, 0, DF_1_NOW | DF_1_PIE, 0, 0),
        5 => (0, 0, 0, 0, 0, 0, 1, 0),
        6 => (DY_ENOTERM, 0, 0, 0, 0, 0, 0, 0),
        7 => (DY_EOFF, 0, 0, 0, 0, 0, 0, 0),
        8 => (DY_ESONAME, 0, 0, 0, 0, 0, 0, 0),
        9 => (0, 0, 0, 0, 0, 0, 0, 3),
        10 => (DY_EEMPTY, 0, 0, 0, 0, 0, 0, 0),
        11 => (0, 0, 0, 0, 0, DF_1_NOW, 0, 0),
        _ => (DY_EID, 0, 0, 0, 0, 0, 0, 0),
    }
}

/// 账本中的一格。
#[derive(Clone, Copy)]
struct DynSlot {
    code: u8,
    needed: u8,
    soname_len: u8,
    rpath_len: u8,
    runpath_len: u8,
    unknown: u8,
    ignored: u8,
    seen: bool,
    flags1: u32,
    spec: DynCase,
}

impl DynSlot {
    const fn zero() -> DynSlot {
        DynSlot {
            code: 0,
            needed: 0,
            soname_len: 0,
            rpath_len: 0,
            runpath_len: 0,
            unknown: 0,
            ignored: 0,
            seen: false,
            flags1: 0,
            spec: DynCase::blank(),
        }
    }
}

/// 动态段解析账本。
pub struct DynLedger {
    slots: [DynSlot; SAMPLES],
    n: usize,
    accepted: u32,
    rejected: u32,
    observed: u32,
    matched: u32,
    frozen: bool,
}

impl DynLedger {
    /// 空账本。
    pub const fn new() -> DynLedger {
        DynLedger {
            slots: [DynSlot::zero(); SAMPLES],
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

    fn duplicate(&self, c: &DynCase) -> bool {
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

    fn commit(&mut self, c: &DynCase, code: u32, info: &DynInfo) -> Result<u64, u32> {
        let at = self.n;
        if at >= SAMPLES {
            return Err(self.fail(DY_EID));
        }
        if let Some(slot) = self.slots.get_mut(at) {
            slot.code = if code == 0 { 0 } else { code.wrapping_sub(SEG_BASE) as u8 };
            slot.needed = info.needed as u8;
            slot.soname_len = info.soname_len as u8;
            slot.rpath_len = info.rpath_len as u8;
            slot.runpath_len = info.runpath_len as u8;
            slot.unknown = info.unknown as u8;
            slot.ignored = info.ignored as u8;
            slot.flags1 = info.flags1;
            slot.seen = false;
            slot.spec = *c;
        }
        self.n = at + 1;
        self.accepted = self.accepted.wrapping_add(1);
        Ok(self.fold())
    }

    /// 登记一个动态段样本。
    pub fn register(&mut self, c: &DynCase) -> Result<u64, u32> {
        if self.frozen {
            return Err(self.fail(DY_EFROZEN));
        }
        if self.duplicate(c) {
            return Err(self.fail(DY_EDUP));
        }
        match parse(c) {
            Ok(info) => self.commit(c, 0, &info),
            Err(code) => Err(self.fail(code)),
        }
    }

    /// 登记并核对期望；期望失败也占一格。
    pub fn register_ruled(&mut self, c: &DynCase, want: u32) -> Result<u64, u32> {
        if self.frozen {
            return Err(self.fail(DY_EFROZEN));
        }
        if self.duplicate(c) {
            return Err(self.fail(DY_EDUP));
        }
        match parse(c) {
            Ok(info) => {
                if want != 0 {
                    return Err(self.fail(DY_EMISMATCH));
                }
                self.commit(c, 0, &info)
            }
            Err(code) => {
                if want == 0 || want != code {
                    return Err(self.fail(DY_EMISMATCH));
                }
                self.commit(c, code, &DynInfo::none())
            }
        }
    }

    /// 观测一格：错误码、六项计数与 FLAGS_1 必须全部一致。
    pub fn observe(&mut self, id: usize) -> Result<u64, u32> {
        if self.frozen {
            return Err(self.fail(DY_EFROZEN));
        }
        if id >= self.n {
            return Err(self.fail(DY_ENOBS));
        }
        let got = match self.slots.get(id) {
            Some(s) => *s,
            None => return Err(self.fail(DY_ENOBS)),
        };
        if got.seen {
            return Err(self.fail(DY_EDUP));
        }
        let (wc, wn, ws, wr, wu, wf, wx, wi) = expected_of(id);
        self.observed = self.observed.wrapping_add(1);
        if let Some(slot) = self.slots.get_mut(id) {
            slot.seen = true;
        }
        let code_ok = if wc == 0 {
            got.code == 0
        } else {
            got.code == (wc.wrapping_sub(SEG_BASE)) as u8
        };
        if !code_ok
            || got.needed as u32 != wn
            || got.soname_len as u32 != ws
            || got.rpath_len as u32 != wr
            || got.runpath_len as u32 != wu
            || got.flags1 != wf
            || got.unknown as u32 != wx
            || got.ignored as u32 != wi
        {
            return Err(self.fail(DY_EMISMATCH));
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

    /// 成功样本的 NEEDED 总数。
    pub fn total_needed(&self) -> u32 {
        self.sum_u8(|s| s.needed)
    }

    /// 带 SONAME 的样本数。
    pub fn soname_count(&self) -> u32 {
        self.count_ok(|s| s.soname_len > 0)
    }

    /// 带 RPATH 的样本数。
    pub fn rpath_count(&self) -> u32 {
        self.count_ok(|s| s.rpath_len > 0)
    }

    /// 带 RUNPATH 的样本数。
    pub fn runpath_count(&self) -> u32 {
        self.count_ok(|s| s.runpath_len > 0)
    }

    /// 带 DF_1_PIE 的样本数。
    pub fn pie_count(&self) -> u32 {
        self.count_ok(|s| s.flags1 & DF_1_PIE != 0)
    }

    /// 带 DF_1_NOW 的样本数。
    pub fn now_count(&self) -> u32 {
        self.count_ok(|s| s.flags1 & DF_1_NOW != 0)
    }

    /// 未知标签总数。
    pub fn unknown_total(&self) -> u32 {
        self.sum_u8(|s| s.unknown)
    }

    /// NULL 之后被忽略的对总数。
    pub fn ignored_total(&self) -> u32 {
        self.sum_u8(|s| s.ignored)
    }

    fn sum_u8<F: Fn(&DynSlot) -> u8>(&self, f: F) -> u32 {
        let mut t = 0u32;
        let mut i = 0usize;
        while i < self.n {
            if let Some(s) = self.slots.get(i) {
                if s.code == 0 {
                    t = t.wrapping_add(f(s) as u32);
                }
            }
            i += 1;
        }
        t
    }

    fn count_ok<F: Fn(&DynSlot) -> bool>(&self, f: F) -> u32 {
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

    /// 逐格重算生效路径：RUNPATH 优先，其次 RPATH，长度必须与登记一致。
    pub fn effective_ok(&self) -> bool {
        let mut i = 0usize;
        while i < self.n {
            if let Some(s) = self.slots.get(i) {
                if s.code == 0 {
                    let eff = match effective(&s.spec) {
                        Ok(v) => v,
                        Err(_) => return false,
                    };
                    let want = if s.runpath_len > 0 {
                        s.runpath_len
                    } else {
                        s.rpath_len
                    };
                    if eff.len() as u32 != want as u32 {
                        return false;
                    }
                    if s.runpath_len > 0 && s.rpath_len > 0 {
                        let info = match parse(&s.spec) {
                            Ok(v) => v,
                            Err(_) => return false,
                        };
                        if let Some(b) = s.spec.strtab.get(
                            info.runpath_off as usize
                                ..(info.runpath_off + info.runpath_len) as usize,
                        ) {
                            if eff != b {
                                return false;
                            }
                        }
                    }
                }
            }
            i += 1;
        }
        true
    }

    /// 冻结核对：全登记、全观测、全匹配、计数恒等式、生效路径可重算。
    pub fn replay(&mut self) -> Result<u64, u32> {
        if self.n != SAMPLES || self.accepted != SAMPLES as u32 {
            return Err(self.fail(DY_ENOBS));
        }
        if self.observed != SAMPLES as u32 || self.matched != SAMPLES as u32 {
            return Err(self.fail(DY_EMISMATCH));
        }
        if !self.effective_ok() {
            return Err(self.fail(DY_EBAL));
        }
        if self.errors() != 4
            || self.total_needed() != 3
            || self.soname_count() != 1
            || self.rpath_count() != 2
            || self.runpath_count() != 1
            || self.pie_count() != 1
            || self.now_count() != 2
            || self.unknown_total() != 1
            || self.ignored_total() != 3
        {
            return Err(self.fail(DY_EBAL));
        }
        Ok(self.fold())
    }

    /// 冻结：先重放再上锁。
    pub fn freeze(&mut self) -> Result<u64, u32> {
        if self.frozen {
            return Err(self.fail(DY_EFROZEN));
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
                h = mix(h, s.needed as u64);
                h = mix(h, s.soname_len as u64);
                h = mix(h, s.rpath_len as u64);
                h = mix(h, s.runpath_len as u64);
                h = mix(h, s.unknown as u64);
                h = mix(h, s.ignored as u64);
                h = mix(h, s.flags1 as u64);
                h = mix(h, if s.seen { 1 } else { 0 });
                let mut k = 0usize;
                while k < MAX_PAIRS {
                    if let Some(v) = s.spec.tag.get(k) {
                        h = mix(h, *v as u64);
                    }
                    if let Some(v) = s.spec.val.get(k) {
                        h = mix(h, *v);
                    }
                    k += 1;
                }
                let mut b = 0usize;
                while b < STR_MAX {
                    if let Some(v) = s.spec.strtab.get(b) {
                        h = mix(h, *v as u64);
                    }
                    b += 1;
                }
                h = mix(h, s.spec.n as u64);
                h = mix(h, s.spec.slen as u64);
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

/// 构建基准账本：登记十二个样本并逐一观测。
pub fn build_ledger() -> Result<DynLedger, u32> {
    let mut l = DynLedger::new();
    let mut i = 0usize;
    while i < SAMPLES {
        let (want, _, _, _, _, _, _, _) = expected_of(i);
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

    let mut a = DynLedger::new();
    match a.observe(0) {
        Err(code) if code == DY_ENOBS => ok[0] = a.rejected() == 1 && a.len() == 0,
        _ => {}
    }

    let mut b = DynLedger::new();
    let _ = b.register(&c0);
    match b.register(&c0) {
        Err(code) if code == DY_EDUP => ok[1] = b.rejected() == 1 && b.len() == 1,
        _ => {}
    }

    let mut d = DynLedger::new();
    let mut i = 0usize;
    while i < SAMPLES {
        let (want, _, _, _, _, _, _, _) = expected_of(i);
        let _ = d.register_ruled(&spec_of(i), want);
        i += 1;
    }
    let mut extra = DynCase::blank();
    push_pair(&mut extra, 0x55, 0);
    push_pair(&mut extra, DT_NULL, 0);
    match d.register(&extra) {
        Err(code) if code == DY_EID => ok[2] = d.rejected() == 1 && d.len() == SAMPLES,
        _ => {}
    }

    if let Ok(mut l) = build_ledger() {
        if l.freeze().is_ok() {
            match l.register(&c0) {
                Err(code) if code == DY_EFROZEN => ok[3] = l.is_frozen() && l.rejected() == 1,
                _ => {}
            }
        }
    }
    ok
}

/// 十三项自检。
pub fn run_unx11255_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F11255");

    let fresh = DynLedger::new();
    s.add(
        "D1-建场静默",
        fresh.len() == 0
            && fresh.is_empty()
            && fresh.rejected() == 0
            && fresh.observed() == 0
            && !fresh.is_frozen(),
        "新账本零格零拒绝",
    );

    let d2 = match (parse(&spec_of(0)), parse(&spec_of(1))) {
        (Ok(a), Ok(b)) => a.needed == 1 && b.needed == 2,
        _ => false,
    };
    s.add("D2-NEEDED累加", d2, "依赖项可重复出现并逐项计数");

    let d3 = match parse(&spec_of(0)) {
        Ok(v) => v.has_soname && v.soname_len == 9,
        Err(_) => false,
    };
    s.add(
        "D3-单例SONAME",
        d3 && parse(&spec_of(8)) == Err(DY_ESONAME),
        "SONAME只允许一次，重复即拒绝",
    );

    let d4 = effective(&spec_of(2)) == Ok(&b"/opt/lib"[..]);
    s.add("D4-RPATH生效", d4, "只有RPATH时搜索路径取RPATH");

    let d5 = match parse(&spec_of(3)) {
        Ok(v) => {
            v.has_rpath
                && v.has_runpath
                && v.rpath_len == 8
                && v.runpath_len == 8
                && effective(&spec_of(3)) == Ok(&b"/run/lib"[..])
        }
        Err(_) => false,
    };
    s.add("D5-RUNPATH压过RPATH", d5, "二者并存时生效路径只取RUNPATH");

    let d6 = match (parse(&spec_of(4)), parse(&spec_of(11))) {
        (Ok(a), Ok(b)) => {
            a.flags1 == DF_1_NOW | DF_1_PIE
                && a.pie
                && a.now
                && a.unknown_flags == 0
                && b.now
                && !b.pie
        }
        _ => false,
    };
    s.add("D6-FLAGS_1位语义", d6, "NOW与PIE两位分别识别");

    let d7 = match parse(&spec_of(5)) {
        Ok(v) => v.unknown == 1 && v.needed == 0 && v.flags1 == 0,
        Err(_) => false,
    };
    s.add("D7-未知标签忽略", d7, "未知d_tag计数后不参与解析");

    s.add(
        "D8-缺少终止符拒绝",
        parse(&spec_of(6)) == Err(DY_ENOTERM),
        "没有DT_NULL时整段拒绝",
    );
    s.add(
        "D9-字符串越界拒绝",
        parse(&spec_of(7)) == Err(DY_EOFF),
        "NEEDED偏移落在表外",
    );
    s.add(
        "D10-空段拒绝",
        parse(&spec_of(10)) == Err(DY_EEMPTY),
        "零标签对视为空动态段",
    );

    let built = build_ledger();
    let (d11, d12) = match &built {
        Ok(l) => {
            let r = l.len() == SAMPLES
                && l.accepted() == SAMPLES as u32
                && l.rejected() == 0
                && l.errors() == 4
                && l.total_needed() == 3
                && l.soname_count() == 1
                && l.rpath_count() == 2
                && l.runpath_count() == 1
                && l.pie_count() == 1
                && l.now_count() == 2
                && l.unknown_total() == 1
                && l.ignored_total() == 3
                && l.effective_ok();
            let mut m = match build_ledger() {
                Ok(v) => v,
                Err(_) => DynLedger::new(),
            };
            let f = match m.freeze() {
                Ok(_) => m.is_frozen() && m.freeze() == Err(DY_EFROZEN) && m.rejected() == 1,
                Err(_) => false,
            };
            (r, f)
        }
        Err(_) => (false, false),
    };
    s.add("D11-账本计数恒等式", d11, "十二格六类计数与生效路径全部吻合");
    s.add("D12-冻结后二次拒绝", d12, "冻结成功且再次冻结返回错误");

    s.add(
        "D13-四路注入",
        inject_four() == [true, true, true, true],
        "越序 重复 越界 冻结四条拒绝路径均触发",
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn needed_and_soname() {
        let a = match parse(&spec_of(0)) {
            Ok(v) => v,
            Err(_) => DynInfo::none(),
        };
        assert_eq!(a.needed, 1);
        assert!(a.has_soname);
        assert_eq!(a.soname_len, 9);
        assert_eq!(a.soname_off, 12);
        let b = match parse(&spec_of(1)) {
            Ok(v) => v,
            Err(_) => DynInfo::none(),
        };
        assert_eq!(b.needed, 2);
        assert!(!b.has_soname);
        assert_eq!(parse(&spec_of(8)), Err(DY_ESONAME));
    }

    #[test]
    fn runpath_wins_over_rpath() {
        assert_eq!(effective(&spec_of(2)), Ok(&b"/opt/lib"[..]));
        assert_eq!(effective(&spec_of(3)), Ok(&b"/run/lib"[..]));
        assert_eq!(effective(&spec_of(0)), Ok(&[][..]));
        let v = match parse(&spec_of(3)) {
            Ok(x) => x,
            Err(_) => DynInfo::none(),
        };
        assert!(v.has_rpath && v.has_runpath);
    }

    #[test]
    fn flags_and_unknowns() {
        let a = match parse(&spec_of(4)) {
            Ok(v) => v,
            Err(_) => DynInfo::none(),
        };
        assert_eq!(a.flags1, DF_1_NOW | DF_1_PIE);
        assert!(a.pie && a.now);
        assert_eq!(a.unknown_flags, 0);
        let b = match parse(&spec_of(5)) {
            Ok(v) => v,
            Err(_) => DynInfo::none(),
        };
        assert_eq!(b.unknown, 1);
        let c = match parse(&spec_of(9)) {
            Ok(v) => v,
            Err(_) => DynInfo::none(),
        };
        assert_eq!(c.ignored, 3);
        assert_eq!(c.needed, 0);
    }

    #[test]
    fn refusal_family() {
        assert_eq!(parse(&spec_of(6)), Err(DY_ENOTERM));
        assert_eq!(parse(&spec_of(7)), Err(DY_EOFF));
        assert_eq!(parse(&spec_of(10)), Err(DY_EEMPTY));
        let mut big = DynCase::blank();
        big.n = (MAX_PAIRS + 1) as u8;
        assert_eq!(parse(&big), Err(DY_ETRUNC));
    }

    #[test]
    fn ledger_totals_and_freeze() {
        let mut l = match build_ledger() {
            Ok(v) => v,
            Err(_) => DynLedger::new(),
        };
        assert_eq!(l.len(), SAMPLES);
        assert_eq!(l.errors(), 4);
        assert_eq!(l.total_needed(), 3);
        assert_eq!(l.soname_count(), 1);
        assert_eq!(l.rpath_count(), 2);
        assert_eq!(l.runpath_count(), 1);
        assert_eq!(l.pie_count(), 1);
        assert_eq!(l.now_count(), 2);
        assert_eq!(l.unknown_total(), 1);
        assert_eq!(l.ignored_total(), 3);
        assert!(l.effective_ok());
        let h = match l.freeze() {
            Ok(v) => v,
            Err(_) => 0,
        };
        assert_eq!(l.replay(), Ok(h));
        assert_eq!(l.freeze(), Err(DY_EFROZEN));
    }

    #[test]
    fn injections_and_duplicates() {
        assert_eq!(inject_four(), [true, true, true, true]);
        let mut l = DynLedger::new();
        let c = spec_of(2);
        assert!(l.register(&c).is_ok());
        assert_eq!(l.register(&c), Err(DY_EDUP));
        assert_eq!(l.observe(1), Err(DY_ENOBS));
        assert_eq!(l.rejected(), 2);
        assert_eq!(l.len(), 1);
    }
}
