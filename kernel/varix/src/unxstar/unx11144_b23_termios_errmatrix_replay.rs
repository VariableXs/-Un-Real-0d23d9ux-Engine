//! UNX-F11144 B23 清账（pty/termios 错误矩阵判据终重放·四格账）（0x1f2c0 段）
//!
//! 纯功能模型：termios 的属性面天然分四格——输入 c_iflag、输出 c_oflag、
//! 控制 c_cflag、本地 c_lflag。pty 侧的八类调用（openpt / grantpt /
//! unlockpt / ptsname / tcgetattr / tcsetattr / tcflush / tcsendbreak）在
//! 每一格上各有自己的合法性边界，因此账本也按四格分行：每格五例，合计
//! 二十例。四格全部闭合（登记齐、实测齐、逐例相符）才算终重放通过。
//!
//! 规则面（对齐 termios(3) 的裁剪版）：
//!   - openpt / grantpt / unlockpt / ptsname 不触及 termios，只在第 0 格合法，
//!     其余三格一律 EINVAL；
//!   - tcgetattr 无副作用，四格皆可成功；
//!   - tcsetattr 在本地格（第 3 格）遇第 5 例（越界序号）落 ENOTTY，其余成功；
//!   - tcflush 的队列选择必须小于三（TCIFLUSH / TCOFLUSH / TCIOFLUSH），
//!     序号 3、4 越界即 EINVAL；
//!   - tcsendbreak 的时长只接受 0 与 1 两档，序号 4 越界即 EINVAL。
//!
//! 拒止面：例号越界、重复登记、例序倒挂、期望码越界、未登记即观测、
//! 缺实测、不符、冻结后再动账、格账不齐。

use crate::alloc::format;
use crate::checks::CheckSet;

// ---------------------------------------------------------------- 错误码
const SEG_BASE: u32 = 0x1f2c0;
/// 例号越界
pub const TM_ECASE: u32 = SEG_BASE + 0x01;
/// 重复登记
pub const TM_EDUP: u32 = SEG_BASE + 0x02;
/// 例序倒挂
pub const TM_EORDER: u32 = SEG_BASE + 0x03;
/// 期望码越界
pub const TM_ENOEXP: u32 = SEG_BASE + 0x04;
/// 未登记即观测
pub const TM_EUNREG: u32 = SEG_BASE + 0x05;
/// 未测即重放
pub const TM_ESHORT: u32 = SEG_BASE + 0x06;
/// 实测与期望不符
pub const TM_EMISMATCH: u32 = SEG_BASE + 0x07;
/// 格账不齐
pub const TM_EQUAD: u32 = SEG_BASE + 0x08;
/// 已封盘
pub const TM_EFROZEN: u32 = SEG_BASE + 0x09;

/// 四格：iflag / oflag / cflag / lflag。
pub const QUADS: usize = 4;
/// 格名。
pub const QUAD_NAMES: [&str; QUADS] = ["iflag", "oflag", "cflag", "lflag"];
/// 每格例数。
pub const PER_QUAD: usize = 5;
/// 总例数。
pub const CASES: usize = QUADS * PER_QUAD;
/// 调用族数。
pub const OPS: usize = 8;
/// 族名。
pub const OP_NAMES: [&str; OPS] = [
    "openpt",
    "grantpt",
    "unlockpt",
    "ptsname",
    "tcgetattr",
    "tcsetattr",
    "tcflush",
    "tcsendbreak",
];
/// 队列选择上界（含）。
pub const FLUSH_MAX: u8 = 2;
/// errno 上界。
pub const ERRNO_MAX: u16 = 120;
/// ENOTTY。
pub const ENOTTY: u32 = 25;
/// EINVAL。
pub const EINVAL: u32 = 22;

const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x100_0000_01b3;

const OP_OPENPT: u8 = 0;
const OP_TCGETATTR: u8 = 4;
const OP_TCSETATTR: u8 = 5;
const OP_TCFLUSH: u8 = 6;
const OP_TCSENDBREAK: u8 = 7;

