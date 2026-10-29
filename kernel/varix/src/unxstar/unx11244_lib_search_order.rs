//! UNX-F11244 库搜索序（RPATH/LD_LIBRARY_PATH/RUNPATH/cache/默认）（0x1f2c0 段）
//!
//! 模型：DT_NEEDED 只给出 soname，真正的路径要在六级目录源里逐级查找。本模型
//! 把「某一级里该 soname 存在于哪些目录」压成 8 位位图，解析器按 glibc 的生效
//! 序逐级扫，第一级第一个命中的目录就是解析结果；六级全空则报 ENOENT。
//!
//! 规则面：① DT_RPATH 只在没有 DT_RUNPATH 时生效；② LD_LIBRARY_PATH 在
//! AT_SECURE 下整体剥离；③ DT_RUNPATH 在 AT_SECURE 下仅对受信对象生效；
//! ④ cache 先于默认目录；⑤ 默认目录先于信任目录；⑥ 位图超出 8 位判坏掩码。
//! 级间顺序固定为 RPATH → LD_LIBRARY_PATH → RUNPATH → cache → default →
//! trusted，这一顺序就是判据要比对的主轴。
//!
//! 账本：每样本一条 slot（soname + 六级位图 + 是否带 RUNPATH + secure/trusted
//! 标记 + 命中级别/目录），另按命中级别分桶、记未命中数。fold 卷全账；
//! replay 重算每一级并要求观测覆盖、级直方图合计等于样本数；freeze 后写拒。
//!
//! 拒止面：样本号越界、跳号登记、重号登记、坏目录掩码、观测未登记、冻结后
//! 写入、未观测即重放、空账重放、指纹漂移、分桶不平衡。

use crate::alloc::format;
use crate::checks::CheckSet;

// ---------------------------------------------------------------- 错误码
const SEG_BASE: u32 = 0x1f2c0;

/// 样本号越界
pub const SL_EID: u32 = SEG_BASE + 0x01;
/// 登记序违例 / 观测目标未登记
pub const SL_EORDER: u32 = SEG_BASE + 0x02;
/// 重号登记 / 重号观测
pub const SL_EDUP: u32 = SEG_BASE + 0x03;
/// 冻结后写入
pub const SL_EFROZEN: u32 = SEG_BASE + 0x04;
/// 未观测即重放
pub const SL_ENOBS: u32 = SEG_BASE + 0x05;
/// 重放指纹不一致
pub const SL_EMISMATCH: u32 = SEG_BASE + 0x06;
/// 分桶不平衡
pub const SL_EBAL: u32 = SEG_BASE + 0x07;
/// 空账重放
pub const SL_EEMPTY: u32 = SEG_BASE + 0x08;
/// 六级全未命中
pub const SL_ENOENT: u32 = SEG_BASE + 0x09;
/// 目录位图超出 8 位
pub const SL_EBADMASK: u32 = SEG_BASE + 0x0a;

// ---------------------------------------------------------------- 域常量
/// 搜索级数
pub const LEVELS: usize = 6;
/// 每级目录位宽
pub const DIRS: usize = 8;
/// 合法目录掩码
pub const DIR_MASK: u16 = 0xff;
/// 级：DT_RPATH
pub const LV_RPATH: usize = 0;
/// 级：LD_LIBRARY_PATH
pub const LV_LDPATH: usize = 1;
/// 级：DT_RUNPATH
pub const LV_RUNPATH: usize = 2;
/// 级：ld.so.cache
pub const LV_CACHE: usize = 3;
/// 级：默认目录（/lib、/usr/lib）
pub const LV_DEFAULT: usize = 4;
/// 级：信任目录（secure 兜底）
pub const LV_TRUSTED: usize = 5;
/// 级名（诊断与逐级判据共用）
pub const LEVEL_NAMES: [&str; LEVELS] =
    ["RPATH", "LD_LIBRARY_PATH", "RUNPATH", "cache", "default", "trusted"];
/// 样本总数（判据要求 30 样本）
pub const SAMPLES: usize = 30;
/// 原型数
pub const ARCHETYPES: usize = 10;
/// 每原型变体数
pub const VARIANTS: usize = 3;
/// 无命中标记
pub const NO_DIR: u8 = 0xff;
/// ENOENT
pub const ENOENT: u32 = 2;
/// EACCES
pub const EACCES: u32 = 13;

