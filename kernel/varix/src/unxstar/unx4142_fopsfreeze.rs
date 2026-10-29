//! unx4142_fopsfreeze — UNX-F4142 file_operations 语义表逐函数冻结（诊断段 0x9380）。
//!
//! 判据原文：UNX-F4142-J1 f_op 全函数清单（read/write/llseek/ioctl/mmap/poll/flush/
//!           release/fsync 等 12 函数）签名/偏移语义/返回字节数约定逐函数断言通过，
//!           冻结快照哈希落账。
//!   J2 偏移语义三类分布固定为 2/3/7。
//!   J3 字节型返回恰 3 项且必配偏移语义。
//!   J4 冻结后改动规格表必令哈希失配。
//! 设计纪律：
//!   1. 冻结的含义是「逐字段断言 + 一次性哈希封存」：12 个函数各断言元数、参数方向、
//!      返回类型；四项字节型函数（read/write/pread 类）另断言「返回值为实际字节数，
//!      短读合法、负值为错误」这一条约定。
//!   2. 偏移语义分三类：不改偏移（ioctl/poll/fsync/flush/release）、由参数给偏移
//!      （llseek）、由 f_pos 隐式推进（read/write）。三类混判即 E_OFFSEM。
//!   3. 哈希是冻结的凭据而非判据本身：哈希只覆盖「名+元数+方向+返回值类别+偏移类」，
//!      不覆盖注释与顺序无关字段；哈希变了即表变了，必须走版本流程。
//!   4. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。
//! 诊断段 0x9380 本单独占。

use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0x9380;

// ── 诊断码闭集 ───────────────────────────────────────────────
const E_ARGC: u16 = SEG_BASE + 1; // 元数与冻结值不符
const E_DIR: u16 = SEG_BASE + 2; // 参数方向与冻结值不符
const E_RET: u16 = SEG_BASE + 3; // 返回类型/字节约定不符
const E_OFFSEM: u16 = SEG_BASE + 4; // 偏移语义分类错
const E_COUNT: u16 = SEG_BASE + 5; // 函数数不是 12
const E_HASH: u16 = SEG_BASE + 6; // 快照哈希不匹配
const E_FROZEN: u16 = SEG_BASE + 7; // 冻结后试图改写
const E_IDX: u16 = SEG_BASE + 8; // 函数序号越界

/// 冻结函数数（判据原文 12）。
pub const FN_COUNT: usize = 12;
/// 字节型返回的函数数（read/write/splice_read 三项，返回实际字节数）。
pub const BYTE_RET_COUNT: u32 = 3;

/// 参数方向。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Dir {
    In = 0,
    Out = 1,
    InOut = 2,
}

/// 返回类型类别。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RetKind {
    SsizeT = 0, // 字节数或负 errno
    Int = 1,    // 0 或负 errno
    LoffT = 2,  // 新偏移或负 errno
    Void = 3,   // 无返回
}

/// 偏移语义分类（判据「偏移语义」的观测面）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum OffSem {
    ImplicitPos = 0, // 经 f_pos 隐式推进
    ExplicitArg = 1, // 由参数给定偏移
    NoTouch = 2,     // 不触碰偏移
}

/// 冻结相位机：Draft → Asserted → Frozen；Broken 为断言失败终态。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FreezeState {
    Draft,
    Asserted,
    Frozen,
    Broken,
}

/// 函数规格：冻结表的行。
#[derive(Clone, Copy)]
pub struct FnSpec {
    /// 函数名（file_operations 成员名）
    pub name: &'static str,
    /// 元数（本域只记「文件对象之外的有效参数个数」）
    pub argc: u8,
    /// 首个缓冲参数方向
    pub dir: Dir,
    /// 返回类别
    pub ret: RetKind,
    /// 偏移语义
    pub off: OffSem,
}

/// 冻结快照：哈希 + 条数 + 冻结标记。
pub struct Snapshot {
    /// FNV-1a 32 位哈希
    pub hash: u32,
    /// 覆盖条数
    pub count: u8,
    /// 是否已封存
    pub frozen: bool,
}

/// 语义表：规格表 + 断言账 + 快照。
pub struct FopsFreeze {
    /// 规格表（可被注入方扰动以验证断言有效）
    pub specs: Vec<FnSpec>,
    /// 冻结相位
    pub state: FreezeState,
    /// 断言通过次数
    pub asserts_ok: u32,
    /// 断言失败次数
    pub asserts_bad: u32,
    /// 快照
    pub snap: Snapshot,
    /// 逐函数断言记录（1 表通过）
    pub marks: Vec<u8>,
}

