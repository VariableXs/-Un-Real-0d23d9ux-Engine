//! UNX-F11259 ld.so 与内核协作面总图账（0x1f710 段）
//!
//! 锚点：UNX-F11259-J1 协作面总图（装载期 syscall 序全景账）落账，30 样本
//! syscall 序与总图一致率 ≥95%。
//!
//! 纯功能模型：把动态装载期 ld.so 与内核的全部协作点立成一张有序总图——十六
//! 个阶段，每阶段钉一枚 syscall。任何一次真实装载都必须在总图上「单调前进」：
//! 阶段的首次出现次序必须与总图次序一致，缺相即账不成立。总账收三十份样本，
//! 逐份算覆盖率（百分点千分制），再求样本级一致率；一致率低于 950‰ 即整账
//! 拒收。账目一经冻结，后续登记一律拒；重放按同一摘要复核。
//!
//! 规则面：样本号 0..29、样本不许重复、syscall 号必须在总图内、轨迹非空且不
//! 超长、阶段首次出现严格递增；拒绝分门别类记账，失败路径各自独立可验。

use crate::alloc::format;
use crate::checks::CheckSet;
use alloc::vec::Vec;

// ---------------------------------------------------------------- 错误码
const SEG_BASE: u32 = 0x1f710;
/// 样本号越界
pub const MK_EID: u32 = SEG_BASE + 0x01;
/// 样本号重复
pub const MK_EDUP: u32 = SEG_BASE + 0x02;
/// 轨迹为空
pub const MK_EEMPTY: u32 = SEG_BASE + 0x03;
/// 轨迹超长
pub const MK_ECAP: u32 = SEG_BASE + 0x04;
/// 未知 syscall 号
pub const MK_ESYS: u32 = SEG_BASE + 0x05;
/// 阶段次序倒挂
pub const MK_EORDER: u32 = SEG_BASE + 0x06;
/// 缺相（账面阶段不全）
pub const MK_EGAP: u32 = SEG_BASE + 0x07;
/// 一致率不达标
pub const MK_ERATIO: u32 = SEG_BASE + 0x08;
/// 冻结后登记
pub const MK_EFROZEN: u32 = SEG_BASE + 0x09;
/// 重放摘要不符
pub const MK_ESTATE: u32 = SEG_BASE + 0x0a;
/// 总账满
pub const MK_EFULL: u32 = SEG_BASE + 0x0b;

// ---------------------------------------------------------------- 域常量
/// 总图阶段数（十六个协作点）。
pub const PHASES: usize = 16;
/// 样本上限。
pub const SAMPLE_CAP: usize = 30;
/// 单样本轨迹上限。
pub const TRACE_CAP: usize = 40;
/// 一致率下限（千分制 950 = 95%）。
pub const RATIO_FLOOR: u32 = 950;

/// syscall 号：openat
pub const SYS_OPENAT: u8 = 0;
/// syscall 号：read
pub const SYS_READ: u8 = 1;
/// syscall 号：pread64
pub const SYS_PREAD64: u8 = 2;
/// syscall 号：fstat
pub const SYS_FSTAT: u8 = 3;
/// syscall 号：mmap
pub const SYS_MMAP: u8 = 4;
/// syscall 号：mprotect
pub const SYS_MPROTECT: u8 = 5;
/// syscall 号：munmap
pub const SYS_MUNMAP: u8 = 6;
/// syscall 号：close
pub const SYS_CLOSE: u8 = 7;
/// syscall 号：readlink
pub const SYS_READLINK: u8 = 8;
/// syscall 号：newfstatat
pub const SYS_NEWFSTATAT: u8 = 9;
/// syscall 号：getrandom
pub const SYS_GETRANDOM: u8 = 10;
/// syscall 号：arch_prctl
pub const SYS_ARCH_PRCTL: u8 = 11;
/// syscall 号：set_tid_address
pub const SYS_SET_TID: u8 = 12;
/// syscall 号：set_robust_list
pub const SYS_ROBUST: u8 = 13;
/// syscall 号：rseq
pub const SYS_RSEQ: u8 = 14;
/// syscall 号：prlimit64
pub const SYS_PRLIMIT: u8 = 15;