const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x100_0000_01b3;

/// 一次解析的落点：级别 + 目录下标。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Hit {
    pub level: u8,
    pub dir: u8,
}

// ---------------------------------------------------------------- 样本
/// 一个搜索样本：soname + 六级目录位图 + 对象属性。
#[derive(Clone, Copy, Debug)]
pub struct LibCase {
    pub soname: u16,
    masks: [u16; LEVELS],
    pub runpath_present: bool,
    pub secure: bool,
    pub trusted: bool,
}

impl LibCase {
    /// 空样本。
    pub const fn blank() -> LibCase {
        LibCase {
            soname: 0,
            masks: [0; LEVELS],
            runpath_present: false,
            secure: false,
            trusted: false,
        }
    }

    /// 构造样本。
    pub const fn new(soname: u16, runpath_present: bool, secure: bool, trusted: bool) -> LibCase {
        LibCase { soname, masks: [0; LEVELS], runpath_present, secure, trusted }
    }

    /// 在次级 level 的 dir 目录登记一次命中。
    pub fn set_hit(&mut self, level: usize, dir: usize) -> bool {
        if level >= LEVELS || dir >= DIRS {
            return false;
        }
        match self.masks.get_mut(level) {
            Some(m) => {
                *m |= 1u16 << dir;
                true
            }
            None => false,
        }
    }

    /// 取某级位图。
    pub fn mask(&self, level: usize) -> u16 {
        match self.masks.get(level) {
            Some(v) => *v,
            None => 0,
        }
    }

    /// 某级目录号（最低置位），未命中给 NO_DIR。
    pub fn first_dir(&self, level: usize) -> u8 {
        let m = self.mask(level);
        let mut d = 0usize;
        while d < DIRS {
            if m & (1u16 << d) != 0 {
                return d as u8;
            }
            d = d.wrapping_add(1);
        }
        NO_DIR
    }
}

// ---------------------------------------------------------------- 解析规则
/// 按生效序逐级查找：返回首个命中（级别 + 目录），全空报 ENOENT。
pub fn resolve(c: &LibCase) -> Result<Hit, u32> {
    let mut lv = 0usize;
    while lv < LEVELS {
        if c.mask(lv) & !DIR_MASK != 0 {
            return Err(SL_EBADMASK);
        }
        lv = lv.wrapping_add(1);
    }
    let mut order = [NO_DIR; LEVELS];
    let mut on = 0usize;
    if !c.runpath_present && (!c.secure || c.trusted) {
        order[on] = LV_RPATH as u8;
        on = on.wrapping_add(1);
    }
    if !c.secure {
        order[on] = LV_LDPATH as u8;
        on = on.wrapping_add(1);
    }
    if !c.secure || c.trusted {
        order[on] = LV_RUNPATH as u8;
        on = on.wrapping_add(1);
    }
    order[on] = LV_CACHE as u8;
    on = on.wrapping_add(1);
    order[on] = LV_DEFAULT as u8;
    on = on.wrapping_add(1);
    order[on] = LV_TRUSTED as u8;
    on = on.wrapping_add(1);
    let mut k = 0usize;
    while k < on {
        let l = match order.get(k) {
            Some(v) => *v as usize,
            None => return Err(SL_EORDER),
        };
        let m = c.mask(l);
        if m != 0 {
            let dir = c.first_dir(l);
            if dir == NO_DIR {
                return Err(SL_EBADMASK);
            }
            return Ok(Hit { level: l as u8, dir });
        }
        k = k.wrapping_add(1);
    }
    Err(SL_ENOENT)
}

/// 生效级别序（诊断用）：把被 RPATH 遮蔽/secure 剥离后的级别列出来。
pub fn effective_order(c: &LibCase) -> ([u8; LEVELS], usize) {
    let mut out = [NO_DIR; LEVELS];
    let mut n = 0usize;
    if !c.runpath_present && (!c.secure || c.trusted) {
        out[n] = LV_RPATH as u8;
        n = n.wrapping_add(1);
    }
    if !c.secure {
        out[n] = LV_LDPATH as u8;
        n = n.wrapping_add(1);
    }
    if !c.secure || c.trusted {
        out[n] = LV_RUNPATH as u8;
        n = n.wrapping_add(1);
    }
    out[n] = LV_CACHE as u8;
    n = n.wrapping_add(1);
    out[n] = LV_DEFAULT as u8;
    n = n.wrapping_add(1);
    out[n] = LV_TRUSTED as u8;
    n = n.wrapping_add(1);
    (out, n)
}

