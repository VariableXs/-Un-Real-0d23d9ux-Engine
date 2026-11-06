//! UNX-F11303 initial-exec 模型消费（0xe8e0 段）
//!
//! initial-exec（IE）是启动期最省的 TLS 访问模型：变量地址 = TP + 负偏移，
//! 偏移在装载时写死进指令，运行期只需一条 FS 段基址访问。代价是模块必须在
//! 启动期就进入静态 TLS 窗口，运行期 dlopen 的模块只能从 surplus（预留余量）
//! 里挤出位置。因此 IE 的消费面同时受三道约束：模块已登记、变量落在模块块内、
//! 动态模块的累计占用不超 surplus。
//!
//! 模型把一次访问拆成五个判据：尺寸非零、区间不越界、变量对齐、地址落在
//! 32 位有符号重定位范围内、动态模块余量足够。五条按顺序判定，先命中者胜出，
//! 错误码因此具有优先级语义——例如尺寸为零的请求即使模块缺失也报尺寸错，
//! 因为它在更早的阶段就被拦下。这一点必须在账本轨迹里可复现。
//!
//! 余量按访问顺序累计：同一个动态模块块内的连续访问共享累计量，超出的那次
//! 访问失败但不回滚已累计的部分，这与装载器“逐项分配、失败即拒”的行为一致。
//! 账本十二格中六格成功、六格失败，失败分别覆盖越界、对齐、缺模块、越界、
//! 尺寸为零与余量耗尽。

use crate::alloc::format;
use crate::checks::CheckSet;
use alloc::vec::Vec;

/// 段基址。
pub const SEG_BASE: u32 = 0xe8e0;

/// 编号非法。
pub const IE_EID: u32 = SEG_BASE + 0x01;
/// 顺序错误。
pub const IE_EORDER: u32 = SEG_BASE + 0x02;
/// 重复登记。
pub const IE_EDUP: u32 = SEG_BASE + 0x03;
/// 已冻结。
pub const IE_EFROZEN: u32 = SEG_BASE + 0x04;
/// 观测不存在的格。
pub const IE_ENOBS: u32 = SEG_BASE + 0x05;
/// 结论不符。
pub const IE_EMISMATCH: u32 = SEG_BASE + 0x06;
/// 账本失衡。
pub const IE_EBAL: u32 = SEG_BASE + 0x07;
/// 尺寸为零。
pub const IE_ESIZE: u32 = SEG_BASE + 0x08;
/// 区间越界。
pub const IE_EBOUNDS: u32 = SEG_BASE + 0x09;
/// 对齐不符。
pub const IE_EALIGN: u32 = SEG_BASE + 0x0a;
/// 模块缺失。
pub const IE_ENOMOD: u32 = SEG_BASE + 0x0b;
/// 余量耗尽。
pub const IE_ESURPLUS: u32 = SEG_BASE + 0x0c;
/// 偏移超 i32。
pub const IE_ERANGE: u32 = SEG_BASE + 0x0d;
/// 票号不一致。
pub const IE_ESEAL: u32 = SEG_BASE + 0x0e;

/// 动态模块余量。
pub const SURPLUS: u32 = 44;
/// 模块数。
pub const MODS: usize = 5;
/// 样本格数。
pub const SAMPLES: usize = 12;

/// FNV 偏移基。
pub const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;
/// FNV 质数。
pub const FNV_PRIME: u64 = 0x100_0000_01b3;

/// 一个已装载模块。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Module {
    pub memsz: u32,
    pub off: i32,
    pub startup: bool,
}

impl Module {
    /// 启动期模块。
    pub const fn startup(memsz: u32, off: i32) -> Module {
        Module { memsz, off, startup: true }
    }

    /// 动态模块。
    pub const fn dynamic(memsz: u32, off: i32) -> Module {
        Module { memsz, off, startup: false }
    }
}

/// 一次 IE 访问请求。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Req {
    pub module: u8,
    pub var_off: u32,
    pub size: u32,
    pub align: u32,
}

impl Req {
    /// 构造。
    pub const fn make(module: u8, var_off: u32, size: u32, align: u32) -> Req {
        Req { module, var_off, size, align }
    }
}

/// 解析结论。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Resolved {
    pub addr: i32,
    pub used: u32,
}

/// 判定 2 的幂。
pub fn is_pow2(v: u32) -> bool {
    v != 0 && (v & (v - 1)) == 0
}

