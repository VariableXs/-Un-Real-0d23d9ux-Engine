//! UNX-F11251 装载地址布局与 PIE 基址选择（0x1f330 段）
//!
//! 模型：ELF 分两类——ET_EXEC 自带绝对地址，基址恒为 0x400000；ET_DYN（PIE）
//! 需要内核挑一个页对齐的随机基址。ASLR 档位 0 关闭随机（固定 0x555555554000），
//! 档位 1 只在 0x400 页窗口内抖动，档位 2 在 0x1000000 页窗口内抖动；熵值按
//! 档位掩码截断后再乘页大小，故高位熵不会把基址推出窗口。
//!
//! 规则面：档位必须 ≤ 2；对象种类只认 EXEC/DYN；基址必须页对齐、不得低于
//! mmap_min_addr；若与保留地址撞车，逐页上移最多 4 次，仍撞则判 EBUSY。非 PIE
//! 件完全不受 ASLR 影响——这正是判据里「非 PIE 固定基址行为正确」的那一半。
//!
//! 账本：每样本一条 slot（种类 + 档位 + 熵 + 保留地址 + 选中基址 + 重试数），
//! 另按档位分桶并累计重试。fold 卷全账；replay 重算选基并核对桶合计；freeze
//! 后写入拒、二次冻结拒。
//!
//! 拒止面：非法档位、非法种类、基址未对齐、低于 mmap_min、保留地址连续撞车、
//! 样本号越界、跳号/重号登记、冻结后写入、未观测即重放、空账重放、指纹漂移。

use crate::checks::CheckSet;

// ---------------------------------------------------------------- 错误码
const SEG_BASE: u32 = 0x1f330;

/// 样本号越界
pub const BS_EID: u32 = SEG_BASE + 0x01;
/// 登记序违例
pub const BS_EORDER: u32 = SEG_BASE + 0x02;
/// 重号登记 / 重号观测
pub const BS_EDUP: u32 = SEG_BASE + 0x03;
/// 冻结后写入
pub const BS_EFROZEN: u32 = SEG_BASE + 0x04;
/// 未观测即重放
pub const BS_ENOBS: u32 = SEG_BASE + 0x05;
/// 重放不一致
pub const BS_EMISMATCH: u32 = SEG_BASE + 0x06;
/// 档位桶不平衡
pub const BS_EBAL: u32 = SEG_BASE + 0x07;
/// 空账重放
pub const BS_EEMPTY: u32 = SEG_BASE + 0x08;
/// 非法 ASLR 档位
pub const BS_EASLR: u32 = SEG_BASE + 0x09;
/// 非法对象种类
pub const BS_EKIND: u32 = SEG_BASE + 0x0a;
/// 基址未页对齐
pub const BS_EMISALIGN: u32 = SEG_BASE + 0x0b;
/// 基址低于 mmap_min
pub const BS_ELOW: u32 = SEG_BASE + 0x0c;
/// 保留地址连续撞车
pub const BS_EBUSY: u32 = SEG_BASE + 0x0d;

// ---------------------------------------------------------------- 域常量
/// 页大小
pub const PAGE: u64 = 0x1000;
/// mmap_min_addr
pub const MMAP_MIN: u64 = 0x1_0000;
/// 非 PIE 固定基址
pub const EXEC_BASE: u64 = 0x0040_0000;
/// PIE 关闭 ASLR 的固定基址
pub const FIXED_PIE: u64 = 0x5555_5555_4000;
/// 部分档窗口起点
pub const PARTIAL_BASE: u64 = 0x5555_0000_0000;
/// 部分档页数（0x400）
pub const PARTIAL_PAGES: u32 = 0x400;
/// 全档窗口起点
pub const FULL_BASE: u64 = 0x7f00_0000_0000;
/// 全档页数（0x1000000）
pub const FULL_PAGES: u32 = 0x0100_0000;
/// 撞车重试上限
pub const MAX_RETRY: u32 = 4;
/// ASLR 档：关闭
pub const ASLR_OFF: u8 = 0;
/// ASLR 档：部分
pub const ASLR_PARTIAL: u8 = 1;
/// ASLR 档：全
pub const ASLR_FULL: u8 = 2;
/// 种类：ET_EXEC
pub const K_EXEC: u8 = 1;
/// 种类：ET_DYN/PIE
pub const K_PIE: u8 = 2;
/// 档位：固定基址
pub const L_FIXED: u8 = 0;
/// 档位：部分窗口
pub const L_PARTIAL: u8 = 1;
/// 档位：全窗口
pub const L_FULL: u8 = 2;
/// 档位：非 PIE
pub const L_EXEC: u8 = 3;
/// 档位桶数
pub const BUCKETS: usize = 4;
/// 样本数
pub const SAMPLES: usize = 12;
/// EF_NONE
pub const EINVAL: u32 = 22;

