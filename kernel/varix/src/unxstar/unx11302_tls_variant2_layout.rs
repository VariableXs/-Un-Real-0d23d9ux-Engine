//! UNX-F11302 TLS Variant II 布局（0xe8d0 段）
//!
//! x86_64 的通用动态 TLS（Variant II）把线程指针 TP 指向 TCB 的首字节，各模块
//! 的静态 TLS 块则紧贴在 TCB 之前、向低地址生长：块尾贴着下一个块的头部，
//! 第一个块的尾就是 TCB 的起点减一字节之后的对齐边界。因此偏移必须是负数，
//! 且每个块的起始偏移要按自己的对齐向下取整——这是“向下分配”而不是“向上
//! 分配”，用截断除法会算错负数对齐，必须使用欧几里得除法。
//!
//! 布局器逐模块计算：起点 = (游标 - memsz) 向下取整到对齐，块尺寸保持模板
//! 原长而不上取整——上取整会把大对齐块的起点再往下推一整个对齐，与基准机
//! 公式不符。游标前移到该起点后继续下一个模块。全部块必须落在预留窗口
//! （本模型取 4096 字节）之内，偏移必须能装进 i32（重定位字段宽度），否则
//! 布局失败。
//!
//! 对齐、尺寸与预算三类错误必须在分配前拒绝，且拒绝不能污染已算出的偏移：
//! 布局以不可变方式构造，任一模块非法则整体返回错误码，调用方拿不到半成品。
//! 账本十二格覆盖单块、双块、四块、1 字节块、大对齐块与三类错误。
//! 冻结前重放每格偏移并与票号对照。

use crate::alloc::format;
use crate::checks::CheckSet;
use alloc::vec;
use alloc::vec::Vec;

/// 段基址。
pub const SEG_BASE: u32 = 0xe8d0;

/// 编号非法。
pub const VL_EID: u32 = SEG_BASE + 0x01;
/// 顺序错误。
pub const VL_EORDER: u32 = SEG_BASE + 0x02;
/// 重复登记。
pub const VL_EDUP: u32 = SEG_BASE + 0x03;
/// 已冻结。
pub const VL_EFROZEN: u32 = SEG_BASE + 0x04;
/// 观测不存在的格。
pub const VL_ENOBS: u32 = SEG_BASE + 0x05;
/// 结论不符。
pub const VL_EMISMATCH: u32 = SEG_BASE + 0x06;
/// 账本失衡。
pub const VL_EBAL: u32 = SEG_BASE + 0x07;
/// 空布局。
pub const VL_EEMPTY: u32 = SEG_BASE + 0x08;
/// 对齐非 2 的幂。
pub const VL_EALIGN: u32 = SEG_BASE + 0x09;
/// 长度为零。
pub const VL_EMEMSZ: u32 = SEG_BASE + 0x0a;
/// 超出预留窗口。
pub const VL_EOVER: u32 = SEG_BASE + 0x0b;
/// 偏移超出 i32。
pub const VL_ERANGE: u32 = SEG_BASE + 0x0c;
/// 票号不一致。
pub const VL_ESEAL: u32 = SEG_BASE + 0x0d;

/// TCB 尺寸。
pub const TCB_SIZE: u16 = 64;
/// TLS 预留窗口。
pub const RESERVE: i64 = 4096;
/// 模块数上限。
pub const MAX_MODS: usize = 4;
/// 样本格数。
pub const SAMPLES: usize = 12;

/// FNV 偏移基。
pub const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;
/// FNV 质数。
pub const FNV_PRIME: u64 = 0x100_0000_01b3;

/// 一个 TLS 模块请求。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Mod {
    pub memsz: u16,
    pub align: u16,
}

impl Mod {
    /// 空模块。
    pub const fn none() -> Mod {
        Mod { memsz: 0, align: 0 }
    }

    /// 构造。
    pub const fn make(memsz: u16, align: u16) -> Mod {
        Mod { memsz, align }
    }
}

/// 判定 2 的幂。
pub fn is_pow2(v: u16) -> bool {
    v != 0 && (v & (v - 1)) == 0
}

