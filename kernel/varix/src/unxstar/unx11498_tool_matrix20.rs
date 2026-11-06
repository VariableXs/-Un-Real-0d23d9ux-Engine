//! UNX-F11498 高频 20 工具判据面收口对账（0x20ef0 段）
//!
//! 收口不是把判据数量相加，而是把二十个高频工具各三条判据摆成 20×3 的格子，
//! 逐格登记、逐行检查缺口。行号是工具在固定名册中的下标，列号是判据车道；
//! 名册顺序一旦定下就不再变动，任何“名字找不到”都是 ENAME 而不是静默跳过。
//!
//! 登记口按工具维 → 车道维 → 重复位过闸：越界的工具记 ETOOL，越界的车道记
//! ELANE，同一格二次落笔记 EDUP。行检查只认三格全亮，缺一格就是 EGAP；两半
//! 判据合流时同一格被两边同时认领是 EMERGE——它意味着两份证据对同一格给出了
//! 重复归属，必须先拆清再合。
//!
//! 账本十四格：八格成功覆盖总格数、行满、缺口、工具下标、车道数与半区合流；
//! 六格拒绝依次是工具越界、车道越界、重复落格、行缺口、合流冲突与无名工具。

use crate::alloc::format;
use crate::checks::CheckSet;

/// 段基址。
pub const SEG_BASE: u32 = 0x20ef0;

/// 编号非法。
pub const TM_EID: u32 = SEG_BASE + 0x01;
/// 登记顺序错误。
pub const TM_EORDER: u32 = SEG_BASE + 0x02;
/// 重复登记。
pub const TM_EDUP: u32 = SEG_BASE + 0x03;
/// 已冻结。
pub const TM_EFROZEN: u32 = SEG_BASE + 0x04;
/// 观测不存在的格。
pub const TM_ENOBS: u32 = SEG_BASE + 0x05;
/// 结论不符。
pub const TM_EMISMATCH: u32 = SEG_BASE + 0x06;
/// 账本失衡。
pub const TM_EBAL: u32 = SEG_BASE + 0x07;
/// 容量满。
pub const TM_EFULL: u32 = SEG_BASE + 0x08;
/// 票号不一致。
pub const TM_ESEAL: u32 = SEG_BASE + 0x09;
/// 工具越界。
pub const TM_ETOOL: u32 = SEG_BASE + 0x0a;
/// 车道越界。
pub const TM_ELANE: u32 = SEG_BASE + 0x0b;
/// 行缺口。
pub const TM_EGAP: u32 = SEG_BASE + 0x0c;
/// 合流冲突。
pub const TM_EMERGE: u32 = SEG_BASE + 0x0d;
/// 无名工具。
pub const TM_ENAME: u32 = SEG_BASE + 0x0e;

/// 判决：成功。
pub const V_OK: u32 = 0;

/// 工具数。
pub const TOOLS: usize = 20;
/// 车道数。
pub const LANES: usize = 3;
/// 总格数。
pub const CELLS: usize = TOOLS * LANES;
/// 样本格数。
pub const SAMPLES: usize = 14;

/// 固定名册。
pub const NAMES: [&[u8]; TOOLS] = [
    b"echo", b"dd", b"xargs", b"cat", b"wc", b"tr", b"head", b"tail", b"sort", b"uniq",
    b"cut", b"sed", b"grep", b"find", b"ls", b"rm", b"cp", b"mv", b"chmod", b"stat",
];

/// 操作：整表统计。
pub const OP_BUILD: u8 = 1;
/// 操作：工具维。
pub const OP_TOOL: u8 = 2;
/// 操作：行检查。
pub const OP_ROW: u8 = 3;
/// 操作：合流。
pub const OP_MERGE: u8 = 4;

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

/// 按名字找名册下标。
pub fn tool_index(name: &[u8]) -> Option<usize> {
    let mut i = 0usize;
    while i < TOOLS {
        match NAMES.get(i) {
            Some(&n) => {
                if n == name {
                    return Some(i);
                }
            }
            None => return None,
        }
        i += 1;
    }
    None
}

/// 按名字查下标，缺失记 ENAME。
pub fn lookup(name: &[u8]) -> Result<usize, u32> {
    match tool_index(name) {
        Some(i) => Ok(i),
        None => Err(TM_ENAME),
    }
}

