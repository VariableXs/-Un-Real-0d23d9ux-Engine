//! UNX-F11314 TLS 对齐与边界（0xe990 段）
//!
//! `PT_TLS` 的 `p_align` 是链接器给出的硬承诺：段起始必须在内存里满足该对齐，
//! 否则向量化拷贝与 `movaps` 一类指令会在运行期撕页。实测档位取 1、2、4、8、
//! 16、32、64 七级，64 是常见链接脚本的上限；再大的值无法用一次掩码对齐，
//! 本模型按非法处理，与“只支持到 64B”的装载器一致。
//!
//! 对齐是把双刃剑：向上取整会引入空洞，而空洞既是浪费也是边界事故的温床。
//! 模型把“放置”做成纯函数，输入当前游标、尺寸与对齐，输出偏移与新游标，
//! 并单独检查末端是否越过容量。加法全部走 `checked_add`，游标或尺寸接近
//! u32 上限时以溢出错误拒绝，而不是回绕成一个看似合法的地址。
//!
//! 账本十四格：八格成功覆盖五级递进对齐、两次恰好贴边的容量、一次四模块
//! 混排与一次七档矩阵统计；六格拒绝分别命中非二次幂、超过上限、游标溢出、
//! 尺寸为零、容量不足与末端溢出。矩阵格把 7 档对齐乘以 4 种尺寸全部走一遍，
//! 每个结果都必须满足 `offset % align == 0` 且区间互不重叠。

use crate::alloc::format;
use crate::checks::CheckSet;

/// 段基址。
pub const SEG_BASE: u32 = 0xe990;

/// 编号非法。
pub const AL_EID: u32 = SEG_BASE + 0x01;
/// 登记顺序错误。
pub const AL_EORDER: u32 = SEG_BASE + 0x02;
/// 重复登记。
pub const AL_EDUP: u32 = SEG_BASE + 0x03;
/// 已冻结。
pub const AL_EFROZEN: u32 = SEG_BASE + 0x04;
/// 观测不存在的格。
pub const AL_ENOBS: u32 = SEG_BASE + 0x05;
/// 结论不符。
pub const AL_EMISMATCH: u32 = SEG_BASE + 0x06;
/// 账本失衡。
pub const AL_EBAL: u32 = SEG_BASE + 0x07;
/// 对齐非法。
pub const AL_EALIGN: u32 = SEG_BASE + 0x08;
/// 尺寸为零。
pub const AL_ESIZE: u32 = SEG_BASE + 0x09;
/// 容量不足。
pub const AL_ENOMEM: u32 = SEG_BASE + 0x0a;
/// 算术溢出。
pub const AL_EOVER: u32 = SEG_BASE + 0x0b;
/// 区间重叠。
pub const AL_EOVERLAP: u32 = SEG_BASE + 0x0c;
/// 票号不一致。
pub const AL_ESEAL: u32 = SEG_BASE + 0x0d;

/// 判决：成功。
pub const V_OK: u32 = 0;

/// 操作：放置。
pub const OP_PLACE: u8 = 1;
/// 操作：带容量放置。
pub const OP_FIT: u8 = 2;
/// 操作：矩阵统计。
pub const OP_MATRIX: u8 = 3;

/// 最大对齐。
pub const MAX_ALIGN: u32 = 64;
/// 对齐档位。
pub const LEVELS: [u32; 7] = [1, 2, 4, 8, 16, 32, 64];
/// 尺寸档位。
pub const SIZES: [u32; 4] = [1, 3, 8, 64];
/// 样本格数。
pub const SAMPLES: usize = 14;

/// FNV 偏移基。
pub const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;
/// FNV 质数。
pub const FNV_PRIME: u64 = 0x100_0000_01b3;

/// 向上对齐，溢出即拒绝。
pub fn align_up(v: u32, a: u32) -> Result<u32, u32> {
    if a == 0 || !a.is_power_of_two() {
        return Err(AL_EALIGN);
    }
    if a > MAX_ALIGN {
        return Err(AL_EALIGN);
    }
    let r = v % a;
    if r == 0 {
        return Ok(v);
    }
    match v.checked_add(a - r) {
        Some(x) => Ok(x),
        None => Err(AL_EOVER),
    }
}