/// 向下分配全部模块，返回从高到低的负偏移。
pub fn alloc_down(mods: &[Mod], tcb: u16) -> Result<Vec<i32>, u32> {
    if mods.is_empty() || mods.len() > MAX_MODS {
        return Err(VL_EEMPTY);
    }
    let mut cur: i64 = -(tcb as i64);
    let mut out = Vec::new();
    let mut i = 0;
    while i < mods.len() {
        let m = match mods.get(i) {
            Some(v) => *v,
            None => return Err(VL_EEMPTY),
        };
        if m.memsz == 0 {
            return Err(VL_EMEMSZ);
        }
        if !is_pow2(m.align) {
            return Err(VL_EALIGN);
        }
        let start = (cur - m.memsz as i64).div_euclid(m.align as i64) * m.align as i64;
        if start < -RESERVE {
            return Err(VL_EOVER);
        }
        if start < i32::MIN as i64 {
            return Err(VL_ERANGE);
        }
        out.push(start as i32);
        cur = start;
        i += 1;
    }
    Ok(out)
}

/// 一格布局样本。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Sample {
    pub mods: [Mod; MAX_MODS],
    pub nmods: u8,
    pub tcb: u16,
    pub expect_ok: bool,
    pub code: u32,
    pub offs: [i32; MAX_MODS],
}

impl Sample {
    /// 模块切片。
    pub fn mods(&self) -> &[Mod] {
        let n = self.nmods as usize;
        if n > MAX_MODS {
            return &[];
        }
        match self.mods.get(..n) {
            Some(v) => v,
            None => &[],
        }
    }
}

/// 布局账本。
pub struct Ledger {
    samples: Vec<Sample>,
    frozen: bool,
    root: u64,
    rejects: u32,
}

impl Ledger {
    /// 空账本。
    pub fn new() -> Ledger {
        Ledger { samples: Vec::new(), frozen: false, root: 0, rejects: 0 }
    }

    /// 登记一格，内部重算并核对。
    pub fn register(
        &mut self,
        slot: u8,
        mods: &[Mod],
        tcb: u16,
        expect_ok: bool,
        code: u32,
        offs: &[i32],
    ) -> Result<(), u32> {
        if self.frozen {
            self.rejects += 1;
            return Err(VL_EFROZEN);
        }
        if (slot as usize) >= SAMPLES {
            self.rejects += 1;
            return Err(VL_EID);
        }
        if (slot as usize) < self.samples.len() {
            self.rejects += 1;
            return Err(VL_EDUP);
        }
        if (slot as usize) != self.samples.len() {
            self.rejects += 1;
            return Err(VL_EORDER);
        }
        if mods.is_empty() || mods.len() > MAX_MODS {
            self.rejects += 1;
            return Err(VL_EEMPTY);
        }
        let r = alloc_down(mods, tcb);
        match r {
            Ok(v) => {
                if !expect_ok || v.len() != offs.len() {
                    return Err(VL_EMISMATCH);
                }
                let mut k = 0;
                while k < v.len() {
                    if v.get(k) != offs.get(k) {
                        return Err(VL_EMISMATCH);
                    }
                    k += 1;
                }
            }
            Err(e) => {
                if expect_ok || e != code {
                    return Err(VL_EMISMATCH);
                }
            }
        }
        let mut s = Sample { mods: [Mod::none(); MAX_MODS], nmods: mods.len() as u8, tcb, expect_ok, code, offs: [0; MAX_MODS] };
        let mut i = 0;
        while i < mods.len() {
            s.mods[i] = mods[i];
            i += 1;
        }
        i = 0;
        while i < offs.len() && i < MAX_MODS {
            s.offs[i] = offs[i];
            i += 1;
        }
        self.samples.push(s);
        Ok(())
    }

