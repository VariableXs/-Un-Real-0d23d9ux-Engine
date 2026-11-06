//! UNX-F11499 高频 20 工具快照库双机汇总验证（0x20f00 段）
//!
//! 快照库把每个工具的一次观测压成 (工具, 全局序号, 载荷摘要) 三元组。采集顺序
//! 因机器而异：A 机按名册正序抓，B 机按名册逆序抓；序号是全局唯一的流水号，与
//! 采集顺序无关，摘要只由载荷决定。汇总时按工具下标重新排序再折叠，两台机器就
//! 必须得出同一个票号——顺序不是证据，集合才是。
//!
//! 采集口按工具维 → 重复工具 → 重号 → 载荷上限 → 容量过闸。同一工具在一座库里
//! 只能有一条快照（EDUP），同一序号也只能出现一次（ESEQ），载荷超过六十四字节
//! 是 EPAY。比较入口先拒绝空库（EEMPTY），再比规范化票号，不等即 EMISMATCH。
//!
//! 账本十四格：八格成功覆盖两库规模、首尾采集下标、序号唯一与双机同码；六格
//! 拒绝依次是工具越界、重复工具、重号、容量满、载荷越界与票号不符。

use crate::alloc::format;
use crate::checks::CheckSet;

/// 段基址。
pub const SEG_BASE: u32 = 0x20f00;

/// 编号非法。
pub const SN_EID: u32 = SEG_BASE + 0x01;
/// 登记顺序错误。
pub const SN_EORDER: u32 = SEG_BASE + 0x02;
/// 重复登记。
pub const SN_EDUP: u32 = SEG_BASE + 0x03;
/// 已冻结。
pub const SN_EFROZEN: u32 = SEG_BASE + 0x04;
/// 观测不存在的格。
pub const SN_ENOBS: u32 = SEG_BASE + 0x05;
/// 结论不符。
pub const SN_EMISMATCH: u32 = SEG_BASE + 0x06;
/// 账本失衡。
pub const SN_EBAL: u32 = SEG_BASE + 0x07;
/// 容量满。
pub const SN_EFULL: u32 = SEG_BASE + 0x08;
/// 票号不一致。
pub const SN_ESEAL: u32 = SEG_BASE + 0x09;
/// 工具越界。
pub const SN_ETOOL: u32 = SEG_BASE + 0x0a;
/// 序号重复。
pub const SN_ESEQ: u32 = SEG_BASE + 0x0b;
/// 载荷越界。
pub const SN_EPAY: u32 = SEG_BASE + 0x0c;
/// 空库不可比。
pub const SN_EEMPTY: u32 = SEG_BASE + 0x0d;

/// 判决：成功。
pub const V_OK: u32 = 0;

/// 工具数。
pub const TOOLS: usize = 20;
/// 单库快照上限。
pub const SNAP_CAP: usize = 20;
/// 载荷上限。
pub const PAY_CAP: usize = 64;
/// 序号基。
pub const SEQ_BASE: u32 = 100;
/// 样本格数。
pub const SAMPLES: usize = 14;

/// 操作：库规模。
pub const OP_CAP: u8 = 1;
/// 操作：采集。
pub const OP_CAPTURE: u8 = 2;
/// 操作：规范化票号。
pub const OP_CANON: u8 = 3;
/// 操作：双库比较。
pub const OP_SAME: u8 = 4;
/// 操作：序号统计。
pub const OP_SEQ: u8 = 5;

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

/// 一条快照。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Snap {
    pub tool: u8,
    pub seq: u32,
    pub digest: u64,
}

impl Snap {
    /// 空白快照。
    pub const fn blank() -> Snap {
        Snap {
            tool: 0,
            seq: 0,
            digest: 0,
        }
    }
}

/// 载荷摘要：长度与字节一起折入。
pub fn digest_payload(payload: &[u8]) -> u64 {
    let mut h = fold(FNV_OFF, payload.len() as u64);
    let mut i = 0usize;
    while i < payload.len() {
        if let Some(&b) = payload.get(i) {
            h = fold(h, b as u64);
        }
        i += 1;
    }
    h
}

/// 某工具的观测载荷。
pub fn payload_for(tool: u8) -> [u8; 3] {
    [b'p', tool, tool ^ 0x5a]
}

/// 快照库。
#[derive(Clone, Copy, Debug)]
pub struct Library {
    rows: [Snap; SNAP_CAP],
    len: usize,
    cap: usize,
}