/// 单例：调用、所属格、格内序号。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CellSpec {
    pub op: u8,
    pub quad: u8,
    pub k: u8,
}

/// 规则纯函数：由（调用, 格, 格内序号）给出应有 errno。
pub fn rule_error(spec: CellSpec) -> u32 {
    if spec.op >= OPS as u8 {
        return EINVAL;
    }
    if spec.quad as usize >= QUADS || spec.k as usize >= PER_QUAD {
        return EINVAL;
    }
    match spec.op {
        // 不触及 termios 的四族：只有第 0 格可走。
        OP_OPENPT..=3 => {
            if spec.quad == 0 {
                0
            } else {
                EINVAL
            }
        }
        OP_TCGETATTR => 0,
        OP_TCSETATTR => {
            if spec.quad == 3 && spec.k == 4 {
                ENOTTY
            } else {
                0
            }
        }
        OP_TCFLUSH => {
            if spec.k > FLUSH_MAX {
                EINVAL
            } else {
                0
            }
        }
        OP_TCSENDBREAK => {
            if spec.k == 4 {
                EINVAL
            } else {
                0
            }
        }
        _ => EINVAL,
    }
}

impl CellSpec {
    /// 该例是否落在应成功一侧。
    pub fn expects_success(&self) -> bool {
        rule_error(*self) == 0
    }

    /// 例号：格在前、格内序号在后。
    pub fn id(&self) -> usize {
        (self.quad as usize).wrapping_mul(PER_QUAD).wrapping_add(self.k as usize)
    }
}

/// 四格账中的一例。
#[derive(Clone, Copy, Debug)]
pub struct Cell {
    pub spec: CellSpec,
    pub expected: u32,
    pub got: u32,
    pub seen: bool,
    pub live: bool,
}

impl Cell {
    const EMPTY: Cell = Cell {
        spec: CellSpec { op: 0, quad: 0, k: 0 },
        expected: 0,
        got: 0,
        seen: false,
        live: false,
    };

    /// 已测且相符。
    pub fn agrees(&self) -> bool {
        self.live && self.seen && self.got == self.expected
    }

    /// 登记期望与规则复算一致。
    pub fn rule_agrees(&self) -> bool {
        self.live && rule_error(self.spec) == self.expected
    }
}

/// termios 四格错误账。
#[derive(Clone, Debug)]
pub struct TermLedger {
    pub base: u32,
    cells: [Cell; CASES],
    registered: usize,
    observed: usize,
    cursor: usize,
    rejects: u32,
    frozen: bool,
}

impl TermLedger {
    /// 新建空账。
    pub fn new(base: u32) -> TermLedger {
        TermLedger {
            base,
            cells: [Cell::EMPTY; CASES],
            registered: 0,
            observed: 0,
            cursor: 0,
            rejects: 0,
            frozen: false,
        }
    }

    fn fail(&mut self, code: u32) -> u32 {
        self.rejects = self.rejects.wrapping_add(1);
        code
    }

    /// 登记一例：例号 = 格 * 5 + 格内序号。
    pub fn register(&mut self, spec: CellSpec, expected: u32) -> Result<(), u32> {
        if self.frozen {
            return Err(self.fail(TM_EFROZEN));
        }
        let id = spec.id();
        if id >= CASES || spec.op >= OPS as u8 {
            return Err(self.fail(TM_ECASE));
        }
        if let Some(c) = self.cells.get(id) {
            if c.live {
                return Err(self.fail(TM_EDUP));
            }
        }
        if id < self.cursor {
            return Err(self.fail(TM_EORDER));
        }
        if expected > ERRNO_MAX as u32 {
            return Err(self.fail(TM_ENOEXP));
        }
        if let Some(slot) = self.cells.get_mut(id) {
            *slot = Cell {
                spec,
                expected,
                got: 0,
                seen: false,
                live: true,
            };
        }
        self.registered = self.registered.wrapping_add(1);
        self.cursor = id.wrapping_add(1);
        Ok(())
    }

    /// 登记并按规则复算期望。
    pub fn register_ruled(&mut self, spec: CellSpec) -> Result<(), u32> {
        let e = rule_error(spec);
        self.register(spec, e)
    }