    /// 格数。
    pub fn len(&self) -> usize {
        self.samples.len()
    }

    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.samples.is_empty()
    }

    /// 取样本。
    pub fn sample(&self, i: usize) -> Option<Sample> {
        match self.samples.get(i) {
            Some(v) => Some(*v),
            None => None,
        }
    }

    /// 成功格数。
    pub fn ok_count(&self) -> usize {
        let mut n = 0;
        let mut i = 0;
        while i < self.samples.len() {
            if let Some(s) = self.samples.get(i) {
                if s.expect_ok {
                    n += 1;
                }
            }
            i += 1;
        }
        n
    }

    /// 失败格数。
    pub fn err_count(&self) -> usize {
        self.samples.len() - self.ok_count()
    }

    /// 自洽审计。
    pub fn audit(&self) -> Result<(), u32> {
        if self.samples.is_empty() || self.samples.len() > SAMPLES {
            return Err(VL_EBAL);
        }
        let mut i = 0;
        while i < self.samples.len() {
            let s = match self.samples.get(i) {
                Some(v) => v,
                None => return Err(VL_ENOBS),
            };
            match alloc_down(s.mods(), s.tcb) {
                Ok(v) => {
                    if !s.expect_ok || v.len() != s.nmods as usize {
                        return Err(VL_EMISMATCH);
                    }
                    let mut k = 0;
                    while k < v.len() {
                        if v.get(k) != s.offs.get(k) {
                            return Err(VL_EMISMATCH);
                        }
                        k += 1;
                    }
                }
                Err(e) => {
                    if s.expect_ok || e != s.code {
                        return Err(VL_EMISMATCH);
                    }
                }
            }
            i += 1;
        }
        Ok(())
    }

    /// 票面签名。
    pub fn seal(&self) -> u64 {
        let mut h = FNV_OFF;
        let mut i = 0;
        while i < self.samples.len() {
            if let Some(s) = self.samples.get(i) {
                h = fold(h, s.nmods as u64);
                h = fold(h, s.tcb as u64);
                let mut k = 0;
                while k < s.nmods as usize {
                    if let Some(m) = s.mods.get(k) {
                        h = fold(h, m.memsz as u64);
                        h = fold(h, m.align as u64);
                    }
                    if let Some(o) = s.offs.get(k) {
                        h = fold(h, *o as i64 as u64);
                    }
                    k += 1;
                }
            }
            i += 1;
        }
        h
    }

    /// 冻结。
    pub fn freeze(&mut self) -> Result<u64, u32> {
        if self.frozen {
            self.rejects += 1;
            return Err(VL_EFROZEN);
        }
        self.root = self.seal();
        self.frozen = true;
        Ok(self.root)
    }

    /// 重放。
    pub fn replay(&self) -> Result<u64, u32> {
        let fresh = build_ledger();
        if fresh.seal() == self.seal() {
            Ok(fresh.seal())
        } else {
            Err(VL_ESEAL)
        }
    }

    /// 冻结根。
    pub fn root(&self) -> u64 {
        self.root
    }

    /// 拒绝计数。
    pub fn rejects(&self) -> u32 {
        self.rejects
    }

    /// 是否冻结。
    pub fn is_frozen(&self) -> bool {
        self.frozen
    }
}

fn fold(h: u64, v: u64) -> u64 {
    let mut h = h;
    let mut i = 0u32;
    while i < 8 {
        h ^= (v >> (i * 8)) & 0xff;
        h = h.wrapping_mul(FNV_PRIME);
        i += 1;
    }
    h
}

/// 构建十二格账本：八格成功、四格失败。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();
    let _ = l.register(0, &[Mod::make(16, 16)], TCB_SIZE, true, 0, &[-80]);
    let _ = l.register(1, &[Mod::make(32, 32)], TCB_SIZE, true, 0, &[-96]);
    let _ = l.register(2, &[Mod::make(8, 8), Mod::make(8, 8)], TCB_SIZE, true, 0, &[-72, -80]);
    let _ = l.register(3, &[Mod::make(16, 16), Mod::make(16, 16), Mod::make(16, 16)], TCB_SIZE, true, 0, &[-80, -96, -112]);
    let _ = l.register(4, &[Mod::make(1, 1)], TCB_SIZE, true, 0, &[-65]);
    let _ = l.register(5, &[Mod::make(64, 64)], TCB_SIZE, true, 0, &[-128]);
    let _ = l.register(6, &[Mod::make(256, 4096)], TCB_SIZE, true, 0, &[-4096]);
    let _ = l.register(7, &[Mod::make(8, 0)], TCB_SIZE, false, VL_EALIGN, &[]);
    let _ = l.register(8, &[Mod::make(8, 3)], TCB_SIZE, false, VL_EALIGN, &[]);
    let _ = l.register(9, &[Mod::make(0, 8)], TCB_SIZE, false, VL_EMEMSZ, &[]);
    let _ = l.register(10, &[Mod::make(8192, 4096)], TCB_SIZE, false, VL_EOVER, &[]);
    let _ = l.register(11, &[Mod::make(8, 8), Mod::make(8, 8), Mod::make(8, 8), Mod::make(8, 8)], TCB_SIZE, true, 0, &[-72, -80, -88, -96]);
    l
}

