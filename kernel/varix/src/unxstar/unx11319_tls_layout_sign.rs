//! UNX-F11319 双线 TLS 布局真值联签（0xe9e0 段）
//!
//! TLS 布局有两条独立实现线：链接器在装配期按段对齐逐模块排布，运行期
//! 装载器按自己的游标再排一遍。两条线只要有一处对齐口径不同，同一个 TLS
//! 变量在两个时刻就会落到不同偏移——这类错在单线内部永远自洽，只有把两
//! 条线的结果放在一起比才会暴露。
//!
//! 模型让两条线各自独立实现：`line_linker` 逐段累加（先算对齐再落座），
//! `line_runtime` 先求游标再取整。两份结果都以 `Line`（偏移表 + 末址 +
//! 摘要）呈现，`sign` 负责联签：模块数不符记 EBAL，逐模块偏移不符记
//! EDIVERGE，末址不符记 ETAIL，摘要不符记 EDIGEST。四类分开，是为了让
//! 后续 AI 一眼看出是“少装了一个模块”还是“最后一个模块尾巴算错”。
//!
//! 联签票号幂等：同一对线重复联签必须得到同一票号，否则说明签名过程本身
//! 带了隐藏状态。票号非零是“已经签过”的最低证据。
//!
//! 账本十四格：八格成功（四条偏移、一次联签、一次幂等复签、一次双线比对、
//! 一次末址对账），六格拒绝（尺寸分歧、对齐分歧、模块数分歧、空线、溢出、
//! 摘要分歧）。

use crate::alloc::format;
use crate::checks::CheckSet;

/// 段基址。
pub const SEG_BASE: u32 = 0xe9e0;

/// 编号非法。
pub const DS_EID: u32 = SEG_BASE + 0x01;
/// 登记顺序错误。
pub const DS_EORDER: u32 = SEG_BASE + 0x02;
/// 重复登记。
pub const DS_EDUP: u32 = SEG_BASE + 0x03;
/// 已冻结。
pub const DS_EFROZEN: u32 = SEG_BASE + 0x04;
/// 观测不存在的格。
pub const DS_ENOBS: u32 = SEG_BASE + 0x05;
/// 结论不符。
pub const DS_EMISMATCH: u32 = SEG_BASE + 0x06;
/// 账本失衡。
pub const DS_EBAL: u32 = SEG_BASE + 0x07;
/// 两条线偏移分歧。
pub const DS_EDIVERGE: u32 = SEG_BASE + 0x08;
/// 末址分歧。
pub const DS_ETAIL: u32 = SEG_BASE + 0x09;
/// 摘要分歧。
pub const DS_EDIGEST: u32 = SEG_BASE + 0x0a;
/// 空线。
pub const DS_EEMPTY: u32 = SEG_BASE + 0x0b;
/// 算术溢出。
pub const DS_EOVER: u32 = SEG_BASE + 0x0c;
/// 对齐非法。
pub const DS_EALIGN: u32 = SEG_BASE + 0x0d;
/// 票号不一致。
pub const DS_ESEAL: u32 = SEG_BASE + 0x0e;

/// 判决：成功。
pub const V_OK: u32 = 0;

/// 操作：链接线取偏移。
pub const OP_LINKER: u8 = 1;
/// 操作：运行线取偏移。
pub const OP_RUNTIME: u8 = 2;
/// 操作：联签。
pub const OP_SIGN: u8 = 3;
/// 操作：末址对账。
pub const OP_TAIL: u8 = 4;

/// 模块数。
pub const MODS: usize = 4;
/// 对齐上限。
pub const MAX_ALIGN: u32 = 64;
/// 样本格数。
pub const SAMPLES: usize = 14;

/// FNV 偏移基。
pub const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;
/// FNV 质数。
pub const FNV_PRIME: u64 = 0x100_0000_01b3;

/// 一条布局线的真值视图。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Line {
    pub offs: [u32; MODS],
    pub n: usize,
    pub end: u32,
    pub digest: u64,
}