/// 总图：阶段序即 syscall 号序（0..15 一一对应）。
pub const MAP_ORDER: [u8; PHASES] = [
    SYS_OPENAT, SYS_READ, SYS_PREAD64, SYS_FSTAT,
    SYS_MMAP, SYS_MPROTECT, SYS_MUNMAP, SYS_CLOSE,
    SYS_READLINK, SYS_NEWFSTATAT, SYS_GETRANDOM, SYS_ARCH_PRCTL,
    SYS_SET_TID, SYS_ROBUST, SYS_RSEQ, SYS_PRLIMIT,
];

/// syscall 名（与总图同序），供检查项与诊断渲染。
pub const SYS_NAMES: [&str; PHASES] = [
    "openat", "read", "pread64", "fstat",
    "mmap", "mprotect", "munmap", "close",
    "readlink", "newfstatat", "getrandom", "arch_prctl",
    "set_tid_address", "set_robust_list", "rseq", "prlimit64",
];

/// 样本模板甲：close 双点（十八拍）。
pub const TPL_A: [u8; 18] = [
    0, 1, 2, 3, 4, 5, 6, 7, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15,
];
/// 样本模板乙：mmap 复点（十八拍）。
pub const TPL_B: [u8; 18] = [
    0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 4, 5,
];
/// 样本模板丙：纯净十六拍。
pub const TPL_C: [u8; 16] = [
    0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15,
];

const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x100_0000_01b3;

/// 一份装载样本：样本号 + 观测到的 syscall 序。
#[derive(Clone, Debug)]
pub struct LoadSample {
    id: u8,
    trace: Vec<u8>,
}

impl LoadSample {
    /// 建样本（调用方保证号与轨迹已过账本校验）。
    pub fn new(id: u8, trace: Vec<u8>) -> LoadSample {
        LoadSample { id, trace }
    }

    /// 样本号。
    pub fn id(&self) -> u8 {
        self.id
    }

    /// 轨迹长度。
    pub fn len(&self) -> usize {
        self.trace.len()
    }

    /// 轨迹是否为空。
    pub fn is_empty(&self) -> bool {
        self.trace.is_empty()
    }

    /// 第 i 拍 syscall 号。
    pub fn at(&self, i: usize) -> Option<u8> {
        self.trace.get(i).copied()
    }

    /// 某阶段在本样本中的首次出现位置。
    pub fn first_index(&self, kind: u8) -> Option<usize> {
        let mut i = 0usize;
        while i < self.trace.len() {
            if let Some(v) = self.trace.get(i) {
                if *v == kind {
                    return Some(i);
                }
            }
            i = i.wrapping_add(1);
        }
        None
    }

    /// 阶段首次出现是否严格单调（缺失阶段不参与比较，由 `missing` 单独判）。
    pub fn monotone(&self) -> bool {
        let mut last: i64 = -1;
        let mut k = 0usize;
        while k < MAP_ORDER.len() {
            if let Some(kind) = MAP_ORDER.get(k) {
                if let Some(at) = self.first_index(*kind) {
                    if (at as i64) <= last {
                        return false;
                    }
                    last = at as i64;
                }
            }
            k = k.wrapping_add(1);
        }
        true
    }

    /// 缺相清单（总图有此阶段、本样本一次未出现）。
    pub fn missing(&self) -> Vec<u8> {
        let mut out: Vec<u8> = Vec::new();
        let mut k = 0usize;
        while k < MAP_ORDER.len() {
            if let Some(kind) = MAP_ORDER.get(k) {
                if self.first_index(*kind).is_none() {
                    out.push(*kind);
                }
            }
            k = k.wrapping_add(1);
        }
        out
    }

    /// 覆盖阶段数（0..=16）。
    pub fn phases_present(&self) -> u32 {
        MAP_ORDER.len() as u32 - self.missing().len() as u32
    }