/// 名册下标对应的车道数。
pub fn lane_count(tool: usize) -> Result<u8, u32> {
    if tool >= TOOLS {
        return Err(TM_ETOOL);
    }
    Ok(LANES as u8)
}

/// 20×3 判据位图。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Matrix {
    bits: [u8; CELLS],
}

impl Matrix {
    /// 空表。
    pub const fn new() -> Matrix {
        Matrix { bits: [0u8; CELLS] }
    }

    fn index(tool: usize, lane: usize) -> Option<usize> {
        if tool >= TOOLS || lane >= LANES {
            return None;
        }
        match tool.checked_mul(LANES) {
            Some(base) => base.checked_add(lane),
            None => None,
        }
    }

    /// 取一格。
    pub fn get(&self, tool: usize, lane: usize) -> Option<bool> {
        match Matrix::index(tool, lane) {
            Some(i) => match self.bits.get(i) {
                Some(&v) => Some(v != 0),
                None => None,
            },
            None => None,
        }
    }

    /// 落一格：工具 → 车道 → 重复。
    pub fn set(&mut self, tool: usize, lane: usize) -> Result<(), u32> {
        if tool >= TOOLS {
            return Err(TM_ETOOL);
        }
        if lane >= LANES {
            return Err(TM_ELANE);
        }
        let i = match Matrix::index(tool, lane) {
            Some(v) => v,
            None => return Err(TM_ETOOL),
        };
        match self.bits.get(i) {
            Some(&0) => {}
            Some(_) => return Err(TM_EDUP),
            None => return Err(TM_ETOOL),
        }
        match self.bits.get_mut(i) {
            Some(v) => *v = 1,
            None => return Err(TM_ETOOL),
        }
        Ok(())
    }

    /// 已落格数。
    pub fn closure(&self) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < CELLS {
            match self.bits.get(i) {
                Some(&v) => {
                    if v != 0 {
                        n += 1;
                    }
                }
                None => return n,
            }
            i += 1;
        }
        n
    }

    /// 缺口数。
    pub fn gaps(&self) -> usize {
        CELLS - self.closure()
    }

    /// 某工具三格是否全亮。
    pub fn row_full(&self, tool: usize) -> Result<bool, u32> {
        if tool >= TOOLS {
            return Err(TM_ETOOL);
        }
        let mut lane = 0usize;
        while lane < LANES {
            match self.get(tool, lane) {
                Some(true) => {}
                _ => return Ok(false),
            }
            lane += 1;
        }
        Ok(true)
    }

    /// 整行满格数。
    pub fn complete_rows(&self) -> usize {
        let mut n = 0usize;
        let mut tool = 0usize;
        while tool < TOOLS {
            if self.row_full(tool) == Ok(true) {
                n += 1;
            }
            tool += 1;
        }
        n
    }

    /// 行检查：缺一格即 EGAP。
    pub fn require_full(&self, tool: usize) -> Result<(), u32> {
        match self.row_full(tool) {
            Ok(true) => Ok(()),
            Ok(false) => Err(TM_EGAP),
            Err(c) => Err(c),
        }
    }

    /// 两半合流：同一格被两边认领是 EMERGE。
    pub fn merge(&self, other: &Matrix) -> Result<Matrix, u32> {
        let mut out = *self;
        let mut i = 0usize;
        while i < CELLS {
            let b = match other.bits.get(i) {
                Some(&v) => v,
                None => return Err(TM_EMERGE),
            };
            if b != 0 {
                match out.bits.get(i) {
                    Some(&0) => {}
                    Some(_) => return Err(TM_EMERGE),
                    None => return Err(TM_EMERGE),
                }
                match out.bits.get_mut(i) {
                    Some(v) => *v = 1,
                    None => return Err(TM_EMERGE),
                }
            }
            i += 1;
        }
        Ok(out)
    }
}

/// 全表。
pub fn full() -> Matrix {
    let mut m = Matrix::new();
    let mut tool = 0usize;
    while tool < TOOLS {
        let mut lane = 0usize;
        while lane < LANES {
            let _ = m.set(tool, lane);
            lane += 1;
        }
        tool += 1;
    }
    m
}