    /// 观测实测：须已登记、只测一次、码域内。
    pub fn observe(&mut self, id: usize, errno: u32) -> Result<(), u32> {
        if self.frozen {
            return Err(self.fail(TM_EFROZEN));
        }
        if id >= CASES {
            return Err(self.fail(TM_ECASE));
        }
        if errno > ERRNO_MAX as u32 {
            return Err(self.fail(TM_ENOEXP));
        }
        let c = match self.cells.get(id) {
            Some(c) => *c,
            None => return Err(self.fail(TM_EUNREG)),
        };
        if !c.live {
            return Err(self.fail(TM_EUNREG));
        }
        if c.seen {
            return Err(self.fail(TM_EDUP));
        }
        if let Some(slot) = self.cells.get_mut(id) {
            slot.got = errno;
            slot.seen = true;
        }
        self.observed = self.observed.wrapping_add(1);
        Ok(())
    }

    /// 登记例数。
    pub fn registered(&self) -> usize {
        self.registered
    }

    /// 已测例数。
    pub fn observed(&self) -> usize {
        self.observed
    }

    /// 拒止计数。
    pub fn rejects(&self) -> u32 {
        self.rejects
    }

    /// 是否封盘。
    pub fn frozen(&self) -> bool {
        self.frozen
    }

    /// 只读单例。
    pub fn cell(&self, id: usize) -> Option<Cell> {
        self.cells.get(id).copied()
    }
}

impl TermLedger {
    /// 某格是否闭合：五例皆登记、皆实测、皆相符。
    pub fn quad_closed(&self, quad: usize) -> bool {
        if quad >= QUADS {
            return false;
        }
        let mut k = 0usize;
        while k < PER_QUAD {
            let id = quad.wrapping_mul(PER_QUAD).wrapping_add(k);
            match self.cells.get(id) {
                Some(c) => {
                    if !c.live || !c.seen || c.got != c.expected {
                        return false;
                    }
                }
                None => return false,
            }
            k = k.wrapping_add(1);
        }
        true
    }

    /// 已闭合格数。
    pub fn closed_quads(&self) -> usize {
        let mut n = 0usize;
        let mut q = 0usize;
        while q < QUADS {
            if self.quad_closed(q) {
                n = n.wrapping_add(1);
            }
            q = q.wrapping_add(1);
        }
        n
    }

    /// 全部格闭合。
    pub fn all_quads_closed(&self) -> bool {
        self.closed_quads() == QUADS
    }

    /// 每格的不符例数。
    pub fn quad_mismatches(&self, quad: usize) -> u32 {
        if quad >= QUADS {
            return 0;
        }
        let mut n = 0u32;
        let mut k = 0usize;
        while k < PER_QUAD {
            let id = quad.wrapping_mul(PER_QUAD).wrapping_add(k);
            if let Some(c) = self.cells.get(id) {
                if c.live && c.seen && c.got != c.expected {
                    n = n.wrapping_add(1);
                }
            }
            k = k.wrapping_add(1);
        }
        n
    }

    /// 不符总数。
    pub fn mismatch_count(&self) -> u32 {
        let mut n = 0u32;
        let mut q = 0usize;
        while q < QUADS {
            n = n.wrapping_add(self.quad_mismatches(q));
            q = q.wrapping_add(1);
        }
        n
    }

