//! UNX-F11316 TLS 与信号上下文（FS 基址保存恢复，0xe9b0 段）
//!
//! x86-64 上 glibc 的线程指针就是 FS 基址。内核在投递信号时把整个用户态
//! 上下文（含 FS 基址）压进信号帧，处理函数返回时由 sigreturn 还原；只要
//! 处理函数不主动改写 FS，进信号前后线程指针必须逐位相等——handler 里
//! 访问 `errno`、`locale`、`stack_guard` 这类 TLS 变量依赖这条不变式。
//!
//! 模型把这条不变式做成两级账：入口 `enter(fs)` 压帧并记录当时的 FS，
//! 出口 `leave(fs)` 弹帧并与帧内 FS 比对。比对不等不是“静默接受”，而是
//! 先按帧还原、再报 EMOVED——因为内核侧的还原是无条件的，用户态违规只
//! 能作为诊断事实被登记，不能被当成拒绝理由而放弃还原。
//!
//! 嵌套是常态：handler 里再触发信号会压第二层帧，深度上限与内核的信号
//! 栈容量同阶（此处 8 层）。超深必须 EDEPTH，空栈弹出必须 EFRAME；两者
//! 分档才能区分“内核栈算错”和“用户态把 leave 调多了”。
//!
//! 备用信号栈（sigaltstack）是 SEGV 场景的救命绳：尺寸低于 MINSIGSTKSZ
//! 必须 EBADSTK，基址未对齐必须 EALIGN——两者都在安装时拒绝，而不是等到
//! 真出事时才发现栈不可用。
//!
//! 账本十四格：八格成功（进出各两步、嵌套三步、备用栈一步、handler 读值
//! 一步，以及关闭帧的一步），六格拒绝（空栈弹出、深度越界、备用栈过小、
//! 备用栈错位、FS 被改写、非法模块号）。

use crate::alloc::format;
use crate::checks::CheckSet;

/// 段基址。
pub const SEG_BASE: u32 = 0xe9b0;

/// 编号非法。
pub const SG_EID: u32 = SEG_BASE + 0x01;
/// 登记顺序错误。
pub const SG_EORDER: u32 = SEG_BASE + 0x02;
/// 重复登记。
pub const SG_EDUP: u32 = SEG_BASE + 0x03;
/// 已冻结。
pub const SG_EFROZEN: u32 = SEG_BASE + 0x04;
/// 观测不存在的格。
pub const SG_ENOBS: u32 = SEG_BASE + 0x05;
/// 结论不符。
pub const SG_EMISMATCH: u32 = SEG_BASE + 0x06;
/// 账本失衡。
pub const SG_EBAL: u32 = SEG_BASE + 0x07;
/// 空栈弹出。
pub const SG_EFRAME: u32 = SEG_BASE + 0x08;
/// 嵌套越界。
pub const SG_EDEPTH: u32 = SEG_BASE + 0x09;
/// 备用栈过小。
pub const SG_EBADSTK: u32 = SEG_BASE + 0x0a;
/// 备用栈错位。
pub const SG_EALIGN: u32 = SEG_BASE + 0x0b;
/// FS 基址被改写。
pub const SG_EMOVED: u32 = SEG_BASE + 0x0c;
/// 非法模块号。
pub const SG_EMOD: u32 = SEG_BASE + 0x0d;
/// 块内偏移越界。
pub const SG_EOFF: u32 = SEG_BASE + 0x0e;
/// 票号不一致。
pub const SG_ESEAL: u32 = SEG_BASE + 0x0f;

/// 判决：成功。
pub const V_OK: u32 = 0;

/// 操作：进信号。
pub const OP_ENTER: u8 = 1;
/// 操作：出信号。
pub const OP_LEAVE: u8 = 2;
/// 操作：handler 读 TLS。
pub const OP_READ: u8 = 3;
/// 操作：安装备用栈。
pub const OP_ALTSTACK: u8 = 4;

