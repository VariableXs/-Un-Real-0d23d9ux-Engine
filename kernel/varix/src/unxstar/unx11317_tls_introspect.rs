//! UNX-F11317 TLS 自省诊断面（0xe9c0 段）
//!
//! 动态链接器与调试器都要回答同一组问题：DTV 有多长、装了几个模块、静态
//! TLS 余量还剩多少。这三个量必须在同一时刻取自同一份状态，否则“模块数
//! 与 DTV 长度不符”这类伪故障会让诊断者去修本来没坏的代码。
//!
//! 模型把 DTV 做成显式账：第零格是代际号，其后每个活模块占一格记录块内
//! 偏移。`probe()` 一次性冻结三量与摘要，`check()` 用同一份状态复核报告，
//! 四类不一致分档上报：模块数超槽（EMODS）、长度与代际不符（EDVTLEN）、
//! 余量为负（ESURR）、摘要不符（EDIGEST）。分档的意义在于把“状态坏了”
//! 与“报告被篡改”区分开。
//!
//! 分配策略是精确尺寸复用：释放的块只有遇到同尺寸请求才会被取走，否则
//! 游标继续前进。这解释了为什么诊断面看到的余量有时小于“释放总量”——
//! 余量是游标余量，不是空洞余量，两者不可混为一谈。
//!
//! 账本十四格：八格成功（四次分配、两次探针、一次释放、一次复检），六格
//! 拒绝（尺寸为零、余量不足、越界下标、重复释放、报告长度篡改、空面取线）。

use crate::alloc::format;
use crate::checks::CheckSet;

/// 段基址。
pub const SEG_BASE: u32 = 0xe9c0;

/// 编号非法。
pub const RP_EID: u32 = SEG_BASE + 0x01;
/// 登记顺序错误。
pub const RP_EORDER: u32 = SEG_BASE + 0x02;
/// 重复登记。
pub const RP_EDUP: u32 = SEG_BASE + 0x03;
/// 已冻结。
pub const RP_EFROZEN: u32 = SEG_BASE + 0x04;
/// 观测不存在的格。
pub const RP_ENOBS: u32 = SEG_BASE + 0x05;
/// 结论不符。
pub const RP_EMISMATCH: u32 = SEG_BASE + 0x06;
/// 账本失衡。
pub const RP_EBAL: u32 = SEG_BASE + 0x07;
/// 尺寸为零。
pub const RP_ESIZE: u32 = SEG_BASE + 0x08;
/// 余量不足。
pub const RP_ENOMEM: u32 = SEG_BASE + 0x09;
/// 越界下标。
pub const RP_EIDX: u32 = SEG_BASE + 0x0a;
/// 重复释放。
pub const RP_EDOUBLE: u32 = SEG_BASE + 0x0b;
/// 模块数超槽。
pub const RP_EMODS: u32 = SEG_BASE + 0x0c;
/// DTV 长度与代际不符。
pub const RP_EDVTLEN: u32 = SEG_BASE + 0x0d;
/// 余量越界。
pub const RP_ESURR: u32 = SEG_BASE + 0x0e;
/// 摘要不符。
pub const RP_EDIGEST: u32 = SEG_BASE + 0x0f;
/// 空面取线。
pub const RP_EEMPTY: u32 = SEG_BASE + 0x10;
/// 槽满。
pub const RP_EFULL: u32 = SEG_BASE + 0x11;

/// 判决：成功。
pub const V_OK: u32 = 0;

/// 操作：分配。
pub const OP_ADD: u8 = 1;
/// 操作：释放。
pub const OP_REMOVE: u8 = 2;
/// 操作：探针。
pub const OP_PROBE: u8 = 3;
/// 操作：报告复核。
pub const OP_CHECK: u8 = 4;
/// 操作：取线。
pub const OP_LINE: u8 = 5;

/// 竞技场容量。
pub const CAP: u32 = 320;
/// 槽数。
pub const SLOTS: usize = 8;
/// 空闲块上限。
pub const FREES: usize = 8;
/// DTV 代际号（第零格恒为该值）。
pub const GEN: u32 = 1;
/// 样本格数。
pub const SAMPLES: usize = 14;

/// FNV 偏移基。
pub const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;
/// FNV 质数。
pub const FNV_PRIME: u64 = 0x100_0000_01b3;

/// 自省报告：一次探针冻结的五元组。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Report {
    pub dtv_len: u32,
    pub mods: u32,
    pub surplus: u32,
    pub gen: u32,
    pub digest: u32,
}