    /// 缺口：未登记或未实测的例数。
    pub fn gaps(&self) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < CASES {
            match self.cells.get(i) {
                Some(c) => {
                    if !c.live || !c.seen {
                        n = n.wrapping_add(1);
                    }
                }
                None => n = n.wrapping_add(1),
            }
            i = i.wrapping_add(1);
        }
        n
    }

    /// 每族登记例数。
    pub fn op_table(&self) -> [u32; OPS] {
        let mut out = [0u32; OPS];
        let mut i = 0usize;
        while i < CASES {
            if let Some(c) = self.cells.get(i) {
                if c.live {
                    if let Some(slot) = out.get_mut(c.spec.op as usize) {
                        *slot = slot.wrapping_add(1);
                    }
                }
            }
            i = i.wrapping_add(1);
        }
        out
    }

    /// 族覆盖：八族各至少一例。
    pub fn ops_covered(&self) -> bool {
        let t = self.op_table();
        let mut i = 0usize;
        while i < OPS {
            if t[i] == 0 {
                return false;
            }
            i = i.wrapping_add(1);
        }
        true
    }

    /// 规则复算一致例数。
    pub fn rule_agreement(&self) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < CASES {
            if let Some(c) = self.cells.get(i) {
                if c.rule_agrees() {
                    n = n.wrapping_add(1);
                }
            }
            i = i.wrapping_add(1);
        }
        n
    }

    /// 单格指纹：格号、格名、五例逐条并入。
    pub fn quad_digest(&self, quad: usize) -> u64 {
        let mut h = FNV_OFF ^ (quad as u64);
        let mut k = 0usize;
        while k < PER_QUAD {
            let id = quad.wrapping_mul(PER_QUAD).wrapping_add(k);
            if let Some(c) = self.cells.get(id) {
                h ^= c.spec.op as u64;
                h = h.wrapping_mul(FNV_PRIME);
                h ^= c.expected as u64;
                h = h.wrapping_mul(FNV_PRIME);
                if c.seen {
                    h ^= c.got as u64;
                    h = h.wrapping_mul(FNV_PRIME);
                }
            }
            k = k.wrapping_add(1);
        }
        h
    }
}

impl TermLedger {
    /// 账本指纹：基址、计数、拒止，再按格并入四格分指纹。
    pub fn fold(&self) -> u64 {
        let mut h = FNV_OFF;
        for b in self.base.to_le_bytes() {
            h ^= b as u64;
            h = h.wrapping_mul(FNV_PRIME);
        }
        h ^= self.registered as u64;
        h = h.wrapping_mul(FNV_PRIME);
        h ^= self.observed as u64;
        h = h.wrapping_mul(FNV_PRIME);
        h ^= self.rejects as u64;
        h = h.wrapping_mul(FNV_PRIME);
        let mut q = 0usize;
        while q < QUADS {
            h ^= self.quad_digest(q);
            h = h.wrapping_mul(FNV_PRIME);
            q = q.wrapping_add(1);
        }
        h
    }

    /// 重放：非空、用例齐、实测齐、格齐、逐例相符。
    pub fn replay(&mut self) -> Result<u64, u32> {
        if self.registered == 0 {
            return Err(self.fail(TM_EQUAD));
        }
        if self.registered != CASES || self.observed != CASES {
            return Err(self.fail(TM_ESHORT));
        }
        if self.gaps() != 0 {
            return Err(self.fail(TM_ESHORT));
        }
        if self.mismatch_count() != 0 {
            return Err(self.fail(TM_EMISMATCH));
        }
        if !self.all_quads_closed() {
            return Err(self.fail(TM_EQUAD));
        }
        self.freeze()
    }

    /// 封盘：幂等。
    pub fn freeze(&mut self) -> Result<u64, u32> {
        let root = self.fold();
        if self.frozen {
            return Ok(root);
        }
        self.frozen = true;
        Ok(self.fold())
    }
}

/// 第 i 号单元的规格：格 = i / 5，格内序号 = i % 5，调用按格内序号轮转。
pub fn spec_of(i: usize) -> CellSpec {
    let quad = i / PER_QUAD;
    let k = i % PER_QUAD;
    CellSpec {
        op: (i % OPS) as u8,
        quad: quad as u8,
        k: k as u8,
    }
}

/// 搭建四格账：逐例按规则登记并回填期望值，最后整账重放。
pub fn build_ledger(base: u32) -> (TermLedger, Result<u64, u32>) {
    let mut l = TermLedger::new(base);
    let mut i = 0usize;
    while i < CASES {
        let spec = spec_of(i);
        if l.register_ruled(spec).is_err() {
            return (l, Err(TM_ECASE));
        }
        i = i.wrapping_add(1);
    }
    let mut k = 0usize;
    while k < CASES {
        let want = match l.cell(k) {
            Some(c) => c.expected,
            None => 0,
        };
        if l.observe(k, want).is_err() {
            return (l, Err(TM_ECASE));
        }
        k = k.wrapping_add(1);
    }
    let r = l.replay();
    (l, r)
}