impl Library {
    /// 空库。
    pub const fn new() -> Library {
        Library {
            rows: [Snap::blank(); SNAP_CAP],
            len: 0,
            cap: SNAP_CAP,
        }
    }

    /// 指定容量。
    pub const fn with_cap(cap: usize) -> Library {
        Library {
            rows: [Snap::blank(); SNAP_CAP],
            len: 0,
            cap,
        }
    }

    /// 采集：工具 → 重复工具 → 重号 → 载荷 → 容量。
    pub fn capture(&mut self, tool: u8, seq: u32, payload: &[u8]) -> Result<usize, u32> {
        if (tool as usize) >= TOOLS {
            return Err(SN_ETOOL);
        }
        let mut i = 0usize;
        while i < self.len {
            if let Some(r) = self.rows.get(i) {
                if r.tool == tool {
                    return Err(SN_EDUP);
                }
                if r.seq == seq {
                    return Err(SN_ESEQ);
                }
            }
            i += 1;
        }
        if payload.len() > PAY_CAP {
            return Err(SN_EPAY);
        }
        if self.len >= self.cap || self.len >= SNAP_CAP {
            return Err(SN_EFULL);
        }
        let snap = Snap {
            tool,
            seq,
            digest: digest_payload(payload),
        };
        match self.rows.get_mut(self.len) {
            Some(x) => *x = snap,
            None => return Err(SN_EFULL),
        }
        let slot = self.len;
        self.len += 1;
        Ok(slot)
    }

    /// 格数。
    pub fn len(&self) -> usize {
        self.len
    }

    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// 取快照。
    pub fn sample(&self, i: usize) -> Option<Snap> {
        if i >= self.len {
            return None;
        }
        self.rows.get(i).copied()
    }

    /// 某工具的快照。
    pub fn of_tool(&self, tool: u8) -> Option<Snap> {
        let mut i = 0usize;
        while i < self.len {
            if let Some(r) = self.rows.get(i) {
                if r.tool == tool {
                    return Some(*r);
                }
            }
            i += 1;
        }
        None
    }

    /// 序号去重后的条数。
    pub fn unique_seqs(&self) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < self.len {
            let mut first = true;
            let mut j = 0usize;
            while j < i {
                if let (Some(a), Some(b)) = (self.rows.get(i), self.rows.get(j)) {
                    if a.seq == b.seq {
                        first = false;
                    }
                }
                j += 1;
            }
            if first {
                n += 1;
            }
            i += 1;
        }
        n
    }

    /// 规范化票号：按工具下标重排后折叠。
    pub fn canon(&self) -> u64 {
        let mut h = FNV_OFF;
        let mut tool = 0u8;
        while (tool as usize) < TOOLS {
            if let Some(r) = self.of_tool(tool) {
                h = fold(h, r.tool as u64);
                h = fold(h, r.seq as u64);
                h = fold(h, r.digest);
            }
            tool += 1;
        }
        h
    }

    /// 双库同码。
    pub fn same(&self, other: &Library) -> Result<(), u32> {
        if self.is_empty() || other.is_empty() {
            return Err(SN_EEMPTY);
        }
        if self.canon() == other.canon() {
            Ok(())
        } else {
            Err(SN_EMISMATCH)
        }
    }
}

/// 正序采集：工具 0..19。
pub fn asc_library() -> Library {
    let mut l = Library::new();
    let mut tool = 0u8;
    while (tool as usize) < TOOLS {
        let p = payload_for(tool);
        let seq = SEQ_BASE + tool as u32 * 2;
        let _ = l.capture(tool, seq, &p);
        tool += 1;
    }
    l
}