impl Line {
    /// 空线。
    pub const fn blank() -> Line {
        Line {
            offs: [0; MODS],
            n: 0,
            end: 0,
            digest: 0,
        }
    }

    /// 取偏移。
    pub fn off(&self, i: usize) -> Option<u32> {
        if i >= self.n {
            return None;
        }
        self.offs.get(i).copied()
    }

    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.n == 0
    }
}

/// 模块规格。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Spec {
    pub size: u32,
    pub align: u32,
}

impl Spec {
    /// 构造。
    pub const fn make(size: u32, align: u32) -> Spec {
        Spec { size, align }
    }
}

/// 对齐检查。
pub fn check_align(a: u32) -> Result<(), u32> {
    if a == 0 || !a.is_power_of_two() || a > MAX_ALIGN {
        return Err(DS_EALIGN);
    }
    Ok(())
}

/// 折入。
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

/// 链接线：先对齐、再落座、再累加。
pub fn line_linker(specs: &[Spec; MODS]) -> Result<Line, u32> {
    let mut l = Line::blank();
    let mut cur = 0u32;
    let mut i = 0;
    while i < MODS {
        let s = match specs.get(i) {
            Some(v) => *v,
            None => return Err(DS_EBAL),
        };
        check_align(s.align)?;
        let mask = s.align - 1;
        let base = match cur.checked_add(mask) {
            Some(v) => v & !mask,
            None => return Err(DS_EOVER),
        };
        let end = match base.checked_add(s.size) {
            Some(v) => v,
            None => return Err(DS_EOVER),
        };
        if let Some(x) = l.offs.get_mut(i) {
            *x = base;
        }
        cur = end;
        i += 1;
    }
    l.n = MODS;
    l.end = cur;
    l.digest = digest_of(&l);
    Ok(l)
}

/// 运行线：先取游标、再向上取整、再前进。独立实现，不与链接线共用中间量。
pub fn line_runtime(specs: &[Spec; MODS]) -> Result<Line, u32> {
    let mut l = Line::blank();
    let mut cursor = 0u32;
    let mut i = 0;
    while i < MODS {
        let s = match specs.get(i) {
            Some(v) => *v,
            None => return Err(DS_EBAL),
        };
        check_align(s.align)?;
        let rem = cursor % s.align;
        let pad = if rem == 0 { 0 } else { s.align - rem };
        let base = match cursor.checked_add(pad) {
            Some(v) => v,
            None => return Err(DS_EOVER),
        };
        let end = match base.checked_add(s.size) {
            Some(v) => v,
            None => return Err(DS_EOVER),
        };
        if let Some(x) = l.offs.get_mut(i) {
            *x = base;
        }
        cursor = end;
        i += 1;
    }
    l.n = MODS;
    l.end = cursor;
    l.digest = digest_of(&l);
    Ok(l)
}

/// 摘要：逐模块折入偏移与末址。
pub fn digest_of(l: &Line) -> u64 {
    let mut h = FNV_OFF;
    h = fold(h, l.n as u64);
    let mut i = 0;
    while i < l.n {
        match l.offs.get(i) {
            Some(o) => h = fold(h, *o as u64),
            None => break,
        }
        i += 1;
    }
    fold(h, l.end as u64)
}

/// 联签：四类分歧分档。
pub fn sign(a: &Line, b: &Line) -> Result<u64, u32> {
    if a.is_empty() || b.is_empty() {
        return Err(DS_EEMPTY);
    }
    if a.n != b.n {
        return Err(DS_EBAL);
    }
    let mut i = 0;
    while i < a.n {
        let x = match a.offs.get(i) {
            Some(v) => *v,
            None => return Err(DS_EBAL),
        };
        let y = match b.offs.get(i) {
            Some(v) => *v,
            None => return Err(DS_EBAL),
        };
        if x != y {
            return Err(DS_EDIVERGE);
        }
        i += 1;
    }
    if a.end != b.end {
        return Err(DS_ETAIL);
    }
    if a.digest != b.digest {
        return Err(DS_EDIGEST);
    }
    let mut h = FNV_OFF;
    h = fold(h, a.digest);
    h = fold(h, b.digest);
    h = fold(h, a.end as u64);
    Ok(h)
}

