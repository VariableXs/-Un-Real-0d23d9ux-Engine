//! UNX-F11313 主线程 TLS 初始化（0xe980 段）
//!
//! 进程启动时没有 `pthread_create`，主线程的 TLS 必须由装载器直接铺好：读取
//! 每个带 `PT_TLS` 头的模块，按各自 `p_align` 求偏移，把 `tdata` 从映像拷入，
//! 把 `tbss` 清零，最后把 TP 指到静态块末端向上十六字节对齐处。这一步做错
//! 的表现极其隐蔽——`errno`、`strtok` 的游标、线程局部缓存全都会读到垃圾。
//!
//! 模型把启动期初始化拆成三件可验的事：布局（偏移与末端）、铺设（数据与零区）、
//! 回读（按模块与块内偏移取字节）。布局的闸门顺序是尺寸、头一致性、对齐、
//! 容量——`filesz > memsz` 是程序头自相矛盾，必须在任何计算前拒绝；对齐非
//! 二次幂同理；只有前三关都过了才轮到容量判断。
//!
//! 账本十四格：八格成功覆盖多模块布局、单模块布局、四个已知变量回读、
//! TP 求值与摘要复算；六格拒绝分别命中头不一致、对齐非法、尺寸为零、
//! 容量不足、回读越界与 TP 失配。摘要以 FNV 折入整块静态区，确保“拷贝
//! 结果”本身可复算，而不是只验几个坐标点。

use crate::alloc::format;
use crate::checks::CheckSet;

/// 段基址。
pub const SEG_BASE: u32 = 0xe980;

/// 编号非法。
pub const IM_EID: u32 = SEG_BASE + 0x01;
/// 登记顺序错误。
pub const IM_EORDER: u32 = SEG_BASE + 0x02;
/// 重复登记。
pub const IM_EDUP: u32 = SEG_BASE + 0x03;
/// 已冻结。
pub const IM_EFROZEN: u32 = SEG_BASE + 0x04;
/// 观测不存在的格。
pub const IM_ENOBS: u32 = SEG_BASE + 0x05;
/// 结论不符。
pub const IM_EMISMATCH: u32 = SEG_BASE + 0x06;
/// 账本失衡。
pub const IM_EBAL: u32 = SEG_BASE + 0x07;
/// 程序头自相矛盾。
pub const IM_EPHDR: u32 = SEG_BASE + 0x08;
/// 对齐非法。
pub const IM_EALIGN: u32 = SEG_BASE + 0x09;
/// 尺寸为零。
pub const IM_ESIZE: u32 = SEG_BASE + 0x0a;
/// 容量不足。
pub const IM_ENOMEM: u32 = SEG_BASE + 0x0b;
/// 回读越界。
pub const IM_EOFF: u32 = SEG_BASE + 0x0c;
/// 模块号越界。
pub const IM_EUNKNOWN: u32 = SEG_BASE + 0x0d;
/// TP 失配。
pub const IM_ETP: u32 = SEG_BASE + 0x0e;
/// 票号不一致。
pub const IM_ESEAL: u32 = SEG_BASE + 0x0f;

/// 判决：成功。
pub const V_OK: u32 = 0;

/// 操作：布局并铺设。
pub const OP_BUILD: u8 = 1;
/// 操作：回读。
pub const OP_READ: u8 = 2;
/// 操作：设 TP。
pub const OP_TP: u8 = 3;
/// 操作：复算摘要。
pub const OP_DIGEST: u8 = 4;

/// 静态区容量。
pub const CAP: usize = 256;
/// 模块数上限。
pub const MAX_MODS: usize = 4;
/// TP 对齐。
pub const TP_ALIGN: u32 = 16;
/// 样本格数。
pub const SAMPLES: usize = 14;

/// FNV 偏移基。
pub const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;
/// FNV 质数。
pub const FNV_PRIME: u64 = 0x100_0000_01b3;

/// 一个 `PT_TLS` 模块头。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Module {
    pub filesz: u32,
    pub memsz: u32,
    pub align: u32,
    pub first: u8,
}