/// 逆序采集：工具 19..0。
pub fn desc_library() -> Library {
    let mut l = Library::new();
    let mut tool = TOOLS as u8;
    while tool > 0 {
        tool -= 1;
        let p = payload_for(tool);
        let seq = SEQ_BASE + tool as u32 * 2;
        let _ = l.capture(tool, seq, &p);
    }
    l
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
            return Err(SN_EFROZEN);
        }
        if (row.slot as usize) >= SAMPLES {
            return Err(SN_EID);
        }
        if (row.slot as usize) < self.len {
            return Err(SN_EDUP);
        }
        if (row.slot as usize) != self.len {
            return Err(SN_EORDER);
        }
        if self.len >= self.cap {
            return Err(SN_EFULL);
        }
        match self.rows.get_mut(self.len) {
            Some(x) => *x = row,
            None => return Err(SN_EID),
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
            return Err(SN_EBAL);
        }
        if self.ok_count() != 8 || self.err_count() != 6 {
            return Err(SN_EBAL);
        }
        let mut i = 0usize;
        while i < self.len {
            match self.rows.get(i) {
                Some(s) => {
                    if s.slot as usize != i {
                        return Err(SN_EORDER);
                    }
                    if s.ok {
                        if s.code != V_OK {
                            return Err(SN_EMISMATCH);
                        }
                    } else if s.code == V_OK || s.value != 0 {
                        return Err(SN_EMISMATCH);
                    }
                }
                None => return Err(SN_ENOBS),
            }
            i += 1;
        }
        let vals = [20u32, 20, 0, 19, 19, 0, 1, 0];
        let mut k = 0usize;
        while k < vals.len() {
            match (self.sample(k), vals.get(k)) {
                (Some(s), Some(w)) => {
                    if !s.ok || s.value != *w {
                        return Err(SN_EMISMATCH);
                    }
                }
                _ => return Err(SN_EBAL),
            }
            k += 1;
        }
        let codes = [SN_ETOOL, SN_EDUP, SN_ESEQ, SN_EFULL, SN_EPAY, SN_EMISMATCH];
        let mut c = 0usize;
        let mut j = 8usize;
        while j < self.len {
            match self.rows.get(j) {
                Some(s) => match codes.get(c) {
                    Some(w) => {
                        if s.code != *w {
                            return Err(SN_EMISMATCH);
                        }
                    }
                    None => return Err(SN_EBAL),
                },
                None => return Err(SN_ENOBS),
            }
            c += 1;
            j += 1;
        }
        if c != codes.len() {
            return Err(SN_EBAL);
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
            return Err(SN_EFROZEN);
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
            return Err(SN_EBAL);
        }
        let h = fresh.seal();
        if self.frozen && h != self.seal {
            return Err(SN_ESEAL);
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
    let a = asc_library();
    let d = desc_library();

    let _ = l.register(cell(0, OP_CAP, Ok(a.len() as u32)));
    let _ = l.register(cell(1, OP_CAP, Ok(d.len() as u32)));
    let _ = l.register(cell(2, OP_SEQ, a.sample(0).map(|s| s.tool as u32).ok_or(0)));
    let _ = l.register(cell(3, OP_SEQ, d.sample(0).map(|s| s.tool as u32).ok_or(0)));
    let _ = l.register(cell(4, OP_SEQ, a.sample(19).map(|s| s.tool as u32).ok_or(0)));
    let _ = l.register(cell(5, OP_SEQ, d.sample(19).map(|s| s.tool as u32).ok_or(0)));
    let _ = l.register(cell(6, OP_CANON, Ok((a.canon() == d.canon()) as u32)));
    let _ = l.register(cell(7, OP_SAME, a.same(&d).map(|_| 0)));

    let _ = l.register(cell(8, OP_CAPTURE, Library::new().capture(TOOLS as u8, 1, b"x").map(|n| n as u32)));
    let mut dup = Library::new();
    let _ = dup.capture(3, SEQ_BASE + 6, b"x");
    let _ = l.register(cell(9, OP_CAPTURE, dup.capture(3, SEQ_BASE + 8, b"y").map(|n| n as u32)));
    let mut seq = Library::new();
    let _ = seq.capture(3, SEQ_BASE + 6, b"x");
    let _ = l.register(cell(10, OP_CAPTURE, seq.capture(4, SEQ_BASE + 6, b"y").map(|n| n as u32)));
    let mut cap = Library::with_cap(2);
    let _ = cap.capture(0, SEQ_BASE, b"a");
    let _ = cap.capture(1, SEQ_BASE + 2, b"b");
    let _ = l.register(cell(11, OP_CAPTURE, cap.capture(2, SEQ_BASE + 4, b"c").map(|n| n as u32)));
    let big = [0u8; 65];
    let _ = l.register(cell(12, OP_CAPTURE, Library::new().capture(0, SEQ_BASE, &big).map(|n| n as u32)));
    let mut off = desc_library();
    match off.rows.get_mut(0) {
        Some(r) => r.seq = r.seq.wrapping_add(1),
        None => {}
    }
    let _ = l.register(cell(13, OP_SAME, off.same(&a).map(|_| 0)));

    l
}