/// 标准四模块：64/64/64/64，全部 64 对齐，偏移 0/64/128/192。
pub const fn std_specs() -> [Spec; MODS] {
    [
        Spec::make(64, 64),
        Spec::make(64, 64),
        Spec::make(64, 64),
        Spec::make(64, 64),
    ]
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
            return Err(DS_EFROZEN);
        }
        if (row.slot as usize) != self.len {
            return Err(DS_EORDER);
        }
        if self.len >= SAMPLES {
            return Err(DS_EID);
        }
        let mut i = 0;
        while i < self.len {
            match self.rows.get(i) {
                Some(x) => {
                    if x.slot == row.slot {
                        return Err(DS_EDUP);
                    }
                }
                None => return Err(DS_ENOBS),
            }
            i += 1;
        }
        match self.rows.get_mut(self.len) {
            Some(x) => *x = row,
            None => return Err(DS_EID),
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
            return Err(DS_EBAL);
        }
        if self.ok_count() != 8 || self.err_count() != 6 {
            return Err(DS_EBAL);
        }
        let mut i = 0;
        while i < self.len {
            match self.rows.get(i) {
                Some(s) => {
                    if s.slot as usize != i {
                        return Err(DS_EORDER);
                    }
                    if s.ok {
                        if s.code != V_OK {
                            return Err(DS_EMISMATCH);
                        }
                    } else if s.code == V_OK || s.value != 0 {
                        return Err(DS_EMISMATCH);
                    }
                }
                None => return Err(DS_ENOBS),
            }
            i += 1;
        }
        let vals = [0u32, 64, 128, 192, 4, 4, 4, 256];
        let mut k = 0;
        while k < vals.len() {
            match (self.sample(k), vals.get(k)) {
                (Some(s), Some(w)) => {
                    if !s.ok || s.value != *w {
                        return Err(DS_EMISMATCH);
                    }
                }
                _ => return Err(DS_EBAL),
            }
            k += 1;
        }
        let codes = [
            DS_EDIVERGE,
            DS_EALIGN,
            DS_EBAL,
            DS_EEMPTY,
            DS_EOVER,
            DS_EDIGEST,
        ];
        let mut c = 0;
        let mut j = 8;
        while j < self.len {
            match self.rows.get(j) {
                Some(s) => match codes.get(c) {
                    Some(w) => {
                        if s.code != *w {
                            return Err(DS_EMISMATCH);
                        }
                    }
                    None => return Err(DS_EBAL),
                },
                None => return Err(DS_ENOBS),
            }
            c += 1;
            j += 1;
        }
        if c != codes.len() {
            return Err(DS_EBAL);
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
            return Err(DS_EFROZEN);
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
            return Err(DS_EBAL);
        }
        let h = fresh.seal();
        if self.frozen && h != self.seal {
            return Err(DS_ESEAL);
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

/// 联签结果折算。
pub fn sign_cell(slot: u8, r: Result<u64, u32>, on_ok: u32) -> Sample {
    match r {
        Ok(_) => Sample {
            slot,
            op: OP_SIGN,
            ok: true,
            code: V_OK,
            value: on_ok,
        },
        Err(c) => Sample {
            slot,
            op: OP_SIGN,
            ok: false,
            code: c,
            value: 0,
        },
    }
}

/// 构建十四格账本。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();
    let specs = std_specs();
    let link = match line_linker(&specs) {
        Ok(v) => v,
        Err(_) => Line::blank(),
    };
    let run = match line_runtime(&specs) {
        Ok(v) => v,
        Err(_) => Line::blank(),
    };

    let _ = l.register(Sample {
        slot: 0,
        op: OP_LINKER,
        ok: link.off(0) == Some(0),
        code: V_OK,
        value: link.off(0).unwrap_or(0),
    });
    let _ = l.register(Sample {
        slot: 1,
        op: OP_LINKER,
        ok: link.off(1) == Some(64),
        code: V_OK,
        value: link.off(1).unwrap_or(0),
    });
    let _ = l.register(Sample {
        slot: 2,
        op: OP_RUNTIME,
        ok: run.off(2) == Some(128),
        code: V_OK,
        value: run.off(2).unwrap_or(0),
    });
    let _ = l.register(Sample {
        slot: 3,
        op: OP_RUNTIME,
        ok: run.off(3) == Some(192),
        code: V_OK,
        value: run.off(3).unwrap_or(0),
    });

    let one = sign(&link, &run);
    let _ = l.register(sign_cell(4, one, MODS as u32));

    let twice = sign(&link, &run);
    let _ = l.register(sign_cell(5, twice, MODS as u32));

    let _ = l.register(Sample {
        slot: 6,
        op: OP_SIGN,
        ok: link == run,
        code: V_OK,
        value: MODS as u32,
    });

    let _ = l.register(Sample {
        slot: 7,
        op: OP_TAIL,
        ok: link.end == 256 && run.end == 256,
        code: V_OK,
        value: link.end,
    });

    let mut width = std_specs();
    if let Some(x) = width.get_mut(1) {
        *x = Spec::make(96, 64);
    }
    let la = match line_linker(&std_specs()) {
        Ok(v) => v,
        Err(_) => Line::blank(),
    };
    let ra = match line_runtime(&width) {
        Ok(v) => v,
        Err(_) => Line::blank(),
    };
    let _ = l.register(sign_cell(8, sign(&la, &ra), 0));

    let badalign = [
        Spec::make(8, 8),
        Spec::make(8, 8),
        Spec::make(8, 96),
        Spec::make(8, 8),
    ];
    let r9 = match line_linker(&badalign) {
        Ok(v) => Ok(v.end),
        Err(c) => Err(c),
    };
    let _ = l.register(cell(9, OP_LINKER, r9));

    let mut short = run;
    short.n = 2;
    short.digest = digest_of(&short);
    let _ = l.register(sign_cell(10, sign(&short, &run), 0));

    let _ = l.register(sign_cell(11, sign(&Line::blank(), &run), 0));

    let huge = [
        Spec::make(u32::MAX, 1),
        Spec::make(1, 1),
        Spec::make(1, 1),
        Spec::make(1, 1),
    ];
    let big = match line_runtime(&huge) {
        Ok(v) => Ok(v.end),
        Err(c) => Err(c),
    };
    let _ = l.register(cell(12, OP_RUNTIME, big));

    let mut forged = run;
    forged.digest = run.digest ^ 0xff;
    let _ = l.register(sign_cell(13, sign(&link, &forged), 0));

    l
}