/// 半区：每个工具的前两条车道。
pub fn low_half() -> Matrix {
    let mut m = Matrix::new();
    let mut tool = 0usize;
    while tool < TOOLS {
        let _ = m.set(tool, 0);
        let _ = m.set(tool, 1);
        tool += 1;
    }
    m
}

/// 半区：每个工具的第三条车道。
pub fn high_half() -> Matrix {
    let mut m = Matrix::new();
    let mut tool = 0usize;
    while tool < TOOLS {
        let _ = m.set(tool, 2);
        tool += 1;
    }
    m
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
            return Err(TM_EFROZEN);
        }
        if (row.slot as usize) >= SAMPLES {
            return Err(TM_EID);
        }
        if (row.slot as usize) < self.len {
            return Err(TM_EDUP);
        }
        if (row.slot as usize) != self.len {
            return Err(TM_EORDER);
        }
        if self.len >= self.cap {
            return Err(TM_EFULL);
        }
        match self.rows.get_mut(self.len) {
            Some(x) => *x = row,
            None => return Err(TM_EID),
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
            return Err(TM_EBAL);
        }
        if self.ok_count() != 8 || self.err_count() != 6 {
            return Err(TM_EBAL);
        }
        let mut i = 0usize;
        while i < self.len {
            match self.rows.get(i) {
                Some(s) => {
                    if s.slot as usize != i {
                        return Err(TM_EORDER);
                    }
                    if s.ok {
                        if s.code != V_OK {
                            return Err(TM_EMISMATCH);
                        }
                    } else if s.code == V_OK || s.value != 0 {
                        return Err(TM_EMISMATCH);
                    }
                }
                None => return Err(TM_ENOBS),
            }
            i += 1;
        }
        let vals = [60u32, 3, 3, 0, 20, 4, 1, 20];
        let mut k = 0usize;
        while k < vals.len() {
            match (self.sample(k), vals.get(k)) {
                (Some(s), Some(w)) => {
                    if !s.ok || s.value != *w {
                        return Err(TM_EMISMATCH);
                    }
                }
                _ => return Err(TM_EBAL),
            }
            k += 1;
        }
        let codes = [TM_ETOOL, TM_ELANE, TM_EDUP, TM_EGAP, TM_EMERGE, TM_ENAME];
        let mut c = 0usize;
        let mut j = 8usize;
        while j < self.len {
            match self.rows.get(j) {
                Some(s) => match codes.get(c) {
                    Some(w) => {
                        if s.code != *w {
                            return Err(TM_EMISMATCH);
                        }
                    }
                    None => return Err(TM_EBAL),
                },
                None => return Err(TM_ENOBS),
            }
            c += 1;
            j += 1;
        }
        if c != codes.len() {
            return Err(TM_EBAL);
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
            return Err(TM_EFROZEN);
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
            return Err(TM_EBAL);
        }
        let h = fresh.seal();
        if self.frozen && h != self.seal {
            return Err(TM_ESEAL);
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
    let m = full();

    let _ = l.register(cell(0, OP_BUILD, Ok(m.closure() as u32)));
    let _ = l.register(cell(1, OP_TOOL, lane_count(0).map(u32::from)));
    let _ = l.register(cell(2, OP_TOOL, lane_count(TOOLS - 1).map(u32::from)));
    let _ = l.register(cell(3, OP_ROW, Ok(m.gaps() as u32)));
    let _ = l.register(cell(4, OP_ROW, Ok(m.complete_rows() as u32)));
    let _ = l.register(cell(5, OP_TOOL, lookup(b"wc").map(|v| v as u32)));

    let mut one = Matrix::new();
    let _ = one.set(0, 0);
    let _ = l.register(cell(6, OP_BUILD, Ok(one.closure() as u32)));

    let low = low_half();
    let high = high_half();
    let mixed = match low.merge(&high) {
        Ok(v) => v,
        Err(_) => Matrix::new(),
    };
    let _ = l.register(cell(7, OP_MERGE, Ok(mixed.complete_rows() as u32)));

    let _ = l.register(cell(8, OP_TOOL, Matrix::new().set(TOOLS, 0).map(|_| 1)));
    let _ = l.register(cell(9, OP_TOOL, Matrix::new().set(0, LANES).map(|_| 1)));
    let mut twice = Matrix::new();
    let _ = twice.set(0, 0);
    let _ = l.register(cell(10, OP_BUILD, twice.set(0, 0).map(|_| 1)));
    let _ = l.register(cell(11, OP_ROW, low.require_full(3).map(|_| 1)));
    let mut a = Matrix::new();
    let _ = a.set(0, 0);
    let mut b = Matrix::new();
    let _ = b.set(0, 0);
    let _ = l.register(cell(12, OP_MERGE, a.merge(&b).map(|v| v.closure() as u32)));
    let _ = l.register(cell(13, OP_TOOL, lookup(b"nosuch").map(|v| v as u32)));

    l
}

