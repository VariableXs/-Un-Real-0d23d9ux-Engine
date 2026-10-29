//! UNX-F11283 GLIBC_2.xx not found 报错路径全链（0x1f530 段）
//!
//! 版本缺失不是一条错误码，而是一条**三阶段链**：检测（该库的提供版本表里
//! 有没有这个版本）→ 报错（拼出与 glibc 逐字一致的诊断行）→ 退出（127）。
//! 版本用 (major<<16)|(minor<<8)|patch 压缩，patch 为 0 时按两段渲染
//! （GLIBC_2.34），否则三段（GLIBC_2.2.5）。
//! 已知版本表固定七项，四个库各持一张提供位图：libc 全有、libm 到 2.28、
//! libpthread 到 2.14、装载器全有。缺版本时按 glibc 原文拼行：
//! `prog: lib: version `GLIBC_x.y' not found (required by prog)`
//! 反引号与单引号都是原文的一部分，一个字符都不改。
//! 账本对每条缺失样本记一次报错：缺失数、exit 127 数与报错文本摘要三者
//! 在 replay 时全部重算比对；漏拼一行或换了引号都会摘要不符。
//! 拒绝面只有 id 越界、顺序倒退、同 id 重复、封册后写账；
//! 库号越界、目标件越界、未知版本号、依赖过深都是样本，进规则产码不拒收。

use crate::alloc::format;
use crate::checks::CheckSet;
use alloc::string::String;

/// 诊断段（0x1f530 独占 0x10）。
pub const SEG_BASE: u32 = 0x1f530;

/// 条目 id 越界。
pub const GL_EID: u32 = SEG_BASE + 0x01;
/// 登记序倒退。
pub const GL_EORDER: u32 = SEG_BASE + 0x02;
/// 同 id 重复登记。
pub const GL_EDUP: u32 = SEG_BASE + 0x03;
/// 条目表已满。
pub const GL_EFULL: u32 = SEG_BASE + 0x04;
/// 封册后写账。
pub const GL_EFROZEN: u32 = SEG_BASE + 0x05;
/// 观测了未登记的 id。
pub const GL_EUNSEEN: u32 = SEG_BASE + 0x06;
/// 复算判据与登记时不符。
pub const GL_EMISMATCH: u32 = SEG_BASE + 0x07;
/// 账未观测满。
pub const GL_EUNOBS: u32 = SEG_BASE + 0x08;
/// 同一条目重复观测。
pub const GL_EDOBS: u32 = SEG_BASE + 0x09;
/// 报错账/退出账不平。
pub const GL_EBALANCE: u32 = SEG_BASE + 0x0a;

/// 判据码：版本缺失。
pub const VV_MISSING: u32 = 1;
/// 判据码：库号越界。
pub const VV_LIB: u32 = 2;
/// 判据码：目标件号越界。
pub const VV_PROG: u32 = 3;
/// 判据码：未知版本号。
pub const VV_VER: u32 = 4;
/// 判据码：依赖深度过深。
pub const VV_DEPTH: u32 = 5;

/// 版本缺失时的退出码（glibc 原值）。
pub const EXIT_MISSING: u32 = 127;
/// 其它宿主错误的退出码。
pub const EXIT_HOST: u32 = 1;
/// 依赖深度上限。
pub const DEPTH_MAX: u32 = 4;
/// 已知版本条数。
pub const VER_COUNT: usize = 7;
/// 库目录条数。
pub const LIB_COUNT: usize = 4;
/// 目标件条数。
pub const PROG_COUNT: usize = 2;
/// 样本条数。
pub const SAMPLES: usize = 12;
/// 条目表定容。
pub const CAP: usize = 12;
/// id 合法域。
pub const ID_MIN: u32 = 1;
pub const ID_MAX: u32 = 4096;

const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x100_0000_01b3;
const TAG: u64 = 0xb04_83;

/// glibc 版本号压缩表（2.2.5 / 2.3 / 2.14 / 2.17 / 2.28 / 2.34 / 2.38）。
pub const KNOWN_VERSIONS: [u32; VER_COUNT] = [
    0x020205, 0x020300, 0x020e00, 0x021100, 0x021c00, 0x022200, 0x022600,
];