impl Report {
    /// 空报告。
    pub const fn blank() -> Report {
        Report {
            dtv_len: 0,
            mods: 0,
            surplus: 0,
            gen: 0,
            digest: 0,
        }
    }
}

/// 空闲块。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Free {
    pub offset: u32,
    pub size: u32,
}

/// DTV 账。
#[derive(Clone, Copy, Debug)]
pub struct Dtv {
    offs: [u32; SLOTS],
    sizes: [u32; SLOTS],
    used: [bool; SLOTS],
    frees: [Free; FREES],
    n: usize,
    nfree: usize,
    cursor: u32,
    gen: u32,
}

impl Dtv {
    /// 空账。
    pub const fn new() -> Dtv {
        Dtv {
            offs: [0; SLOTS],
            sizes: [0; SLOTS],
            used: [false; SLOTS],
            frees: [Free { offset: 0, size: 0 }; FREES],
            n: 0,
            nfree: 0,
            cursor: 0,
            gen: GEN,
        }
    }

    /// 活模块数。
    pub fn mods(&self) -> u32 {
        self.n as u32
    }

    /// DTV 长度：代际格 + 每模块一格。
    pub fn dtv_len(&self) -> u32 {
        self.gen + self.n as u32
    }

    /// 游标余量。
    pub fn surplus(&self) -> u32 {
        CAP.saturating_sub(self.cursor)
    }

    /// 游标位置。
    pub fn cursor(&self) -> u32 {
        self.cursor
    }

    /// 活块字节数。
    pub fn bytes(&self) -> u32 {
        let mut b = 0u32;
        let mut i = 0;
        while i < SLOTS {
            match (self.used.get(i), self.sizes.get(i)) {
                (Some(true), Some(s)) => b += *s,
                _ => {}
            }
            i += 1;
        }
        b
    }

    /// 空闲块数。
    pub fn free_blocks(&self) -> usize {
        self.nfree
    }

    /// 取槽偏移。
    pub fn offset(&self, slot: usize) -> Option<u32> {
        match self.used.get(slot) {
            Some(true) => self.offs.get(slot).copied(),
            _ => None,
        }
    }

    /// 取槽尺寸。
    pub fn size_at(&self, slot: usize) -> Option<u32> {
        match self.used.get(slot) {
            Some(true) => self.sizes.get(slot).copied(),
            _ => None,
        }
    }

    /// 取空闲块。
    fn take_free(&mut self, size: u32) -> Option<u32> {
        let mut i = 0;
        while i < self.nfree {
            let f = match self.frees.get(i) {
                Some(v) => *v,
                None => return None,
            };
            if f.size == size {
                let off = f.offset;
                let mut j = i;
                while j + 1 < self.nfree {
                    let next = match self.frees.get(j + 1) {
                        Some(v) => *v,
                        None => break,
                    };
                    if let Some(x) = self.frees.get_mut(j) {
                        *x = next;
                    }
                    j += 1;
                }
                self.nfree -= 1;
                return Some(off);
            }
            i += 1;
        }
        None
    }

    /// 分配：尺寸 → 槽 → 空闲块 → 游标。
    pub fn add(&mut self, size: u32) -> Result<(u32, u32), u32> {
        if size == 0 {
            return Err(RP_ESIZE);
        }
        let mut idx = SLOTS;
        let mut i = 0;
        while i < SLOTS {
            match self.used.get(i) {
                Some(false) => {
                    idx = i;
                    break;
                }
                _ => {}
            }
            i += 1;
        }
        if idx == SLOTS {
            return Err(RP_EFULL);
        }
        let off = match self.take_free(size) {
            Some(v) => v,
            None => {
                let end = match self.cursor.checked_add(size) {
                    Some(v) => v,
                    None => return Err(RP_ENOMEM),
                };
                if end > CAP {
                    return Err(RP_ENOMEM);
                }
                let o = self.cursor;
                self.cursor = end;
                o
            }
        };
        if let Some(x) = self.used.get_mut(idx) {
            *x = true;
        }
        if let Some(x) = self.offs.get_mut(idx) {
            *x = off;
        }
        if let Some(x) = self.sizes.get_mut(idx) {
            *x = size;
        }
        self.n += 1;
        Ok((idx as u32, off))
    }