    /// 覆盖率（千分制）。
    pub fn coverage_permille(&self) -> u32 {
        self.phases_present() * 1000 / MAP_ORDER.len() as u32
    }

    /// 样本指纹：号 + 逐拍 syscall。
    pub fn fold(&self) -> u64 {
        let mut h = FNV_OFF;
        h = (h ^ self.id as u64).wrapping_mul(FNV_PRIME);
        let mut i = 0usize;
        while i < self.trace.len() {
            if let Some(v) = self.trace.get(i) {
                h = (h ^ *v as u64).wrapping_mul(FNV_PRIME);
            }
            i = i.wrapping_add(1);
        }
        h
    }
}

/// 协作面总账：收样本、算一致率、冻结重放。
pub struct MapLedger {
    samples: Vec<LoadSample>,
    frozen: bool,
    root: u64,
    rejects: u32,
    unknowns: u32,
    orders: u32,
    gaps: u32,
}

impl MapLedger {
    /// 空账。
    pub fn new() -> MapLedger {
        MapLedger {
            samples: Vec::new(),
            frozen: false,
            root: 0,
            rejects: 0,
            unknowns: 0,
            orders: 0,
            gaps: 0,
        }
    }

    fn fail<T>(&mut self, code: u32) -> Result<T, u32> {
        self.rejects = self.rejects.wrapping_add(1);
        if code == MK_ESYS {
            self.unknowns = self.unknowns.wrapping_add(1);
        }
        if code == MK_EORDER {
            self.orders = self.orders.wrapping_add(1);
        }
        if code == MK_EGAP {
            self.gaps = self.gaps.wrapping_add(1);
        }
        Err(code)
    }

    /// 登记一份样本：八关依次过。
    pub fn register(&mut self, id: u8, trace: &[u8]) -> Result<u64, u32> {
        if self.frozen {
            return self.fail(MK_EFROZEN);
        }
        if id as usize >= SAMPLE_CAP {
            return self.fail(MK_EID);
        }
        if self.find(id).is_some() {
            return self.fail(MK_EDUP);
        }
        if trace.is_empty() {
            return self.fail(MK_EEMPTY);
        }
        if trace.len() > TRACE_CAP {
            return self.fail(MK_ECAP);
        }
        let mut i = 0usize;
        while i < trace.len() {
            match trace.get(i) {
                Some(v) => {
                    if (*v as usize) >= PHASES {
                        return self.fail(MK_ESYS);
                    }
                }
                None => return self.fail(MK_ESTATE),
            }
            i = i.wrapping_add(1);
        }
        let mut kind = 0u8;
        let mut last: i64 = -1;
        while (kind as usize) < PHASES {
            let mut at: Option<usize> = None;
            let mut j = 0usize;
            while j < trace.len() {
                if let Some(v) = trace.get(j) {
                    if *v == kind {
                        at = Some(j);
                        break;
                    }
                }
                j = j.wrapping_add(1);
            }
            if let Some(pos) = at {
                if (pos as i64) <= last {
                    return self.fail(MK_EORDER);
                }
                last = pos as i64;
            }
            kind = kind.wrapping_add(1);
        }
        let mut owned: Vec<u8> = Vec::new();
        let mut k = 0usize;
        while k < trace.len() {
            if let Some(v) = trace.get(k) {
                owned.push(*v);
            }
            k = k.wrapping_add(1);
        }
        self.samples.push(LoadSample::new(id, owned));
        Ok(self.fold())
    }

    /// 按号取样本。
    pub fn find(&self, id: u8) -> Option<&LoadSample> {
        let mut i = 0usize;
        while i < self.samples.len() {
            if let Some(s) = self.samples.get(i) {
                if s.id() == id {
                    return Some(s);
                }
            }
            i = i.wrapping_add(1);
        }
        None
    }

    /// 已收样本数。
    pub fn len(&self) -> usize {
        self.samples.len()
    }

    /// 是否空账。
    pub fn is_empty(&self) -> bool {
        self.samples.is_empty()
    }

    /// 第 i 份样本（登记序）。
    pub fn sample(&self, i: usize) -> Option<&LoadSample> {
        self.samples.get(i)
    }