/// 库路径表。
pub const LIBS: [&str; LIB_COUNT] = [
    "/lib/x86_64-linux-gnu/libc.so.6",
    "/lib/x86_64-linux-gnu/libm.so.6",
    "/lib/x86_64-linux-gnu/libpthread.so.0",
    "/lib64/ld-linux-x86-64.so.2",
];

/// 各库的提供位图：libc 全七项、libm 前五、libpthread 前三、装载器全七项。
pub const PROVIDED: [u32; LIB_COUNT] = [
    0b111_1111,
    0b001_1111,
    0b000_0111,
    0b111_1111,
];

/// 目标件表。
pub const PROGS: [&str; PROG_COUNT] = ["./prog", "/bin/busybox"];

/// 版本三段解包。
pub fn ver_parts(v: u32) -> (u32, u32, u32) {
    ((v >> 16) & 0xff, (v >> 8) & 0xff, v & 0xff)
}

/// 版本渲染：patch 为 0 两段，否则三段。
pub fn ver_text(v: u32) -> String {
    let (a, b, c) = ver_parts(v);
    if c == 0 {
        format!("{}.{}", a, b)
    } else {
        format!("{}.{}.{}", a, b, c)
    }
}

/// 已知版本下标；未知返回 None。
pub fn known_index(v: u32) -> Option<usize> {
    let mut k = 0usize;
    while k < VER_COUNT {
        if let Some(x) = KNOWN_VERSIONS.get(k) {
            if *x == v {
                return Some(k);
            }
        }
        k = k.wrapping_add(1);
    }
    None
}

/// 一条版本样本：版本 + 库号 + 目标件号 + 深度 + 指纹。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VerCase {
    pub id: u32,
    pub ver: u32,
    pub lib: u32,
    pub prog: u32,
    pub depth: u32,
    pub hash: u64,
}

impl VerCase {
    pub const fn blank() -> VerCase {
        VerCase { id: 0, ver: 0, lib: 0, prog: 0, depth: 0, hash: 0 }
    }

    pub fn render(&self) -> String {
        format!("V{} lib{} prog{} depth{} v={:06x}",
            self.id, self.lib, self.prog, self.depth, self.ver)
    }
}

/// 纯判据：库号 → 目标件 → 版本已知 → 深度 → 提供位图。
pub fn version_verdict(c: &VerCase) -> u32 {
    if c.lib as usize >= LIB_COUNT {
        return VV_LIB;
    }
    if c.prog as usize >= PROG_COUNT {
        return VV_PROG;
    }
    let idx = match known_index(c.ver) {
        Some(v) => v,
        None => return VV_VER,
    };
    if c.depth > DEPTH_MAX {
        return VV_DEPTH;
    }
    let mask = match PROVIDED.get(c.lib as usize) {
        Some(v) => *v,
        None => 0,
    };
    if mask & (1u32 << idx) != 0 {
        0
    } else {
        VV_MISSING
    }
}

/// 检测阶段：该库是否声明提供此版本。
pub fn provided(ver: u32, lib: u32) -> bool {
    let idx = match known_index(ver) {
        Some(v) => v,
        None => return false,
    };
    match PROVIDED.get(lib as usize) {
        Some(m) => m & (1u32 << idx) != 0,
        None => false,
    }
}

/// 报错阶段：按 glibc 原文拼诊断行（只在缺失时产行）。
pub fn missing_message(c: &VerCase) -> Option<String> {
    if version_verdict(c) != VV_MISSING {
        return None;
    }
    let lib = match LIBS.get(c.lib as usize) {
        Some(v) => *v,
        None => return None,
    };
    let prog = match PROGS.get(c.prog as usize) {
        Some(v) => *v,
        None => return None,
    };
    Some(format!("{}: {}: version `GLIBC_{}' not found (required by {})",
        prog, lib, ver_text(c.ver), prog))
}