/// 四路注入：账本自洽、双库同码、采集门序、拒绝后恢复。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];

    let mut l = build_ledger();
    let base = l.len() == SAMPLES && l.audit().is_ok();
    let rep = l.replay();
    let root = match l.freeze() {
        Ok(v) => v,
        Err(_) => 0,
    };
    let again = l.freeze() == Err(SN_EFROZEN);
    out[0] = base && root != 0 && rep == Ok(root) && again && l.rejects() == 1;

    let a = asc_library();
    let d = desc_library();
    out[1] = a.len() == TOOLS
        && d.len() == TOOLS
        && a.sample(0).map(|s| s.tool) == Some(0)
        && d.sample(0).map(|s| s.tool) == Some(19)
        && a.canon() == d.canon()
        && a.same(&d).is_ok();

    let mut f = Library::new();
    let first = f.capture(3, SEQ_BASE + 6, b"P");
    let dup = f.capture(3, SEQ_BASE + 8, b"P") == Err(SN_EDUP);
    let seq = f.capture(4, SEQ_BASE + 6, b"P") == Err(SN_ESEQ);
    let next = f.capture(4, SEQ_BASE + 8, b"Q");
    out[2] = first == Ok(0) && dup && seq && next == Ok(1) && f.unique_seqs() == 2;

    let tool = Library::new().capture(TOOLS as u8, 1, b"x") == Err(SN_ETOOL);
    let pay = Library::new().capture(0, 1, &[0u8; 65]) == Err(SN_EPAY);
    let empty = Library::new().same(&a) == Err(SN_EEMPTY);
    let recovered = Library::new().capture(0, 1, b"x") == Ok(0);
    out[3] = tool && pay && empty && recovered;
    out
}

