//! UNX-F11301 PT_TLS 段挂点与消费联签（0xe8c0 段）
//!
//! 动态链接器在装载带静态 TLS 的共享对象时，会把 PT_TLS 段的三参数搬进模块
//! 描述：模板地址 addr、模板长度 memsz、对齐 align。消费方（线程库与
//! __tls_get_addr）只信这三参数，因此三类坏值必须在挂点处被拦住：align 必须
//! 是 2 的幂，addr 必须按 align 对齐，memsz 必须大于零；addr+memsz 还必须
//! 不溢出地址空间。任何一条失败都表示链接器给的模板不可消费。
//!
//! 挂点还有一层“联签”：模板参数要与 C1 挂点登记的三元组一致——同一模块
//! 不允许出现两个互斥的挂点记录，也不允许挂点登记了而模板缺席。联签结论
//! 分为四档：可消费、参数非法、挂点冲突、挂点缺席。四档互斥且必须逐格复现。
//!
//! 账本登记十四格：七格可消费（覆盖 align 1/8/16/64/4096 与多档长度）；
//! 另外七格为两路缺席各一、对齐非幂、地址未落位、长度为零、区间溢出、
//! 冲突位各一。
//! 冻结前重放全部格子并核对票号，票号同时折叠三参数与联签档位。

use crate::alloc::format;
use crate::checks::CheckSet;
use alloc::vec::Vec;

/// 段基址。
pub const SEG_BASE: u32 = 0xe8c0;

/// 编号非法。
pub const TL_EID: u32 = SEG_BASE + 0x01;
/// 顺序错误。
pub const TL_EORDER: u32 = SEG_BASE + 0x02;
/// 重复登记。
pub const TL_EDUP: u32 = SEG_BASE + 0x03;
/// 已冻结。
pub const TL_EFROZEN: u32 = SEG_BASE + 0x04;
/// 观测不存在的格。
pub const TL_ENOBS: u32 = SEG_BASE + 0x05;
/// 结论不符。
pub const TL_EMISMATCH: u32 = SEG_BASE + 0x06;
/// 账本失衡。
pub const TL_EBAL: u32 = SEG_BASE + 0x07;
/// 空段。
pub const TL_EEMPTY: u32 = SEG_BASE + 0x08;
/// 对齐非 2 的幂。
pub const TL_EALIGN: u32 = SEG_BASE + 0x09;
/// 地址未按对齐落位。
pub const TL_EADDR: u32 = SEG_BASE + 0x0a;
/// 长度为零。
pub const TL_EMEMSZ: u32 = SEG_BASE + 0x0b;
/// 地址区间溢出。
pub const TL_EOVER: u32 = SEG_BASE + 0x0c;
/// 挂点冲突。
pub const TL_ECONF: u32 = SEG_BASE + 0x0d;
/// 票号不一致。
pub const TL_ESEAL: u32 = SEG_BASE + 0x0e;

/// 可消费。
pub const V_OK: u8 = 0;
/// 成功码。
pub const C_OK: u32 = 0;
/// 参数非法。
pub const V_BAD: u8 = 1;
/// 挂点冲突。
pub const V_CONF: u8 = 2;
/// 挂点缺席。
pub const V_MISS: u8 = 3;

/// 联签档位掩码：模板到场。
pub const F_TEMPLATE: u8 = 0x01;
/// 联签档位掩码：C1 挂点到场。
pub const F_HANG: u8 = 0x02;
/// 联签档位掩码：两路齐备。
pub const F_BOTH: u8 = 0x03;

/// 样本格数。
pub const SAMPLES: usize = 14;

/// FNV 偏移基。
pub const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;
/// FNV 质数。
pub const FNV_PRIME: u64 = 0x100_0000_01b3;

/// PT_TLS 三参数。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Seg {
    pub addr: u64,
    pub memsz: u64,
    pub align: u64,
}