impl Module {
    /// 构造。
    pub const fn new(filesz: u32, memsz: u32, align: u32, first: u8) -> Module {
        Module {
            filesz,
            memsz,
            align,
            first,
        }
    }
}

/// 布局结果。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Report {
    pub offsets: [u32; MAX_MODS],
    pub n: usize,
    pub end: u32,
    pub digest: u64,
}

impl Report {
    /// 空报告。
    pub const fn empty() -> Report {
        Report {
            offsets: [0; MAX_MODS],
            n: 0,
            end: 0,
            digest: 0,
        }
    }

    /// 取模块偏移。
    pub fn offset(&self, i: usize) -> Option<u32> {
        if i >= self.n {
            return None;
        }
        self.offsets.get(i).copied()
    }
}

fn align_up(v: u32, a: u32) -> Option<u32> {
    if a == 0 {
        return None;
    }
    let r = v % a;
    if r == 0 {
        Some(v)
    } else {
        v.checked_add(a - r)
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

/// 静态区摘要。
pub fn digest(buf: &[u8; CAP], upto: u32) -> u64 {
    let mut h = FNV_OFF;
    let mut i = 0;
    while i < upto as usize && i < CAP {
        if let Some(b) = buf.get(i) {
            h = fold(h, *b as u64);
        }
        i += 1;
    }
    h
}

/// 布局：尺寸 → 头一致性 → 对齐 → 容量。
pub fn layout(mods: &[Module], cap: u32) -> Result<Report, u32> {
    let mut r = Report::empty();
    let mut cursor = 0u32;
    let mut i = 0;
    while i < mods.len() && i < MAX_MODS {
        let m = match mods.get(i) {
            Some(v) => *v,
            None => return Err(IM_EUNKNOWN),
        };
        if m.memsz == 0 {
            return Err(IM_ESIZE);
        }
        if m.filesz > m.memsz {
            return Err(IM_EPHDR);
        }
        if m.align == 0 || !m.align.is_power_of_two() || m.align > 64 {
            return Err(IM_EALIGN);
        }
        let off = match align_up(cursor, m.align) {
            Some(v) => v,
            None => return Err(IM_ENOMEM),
        };
        let end = match off.checked_add(m.memsz) {
            Some(v) => v,
            None => return Err(IM_ENOMEM),
        };
        if end > cap || (end as usize) > CAP {
            return Err(IM_ENOMEM);
        }
        if let Some(slot) = r.offsets.get_mut(i) {
            *slot = off;
        }
        r.n = i + 1;
        cursor = end;
        i += 1;
    }
    r.end = cursor;
    Ok(r)
}

/// 铺设：按布局拷 `tdata`、清 `tbss`、求摘要。
pub fn build(mods: &[Module], cap: u32, buf: &mut [u8; CAP]) -> Result<Report, u32> {
    let mut r = layout(mods, cap)?;
    let mut i = 0;
    while i < r.n {
        let m = match mods.get(i) {
            Some(v) => *v,
            None => return Err(IM_EUNKNOWN),
        };
        let off = match r.offset(i) {
            Some(v) => v,
            None => return Err(IM_EUNKNOWN),
        };
        let mut k = 0;
        while k < m.filesz {
            match buf.get_mut(off as usize + k as usize) {
                Some(b) => *b = m.first.wrapping_add(k as u8),
                None => return Err(IM_ENOMEM),
            }
            k += 1;
        }
        let mut z = m.filesz;
        while z < m.memsz {
            match buf.get_mut(off as usize + z as usize) {
                Some(b) => *b = 0,
                None => return Err(IM_ENOMEM),
            }
            z += 1;
        }
        i += 1;
    }
    r.digest = digest(buf, r.end);
    Ok(r)
}

/// 回读：模块号 → 块内偏移 → 字节。
pub fn read(mods: &[Module], r: &Report, buf: &[u8; CAP], i: usize, off: u32) -> Result<u8, u32> {
    let m = match mods.get(i) {
        Some(v) => *v,
        None => return Err(IM_EUNKNOWN),
    };
    if i >= r.n {
        return Err(IM_EUNKNOWN);
    }
    if off >= m.memsz {
        return Err(IM_EOFF);
    }
    let base = match r.offset(i) {
        Some(v) => v,
        None => return Err(IM_EUNKNOWN),
    };
    match buf.get(base as usize + off as usize) {
        Some(b) => Ok(*b),
        None => Err(IM_EOFF),
    }
}

/// 设 TP：末端向上十六字节对齐。
pub fn set_tp(r: &Report) -> Result<u32, u32> {
    let tp = match align_up(r.end, TP_ALIGN) {
        Some(v) => v,
        None => return Err(IM_ETP),
    };
    if tp % TP_ALIGN != 0 || (tp as usize) > CAP {
        return Err(IM_ETP);
    }
    Ok(tp)
}

/// 一个账本样本。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Sample {
    pub slot: u8,
    pub op: u8,
    pub ok: bool,
    pub code: u32,
    pub value: u64,
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
            return Err(IM_EFROZEN);
        }
        if (row.slot as usize) != self.len {
            return Err(IM_EORDER);
        }
        if self.len >= SAMPLES {
            return Err(IM_EID);
        }
        let mut i = 0;
        while i < self.len {
            match self.rows.get(i) {
                Some(x) => {
                    if x.slot == row.slot {
                        return Err(IM_EDUP);
                    }
                }
                None => return Err(IM_ENOBS),
            }
            i += 1;
        }
        match self.rows.get_mut(self.len) {
            Some(x) => *x = row,
            None => return Err(IM_EID),
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
            return Err(IM_EBAL);
        }
        if self.ok_count() != 8 || self.err_count() != 6 {
            return Err(IM_EBAL);
        }
        let mut i = 0;
        while i < self.len {
            match self.rows.get(i) {
                Some(s) => {
                    if s.slot as usize != i {
                        return Err(IM_EORDER);
                    }
                    if s.ok {
                        if s.code != V_OK {
                            return Err(IM_EMISMATCH);
                        }
                    } else if s.code == V_OK || s.value != 0 {
                        return Err(IM_EMISMATCH);
                    }
                }
                None => return Err(IM_ENOBS),
            }
            i += 1;
        }
        let vals = [
            (0usize, 104u64),
            (1, 64),
            (2, 0x10),
            (3, 0x37),
            (4, 0),
            (5, 0),
            (6, 112),
            (7, 0),
        ];
        let mut k = 0;
        while k < vals.len() {
            match vals.get(k) {
                Some((i, w)) => match self.sample(*i) {
                    Some(s) => {
                        if !s.ok || (*i != 7 && s.value != *w) {
                            return Err(IM_EMISMATCH);
                        }
                    }
                    None => return Err(IM_ENOBS),
                },
                None => return Err(IM_EBAL),
            }
            k += 1;
        }
        let codes = [
            IM_EPHDR,
            IM_EALIGN,
            IM_ESIZE,
            IM_ENOMEM,
            IM_EOFF,
            IM_ETP,
        ];
        let mut c = 0;
        let mut j = 8;
        while j < self.len {
            if let Some(s) = self.rows.get(j) {
                match codes.get(c) {
                    Some(w) => {
                        if s.code != *w {
                            return Err(IM_EMISMATCH);
                        }
                    }
                    None => return Err(IM_EBAL),
                }
                c += 1;
            }
            j += 1;
        }
        if c != codes.len() {
            return Err(IM_EBAL);
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
                h = fold(h, s.value);
            }
            i += 1;
        }
        h
    }

    /// 冻结。
    pub fn freeze(&mut self) -> Result<u64, u32> {
        if self.frozen {
            self.rejects += 1;
            return Err(IM_EFROZEN);
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
            return Err(IM_EBAL);
        }
        let h = fresh.seal();
        if self.frozen && h != self.seal {
            return Err(IM_ESEAL);
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

/// 三模块启动镜像。
pub fn boot_modules() -> [Module; 3] {
    [
        Module::new(16, 32, 16, 0x10),
        Module::new(24, 24, 8, 0x20),
        Module::new(8, 40, 32, 0x30),
    ]
}

/// 构建十四格账本。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();
    let mods = boot_modules();
    let mut buf = [0u8; CAP];
    let rep = match build(&mods, CAP as u32, &mut buf) {
        Ok(v) => v,
        Err(_) => Report::empty(),
    };

    let mut push = |slot: u8, op: u8, r: Result<u64, u32>| {
        let row = match r {
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
        };
        let _ = l.register(row);
    };

    push(0, OP_BUILD, build(&mods, CAP as u32, &mut buf).map(|r| r.end as u64));

    let single = [Module::new(64, 64, 8, 0x40)];
    let mut s2 = [0u8; CAP];
    push(1, OP_BUILD, build(&single, 128, &mut s2).map(|r| r.end as u64));

    push(2, OP_READ, read(&mods, &rep, &buf, 0, 0).map(|b| b as u64));
    push(3, OP_READ, read(&mods, &rep, &buf, 1, 23).map(|b| b as u64));
    push(4, OP_READ, read(&mods, &rep, &buf, 2, 8).map(|b| b as u64));
    push(5, OP_READ, read(&mods, &rep, &buf, 2, 39).map(|b| b as u64));
    push(6, OP_TP, set_tp(&rep).map(|v| v as u64));
    push(
        7,
        OP_DIGEST,
        {
            let again = digest(&buf, rep.end);
            if again == rep.digest && again != 0 {
                Ok(again)
            } else {
                Err(IM_EMISMATCH)
            }
        },
    );

    let bad_hdr = [Module::new(40, 32, 16, 1)];
    push(8, OP_BUILD, build(&bad_hdr, CAP as u32, &mut buf).map(|r| r.end as u64));

    let bad_align = [Module::new(8, 8, 3, 1)];
    push(9, OP_BUILD, build(&bad_align, CAP as u32, &mut buf).map(|r| r.end as u64));

    let zero = [Module::new(0, 0, 16, 1)];
    push(10, OP_BUILD, build(&zero, CAP as u32, &mut buf).map(|r| r.end as u64));

    let huge = [Module::new(8, 300, 16, 1)];
    push(11, OP_BUILD, build(&huge, CAP as u32, &mut buf).map(|r| r.end as u64));

    push(12, OP_READ, read(&mods, &rep, &buf, 1, 24).map(|b| b as u64));

    let odd = Report {
        offsets: rep.offsets,
        n: rep.n,
        end: 257,
        digest: rep.digest,
    };
    push(13, OP_TP, set_tp(&odd).map(|v| v as u64));

    l
}

/// 四路注入：账本重放、布局轨迹、变量回读、闸门优先级。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];

    let l = build_ledger();
    let base = l.len() == SAMPLES && l.audit().is_ok();
    let rep = l.replay();
    let root = l.seal();
    out[0] = base && rep == Ok(root) && root != 0;

    let mods = boot_modules();
    let mut buf = [0u8; CAP];
    let r = match build(&mods, CAP as u32, &mut buf) {
        Ok(v) => v,
        Err(_) => return out,
    };
    out[1] = r.end == 104 && r.n == 3 && r.offset(0) == Some(0) && r.offset(1) == Some(32)
        && r.offset(2) == Some(64);

    let mut ok = set_tp(&r) == Ok(112);
    let mut i = 0;
    while i < 3 {
        let m = mods[i];
        let mut k = 0;
        while k < m.filesz {
            if read(&mods, &r, &buf, i, k) != Ok(m.first.wrapping_add(k as u8)) {
                ok = false;
            }
            k += 1;
        }
        let mut z = m.filesz;
        while z < m.memsz {
            if read(&mods, &r, &buf, i, z) != Ok(0) {
                ok = false;
            }
            z += 1;
        }
        i += 1;
    }
    out[2] = ok;

    out[3] = layout(&[Module::new(40, 32, 16, 0)], CAP as u32) == Err(IM_EPHDR)
        && layout(&[Module::new(8, 8, 3, 0)], CAP as u32) == Err(IM_EALIGN)
        && layout(&[Module::new(0, 0, 16, 0)], CAP as u32) == Err(IM_ESIZE)
        && layout(&[Module::new(8, 300, 16, 0)], CAP as u32) == Err(IM_ENOMEM)
        && layout(&[Module::new(8, 8, 16, 0)], 0) == Err(IM_ENOMEM)
        && read(&mods, &r, &buf, 3, 0) == Err(IM_EUNKNOWN)
        && read(&mods, &r, &buf, 0, 32) == Err(IM_EOFF)
        && set_tp(&Report {
            offsets: r.offsets,
            n: r.n,
            end: 100,
            digest: r.digest,
        }) == Ok(112);

    out
}