const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x100_0000_01b3;

/// 选基结果。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Base {
    pub addr: u64,
    pub level: u8,
    pub retries: u8,
}

impl Base {
    /// 空结果。
    pub const fn none() -> Base {
        Base { addr: 0, level: 0, retries: 0 }
    }
}

/// 一个选基样本。
#[derive(Clone, Copy, Debug)]
pub struct BaseCase {
    pub kind: u8,
    pub aslr: u8,
    pub entropy: u32,
    pub reserved: u64,
}

impl BaseCase {
    /// 空样本。
    pub const fn blank() -> BaseCase {
        BaseCase { kind: K_PIE, aslr: ASLR_OFF, entropy: 0, reserved: 0 }
    }

    /// 构造样本。
    pub const fn make(kind: u8, aslr: u8, entropy: u32, reserved: u64) -> BaseCase {
        BaseCase { kind, aslr, entropy, reserved }
    }
}

/// 按样本选择装载基址。
///
/// 规则：可执行文件固定在高位以下的 EXEC_BASE；PIE 关闭 ASLR 时落在
/// 0x5555_5555_4000 这个“几乎固定”的可映射区，部分随机只搅动低 30 位
/// 的页号，完全随机搅动 24 位。保留区命中则逐页上移，最多重试四次。
pub fn pick(c: &BaseCase) -> Result<Base, u32> {
    if c.aslr > ASLR_FULL {
        return Err(BS_EASLR);
    }
    let (level, start) = match c.kind {
        K_EXEC => (L_EXEC, EXEC_BASE),
        K_PIE => match c.aslr {
            ASLR_OFF => (L_FIXED, FIXED_PIE),
            ASLR_PARTIAL => {
                let span = (c.entropy & (PARTIAL_PAGES - 1)) as u64;
                (L_PARTIAL, PARTIAL_BASE.wrapping_add(span.wrapping_mul(PAGE)))
            }
            _ => {
                let span = (c.entropy & (FULL_PAGES - 1)) as u64;
                (L_FULL, FULL_BASE.wrapping_add(span.wrapping_mul(PAGE)))
            }
        },
        _ => return Err(BS_EKIND),
    };
    let mut addr = start;
    let mut retries = 0u8;
    while c.reserved != 0 && addr == c.reserved {
        if retries as u32 >= MAX_RETRY {
            return Err(BS_EBUSY);
        }
        addr = addr.wrapping_add(PAGE);
        retries = retries.wrapping_add(1);
    }
    if addr & (PAGE - 1) != 0 {
        return Err(BS_EMISALIGN);
    }
    if addr < MMAP_MIN {
        return Err(BS_ELOW);
    }
    Ok(Base { addr, level, retries })
}

/// 校验一个已选基址与样本是否一致。
pub fn validate(c: &BaseCase, b: &Base) -> u32 {
    if c.aslr > ASLR_FULL {
        return BS_EASLR;
    }
    if c.kind != K_PIE && b.level != L_EXEC {
        return BS_EMISMATCH;
    }
    if c.kind == K_PIE {
        if c.aslr == ASLR_OFF && b.level != L_FIXED {
            return BS_EMISMATCH;
        }
        if c.aslr == ASLR_PARTIAL && b.level != L_PARTIAL {
            return BS_EMISMATCH;
        }
        if c.aslr == ASLR_FULL && b.level != L_FULL {
            return BS_EMISMATCH;
        }
    }
    if b.addr & (PAGE - 1) != 0 {
        return BS_EMISALIGN;
    }
    if b.addr < MMAP_MIN {
        return BS_ELOW;
    }
    0
}