impl Seg {
    /// 空段。
    pub const fn none() -> Seg {
        Seg { addr: 0, memsz: 0, align: 0 }
    }

    /// 构造。
    pub const fn make(addr: u64, memsz: u64, align: u64) -> Seg {
        Seg { addr, memsz, align }
    }
}

/// 联签档案。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Dossier {
    pub seg: Seg,
    pub flags: u8,
    pub verdict: u8,
    pub code: u32,
}

impl Dossier {
    /// 空档案。
    pub const fn none() -> Dossier {
        Dossier { seg: Seg::none(), flags: 0, verdict: V_BAD, code: TL_EEMPTY }
    }
}

/// 判定对齐是否 2 的幂。
pub fn is_pow2(v: u64) -> bool {
    v != 0 && (v & (v - 1)) == 0
}

/// 判定单段是否可消费。
pub fn judge(seg: Seg) -> Result<Seg, u32> {
    if seg.memsz == 0 {
        return Err(TL_EMEMSZ);
    }
    if !is_pow2(seg.align) {
        return Err(TL_EALIGN);
    }
    if seg.align > 1 && seg.addr % seg.align != 0 {
        return Err(TL_EADDR);
    }
    match seg.addr.checked_add(seg.memsz) {
        Some(_) => Ok(seg),
        None => Err(TL_EOVER),
    }
}

/// 联签：模板与挂点两路齐备且参数合法才可消费。
pub fn co_sign(seg: Seg, flags: u8) -> Dossier {
    if flags & F_TEMPLATE == 0 || flags & F_HANG == 0 {
        return Dossier { seg, flags, verdict: V_MISS, code: TL_EEMPTY };
    }
    if flags & !F_BOTH != 0 {
        return Dossier { seg, flags, verdict: V_CONF, code: TL_ECONF };
    }
    match judge(seg) {
        Ok(v) => Dossier { seg: v, flags, verdict: V_OK, code: C_OK },
        Err(e) => Dossier { seg, flags, verdict: V_BAD, code: e },
    }
}

/// 一格样本。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Sample {
    pub seg: Seg,
    pub flags: u8,
    pub expect_verdict: u8,
    pub expect_code: u32,
}

/// 联签账本。
pub struct Ledger {
    samples: Vec<Sample>,
    dossiers: Vec<Dossier>,
    frozen: bool,
    root: u64,
    rejects: u32,
}

impl Ledger {
    /// 空账本。
    pub fn new() -> Ledger {
        Ledger { samples: Vec::new(), dossiers: Vec::new(), frozen: false, root: 0, rejects: 0 }
    }