/// 退出阶段：缺失 127，宿主错误 1，正常 0。
pub fn exit_code(v: u32) -> u32 {
    if v == 0 {
        0
    } else if v == VV_MISSING {
        EXIT_MISSING
    } else {
        EXIT_HOST
    }
}

/// 版本账本：样本 + 判据 + 六槽直方 + 缺失数/127 数 + 报错文本摘要。
#[derive(Clone, Copy, Debug)]
pub struct VerLedger {
    cases: [VerCase; CAP],
    verdicts: [u32; CAP],
    seen: [bool; CAP],
    hist: [u32; 6],
    missing_count: u32,
    exit127_count: u32,
    msg_digest: u64,
    len: usize,
    attempts: u32,
    accepts: u32,
    rejects: u32,
    observed: u32,
    matched: u32,
    frozen: bool,
    root: Option<u64>,
}

impl VerLedger {
    pub const fn new() -> VerLedger {
        VerLedger {
            cases: [VerCase::blank(); CAP],
            verdicts: [0u32; CAP],
            seen: [false; CAP],
            hist: [0u32; 6],
            missing_count: 0,
            exit127_count: 0,
            msg_digest: FNV_OFF ^ TAG,
            len: 0,
            attempts: 0,
            accepts: 0,
            rejects: 0,
            observed: 0,
            matched: 0,
            frozen: false,
            root: None,
        }
    }

    fn fail(&mut self, code: u32) -> u32 {
        self.rejects = self.rejects.wrapping_add(1);
        code
    }

    /// 登记：封册 → id 域 → 重复 → 顺序 → 定容。
    pub fn register(&mut self, c: VerCase) -> Result<usize, u32> {
        self.attempts = self.attempts.wrapping_add(1);
        if self.frozen {
            return Err(self.fail(GL_EFROZEN));
        }
        if c.id < ID_MIN || c.id > ID_MAX {
            return Err(self.fail(GL_EID));
        }
        let mut k = 0usize;
        while k < self.len {
            if let Some(x) = self.cases.get(k) {
                if x.id == c.id {
                    return Err(self.fail(GL_EDUP));
                }
            }
            k = k.wrapping_add(1);
        }
        if self.len > 0 {
            if let Some(last) = self.cases.get(self.len - 1) {
                if c.id <= last.id {
                    return Err(self.fail(GL_EORDER));
                }
            }
        }
        if self.len >= CAP {
            return Err(self.fail(GL_EFULL));
        }
        if let Some(slot) = self.cases.get_mut(self.len) {
            *slot = c;
        }
        self.len = self.len.wrapping_add(1);
        self.accepts = self.accepts.wrapping_add(1);
        Ok(self.len - 1)
    }

    /// 登记 + 判据：缺失样本走完报错与退出两阶段并落账。
    pub fn register_ruled(&mut self, c: VerCase) -> Result<(usize, u32), u32> {
        let idx = self.register(c)?;
        let v = version_verdict(&c);
        if let Some(slot) = self.verdicts.get_mut(idx) {
            *slot = v;
        }
        if let Some(h) = self.hist.get_mut(v as usize) {
            *h = h.wrapping_add(1);
        }
        if v == VV_MISSING {
            self.missing_count = self.missing_count.wrapping_add(1);
            if exit_code(v) == EXIT_MISSING {
                self.exit127_count = self.exit127_count.wrapping_add(1);
            }
            if let Some(msg) = missing_message(&c) {
                let mut h = FNV_OFF ^ TAG;
                let bytes = msg.as_bytes();
                let mut b = 0usize;
                while b < bytes.len() {
                    if let Some(x) = bytes.get(b) {
                        h = (h ^ *x as u64).wrapping_mul(FNV_PRIME);
                    }
                    b = b.wrapping_add(1);
                }
                self.msg_digest = (self.msg_digest ^ h).wrapping_mul(FNV_PRIME);
            }
        }
        Ok((idx, v))
    }

