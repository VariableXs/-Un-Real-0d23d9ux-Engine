//! UNX-F9233 错误码返回路径探针（负值编码与 err 链路） · 0x7A7C0段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（派工卡 UNX-F9233 逐字）：
//!   ① 纯功能代码 ≥320 行；② 判据原文：UNX-F9233-J1 负值编码探针判据过，
//!   -Errno 逐码编码一致格过，err 链路与 B13 编码器单源格过；③ 完成时
//!   API/软件写回：状态=已完成、结果摘要非空。
//!
//! 设计要点：
//!   ① 内核返回约定是「负 errno 进寄存器、0 表示成功」；编码器只能产出
//!      `-(errno as i64)`，任何本地再取负、取绝对值、改符号都算链路污染。
//!   ② 解码必须四态：0=成功、正数=用户态忘了取负、范围内负值=错误码、
//!      超范围负值/不可取负（i64::MIN）=故障。四态各自有闭集标签，不许
//!      把正数当成错误码、也不许把 -4096 当合法 errno 吞掉。
//!   ③ B13 编码器是单一来源：本文件只保留一张冻结对账表，编码函数按表
//!      查值；逐码复算必须与表逐行一致，且表里恰好 13 行、epoch=13。
//!   ④ 链路四跳固定：内核 Errno → 寄存器 raw(i64) → 系统调用包装 wrapper
//!      → libc 正 errno(i32)。四跳逐跳比符号与幅值，任何一跳翻符号即红。
//!   ⑤ err 指针面用无符号比较：`(raw as u64) >= -MAX_ERRNO` 与 Linux
//!      IS_ERR_VALUE 同构；0 与正数必须落在 err 指针区之外。
//!   ⑥ 探针语料 65 发 = 13 合法编码 + 13 正数污染 + 13 越界负值 + 13 零值
//!      成功 + 13 链路审计；分桶总和必须等于发数，逃逸必须为 0。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use crate::checks::CheckSet;

pub const SEG_BASE: u32 = 0x7A7C0;

pub const MAX_ERRNO: i32 = 4095;
pub const B13_EPOCH: u32 = 13;

/// errno 闭集（本卡只收 B03 错误面用到的 13 个码）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Errno {
    Enoent,
    Ebadf,
    Efault,
    Eexist,
    Enotdir,
    Eisdir,
    Einval,
    Emfile,
    Enospc,
    Enametoolong,
    Enotempty,
    Eloop,
    Erange,
}

impl Errno {
    pub const ALL: [Errno; 13] = [
        Errno::Enoent,
        Errno::Ebadf,
        Errno::Efault,
        Errno::Eexist,
        Errno::Enotdir,
        Errno::Eisdir,
        Errno::Einval,
        Errno::Emfile,
        Errno::Enospc,
        Errno::Enametoolong,
        Errno::Enotempty,
        Errno::Eloop,
        Errno::Erange,
    ];

    pub fn val(self) -> i32 {
        match self {
            Errno::Enoent => 2,
            Errno::Ebadf => 9,
            Errno::Efault => 14,
            Errno::Eexist => 17,
            Errno::Enotdir => 20,
            Errno::Eisdir => 21,
            Errno::Einval => 22,
            Errno::Emfile => 24,
            Errno::Enospc => 28,
            Errno::Erange => 34,
            Errno::Enametoolong => 36,
            Errno::Enotempty => 39,
            Errno::Eloop => 40,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Errno::Enoent => "ENOENT",
            Errno::Ebadf => "EBADF",
            Errno::Efault => "EFAULT",
            Errno::Eexist => "EEXIST",
            Errno::Enotdir => "ENOTDIR",
            Errno::Eisdir => "EISDIR",
            Errno::Einval => "EINVAL",
            Errno::Emfile => "EMFILE",
            Errno::Enospc => "ENOSPC",
            Errno::Enametoolong => "ENAMETOOLONG",
            Errno::Enotempty => "ENOTEMPTY",
            Errno::Eloop => "ELOOP",
            Errno::Erange => "ERANGE",
        }
    }