/// 四路注入：账本重放、双线同值、票号幂等、分歧分档。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];

    let l = build_ledger();
    let base = l.len() == SAMPLES && l.audit().is_ok();
    let rep = l.replay();
    let root = l.seal();
    out[0] = base && rep == Ok(root) && root != 0;

    let specs = std_specs();
    let a = line_linker(&specs);
    let b = line_runtime(&specs);
    out[1] = match (a, b) {
        (Ok(x), Ok(y)) => {
            let mut same = x.n == y.n && x.end == y.end && x.digest == y.digest;
            let mut i = 0;
            while i < MODS {
                if x.off(i) != y.off(i) {
                    same = false;
                }
                i += 1;
            }
            same && sign(&x, &y).is_ok()
        }
        _ => false,
    };

    let mut mixed = std_specs();
    if let Some(x) = mixed.get_mut(1) {
        *x = Spec::make(40, 8);
    }
    out[2] = match (line_linker(&mixed), line_runtime(&mixed)) {
        (Ok(x), Ok(y)) => {
            let t1 = sign(&x, &y);
            let t2 = sign(&x, &y);
            t1.is_ok() && t1 == t2 && x.off(2) == Some(128) && y.off(2) == Some(128)
        }
        _ => false,
    };

    out[3] = {
        let specs2 = std_specs();
        let x = match line_linker(&specs2) {
            Ok(v) => v,
            Err(_) => return out,
        };
        let mut y = x;
        if let Some(v) = y.offs.get_mut(1) {
            *v = v.wrapping_add(8);
        }
        let mut z = x;
        z.end += 64;
        let mut w = x;
        w.digest ^= 1;
        let mut s = x;
        s.n = 0;
        sign(&x, &y) == Err(DS_EDIVERGE)
            && sign(&x, &z) == Err(DS_ETAIL)
            && sign(&x, &w) == Err(DS_EDIGEST)
            && sign(&x, &s) == Err(DS_EEMPTY)
            && sign(&x, &x).is_ok()
    };

    out
}