    /// 观测实测判据码。
    pub fn observe(&mut self, id: u32, seen_code: u32) -> Result<bool, u32> {
        if self.frozen {
            return Err(GL_EFROZEN);
        }
        let mut k = 0usize;
        let mut hit: Option<usize> = None;
        while k < self.len {
            if let Some(x) = self.cases.get(k) {
                if x.id == id {
                    hit = Some(k);
                }
            }
            k = k.wrapping_add(1);
        }
        let idx = match hit {
            Some(v) => v,
            None => return Err(GL_EUNSEEN),
        };
        match self.seen.get(idx) {
            Some(true) => return Err(GL_EDOBS),
            _ => {}
        }
        if let Some(slot) = self.seen.get_mut(idx) {
            *slot = true;
        }
        self.observed = self.observed.wrapping_add(1);
        let expect = match self.verdicts.get(idx) {
            Some(v) => *v,
            None => 0,
        };
        if seen_code == expect {
            self.matched = self.matched.wrapping_add(1);
            Ok(true)
        } else {
            Ok(false)
        }
    }

    pub fn len(&self) -> usize {
        self.len
    }
    pub fn attempts(&self) -> u32 {
        self.attempts
    }
    pub fn accepts(&self) -> u32 {
        self.accepts
    }
    pub fn rejects(&self) -> u32 {
        self.rejects
    }
    pub fn observed(&self) -> u32 {
        self.observed
    }
    pub fn matched(&self) -> u32 {
        self.matched
    }
    pub fn missing_count(&self) -> u32 {
        self.missing_count
    }
    pub fn exit127_count(&self) -> u32 {
        self.exit127_count
    }
    pub fn msg_digest(&self) -> u64 {
        self.msg_digest
    }
    pub fn hist_at(&self, i: usize) -> u32 {
        match self.hist.get(i) {
            Some(v) => *v,
            None => 0,
        }
    }
    pub fn hist_sum(&self) -> u32 {
        let mut s = 0u32;
        let mut k = 0usize;
        while k < 6 {
            if let Some(v) = self.hist.get(k) {
                s = s.wrapping_add(*v);
            }
            k = k.wrapping_add(1);
        }
        s
    }
    pub fn zero_verdicts(&self) -> u32 {
        self.hist_at(0)
    }
    pub fn case_at(&self, i: usize) -> Option<VerCase> {
        self.cases.get(i).copied()
    }
    pub fn verdict_at(&self, i: usize) -> Option<u32> {
        self.verdicts.get(i).copied()
    }
    pub fn is_frozen(&self) -> bool {
        self.frozen
    }
    pub fn root(&self) -> Option<u64> {
        self.root
    }

    fn twine(h: u64, v: u64) -> u64 {
        (h ^ v).wrapping_mul(FNV_PRIME)
    }

    /// FNV-1a：覆盖样本、判据、观测位、直方与五项计数。
    pub fn fold(&self) -> u64 {
        let mut h = FNV_OFF ^ TAG;
        let mut k = 0usize;
        while k < CAP {
            if let Some(c) = self.cases.get(k) {
                h = Self::twine(h, c.id as u64);
                h = Self::twine(h, c.ver as u64);
                h = Self::twine(h, c.lib as u64);
                h = Self::twine(h, c.prog as u64);
                h = Self::twine(h, c.depth as u64);
                h = Self::twine(h, c.hash);
            }
            if let Some(v) = self.verdicts.get(k) {
                h = Self::twine(h, *v as u64);
            }
            if let Some(b) = self.seen.get(k) {
                h = Self::twine(h, if *b { 1 } else { 0 });
            }
            if let Some(x) = self.hist.get(k % 6) {
                h = Self::twine(h, *x as u64);
            }
            k = k.wrapping_add(1);
        }
        h = Self::twine(h, self.missing_count as u64);
        h = Self::twine(h, self.exit127_count as u64);
        h = Self::twine(h, self.msg_digest);
        h = Self::twine(h, self.len as u64);
        h = Self::twine(h, self.attempts as u64);
        h = Self::twine(h, self.accepts as u64);
        h = Self::twine(h, self.rejects as u64);
        h = Self::twine(h, self.observed as u64);
        h = Self::twine(h, self.matched as u64);
        h = Self::twine(h, if self.frozen { 1 } else { 0 });
        h
    }