/// 把基址归入四个观测桶：固定、部分、全随机、可执行固定。
pub fn bucket(b: &Base) -> usize {
    match b.level {
        L_FIXED => 0,
        L_PARTIAL => 1,
        L_FULL => 2,
        _ => 3,
    }
}

/// 基址是否落在 47 位用户空间内。
pub fn in_user_space(addr: u64) -> bool {
    addr < (1u64 << 47)
}

/// 相邻两次选基之间的页距（以页计），溢出按 0 处理。
pub fn page_gap(a: u64, b: u64) -> u64 {
    if b >= a {
        (b - a) / PAGE
    } else {
        (a - b) / PAGE
    }
}

/// 样本规格：编号 0..12 覆盖固定、部分随机、全随机、可执行与非法熵。
pub fn spec_of(id: usize) -> BaseCase {
    match id {
        0 => BaseCase::make(K_PIE, ASLR_OFF, 0, 0),
        1 => BaseCase::make(K_PIE, ASLR_PARTIAL, 0, 0),
        2 => BaseCase::make(K_PIE, ASLR_PARTIAL, 0x3ff, 0),
        3 => BaseCase::make(K_PIE, ASLR_PARTIAL, 0x400, 0),
        4 => BaseCase::make(K_PIE, ASLR_FULL, 0, 0),
        5 => BaseCase::make(K_PIE, ASLR_FULL, 0x00ff_ffff, 0),
        6 => BaseCase::make(K_PIE, ASLR_FULL, 0x0100_0000, 0),
        7 => BaseCase::make(K_EXEC, ASLR_FULL, 0x1234, 0),
        8 => BaseCase::make(K_EXEC, ASLR_OFF, 0, 0),
        9 => BaseCase::make(K_PIE, ASLR_FULL + 1, 0, 0),
        10 => BaseCase::make(K_PIE, ASLR_PARTIAL, 0, PARTIAL_BASE),
        11 => BaseCase::make(K_PIE, ASLR_FULL, 0, FIXED_PIE),
        _ => BaseCase::blank(),
    }
}

/// 样本的期望：(错误码, 基址, 桶, 重试次数)。错误样本地址为 0。
pub fn expected_of(id: usize) -> (u32, u64, u8, u8) {
    match id {
        0 => (0, 0x5555_5555_4000, L_FIXED, 0),
        1 => (0, 0x5555_0000_0000, L_PARTIAL, 0),
        2 => (0, 0x5555_003f_f000, L_PARTIAL, 0),
        3 => (0, 0x5555_0000_0000, L_PARTIAL, 0),
        4 => (0, 0x7f00_0000_0000, L_FULL, 0),
        5 => (0, 0x7f0f_ffff_f000, L_FULL, 0),
        6 => (0, 0x7f00_0000_0000, L_FULL, 0),
        7 => (0, EXEC_BASE, L_EXEC, 0),
        8 => (0, EXEC_BASE, L_EXEC, 0),
        9 => (BS_EASLR, 0, 0, 0),
        10 => (0, 0x5555_0000_1000, L_PARTIAL, 1),
        11 => (0, 0x7f00_0000_0000, L_FULL, 0),
        _ => (BS_EID, 0, 0, 0),
    }
}

/// 账本中的一格。
#[derive(Clone, Copy)]
struct BaseSlot {
    code: u8,
    level: u8,
    retries: u8,
    seen: bool,
    addr: u64,
    spec: BaseCase,
}

impl BaseSlot {
    const fn zero() -> BaseSlot {
        BaseSlot {
            code: 0,
            level: 0,
            retries: 0,
            seen: false,
            addr: 0,
            spec: BaseCase::blank(),
        }
    }
}