/// 十三项判据。
pub fn run_unx11313_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11313.maintls");
    let l = build_ledger();
    let mods = boot_modules();
    let mut buf = [0u8; CAP];
    let rep = build(&mods, CAP as u32, &mut buf).unwrap_or(Report::empty());

    s.add(
        "IM1-十四格齐备",
        l.len() == SAMPLES && !l.is_empty() && l.sample(0).map(|x| x.op) == Some(OP_BUILD),
        "static",
    );

    s.add_dyn(
        "IM2-八允六拒分布正确",
        l.ok_count() == 8 && l.err_count() == 6 && l.audit().is_ok(),
        &format!("ok={} err={}", l.ok_count(), l.err_count()),
    );

    s.add(
        "IM3-布局偏移与末端",
        rep.end == 104
            && rep.offset(0) == Some(0)
            && rep.offset(1) == Some(32)
            && rep.offset(2) == Some(64)
            && rep.offset(3) == None,
        "static",
    );

    s.add(
        "IM4-数据与零区铺设",
        {
            let mut ok = true;
            let mut i = 0;
            while i < 16 {
                if buf.get(i) != Some(&(0x10u8.wrapping_add(i as u8))) {
                    ok = false;
                }
                i += 1;
            }
            let mut z = 16;
            while z < 32 {
                if buf.get(z) != Some(&0) {
                    ok = false;
                }
                z += 1;
            }
            ok
        },
        "static",
    );

    s.add(
        "IM5-启动期变量回读",
        read(&mods, &rep, &buf, 0, 0) == Ok(0x10)
            && read(&mods, &rep, &buf, 0, 15) == Ok(0x1f)
            && read(&mods, &rep, &buf, 1, 0) == Ok(0x20)
            && read(&mods, &rep, &buf, 1, 23) == Ok(0x37),
        "static",
    );

    s.add(
        "IM6-零区回读为零",
        read(&mods, &rep, &buf, 2, 8) == Ok(0)
            && read(&mods, &rep, &buf, 2, 39) == Ok(0)
            && read(&mods, &rep, &buf, 0, 31) == Ok(0),
        "static",
    );

    s.add(
        "IM7-TP 对齐求值",
        set_tp(&rep) == Ok(112) && 112 % TP_ALIGN == 0 && l.sample(6).map(|x| x.value) == Some(112),
        "static",
    );

    s.add(
        "IM8-摘要可复算",
        {
            let again = digest(&buf, rep.end);
            again == rep.digest && again != 0 && l.sample(7).map(|x| x.value) == Some(again)
        },
        "static",
    );

    s.add(
        "IM9-头一致性闸门",
        l.sample(8).map(|x| x.code) == Some(IM_EPHDR)
            && layout(&[Module::new(33, 32, 16, 0)], CAP as u32) == Err(IM_EPHDR),
        "static",
    );

    s.add(
        "IM10-对齐闸门",
        l.sample(9).map(|x| x.code) == Some(IM_EALIGN)
            && layout(&[Module::new(8, 8, 0, 0)], CAP as u32) == Err(IM_EALIGN)
            && layout(&[Module::new(8, 8, 128, 0)], CAP as u32) == Err(IM_EALIGN),
        "static",
    );

    s.add(
        "IM11-尺寸与容量闸门",
        l.sample(10).map(|x| x.code) == Some(IM_ESIZE)
            && l.sample(11).map(|x| x.code) == Some(IM_ENOMEM)
            && layout(&[Module::new(8, 8, 16, 0), Module::new(8, 200, 16, 0)], 128) == Err(IM_ENOMEM),
        "static",
    );

    s.add(
        "IM12-回读越界与 TP 失配",
        l.sample(12).map(|x| x.code) == Some(IM_EOFF)
            && l.sample(13).map(|x| x.code) == Some(IM_ETP)
            && read(&mods, &rep, &buf, 0, 32) == Err(IM_EOFF)
            && read(&mods, &rep, &buf, 9, 0) == Err(IM_EUNKNOWN),
        "static",
    );

    let mut f = build_ledger();
    let rep2 = f.replay();
    let root = match f.freeze() {
        Ok(v) => v,
        Err(_) => 0,
    };
    let again = f.freeze() == Err(IM_EFROZEN);
    let inj = inject_four();
    s.add_dyn(
        "IM13-冻结重放与四路注入",
        root != 0 && rep2 == Ok(root) && again && f.root() == root && f.rejects() == 1 && inj == [true; 4],
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

    fn boot() -> (Report, [u8; CAP]) {
        let mods = boot_modules();
        let mut buf = [0u8; CAP];
        let r = build(&mods, CAP as u32, &mut buf).expect("build");
        (r, buf)
    }

    #[test]
    fn boot_layout_is_dense() {
        let (r, _) = boot();
        assert_eq!(r.n, 3);
        assert_eq!(r.offset(0), Some(0));
        assert_eq!(r.offset(1), Some(32));
        assert_eq!(r.offset(2), Some(64));
        assert_eq!(r.end, 104);
        assert_eq!(set_tp(&r), Ok(112));
    }

    #[test]
    fn variables_read_back() {
        let mods = boot_modules();
        let (r, buf) = boot();
        assert_eq!(read(&mods, &r, &buf, 0, 0), Ok(0x10));
        assert_eq!(read(&mods, &r, &buf, 1, 23), Ok(0x37));
        assert_eq!(read(&mods, &r, &buf, 2, 39), Ok(0));
        assert_eq!(read(&mods, &r, &buf, 1, 24), Err(IM_EOFF));
    }

    #[test]
    fn layout_gates() {
        assert_eq!(layout(&[Module::new(40, 32, 16, 0)], CAP as u32), Err(IM_EPHDR));
        assert_eq!(layout(&[Module::new(8, 8, 3, 0)], CAP as u32), Err(IM_EALIGN));
        assert_eq!(layout(&[Module::new(0, 0, 16, 0)], CAP as u32), Err(IM_ESIZE));
        assert_eq!(layout(&[Module::new(8, 300, 16, 0)], CAP as u32), Err(IM_ENOMEM));
    }

    #[test]
    fn digest_is_reproducible() {
        let (r, buf) = boot();
        assert_eq!(digest(&buf, r.end), r.digest);
        let (r2, buf2) = boot();
        assert_eq!(r2.digest, r.digest);
        assert_eq!(buf2, buf);
    }

    #[test]
    fn tp_rounds_up_to_sixteen() {
        let mut r = Report::empty();
        r.end = 100;
        assert_eq!(set_tp(&r), Ok(112));
        r.end = 112;
        assert_eq!(set_tp(&r), Ok(112));
        r.end = CAP as u32;
        assert_eq!(set_tp(&r), Ok(CAP as u32));
        r.end = CAP as u32 + 1;
        assert_eq!(set_tp(&r), Err(IM_ETP));
    }

    #[test]
    fn ledger_seals_and_replays() {
        let mut l = build_ledger();
        assert!(l.audit().is_ok());
        let root = l.freeze().expect("freeze");
        assert_eq!(l.replay(), Ok(root));
        assert_eq!(l.freeze(), Err(IM_EFROZEN));
        assert_eq!(l.rejects(), 1);
    }

    #[test]
    fn checks_pass() {
        let s = run_unx11313_checks();
        assert_eq!(s.len(), 13);
        assert!(s.all_passed(), "{}", s.red_items_text());
    }
}