    pub fn detail(self) -> &'static str {
        match self {
            Errno::Enoent => "路径/对象缺失：先确认 fd 面已过再看解析面",
            Errno::Ebadf => "坏句柄：槽位或代际失效，禁止继续访问对象",
            Errno::Efault => "用户指针不可读：拷贝失败先于其他判断",
            Errno::Eexist => "目标已存在：创建类调用的独占语义",
            Errno::Enotdir => "前缀或 dirfd 非目录：目录性段先于查找段",
            Errno::Eisdir => "对象是目录：写/删类操作需换形态或先递归",
            Errno::Einval => "参数/标志非法：校验段最靠前，先修调用方",
            Errno::Emfile => "fd 表耗尽：先收缩句柄再重试",
            Errno::Enospc => "容量耗尽：回收空间或拒绝写入，不得半写",
            Errno::Enametoolong => "路径超限：在解析前截断并报错",
            Errno::Enotempty => "目录非空：递归删除或换目标",
            Errno::Eloop => "链接环超限：查找段终止，检查符号链接图",
            Errno::Erange => "数值越界：检查长度/偏移后再提交",
        }
    }

    pub fn from_val(v: i32) -> Option<Errno> {
        match v {
            2 => Some(Errno::Enoent),
            9 => Some(Errno::Ebadf),
            14 => Some(Errno::Efault),
            17 => Some(Errno::Eexist),
            20 => Some(Errno::Enotdir),
            21 => Some(Errno::Eisdir),
            22 => Some(Errno::Einval),
            24 => Some(Errno::Emfile),
            28 => Some(Errno::Enospc),
            34 => Some(Errno::Erange),
            36 => Some(Errno::Enametoolong),
            39 => Some(Errno::Enotempty),
            40 => Some(Errno::Eloop),
            _ => None,
        }
    }
}

/// B13 冻结对账表：编码值唯一来源。
pub const B13_TABLE: [(Errno, i64); 13] = [
    (Errno::Enoent, -2),
    (Errno::Ebadf, -9),
    (Errno::Efault, -14),
    (Errno::Eexist, -17),
    (Errno::Enotdir, -20),
    (Errno::Eisdir, -21),
    (Errno::Einval, -22),
    (Errno::Emfile, -24),
    (Errno::Enospc, -28),
    (Errno::Erange, -34),
    (Errno::Enametoolong, -36),
    (Errno::Enotempty, -39),
    (Errno::Eloop, -40),
];

/// 按表编码；表未覆盖返回 i64::MIN（解码必报 Overflow，绝不静默成 0）。
pub fn b13_encode(e: Errno) -> i64 {
    let mut i = 0usize;
    while i < B13_TABLE.len() {
        if let Some((code, raw)) = B13_TABLE.get(i) {
            if *code == e {
                return *raw;
            }
        }
        i += 1;
    }
    i64::MIN
}

/// 内核态返回编码（唯一入口，禁止本地再取负）。
pub fn encode_i64(e: Errno) -> i64 {
    b13_encode(e)
}

pub fn encode_i32(e: Errno) -> i32 {
    b13_encode(e) as i32
}

/// 解码四态闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Decoded {
    Success,
    Errno(Errno),
    Unknown(i32),
    Positive(i64),
    OutOfRange,
    Overflow,
}

impl Decoded {
    pub fn label(self) -> &'static str {
        match self {
            Decoded::Success => "成功",
            Decoded::Errno(_) => "已知错误码",
            Decoded::Unknown(_) => "范围内未知码",
            Decoded::Positive(_) => "正数污染",
            Decoded::OutOfRange => "越界负值",
            Decoded::Overflow => "不可取负",
        }
    }

    pub fn is_error(self) -> bool {
        match self {
            Decoded::Errno(_) | Decoded::Unknown(_) => true,
            _ => false,
        }
    }
}

/// 解码：0 成功；正数污染；负值按幅值与 MAX_ERRNO 分四态。
pub fn decode_i64(raw: i64) -> Decoded {
    if raw == 0 {
        return Decoded::Success;
    }
    if raw > 0 {
        return Decoded::Positive(raw);
    }
    let magnitude = match raw.checked_neg() {
        Some(v) => v,
        None => return Decoded::Overflow,
    };
    if magnitude > MAX_ERRNO as i64 {
        return Decoded::OutOfRange;
    }
    match Errno::from_val(magnitude as i32) {
        Some(e) => Decoded::Errno(e),
        None => Decoded::Unknown(magnitude as i32),
    }
}