    /// 样本级一致率（千分制）：覆盖全十六相的样本占比。
    pub fn agree_permille(&self) -> u32 {
        if self.samples.is_empty() {
            return 0;
        }
        let mut full = 0u32;
        let mut i = 0usize;
        while i < self.samples.len() {
            if let Some(s) = self.samples.get(i) {
                if s.phases_present() as usize == PHASES {
                    full = full.wrapping_add(1);
                }
            }
            i = i.wrapping_add(1);
        }
        full * 1000 / self.samples.len() as u32
    }

    /// 全账平均覆盖率（千分制）。
    pub fn mean_coverage(&self) -> u32 {
        if self.samples.is_empty() {
            return 0;
        }
        let mut sum = 0u32;
        let mut i = 0usize;
        while i < self.samples.len() {
            if let Some(s) = self.samples.get(i) {
                sum = sum.wrapping_add(s.coverage_permille());
            }
            i = i.wrapping_add(1);
        }
        sum / self.samples.len() as u32
    }

    /// 缺相合计（逐样相加）。
    pub fn missing_total(&self) -> u32 {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < self.samples.len() {
            if let Some(s) = self.samples.get(i) {
                n = n.wrapping_add(s.missing().len() as u32);
            }
            i = i.wrapping_add(1);
        }
        n
    }

    /// 累计拒绝数。
    pub fn rejects(&self) -> u32 {
        self.rejects
    }

    /// 未知 syscall 拒绝数。
    pub fn unknown_rejects(&self) -> u32 {
        self.unknowns
    }

    /// 次序倒挂拒绝数。
    pub fn order_rejects(&self) -> u32 {
        self.orders
    }

    /// 缺相拒绝数。
    pub fn gap_rejects(&self) -> u32 {
        self.gaps
    }

    /// 全账指纹：样本登记序指纹 + 四类拒绝计数。
    pub fn fold(&self) -> u64 {
        let mut h = FNV_OFF;
        let mut i = 0usize;
        while i < self.samples.len() {
            if let Some(s) = self.samples.get(i) {
                h = (h ^ s.fold()).wrapping_mul(FNV_PRIME);
            }
            i = i.wrapping_add(1);
        }
        h = (h ^ self.rejects as u64).wrapping_mul(FNV_PRIME);
        h = (h ^ self.unknowns as u64).wrapping_mul(FNV_PRIME);
        h = (h ^ self.orders as u64).wrapping_mul(FNV_PRIME);
        h = (h ^ self.gaps as u64).wrapping_mul(FNV_PRIME);
        h
    }

    /// 交账审计：满三十份、零缺相、一致率达标，三关全过才算账成立。
    pub fn audit(&mut self) -> Result<u32, u32> {
        if self.frozen {
            return self.fail(MK_EFROZEN);
        }
        if self.samples.len() != SAMPLE_CAP {
            return self.fail(MK_EFULL);
        }
        if self.missing_total() != 0 {
            return self.fail(MK_EGAP);
        }
        let ratio = self.agree_permille();
        if ratio < RATIO_FLOOR {
            return self.fail(MK_ERATIO);
        }
        Ok(ratio)
    }

    /// 冻结：审计通过才允许封账，封账后不可再登记。
    pub fn freeze(&mut self) -> Result<u64, u32> {
        if self.frozen {
            return self.fail(MK_EFROZEN);
        }
        if self.samples.len() != SAMPLE_CAP
            || self.missing_total() != 0
            || self.agree_permille() < RATIO_FLOOR
        {
            return self.fail(MK_ESTATE);
        }
        self.frozen = true;
        self.root = self.fold();
        Ok(self.root)
    }

    /// 冻结态重放：重算指纹必须与封账基准一致。
    pub fn replay(&mut self) -> Result<u64, u32> {
        if !self.frozen {
            return self.fail(MK_ESTATE);
        }
        let now = self.fold();
        if now != self.root {
            return self.fail(MK_ESTATE);
        }
        Ok(now)
    }

    /// 是否已封账。
    pub fn is_frozen(&self) -> bool {
        self.frozen
    }