/// 解析一次 IE 访问；`used` 为动态模块已占用余量。
pub fn resolve(req: Req, mods: &[Module], used: u32) -> Result<Resolved, u32> {
    if req.size == 0 {
        return Err(IE_ESIZE);
    }
    if !is_pow2(req.align) {
        return Err(IE_EALIGN);
    }
    let m = match mods.get(req.module as usize) {
        Some(v) => *v,
        None => return Err(IE_ENOMOD),
    };
    let end = req.var_off as u64 + req.size as u64;
    if end > m.memsz as u64 {
        return Err(IE_EBOUNDS);
    }
    let addr = m.off as i64 + req.var_off as i64;
    if addr > i32::MAX as i64 || addr < i32::MIN as i64 {
        return Err(IE_ERANGE);
    }
    if (addr as i64).rem_euclid(req.align as i64) != 0 {
        return Err(IE_EALIGN);
    }
    let mut used2 = used;
    if !m.startup {
        let total = used as u64 + req.size as u64;
        if total > SURPLUS as u64 {
            return Err(IE_ESURPLUS);
        }
        used2 = total as u32;
    }
    Ok(Resolved { addr: addr as i32, used: used2 })
}

/// 标准模块表。
pub fn build_mods() -> Vec<Module> {
    let mut v = Vec::new();
    v.push(Module::startup(64, -128));
    v.push(Module::startup(32, -160));
    v.push(Module::startup(16, -176));
    v.push(Module::dynamic(32, -224));
    v.push(Module::dynamic(8, -232));
    v
}

/// 一格样本。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Sample {
    pub req: Req,
    pub expect_ok: bool,
    pub code: u32,
    pub expect_addr: i32,
}

/// 账本。
pub struct Ledger {
    samples: Vec<Sample>,
    addrs: Vec<i32>,
    used: Vec<u32>,
    frozen: bool,
    root: u64,
    rejects: u32,
}

impl Ledger {
    /// 空账本。
    pub fn new() -> Ledger {
        Ledger { samples: Vec::new(), addrs: Vec::new(), used: Vec::new(), frozen: false, root: 0, rejects: 0 }
    }