/// 帧数上限。
pub const FRAMES: usize = 8;
/// 模块数。
pub const MODS: usize = 4;
/// 备用栈最小尺寸。
pub const MINSIGSTKSZ: u32 = 2048;
/// 主线程线程指针。
pub const TP: u64 = 0x0000_7f00_0000_0000;
/// 样本格数。
pub const SAMPLES: usize = 14;

/// FNV 偏移基。
pub const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;
/// FNV 质数。
pub const FNV_PRIME: u64 = 0x100_0000_01b3;

/// 一帧信号上下文。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Frame {
    pub fs: u64,
    pub alt: bool,
}

impl Frame {
    /// 空帧。
    pub const fn blank() -> Frame {
        Frame { fs: 0, alt: false }
    }
}

/// 模型化的 TP 相对段表：模块 0 -128/64、1 -64/32、2 0/48、3 64/16。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TpSeg {
    rel: [i32; MODS],
    size: [u32; MODS],
}

impl TpSeg {
    /// 标准表。
    pub const fn new() -> TpSeg {
        TpSeg {
            rel: [-128, -64, 0, 64],
            size: [64, 32, 48, 16],
        }
    }

    /// 取段。
    pub fn seg(&self, m: usize) -> Option<(i32, u32)> {
        match (self.rel.get(m), self.size.get(m)) {
            (Some(r), Some(s)) => Some((*r, *s)),
            _ => None,
        }
    }
}

/// 信号上下文账：帧栈 + 备用栈 + 两个计数。
#[derive(Clone, Copy, Debug)]
pub struct SigCtx {
    frames: [Frame; FRAMES],
    depth: usize,
    alt_base: u64,
    alt_size: u32,
    enters: u32,
    restores: u32,
    violations: u32,
}

impl SigCtx {
    /// 空上下文。
    pub const fn new() -> SigCtx {
        SigCtx {
            frames: [Frame::blank(); FRAMES],
            depth: 0,
            alt_base: 0,
            alt_size: 0,
            enters: 0,
            restores: 0,
            violations: 0,
        }
    }

    /// 当前深度。
    pub fn depth(&self) -> usize {
        self.depth
    }

    /// 顶层帧。
    pub fn top(&self) -> Option<Frame> {
        if self.depth == 0 {
            return None;
        }
        self.frames.get(self.depth - 1).copied()
    }

    /// 进信号：压帧，返回新深度。
    pub fn enter(&mut self, fs: u64) -> Result<usize, u32> {
        if self.depth >= FRAMES {
            return Err(SG_EDEPTH);
        }
        let f = Frame {
            fs,
            alt: self.alt_base != 0 && self.alt_size >= MINSIGSTKSZ,
        };
        match self.frames.get_mut(self.depth) {
            Some(x) => *x = f,
            None => return Err(SG_EDEPTH),
        }
        self.depth += 1;
        self.enters += 1;
        Ok(self.depth)
    }

    /// 出信号：弹帧。FS 不等时先还原再报 EMOVED。
    pub fn leave(&mut self, cur_fs: u64) -> Result<usize, u32> {
        if self.depth == 0 {
            return Err(SG_EFRAME);
        }
        self.depth -= 1;
        let f = match self.frames.get(self.depth) {
            Some(v) => *v,
            None => return Err(SG_EFRAME),
        };
        if cur_fs != f.fs {
            self.violations += 1;
            return Err(SG_EMOVED);
        }
        self.restores += 1;
        Ok(self.depth)
    }

    /// 安装备用栈：尺寸、基址、对齐三道闸。
    pub fn sigaltstack(&mut self, base: u64, size: u32) -> Result<u32, u32> {
        if base == 0 || size < MINSIGSTKSZ {
            return Err(SG_EBADSTK);
        }
        if (base & 0xf) != 0 || (size & 0xf) != 0 {
            return Err(SG_EALIGN);
        }
        self.alt_base = base;
        self.alt_size = size;
        Ok(size)
    }

    /// handler 内读 TLS：必须站在帧上，且按帧内 FS 的段表求址。
    pub fn read_in_handler(&self, m: usize, off: u32) -> Result<u32, u32> {
        if self.depth == 0 {
            return Err(SG_EFRAME);
        }
        let seg = TpSeg::new();
        let (rel, size) = match seg.seg(m) {
            Some(v) => v,
            None => return Err(SG_EMOD),
        };
        if off >= size {
            return Err(SG_EOFF);
        }
        Ok((rel + off as i32) as u32)
    }