// ---------------------------------------------------------------- 账本
/// 一条搜索账。
#[derive(Clone, Copy, Debug)]
pub struct Slot {
    pub case: LibCase,
    pub level: u8,
    pub dir: u8,
    pub verdict: u8,
    pub observed: bool,
}

impl Slot {
    /// 空账位。
    pub const fn blank() -> Slot {
        Slot { case: LibCase::blank(), level: NO_DIR, dir: NO_DIR, verdict: 0, observed: false }
    }

    /// 本样本是否解析成功。
    pub fn hit(&self) -> bool {
        self.verdict == 0
    }
}

/// 六级搜索序账本。
#[derive(Clone, Debug)]
pub struct OrderLedger {
    slots: [Option<Slot>; SAMPLES],
    n: usize,
    level_hist: [u32; LEVELS],
    misses: u32,
    observed: u32,
    accepts: u64,
    rejects: u64,
    root: Option<u64>,
    frozen: bool,
}

impl OrderLedger {
    /// 空账。
    pub const fn new() -> OrderLedger {
        OrderLedger {
            slots: [None; SAMPLES],
            n: 0,
            level_hist: [0; LEVELS],
            misses: 0,
            observed: 0,
            accepts: 0,
            rejects: 0,
            root: None,
            frozen: false,
        }
    }

    /// 记一次拒止。
    fn fail(&mut self, code: u32) -> u32 {
        self.rejects = self.rejects.wrapping_add(1);
        code
    }

    /// 已登样本数。
    pub fn len(&self) -> usize {
        self.n
    }

    /// 某级命中计数。
    pub fn level_bucket(&self, lv: usize) -> u32 {
        match self.level_hist.get(lv) {
            Some(v) => *v,
            None => 0,
        }
    }

    /// 未命中数。
    pub fn misses(&self) -> u32 {
        self.misses
    }

    /// 已观测条数。
    pub fn observed(&self) -> u32 {
        self.observed
    }

    /// 取一条账。
    pub fn slot(&self, i: usize) -> Option<Slot> {
        match self.slots.get(i) {
            Some(v) => *v,
            None => None,
        }
    }

    /// 定锚指纹。
    pub fn root_value(&self) -> Option<u64> {
        self.root
    }

    /// 是否已冻结。
    pub fn is_frozen(&self) -> bool {
        self.frozen
    }
}

impl OrderLedger {
    /// 登记一个样本：冻结 → 越界 → 回填 → 跳号，四道门后跑搜索规则。
    pub fn register(&mut self, id: usize) -> Result<u64, u32> {
        if self.frozen {
            return Err(self.fail(SL_EFROZEN));
        }
        if id >= SAMPLES {
            return Err(self.fail(SL_EID));
        }
        if id < self.n {
            return Err(self.fail(SL_EDUP));
        }
        if id > self.n {
            return Err(self.fail(SL_EORDER));
        }
        let case = spec_of(id);
        let mut slot = Slot::blank();
        slot.case = case;
        match resolve(&case) {
            Ok(h) => {
                slot.level = h.level;
                slot.dir = h.dir;
                if let Some(c) = self.level_hist.get_mut(h.level as usize) {
                    *c = c.wrapping_add(1);
                }
            }
            Err(code) => {
                slot.verdict = (code - SEG_BASE) as u8;
                if code == SL_ENOENT {
                    self.misses = self.misses.wrapping_add(1);
                }
            }
        }
        let at = self.n;
        if let Some(cell) = self.slots.get_mut(at) {
            *cell = Some(slot);
        }
        self.n = self.n.wrapping_add(1);
        self.accepts = self.accepts.wrapping_add(1);
        Ok(self.fold())
    }

    /// 观测一条账。
    pub fn observe(&mut self, id: usize) -> Result<(), u32> {
        if self.frozen {
            return Err(self.fail(SL_EFROZEN));
        }
        if id >= self.n {
            return Err(self.fail(SL_EORDER));
        }
        let cell = match self.slots.get(id) {
            Some(v) => *v,
            None => None,
        };
        match cell {
            Some(mut s) => {
                if s.observed {
                    return Err(self.fail(SL_EDUP));
                }
                s.observed = true;
                if let Some(dst) = self.slots.get_mut(id) {
                    *dst = Some(s);
                }
                self.observed = self.observed.wrapping_add(1);
                Ok(())
            }
            None => Err(self.fail(SL_EORDER)),
        }
    }