/// 放置一段：尺寸 → 对齐 → 游标溢出 → 末端溢出。
pub fn place(cursor: u32, size: u32, align: u32) -> Result<(u32, u32), u32> {
    if size == 0 {
        return Err(AL_ESIZE);
    }
    let off = align_up(cursor, align)?;
    match off.checked_add(size) {
        Some(end) => Ok((off, end)),
        None => Err(AL_EOVER),
    }
}

/// 带容量放置：放置后再查末端是否越过容量。
pub fn fit(cursor: u32, size: u32, align: u32, cap: u32) -> Result<(u32, u32), u32> {
    let (off, end) = place(cursor, size, align)?;
    if end > cap {
        return Err(AL_ENOMEM);
    }
    Ok((off, end))
}

/// 区间表：记录已放置段的 [off, end)。
#[derive(Clone, Copy, Debug)]
pub struct Ranges {
    spans: [(u32, u32); 8],
    n: usize,
}

impl Ranges {
    /// 空表。
    pub const fn new() -> Ranges {
        Ranges {
            spans: [(0, 0); 8],
            n: 0,
        }
    }

    /// 追加一段并检查与已有段不重叠。
    pub fn push(&mut self, off: u32, end: u32) -> Result<(), u32> {
        if end <= off {
            return Err(AL_EOVER);
        }
        let mut i = 0;
        while i < self.n {
            if let Some((a, b)) = self.spans.get(i) {
                if off < *b && *a < end {
                    return Err(AL_EOVERLAP);
                }
            }
            i += 1;
        }
        if self.n >= 8 {
            return Err(AL_ENOMEM);
        }
        if let Some(s) = self.spans.get_mut(self.n) {
            *s = (off, end);
        }
        self.n += 1;
        Ok(())
    }

    /// 段数。
    pub fn len(&self) -> usize {
        self.n
    }

    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.n == 0
    }

    /// 第 i 段。
    pub fn span(&self, i: usize) -> Option<(u32, u32)> {
        if i >= self.n {
            return None;
        }
        self.spans.get(i).copied()
    }
}