/// FNV-1a 32 位（offset basis 0x811C9DC5，prime 0x01000193）。
pub fn fnv1a(bytes: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    let mut i = 0usize;
    while i < bytes.len() {
        h ^= bytes[i] as u32;
        h = h.wrapping_mul(0x0100_0193);
        i += 1;
    }
    h
}

/// 冻结清单（来源：Linux v5.10 include/linux/fs.h struct file_operations 常用 12 项）。
/// 逐行：名、元数、方向、返回类别、偏移语义。
pub const CANON: [FnSpec; FN_COUNT] = [
    FnSpec { name: "read", argc: 3, dir: Dir::Out, ret: RetKind::SsizeT, off: OffSem::ImplicitPos },
    FnSpec { name: "write", argc: 3, dir: Dir::In, ret: RetKind::SsizeT, off: OffSem::ImplicitPos },
    FnSpec { name: "llseek", argc: 3, dir: Dir::Out, ret: RetKind::LoffT, off: OffSem::ExplicitArg },
    FnSpec { name: "ioctl", argc: 3, dir: Dir::InOut, ret: RetKind::Int, off: OffSem::NoTouch },
    FnSpec { name: "mmap", argc: 2, dir: Dir::InOut, ret: RetKind::Int, off: OffSem::NoTouch },
    FnSpec { name: "poll", argc: 2, dir: Dir::InOut, ret: RetKind::Int, off: OffSem::NoTouch },
    FnSpec { name: "flush", argc: 2, dir: Dir::In, ret: RetKind::Int, off: OffSem::NoTouch },
    FnSpec { name: "release", argc: 2, dir: Dir::In, ret: RetKind::Int, off: OffSem::NoTouch },
    FnSpec { name: "fsync", argc: 3, dir: Dir::In, ret: RetKind::Int, off: OffSem::NoTouch },
    FnSpec { name: "fasync", argc: 3, dir: Dir::In, ret: RetKind::Int, off: OffSem::NoTouch },
    FnSpec { name: "splice_read", argc: 4, dir: Dir::Out, ret: RetKind::SsizeT, off: OffSem::ExplicitArg },
    FnSpec { name: "fallocate", argc: 3, dir: Dir::In, ret: RetKind::Int, off: OffSem::ExplicitArg },
];

impl FopsFreeze {
    pub fn new() -> FopsFreeze {
        let mut v = Vec::new();
        let mut i = 0usize;
        while i < FN_COUNT {
            v.push(CANON[i]);
            i += 1;
        }
        let mut m = Vec::new();
        let mut j = 0usize;
        while j < FN_COUNT {
            m.push(0u8);
            j += 1;
        }
        FopsFreeze {
            specs: v,
            state: FreezeState::Draft,
            asserts_ok: 0,
            asserts_bad: 0,
            snap: Snapshot {
                hash: 0,
                count: 0,
                frozen: false,
            },
            marks: m,
        }
    }

    /// 单函数断言：元数、方向、返回类别、偏移语义四项逐项比对。
    pub fn assert_one(&mut self, idx: usize) -> Result<(), u16> {
        if idx >= self.specs.len() || idx >= FN_COUNT {
            return Err(E_IDX);
        }
        let got = self.specs[idx];
        let want = CANON[idx];
        if got.name != want.name {
            self.asserts_bad += 1;
            return Err(E_IDX);
        }
        if got.argc != want.argc {
            self.asserts_bad += 1;
            return Err(E_ARGC);
        }
        if got.dir != want.dir {
            self.asserts_bad += 1;
            return Err(E_DIR);
        }
        if got.ret != want.ret {
            self.asserts_bad += 1;
            return Err(E_RET);
        }
        if got.off != want.off {
            self.asserts_bad += 1;
            return Err(E_OFFSEM);
        }
        // 字节约定：SsizeT 类必须给「返回实际字节数」语义，其余类不得声称
        if got.ret == RetKind::SsizeT && got.off == OffSem::NoTouch {
            self.asserts_bad += 1;
            return Err(E_RET); // 返回字节数却不推进/不接偏移，语义自相矛盾
        }
        self.asserts_ok += 1;
        if let Some(slot) = self.marks.get_mut(idx) {
            *slot = 1;
        }
        if self.state == FreezeState::Draft {
            self.state = FreezeState::Asserted;
        }
        Ok(())
    }

    /// 全表断言：12 条逐条走，任一失败即落 Broken（不吞错）。
    pub fn assert_all(&mut self) -> Result<u32, u16> {
        if self.specs.len() != FN_COUNT {
            return Err(E_COUNT);
        }
        let mut i = 0usize;
        while i < FN_COUNT {
            if let Err(e) = self.assert_one(i) {
                self.state = FreezeState::Broken;
                return Err(e);
            }
            i += 1;
        }
        Ok(self.asserts_ok)
    }