    /// 全账指纹。
    pub fn fold(&self) -> u64 {
        let mut h = FNV_OFF;
        let mut i = 0usize;
        while i < self.n {
            if let Some(Some(s)) = self.slots.get(i) {
                h = (h ^ s.case.soname as u64).wrapping_mul(FNV_PRIME);
                let mut lv = 0usize;
                while lv < LEVELS {
                    h = (h ^ s.case.mask(lv) as u64).wrapping_mul(FNV_PRIME);
                    lv = lv.wrapping_add(1);
                }
                h = (h ^ s.case.runpath_present as u64).wrapping_mul(FNV_PRIME);
                h = (h ^ s.case.secure as u64).wrapping_mul(FNV_PRIME);
                h = (h ^ s.case.trusted as u64).wrapping_mul(FNV_PRIME);
                h = (h ^ s.level as u64).wrapping_mul(FNV_PRIME);
                h = (h ^ s.dir as u64).wrapping_mul(FNV_PRIME);
                h = (h ^ s.verdict as u64).wrapping_mul(FNV_PRIME);
                h = (h ^ s.observed as u64).wrapping_mul(FNV_PRIME);
            }
            i = i.wrapping_add(1);
        }
        let mut lv = 0usize;
        while lv < LEVELS {
            if let Some(v) = self.level_hist.get(lv) {
                h = (h ^ *v as u64).wrapping_mul(FNV_PRIME);
            }
            lv = lv.wrapping_add(1);
        }
        h = (h ^ self.n as u64).wrapping_mul(FNV_PRIME);
        h = (h ^ self.misses as u64).wrapping_mul(FNV_PRIME);
        h = (h ^ self.accepts).wrapping_mul(FNV_PRIME);
        h = (h ^ self.rejects).wrapping_mul(FNV_PRIME);
        h
    }
}

impl OrderLedger {
    /// 重放：观测覆盖、逐条重算、级桶合计 + 未命中 = 样本数、指纹比对。
    pub fn replay(&mut self) -> Result<u64, u32> {
        if self.n == 0 {
            return Err(self.fail(SL_EEMPTY));
        }
        if self.observed as usize != self.n {
            return Err(self.fail(SL_ENOBS));
        }
        let mut i = 0usize;
        let mut hist_sum = 0u32;
        while i < self.n {
            let s = match self.slots.get(i).copied() {
                Some(Some(v)) => v,
                _ => return Err(self.fail(SL_EORDER)),
            };
            let again = resolve(&s.case);
            let coherent = match again {
                Ok(h) => s.verdict == 0 && h.level == s.level && h.dir == s.dir,
                Err(code) => (code - SEG_BASE) as u8 == s.verdict,
            };
            if !coherent {
                return Err(self.fail(SL_EMISMATCH));
            }
            if s.verdict == 0 {
                hist_sum = hist_sum.wrapping_add(1);
            }
            i = i.wrapping_add(1);
        }
        let mut bucket_sum = 0u32;
        let mut lv = 0usize;
        while lv < LEVELS {
            if let Some(v) = self.level_hist.get(lv) {
                bucket_sum = bucket_sum.wrapping_add(*v);
            }
            lv = lv.wrapping_add(1);
        }
        if bucket_sum != hist_sum || bucket_sum.wrapping_add(self.misses) != self.n as u32 {
            return Err(self.fail(SL_EBAL));
        }
        let v = self.fold();
        match self.root {
            Some(r) if r != v => Err(self.fail(SL_EMISMATCH)),
            _ => Ok(v),
        }
    }

    /// 先重放再封账；二次冻结拒。
    pub fn freeze(&mut self) -> Result<u64, u32> {
        if self.frozen {
            return Err(self.fail(SL_EFROZEN));
        }
        let v = self.replay()?;
        self.root = Some(v);
        self.frozen = true;
        Ok(v)
    }
}