/// 十三项判据。
pub fn run_unx11319_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11319.layoutsign");
    let l = build_ledger();
    let specs = std_specs();

    s.add(
        "DS1-十四格齐备",
        l.len() == SAMPLES && !l.is_empty() && l.sample(0).map(|x| x.op) == Some(OP_LINKER),
        "static",
    );

    s.add_dyn(
        "DS2-八允六拒分布正确",
        l.ok_count() == 8 && l.err_count() == 6 && l.audit().is_ok(),
        &format!("ok={} err={}", l.ok_count(), l.err_count()),
    );

    s.add(
        "DS3-双线偏移轨迹",
        match (line_linker(&specs), line_runtime(&specs)) {
            (Ok(a), Ok(b)) => {
                a.off(0) == Some(0)
                    && a.off(1) == Some(64)
                    && b.off(2) == Some(128)
                    && b.off(3) == Some(192)
                    && a.end == 256
                    && b.end == 256
            }
            _ => false,
        },
        "static",
    );

    s.add(
        "DS4-两线逐模块同值",
        match (line_linker(&specs), line_runtime(&specs)) {
            (Ok(a), Ok(b)) => {
                let mut same = true;
                let mut i = 0;
                while i < MODS {
                    if a.off(i) != b.off(i) {
                        same = false;
                    }
                    i += 1;
                }
                same
            }
            _ => false,
        },
        "static",
    );

    s.add(
        "DS5-联签票号非零",
        match (line_linker(&specs), line_runtime(&specs)) {
            (Ok(a), Ok(b)) => match sign(&a, &b) {
                Ok(t) => t != 0,
                Err(_) => false,
            },
            _ => false,
        },
        "static",
    );

    s.add(
        "DS6-联签幂等",
        match (line_linker(&specs), line_runtime(&specs)) {
            (Ok(a), Ok(b)) => {
                let t1 = sign(&a, &b);
                let t2 = sign(&a, &b);
                let t3 = sign(&b, &a);
                t1.is_ok() && t1 == t2 && t1 == t3
            }
            _ => false,
        },
        "static",
    );

    s.add(
        "DS7-尺寸分歧检出",
        l.sample(8).map(|x| x.code) == Some(DS_EDIVERGE)
            && {
                let mut w = std_specs();
                if let Some(x) = w.get_mut(2) {
                    *x = Spec::make(96, 64);
                }
                match (line_linker(&std_specs()), line_runtime(&w)) {
                    (Ok(a), Ok(b)) => sign(&a, &b) == Err(DS_EDIVERGE),
                    _ => false,
                }
            },
        "static",
    );

    s.add(
        "DS8-对齐闸门",
        l.sample(9).map(|x| x.code) == Some(DS_EALIGN)
            && {
                let w = [
                    Spec::make(8, 8),
                    Spec::make(8, 8),
                    Spec::make(8, 96),
                    Spec::make(8, 8),
                ];
                line_linker(&w) == Err(DS_EALIGN) && line_runtime(&w) == Err(DS_EALIGN)
            },
        "static",
    );

    s.add(
        "DS9-模块数分歧",
        l.sample(10).map(|x| x.code) == Some(DS_EBAL),
        "static",
    );

    s.add(
        "DS10-空线闸门",
        l.sample(11).map(|x| x.code) == Some(DS_EEMPTY)
            && sign(&Line::blank(), &Line::blank()) == Err(DS_EEMPTY),
        "static",
    );

    s.add(
        "DS11-溢出闸门",
        l.sample(12).map(|x| x.code) == Some(DS_EOVER)
            && {
                let w = [
                    Spec::make(u32::MAX, 1),
                    Spec::make(1, 1),
                    Spec::make(1, 1),
                    Spec::make(1, 1),
                ];
                line_linker(&w) == Err(DS_EOVER) && line_runtime(&w) == Err(DS_EOVER)
            },
        "static",
    );

    s.add(
        "DS12-摘要分歧与对齐闸门",
        l.sample(13).map(|x| x.code) == Some(DS_EDIGEST)
            && check_align(64).is_ok()
            && check_align(96) == Err(DS_EALIGN)
            && check_align(0) == Err(DS_EALIGN)
            && {
                let w = [
                    Spec::make(8, 8),
                    Spec::make(8, 8),
                    Spec::make(8, 8),
                    Spec::make(8, 16),
                ];
                match (line_linker(&w), line_runtime(&w)) {
                    (Ok(a), Ok(b)) => a.off(3) == Some(32) && b.off(3) == Some(32),
                    _ => false,
                }
            },
        "static",
    );

    s.add(
        "DS13-冻结重放与四路注入",
        {
            let mut f = build_ledger();
            let rep = f.replay();
            let one = f.freeze();
            let two = f.freeze();
            let inj = inject_four();
            one.is_ok()
                && one == rep
                && two == Err(DS_EFROZEN)
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
        let s = run_unx11319_checks();
        assert_eq!(s.len(), 13);
        assert!(s.all_passed());
    }

    #[test]
    fn two_lines_agree_on_std() {
        let w = std_specs();
        let a = line_linker(&w);
        let b = line_runtime(&w);
        assert_eq!(a, b);
    }

    #[test]
    fn ragged_sizes_still_agree() {
        let w = [
            Spec::make(40, 8),
            Spec::make(24, 8),
            Spec::make(56, 32),
            Spec::make(16, 8),
        ];
        let a = line_linker(&w);
        let b = line_runtime(&w);
        assert_eq!(a, b);
        if let Ok(x) = a {
            assert_eq!(x.off(1), Some(40));
            assert_eq!(x.off(2), Some(64));
            assert_eq!(x.off(3), Some(120));
            assert_eq!(x.end, 136);
        }
    }

    #[test]
    fn diverging_lines_reject() {
        let w = std_specs();
        let a = match line_linker(&w) {
            Ok(v) => v,
            Err(_) => Line::blank(),
        };
        let mut b = a;
        if let Some(x) = b.offs.get_mut(0) {
            *x = 8;
        }
        assert_eq!(sign(&a, &b), Err(DS_EDIVERGE));
    }

    #[test]
    fn tail_mismatch_is_distinct() {
        let w = std_specs();
        let a = match line_runtime(&w) {
            Ok(v) => v,
            Err(_) => Line::blank(),
        };
        let mut b = a;
        b.end += 1;
        assert_eq!(sign(&a, &b), Err(DS_ETAIL));
    }

    #[test]
    fn align_gate() {
        assert!(check_align(1).is_ok());
        assert!(check_align(64).is_ok());
        assert_eq!(check_align(3), Err(DS_EALIGN));
        assert_eq!(check_align(128), Err(DS_EALIGN));
    }

    #[test]
    fn freeze_then_replay_matches() {
        let mut l = build_ledger();
        let root = l.seal();
        assert_eq!(l.freeze(), Ok(root));
        assert_eq!(l.replay(), Ok(root));
        assert_eq!(l.freeze(), Err(DS_EFROZEN));
    }
}