/// 四路注入：偏移公式、向下贴合、三类拒绝、预算边界。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];

    let l = build_ledger();
    let base = l.len() == SAMPLES && l.ok_count() == 8 && l.err_count() == 4 && l.audit().is_ok();
    let rep = l.replay();
    let root = l.seal();
    out[0] = base && rep == Ok(root) && root != 0;

    out[1] = alloc_down(&[Mod::make(24, 8)], TCB_SIZE) == Ok(vec![-88])
        && alloc_down(&[Mod::make(24, 16)], TCB_SIZE) == Ok(vec![-96]);

    out[2] = alloc_down(&[Mod::make(8, 3)], TCB_SIZE) == Err(VL_EALIGN)
        && alloc_down(&[Mod::make(0, 8)], TCB_SIZE) == Err(VL_EMEMSZ)
        && alloc_down(&[Mod::make(8192, 4096)], TCB_SIZE) == Err(VL_EOVER);

    let edge = alloc_down(&[Mod::make(4096 - 64, 1)], TCB_SIZE);
    let over = alloc_down(&[Mod::make(4096 - 63, 1)], TCB_SIZE);
    out[3] = edge == Ok(vec![-(RESERVE as i32)]) && over == Err(VL_EOVER);

    out
}

/// 十三项判据。
pub fn run_unx11302_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11302.variant2");
    let l = build_ledger();

    s.add(
        "VL1-十二格齐备",
        l.len() == SAMPLES && !l.is_empty() && l.sample(0).map(|x| x.tcb) == Some(TCB_SIZE),
        "static",
    );

    s.add_dyn(
        "VL2-八应四拒分布正确",
        l.ok_count() == 8 && l.err_count() == 4 && l.audit().is_ok(),
        &format!("ok={} err={}", l.ok_count(), l.err_count()),
    );

    s.add(
        "VL3-单块偏移公式",
        alloc_down(&[Mod::make(16, 16)], TCB_SIZE) == Ok(vec![-80])
            && alloc_down(&[Mod::make(32, 32)], TCB_SIZE) == Ok(vec![-96]),
        "static",
    );

    s.add(
        "VL4-多块向下贴合",
        alloc_down(&[Mod::make(16, 16), Mod::make(16, 16), Mod::make(16, 16)], TCB_SIZE)
            == Ok(vec![-80, -96, -112]),
        "static",
    );

    s.add(
        "VL5-负偏移按对齐向下取整",
        alloc_down(&[Mod::make(24, 8)], TCB_SIZE) == Ok(vec![-88])
            && alloc_down(&[Mod::make(24, 16)], TCB_SIZE) == Ok(vec![-96])
            && alloc_down(&[Mod::make(24, 32)], TCB_SIZE) == Ok(vec![-96]),
        "static",
    );

    s.add(
        "VL6-一字节块贴 TCB",
        alloc_down(&[Mod::make(1, 1)], TCB_SIZE) == Ok(vec![-65]),
        "static",
    );

    s.add(
        "VL7-大对齐块落窗口顶",
        alloc_down(&[Mod::make(256, 4096)], TCB_SIZE) == Ok(vec![-4096]),
        "static",
    );

    s.add(
        "VL8-零长与零对齐拒绝",
        alloc_down(&[Mod::make(0, 8)], TCB_SIZE) == Err(VL_EMEMSZ)
            && alloc_down(&[Mod::make(8, 0)], TCB_SIZE) == Err(VL_EALIGN),
        "static",
    );

    s.add(
        "VL9-非幂对齐拒绝",
        alloc_down(&[Mod::make(8, 3)], TCB_SIZE) == Err(VL_EALIGN)
            && alloc_down(&[Mod::make(8, 24)], TCB_SIZE) == Err(VL_EALIGN),
        "static",
    );

    s.add(
        "VL10-预算窗口拦截",
        alloc_down(&[Mod::make(8192, 4096)], TCB_SIZE) == Err(VL_EOVER)
            && alloc_down(&[Mod::make(RESERVE as u16 - 64, 1)], TCB_SIZE) == Ok(vec![-(RESERVE as i32)])
            && alloc_down(&[Mod::make(RESERVE as u16 - 63, 1)], TCB_SIZE) == Err(VL_EOVER),
        "static",
    );

    s.add(
        "VL11-四块序列无重叠",
        {
            let offs = alloc_down(&[Mod::make(8, 8), Mod::make(8, 8), Mod::make(8, 8), Mod::make(8, 8)], TCB_SIZE);
            offs == Ok(vec![-72, -80, -88, -96])
        },
        "static",
    );

    let mut f = build_ledger();
    let rep = f.replay();
    let root = match f.freeze() {
        Ok(v) => v,
        Err(_) => 0,
    };
    let again = f.freeze() == Err(VL_EFROZEN);
    s.add_dyn(
        "VL12-冻结重放幂等",
        root != 0 && rep == Ok(root) && again && f.root() == root && f.rejects() == 1,
        &format!("root={:016x}", root),
    );

    let inj = inject_four();
    s.add_dyn("VL13-四路注入全绿", inj == [true; 4], &format!("inject={:?}", inj));

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
    fn single_block_formula() {
        assert_eq!(alloc_down(&[Mod::make(16, 16)], 64), Ok(vec![-80]));
        assert_eq!(alloc_down(&[Mod::make(1, 1)], 64), Ok(vec![-65]));
    }

    #[test]
    fn floor_alignment_for_negatives() {
        assert_eq!(alloc_down(&[Mod::make(24, 8)], 64), Ok(vec![-88]));
        assert_eq!(alloc_down(&[Mod::make(24, 16)], 64), Ok(vec![-96]));
        assert_eq!(alloc_down(&[Mod::make(40, 32)], 64), Ok(vec![-128]));
    }

    #[test]
    fn four_blocks_descend() {
        let offs = alloc_down(&[Mod::make(8, 8), Mod::make(8, 8), Mod::make(8, 8), Mod::make(8, 8)], 64);
        assert_eq!(offs, Ok(vec![-72, -80, -88, -96]));
    }

    #[test]
    fn invalid_inputs() {
        assert_eq!(alloc_down(&[Mod::make(0, 8)], 64), Err(VL_EMEMSZ));
        assert_eq!(alloc_down(&[Mod::make(8, 0)], 64), Err(VL_EALIGN));
        assert_eq!(alloc_down(&[Mod::make(8, 3)], 64), Err(VL_EALIGN));
        assert_eq!(alloc_down(&[], 64), Err(VL_EEMPTY));
    }

    #[test]
    fn reserve_edges() {
        assert_eq!(alloc_down(&[Mod::make(4096 - 64, 1)], 64), Ok(vec![-4096]));
        assert_eq!(alloc_down(&[Mod::make(4096 - 63, 1)], 64), Err(VL_EOVER));
    }

    #[test]
    fn ledger_distribution() {
        let l = build_ledger();
        assert_eq!(l.len(), SAMPLES);
        assert_eq!(l.ok_count(), 8);
        assert_eq!(l.err_count(), 4);
        assert!(l.audit().is_ok());
        assert!(summary(&l).starts_with("rows=12"));
    }

    #[test]
    fn refusal_matrix() {
        let mut l = Ledger::new();
        assert_eq!(l.register(0, &[Mod::make(16, 16)], 64, true, 0, &[-80]), Ok(()));
        assert_eq!(l.register(0, &[Mod::make(16, 16)], 64, true, 0, &[-80]), Err(VL_EDUP));
        assert_eq!(l.register(3, &[Mod::make(16, 16)], 64, true, 0, &[-80]), Err(VL_EORDER));
        assert_eq!(l.register(12, &[Mod::make(16, 16)], 64, true, 0, &[-80]), Err(VL_EID));
        assert_eq!(l.register(1, &[Mod::make(16, 16)], 64, true, 0, &[-79]), Err(VL_EMISMATCH));
        let _ = l.freeze();
        assert_eq!(l.register(1, &[Mod::make(16, 16)], 64, true, 0, &[-80]), Err(VL_EFROZEN));
    }

    #[test]
    fn checks_green() {
        let s = run_unx11302_checks();
        assert_eq!(s.len(), 13);
        assert_eq!(s.failed_count(), 0);
        assert_eq!(inject_four(), [true; 4]);
    }
}