    /// 登记一格。
    pub fn register(
        &mut self,
        slot: u8,
        seg: Seg,
        flags: u8,
        verdict: u8,
        code: u32,
    ) -> Result<(), u32> {
        if self.frozen {
            self.rejects += 1;
            return Err(TL_EFROZEN);
        }
        if (slot as usize) >= SAMPLES {
            self.rejects += 1;
            return Err(TL_EID);
        }
        if (slot as usize) < self.samples.len() {
            self.rejects += 1;
            return Err(TL_EDUP);
        }
        if (slot as usize) != self.samples.len() {
            self.rejects += 1;
            return Err(TL_EORDER);
        }
        let d = co_sign(seg, flags);
        if d.verdict != verdict || d.code != code {
            return Err(TL_EMISMATCH);
        }
        self.samples.push(Sample { seg, flags, expect_verdict: verdict, expect_code: code });
        self.dossiers.push(d);
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

    /// 取档案。
    pub fn dossier(&self, i: usize) -> Option<Dossier> {
        match self.dossiers.get(i) {
            Some(v) => Some(*v),
            None => None,
        }
    }

    /// 按档位计数。
    pub fn count(&self, verdict: u8) -> usize {
        let mut n = 0;
        let mut i = 0;
        while i < self.dossiers.len() {
            if let Some(d) = self.dossiers.get(i) {
                if d.verdict == verdict {
                    n += 1;
                }
            }
            i += 1;
        }
        n
    }

    /// 账本自洽。
    pub fn audit(&self) -> Result<(), u32> {
        if self.samples.is_empty() || self.samples.len() != self.dossiers.len() {
            return Err(TL_EBAL);
        }
        let mut i = 0;
        while i < self.samples.len() {
            let s = match self.samples.get(i) {
                Some(v) => v,
                None => return Err(TL_ENOBS),
            };
            let d = match self.dossiers.get(i) {
                Some(v) => v,
                None => return Err(TL_ENOBS),
            };
            let fresh = co_sign(s.seg, s.flags);
            if fresh != *d || d.verdict != s.expect_verdict || d.code != s.expect_code {
                return Err(TL_EMISMATCH);
            }
            i += 1;
        }
        Ok(())
    }

    /// 票面签名。
    pub fn seal(&self) -> u64 {
        let mut h = FNV_OFF;
        let mut i = 0;
        while i < self.dossiers.len() {
            if let Some(d) = self.dossiers.get(i) {
                h = fold(h, d.seg.addr);
                h = fold(h, d.seg.memsz);
                h = fold(h, d.seg.align);
                h = fold(h, d.flags as u64);
                h = fold(h, d.verdict as u64);
                h = fold(h, d.code as u64);
            }
            i += 1;
        }
        h = fold(h, self.count(V_OK) as u64);
        h
    }

    /// 冻结。
    pub fn freeze(&mut self) -> Result<u64, u32> {
        if self.frozen {
            self.rejects += 1;
            return Err(TL_EFROZEN);
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
            Err(TL_ESEAL)
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

/// 构建十四格账本：九格可消费，五格失败。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();
    let both = F_BOTH;
    let _ = l.register(0, Seg::make(0x1000, 64, 8), both, V_OK, C_OK);
    let _ = l.register(1, Seg::make(0x2000, 128, 16), both, V_OK, C_OK);
    let _ = l.register(2, Seg::make(0x3000, 32, 1), both, V_OK, C_OK);
    let _ = l.register(3, Seg::make(0x4000, 256, 64), both, V_OK, C_OK);
    let _ = l.register(4, Seg::make(0x5000, 4096, 4096), both, V_OK, C_OK);
    let _ = l.register(5, Seg::make(0x6000, 8, 8), both, V_OK, C_OK);
    let _ = l.register(6, Seg::make(0x7000, 16, 16), F_TEMPLATE, V_MISS, TL_EEMPTY);
    let _ = l.register(7, Seg::make(0x8000, 16, 16), F_HANG, V_MISS, TL_EEMPTY);
    let _ = l.register(8, Seg::make(0x9000, 16, 0), both, V_BAD, TL_EALIGN);
    let _ = l.register(9, Seg::make(0x9ff8, 16, 16), both, V_BAD, TL_EADDR);
    let _ = l.register(10, Seg::make(0xa000, 0, 8), both, V_BAD, TL_EMEMSZ);
    let _ = l.register(11, Seg::make(u64::MAX - 3, 16, 1), both, V_BAD, TL_EOVER);
    let _ = l.register(12, Seg::make(0xb000, 16, 16), 0x07, V_CONF, TL_ECONF);
    let _ = l.register(13, Seg::make(0xb100, 24, 8), both, V_OK, C_OK);
    l
}

/// 四路注入：对齐幂、区间溢出、两路缺席、冲突位。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];

    let l = build_ledger();
    let base = l.len() == SAMPLES
        && l.count(V_OK) == 7
        && l.count(V_MISS) == 2
        && l.count(V_BAD) == 4
        && l.count(V_CONF) == 1
        && l.audit().is_ok();
    let rep = l.replay();
    let root = l.seal();
    out[0] = base && rep == Ok(root) && root != 0;

    out[1] = is_pow2(1) && is_pow2(2) && is_pow2(4096) && !is_pow2(0) && !is_pow2(3) && !is_pow2(24);

    let over = Seg::make(u64::MAX, 2, 1);
    let miss = co_sign(Seg::make(0x1000, 8, 8), 0);
    out[2] = judge(over) == Err(TL_EOVER) && miss.verdict == V_MISS && miss.code == TL_EEMPTY;

    let conf = co_sign(Seg::make(0x1000, 8, 8), 0x0b);
    let ok = co_sign(Seg::make(0x1000, 8, 8), F_BOTH);
    out[3] = conf.verdict == V_CONF && ok.verdict == V_OK && ok.seg == Seg::make(0x1000, 8, 8);

    out
}

/// 十三项判据。
pub fn run_unx11301_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11301.pttls");
    let l = build_ledger();

    s.add(
        "TL1-十四格齐备",
        l.len() == SAMPLES && !l.is_empty() && l.sample(0).map(|x| x.seg.align) == Some(8),
        "static",
    );

    s.add_dyn(
        "TL2-四档分布正确",
        l.count(V_OK) == 7 && l.count(V_MISS) == 2 && l.count(V_BAD) == 4 && l.count(V_CONF) == 1 && l.audit().is_ok(),
        &format!("ok={} miss={} bad={} conf={}", l.count(V_OK), l.count(V_MISS), l.count(V_BAD), l.count(V_CONF)),
    );

    s.add(
        "TL3-三参数逐格保真",
        l.dossier(1).map(|d| d.seg) == Some(Seg::make(0x2000, 128, 16))
            && l.dossier(4).map(|d| d.seg) == Some(Seg::make(0x5000, 4096, 4096))
            && l.dossier(13).map(|d| d.seg) == Some(Seg::make(0xb100, 24, 8)),
        "static",
    );

    s.add(
        "TL4-对齐须为 2 的幂",
        l.dossier(8).map(|d| d.code) == Some(TL_EALIGN)
            && judge(Seg::make(0x10, 8, 3)) == Err(TL_EALIGN)
            && judge(Seg::make(0x10, 8, 0)) == Err(TL_EALIGN),
        "static",
    );

    s.add(
        "TL5-地址须按对齐落位",
        l.dossier(9).map(|d| d.code) == Some(TL_EADDR)
            && judge(Seg::make(0x1008, 8, 16)) == Err(TL_EADDR)
            && judge(Seg::make(0x1000, 8, 16)).is_ok(),
        "static",
    );

    s.add(
        "TL6-长度为零拒绝",
        l.dossier(10).map(|d| d.code) == Some(TL_EMEMSZ)
            && judge(Seg::make(0x1000, 0, 8)) == Err(TL_EMEMSZ),
        "static",
    );

    s.add(
        "TL7-区间溢出拦截",
        l.dossier(11).map(|d| d.code) == Some(TL_EOVER)
            && judge(Seg::make(u64::MAX, 2, 1)) == Err(TL_EOVER)
            && judge(Seg::make(u64::MAX - 16, 16, 1)).is_ok(),
        "static",
    );

    s.add(
        "TL8-单路缺席判缺",
        l.dossier(6).map(|d| d.verdict) == Some(V_MISS)
            && l.dossier(7).map(|d| d.verdict) == Some(V_MISS)
            && co_sign(Seg::make(1, 8, 1), 0).verdict == V_MISS,
        "static",
    );

    s.add(
        "TL9-未知位判冲突",
        l.dossier(12).map(|d| d.verdict) == Some(V_CONF)
            && co_sign(Seg::make(1, 8, 1), 0x0b).verdict == V_CONF,
        "static",
    );

    s.add(
        "TL10-可消费档位放行",
        co_sign(Seg::make(0x40, 8, 8), F_BOTH).verdict == V_OK
            && co_sign(Seg::make(0x40, 8, 8), F_BOTH).code == C_OK,
        "static",
    );

    s.add(
        "TL11-对齐一允许任意地址",
        judge(Seg::make(0x1234_5679, 8, 1)).is_ok()
            && judge(Seg::make(0x1234_5679, 8, 2)) == Err(TL_EADDR),
        "static",
    );

    let mut f = build_ledger();
    let rep = f.replay();
    let root = match f.freeze() {
        Ok(v) => v,
        Err(_) => 0,
    };
    let again = f.freeze() == Err(TL_EFROZEN);
    s.add_dyn(
        "TL12-冻结重放幂等",
        root != 0 && rep == Ok(root) && again && f.root() == root && f.rejects() == 1,
        &format!("root={:016x}", root),
    );

    let inj = inject_four();
    s.add_dyn("TL13-四路注入全绿", inj == [true; 4], &format!("inject={:?}", inj));

    s
}

/// 摘要。
pub fn summary(l: &Ledger) -> alloc::string::String {
    format!(
        "rows={} ok={} miss={} bad={} conf={}",
        l.len(),
        l.count(V_OK),
        l.count(V_MISS),
        l.count(V_BAD),
        l.count(V_CONF)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn powers_of_two() {
        assert!(is_pow2(1) && is_pow2(64) && is_pow2(4096));
        assert!(!is_pow2(0) && !is_pow2(6) && !is_pow2(24));
    }

    #[test]
    fn alignment_and_bounds() {
        assert_eq!(judge(Seg::make(0x1000, 8, 16)), Ok(Seg::make(0x1000, 8, 16)));
        assert_eq!(judge(Seg::make(0x1008, 8, 16)), Err(TL_EADDR));
        assert_eq!(judge(Seg::make(u64::MAX, 2, 1)), Err(TL_EOVER));
    }

    #[test]
    fn zero_memsz_rejected() {
        assert_eq!(judge(Seg::make(0x1000, 0, 1)), Err(TL_EMEMSZ));
    }

    #[test]
    fn co_sign_states() {
        assert_eq!(co_sign(Seg::make(0x10, 8, 8), F_BOTH).verdict, V_OK);
        assert_eq!(co_sign(Seg::make(0x10, 8, 8), F_TEMPLATE).verdict, V_MISS);
        assert_eq!(co_sign(Seg::make(0x10, 8, 8), 0x0b).verdict, V_CONF);
        assert_eq!(co_sign(Seg::make(0x10, 8, 3), F_BOTH).verdict, V_BAD);
    }

    #[test]
    fn ledger_audit_and_counts() {
        let l = build_ledger();
        assert_eq!(l.len(), SAMPLES);
        assert_eq!(l.count(V_OK), 7);
        assert!(l.audit().is_ok());
        assert!(summary(&l).starts_with("rows=14"));
    }

    #[test]
    fn refusal_matrix() {
        let mut l = Ledger::new();
        assert_eq!(l.register(0, Seg::make(1, 8, 1), F_BOTH, V_OK, C_OK), Ok(()));
        assert_eq!(l.register(0, Seg::make(1, 8, 1), F_BOTH, V_OK, C_OK), Err(TL_EDUP));
        assert_eq!(l.register(3, Seg::make(1, 8, 1), F_BOTH, V_OK, C_OK), Err(TL_EORDER));
        assert_eq!(l.register(14, Seg::make(1, 8, 1), F_BOTH, V_OK, C_OK), Err(TL_EID));
        assert_eq!(l.register(1, Seg::make(1, 8, 1), F_BOTH, V_OK, TL_EALIGN), Err(TL_EMISMATCH));
    }

    #[test]
    fn checks_green() {
        let s = run_unx11301_checks();
        assert_eq!(s.len(), 13);
        assert_eq!(s.failed_count(), 0);
        assert_eq!(inject_four(), [true; 4]);
    }
}