    /// 封账基准指纹。
    pub fn root(&self) -> u64 {
        self.root
    }
}

/// 取模板：0 甲、1 乙、2 丙。
pub fn template_of(idx: u8) -> &'static [u8] {
    match idx % 3 {
        0 => &TPL_A,
        1 => &TPL_B,
        _ => &TPL_C,
    }
}

/// 标准总账：三十份样本按模板轮转装入。
pub fn build_ledger() -> MapLedger {
    let mut l = MapLedger::new();
    let mut id = 0u8;
    while (id as usize) < SAMPLE_CAP {
        let _ = l.register(id, template_of(id));
        id = id.wrapping_add(1);
    }
    l
}

/// 四路独立注入：未知 syscall、次序倒挂、样本重号、封账后登记。
pub fn inject_four() -> [bool; 4] {
    let mut ok = [false; 4];

    let mut a = MapLedger::new();
    ok[0] = matches!(a.register(0, &[0, 1, 16]), Err(MK_ESYS));

    let mut b = MapLedger::new();
    ok[1] = matches!(b.register(0, &[1, 0]), Err(MK_EORDER));

    let mut c = MapLedger::new();
    let _ = c.register(0, &TPL_C);
    ok[2] = matches!(c.register(0, &TPL_C), Err(MK_EDUP));

    let mut d = build_ledger();
    let _ = d.freeze();
    ok[3] = matches!(d.register(29, &TPL_C), Err(MK_EFROZEN));

    ok
}