pub fn decode_i32(raw: i32) -> Decoded {
    decode_i64(raw as i64)
}

/// err 指针下界：与 Linux `(unsigned long)x >= -MAX_ERRNO` 同构。
pub const ERR_PTR_FLOOR: u64 = u64::MAX - MAX_ERRNO as u64 + 1;

pub fn is_err_value(raw: i64) -> bool {
    (raw as u64) >= ERR_PTR_FLOOR
}

/// 系统调用包装层（只搬运，不改符号）。
pub fn wrapper_pass(raw: i64) -> i64 {
    raw
}

/// libc 侧还原正 errno（只用于对账，不参与内核编码）。
pub fn libc_errno(raw: i64) -> i32 {
    if raw < 0 {
        match raw.checked_neg() {
            Some(v) => v as i32,
            None => 0,
        }
    } else {
        raw as i32
    }
}

/// 链路一跳记录。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ChainRecord {
    pub e: Errno,
    pub raw: i64,
    pub wrapper: i64,
    pub libc: i32,
    pub ok: bool,
}

/// 单码链路复算：四跳符号/幅值逐个比对。
pub fn chain(e: Errno) -> ChainRecord {
    let raw = b13_encode(e);
    let wrapper = wrapper_pass(raw);
    let libc = libc_errno(wrapper);
    let ok = raw == -(e.val() as i64)
        && wrapper == raw
        && libc == e.val()
        && raw < 0
        && is_err_value(raw);
    ChainRecord { e, raw, wrapper, libc, ok }
}

#[derive(Clone, Copy)]
pub struct ChainAudit {
    pub records: [ChainRecord; 13],
    pub bad: usize,
}

/// 13 码全链路审计。
pub fn chain_all() -> ChainAudit {
    let mut records = [ChainRecord {
        e: Errno::Enoent,
        raw: 0,
        wrapper: 0,
        libc: 0,
        ok: false,
    }; 13];
    let mut bad = 0usize;
    let mut i = 0usize;
    while i < Errno::ALL.len() {
        if let Some(e) = Errno::ALL.get(i) {
            let rec = chain(*e);
            if !rec.ok {
                bad += 1;
            }
            if let Some(slot) = records.get_mut(i) {
                *slot = rec;
            }
        }
        i += 1;
    }
    ChainAudit { records, bad }
}

/// 链路审计器：给任意 raw 判是否仍与 B13 单源一致。
pub fn audit_raw(e: Errno, raw: i64) -> bool {
    raw == b13_encode(e) && raw < 0 && is_err_value(raw) && libc_errno(wrapper_pass(raw)) == e.val()
}

/// 探针账本。
#[derive(Clone, Copy)]
pub struct ErrLedger {
    pub probes: usize,
    pub encoded_ok: usize,
    pub rejected_bad: usize,
    pub success_zero: usize,
    pub chain_ok: usize,
    pub mismatches: usize,
    pub escaped: usize,
    pub chain_bad: usize,
}

impl ErrLedger {
    pub fn conserved(&self) -> bool {
        let sum = self.encoded_ok + self.rejected_bad + self.success_zero + self.chain_ok;
        self.probes > 0
            && sum == self.probes
            && self.mismatches == 0
            && self.escaped == 0
            && self.chain_bad == 0
    }
}