    /// 释放：下标 → 重复 → 归还。
    pub fn remove(&mut self, slot: usize) -> Result<u32, u32> {
        if slot >= SLOTS {
            return Err(RP_EIDX);
        }
        match self.used.get(slot) {
            Some(true) => {}
            Some(false) => return Err(RP_EDOUBLE),
            None => return Err(RP_EIDX),
        }
        let off = match self.offs.get(slot) {
            Some(v) => *v,
            None => return Err(RP_EIDX),
        };
        let size = match self.sizes.get(slot) {
            Some(v) => *v,
            None => return Err(RP_EIDX),
        };
        if self.nfree < FREES {
            if let Some(x) = self.frees.get_mut(self.nfree) {
                *x = Free { offset: off, size };
                self.nfree += 1;
            }
        }
        if let Some(x) = self.used.get_mut(slot) {
            *x = false;
        }
        self.n -= 1;
        Ok(off)
    }

    /// 摘要：对整个 DTV 逐格折入。
    pub fn digest(&self) -> u32 {
        let mut h = FNV_OFF;
        h = fold(h, self.gen as u64);
        let mut i = 0;
        while i < SLOTS {
            match (self.used.get(i), self.offs.get(i)) {
                (Some(u), Some(o)) => {
                    if *u {
                        h = fold(h, i as u64);
                        h = fold(h, *o as u64);
                    }
                }
                _ => {}
            }
            i += 1;
        }
        (h & 0xffff_ffff) as u32
    }

    /// 一次冻结三量与摘要。
    pub fn probe(&self) -> Report {
        Report {
            dtv_len: self.dtv_len(),
            mods: self.mods(),
            surplus: self.surplus(),
            gen: self.gen,
            digest: self.digest(),
        }
    }

    /// 复核报告：四类不一致分档。
    pub fn check(&self, r: &Report) -> Result<(), u32> {
        if r.mods > SLOTS as u32 {
            return Err(RP_EMODS);
        }
        if r.dtv_len != r.mods + r.gen {
            return Err(RP_EDVTLEN);
        }
        if r.surplus > CAP {
            return Err(RP_ESURR);
        }
        if r.digest != self.digest() {
            return Err(RP_EDIGEST);
        }
        Ok(())
    }

    /// 取诊断线：空面拒绝。
    pub fn line(&self) -> Result<u32, u32> {
        if self.n == 0 {
            return Err(RP_EEMPTY);
        }
        Ok(self.digest())
    }
}