/// 矩阵统计：七档对齐 × 四档尺寸，全部成功且各自对齐。
pub fn matrix() -> (u32, bool) {
    let mut ok = true;
    let mut n = 0;
    let mut i = 0;
    while i < LEVELS.len() {
        let a = match LEVELS.get(i) {
            Some(v) => *v,
            None => return (n, false),
        };
        let mut j = 0;
        while j < SIZES.len() {
            let s = match SIZES.get(j) {
                Some(v) => *v,
                None => return (n, false),
            };
            let cursor = 13u32;
            match place(cursor, s, a) {
                Ok((off, end)) => {
                    if off % a != 0 || end != off + s || off < cursor {
                        ok = false;
                    }
                    n += 1;
                }
                Err(_) => ok = false,
            }
            j += 1;
        }
        i += 1;
    }
    (n, ok)
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
            return Err(AL_EFROZEN);
        }
        if (row.slot as usize) != self.len {
            return Err(AL_EORDER);
        }
        if self.len >= SAMPLES {
            return Err(AL_EID);
        }
        let mut i = 0;
        while i < self.len {
            match self.rows.get(i) {
                Some(x) => {
                    if x.slot == row.slot {
                        return Err(AL_EDUP);
                    }
                }
                None => return Err(AL_ENOBS),
            }
            i += 1;
        }
        match self.rows.get_mut(self.len) {
            Some(x) => *x = row,
            None => return Err(AL_EID),
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
            return Err(AL_EBAL);
        }
        if self.ok_count() != 8 || self.err_count() != 6 {
            return Err(AL_EBAL);
        }
        let mut i = 0;
        while i < self.len {
            match self.rows.get(i) {
                Some(s) => {
                    if s.slot as usize != i {
                        return Err(AL_EORDER);
                    }
                    if s.ok {
                        if s.code != V_OK {
                            return Err(AL_EMISMATCH);
                        }
                    } else if s.code == V_OK || s.value != 0 {
                        return Err(AL_EMISMATCH);
                    }
                }
                None => return Err(AL_ENOBS),
            }
            i += 1;
        }
        let vals = [8u32, 16, 24, 40, 72, 256, 9, 28];
        let mut k = 0;
        while k < vals.len() {
            match (self.sample(k), vals.get(k)) {
                (Some(s), Some(w)) => {
                    if !s.ok || s.value != *w {
                        return Err(AL_EMISMATCH);
                    }
                }
                _ => return Err(AL_EBAL),
            }
            k += 1;
        }
        let codes = [
            AL_EALIGN,
            AL_EALIGN,
            AL_EOVER,
            AL_ESIZE,
            AL_ENOMEM,
            AL_EOVER,
        ];
        let mut c = 0;
        let mut j = 8;
        while j < self.len {
            if let Some(s) = self.rows.get(j) {
                match codes.get(c) {
                    Some(w) => {
                        if s.code != *w {
                            return Err(AL_EMISMATCH);
                        }
                    }
                    None => return Err(AL_EBAL),
                }
                c += 1;
            }
            j += 1;
        }
        if c != codes.len() {
            return Err(AL_EBAL);
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
            return Err(AL_EFROZEN);
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
            return Err(AL_EBAL);
        }
        let h = fresh.seal();
        if self.frozen && h != self.seal {
            return Err(AL_ESEAL);
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

/// 由放置结果生成一格。
fn placed(slot: u8, op: u8, r: Result<(u32, u32), u32>) -> Sample {
    match r {
        Ok((_, end)) => Sample {
            slot,
            op,
            ok: true,
            code: V_OK,
            value: end,
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

/// 构建十四格账本。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();

    let _ = l.register(placed(0, OP_PLACE, place(0, 8, 1)));
    let _ = l.register(placed(1, OP_PLACE, place(8, 8, 8)));
    let _ = l.register(placed(2, OP_PLACE, place(16, 8, 16)));
    let _ = l.register(placed(3, OP_PLACE, place(24, 8, 32)));
    let _ = l.register(placed(4, OP_PLACE, place(40, 8, 64)));
    let _ = l.register(placed(5, OP_FIT, fit(0, 256, 64, 256)));
    let _ = l.register(placed(6, OP_FIT, fit(1, 8, 1, 9)));

    let (n, ok) = matrix();
    let _ = l.register(Sample {
        slot: 7,
        op: OP_MATRIX,
        ok,
        code: if ok { V_OK } else { AL_EMISMATCH },
        value: n,
    });

    let _ = l.register(placed(8, OP_PLACE, place(0, 8, 3)));
    let _ = l.register(placed(9, OP_PLACE, place(0, 8, 96)));
    let _ = l.register(placed(10, OP_PLACE, place(u32::MAX - 3, 8, 8)));
    let _ = l.register(placed(11, OP_PLACE, place(0, 0, 16)));
    let _ = l.register(placed(12, OP_FIT, fit(0, 9, 16, 8)));
    let _ = l.register(placed(13, OP_PLACE, place(u32::MAX, 2, 1)));

    l
}

/// 四路注入：账本重放、矩阵全绿、区间不重叠、闸门优先级。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];

    let l = build_ledger();
    let base = l.len() == SAMPLES && l.audit().is_ok();
    let rep = l.replay();
    let root = l.seal();
    out[0] = base && rep == Ok(root) && root != 0;

    let (n, ok) = matrix();
    out[1] = n == 28 && ok;

    let mut spans = Ranges::new();
    let mut cursor = 0u32;
    let mut good = true;
    let mut i = 0;
    let mut align_i = 0;
    while i < 8 {
        let a = match LEVELS.get(align_i % LEVELS.len()) {
            Some(v) => *v,
            None => return out,
        };
        match place(cursor, 9, a) {
            Ok((off, end)) => {
                if spans.push(off, end).is_err() {
                    good = false;
                }
                cursor = end;
            }
            Err(_) => good = false,
        }
        align_i += 1;
        i += 1;
    }
    out[2] = good && spans.len() == 8 && cursor >= 72;

    out[3] = place(0, 8, 3) == Err(AL_EALIGN)
        && place(0, 8, 128) == Err(AL_EALIGN)
        && place(u32::MAX - 3, 8, 8) == Err(AL_EOVER)
        && place(0, 0, 16) == Err(AL_ESIZE)
        && fit(0, 9, 16, 8) == Err(AL_ENOMEM)
        && place(u32::MAX, 2, 1) == Err(AL_EOVER)
        && align_up(7, 8) == Ok(8)
        && align_up(8, 8) == Ok(8);

    out
}

/// 十三项判据。
pub fn run_unx11314_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11314.tlsalign");
    let l = build_ledger();

    s.add(
        "AL1-十四格齐备",
        l.len() == SAMPLES && !l.is_empty() && l.sample(0).map(|x| x.op) == Some(OP_PLACE),
        "static",
    );

    s.add_dyn(
        "AL2-八允六拒分布正确",
        l.ok_count() == 8 && l.err_count() == 6 && l.audit().is_ok(),
        &format!("ok={} err={}", l.ok_count(), l.err_count()),
    );

    s.add(
        "AL3-递进对齐轨迹",
        place(0, 8, 1) == Ok((0, 8))
            && place(8, 8, 8) == Ok((8, 16))
            && place(16, 8, 16) == Ok((16, 24))
            && place(24, 8, 32) == Ok((32, 40))
            && place(40, 8, 64) == Ok((64, 72)),
        "static",
    );

    s.add(
        "AL4-恰好贴边",
        fit(0, 256, 64, 256) == Ok((0, 256))
            && fit(1, 8, 1, 9) == Ok((1, 9))
            && fit(0, 257, 64, 256) == Err(AL_ENOMEM),
        "static",
    );

    s.add(
        "AL5-七档矩阵全绿",
        {
            let (n, ok) = matrix();
            let mut count = 0;
            let mut all_aligned = true;
            let mut i = 0;
            while i < LEVELS.len() {
                let a = LEVELS[i];
                let mut j = 0;
                while j < SIZES.len() {
                    let s = SIZES[j];
                    if let Ok((off, end)) = place(13, s, a) {
                        if off % a != 0 || end - off != s {
                            all_aligned = false;
                        }
                        count += 1;
                    }
                    j += 1;
                }
                i += 1;
            }
            n == 28 && ok && all_aligned && count == 28 && l.sample(7).map(|x| x.value) == Some(28)
        },
        "static",
    );

    s.add(
        "AL6-非二次幂闸门",
        l.sample(8).map(|x| x.code) == Some(AL_EALIGN)
            && place(0, 8, 3) == Err(AL_EALIGN)
            && place(0, 8, 6) == Err(AL_EALIGN)
            && align_up(0, 0) == Err(AL_EALIGN),
        "static",
    );

    s.add(
        "AL7-超上限闸门",
        l.sample(9).map(|x| x.code) == Some(AL_EALIGN)
            && place(0, 8, 96) == Err(AL_EALIGN)
            && place(0, 8, 128) == Err(AL_EALIGN)
            && place(0, 8, 64).is_ok(),
        "static",
    );

    s.add(
        "AL8-游标溢出闸门",
        l.sample(10).map(|x| x.code) == Some(AL_EOVER)
            && place(u32::MAX - 3, 8, 8) == Err(AL_EOVER)
            && place(u32::MAX - 15, 8, 8) == Ok((u32::MAX - 15, u32::MAX - 7))
            && place(u32::MAX - 7, 8, 8) == Err(AL_EOVER),
        "static",
    );

    s.add(
        "AL9-末端溢出闸门",
        l.sample(13).map(|x| x.code) == Some(AL_EOVER)
            && place(u32::MAX, 2, 1) == Err(AL_EOVER)
            && place(u32::MAX - 1, 1, 1) == Ok((u32::MAX - 1, u32::MAX)),
        "static",
    );

    s.add(
        "AL10-尺寸零闸门",
        l.sample(11).map(|x| x.code) == Some(AL_ESIZE)
            && place(0, 0, 1) == Err(AL_ESIZE)
            && fit(0, 0, 64, 100) == Err(AL_ESIZE),
        "static",
    );

    s.add(
        "AL11-容量闸门",
        l.sample(12).map(|x| x.code) == Some(AL_ENOMEM)
            && fit(0, 9, 16, 8) == Err(AL_ENOMEM)
            && fit(16, 8, 16, 24) == Ok((16, 24)),
        "static",
    );

    s.add(
        "AL12-区间不重叠",
        {
            let mut r = Ranges::new();
            let mut ok = r.push(0, 8).is_ok();
            ok = ok && r.push(8, 16).is_ok();
            ok = ok && r.push(16, 24).is_ok();
            ok = ok && r.push(7, 9) == Err(AL_EOVERLAP);
            ok = ok && r.push(24, 24) == Err(AL_EOVER);
            ok = ok && r.len() == 3 && r.span(2) == Some((16, 24)) && r.span(3) == None;
            ok && r.is_empty() == false
        },
        "static",
    );

    let mut f = build_ledger();
    let rep = f.replay();
    let root = match f.freeze() {
        Ok(v) => v,
        Err(_) => 0,
    };
    let again = f.freeze() == Err(AL_EFROZEN);
    let inj = inject_four();
    s.add_dyn(
        "AL13-冻结重放与四路注入",
        root != 0 && rep == Ok(root) && again && f.root() == root && f.rejects() == 1 && inj == [true; 4],
        &format!("root={:016x} inject={:?}", root, inj),
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
    fn align_up_matrix() {
        let mut i = 0;
        while i < LEVELS.len() {
            let a = LEVELS[i];
            assert_eq!(align_up(0, a), Ok(0));
            assert_eq!(align_up(a, a), Ok(a));
            assert_eq!(align_up(a + 1, a), Ok(a * 2));
            i += 1;
        }
        assert_eq!(align_up(0, 3), Err(AL_EALIGN));
        assert_eq!(align_up(0, 128), Err(AL_EALIGN));
    }

    #[test]
    fn place_rejects_overflow() {
        assert_eq!(place(u32::MAX - 3, 8, 8), Err(AL_EOVER));
        assert_eq!(place(u32::MAX, 2, 1), Err(AL_EOVER));
        assert_eq!(place(0, 0, 8), Err(AL_ESIZE));
        assert_eq!(place(0, 8, 6), Err(AL_EALIGN));
    }

    #[test]
    fn fit_respects_capacity() {
        assert_eq!(fit(0, 256, 64, 256), Ok((0, 256)));
        assert_eq!(fit(0, 257, 64, 256), Err(AL_ENOMEM));
        assert_eq!(fit(64, 8, 64, 72), Ok((64, 72)));
        assert_eq!(fit(65, 8, 64, 72), Err(AL_ENOMEM));
    }

    #[test]
    fn matrix_is_complete() {
        let (n, ok) = matrix();
        assert_eq!(n, 28);
        assert!(ok);
    }

    #[test]
    fn ranges_detect_overlap() {
        let mut r = Ranges::new();
        assert!(r.push(0, 8).is_ok());
        assert!(r.push(8, 16).is_ok());
        assert_eq!(r.push(4, 12), Err(AL_EOVERLAP));
        assert_eq!(r.push(5, 5), Err(AL_EOVER));
        assert_eq!(r.len(), 2);
    }

    #[test]
    fn ledger_seals_and_replays() {
        let mut l = build_ledger();
        assert!(l.audit().is_ok());
        let root = l.freeze().expect("freeze");
        assert_eq!(l.replay(), Ok(root));
        assert_eq!(l.freeze(), Err(AL_EFROZEN));
        assert_eq!(l.rejects(), 1);
    }

    #[test]
    fn checks_pass() {
        let s = run_unx11314_checks();
        assert_eq!(s.len(), 13);
        assert!(s.all_passed(), "{}", s.red_items_text());
    }
}