// ---------------------------------------------------------------- 样本表
/// 原型：rpath / ldpath / runpath / cache / default / trusted 的基准目录
/// （NO_DIR 表示该级无命中），末两位为 runpath_present 与 secure。
pub const ARCH: [(u8, u8, u8, u8, u8, u8, bool, bool); ARCHETYPES] = [
    (0, NO_DIR, NO_DIR, NO_DIR, NO_DIR, NO_DIR, false, false),
    (0, 3, NO_DIR, NO_DIR, NO_DIR, NO_DIR, false, false),
    (0, NO_DIR, 1, NO_DIR, NO_DIR, NO_DIR, true, false),
    (NO_DIR, 3, NO_DIR, NO_DIR, NO_DIR, NO_DIR, false, false),
    (NO_DIR, NO_DIR, NO_DIR, 2, NO_DIR, NO_DIR, false, false),
    (NO_DIR, NO_DIR, NO_DIR, NO_DIR, 5, NO_DIR, false, false),
    (NO_DIR, NO_DIR, NO_DIR, NO_DIR, NO_DIR, 0, false, true),
    (NO_DIR, 3, NO_DIR, 2, NO_DIR, NO_DIR, false, true),
    (NO_DIR, NO_DIR, NO_DIR, NO_DIR, NO_DIR, NO_DIR, false, false),
    (4, NO_DIR, NO_DIR, NO_DIR, NO_DIR, NO_DIR, false, false),
];

/// 原型期望命中级别（NO_DIR = 全未命中）。与解析器相互独立，用作比对基准。
pub const LEVEL_OF_ARCH: [u8; ARCHETYPES] =
    [LV_RPATH as u8, LV_RPATH as u8, LV_RUNPATH as u8, LV_LDPATH as u8, LV_CACHE as u8,
     LV_DEFAULT as u8, LV_TRUSTED as u8, LV_CACHE as u8, NO_DIR, LV_RPATH as u8];

/// 由样本号构造样本：原型 × 变体，变体把目录号整体平移。
pub fn spec_of(id: usize) -> LibCase {
    if id >= SAMPLES {
        return LibCase::blank();
    }
    let arch = id % ARCHETYPES;
    let var = id / ARCHETYPES;
    let a = match ARCH.get(arch) {
        Some(v) => *v,
        None => return LibCase::blank(),
    };
    let secure = a.7;
    let mut c = LibCase::new(300u16.wrapping_add(id as u16), a.6, secure, secure);
    let bases = [a.0, a.1, a.2, a.3, a.4, a.5];
    let mut lv = 0usize;
    while lv < LEVELS {
        let base = match bases.get(lv) {
            Some(v) => *v,
            None => NO_DIR,
        };
        if base != NO_DIR {
            let dir = (base as usize + var) % DIRS;
            let _ = c.set_hit(lv, dir);
        }
        lv = lv.wrapping_add(1);
    }
    c
}

/// 独立期望：解析器应当落到的（级别，目录）。
pub fn expected_of(id: usize) -> (u8, u8) {
    if id >= SAMPLES {
        return (NO_DIR, NO_DIR);
    }
    let arch = id % ARCHETYPES;
    let var = (id / ARCHETYPES) as u8;
    let lv = match LEVEL_OF_ARCH.get(arch) {
        Some(v) => *v,
        None => NO_DIR,
    };
    if lv == NO_DIR {
        return (NO_DIR, NO_DIR);
    }
    let a = match ARCH.get(arch) {
        Some(v) => *v,
        None => return (NO_DIR, NO_DIR),
    };
    let base = match lv as usize {
        0 => a.0,
        1 => a.1,
        2 => a.2,
        3 => a.3,
        4 => a.4,
        _ => a.5,
    };
    if base == NO_DIR {
        return (NO_DIR, NO_DIR);
    }
    (lv, (base + var) % (DIRS as u8))
}

/// 建全账：30 样本登记 → 全观测 → 封账。
pub fn build_ledger() -> OrderLedger {
    let mut l = OrderLedger::new();
    let mut i = 0usize;
    while i < SAMPLES {
        let _ = l.register(i);
        i = i.wrapping_add(1);
    }
    let mut k = 0usize;
    while k < l.len() {
        let _ = l.observe(k);
        k = k.wrapping_add(1);
    }
    let _ = l.freeze();
    l
}