/// FNV 折入。
pub fn fold(h: u64, v: u64) -> u64 {
    let mut x = h;
    let mut n = v;
    let mut i = 0;
    while i < 8 {
        x ^= n & 0xff;
        x = x.wrapping_mul(FNV_PRIME);
        n >>= 8;
        i += 1;
    }
    x
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
            seal: 0,
            frozen: false,
            rejects: 0,
        }
    }

    /// 登记一格。
    pub fn register(&mut self, row: Sample) -> Result<(), u32> {
        if self.frozen {
            return Err(RP_EFROZEN);
        }
        if (row.slot as usize) != self.len {
            return Err(RP_EORDER);
        }
        if self.len >= SAMPLES {
            return Err(RP_EID);
        }
        let mut i = 0;
        while i < self.len {
            match self.rows.get(i) {
                Some(x) => {
                    if x.slot == row.slot {
                        return Err(RP_EDUP);
                    }
                }
                None => return Err(RP_ENOBS),
            }
            i += 1;
        }
        match self.rows.get_mut(self.len) {
            Some(x) => *x = row,
            None => return Err(RP_EID),
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
        let mut n = 0;
        let mut i = 0;
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
            return Err(RP_EBAL);
        }
        if self.ok_count() != 8 || self.err_count() != 6 {
            return Err(RP_EBAL);
        }
        let mut i = 0;
        while i < self.len {
            match self.rows.get(i) {
                Some(s) => {
                    if s.slot as usize != i {
                        return Err(RP_EORDER);
                    }
                    if s.ok {
                        if s.code != V_OK {
                            return Err(RP_EMISMATCH);
                        }
                    } else if s.code == V_OK || s.value != 0 {
                        return Err(RP_EMISMATCH);
                    }
                }
                None => return Err(RP_ENOBS),
            }
            i += 1;
        }
        let vals = [0u32, 64, 128, 256, 4, 64, 288, 4];
        let mut k = 0;
        while k < vals.len() {
            match (self.sample(k), vals.get(k)) {
                (Some(s), Some(w)) => {
                    if !s.ok || s.value != *w {
                        return Err(RP_EMISMATCH);
                    }
                }
                _ => return Err(RP_EBAL),
            }
            k += 1;
        }
        let codes = [
            RP_ESIZE,
            RP_ENOMEM,
            RP_EIDX,
            RP_EDOUBLE,
            RP_EDVTLEN,
            RP_EEMPTY,
        ];
        let mut c = 0;
        let mut j = 8;
        while j < self.len {
            match self.rows.get(j) {
                Some(s) => match codes.get(c) {
                    Some(w) => {
                        if s.code != *w {
                            return Err(RP_EMISMATCH);
                        }
                    }
                    None => return Err(RP_EBAL),
                },
                None => return Err(RP_ENOBS),
            }
            c += 1;
            j += 1;
        }
        if c != codes.len() {
            return Err(RP_EBAL);
        }
        Ok(())
    }

    /// 熔断票号。
    pub fn seal(&self) -> u64 {
        let mut h = FNV_OFF;
        let mut i = 0;
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
            return Err(RP_EFROZEN);
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
            return Err(RP_EBAL);
        }
        let h = fresh.seal();
        if self.frozen && h != self.seal {
            return Err(RP_ESEAL);
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

/// 票号不一致。
pub const RP_ESEAL: u32 = SEG_BASE + 0x12;

/// 由结果生成一格。
pub fn cell(slot: u8, op: u8, r: Result<u32, u32>) -> Sample {
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

/// 分配结果取偏移。
pub fn add_cell(slot: u8, r: Result<(u32, u32), u32>) -> Sample {
    match r {
        Ok((_, off)) => Sample {
            slot,
            op: OP_ADD,
            ok: true,
            code: V_OK,
            value: off,
        },
        Err(c) => Sample {
            slot,
            op: OP_ADD,
            ok: false,
            code: c,
            value: 0,
        },
    }
}

/// 构建十四格账本。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();
    let mut d = Dtv::new();

    let _ = l.register(add_cell(0, d.add(64)));
    let _ = l.register(add_cell(1, d.add(64)));
    let _ = l.register(add_cell(2, d.add(128)));
    let _ = l.register(add_cell(3, d.add(32)));

    let p = d.probe();
    let _ = l.register(Sample {
        slot: 4,
        op: OP_PROBE,
        ok: p.dtv_len == d.dtv_len() && p.mods == d.mods(),
        code: V_OK,
        value: p.mods,
    });

    let _ = l.register(cell(5, OP_REMOVE, d.remove(1)));
    let _ = l.register(add_cell(6, d.add(32)));
    let _ = l.register(Sample {
        slot: 7,
        op: OP_PROBE,
        ok: true,
        code: V_OK,
        value: d.mods(),
    });

    let _ = l.register(add_cell(8, d.add(0)));
    let _ = l.register(add_cell(9, d.add(96)));
    let _ = l.register(cell(10, OP_REMOVE, d.remove(9)));
    let _ = d.remove(2);
    let _ = l.register(cell(11, OP_REMOVE, d.remove(2)));

    let bad = Report {
        dtv_len: 99,
        mods: 4,
        surplus: 0,
        gen: GEN,
        digest: d.digest(),
    };
    let _ = l.register(Sample {
        slot: 12,
        op: OP_CHECK,
        ok: d.check(&bad).is_ok(),
        code: match d.check(&bad) {
            Ok(_) => V_OK,
            Err(c) => c,
        },
        value: 0,
    });

    let empty = Dtv::new();
    let _ = l.register(cell(13, OP_LINE, empty.line()));

    l
}

/// 四路注入：账本重放、三量一致、自省面守恒、闸门齐全。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];

    let l = build_ledger();
    let base = l.len() == SAMPLES && l.audit().is_ok();
    let rep = l.replay();
    let root = l.seal();
    out[0] = base && rep == Ok(root) && root != 0;

    let mut d = Dtv::new();
    let _ = d.add(64);
    let _ = d.add(64);
    let _ = d.add(32);
    let p = d.probe();
    out[1] = p.mods == 3
        && p.dtv_len == GEN + 3
        && p.surplus == CAP - 160
        && p.gen == GEN
        && p.digest == d.digest()
        && d.check(&p).is_ok();

    let mut q = Dtv::new();
    let mut i = 0;
    let mut good = true;
    while i < 100 {
        match q.add(16) {
            Ok((slot, off)) => {
                if slot != 0 || off != 0 {
                    good = false;
                }
                if q.remove(slot as usize) != Ok(0) {
                    good = false;
                }
            }
            Err(_) => good = false,
        }
        i += 1;
    }
    out[2] = good && q.mods() == 0 && q.cursor() == 16 && q.surplus() == CAP - 16;

    out[3] = probe_gates();

    out
}