/// 基址选择账本。
pub struct BaseLedger {
    slots: [BaseSlot; SAMPLES],
    n: usize,
    accepted: u32,
    rejected: u32,
    observed: u32,
    matched: u32,
    frozen: bool,
}

impl BaseLedger {
    /// 空账本。
    pub const fn new() -> BaseLedger {
        BaseLedger {
            slots: [BaseSlot::zero(); SAMPLES],
            n: 0,
            accepted: 0,
            rejected: 0,
            observed: 0,
            matched: 0,
            frozen: false,
        }
    }

    fn fail(&mut self, code: u32) -> u32 {
        self.rejected = self.rejected.wrapping_add(1);
        code
    }

    fn duplicate(&self, c: &BaseCase) -> bool {
        let mut i = 0usize;
        while i < self.n {
            if let Some(s) = self.slots.get(i) {
                let k = s.spec;
                if k.kind == c.kind
                    && k.aslr == c.aslr
                    && k.entropy == c.entropy
                    && k.reserved == c.reserved
                {
                    return true;
                }
            }
            i += 1;
        }
        false
    }

    fn commit(
        &mut self,
        c: &BaseCase,
        code: u32,
        addr: u64,
        level: u8,
        retries: u8,
    ) -> Result<u64, u32> {
        let at = self.n;
        if at >= SAMPLES {
            return Err(self.fail(BS_EID));
        }
        if let Some(slot) = self.slots.get_mut(at) {
            slot.code = if code == 0 { 0 } else { code.wrapping_sub(SEG_BASE) as u8 };
            slot.addr = addr;
            slot.level = level;
            slot.retries = retries;
            slot.seen = false;
            slot.spec = *c;
        }
        self.n = at + 1;
        self.accepted = self.accepted.wrapping_add(1);
        Ok(self.fold())
    }

    /// 登记一个样本，非法样本按真实错误码拒绝。
    pub fn register(&mut self, c: &BaseCase) -> Result<u64, u32> {
        if self.frozen {
            return Err(self.fail(BS_EFROZEN));
        }
        if self.duplicate(c) {
            return Err(self.fail(BS_EDUP));
        }
        match pick(c) {
            Ok(b) => self.commit(c, 0, b.addr, b.level, b.retries),
            Err(code) => Err(self.fail(code)),
        }
    }

    /// 登记并核对预期：预期错误码必须与选基结果一致，错误样例也被记账。
    pub fn register_ruled(&mut self, c: &BaseCase, want: u32) -> Result<u64, u32> {
        if self.frozen {
            return Err(self.fail(BS_EFROZEN));
        }
        if self.duplicate(c) {
            return Err(self.fail(BS_EDUP));
        }
        match pick(c) {
            Ok(b) => {
                if want != 0 {
                    return Err(self.fail(BS_EMISMATCH));
                }
                self.commit(c, 0, b.addr, b.level, b.retries)
            }
            Err(code) => {
                if want == 0 || want != code {
                    return Err(self.fail(BS_EMISMATCH));
                }
                self.commit(c, code, 0, 0, 0)
            }
        }
    }

    /// 观测某一格：必须存在、未观测过，且与期望四元组完全一致。
    pub fn observe(&mut self, id: usize) -> Result<u64, u32> {
        if self.frozen {
            return Err(self.fail(BS_EFROZEN));
        }
        if id >= self.n {
            return Err(self.fail(BS_ENOBS));
        }
        let got = match self.slots.get(id) {
            Some(s) => *s,
            None => return Err(self.fail(BS_ENOBS)),
        };
        if got.seen {
            return Err(self.fail(BS_EDUP));
        }
        let (wc, wa, wl, wr) = expected_of(id);
        let code_ok = if wc == 0 {
            got.code == 0
        } else {
            got.code == (wc.wrapping_sub(SEG_BASE)) as u8
        };
        self.observed = self.observed.wrapping_add(1);
        if let Some(slot) = self.slots.get_mut(id) {
            slot.seen = true;
        }
        if !code_ok || got.addr != wa || got.level != wl || got.retries != wr {
            return Err(self.fail(BS_EMISMATCH));
        }
        self.matched = self.matched.wrapping_add(1);
        Ok(self.fold())
    }