/// 四路注入：封盘后动账、重复登记、未登记观测、期望不符。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];

    // ① 封盘后登记必被拒。
    let (mut a, ok1) = build_ledger(0x2100);
    out[0] = ok1.is_ok() && a.register_ruled(spec_of(0)).err() == Some(TM_EFROZEN);

    // ② 重复登记必被拒。
    let mut b = TermLedger::new(0x2101);
    let first = b.register_ruled(spec_of(0));
    let twice = b.register_ruled(spec_of(0));
    out[1] = first.is_ok() && twice.err() == Some(TM_EDUP);

    // ③ 未登记即观测必被拒。
    let mut c = TermLedger::new(0x2102);
    out[2] = c.observe(9, 0).err() == Some(TM_EUNREG);

    // ④ 期望与实测不符必被格账抓到。
    let mut d = TermLedger::new(0x2103);
    let mut ok4 = d.register_ruled(spec_of(0)).is_ok();
    // 0 号例在第 0 格 openpt 上按规则应成功（0），此处回填 ENOTTY 制造不符。
    ok4 = ok4 && d.observe(0, ENOTTY).is_ok();
    out[3] = ok4 && d.quad_mismatches(0) == 1 && !d.quad_closed(0);

    out
}

/// 判据集：T1~T13。
pub fn run_unx11144_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11144.b23_termios_errmatrix");

    // T1 常量面：段基址、四格、每格五例、总二十。
    let consts = SEG_BASE == 0x1f2c0
        && QUADS == 4
        && PER_QUAD == 5
        && CASES == 20
        && OPS == 8
        && OP_NAMES.len() == OPS
        && QUAD_NAMES.len() == QUADS;
    s.add("T1-常量面", consts, "四格 x 五例 = 20，八族调用");

    // T2 空账重放被拒。
    let mut empty = TermLedger::new(0x1100);
    let e2 = empty.replay().err();
    s.add(
        "T2-空账拒止",
        e2 == Some(TM_EQUAD) && empty.rejects() == 1,
        "空账 replay 落 TM_EQUAD",
    );

    // T3 缺格重放被拒：只搭第 0 格。
    let mut part = TermLedger::new(0x1101);
    let mut ok3 = true;
    let mut k3 = 0usize;
    while k3 < PER_QUAD {
        ok3 = ok3 && part.register_ruled(spec_of(k3)).is_ok();
        let want = match part.cell(k3) {
            Some(c) => c.expected,
            None => 0,
        };
        ok3 = ok3 && part.observe(k3, want).is_ok();
        k3 = k3.wrapping_add(1);
    }
    let e3 = part.replay().err();
    s.add(
        "T3-缺格拒止",
        ok3 && part.quad_closed(0) && e3 == Some(TM_ESHORT),
        "仅一格闭合时 replay 落 TM_ESHORT",
    );

    // T4 全账搭建成功并封盘。
    let (l4, r4) = build_ledger(0x1102);
    s.add(
        "T4-搭建封盘",
        r4.is_ok() && l4.frozen(),
        "build_ledger 重放通过并封盘",
    );

    // T5 用例齐、实测齐。
    s.add_dyn(
        "T5-两账齐",
        l4.registered() == CASES && l4.observed() == CASES,
        &format!("reg={} obs={}", l4.registered(), l4.observed()),
    );

    // T6 逐例闭合：不符与缺口均为零。
    s.add_dyn(
        "T6-逐例闭合",
        l4.mismatch_count() == 0 && l4.gaps() == 0,
        &format!("mismatch={} gaps={}", l4.mismatch_count(), l4.gaps()),
    );

    // T7 四格全闭合，且每格不符为零。
    let mut m7 = 0u32;
    let mut q7 = 0usize;
    while q7 < QUADS {
        m7 = m7.wrapping_add(l4.quad_mismatches(q7));
        q7 = q7.wrapping_add(1);
    }
    s.add_dyn(
        "T7-四格闭合",
        l4.all_quads_closed() && l4.closed_quads() == QUADS && m7 == 0,
        &format!("closed={}/{} mismatch={}", l4.closed_quads(), QUADS, m7),
    );

    // T8 期望与规则复算逐例一致。
    s.add_dyn(
        "T8-规则一致",
        l4.rule_agreement() == CASES,
        &format!("agree={}/{}", l4.rule_agreement(), CASES),
    );

    // T9 族覆盖：八族各至少一例，计数和等于用例数。
    let t9 = l4.op_table();
    let mut sum9 = 0u32;
    let mut i9 = 0usize;
    while i9 < OPS {
        sum9 = sum9.wrapping_add(t9[i9]);
        i9 = i9.wrapping_add(1);
    }
    s.add_dyn(
        "T9-族覆盖",
        l4.ops_covered() && sum9 == CASES as u32,
        &format!("sum={} ops={}", sum9, OPS),
    );

    // T10 分格指纹互异（四格账不得撞成一本）。
    let d0 = l4.quad_digest(0);
    let d1 = l4.quad_digest(1);
    let d2 = l4.quad_digest(2);
    let d3 = l4.quad_digest(3);
    let distinct = d0 != d1 && d1 != d2 && d2 != d3 && d0 != d2 && d0 != d3 && d1 != d3;
    s.add_dyn(
        "T10-分格互异",
        distinct,
        &format!("{:x}/{:x}/{:x}/{:x}", d0, d1, d2, d3),
    );

    // T11 封盘幂等：二次冻结指纹不变。
    let mut l11 = TermLedger::new(0x1103);
    let ra = l11.freeze();
    let rb = l11.freeze();
    s.add_dyn(
        "T11-幂等",
        ra.is_ok() && rb.is_ok() && ra == rb,
        &format!("root={:#x}", ra.unwrap_or(0)),
    );

    // T12 四路注入全过。
    let inj = inject_four();
    let all12 = inj[0] && inj[1] && inj[2] && inj[3];
    s.add_dyn(
        "T12-四注入",
        all12,
        &format!("{}/{}/{}/{}", inj[0], inj[1], inj[2], inj[3]),
    );

    // T13 干净账拒止计数为零（注入皆在独立实例上进行）。
    s.add_dyn(
        "T13-干净账",
        l4.rejects() == 0,
        &format!("rejects={}", l4.rejects()),
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f11144_flow() {
        let (l, r) = build_ledger(0x1000);
        assert!(r.is_ok());
        assert_eq!(l.registered(), CASES);
        assert_eq!(l.observed(), CASES);
        assert_eq!(l.mismatch_count(), 0);
        assert_eq!(l.gaps(), 0);
        assert_eq!(l.closed_quads(), QUADS);
        assert!(l.all_quads_closed());
        assert!(l.frozen());
        let mut l2 = l.clone();
        assert_eq!(l2.freeze(), l2.freeze());
    }

    #[test]
    fn f11144_gates() {
        let mut l = TermLedger::new(0x3100);
        assert!(l.register_ruled(spec_of(0)).is_ok());
        // 重复登记
        assert_eq!(l.register_ruled(spec_of(0)).err(), Some(TM_EDUP));
        // 例号越界（格 5 推出例号 25）
        assert_eq!(
            l.register(CellSpec { op: 0, quad: 5, k: 0 }, 0).err(),
            Some(TM_ECASE)
        );
        // 跨格登记
        assert!(l.register_ruled(CellSpec { op: 0, quad: 1, k: 0 }).is_ok());
        // 序倒挂：回头补第 0 格第 3 例
        assert_eq!(
            l.register(CellSpec { op: 1, quad: 0, k: 3 }, 0).err(),
            Some(TM_EORDER)
        );
        // 期望码越界
        assert_eq!(
            l.register(CellSpec { op: 1, quad: 2, k: 0 }, 999).err(),
            Some(TM_ENOEXP)
        );
        // 未登记即观测
        assert_eq!(l.observe(2, 0).err(), Some(TM_EUNREG));
        // 重复观测
        assert!(l.observe(0, 0).is_ok());
        assert_eq!(l.observe(0, 0).err(), Some(TM_EDUP));
        assert_eq!(l.rejects(), 6);
        // 封盘后再动账
        assert!(l.freeze().is_ok());
        assert_eq!(l.register_ruled(spec_of(5)).err(), Some(TM_EFROZEN));
        assert_eq!(l.observe(5, 0).err(), Some(TM_EFROZEN));
        assert_eq!(l.rejects(), 8);
    }

    #[test]
    fn f11144_rules() {
        // 不触及 termios 的四族：只有第 0 格可走。
        assert_eq!(rule_error(CellSpec { op: 0, quad: 0, k: 0 }), 0);
        assert_eq!(rule_error(CellSpec { op: 3, quad: 0, k: 4 }), 0);
        assert_eq!(rule_error(CellSpec { op: 0, quad: 1, k: 0 }), EINVAL);
        assert_eq!(rule_error(CellSpec { op: 2, quad: 3, k: 2 }), EINVAL);
        // tcgetattr 四格皆通。
        assert_eq!(rule_error(CellSpec { op: 4, quad: 3, k: 4 }), 0);
        // tcsetattr 只在本地格第 5 例落 ENOTTY。
        assert_eq!(rule_error(CellSpec { op: 5, quad: 3, k: 4 }), ENOTTY);
        assert_eq!(rule_error(CellSpec { op: 5, quad: 3, k: 3 }), 0);
        assert_eq!(rule_error(CellSpec { op: 5, quad: 2, k: 4 }), 0);
        // tcflush 队列选择上界。
        assert_eq!(rule_error(CellSpec { op: 6, quad: 1, k: 2 }), 0);
        assert_eq!(rule_error(CellSpec { op: 6, quad: 1, k: 3 }), EINVAL);
        // tcsendbreak 时长只收 0 与 1 两档。
        assert_eq!(rule_error(CellSpec { op: 7, quad: 0, k: 3 }), 0);
        assert_eq!(rule_error(CellSpec { op: 7, quad: 0, k: 4 }), EINVAL);
        // 越界调用与越界格序。
        assert_eq!(rule_error(CellSpec { op: OPS as u8, quad: 0, k: 0 }), EINVAL);
        assert_eq!(rule_error(CellSpec { op: 4, quad: QUADS as u8, k: 0 }), EINVAL);
        assert_eq!(rule_error(CellSpec { op: 4, quad: 0, k: PER_QUAD as u8 }), EINVAL);
        assert!(CellSpec { op: 4, quad: 0, k: 0 }.expects_success());
        assert!(!CellSpec { op: 6, quad: 0, k: 4 }.expects_success());
    }

    #[test]
    fn f11144_quad_shape() {
        let (l, r) = build_ledger(0x1004);
        assert!(r.is_ok());
        // 例号映射：格在前、格内序号在后。
        assert_eq!(spec_of(0).id(), 0);
        assert_eq!(spec_of(7).id(), 7);
        assert_eq!(spec_of(19).id(), 19);
        let c = l.cell(19).expect("19 号在册");
        assert_eq!(c.spec.quad, 3);
        assert_eq!(c.spec.k, 4);
        // 四格分指纹互异。
        let ds = [
            l.quad_digest(0),
            l.quad_digest(1),
            l.quad_digest(2),
            l.quad_digest(3),
        ];
        let mut i = 0usize;
        while i < QUADS {
            let mut j = i + 1;
            while j < QUADS {
                assert_ne!(ds[i], ds[j]);
                j += 1;
            }
            i += 1;
        }
    }

    #[test]
    fn f11144_inject() {
        assert_eq!(inject_four(), [true, true, true, true]);
        let (l, r) = build_ledger(0x1005);
        assert!(r.is_ok());
        assert_eq!(l.rejects(), 0);
    }

    #[test]
    fn f11144_checks_green() {
        let cs = run_unx11144_checks();
        assert!(cs.all_passed(), "checks red in unx11144");
    }
}