    /// 复算：全登记、全观测、全相符、直方与报错账皆平。
    pub fn replay(&self) -> Result<u64, u32> {
        if self.len == 0 || self.observed != self.len as u32 || self.matched != self.len as u32 {
            return Err(GL_EUNOBS);
        }
        if self.accepts.wrapping_add(self.rejects) != self.attempts {
            return Err(GL_EBALANCE);
        }
        if self.hist_sum() != self.len as u32 {
            return Err(GL_EBALANCE);
        }
        let mut miss = 0u32;
        let mut x127 = 0u32;
        let mut dig = FNV_OFF ^ TAG;
        let mut k = 0usize;
        while k < self.len {
            let c = match self.cases.get(k) {
                Some(v) => *v,
                None => return Err(GL_EBALANCE),
            };
            if self.seen.get(k) != Some(&true) {
                return Err(GL_EUNOBS);
            }
            let v = version_verdict(&c);
            if self.verdicts.get(k) != Some(&v) {
                return Err(GL_EMISMATCH);
            }
            if v == VV_MISSING {
                miss = miss.wrapping_add(1);
                if exit_code(v) == EXIT_MISSING {
                    x127 = x127.wrapping_add(1);
                }
                match missing_message(&c) {
                    Some(msg) => {
                        let bytes = msg.as_bytes();
                        let mut m = FNV_OFF ^ TAG;
                        let mut b = 0usize;
                        while b < bytes.len() {
                            if let Some(x) = bytes.get(b) {
                                m = (m ^ *x as u64).wrapping_mul(FNV_PRIME);
                            }
                            b = b.wrapping_add(1);
                        }
                        dig = (dig ^ m).wrapping_mul(FNV_PRIME);
                    }
                    None => return Err(GL_EMISMATCH),
                }
            }
            k = k.wrapping_add(1);
        }
        if miss != self.missing_count
            || x127 != self.exit127_count
            || dig != self.msg_digest
            || self.hist_at(VV_MISSING as usize) != miss
        {
            return Err(GL_EBALANCE);
        }
        Ok(self.fold())
    }

    /// 封册：先复算再落锁；二次封册只回错不加拒绝计数。
    pub fn freeze(&mut self) -> Result<u64, u32> {
        if self.frozen {
            return Err(GL_EFROZEN);
        }
        self.replay()?;
        self.frozen = true;
        let sealed = self.fold();
        self.root = Some(sealed);
        Ok(sealed)
    }
}

/// 十二条样本：五条通过、三条缺版本、四条各踩一类非法域。
pub fn spec_of(id: usize) -> (VerCase, u32) {
    if id < 1 || id > SAMPLES {
        return (VerCase::blank(), GL_EID);
    }
    let (ver, lib, prog, depth) = match id {
        1 => (0x022200u32, 0u32, 0u32, 0u32),
        2 => (0x022600, 0, 0, 0),
        3 => (0x022200, 1, 0, 0),
        4 => (0x021c00, 2, 1, 0),
        5 => (0x022200, 9, 0, 0),
        6 => (0x022200, 0, 5, 0),
        7 => (0x000999, 0, 0, 0),
        8 => (0x022200, 0, 0, 5),
        9 => (0x021100, 1, 1, 2),
        10 => (0x020205, 2, 0, 1),
        11 => (0x022600, 1, 1, 0),
        _ => (0x020205, 0, 0, 0),
    };
    let c = VerCase {
        id: id as u32,
        ver,
        lib,
        prog,
        depth,
        hash: 0x1128_3000u64.wrapping_add(id as u64),
    };
    (c, version_verdict(&c))
}

/// 逐条登记 + 观测；样本 id 回填后入账。
pub fn build_ledger() -> VerLedger {
    let mut l = VerLedger::new();
    let mut i = 1usize;
    while i <= SAMPLES {
        let (mut c, expect) = spec_of(i);
        c.id = i as u32;
        if l.register_ruled(c).is_ok() {
            let _ = l.observe(c.id, expect);
        }
        i = i.wrapping_add(1);
    }
    l
}