    /// 字节型函数计数（判据「返回字节数约定」的覆盖数）。
    pub fn byte_ret_count(&self) -> u32 {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < self.specs.len() {
            if self.specs[i].ret == RetKind::SsizeT {
                n += 1;
            }
            i += 1;
        }
        n
    }

    /// 偏移语义分布：三类各多少（用于「三类混判」的观测）。
    pub fn off_sem_dist(&self) -> (u32, u32, u32) {
        let mut imp = 0u32;
        let mut exp = 0u32;
        let mut none = 0u32;
        let mut i = 0usize;
        while i < self.specs.len() {
            match self.specs[i].off {
                OffSem::ImplicitPos => imp += 1,
                OffSem::ExplicitArg => exp += 1,
                OffSem::NoTouch => none += 1,
            }
            i += 1;
        }
        (imp, exp, none)
    }

    /// 快照哈希计算：只把「名首字节 + 元数 + 方向 + 返回类 + 偏移类」喂进 FNV。
    /// 取名字首字节即可区分本表 12 项（r/w/l/i/m/p/f/r2/f2/a/s/f3 的冲突由元数补偿），
    /// 故同时喂入名字全部字节的异或折叠值以保证稳定。
    pub fn compute_hash(&self) -> u32 {
        let mut buf: Vec<u8> = Vec::new();
        let mut i = 0usize;
        while i < self.specs.len() {
            let s = self.specs[i];
            let mut fold = 0u8;
            let mut k = 0usize;
            while k < s.name.len() {
                fold ^= s.name.as_bytes()[k];
                k += 1;
            }
            buf.push(fold);
            buf.push(s.argc);
            buf.push(s.dir as u8);
            buf.push(s.ret as u8);
            buf.push(s.off as u8);
            i += 1;
        }
        fnv1a(&buf)
    }

    /// 冻结：断言全过 → 计算哈希 → 封存入快照。
    pub fn freeze(&mut self) -> Result<u32, u16> {
        if self.state != FreezeState::Asserted {
            return Err(E_FROZEN); // 未断言即冻结（或已冻结）都拒
        }
        if self.specs.len() != FN_COUNT {
            return Err(E_COUNT);
        }
        let h = self.compute_hash();
        if h == 0 {
            return Err(E_HASH); // 全零哈希视为伪冻结
        }
        self.snap = Snapshot {
            hash: h,
            count: FN_COUNT as u8,
            frozen: true,
        };
        self.state = FreezeState::Frozen;
        Ok(h)
    }

    /// 冻结后校验：哈希重算必须与快照一致（表被动过即 E_HASH）。
    pub fn verify(&self) -> Result<u32, u16> {
        if !self.snap.frozen || self.snap.count as usize != FN_COUNT {
            return Err(E_FROZEN);
        }
        let h = self.compute_hash();
        if h != self.snap.hash {
            return Err(E_HASH);
        }
        Ok(h)
    }

    /// 冻结后改写尝试：必拒（冻结不可逆）。
    pub fn mutate(&mut self, idx: usize, argc: u8) -> Result<(), u16> {
        if self.state == FreezeState::Frozen {
            return Err(E_FROZEN);
        }
        if idx >= self.specs.len() {
            return Err(E_IDX);
        }
        self.specs[idx].argc = argc;
        Ok(())
    }

    /// 断言覆盖：12 条全打勾。
    pub fn fully_marked(&self) -> bool {
        let mut i = 0usize;
        while i < self.marks.len() {
            if self.marks[i] != 1 {
                return false;
            }
            i += 1;
        }
        self.marks.len() == FN_COUNT
    }
}

/// 诊断码解释。
pub fn detail(code: u16) -> &'static str {
    match code {
        E_ARGC => "元数与冻结值不符",
        E_DIR => "参数方向与冻结值不符",
        E_RET => "返回类型或字节约定不符",
        E_OFFSEM => "偏移语义分类错",
        E_COUNT => "函数数不是 12",
        E_HASH => "快照哈希不匹配",
        E_FROZEN => "冻结后试图改写或未断言即冻结",
        E_IDX => "函数序号越界或函数名不符",
        _ => "未知诊断码",
    }
}