/// 65 发语料：13 合法 + 13 正数 + 13 越界 + 13 零值 + 13 链路。
pub fn run_corpus() -> ErrLedger {
    let mut l = ErrLedger {
        probes: 0,
        encoded_ok: 0,
        rejected_bad: 0,
        success_zero: 0,
        chain_ok: 0,
        mismatches: 0,
        escaped: 0,
        chain_bad: 0,
    };

    for e in Errno::ALL.iter().copied() {
        // ① 合法负值：解码必须回到同一码。
        l.probes += 1;
        if decode_i64(encode_i64(e)) == Decoded::Errno(e) {
            l.encoded_ok += 1;
        } else {
            l.mismatches += 1;
        }

        // ② 正数污染：必须识别为 Positive，且绝不能变成错误码。
        l.probes += 1;
        let positive = decode_i64(e.val() as i64);
        if positive == Decoded::Positive(e.val() as i64) {
            l.rejected_bad += 1;
        } else {
            l.mismatches += 1;
        }
        if positive.is_error() {
            l.escaped += 1;
        }

        // ③ 越界负值：幅值超过 MAX_ERRNO 必须 OutOfRange。
        l.probes += 1;
        let beyond = -(MAX_ERRNO as i64 + 1 + e.val() as i64);
        if decode_i64(beyond) == Decoded::OutOfRange {
            l.rejected_bad += 1;
        } else {
            l.mismatches += 1;
        }

        // ④ 零值：成功，不是错误。
        l.probes += 1;
        if decode_i64(0) == Decoded::Success {
            l.success_zero += 1;
        } else {
            l.mismatches += 1;
        }

        // ⑤ 链路审计：四跳逐码。
        l.probes += 1;
        let rec = chain(e);
        if rec.ok {
            l.chain_ok += 1;
        } else {
            l.chain_bad += 1;
        }
    }
    l
}

pub fn fnv1a(mut h: u32, bytes: &[u8]) -> u32 {
    for b in bytes.iter().copied() {
        h ^= b as u32;
        h = h.wrapping_mul(16_777_619);
    }
    h
}

pub fn corpus_fingerprint() -> u32 {
    let mut h = 0x811C_9DC5u32;
    let mut i = 0usize;
    while i < B13_TABLE.len() {
        if let Some((code, raw)) = B13_TABLE.get(i) {
            h = fnv1a(h, code.name().as_bytes());
            h = fnv1a(h, &[*raw as u8]);
        }
        i += 1;
    }
    let l = run_corpus();
    let buckets = [
        l.probes as u8,
        l.encoded_ok as u8,
        l.rejected_bad as u8,
        l.success_zero as u8,
        l.chain_ok as u8,
    ];
    h = fnv1a(h, &buckets);
    h
}