    /// 帧栈是否是空。
    pub fn is_empty(&self) -> bool {
        self.depth == 0
    }

    /// 进入次数。
    pub fn enters(&self) -> u32 {
        self.enters
    }

    /// 还原次数。
    pub fn restores(&self) -> u32 {
        self.restores
    }

    /// 违规次数。
    pub fn violations(&self) -> u32 {
        self.violations
    }

    /// 备用栈尺寸。
    pub fn alt_size(&self) -> u32 {
        self.alt_size
    }

    /// 备用栈基址。
    pub fn alt_base(&self) -> u64 {
        self.alt_base
    }
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
            return Err(SG_EFROZEN);
        }
        if (row.slot as usize) != self.len {
            return Err(SG_EORDER);
        }
        if self.len >= SAMPLES {
            return Err(SG_EID);
        }
        let mut i = 0;
        while i < self.len {
            match self.rows.get(i) {
                Some(x) => {
                    if x.slot == row.slot {
                        return Err(SG_EDUP);
                    }
                }
                None => return Err(SG_ENOBS),
            }
            i += 1;
        }
        match self.rows.get_mut(self.len) {
            Some(x) => *x = row,
            None => return Err(SG_EID),
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
            return Err(SG_EBAL);
        }
        if self.ok_count() != 8 || self.err_count() != 6 {
            return Err(SG_EBAL);
        }
        let mut i = 0;
        while i < self.len {
            match self.rows.get(i) {
                Some(s) => {
                    if s.slot as usize != i {
                        return Err(SG_EORDER);
                    }
                    if s.ok {
                        if s.code != V_OK {
                            return Err(SG_EMISMATCH);
                        }
                    } else if s.code == V_OK || s.value != 0 {
                        return Err(SG_EMISMATCH);
                    }
                }
                None => return Err(SG_ENOBS),
            }
            i += 1;
        }
        let vals = [1u32, 20, 0, 3, 0, 2048, 1, 0];
        let mut k = 0;
        while k < vals.len() {
            match (self.sample(k), vals.get(k)) {
                (Some(s), Some(w)) => {
                    if !s.ok || s.value != *w {
                        return Err(SG_EMISMATCH);
                    }
                }
                _ => return Err(SG_EBAL),
            }
            k += 1;
        }
        let codes = [
            SG_EFRAME,
            SG_EDEPTH,
            SG_EBADSTK,
            SG_EALIGN,
            SG_EMOVED,
            SG_EMOD,
        ];
        let mut c = 0;
        let mut j = 8;
        while j < self.len {
            match self.rows.get(j) {
                Some(s) => match codes.get(c) {
                    Some(w) => {
                        if s.code != *w {
                            return Err(SG_EMISMATCH);
                        }
                    }
                    None => return Err(SG_EBAL),
                },
                None => return Err(SG_ENOBS),
            }
            c += 1;
            j += 1;
        }
        if c != codes.len() {
            return Err(SG_EBAL);
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
            return Err(SG_EFROZEN);
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
            return Err(SG_EBAL);
        }
        let h = fresh.seal();
        if self.frozen && h != self.seal {
            return Err(SG_ESEAL);
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

/// 由结果生成一格。
pub fn cell(slot: u8, op: u8, r: Result<u32, u32>) -> Sample {
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

/// 深度结果折算成 u32。
pub fn depth_cell(slot: u8, op: u8, r: Result<usize, u32>) -> Sample {
    match r {
        Ok(v) => Sample {
            slot,
            op,
            ok: true,
            code: V_OK,
            value: v as u32,
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

/// 读值结果折算成 u32。
pub fn read_cell(slot: u8, r: Result<u32, u32>) -> Sample {
    cell(slot, OP_READ, r)
}

/// 构建十四格账本。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();
    let mut c = SigCtx::new();

    let _ = l.register(depth_cell(0, OP_ENTER, c.enter(TP)));
    let _ = l.register(read_cell(1, c.read_in_handler(2, 20)));
    let _ = l.register(depth_cell(2, OP_LEAVE, c.leave(TP)));

    let _ = c.enter(TP);
    let _ = c.enter(TP);
    let d3 = c.enter(TP);
    let _ = l.register(depth_cell(3, OP_ENTER, d3));

    let _ = c.leave(TP);
    let _ = c.leave(TP);
    let d4 = c.leave(TP);
    let _ = l.register(depth_cell(4, OP_LEAVE, d4));

    let _ = l.register(cell(5, OP_ALTSTACK, c.sigaltstack(0x0000_7f00_0000_1000, 2048)));

    let _ = l.register(depth_cell(6, OP_ENTER, c.enter(TP)));
    let _ = l.register(depth_cell(7, OP_LEAVE, c.leave(TP)));

    let _ = l.register(depth_cell(8, OP_LEAVE, c.leave(TP)));

    let mut deep = SigCtx::new();
    let mut i = 0;
    let mut last = Ok(0usize);
    while i < FRAMES + 1 {
        last = deep.enter(TP);
        i += 1;
    }
    let _ = l.register(depth_cell(9, OP_ENTER, last));

    let _ = l.register(cell(10, OP_ALTSTACK, c.sigaltstack(0x0000_7f00_0000_2000, 1024)));
    let _ = l.register(cell(11, OP_ALTSTACK, c.sigaltstack(0x0000_7f00_0000_2008, 4096)));

    let _ = c.enter(TP);
    let _ = l.register(depth_cell(12, OP_LEAVE, c.leave(TP + 64)));

    let _ = c.enter(TP);
    let _ = l.register(read_cell(13, c.read_in_handler(7, 0)));

    l
}

/// 四路注入：账本重放、FS 不变式、嵌套与备用栈、闸门齐全。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];

    let l = build_ledger();
    let base = l.len() == SAMPLES && l.audit().is_ok();
    let rep = l.replay();
    let root = l.seal();
    out[0] = base && rep == Ok(root) && root != 0;

    let mut c = SigCtx::new();
    let mut i = 0;
    let mut good = true;
    while i < 1000 {
        if c.enter(TP) != Ok(1) {
            good = false;
        }
        if c.leave(TP) != Ok(0) {
            good = false;
        }
        i += 1;
    }
    out[1] = good
        && c.restores() == 1000
        && c.violations() == 0
        && c.is_empty()
        && c.enters() == 1000;

    let mut n = SigCtx::new();
    let alt = n.sigaltstack(0x0000_7f00_0000_3000, 4096);
    let mut all = true;
    let mut k = 0;
    while k < FRAMES {
        if n.enter(TP) != Ok(k + 1) {
            all = false;
        }
        match n.top() {
            Some(f) => {
                if !f.alt {
                    all = false;
                }
            }
            None => all = false,
        }
        k += 1;
    }
    let mut pop = true;
    while !n.is_empty() {
        let want = n.depth() - 1;
        if n.leave(TP) != Ok(want) {
            pop = false;
        }
    }
    out[2] = alt == Ok(4096) && all && pop && n.restores() == FRAMES as u32;

    out[3] = gate_probe();

    out
}

/// 六道闸门逐一验证。
pub fn gate_probe() -> bool {
    let mut c = SigCtx::new();
    if c.leave(TP) != Err(SG_EFRAME) {
        return false;
    }
    let mut d = SigCtx::new();
    let mut i = 0;
    while i < FRAMES {
        if d.enter(TP) != Ok(i + 1) {
            return false;
        }
        i += 1;
    }
    if d.enter(TP) != Err(SG_EDEPTH) {
        return false;
    }
    if c.sigaltstack(0, 4096) != Err(SG_EBADSTK) {
        return false;
    }
    if c.sigaltstack(0x1000, 1024) != Err(SG_EBADSTK) {
        return false;
    }
    if c.sigaltstack(0x1008, 4096) != Err(SG_EALIGN) {
        return false;
    }
    if c.sigaltstack(0x1000, 2047) != Err(SG_EBADSTK) {
        return false;
    }
    if c.sigaltstack(0x1000, 2048) != Ok(2048) {
        return false;
    }
    let mut e = SigCtx::new();
    let _ = e.enter(TP);
    if e.leave(TP + 8) != Err(SG_EMOVED) || e.violations() != 1 || e.restores() != 0 {
        return false;
    }
    let mut f = SigCtx::new();
    let _ = f.enter(TP);
    if f.read_in_handler(9, 0) != Err(SG_EMOD) {
        return false;
    }
    if f.read_in_handler(0, 64) != Err(SG_EOFF) {
        return false;
    }
    if f.read_in_handler(0, 63) != Ok((-128i32 + 63) as u32) {
        return false;
    }
    let g = SigCtx::new();
    if g.read_in_handler(0, 0) != Err(SG_EFRAME) {
        return false;
    }
    true
}

/// 十三项判据。
pub fn run_unx11316_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11316.sigtls");
    let l = build_ledger();

    s.add(
        "SG1-十四格齐备",
        l.len() == SAMPLES && !l.is_empty() && l.sample(0).map(|x| x.op) == Some(OP_ENTER),
        "static",
    );

    s.add_dyn(
        "SG2-八允六拒分布正确",
        l.ok_count() == 8 && l.err_count() == 6 && l.audit().is_ok(),
        &format!("ok={} err={}", l.ok_count(), l.err_count()),
    );

    s.add(
        "SG3-帧栈深度轨迹",
        l.sample(0).map(|x| x.value) == Some(1)
            && l.sample(2).map(|x| x.value) == Some(0)
            && l.sample(3).map(|x| x.value) == Some(3)
            && l.sample(4).map(|x| x.value) == Some(0),
        "static",
    );

    s.add(
        "SG4-handler 内 TLS 读值",
        {
            let mut c = SigCtx::new();
            let _ = c.enter(TP);
            c.read_in_handler(2, 20) == Ok(20)
                && c.read_in_handler(3, 15) == Ok(79)
                && c.read_in_handler(1, 31) == Ok((-64i32 + 31) as u32)
        },
        "static",
    );

    s.add(
        "SG5-FS 保存与还原",
        {
            let mut c = SigCtx::new();
            let _ = c.enter(TP);
            let d = c.leave(TP);
            d == Ok(0) && c.restores() == 1 && c.violations() == 0 && c.is_empty()
        },
        "static",
    );

    s.add(
        "SG6-八层嵌套上限",
        {
            let mut c = SigCtx::new();
            let mut i = 0;
            let mut ok = true;
            while i < FRAMES {
                if c.enter(TP) != Ok(i + 1) {
                    ok = false;
                }
                i += 1;
            }
            ok && c.depth() == FRAMES && c.enter(TP) == Err(SG_EDEPTH)
        },
        "static",
    );

    s.add(
        "SG7-空栈弹出闸门",
        l.sample(8).map(|x| x.code) == Some(SG_EFRAME) && SigCtx::new().leave(TP) == Err(SG_EFRAME),
        "static",
    );

    s.add(
        "SG8-FS 改写检出",
        l.sample(12).map(|x| x.code) == Some(SG_EMOVED)
            && {
                let mut c = SigCtx::new();
                let _ = c.enter(TP);
                let r = c.leave(TP + 64);
                r == Err(SG_EMOVED) && c.violations() == 1 && c.restores() == 0
            },
        "static",
    );

    s.add(
        "SG9-备用栈尺寸闸门",
        l.sample(10).map(|x| x.code) == Some(SG_EBADSTK)
            && {
                let mut c = SigCtx::new();
                c.sigaltstack(0x1000, 1024) == Err(SG_EBADSTK)
                    && c.sigaltstack(0x1000, MINSIGSTKSZ - 1) == Err(SG_EBADSTK)
                    && c.sigaltstack(0x1000, MINSIGSTKSZ) == Ok(MINSIGSTKSZ)
                    && c.alt_size() == MINSIGSTKSZ
            },
        "static",
    );

    s.add(
        "SG10-备用栈对齐闸门",
        l.sample(11).map(|x| x.code) == Some(SG_EALIGN)
            && {
                let mut c = SigCtx::new();
                c.sigaltstack(0x1008, 4096) == Err(SG_EALIGN)
                    && c.sigaltstack(0x1000, 4100) == Err(SG_EALIGN)
                    && c.sigaltstack(0x1000, 4096) == Ok(4096)
                    && c.alt_base() == 0x1000
            },
        "static",
    );

    s.add(
        "SG11-模块与偏移闸门",
        l.sample(13).map(|x| x.code) == Some(SG_EMOD)
            && {
                let mut c = SigCtx::new();
                let _ = c.enter(TP);
                c.read_in_handler(7, 0) == Err(SG_EMOD)
                    && c.read_in_handler(0, 64) == Err(SG_EOFF)
                    && c.read_in_handler(2, 47) == Ok(47)
            },
        "static",
    );

    s.add(
        "SG12-千轮进出守恒",
        {
            let mut c = SigCtx::new();
            let mut i = 0;
            let mut good = true;
            while i < 1000 {
                if c.enter(TP) != Ok(1) || c.leave(TP) != Ok(0) {
                    good = false;
                }
                i += 1;
            }
            good && c.is_empty() && c.enters() == 1000 && c.restores() == 1000
        },
        "static",
    );

    s.add(
        "SG13-冻结重放与四路注入",
        {
            let mut f = build_ledger();
            let rep = f.replay();
            let one = f.freeze();
            let two = f.freeze();
            let inj = inject_four();
            one.is_ok()
                && one == rep
                && two == Err(SG_EFROZEN)
                && f.rejects() == 1
                && f.is_frozen()
                && f.root() != 0
                && inj == [true, true, true, true]
        },
        "static",
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ledger_shape() {
        let l = build_ledger();
        assert_eq!(l.len(), SAMPLES);
        assert_eq!(l.ok_count(), 8);
        assert_eq!(l.err_count(), 6);
        assert!(l.audit().is_ok());
    }

    #[test]
    fn checks_pass() {
        let s = run_unx11316_checks();
        assert_eq!(s.len(), 13);
        assert!(s.all_passed());
    }

    #[test]
    fn nest_and_unwind() {
        let mut c = SigCtx::new();
        assert_eq!(c.enter(TP), Ok(1));
        assert_eq!(c.enter(TP + 16), Ok(2));
        assert_eq!(c.leave(TP + 16), Ok(1));
        assert_eq!(c.leave(TP), Ok(0));
        assert_eq!(c.restores(), 2);
        assert_eq!(c.violations(), 0);
    }

    #[test]
    fn moved_fs_restores_anyway() {
        let mut c = SigCtx::new();
        let _ = c.enter(TP);
        assert_eq!(c.leave(TP + 1), Err(SG_EMOVED));
        assert_eq!(c.violations(), 1);
        assert!(c.is_empty());
    }

    #[test]
    fn altstack_flags_frames() {
        let mut c = SigCtx::new();
        assert_eq!(c.enter(TP), Ok(1));
        assert_eq!(c.top().map(|f| f.alt), Some(false));
        assert_eq!(c.sigaltstack(0x2000, 2048), Ok(2048));
        assert_eq!(c.enter(TP), Ok(2));
        assert_eq!(c.top().map(|f| f.alt), Some(true));
    }

    #[test]
    fn depth_wall_is_exact() {
        let mut c = SigCtx::new();
        let mut i = 0;
        while i < FRAMES {
            assert!(c.enter(TP).is_ok());
            i += 1;
        }
        assert_eq!(c.depth(), FRAMES);
        assert_eq!(c.enter(TP), Err(SG_EDEPTH));
    }

    #[test]
    fn handler_read_requires_frame() {
        let c = SigCtx::new();
        assert_eq!(c.read_in_handler(0, 0), Err(SG_EFRAME));
    }

    #[test]
    fn freeze_then_replay_matches() {
        let mut l = build_ledger();
        let root = l.seal();
        assert_eq!(l.freeze(), Ok(root));
        assert_eq!(l.replay(), Ok(root));
        assert_eq!(l.freeze(), Err(SG_EFROZEN));
    }
}