/// 自省面闸门：四类篡改与三种调用错误。
pub fn probe_gates() -> bool {
    let mut d = Dtv::new();
    let _ = d.add(32);
    let _ = d.add(32);
    let ok = d.probe();
    if d.check(&ok).is_err() {
        return false;
    }
    let mut r = ok;
    r.dtv_len = 7;
    if d.check(&r) != Err(RP_EDVTLEN) {
        return false;
    }
    let mut r2 = ok;
    r2.mods = (SLOTS + 1) as u32;
    if d.check(&r2) != Err(RP_EMODS) {
        return false;
    }
    let mut r3 = ok;
    r3.surplus = CAP + 1;
    if d.check(&r3) != Err(RP_ESURR) {
        return false;
    }
    let mut r4 = ok;
    r4.digest ^= 1;
    if d.check(&r4) != Err(RP_EDIGEST) {
        return false;
    }
    if Dtv::new().line() != Err(RP_EEMPTY) {
        return false;
    }
    if Dtv::new().remove(0) != Err(RP_EDOUBLE) {
        return false;
    }
    if Dtv::new().remove(SLOTS) != Err(RP_EIDX) {
        return false;
    }
    let mut full = Dtv::new();
    let mut i = 0;
    while i < SLOTS {
        if full.add(8) != Ok((i as u32, (i * 8) as u32)) {
            return false;
        }
        i += 1;
    }
    if full.add(8) != Err(RP_EFULL) {
        return false;
    }
    true
}