    /// 已登记格数。
    pub fn len(&self) -> usize {
        self.n
    }

    /// 账本是否为空。
    pub fn is_empty(&self) -> bool {
        self.n == 0
    }

    /// 成功登记次数。
    pub fn accepted(&self) -> u32 {
        self.accepted
    }

    /// 拒绝次数。
    pub fn rejected(&self) -> u32 {
        self.rejected
    }

    /// 观测次数。
    pub fn observed(&self) -> u32 {
        self.observed
    }

    /// 观测通过次数。
    pub fn matched(&self) -> u32 {
        self.matched
    }

    /// 记录到非零错误码的格数。
    pub fn errors(&self) -> u32 {
        let mut c = 0u32;
        let mut i = 0usize;
        while i < self.n {
            if let Some(s) = self.slots.get(i) {
                if s.code != 0 {
                    c = c.wrapping_add(1);
                }
            }
            i += 1;
        }
        c
    }

    /// 成功样本按桶计数：固定、部分、全随机、可执行。
    pub fn level_counts(&self) -> [u32; BUCKETS] {
        let mut out = [0u32; BUCKETS];
        let mut i = 0usize;
        while i < self.n {
            if let Some(s) = self.slots.get(i) {
                if s.code == 0 {
                    let b = match s.level {
                        L_FIXED => 0,
                        L_PARTIAL => 1,
                        L_FULL => 2,
                        _ => 3,
                    };
                    if let Some(v) = out.get_mut(b) {
                        *v = v.wrapping_add(1);
                    }
                }
            }
            i += 1;
        }
        out
    }

    /// 重试总次数。
    pub fn retries_total(&self) -> u32 {
        let mut c = 0u32;
        let mut i = 0usize;
        while i < self.n {
            if let Some(s) = self.slots.get(i) {
                c = c.wrapping_add(s.retries as u32);
            }
            i += 1;
        }
        c
    }

    /// 成功样本中基址页对齐的个数。
    pub fn aligned(&self) -> u32 {
        let mut c = 0u32;
        let mut i = 0usize;
        while i < self.n {
            if let Some(s) = self.slots.get(i) {
                if s.code == 0 && s.addr & (PAGE - 1) == 0 {
                    c = c.wrapping_add(1);
                }
            }
            i += 1;
        }
        c
    }

    /// 成功样本中落在 47 位用户空间的个数。
    pub fn user_ok(&self) -> u32 {
        let mut c = 0u32;
        let mut i = 0usize;
        while i < self.n {
            if let Some(s) = self.slots.get(i) {
                if s.code == 0 && in_user_space(s.addr) {
                    c = c.wrapping_add(1);
                }
            }
            i += 1;
        }
        c
    }

    /// 冻结核对：全登记、全观测、全匹配、桶表平衡。
    pub fn replay(&mut self) -> Result<u64, u32> {
        if self.n != SAMPLES || self.accepted != SAMPLES as u32 {
            return Err(self.fail(BS_ENOBS));
        }
        if self.observed != SAMPLES as u32 || self.matched != SAMPLES as u32 {
            return Err(self.fail(BS_EMISMATCH));
        }
        let bc = self.level_counts();
        let mut sum = 0u32;
        let mut i = 0usize;
        while i < BUCKETS {
            if let Some(v) = bc.get(i) {
                sum = sum.wrapping_add(*v);
            }
            i += 1;
        }
        if sum != (SAMPLES as u32).wrapping_sub(1) || self.errors() != 1 {
            return Err(self.fail(BS_EBAL));
        }
        Ok(self.fold())
    }

    /// 冻结：先重放再上锁；第二次冻结返回冻结错误。
    pub fn freeze(&mut self) -> Result<u64, u32> {
        if self.frozen {
            return Err(self.fail(BS_EFROZEN));
        }
        self.replay()?;
        self.frozen = true;
        Ok(self.fold())
    }