/// 四条独立拒绝：顺序、重复、id 越界、封册后写账。
pub fn inject_four() -> [bool; 4] {
    let mut a = VerLedger::new();
    let mut hi = VerCase::blank();
    hi.id = 2;
    hi.ver = 0x022200;
    let _ = a.register(hi);
    let mut lo = VerCase::blank();
    lo.id = 1;
    lo.ver = 0x020205;
    let bad_a = a.register(lo).err() == Some(GL_EORDER);

    let mut b = VerLedger::new();
    let one = match spec_of(1) {
        (c, _) => c,
    };
    let _ = b.register(one);
    let bad_b = b.register(one).err() == Some(GL_EDUP);

    let mut c = VerLedger::new();
    let mut wide = VerCase::blank();
    wide.id = ID_MAX + 1;
    let bad_c = c.register(wide).err() == Some(GL_EID);

    let mut d = build_ledger();
    let _ = d.freeze();
    let mut late = VerCase::blank();
    late.id = 40;
    let bad_d = d.register(late).err() == Some(GL_EFROZEN);
    [bad_a, bad_b, bad_c, bad_d]
}

/// 十三条自检：前九项压版本链与报错原文，后四项压拒绝与封册。
pub fn run_unx11283_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F11283");

    let layout = SEG_BASE == 0x1f530
        && GL_EID == SEG_BASE + 0x01
        && GL_EORDER == SEG_BASE + 0x02
        && GL_EDUP == SEG_BASE + 0x03
        && GL_EFULL == SEG_BASE + 0x04
        && GL_EFROZEN == SEG_BASE + 0x05
        && GL_EUNSEEN == SEG_BASE + 0x06
        && GL_EMISMATCH == SEG_BASE + 0x07
        && GL_EUNOBS == SEG_BASE + 0x08
        && GL_EDOBS == SEG_BASE + 0x09
        && GL_EBALANCE == SEG_BASE + 0x0a;
    s.add("S1-段布局与十个错误码", layout, "0x1f530 段十码连续");

    let table_ok = KNOWN_VERSIONS.len() == 7
        && VER_COUNT == 7
        && ver_parts(0x020205) == (2, 2, 5)
        && ver_parts(0x020e00) == (2, 14, 0)
        && ver_text(0x022200) == "2.34"
        && ver_text(0x022600) == "2.38"
        && ver_text(0x020205) == "2.2.5"
        && known_index(0x020205) == Some(0)
        && known_index(0x022600) == Some(6)
        && known_index(0x099900).is_none();
    s.add("S2-七项版本表与两段/三段渲染", table_ok,
        "2.34 两段、2.2.5 三段、未知版本不在表");

    let prov_ok = provided(0x022200, 0)
        && provided(0x022600, 0)
        && !provided(0x022200, 1)
        && provided(0x021c00, 1)
        && !provided(0x021c00, 2)
        && provided(0x020e00, 2)
        && provided(0x022600, 3)
        && !provided(0x022200, 9)
        && !provided(0x099900, 0);
    s.add("S3-四库提供位图", prov_ok,
        "libc 全有、libm 到 2.28、libpthread 到 2.14、装载器全有");

    let mut chain_ok = true;
    let mut i = 1usize;
    while i <= SAMPLES {
        let (c, expect) = spec_of(i);
        if version_verdict(&c) != expect {
            chain_ok = false;
        }
        i = i.wrapping_add(1);
    }
    s.add("S4-十二样本判据链", chain_ok,
        "库号→目标件→已知版本→深度→位图，五道闸逐样本相符");

    let m3 = match missing_message(&spec_of(3).0) {
        Some(v) => v,
        None => String::new(),
    };
    let want3 = "./prog: /lib/x86_64-linux-gnu/libm.so.6: version `GLIBC_2.34' not found (required by ./prog)";
    s.add("S5-libm 缺 2.34 原文逐字", m3 == want3,
        "prog、库路径、反引号与单引号全按 glibc 原文");

    let m4 = match missing_message(&spec_of(4).0) {
        Some(v) => v,
        None => String::new(),
    };
    let want4 = "/bin/busybox: /lib/x86_64-linux-gnu/libpthread.so.0: version `GLIBC_2.28' not found (required by /bin/busybox)";
    s.add("S6-libpthread 缺 2.28 原文逐字", m4 == want4,
        "busybox 目标件与三段版本 2.28 同时出现");

    s.add("S7-非缺失样本不产报错行", missing_message(&spec_of(1).0).is_none()
        && missing_message(&spec_of(5).0).is_none()
        && missing_message(&spec_of(7).0).is_none(),
        "通过样本、库号越界、未知版本都不得拼出诊断行");

    s.add("S8-退出码三档", exit_code(0) == 0
        && exit_code(VV_MISSING) == EXIT_MISSING
        && exit_code(VV_LIB) == EXIT_HOST
        && exit_code(VV_DEPTH) == EXIT_HOST,
        "正常 0、缺版本 127、其余宿主错误 1");

    let led = build_ledger();
    let counts_ok = led.len() == SAMPLES
        && led.zero_verdicts() == 5
        && led.hist_at(VV_MISSING as usize) == 3
        && led.hist_at(VV_LIB as usize) == 1
        && led.hist_at(VV_PROG as usize) == 1
        && led.hist_at(VV_VER as usize) == 1
        && led.hist_at(VV_DEPTH as usize) == 1
        && led.hist_sum() == SAMPLES as u32
        && led.missing_count() == 3
        && led.exit127_count() == 3;
    s.add("S9-十二样本落账配平", counts_ok,
        "直方 5+3+1+1+1+1=12，缺失数与 127 数同为 3");

    let mut dig = FNV_OFF ^ TAG;
    let mut k = 1usize;
    while k <= SAMPLES {
        let (c, v) = spec_of(k);
        if v == VV_MISSING {
            if let Some(msg) = missing_message(&c) {
                let bytes = msg.as_bytes();
                let mut m = FNV_OFF ^ TAG;
                let mut b = 0usize;
                while b < bytes.len() {
                    if let Some(x) = bytes.get(b) {
                        m = (m ^ *x as u64).wrapping_mul(FNV_PRIME);
                    }
                    b = b.wrapping_add(1);
                }
                dig = (dig ^ m).wrapping_mul(FNV_PRIME);
            }
        }
        k = k.wrapping_add(1);
    }
    let led2 = build_ledger();
    s.add("S10-报错摘要复算与折叠稳定", dig == led.msg_digest()
        && led.fold() == led2.fold()
        && led.replay().is_ok()
        && led.root().is_none(),
        "三条缺失行摘要逐字节复算相同，两次构建折叠同值");

    let mut o = VerLedger::new();
    let mut c1 = spec_of(1).0;
    c1.id = 1;
    let reg = o.register_ruled(c1).is_ok();
    let first = o.observe(1, 0) == Ok(true);
    let twice = o.observe(1, 0).err() == Some(GL_EDOBS);
    let unseen = o.observe(99, 0).err() == Some(GL_EUNSEEN);
    let mut o2 = VerLedger::new();
    let mut c2 = spec_of(2).0;
    c2.id = 2;
    let _ = o2.register_ruled(c2);
    let wrong = o2.observe(2, VV_MISSING) == Ok(false);
    s.add("S11-观测语义", reg && first && twice && unseen && wrong,
        "正确判据回真、重复观测回 EDOBS、未见 id 回 EUNSEEN、错码回假");

    let four = inject_four();
    let mut full = build_ledger();
    let mut more = VerCase::blank();
    more.id = 40;
    let full_err = full.register(more).err() == Some(GL_EFULL);
    s.add("S12-四条拒绝与定容", four[0] && four[1] && four[2] && four[3] && full_err,
        "顺序、重复、id 越界、封册后写账、满表各拒一次");

    let mut f = build_ledger();
    let d = match f.freeze() {
        Ok(v) => v,
        Err(_) => 0,
    };
    let rb = f.rejects();
    let again = match f.freeze() {
        Err(GL_EFROZEN) => true,
        _ => false,
    };
    s.add("S13-封册落根与二次拒绝", d != 0
        && d == f.fold()
        && f.root() == Some(d)
        && again
        && f.rejects() == rb
        && f.replay().is_ok(),
        "首封落根等于折叠值，二次封册回 EFROZEN 不计拒绝");

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_render_by_patch() {
        assert_eq!(ver_parts(0x020205), (2, 2, 5));
        assert_eq!(ver_text(0x022200), "2.34");
        assert_eq!(ver_text(0x020205), "2.2.5");
        assert_eq!(known_index(0x020205), Some(0));
        assert_eq!(known_index(0x021c00), Some(4));
        assert!(known_index(0x020206).is_none());
    }

    #[test]
    fn provided_masks_follow_library() {
        assert!(provided(0x022600, 0));
        assert!(!provided(0x022200, 1));
        assert!(provided(0x021c00, 1));
        assert!(!provided(0x021c00, 2));
        assert!(provided(0x020e00, 2));
        assert!(provided(0x020205, 3));
        assert!(!provided(0x022200, 7));
    }

    #[test]
    fn verdict_chain_covers_twelve() {
        let mut zero = 0u32;
        let mut i = 1usize;
        while i <= SAMPLES {
            let (c, expect) = spec_of(i);
            assert_eq!(version_verdict(&c), expect);
            if expect == 0 {
                zero = zero.wrapping_add(1);
            }
            i = i.wrapping_add(1);
        }
        assert_eq!(zero, 5);
        assert_eq!(version_verdict(&spec_of(3).0), VV_MISSING);
        assert_eq!(version_verdict(&spec_of(4).0), VV_MISSING);
        assert_eq!(version_verdict(&spec_of(5).0), VV_LIB);
        assert_eq!(version_verdict(&spec_of(6).0), VV_PROG);
        assert_eq!(version_verdict(&spec_of(7).0), VV_VER);
        assert_eq!(version_verdict(&spec_of(8).0), VV_DEPTH);
    }

    #[test]
    fn missing_lines_match_glibc() {
        let m = match missing_message(&spec_of(3).0) {
            Some(v) => v,
            None => String::from(""),
        };
        assert_eq!(m, "./prog: /lib/x86_64-linux-gnu/libm.so.6: version `GLIBC_2.34' not found (required by ./prog)");
        let b = match missing_message(&spec_of(4).0) {
            Some(v) => v,
            None => String::from(""),
        };
        assert_eq!(b, "/bin/busybox: /lib/x86_64-linux-gnu/libpthread.so.0: version `GLIBC_2.28' not found (required by /bin/busybox)");
        assert!(missing_message(&spec_of(1).0).is_none());
        assert_eq!(exit_code(VV_MISSING), 127);
        assert_eq!(exit_code(VV_VER), 1);
        assert_eq!(exit_code(0), 0);
    }

    #[test]
    fn ledger_balances_and_replays() {
        let l = build_ledger();
        assert_eq!(l.len(), 12);
        assert_eq!(l.zero_verdicts(), 5);
        assert_eq!(l.missing_count(), 3);
        assert_eq!(l.exit127_count(), 3);
        assert_eq!(l.hist_sum(), 12);
        assert_eq!(l.attempts(), 12);
        assert_eq!(l.accepts(), 12);
        assert_eq!(l.observed(), 12);
        assert_eq!(l.matched(), 12);
        let d = l.replay();
        assert!(d.is_ok());
        match d {
            Ok(v) => assert_eq!(v, l.fold()),
            Err(_) => assert!(false),
        }
    }

    #[test]
    fn refusals_and_frozen_are_distinct() {
        let four = inject_four();
        assert!(four[0] && four[1] && four[2] && four[3]);
        let mut f = build_ledger();
        let first = match f.freeze() {
            Ok(v) => v != 0,
            Err(_) => false,
        };
        assert!(first);
        let rb = f.rejects();
        assert_eq!(f.freeze().err(), Some(GL_EFROZEN));
        assert_eq!(f.rejects(), rb);
        assert!(f.is_frozen());
        assert_eq!(f.root(), Some(f.fold()));
    }

    #[test]
    fn checks_are_green() {
        let s = run_unx11283_checks();
        assert!(s.all_passed());
        assert_eq!(s.len(), 13);
    }
}