// ---------------------------------------------------------------- 判据
/// 十三项判据：总图形状、样本登记、覆盖与一致率、五类拒绝、审计、冻结重放。
pub fn run_unx11259_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11259.ldso_map");

    let mut order_ok = MAP_ORDER.len() == PHASES;
    let mut k = 0usize;
    while k < MAP_ORDER.len() {
        if let Some(v) = MAP_ORDER.get(k) {
            if *v as usize != k {
                order_ok = false;
            }
        }
        k = k.wrapping_add(1);
    }
    s.add(
        "MK1-总图十六相自洽",
        order_ok && SYS_NAMES.len() == PHASES && PHASES == 16,
        "阶段序与 syscall 号序一一对应",
    );

    let empty = MapLedger::new();
    s.add(
        "MK2-空账基线",
        empty.len() == 0
            && empty.is_empty()
            && empty.agree_permille() == 0
            && empty.mean_coverage() == 0
            && empty.missing_total() == 0,
        "static",
    );

    let l = build_ledger();
    let mut ids_ok = true;
    let mut len_ok = true;
    let mut i = 0usize;
    while i < SAMPLE_CAP {
        match l.sample(i) {
            Some(smp) => {
                if smp.id() as usize != i {
                    ids_ok = false;
                }
                let want = if i % 3 == 2 { 16usize } else { 18usize };
                if smp.len() != want {
                    len_ok = false;
                }
            }
            None => {
                ids_ok = false;
                len_ok = false;
            }
        }
        i = i.wrapping_add(1);
    }
    s.add(
        "MK3-三十样本登记",
        l.len() == SAMPLE_CAP && ids_ok && len_ok,
        "static",
    );

    let mut mono_ok = true;
    let mut j = 0usize;
    while j < SAMPLE_CAP {
        match l.sample(j) {
            Some(smp) => {
                if !smp.monotone() || !smp.missing().is_empty() {
                    mono_ok = false;
                }
            }
            None => mono_ok = false,
        }
        j = j.wrapping_add(1);
    }
    s.add_dyn(
        "MK4-样本覆盖全相",
        mono_ok
            && l.missing_total() == 0
            && l.mean_coverage() == 1000
            && l.agree_permille() == 1000,
        &format!("mean={} agree={}", l.mean_coverage(), l.agree_permille()),
    );

    s.add(
        "MK5-一致率门槛",
        RATIO_FLOOR == 950 && l.agree_permille() >= RATIO_FLOOR,
        "千分制下限 950",
    );

    let mut a = MapLedger::new();
    let u1 = a.register(0, &[0, 1, 16]) == Err(MK_ESYS);
    let u2 = a.unknown_rejects() == 1;
    let u3 = a.register(1, &[0, 1, 2]).is_ok();
    s.add(
        "MK6-未知syscall拒绝",
        u1 && u2 && u3 && a.rejects() == 1,
        "static",
    );

    let mut b = MapLedger::new();
    let e1 = b.register(0, &[]) == Err(MK_EEMPTY);
    let mut long: Vec<u8> = Vec::new();
    let mut c = 0u8;
    while (c as usize) < TRACE_CAP + 1 {
        long.push((c as usize % PHASES) as u8);
        c = c.wrapping_add(1);
    }
    let e2 = b.register(1, &long) == Err(MK_ECAP);
    s.add(
        "MK7-空轨迹与超长",
        e1 && e2 && b.rejects() == 2,
        "static",
    );

    let mut d = MapLedger::new();
    let i1 = d.register(SAMPLE_CAP as u8, &TPL_C) == Err(MK_EID);
    let i2 = d.register(0, &TPL_C).is_ok();
    let i3 = d.register(0, &TPL_C) == Err(MK_EDUP);
    s.add(
        "MK8-样本号越界与重号",
        i1 && i2 && i3 && d.len() == 1 && d.rejects() == 2,
        "static",
    );

    let mut e = MapLedger::new();
    let o1 = e.register(0, &[1, 0]) == Err(MK_EORDER);
    let o2 = e.order_rejects() == 1;
    let o3 = e.register(0, &[0, 1, 2]).is_ok();
    s.add(
        "MK9-阶段次序倒挂",
        o1 && o2 && o3 && e.rejects() == 1,
        "static",
    );

    let mut f = MapLedger::new();
    let _ = f.register(0, &[0, 1, 2]);
    let g1 = match f.find(0) {
        Some(smp) => smp.missing().len() == PHASES - 3 && smp.phases_present() == 3,
        None => false,
    };
    let g2 = f.missing_total() == (PHASES as u32 - 3);
    let g3 = match f.find(0) {
        Some(smp) => smp.coverage_permille() == 187,
        None => false,
    };
    s.add("MK10-缺相逐项清点", g1 && g2 && g3, "static");

    let mut full = build_ledger();
    let au = full.audit();
    let mut short = MapLedger::new();
    let mut sid = 0u8;
    while (sid as usize) < SAMPLE_CAP - 1 {
        let _ = short.register(sid, template_of(sid));
        sid = sid.wrapping_add(1);
    }
    let au_short = short.audit() == Err(MK_EFULL);
    let mut thin = MapLedger::new();
    let mut tid = 0u8;
    while (tid as usize) < SAMPLE_CAP {
        let _ = thin.register(tid, &[0, 1, 2]);
        tid = tid.wrapping_add(1);
    }
    let au_thin = thin.audit() == Err(MK_EGAP);
    s.add(
        "MK11-审计三关",
        au == Ok(1000) && au_short && au_thin,
        "满册、零缺相、达率",
    );

    let mut z = build_ledger();
    let fr = z.freeze();
    let root = match fr {
        Ok(v) => v,
        Err(_) => 0,
    };
    let rp = z.replay();
    let again = z.freeze() == Err(MK_EFROZEN);
    let late = z.register(0, &TPL_C) == Err(MK_EFROZEN);
    let root_at = z.root();
    s.add_dyn(
        "MK12-封账重放幂等",
        fr.is_ok() && rp == fr && again && late && root_at == root && z.is_frozen(),
        &format!("root={:x}", root),
    );

    let inj = inject_four();
    s.add(
        "MK13-四路注入",
        inj[0] && inj[1] && inj[2] && inj[3],
        "static",
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diagram_shape() {
        assert_eq!(MAP_ORDER.len(), PHASES);
        assert_eq!(SYS_NAMES.len(), PHASES);
        let mut i = 0usize;
        while i < PHASES {
            assert_eq!(MAP_ORDER[i] as usize, i);
            assert!(!SYS_NAMES[i].is_empty());
            i += 1;
        }
        assert_eq!(SYS_OPENAT, 0);
        assert_eq!(SYS_PRLIMIT, 15);
        assert_eq!(SAMPLE_CAP, 30);
        assert_eq!(TRACE_CAP, 40);
        assert_eq!(RATIO_FLOOR, 950);
    }

    #[test]
    fn templates_are_monotone_and_full() {
        let l = build_ledger();
        assert_eq!(l.len(), 30);
        let mut i = 0usize;
        while i < 30 {
            let smp = match l.sample(i) {
                Some(v) => v,
                None => panic!(),
            };
            assert!(smp.monotone());
            assert!(smp.missing().is_empty());
            assert_eq!(smp.phases_present(), PHASES as u32);
            assert_eq!(smp.coverage_permille(), 1000);
            i += 1;
        }
        assert_eq!(l.agree_permille(), 1000);
        assert_eq!(l.mean_coverage(), 1000);
        assert_eq!(l.missing_total(), 0);
    }

    #[test]
    fn audit_gates() {
        let mut full = build_ledger();
        assert_eq!(full.audit(), Ok(1000));
        assert!(full.freeze().is_ok());

        let mut short = MapLedger::new();
        let mut sid = 0u8;
        while (sid as usize) < SAMPLE_CAP - 1 {
            assert!(short.register(sid, template_of(sid)).is_ok());
            sid += 1;
        }
        assert_eq!(short.audit(), Err(MK_EFULL));

        let mut thin = MapLedger::new();
        let mut tid = 0u8;
        while (tid as usize) < SAMPLE_CAP {
            assert!(thin.register(tid, &[0, 1, 2]).is_ok());
            tid += 1;
        }
        assert_eq!(thin.missing_total(), (PHASES as u32 - 3) * SAMPLE_CAP as u32);
        assert_eq!(thin.audit(), Err(MK_EGAP));
    }

    #[test]
    fn refusal_matrix() {
        let mut l = MapLedger::new();
        assert_eq!(l.register(SAMPLE_CAP as u8, &TPL_C), Err(MK_EID));
        assert_eq!(l.register(0, &[]), Err(MK_EEMPTY));
        let mut long: Vec<u8> = Vec::new();
        let mut c = 0u8;
        while (c as usize) < TRACE_CAP + 1 {
            long.push((c as usize % PHASES) as u8);
            c += 1;
        }
        assert_eq!(l.register(0, &long), Err(MK_ECAP));
        assert_eq!(l.register(0, &[0, 1, 16]), Err(MK_ESYS));
        assert_eq!(l.unknown_rejects(), 1);
        assert_eq!(l.register(0, &[1, 0]), Err(MK_EORDER));
        assert_eq!(l.order_rejects(), 1);
        assert!(l.register(0, &[0, 1, 2]).is_ok());
        assert_eq!(l.register(0, &[0, 1, 2]), Err(MK_EDUP));
        assert_eq!(l.len(), 1);
        assert_eq!(l.rejects(), 6);
    }

    #[test]
    fn freeze_replay_idempotent() {
        let mut l = build_ledger();
        let root = l.freeze();
        assert!(root.is_ok());
        assert_eq!(l.replay(), root);
        assert_eq!(l.freeze(), Err(MK_EFROZEN));
        assert_eq!(l.register(0, &TPL_C), Err(MK_EFROZEN));
        assert_eq!(l.root(), match root {
            Ok(v) => v,
            Err(_) => 0,
        });
        let mut cold = MapLedger::new();
        assert_eq!(cold.replay(), Err(MK_ESTATE));
        assert_eq!(cold.freeze(), Err(MK_ESTATE));
    }

    #[test]
    fn checks_green_and_injection() {
        let s = run_unx11259_checks();
        assert!(s.all_passed(), "UNX-F11259 判据未全绿: {}", s.red_items_text());
        assert_eq!(s.len(), 13);
        assert_eq!(s.failed_count(), 0);
        assert_eq!(inject_four(), [true, true, true, true]);
    }
}