    /// 登记一格，内部按序重算。
    pub fn register(&mut self, slot: u8, req: Req, ok: bool, code: u32, addr: i32) -> Result<(), u32> {
        if self.frozen {
            self.rejects += 1;
            return Err(IE_EFROZEN);
        }
        if (slot as usize) >= SAMPLES {
            self.rejects += 1;
            return Err(IE_EID);
        }
        if (slot as usize) < self.samples.len() {
            self.rejects += 1;
            return Err(IE_EDUP);
        }
        if (slot as usize) != self.samples.len() {
            self.rejects += 1;
            return Err(IE_EORDER);
        }
        let used = match self.used.last() {
            Some(v) => *v,
            None => 0,
        };
        let mods = build_mods();
        match resolve(req, &mods, used) {
            Ok(r) => {
                if !ok || r.addr != addr {
                    return Err(IE_EMISMATCH);
                }
                self.addrs.push(r.addr);
                self.used.push(r.used);
            }
            Err(e) => {
                if ok || e != code {
                    return Err(IE_EMISMATCH);
                }
                self.addrs.push(0);
                self.used.push(used);
            }
        }
        self.samples.push(Sample { req, expect_ok: ok, code, expect_addr: addr });
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

    /// 取地址。
    pub fn addr(&self, i: usize) -> Option<i32> {
        match self.addrs.get(i) {
            Some(v) => Some(*v),
            None => None,
        }
    }

    /// 最终余量占用。
    pub fn used(&self) -> u32 {
        match self.used.last() {
            Some(v) => *v,
            None => 0,
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
        if self.samples.is_empty() || self.samples.len() != self.addrs.len() || self.samples.len() != self.used.len() {
            return Err(IE_EBAL);
        }
        let mods = build_mods();
        let mut used = 0u32;
        let mut i = 0;
        while i < self.samples.len() {
            let s = match self.samples.get(i) {
                Some(v) => v,
                None => return Err(IE_ENOBS),
            };
            match resolve(s.req, &mods, used) {
                Ok(r) => {
                    if !s.expect_ok || r.addr != s.expect_addr {
                        return Err(IE_EMISMATCH);
                    }
                    used = r.used;
                }
                Err(e) => {
                    if s.expect_ok || e != s.code {
                        return Err(IE_EMISMATCH);
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
                h = fold(h, s.req.module as u64);
                h = fold(h, s.req.var_off as u64);
                h = fold(h, s.req.size as u64);
                h = fold(h, s.req.align as u64);
                h = fold(h, s.expect_addr as i64 as u64);
                if let Some(u) = self.used.get(i) {
                    h = fold(h, *u as u64);
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
            return Err(IE_EFROZEN);
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
            Err(IE_ESEAL)
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

/// 构建十二格账本：六格成功、六格失败。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();
    let _ = l.register(0, Req::make(0, 0, 8, 8), true, 0, -128);
    let _ = l.register(1, Req::make(0, 32, 16, 16), true, 0, -96);
    let _ = l.register(2, Req::make(1, 8, 8, 8), true, 0, -152);
    let _ = l.register(3, Req::make(2, 0, 16, 16), true, 0, -176);
    let _ = l.register(4, Req::make(3, 0, 32, 32), true, 0, -224);
    let _ = l.register(5, Req::make(4, 0, 8, 8), true, 0, -232);
    let _ = l.register(6, Req::make(1, 24, 16, 8), false, IE_EBOUNDS, 0);
    let _ = l.register(7, Req::make(1, 4, 8, 8), false, IE_EALIGN, 0);
    let _ = l.register(8, Req::make(9, 0, 8, 8), false, IE_ENOMOD, 0);
    let _ = l.register(9, Req::make(3, 16, 24, 8), false, IE_EBOUNDS, 0);
    let _ = l.register(10, Req::make(0, 0, 0, 8), false, IE_ESIZE, 0);
    let _ = l.register(11, Req::make(4, 0, 8, 8), false, IE_ESURPLUS, 0);
    l
}

/// 四路注入：地址公式、边界、错误优先级、余量顺序。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];

    let l = build_ledger();
    let base = l.len() == SAMPLES && l.ok_count() == 6 && l.err_count() == 6 && l.audit().is_ok();
    let rep = l.replay();
    let root = l.seal();
    out[0] = base && rep == Ok(root) && root != 0;

    let mods = build_mods();
    out[1] = resolve(Req::make(0, 0, 8, 8), &mods, 0) == Ok(Resolved { addr: -128, used: 0 })
        && resolve(Req::make(3, 0, 8, 16), &mods, 0) == Ok(Resolved { addr: -224, used: 8 });

    out[2] = resolve(Req::make(0, 0, 0, 8), &mods, 0) == Err(IE_ESIZE)
        && resolve(Req::make(9, 0, 8, 8), &mods, 0) == Err(IE_ENOMOD)
        && resolve(Req::make(0, 0, 8, 6), &mods, 0) == Err(IE_EALIGN)
        && resolve(Req::make(1, 24, 16, 8), &mods, 0) == Err(IE_EBOUNDS);

    let alone = resolve(Req::make(4, 0, 8, 8), &mods, 0);
    let after = resolve(Req::make(4, 0, 8, 8), &mods, 40);
    let exhaust = resolve(Req::make(4, 0, 8, 8), &mods, 37);
    out[3] = alone == Ok(Resolved { addr: -232, used: 8 }) && after == Err(IE_ESURPLUS) && exhaust == Err(IE_ESURPLUS);

    out
}

/// 十三项判据。
pub fn run_unx11303_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11303.initexec");
    let l = build_ledger();

    s.add(
        "IE1-十二格齐备",
        l.len() == SAMPLES && !l.is_empty() && l.sample(0).map(|x| x.req.module) == Some(0),
        "static",
    );

    s.add_dyn(
        "IE2-六应六拒分布正确",
        l.ok_count() == 6 && l.err_count() == 6 && l.audit().is_ok(),
        &format!("ok={} err={} used={}", l.ok_count(), l.err_count(), l.used()),
    );

    s.add(
        "IE3-地址均为负偏移",
        l.addr(0) == Some(-128)
            && l.addr(1) == Some(-96)
            && l.addr(2) == Some(-152)
            && l.addr(3) == Some(-176)
            && l.addr(4) == Some(-224)
            && l.addr(5) == Some(-232),
        "static",
    );

    s.add(
        "IE4-模块块互不重叠",
        {
            let m = build_mods();
            let mut ok = true;
            let mut i = 1;
            while i < m.len() {
                if let (Some(a), Some(b)) = (m.get(i - 1), m.get(i)) {
                    if b.off as i64 + b.memsz as i64 > a.off as i64 {
                        ok = false;
                    }
                }
                i += 1;
            }
            ok
        },
        "static",
    );

    s.add(
        "IE5-越界访问拒绝",
        resolve(Req::make(1, 24, 16, 8), &build_mods(), 0) == Err(IE_EBOUNDS)
            && resolve(Req::make(3, 16, 24, 8), &build_mods(), 0) == Err(IE_EBOUNDS),
        "static",
    );

    s.add(
        "IE6-变量对齐拒绝",
        resolve(Req::make(1, 4, 8, 8), &build_mods(), 0) == Err(IE_EALIGN)
            && resolve(Req::make(0, 0, 8, 8), &build_mods(), 0).is_ok(),
        "static",
    );

    s.add(
        "IE7-模块缺失拒绝",
        resolve(Req::make(9, 0, 8, 8), &build_mods(), 0) == Err(IE_ENOMOD)
            && resolve(Req::make(5, 0, 8, 8), &build_mods(), 0) == Err(IE_ENOMOD),
        "static",
    );

    s.add(
        "IE8-尺寸为零优先拒绝",
        l.sample(10).map(|x| x.code) == Some(IE_ESIZE)
            && resolve(Req::make(9, 0, 0, 8), &build_mods(), 0) == Err(IE_ESIZE),
        "static",
    );

    s.add(
        "IE9-对齐须为 2 的幂",
        resolve(Req::make(0, 0, 8, 6), &build_mods(), 0) == Err(IE_EALIGN)
            && resolve(Req::make(0, 0, 8, 0), &build_mods(), 0) == Err(IE_EALIGN),
        "static",
    );

    s.add(
        "IE10-余量耗尽拒绝",
        l.used() == 40
            && resolve(Req::make(4, 0, 8, 8), &build_mods(), 40) == Err(IE_ESURPLUS)
            && resolve(Req::make(4, 0, 8, 8), &build_mods(), 36) == Ok(Resolved { addr: -232, used: 44 }),
        "static",
    );

    s.add(
        "IE11-启动期模块不占余量",
        {
            let r = resolve(Req::make(0, 0, 8, 8), &build_mods(), 40);
            r == Ok(Resolved { addr: -128, used: 40 })
        },
        "static",
    );

    let mut f = build_ledger();
    let rep = f.replay();
    let root = match f.freeze() {
        Ok(v) => v,
        Err(_) => 0,
    };
    let again = f.freeze() == Err(IE_EFROZEN);
    s.add_dyn(
        "IE12-冻结重放幂等",
        root != 0 && rep == Ok(root) && again && f.root() == root && f.rejects() == 1,
        &format!("root={:016x}", root),
    );

    let inj = inject_four();
    s.add_dyn("IE13-四路注入全绿", inj == [true; 4], &format!("inject={:?}", inj));

    s
}

/// 摘要。
pub fn summary(l: &Ledger) -> alloc::string::String {
    format!("rows={} ok={} err={} used={}", l.len(), l.ok_count(), l.err_count(), l.used())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn address_formula() {
        let m = build_mods();
        assert_eq!(resolve(Req::make(0, 0, 8, 8), &m, 0), Ok(Resolved { addr: -128, used: 0 }));
        assert_eq!(resolve(Req::make(0, 32, 16, 16), &m, 0), Ok(Resolved { addr: -96, used: 0 }));
    }

    #[test]
    fn bounds_and_align() {
        let m = build_mods();
        assert_eq!(resolve(Req::make(1, 24, 16, 8), &m, 0), Err(IE_EBOUNDS));
        assert_eq!(resolve(Req::make(1, 4, 8, 8), &m, 0), Err(IE_EALIGN));
        assert_eq!(resolve(Req::make(0, 0, 8, 6), &m, 0), Err(IE_EALIGN));
    }

    #[test]
    fn size_zero_first() {
        let m = build_mods();
        assert_eq!(resolve(Req::make(9, 0, 0, 8), &m, 0), Err(IE_ESIZE));
        assert_eq!(resolve(Req::make(9, 0, 8, 8), &m, 0), Err(IE_ENOMOD));
    }

    #[test]
    fn surplus_sequence() {
        let m = build_mods();
        assert_eq!(resolve(Req::make(3, 0, 32, 32), &m, 0), Ok(Resolved { addr: -224, used: 32 }));
        assert_eq!(resolve(Req::make(4, 0, 8, 8), &m, 32), Ok(Resolved { addr: -232, used: 40 }));
        assert_eq!(resolve(Req::make(4, 0, 8, 8), &m, 40), Err(IE_ESURPLUS));
    }

    #[test]
    fn ledger_distribution() {
        let l = build_ledger();
        assert_eq!(l.len(), SAMPLES);
        assert_eq!(l.ok_count(), 6);
        assert_eq!(l.err_count(), 6);
        assert_eq!(l.used(), 40);
        assert!(l.audit().is_ok());
    }

    #[test]
    fn refusal_matrix() {
        let mut l = Ledger::new();
        assert_eq!(l.register(0, Req::make(0, 0, 8, 8), true, 0, -128), Ok(()));
        assert_eq!(l.register(0, Req::make(0, 0, 8, 8), true, 0, -128), Err(IE_EDUP));
        assert_eq!(l.register(3, Req::make(0, 0, 8, 8), true, 0, -128), Err(IE_EORDER));
        assert_eq!(l.register(12, Req::make(0, 0, 8, 8), true, 0, -128), Err(IE_EID));
        assert_eq!(l.register(1, Req::make(0, 0, 8, 8), true, 0, -127), Err(IE_EMISMATCH));
        let _ = l.freeze();
        assert_eq!(l.register(1, Req::make(0, 0, 8, 8), true, 0, -128), Err(IE_EFROZEN));
    }

    #[test]
    fn checks_green() {
        let s = run_unx11303_checks();
        assert_eq!(s.len(), 13);
        assert_eq!(s.failed_count(), 0);
        assert_eq!(inject_four(), [true; 4]);
    }
}