/// 四路独立拒止：登记序 / 重号 / 越界样本号 / 冻结后登记。
pub fn inject_four() -> [bool; 4] {
    let mut a = OrderLedger::new();
    let order = a.register(1).err() == Some(SL_EORDER) && a.len() == 0 && a.rejects == 1;

    let mut b = OrderLedger::new();
    let first = b.register(0).is_ok();
    let dup = b.register(0).err() == Some(SL_EDUP) && b.len() == 1;

    let mut c = OrderLedger::new();
    let range = c.register(SAMPLES).err() == Some(SL_EID) && c.len() == 0;

    let mut d = build_ledger();
    let frozen = d.register(0).err() == Some(SL_EFROZEN) && d.is_frozen();

    [order, first && dup, range, frozen]
}

// ---------------------------------------------------------------- 自检
/// 判据：UNX-F11244 六级搜索序与 30 样本解析路径。
pub fn run_unx11244_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F11244");

    // S1 六级定序
    s.add(
        "S1-六级序定名",
        LEVELS == 6
            && LEVEL_NAMES[0] == "RPATH"
            && LEVEL_NAMES[1] == "LD_LIBRARY_PATH"
            && LEVEL_NAMES[2] == "RUNPATH"
            && LEVEL_NAMES[3] == "cache"
            && LEVEL_NAMES[4] == "default"
            && LEVEL_NAMES[5] == "trusted",
        "六级名与下标一一对应",
    );

    // S2 RPATH 压过 LD_LIBRARY_PATH
    let h1 = resolve(&spec_of(1));
    s.add_dyn(
        "S2-RPATH先于LD_LIBRARY_PATH",
        h1 == Ok(Hit { level: LV_RPATH as u8, dir: 0 }),
        &format!("hit={:?}", h1),
    );

    // S3 RUNPATH 遮蔽 RPATH
    let c2 = spec_of(2);
    let (eff2, n2) = effective_order(&c2);
    s.add_dyn(
        "S3-RUNPATH遮蔽RPATH",
        resolve(&c2) == Ok(Hit { level: LV_RUNPATH as u8, dir: 1 })
            && eff2[0] == LV_LDPATH as u8
            && eff2[1] == LV_RUNPATH as u8
            && n2 == 5,
        &format!("levels={:?} n={}", eff2, n2),
    );

    // S4 LD_LIBRARY_PATH 级
    s.add(
        "S4-LD_LIBRARY_PATH级",
        resolve(&spec_of(3)) == Ok(Hit { level: LV_LDPATH as u8, dir: 3 }),
        "仅 LD_LIBRARY_PATH 命中时落第 2 级",
    );

    // S5 cache 先于默认目录
    let mut both = LibCase::new(555, false, false, false);
    let _ = both.set_hit(LV_CACHE, 4);
    let _ = both.set_hit(LV_DEFAULT, 1);
    s.add(
        "S5-cache先于默认目录",
        resolve(&spec_of(4)) == Ok(Hit { level: LV_CACHE as u8, dir: 2 })
            && resolve(&both) == Ok(Hit { level: LV_CACHE as u8, dir: 4 }),
        "cache 与默认同时可命中时取 cache",
    );

    // S6 默认目录
    s.add(
        "S6-默认目录级",
        resolve(&spec_of(5)) == Ok(Hit { level: LV_DEFAULT as u8, dir: 5 }),
        "只有 /lib 命中时落第 5 级",
    );

    // S7 信任目录兜底
    s.add(
        "S7-信任目录兜底",
        resolve(&spec_of(6)) == Ok(Hit { level: LV_TRUSTED as u8, dir: 0 }),
        "secure 对象落 trusted 兜底目录",
    );

    // S8 secure 剥离 LD_LIBRARY_PATH
    let mut sec = LibCase::new(777, true, true, false);
    let _ = sec.set_hit(LV_RUNPATH, 1);
    let _ = sec.set_hit(LV_CACHE, 2);
    s.add(
        "S8-secure剥离LD_LIBRARY_PATH",
        resolve(&spec_of(7)) == Ok(Hit { level: LV_CACHE as u8, dir: 2 })
            && resolve(&sec) == Ok(Hit { level: LV_CACHE as u8, dir: 2 }),
        "secure 下 LD_PATH 与未受信对象的 RUNPATH 都不参与",
    );

    // S9 30 样本解析路径全对
    let mut wrong = 0u32;
    let mut i = 0usize;
    while i < SAMPLES {
        let want = expected_of(i);
        let got = resolve(&spec_of(i));
        let ok = match got {
            Ok(h) => (h.level, h.dir) == want,
            Err(_) => want.0 == NO_DIR,
        };
        if !ok {
            wrong = wrong.wrapping_add(1);
        }
        i = i.wrapping_add(1);
    }
    s.add_dyn("S9-30样本路径全对", wrong == 0 && SAMPLES == 30, &format!("wrong={}", wrong));

    // S10 未命中路径
    s.add(
        "S10-全未命中",
        resolve(&spec_of(8)).err() == Some(SL_ENOENT)
            && resolve(&spec_of(28)).err() == Some(SL_ENOENT),
        "六级全空报 ENOENT",
    );

    // S11 级直方图与未命中平衡
    let l = build_ledger();
    let sum = l.level_bucket(0)
        .wrapping_add(l.level_bucket(1))
        .wrapping_add(l.level_bucket(2))
        .wrapping_add(l.level_bucket(3))
        .wrapping_add(l.level_bucket(4))
        .wrapping_add(l.level_bucket(5));
    s.add_dyn(
        "S11-级直方图平衡",
        l.level_bucket(0) == 9
            && l.level_bucket(1) == 3
            && l.level_bucket(2) == 3
            && l.level_bucket(3) == 6
            && l.level_bucket(4) == 3
            && l.level_bucket(5) == 3
            && l.misses() == 3
            && sum + l.misses() == SAMPLES as u32,
        &format!("sum={} misses={}", sum, l.misses()),
    );

    // S12 登记序 / 重号 / 越界
    let inj = inject_four();
    s.add(
        "S12-登记序重号越界拒止",
        inj[0] && inj[1] && inj[2],
        "跳号、重号、越界样本号一律拒",
    );

    // S13 重放 + 冻结 + 冻结后写入
    let mut r = l.clone();
    let rp = r.replay();
    let twice = r.freeze().err() == Some(SL_EFROZEN);
    s.add_dyn(
        "S13-重放与冻结门",
        rp.is_ok() && l.root_value().is_some() && twice && inj[3] && l.observed() == SAMPLES as u32,
        &format!("observed={} twice={}", l.observed(), twice),
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f11244_level_precedence() {
        assert_eq!(resolve(&spec_of(1)), Ok(Hit { level: 0, dir: 0 }));
        assert_eq!(resolve(&spec_of(2)), Ok(Hit { level: 2, dir: 1 }));
        assert_eq!(resolve(&spec_of(3)), Ok(Hit { level: 1, dir: 3 }));
        assert_eq!(resolve(&spec_of(4)), Ok(Hit { level: 3, dir: 2 }));
        assert_eq!(resolve(&spec_of(5)), Ok(Hit { level: 4, dir: 5 }));
    }

    #[test]
    fn f11244_secure_strips() {
        let c = spec_of(7);
        assert!(c.secure);
        assert_eq!(resolve(&c), Ok(Hit { level: 3, dir: 2 }));
        let mut u = LibCase::new(9, true, true, false);
        let _ = u.set_hit(LV_RUNPATH, 1);
        let _ = u.set_hit(LV_DEFAULT, 6);
        assert_eq!(resolve(&u), Ok(Hit { level: 4, dir: 6 }));
    }

    #[test]
    fn f11244_thirty_samples() {
        let mut i = 0usize;
        while i < SAMPLES {
            let want = expected_of(i);
            match resolve(&spec_of(i)) {
                Ok(h) => {
                    assert_eq!((h.level, h.dir), want);
                }
                Err(code) => {
                    assert_eq!(code, SL_ENOENT);
                    assert_eq!(want.0, NO_DIR);
                }
            }
            i += 1;
        }
    }

    #[test]
    fn f11244_ledger_balance() {
        let l = build_ledger();
        assert_eq!(l.len(), SAMPLES);
        assert_eq!(l.misses(), 3);
        assert_eq!(l.level_bucket(3), 6);
        let root = l.root_value();
        assert!(root.is_some());
        let mut f = l.clone();
        assert_eq!(f.replay().ok(), root);
        assert_eq!(f.freeze().err(), Some(SL_EFROZEN));
    }

    #[test]
    fn f11244_checks_green() {
        let cs = run_unx11244_checks();
        assert_eq!(cs.len(), 13);
        assert!(cs.all_passed());
    }
}