/// 本卡自检入口（≥12 条，动态明细 ≤8 条）。
pub fn run_unx9233_checks() -> CheckSet {
    let mut s = CheckSet::new("unx9233");

    let seg_ok = SEG_BASE % 0x40 == 0 && (0x7A000..=0x7D1C0).contains(&SEG_BASE);
    s.add(
        "段位守卫：0x40 对齐且落在 0x7A000..=0x7D1C0",
        seg_ok,
        "段基址唯一；重叠即拒绝注册",
    );

    let mut set_ok = Errno::ALL.len() == 13;
    let mut i = 0usize;
    while i < Errno::ALL.len() {
        if let Some(e) = Errno::ALL.get(i) {
            if e.val() <= 0 || e.val() > MAX_ERRNO || e.detail().is_empty() || e.name().is_empty()
            {
                set_ok = false;
            }
            if Errno::from_val(e.val()) != Some(*e) {
                set_ok = false;
            }
            let mut j = i + 1;
            while j < Errno::ALL.len() {
                if let Some(f) = Errno::ALL.get(j) {
                    if e.val() == f.val() {
                        set_ok = false;
                    }
                }
                j += 1;
            }
        }
        i += 1;
    }
    s.add(
        "errno 闭集：13 码、值唯一且在 1..=4095、往返可逆",
        set_ok,
        "0 永远不是错误码；闭集外一律拒绝，不许猜",
    );

    let mut enc_ok = true;
    let mut k = 0usize;
    while k < Errno::ALL.len() {
        if let Some(e) = Errno::ALL.get(k) {
            let raw = encode_i64(*e);
            if raw != -(e.val() as i64) || raw >= 0 || !is_err_value(raw) {
                enc_ok = false;
            }
        }
        k += 1;
    }
    s.add(
        "-Errno 逐码编码：13 码全为负值且幅值等于 errno",
        enc_ok,
        "编码只有 b13_encode 一个入口；本地取负即链路分叉",
    );

    let mut dec_ok = true;
    let mut m = 0usize;
    while m < Errno::ALL.len() {
        if let Some(e) = Errno::ALL.get(m) {
            if decode_i64(-(e.val() as i64)) != Decoded::Errno(*e) {
                dec_ok = false;
            }
            if decode_i32(-e.val()) != Decoded::Errno(*e) {
                dec_ok = false;
            }
        }
        m += 1;
    }
    s.add(
        "解码往返：i64/i32 两宽度逐码回到原码",
        dec_ok,
        "32 位寄存器截断后仍须与 64 位同解，两宽度都验",
    );

    s.add(
        "零值语义：decode(0) 恒为成功，绝不产出错误码",
        decode_i64(0) == Decoded::Success && !decode_i64(0).is_error(),
        "把 0 当错误或把错误当 0 都是最危险的符号事故",
    );

    let mut pos_ok = true;
    let mut p = 0usize;
    while p < Errno::ALL.len() {
        if let Some(e) = Errno::ALL.get(p) {
            let d = decode_i64(e.val() as i64);
            if d != Decoded::Positive(e.val() as i64) || d.is_error() {
                pos_ok = false;
            }
        }
        p += 1;
    }
    s.add(
        "正数污染：13 个裸 errno 全判 Positive，禁止当错误码",
        pos_ok,
        "用户态忘取负时内核回得是正数；误判会把成功读成失败",
    );

    s.add(
        "范围边界：-4095 未知码、-4096 越界、i64::MIN 不可取负",
        decode_i64(-(MAX_ERRNO as i64)) == Decoded::Unknown(MAX_ERRNO)
            && decode_i64(-(MAX_ERRNO as i64 + 1)) == Decoded::OutOfRange
            && decode_i64(i64::MIN) == Decoded::Overflow
            && decode_i32(i32::MIN) == Decoded::OutOfRange,
        "四态边界必须逐点钉死；-4096 不是 errno，i64::MIN 连取负都做不到",
    );

    let mut ptr_ok = true;
    let mut q = 0usize;
    while q < Errno::ALL.len() {
        if let Some(e) = Errno::ALL.get(q) {
            let raw = encode_i64(*e);
            if !is_err_value(raw) {
                ptr_ok = false;
            }
        }
        q += 1;
    }
    ptr_ok = ptr_ok
        && !is_err_value(0)
        && !is_err_value(1)
        && !is_err_value(-(MAX_ERRNO as i64 + 1))
        && is_err_value(-1)
        && ERR_PTR_FLOOR == u64::MAX - MAX_ERRNO as u64 + 1;
    s.add(
        "err 指针面：编码值全落无符号 err 区，0/正数/越界负值全落区外",
        ptr_ok,
        "IS_ERR_VALUE 用无符号比较；有符号比较会把正数误判成错误",
    );

    let audit = chain_all();
    s.add(
        "链路四跳：13 码 raw→wrapper→libc 逐跳符号/幅值一致",
        audit.bad == 0
            && audit.records.len() == 13
            && chain(Errno::Enoent).libc == 2
            && chain(Errno::Eloop).libc == 40,
        "包装层只搬运不改符号；libc 只负责取正，链路不得二次编码",
    );

    let mut src_ok = B13_EPOCH == 13 && B13_TABLE.len() == 13;
    let mut r = 0usize;
    while r < B13_TABLE.len() {
        if let Some((code, raw)) = B13_TABLE.get(r) {
            if b13_encode(*code) != *raw {
                src_ok = false;
            }
            if *raw >= 0 || *raw < -(MAX_ERRNO as i64) {
                src_ok = false;
            }
        }
        r += 1;
    }
    s.add(
        "B13 单源：冻结表 13 行、epoch=13、逐行等于编码结果",
        src_ok,
        "编码器按表查值；表外码回 i64::MIN，解码必报 Overflow",
    );

    s.add(
        "分叉检测：正数与符号翻转的 raw 一律被审计器拒绝",
        !audit_raw(Errno::Ebadf, 9)
            && !audit_raw(Errno::Ebadf, -10)
            && audit_raw(Errno::Ebadf, -9)
            && !audit_raw(Errno::Ebadf, 0),
        "审计器是单源守门员：幅值对但符号错也必须红",
    );

    let l = run_corpus();
    s.add(
        "探针分桶：65 = 13 合法 + 26 拒绝 + 13 零值 + 13 链路",
        l.conserved()
            && l.probes == 65
            && l.encoded_ok == 13
            && l.rejected_bad == 26
            && l.success_zero == 13
            && l.chain_ok == 13
            && l.escaped == 0,
        "分桶总和必须等于发数；逃逸=正数被当错误码，必须为 0",
    );

    let mut trunc_ok = true;
    let mut t = 0usize;
    while t < Errno::ALL.len() {
        if let Some(e) = Errno::ALL.get(t) {
            let raw64 = encode_i64(*e);
            let raw32 = encode_i32(*e);
            if raw32 != -(e.val()) || raw64 as i32 != raw32 || decode_i32(raw32) != Decoded::Errno(*e)
            {
                trunc_ok = false;
            }
        }
        t += 1;
    }
    s.add(
        "宽度截断：64→32 位强转不丢符号与幅值",
        trunc_ok,
        "寄存器宽度更换时最易发生符号扩展错误；逐码双宽度复算",
    );

    s.add(
        "双跑一致：语料指纹两次相等且账本守恒",
        corpus_fingerprint() == corpus_fingerprint() && l.conserved(),
        "确定性的探针才能作为回归基线；指纹漂移即有隐藏状态",
    );

    let mut text_ok = true;
    let mut u = 0usize;
    while u < Errno::ALL.len() {
        if let Some(e) = Errno::ALL.get(u) {
            if e.name().is_empty() || e.detail().is_empty() {
                text_ok = false;
            }
            let mut v = u + 1;
            while v < Errno::ALL.len() {
                if let Some(f) = Errno::ALL.get(v) {
                    if e.name() == f.name() {
                        text_ok = false;
                    }
                }
                v += 1;
            }
        }
        u += 1;
    }
    s.add(
        "文案闭集：13 个名字/建议非空且名字互异",
        text_ok,
        "错误码文案是排障第一入口；重名等于没有诊断",
    );

    s.add(
        "成功语义不被污染：编码区不含 0，解码 0 不进错误分支",
        !Errno::ALL.iter().copied().any(|e| e.val() == 0)
            && decode_i64(0) == Decoded::Success
            && !is_err_value(0),
        "0 同时是「无错误」和「空指针」；两条语义都不得被编码器碰",
    );

    let corpus_text = format!(
        "65发 合法{}/拒绝{}/零{}/链路{}",
        l.encoded_ok, l.rejected_bad, l.success_zero, l.chain_ok
    );
    s.add_dyn("探针账快照", l.conserved() && l.probes == 65, &corpus_text);

    let chain_text = format!(
        "链路 13码/坏{}/表{}行/epoch{}",
        audit.bad,
        B13_TABLE.len(),
        B13_EPOCH
    );
    s.add_dyn(
        "链路快照",
        audit.bad == 0 && B13_TABLE.len() == 13 && B13_EPOCH == 13,
        &chain_text,
    );

    let fp = corpus_fingerprint();
    let fp_text = format!("语料指纹 0x{:08X}", fp);
    s.add_dyn("指纹快照", fp == corpus_fingerprint(), &fp_text);

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seg_base_guard() {
        assert_eq!(SEG_BASE, 0x7A7C0);
        assert_eq!(SEG_BASE % 0x40, 0);
        assert!((0x7A000..=0x7D1C0).contains(&SEG_BASE));
    }

    #[test]
    fn errno_closed_set() {
        assert_eq!(Errno::ALL.len(), 13);
        for e in Errno::ALL.iter().copied() {
            assert!(e.val() > 0 && e.val() <= MAX_ERRNO);
            assert_eq!(Errno::from_val(e.val()), Some(e));
            assert!(!e.name().is_empty());
            assert!(!e.detail().is_empty());
        }
        assert_eq!(Errno::from_val(0), None);
        assert_eq!(Errno::from_val(-9), None);
        assert_eq!(Errno::Enoent.val(), 2);
        assert_eq!(Errno::Eloop.val(), 40);
    }

    #[test]
    fn encode_each_code_negative() {
        for e in Errno::ALL.iter().copied() {
            assert_eq!(encode_i64(e), -(e.val() as i64));
            assert!(encode_i64(e) < 0);
            assert_eq!(encode_i32(e), -e.val());
            assert!(is_err_value(encode_i64(e)));
        }
    }

    #[test]
    fn decode_roundtrip_both_widths() {
        for e in Errno::ALL.iter().copied() {
            assert_eq!(decode_i64(encode_i64(e)), Decoded::Errno(e));
            assert_eq!(decode_i32(encode_i32(e)), Decoded::Errno(e));
            assert!(decode_i64(encode_i64(e)).is_error());
        }
    }

    #[test]
    fn zero_and_positive_semantics() {
        assert_eq!(decode_i64(0), Decoded::Success);
        assert!(!decode_i64(0).is_error());
        assert!(!Decoded::Success.label().is_empty());
        assert!(!Decoded::Positive(9).label().is_empty());
        assert!(!Decoded::OutOfRange.label().is_empty());
        assert!(!Decoded::Overflow.label().is_empty());
        for e in Errno::ALL.iter().copied() {
            assert_eq!(decode_i64(e.val() as i64), Decoded::Positive(e.val() as i64));
            assert!(!decode_i64(e.val() as i64).is_error());
        }
    }

    #[test]
    fn range_edges() {
        assert_eq!(decode_i64(-2), Decoded::Errno(Errno::Enoent));
        assert_eq!(decode_i64(-4095), Decoded::Unknown(4095));
        assert_eq!(decode_i64(-4096), Decoded::OutOfRange);
        assert_eq!(decode_i64(-100000), Decoded::OutOfRange);
        assert_eq!(decode_i64(i64::MIN), Decoded::Overflow);
        assert_eq!(decode_i32(i32::MIN), Decoded::OutOfRange);
    }

    #[test]
    fn err_ptr_guard() {
        assert!(is_err_value(-1));
        assert!(is_err_value(-MAX_ERRNO as i64));
        assert!(!is_err_value(0));
        assert!(!is_err_value(1));
        assert!(!is_err_value(-(MAX_ERRNO as i64) - 1));
        assert_eq!(ERR_PTR_FLOOR, u64::MAX - MAX_ERRNO as u64 + 1);
    }

    #[test]
    fn chain_single_source() {
        let audit = chain_all();
        assert_eq!(audit.bad, 0);
        assert_eq!(audit.records.len(), 13);
        for rec in audit.records.iter() {
            assert!(rec.ok);
            assert_eq!(rec.raw, rec.wrapper);
            assert_eq!(rec.libc, rec.e.val());
            assert_eq!(rec.raw, -(rec.e.val() as i64));
        }
        assert_eq!(B13_EPOCH, 13);
        assert_eq!(B13_TABLE.len(), 13);
        for (code, raw) in B13_TABLE.iter() {
            assert_eq!(b13_encode(*code), *raw);
        }
        assert_eq!(b13_encode(Errno::Ebadf), -9);
    }

    #[test]
    fn rogue_encodings_rejected() {
        assert!(audit_raw(Errno::Ebadf, -9));
        assert!(!audit_raw(Errno::Ebadf, 9));
        assert!(!audit_raw(Errno::Ebadf, -10));
        assert!(!audit_raw(Errno::Ebadf, 0));
        assert!(!audit_raw(Errno::Ebadf, -4096));
    }

    #[test]
    fn corpus_partition() {
        let l = run_corpus();
        assert_eq!(l.probes, 65);
        assert_eq!(l.encoded_ok, 13);
        assert_eq!(l.rejected_bad, 26);
        assert_eq!(l.success_zero, 13);
        assert_eq!(l.chain_ok, 13);
        assert_eq!(l.mismatches, 0);
        assert_eq!(l.escaped, 0);
        assert_eq!(l.chain_bad, 0);
        assert!(l.conserved());
    }

    #[test]
    fn deterministic_corpus() {
        assert_eq!(corpus_fingerprint(), corpus_fingerprint());
        assert_eq!(run_corpus().probes, run_corpus().probes);
    }

    #[test]
    fn checks_all_green() {
        let s = run_unx9233_checks();
        assert!(s.all_passed(), "{}", s.red_items_text());
        assert!(s.len() >= 12);
        assert!(!s.truncated());
    }
}