/// 十三项判据。
pub fn run_unx11317_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11317.tlsintros");
    let l = build_ledger();

    s.add(
        "RP1-十四格齐备",
        l.len() == SAMPLES && !l.is_empty() && l.sample(0).map(|x| x.op) == Some(OP_ADD),
        "static",
    );

    s.add_dyn(
        "RP2-八允六拒分布正确",
        l.ok_count() == 8 && l.err_count() == 6 && l.audit().is_ok(),
        &format!("ok={} err={}", l.ok_count(), l.err_count()),
    );

    s.add(
        "RP3-偏移轨迹",
        l.sample(0).map(|x| x.value) == Some(0)
            && l.sample(1).map(|x| x.value) == Some(64)
            && l.sample(2).map(|x| x.value) == Some(128)
            && l.sample(3).map(|x| x.value) == Some(256),
        "static",
    );

    s.add(
        "RP4-自省三量一致",
        {
            let mut d = Dtv::new();
            let _ = d.add(64);
            let _ = d.add(64);
            let _ = d.add(128);
            let _ = d.add(32);
            let p = d.probe();
            p.mods == 4
                && p.dtv_len == GEN + 4
                && p.surplus == CAP - 288
                && p.digest == d.digest()
                && d.bytes() == 288
        },
        "static",
    );

    s.add(
        "RP5-释放与模块数衰减",
        {
            let mut d = Dtv::new();
            let _ = d.add(64);
            let _ = d.add(64);
            let _ = d.add(128);
            let _ = d.add(32);
            let r = d.remove(1);
            r == Ok(64) && d.mods() == 3 && d.surplus() == CAP - 288 && d.bytes() == 224
        },
        "static",
    );

    s.add(
        "RP6-同尺寸复用",
        {
            let mut d = Dtv::new();
            let a = d.add(32);
            let rm = d.remove(0);
            let b = d.add(32);
            a == Ok((0, 0))
                && rm == Ok(0)
                && b == Ok((0, 0))
                && d.cursor() == 32
                && d.free_blocks() == 0
                && d.digest() != 0
        },
        "static",
    );

    s.add(
        "RP7-余量归零闸门",
        l.sample(9).map(|x| x.code) == Some(RP_ENOMEM)
            && {
                let mut d = Dtv::new();
                d.add(320) == Ok((0, 0)) && d.surplus() == 0 && d.add(1) == Err(RP_ENOMEM)
            },
        "static",
    );

    s.add(
        "RP8-尺寸零与槽满闸门",
        l.sample(8).map(|x| x.code) == Some(RP_ESIZE)
            && {
                let mut d = Dtv::new();
                let mut i = 0;
                let mut all = true;
                while i < SLOTS {
                    if d.add(8) != Ok((i as u32, (i * 8) as u32)) {
                        all = false;
                    }
                    i += 1;
                }
                all && d.add(8) == Err(RP_EFULL)
            },
        "static",
    );

    s.add(
        "RP9-下标闸门",
        l.sample(10).map(|x| x.code) == Some(RP_EIDX)
            && l.sample(11).map(|x| x.code) == Some(RP_EDOUBLE)
            && {
                let mut d = Dtv::new();
                let _ = d.add(8);
                d.remove(0) == Ok(0) && d.remove(0) == Err(RP_EDOUBLE) && d.remove(99) == Err(RP_EIDX)
            },
        "static",
    );

    s.add(
        "RP10-自报自检闭合",
        {
            let mut d = Dtv::new();
            let _ = d.add(16);
            let _ = d.add(16);
            let p = d.probe();
            d.check(&p).is_ok() && d.check(&d.probe()).is_ok()
        },
        "static",
    );

    s.add(
        "RP11-四类篡改检出",
        l.sample(12).map(|x| x.code) == Some(RP_EDVTLEN) && {
            let mut d = Dtv::new();
            let _ = d.add(32);
            let base = d.probe();
            let mut a = base;
            a.mods = (SLOTS + 1) as u32;
            let mut b = base;
            b.surplus = CAP + 1;
            let mut c = base;
            c.digest = base.digest ^ 0xffff;
            d.check(&a) == Err(RP_EMODS)
                && d.check(&b) == Err(RP_ESURR)
                && d.check(&c) == Err(RP_EDIGEST)
                && d.check(&base).is_ok()
        },
        "static",
    );

    s.add(
        "RP12-空面拒绝",
        l.sample(13).map(|x| x.code) == Some(RP_EEMPTY)
            && Dtv::new().line() == Err(RP_EEMPTY)
            && {
                let mut d = Dtv::new();
                let _ = d.add(8);
                d.line().is_ok() && d.line() == Ok(d.digest()) && d.digest() != 0
            },
        "static",
    );

    s.add(
        "RP13-冻结重放与四路注入",
        {
            let mut f = build_ledger();
            let rep = f.replay();
            let one = f.freeze();
            let two = f.freeze();
            let inj = inject_four();
            one.is_ok()
                && one == rep
                && two == Err(RP_EFROZEN)
                && f.rejects() == 1
                && f.is_frozen()
                && f.root() != 0
                && inj == [true, true, true, true]
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
        assert!(l.audit().is_ok());
    }

    #[test]
    fn checks_pass() {
        let s = run_unx11317_checks();
        assert_eq!(s.len(), 13);
        assert!(s.all_passed());
    }

    #[test]
    fn dtv_len_tracks_generation() {
        let mut d = Dtv::new();
        assert_eq!(d.dtv_len(), GEN);
        assert_eq!(d.add(16), Ok((0, 0)));
        assert_eq!(d.dtv_len(), GEN + 1);
        assert_eq!(d.remove(0), Ok(0));
        assert_eq!(d.dtv_len(), GEN);
    }

    #[test]
    fn surplus_is_cursor_based() {
        let mut d = Dtv::new();
        assert_eq!(d.add(64), Ok((0, 0)));
        assert_eq!(d.remove(0), Ok(0));
        assert_eq!(d.surplus(), CAP - 64);
        assert_eq!(d.add(32), Ok((0, 64)));
        assert_eq!(d.surplus(), CAP - 96);
    }

    #[test]
    fn digest_changes_on_use() {
        let mut d = Dtv::new();
        let empty = d.digest();
        let _ = d.add(8);
        assert_ne!(d.digest(), empty);
        let _ = d.remove(0);
        assert_eq!(d.digest(), empty);
    }

    #[test]
    fn check_rejects_tampering() {
        let mut d = Dtv::new();
        let _ = d.add(32);
        let base = d.probe();
        assert!(d.check(&base).is_ok());
        let mut bad = base;
        bad.dtv_len += 1;
        assert_eq!(d.check(&bad), Err(RP_EDVTLEN));
        let mut worse = base;
        worse.mods = 99;
        assert_eq!(d.check(&worse), Err(RP_EMODS));
    }

    #[test]
    fn line_requires_live_module() {
        assert_eq!(Dtv::new().line(), Err(RP_EEMPTY));
    }

    #[test]
    fn freeze_then_replay_matches() {
        let mut l = build_ledger();
        let root = l.seal();
        assert_eq!(l.freeze(), Ok(root));
        assert_eq!(l.replay(), Ok(root));
        assert_eq!(l.freeze(), Err(RP_EFROZEN));
    }
}