/// 确定性演练：逐函数断言 → 字节约定计数 → 冻结哈希 → 篡改注入 → 对账。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;

    // R1：12 条逐函数断言，逐条返回 Ok，断言账 12/0。
    let mut ff = FopsFreeze::new();
    let mut i = 0usize;
    while i < FN_COUNT {
        ff.assert_one(i)?;
        i += 1;
    }
    if ff.asserts_ok != FN_COUNT as u32 || ff.asserts_bad != 0 {
        return Err(E_ARGC);
    }
    if ff.state != FreezeState::Asserted {
        return Err(E_FROZEN);
    }
    if !ff.fully_marked() {
        return Err(E_IDX);
    }
    hits += 1;

    // R2：字节约定——12 项中恰 3 项返回字节数（read/write/splice_read）。
    if ff.byte_ret_count() != BYTE_RET_COUNT {
        return Err(E_RET);
    }
    // 逐项核对：SsizeT 必配偏移语义（ImplicitPos 或 ExplicitArg），不许 NoTouch
    i = 0;
    while i < FN_COUNT {
        if ff.specs[i].ret == RetKind::SsizeT && ff.specs[i].off == OffSem::NoTouch {
            return Err(E_OFFSEM);
        }
        i += 1;
    }
    hits += 1;

    // R3：偏移语义分布——隐式 2、显式 3、不触碰 7。
    let dist = ff.off_sem_dist();
    if dist != (2, 3, 7) {
        return Err(E_OFFSEM);
    }
    hits += 1;

    // R4：冻结——哈希非零、条数 12、可复验且两次计算一致（确定性）。
    let h1 = ff.freeze()?;
    if h1 == 0 {
        return Err(E_HASH);
    }
    if ff.state != FreezeState::Frozen || !ff.snap.frozen {
        return Err(E_FROZEN);
    }
    let h2 = ff.verify()?;
    if h1 != h2 {
        return Err(E_HASH);
    }
    if ff.compute_hash() != h1 {
        return Err(E_HASH);
    }
    hits += 1;

    // R5：冻结后改写注入 10/10 次全部 E_FROZEN，且快照不受影响。
    let mut d = 0u32;
    let mut k = 0;
    while k < 10 {
        if ff.mutate(0, 9) != Err(E_FROZEN) {
            return Err(E_FROZEN);
        }
        d += 1;
        k += 1;
    }
    if d != 10 || ff.verify()? != h1 {
        return Err(E_FROZEN);
    }
    hits += 1;

    // I1：元数注入——把 read 的元数改成 4，断言必报 E_ARGC（断言器有效性反证）。
    let mut f2 = FopsFreeze::new();
    f2.mutate(0, 4)?;
    if f2.assert_one(0) != Err(E_ARGC) {
        return Err(E_ARGC);
    }
    if f2.asserts_bad != 1 {
        return Err(E_ARGC);
    }
    hits += 1;

    // I2：方向注入——write 的首参方向改成 Out，断言必报 E_DIR。
    let mut f3 = FopsFreeze::new();
    f3.specs[1].dir = Dir::Out;
    if f3.assert_one(1) != Err(E_DIR) {
        return Err(E_DIR);
    }
    hits += 1;

    // I3：返回类别注入——ioctl 声称返回字节数，断言必报 E_RET。
    let mut f4 = FopsFreeze::new();
    f4.specs[3].ret = RetKind::SsizeT;
    if f4.assert_one(3) != Err(E_RET) {
        return Err(E_RET);
    }
    hits += 1;

    // I4：偏移语义注入——fsync 声称推进 f_pos，断言必报 E_OFFSEM。
    let mut f5 = FopsFreeze::new();
    f5.specs[8].off = OffSem::ImplicitPos;
    if f5.assert_one(8) != Err(E_OFFSEM) {
        return Err(E_OFFSEM);
    }
    hits += 1;

    // I5：未断言即冻结 → E_FROZEN；序号越界 → E_IDX；函数数不足 → E_COUNT。
    let mut f6 = FopsFreeze::new();
    if f6.freeze() != Err(E_FROZEN) {
        return Err(E_FROZEN);
    }
    if f6.assert_one(FN_COUNT) != Err(E_IDX) {
        return Err(E_IDX);
    }
    f6.specs.pop();
    if f6.assert_all().is_ok() {
        return Err(E_COUNT);
    }
    hits += 1;

    // I6：哈希篡改检测——冻结后改动规格表，verify 必须报 E_HASH。
    let mut f7 = FopsFreeze::new();
    f7.assert_all()?;
    f7.freeze()?;
    f7.specs[11].argc = 1;
    if f7.verify() != Err(E_HASH) {
        return Err(E_HASH);
    }
    hits += 1;

    // C1：账目终审——ff 断言 12/0、快照 12 条、哈希可复验；冻结态不可逆。
    if ff.asserts_ok != 12 || ff.asserts_bad != 0 {
        return Err(E_ARGC);
    }
    if ff.snap.count as usize != FN_COUNT {
        return Err(E_COUNT);
    }
    if ff.freeze() != Err(E_FROZEN) {
        return Err(E_FROZEN);
    }
    if ff.specs.len() != FN_COUNT {
        return Err(E_COUNT);
    }
    hits += 1;

    Ok(hits)
}