/// 四路注入：账本自洽、全表收口、落格与重复、合流与恢复。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];

    let mut l = build_ledger();
    let base = l.len() == SAMPLES && l.audit().is_ok();
    let rep = l.replay();
    let root = match l.freeze() {
        Ok(v) => v,
        Err(_) => 0,
    };
    let again = l.freeze() == Err(TM_EFROZEN);
    out[0] = base && root != 0 && rep == Ok(root) && again && l.rejects() == 1;

    let m = full();
    out[1] = m.closure() == CELLS
        && m.gaps() == 0
        && m.complete_rows() == TOOLS
        && m.row_full(0) == Ok(true)
        && m.row_full(TOOLS - 1) == Ok(true);

    let mut one = Matrix::new();
    let first = one.set(0, 0);
    let dup = one.set(0, 0);
    let second = one.set(0, 1);
    out[2] = first.is_ok() && dup == Err(TM_EDUP) && second.is_ok() && one.closure() == 2;

    let mixed = match low_half().merge(&high_half()) {
        Ok(v) => v,
        Err(_) => Matrix::new(),
    };
    let mut c = Matrix::new();
    let _ = c.set(2, 0);
    let conflict = low_half().merge(&c) == Err(TM_EMERGE);
    let gap = low_half().require_full(0) == Err(TM_EGAP);
    let named = lookup(b"ls") == Ok(14);
    let missing = lookup(b"nosuch") == Err(TM_ENAME);
    out[3] = mixed.closure() == CELLS && conflict && gap && named && missing;
    out
}