/// 十三判据。
pub fn run_unx11499_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11499.snapshot");
    let l = build_ledger();
    let a = asc_library();
    let d = desc_library();

    s.add(
        "SN1-十四格齐备",
        l.len() == SAMPLES && !l.is_empty() && l.sample(0).map(|x| x.op) == Some(OP_CAP),
        "static",
    );
    s.add_dyn(
        "SN2-八允六拒分布",
        l.ok_count() == 8 && l.err_count() == 6 && l.audit().is_ok(),
        &format!("ok={} err={} audit={:?}", l.ok_count(), l.err_count(), l.audit()),
    );
    s.add(
        "SN3-正序采集",
        a.len() == TOOLS
            && a.sample(0).map(|x| x.tool) == Some(0)
            && a.sample(19).map(|x| x.tool) == Some(19)
            && a.of_tool(7).map(|x| x.seq) == Some(SEQ_BASE + 14)
            && l.sample(0).map(|x| x.value) == Some(20),
        "static",
    );
    s.add(
        "SN4-逆序采集",
        d.len() == TOOLS
            && d.sample(0).map(|x| x.tool) == Some(19)
            && d.sample(19).map(|x| x.tool) == Some(0)
            && d.of_tool(7).map(|x| x.seq) == Some(SEQ_BASE + 14),
        "static",
    );
    s.add(
        "SN5-序号唯一",
        a.unique_seqs() == TOOLS
            && d.unique_seqs() == TOOLS
            && a.of_tool(0).map(|x| x.seq) == Some(SEQ_BASE)
            && a.of_tool(19).map(|x| x.seq) == Some(SEQ_BASE + 38),
        "static",
    );
    s.add(
        "SN6-摘要随载荷",
        digest_payload(b"abc") == digest_payload(b"abc")
            && digest_payload(b"abc") != digest_payload(b"abd")
            && digest_payload(b"abc") != digest_payload(b"abc\0")
            && a.of_tool(5).map(|x| x.digest) == d.of_tool(5).map(|x| x.digest),
        "static",
    );
    s.add(
        "SN7-规范化聚合",
        a.canon() == d.canon()
            && a.canon() != 0
            && l.sample(6).map(|x| x.value) == Some(1),
        "static",
    );
    s.add(
        "SN8-双机同码",
        a.same(&d) == Ok(())
            && l.sample(7).map(|x| x.value) == Some(0),
        "static",
    );
    s.add(
        "SN9-工具越界",
        Library::new().capture(TOOLS as u8, 1, b"x") == Err(SN_ETOOL)
            && l.sample(8).map(|x| x.code) == Some(SN_ETOOL),
        "static",
    );
    s.add(
        "SN10-重复工具",
        {
            let mut f = Library::new();
            f.capture(3, SEQ_BASE + 6, b"x").is_ok()
                && f.capture(3, SEQ_BASE + 8, b"y") == Err(SN_EDUP)
                && f.len() == 1
                && l.sample(9).map(|x| x.code) == Some(SN_EDUP)
        },
        "static",
    );
    s.add(
        "SN11-重号",
        {
            let mut f = Library::new();
            f.capture(3, SEQ_BASE + 6, b"x").is_ok()
                && f.capture(4, SEQ_BASE + 6, b"y") == Err(SN_ESEQ)
                && f.capture(4, SEQ_BASE + 8, b"y") == Ok(1)
        },
        "static",
    );
    s.add(
        "SN12-容量与载荷",
        {
            let mut f = Library::with_cap(2);
            f.capture(0, SEQ_BASE, b"a").is_ok()
                && f.capture(1, SEQ_BASE + 2, b"b").is_ok()
                && f.capture(2, SEQ_BASE + 4, b"c") == Err(SN_EFULL)
                && Library::new().capture(0, 1, &[0u8; 65]) == Err(SN_EPAY)
                && l.sample(11).map(|x| x.code) == Some(SN_EFULL)
                && l.sample(12).map(|x| x.code) == Some(SN_EPAY)
        },
        "static",
    );
    let mut off = desc_library();
    match off.rows.get_mut(0) {
        Some(r) => r.seq = r.seq.wrapping_add(1),
        None => {}
    }
    let mut f = build_ledger();
    let rep = f.replay();
    let root = match f.freeze() {
        Ok(v) => v,
        Err(_) => 0,
    };
    let again = f.freeze() == Err(SN_EFROZEN);
    let inj = inject_four();
    s.add_dyn(
        "SN13-票号不符与冻结重放",
        off.same(&a) == Err(SN_EMISMATCH)
            && Library::new().same(&a) == Err(SN_EEMPTY)
            && l.sample(13).map(|x| x.code) == Some(SN_EMISMATCH)
            && root != 0
            && rep == Ok(root)
            && again
            && f.root() == root
            && inj == [true; 4],
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
    fn ascending_and_descending_agree() {
        let a = asc_library();
        let d = desc_library();
        assert_eq!(a.len(), TOOLS);
        assert_eq!(d.len(), TOOLS);
        assert_eq!(a.sample(0).map(|s| s.tool), Some(0));
        assert_eq!(d.sample(0).map(|s| s.tool), Some(19));
        assert_eq!(a.canon(), d.canon());
        assert_eq!(a.same(&d), Ok(()));
    }

    #[test]
    fn capture_gates() {
        let mut f = Library::new();
        assert_eq!(f.capture(3, 106, b"x"), Ok(0));
        assert_eq!(f.capture(3, 108, b"y"), Err(SN_EDUP));
        assert_eq!(f.capture(4, 106, b"y"), Err(SN_ESEQ));
        assert_eq!(f.capture(4, 108, b"y"), Ok(1));
        assert_eq!(f.capture(TOOLS as u8, 120, b"z"), Err(SN_ETOOL));
    }

    #[test]
    fn capacity_and_payload() {
        let mut f = Library::with_cap(2);
        assert!(f.capture(0, 1, b"a").is_ok());
        assert!(f.capture(1, 3, b"b").is_ok());
        assert_eq!(f.capture(2, 5, b"c"), Err(SN_EFULL));
        assert_eq!(Library::new().capture(0, 1, &[0u8; 65]), Err(SN_EPAY));
    }

    #[test]
    fn digests_track_payload() {
        assert_eq!(digest_payload(b"abc"), digest_payload(b"abc"));
        assert_ne!(digest_payload(b"abc"), digest_payload(b"abd"));
        assert_ne!(digest_payload(b"abc"), digest_payload(b"abc\0"));
    }

    #[test]
    fn empty_libraries_are_not_comparable() {
        assert_eq!(Library::new().same(&Library::new()), Err(SN_EEMPTY));
        assert_eq!(Library::new().same(&asc_library()), Err(SN_EEMPTY));
    }

    #[test]
    fn ledger_and_checks() {
        let l = build_ledger();
        assert_eq!(l.len(), SAMPLES);
        assert_eq!(l.ok_count(), 8);
        assert!(l.audit().is_ok());
        let s = run_unx11499_checks();
        assert_eq!(s.len(), 13);
        assert!(s.all_passed(), "{}", s.red_items_text());
        assert_eq!(inject_four(), [true; 4]);
    }
}