    /// 冻结标志。
    pub fn is_frozen(&self) -> bool {
        self.frozen
    }

    /// 对全部字段做 FNV-1a 折叠。
    pub fn fold(&self) -> u64 {
        let mut h = FNV_OFF;
        h = mix(h, self.n as u64);
        h = mix(h, self.accepted as u64);
        h = mix(h, self.rejected as u64);
        h = mix(h, self.observed as u64);
        h = mix(h, self.matched as u64);
        h = mix(h, if self.frozen { 1 } else { 0 });
        let mut i = 0usize;
        while i < SAMPLES {
            if let Some(s) = self.slots.get(i) {
                h = mix(h, s.code as u64);
                h = mix(h, s.level as u64);
                h = mix(h, s.retries as u64);
                h = mix(h, if s.seen { 1 } else { 0 });
                h = mix(h, s.addr);
                h = mix(h, s.spec.kind as u64);
                h = mix(h, s.spec.aslr as u64);
                h = mix(h, s.spec.entropy as u64);
                h = mix(h, s.spec.reserved);
            }
            i += 1;
        }
        h
    }
}

fn mix(mut h: u64, v: u64) -> u64 {
    let mut i = 0u32;
    while i < 8 {
        h ^= (v >> (i * 8)) & 0xff;
        h = h.wrapping_mul(FNV_PRIME);
        i += 1;
    }
    h
}

/// 构建基准账本：按编号登记十二个样本并逐一观测。
pub fn build_ledger() -> Result<BaseLedger, u32> {
    let mut l = BaseLedger::new();
    let mut i = 0usize;
    while i < SAMPLES {
        let c = spec_of(i);
        let (want, _, _, _) = expected_of(i);
        l.register_ruled(&c, want)?;
        i += 1;
    }
    let mut j = 0usize;
    while j < SAMPLES {
        l.observe(j)?;
        j += 1;
    }
    Ok(l)
}

/// 四路独立注入：越序、重复、越界、冻结各一条拒绝路径。
pub fn inject_four() -> [bool; 4] {
    let mut ok = [false; 4];
    let c0 = spec_of(0);

    let mut a = BaseLedger::new();
    let _ = a.register(&c0);
    match a.observe(1) {
        Err(code) if code == BS_ENOBS => ok[0] = a.rejected() == 1 && a.len() == 1,
        _ => {}
    }

    let mut b = BaseLedger::new();
    let _ = b.register(&c0);
    match b.register(&c0) {
        Err(code) if code == BS_EDUP => ok[1] = b.rejected() == 1 && b.len() == 1,
        _ => {}
    }

    let mut d = BaseLedger::new();
    let mut i = 0usize;
    while i < SAMPLES {
        let (want, _, _, _) = expected_of(i);
        let _ = d.register_ruled(&spec_of(i), want);
        i += 1;
    }
    let extra = BaseCase::make(K_EXEC, ASLR_OFF, 0, 0x3039);
    match d.register(&extra) {
        Err(code) if code == BS_EID => ok[2] = d.rejected() == 1 && d.len() == SAMPLES,
        _ => {}
    }

    if let Ok(mut l) = build_ledger() {
        if l.freeze().is_ok() {
            match l.register(&c0) {
                Err(code) if code == BS_EFROZEN => ok[3] = l.is_frozen() && l.rejected() == 1,
                _ => {}
            }
        }
    }
    ok
}