/// 十三判据。
pub fn run_unx11498_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11498.matrix20");
    let l = build_ledger();
    let m = full();

    s.add(
        "TM1-十四格齐备",
        l.len() == SAMPLES && !l.is_empty() && l.sample(0).map(|x| x.op) == Some(OP_BUILD),
        "static",
    );
    s.add_dyn(
        "TM2-八允六拒分布",
        l.ok_count() == 8 && l.err_count() == 6 && l.audit().is_ok(),
        &format!("ok={} err={} audit={:?}", l.ok_count(), l.err_count(), l.audit()),
    );
    s.add(
        "TM3-二十乘三总量",
        m.closure() == CELLS && CELLS == 60 && m.gaps() == 0 && l.sample(0).map(|x| x.value) == Some(60),
        "static",
    );
    s.add(
        "TM4-行满与缺口",
        m.complete_rows() == TOOLS && m.row_full(0) == Ok(true) && m.row_full(19) == Ok(true),
        "static",
    );
    s.add(
        "TM5-名册下标",
        lookup(b"echo") == Ok(0)
            && lookup(b"wc") == Ok(4)
            && lookup(b"tr") == Ok(5)
            && lookup(b"stat") == Ok(TOOLS - 1)
            && l.sample(5).map(|x| x.value) == Some(4),
        "static",
    );
    s.add(
        "TM6-车道计数",
        lane_count(0) == Ok(3)
            && lane_count(19) == Ok(3)
            && lane_count(TOOLS) == Err(TM_ETOOL)
            && l.sample(1).map(|x| x.value) == Some(3),
        "static",
    );
    s.add(
        "TM7-单格登记",
        {
            let mut one = Matrix::new();
            one.set(0, 0).is_ok()
                && one.get(0, 0) == Some(true)
                && one.get(0, 1) == Some(false)
                && one.closure() == 1
                && l.sample(6).map(|x| x.value) == Some(1)
        },
        "static",
    );
    s.add(
        "TM8-工具越界",
        Matrix::new().set(TOOLS, 0) == Err(TM_ETOOL)
            && Matrix::new().row_full(TOOLS) == Err(TM_ETOOL)
            && m.get(TOOLS, 0).is_none()
            && l.sample(8).map(|x| x.code) == Some(TM_ETOOL),
        "static",
    );
    s.add(
        "TM9-车道越界与重复",
        Matrix::new().set(0, LANES) == Err(TM_ELANE)
            && {
                let mut twice = Matrix::new();
                twice.set(0, 0).is_ok() && twice.set(0, 0) == Err(TM_EDUP)
            }
            && l.sample(9).map(|x| x.code) == Some(TM_ELANE)
            && l.sample(10).map(|x| x.code) == Some(TM_EDUP),
        "static",
    );
    s.add(
        "TM10-行缺口拦截",
        low_half().require_full(0) == Err(TM_EGAP)
            && low_half().row_full(0) == Ok(false)
            && low_half().complete_rows() == 0
            && l.sample(11).map(|x| x.code) == Some(TM_EGAP),
        "static",
    );
    s.add(
        "TM11-合流冲突",
        {
            let mut c = Matrix::new();
            let _ = c.set(4, 1);
            low_half().merge(&c) == Err(TM_EMERGE)
                && l.sample(12).map(|x| x.code) == Some(TM_EMERGE)
        },
        "static",
    );
    s.add(
        "TM12-半区合流与无名",
        {
            let mixed = low_half().merge(&high_half());
            mixed == Ok(m)
                && lookup(b"nosuch") == Err(TM_ENAME)
                && l.sample(7).map(|x| x.value) == Some(20)
                && l.sample(13).map(|x| x.code) == Some(TM_ENAME)
        },
        "static",
    );
    let mut f = build_ledger();
    let rep = f.replay();
    let root = match f.freeze() {
        Ok(v) => v,
        Err(_) => 0,
    };
    let again = f.freeze() == Err(TM_EFROZEN);
    let inj = inject_four();
    s.add_dyn(
        "TM13-冻结重放与四路",
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
    fn roster_is_fixed() {
        assert_eq!(NAMES.len(), TOOLS);
        assert_eq!(lookup(b"echo"), Ok(0));
        assert_eq!(lookup(b"stat"), Ok(19));
        assert_eq!(lookup(b"nosuch"), Err(TM_ENAME));
        assert_eq!(lane_count(3), Ok(3));
    }

    #[test]
    fn full_matrix_closes() {
        let m = full();
        assert_eq!(m.closure(), CELLS);
        assert_eq!(m.gaps(), 0);
        assert_eq!(m.complete_rows(), TOOLS);
        assert_eq!(m.row_full(0), Ok(true));
    }

    #[test]
    fn set_and_duplicate() {
        let mut m = Matrix::new();
        assert_eq!(m.set(0, 0), Ok(()));
        assert_eq!(m.get(0, 0), Some(true));
        assert_eq!(m.set(0, 0), Err(TM_EDUP));
        assert_eq!(m.set(TOOLS, 0), Err(TM_ETOOL));
        assert_eq!(m.set(0, LANES), Err(TM_ELANE));
    }

    #[test]
    fn row_gap_is_reported() {
        let low = low_half();
        assert_eq!(low.require_full(0), Err(TM_EGAP));
        assert_eq!(low.complete_rows(), 0);
        assert_eq!(low.gaps(), TOOLS);
    }

    #[test]
    fn merge_is_disjoint_only() {
        assert_eq!(low_half().merge(&high_half()), Ok(full()));
        let mut c = Matrix::new();
        let _ = c.set(2, 0);
        assert_eq!(low_half().merge(&c), Err(TM_EMERGE));
    }

    #[test]
    fn ledger_and_checks() {
        let l = build_ledger();
        assert_eq!(l.len(), SAMPLES);
        assert_eq!(l.ok_count(), 8);
        assert!(l.audit().is_ok());
        let s = run_unx11498_checks();
        assert_eq!(s.len(), 13);
        assert!(s.all_passed(), "{}", s.red_items_text());
        assert_eq!(inject_four(), [true; 4]);
    }
}