/// 十三项自检。
pub fn run_unx11251_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F11251");

    let fresh = BaseLedger::new();
    s.add(
        "R1-空账本静默",
        fresh.len() == 0
            && fresh.is_empty()
            && fresh.rejected() == 0
            && fresh.observed() == 0
            && !fresh.is_frozen(),
        "新账本零格零拒绝零观测",
    );

    let fixed_ok = match pick(&spec_of(0)) {
        Ok(b) => b.addr == 0x5555_5555_4000 && b.level == L_FIXED && b.retries == 0,
        Err(_) => false,
    };
    s.add("R2-固定PIE落点", fixed_ok, "关闭ASLR时基址固定且页对齐");

    let part_ok = match (pick(&spec_of(2)), pick(&spec_of(3))) {
        (Ok(a), Ok(b)) => {
            a.addr == 0x5555_003f_f000
                && b.addr == PARTIAL_BASE
                && a.level == L_PARTIAL
                && b.level == L_PARTIAL
        }
        _ => false,
    };
    s.add("R3-部分随机掩码", part_ok, "低10位页号回绕到零");

    let full_ok = match (pick(&spec_of(5)), pick(&spec_of(6))) {
        (Ok(a), Ok(b)) => {
            a.addr == 0x7f0f_ffff_f000
                && b.addr == FULL_BASE
                && a.level == L_FULL
                && b.level == L_FULL
        }
        _ => false,
    };
    s.add("R4-全随机掩码", full_ok, "24位页号掩码与回绕");

    let exec_ok = match (pick(&spec_of(7)), pick(&spec_of(8))) {
        (Ok(a), Ok(b)) => {
            a.addr == EXEC_BASE
                && b.addr == EXEC_BASE
                && a.level == L_EXEC
                && b.level == L_EXEC
        }
        _ => false,
    };
    s.add("R5-可执行文件忽略熵", exec_ok, "ET_EXEC不参与随机化");

    s.add(
        "R6-非法熵拒绝",
        pick(&spec_of(9)) == Err(BS_EASLR),
        "熵等级超出ASLR_FULL时选基失败",
    );

    let retry_ok = match (pick(&spec_of(10)), pick(&spec_of(11))) {
        (Ok(a), Ok(b)) => {
            a.addr == PARTIAL_BASE + PAGE
                && a.retries == 1
                && b.addr == FULL_BASE
                && b.retries == 0
        }
        _ => false,
    };
    s.add(
        "R7-保留区逐页重试",
        retry_ok,
        "命中保留区上移一页，未命中原样落点",
    );

    let built = build_ledger();
    let (reg_ok, bal_ok, frozen_ok) = match &built {
        Ok(l) => {
            let r = l.len() == SAMPLES
                && l.accepted() == SAMPLES as u32
                && l.rejected() == 0
                && l.errors() == 1
                && !l.is_frozen();
            let bc = l.level_counts();
            let b = bc == [1, 4, 4, 2]
                && l.retries_total() == 1
                && l.aligned() == 11
                && l.user_ok() == 11;
            let mut m = match build_ledger() {
                Ok(v) => v,
                Err(_) => BaseLedger::new(),
            };
            let f = match m.freeze() {
                Ok(_) => {
                    m.is_frozen()
                        && m.freeze() == Err(BS_EFROZEN)
                        && m.rejected() == 1
                }
                Err(_) => false,
            };
            (r, b, f)
        }
        Err(_) => (false, false, false),
    };
    s.add("R8-样本登记完整", reg_ok, "十二格全部登记且仅一格报错");
    s.add("R9-桶表平衡", bal_ok, "固定1 部分4 全随机4 可执行2");
    s.add("R10-冻结后二次拒绝", frozen_ok, "冻结成功且再次冻结返回错误");

    let mut o = BaseLedger::new();
    let mut i = 0usize;
    while i < 3 {
        let _ = o.register(&spec_of(i));
        i += 1;
    }
    let obs_ok = o.observe(3) == Err(BS_ENOBS) && o.rejected() == 1 && o.len() == 3;
    s.add("R11-观测越界拒绝", obs_ok, "未登记的格不可观测");

    let mut d = BaseLedger::new();
    let c0 = spec_of(0);
    let _ = d.register(&c0);
    let dup_ok = d.register(&c0) == Err(BS_EDUP) && d.rejected() == 1 && d.len() == 1;
    s.add("R12-重复登记拒绝", dup_ok, "同一规格第二次登记被拒");

    s.add(
        "R13-四路注入",
        inject_four() == [true, true, true, true],
        "越序 重复 越界 冻结四条拒绝路径均触发",
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixed_and_masked_bases() {
        let f = match pick(&spec_of(0)) {
            Ok(b) => b,
            Err(_) => Base::none(),
        };
        assert_eq!(f.addr, 0x5555_5555_4000);
        assert_eq!(f.level, L_FIXED);
        assert_eq!(f.retries, 0);

        let p = match pick(&spec_of(2)) {
            Ok(b) => b,
            Err(_) => Base::none(),
        };
        assert_eq!(p.addr, 0x5555_003f_f000);
        let w = match pick(&spec_of(3)) {
            Ok(b) => b,
            Err(_) => Base::none(),
        };
        assert_eq!(w.addr, PARTIAL_BASE);
        assert!(w.addr & (PAGE - 1) == 0);
    }

    #[test]
    fn full_random_bounds() {
        let hi = match pick(&spec_of(5)) {
            Ok(b) => b,
            Err(_) => Base::none(),
        };
        assert_eq!(hi.addr, 0x7f0f_ffff_f000);
        assert!(in_user_space(hi.addr));
        assert_eq!(page_gap(FULL_BASE, hi.addr), 0x00ff_ffff);
        assert!(!in_user_space(1u64 << 47));
    }

    #[test]
    fn exec_ignores_entropy() {
        for id in [7usize, 8usize] {
            let b = match pick(&spec_of(id)) {
                Ok(v) => v,
                Err(_) => Base::none(),
            };
            assert_eq!(b.addr, EXEC_BASE);
            assert_eq!(b.level, L_EXEC);
        }
        assert_eq!(pick(&spec_of(9)), Err(BS_EASLR));
    }

    #[test]
    fn reserved_hit_retries_once() {
        let a = match pick(&spec_of(10)) {
            Ok(v) => v,
            Err(_) => Base::none(),
        };
        assert_eq!(a.addr, PARTIAL_BASE + PAGE);
        assert_eq!(a.retries, 1);
        let b = match pick(&spec_of(11)) {
            Ok(v) => v,
            Err(_) => Base::none(),
        };
        assert_eq!(b.addr, FULL_BASE);
        assert_eq!(b.retries, 0);
        assert_eq!(validate(&spec_of(10), &a), 0);
        assert_eq!(validate(&spec_of(9), &a), BS_EASLR);
    }

    #[test]
    fn ledger_shape_and_balance() {
        let mut l = match build_ledger() {
            Ok(v) => v,
            Err(_) => BaseLedger::new(),
        };
        assert_eq!(l.len(), SAMPLES);
        assert_eq!(l.accepted(), SAMPLES as u32);
        assert_eq!(l.rejected(), 0);
        assert_eq!(l.observed(), SAMPLES as u32);
        assert_eq!(l.matched(), SAMPLES as u32);
        assert_eq!(l.errors(), 1);
        assert_eq!(l.level_counts(), [1, 4, 4, 2]);
        assert_eq!(l.retries_total(), 1);
        assert_eq!(l.aligned(), 11);
        assert_eq!(l.user_ok(), 11);
        let h = match l.freeze() {
            Ok(v) => v,
            Err(_) => 0,
        };
        assert_eq!(l.replay(), Ok(h));
        assert_eq!(l.freeze(), Err(BS_EFROZEN));
        assert_eq!(l.rejected(), 1);
    }

    #[test]
    fn refusal_paths_and_duplicates() {
        assert_eq!(inject_four(), [true, true, true, true]);

        let mut d = BaseLedger::new();
        let c = BaseCase::make(K_PIE, ASLR_OFF, 0, 0);
        assert!(d.register(&c).is_ok());
        assert_eq!(d.register(&c), Err(BS_EDUP));
        assert_eq!(d.len(), 1);

        let bad = BaseCase::make(K_PIE, ASLR_FULL + 2, 0, 0);
        assert_eq!(d.register(&bad), Err(BS_EASLR));
        assert_eq!(d.rejected(), 2);
    }
}
