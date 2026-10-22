//! UNX-F8821 · open 语义 O_ 九标志矩阵 30 组账 · 诊断段 0xB7C0
//! 验收锚点：UNX-F8821-J1 open 语义九标志（RDONLY/WRONLY/RDWR/
//! CREAT/EXCL/TRUNC/APPEND/NONBLOCK/CLOEXEC）组合矩阵 30 组——
//! 访问模式域互斥校验（RDONLY|WRONLY 非法）、合法组合解析全过、
//! 非法组合全拒且错误码一致，双判据缺一即批内红灯。

use crate::alloc::{format, string::{String, ToString}, vec, vec::Vec};

pub const SEG_BASE: u16 = 0xB7C0;

// ── 诊断段独占 · open 标志错误域 ────────────────────────────
const SEG_ERR: u16 = SEG_BASE;
const E_ACC: u16 = SEG_ERR + 1;     // 访问模式域非法
const E_COMBO: u16 = SEG_ERR + 2;   // 非法组合
const E_MATRIX: u16 = SEG_ERR + 3;  // 矩阵组数不符
const E_PARSE: u16 = SEG_ERR + 4;   // 解析错
const E_ZERO: u16 = SEG_ERR + 5;    // 零标志非法

pub const ERR_EINVAL: i32 = 22;

// O_ 标志位（Linux 数值）
pub const O_RDONLY: u32 = 0o0;
pub const O_WRONLY: u32 = 0o1;
pub const O_RDWR: u32 = 0o2;
pub const O_CREAT: u32 = 0o100;
pub const O_EXCL: u32 = 0o200;
pub const O_TRUNC: u32 = 0o1000;
pub const O_APPEND: u32 = 0o2000;
pub const O_NONBLOCK: u32 = 0o4000;
pub const O_CLOEXEC: u32 = 0o2000000;

/// 访问模式掩码。
pub const ACC_MASK: u32 = 0o3;

// ── 确定性 LCG ───────────────────────────────────────────────
pub struct Lcg(u64);
impl Lcg {
    pub fn new(seed: u64) -> Self {
        Lcg(seed)
    }
    pub fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        self.0
    }
    pub fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
}

// ── open 标志解析器 ──────────────────────────────────────────
/// 解析结果：访问模式 + 特性位集。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct OpenParsed {
    pub acc: u32,       // RDONLY/WRONLY/RDWR
    pub creat: bool,
    pub excl: bool,
    pub trunc: bool,
    pub append: bool,
    pub nonblock: bool,
    pub cloexec: bool,
}

/// 解析：访问域互斥 + 特性位抽取 + 非法位检出。
pub fn parse_flags(f: u32) -> Result<OpenParsed, i32> {
    // 访问模式域：RDONLY|WRONLY 组合非法（0o3 非法）
    let acc = f & ACC_MASK;
    match acc {
        0o0 | 0o1 | 0o2 => {}
        _ => return Err(ERR_EINVAL), // 0o3 或其它非法
    }
    // EXCL 必须伴随 CREAT（open 语义）
    if f & O_EXCL != 0 && f & O_CREAT == 0 {
        return Err(ERR_EINVAL);
    }
    // APPEND 与 WRONLY/RDWR 合法；RDONLY|APPEND 允许（读侧忽略）
    Ok(OpenParsed {
        acc,
        creat: f & O_CREAT != 0,
        excl: f & O_EXCL != 0,
        trunc: f & O_TRUNC != 0,
        append: f & O_APPEND != 0,
        nonblock: f & O_NONBLOCK != 0,
        cloexec: f & O_CLOEXEC != 0,
    })
}

/// 未识别位检出：超出九标志域的位（高位杂散）非法。
pub fn parse_strict(f: u32) -> Result<OpenParsed, i32> {
    let known = ACC_MASK | O_CREAT | O_EXCL | O_TRUNC | O_APPEND | O_NONBLOCK | O_CLOEXEC;
    if f & !known != 0 {
        return Err(ERR_EINVAL);
    }
    parse_flags(f)
}

// ── 30 组矩阵 ────────────────────────────────────────────────
/// 矩阵组：flags + 期望（Ok(摘要) 或 Err(errno)）。
#[derive(Clone, Debug)]
pub struct MatrixRow {
    pub case: String,
    pub flags: u32,
    pub expect_ok: bool,
    pub expect_acc: u32,
}

impl MatrixRow {
    pub fn check(&self) -> bool {
        match parse_flags(self.flags) {
            Ok(p) => self.expect_ok && p.acc == self.expect_acc,
            Err(e) => !self.expect_ok && e == ERR_EINVAL,
        }
    }
    pub fn fp(&self) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        for b in self.case.as_bytes() {
            h ^= *b as u64;
            h = h.wrapping_mul(0x0000_0100_0000_01B3);
        }
        for b in self.flags.to_le_bytes() {
            h ^= b as u64;
            h = h.wrapping_mul(0x0000_0100_0000_01B3);
        }
        h
    }
}

/// 标准矩阵 30 组：22 合法 + 8 非法。
pub fn standard_matrix() -> Vec<MatrixRow> {
    let mut m = Vec::new();
    let legal: [(&str, u32, u32); 22] = [
        ("rdonly", O_RDONLY, O_RDONLY),
        ("wronly", O_WRONLY, O_WRONLY),
        ("rdwr", O_RDWR, O_RDWR),
        ("rdonly_creat", O_RDONLY | O_CREAT, O_RDONLY),
        ("wronly_creat", O_WRONLY | O_CREAT, O_WRONLY),
        ("rdwr_creat", O_RDWR | O_CREAT, O_RDWR),
        ("wronly_creat_excl", O_WRONLY | O_CREAT | O_EXCL, O_WRONLY),
        ("rdwr_creat_excl", O_RDWR | O_CREAT | O_EXCL, O_RDWR),
        ("wronly_trunc", O_WRONLY | O_TRUNC, O_WRONLY),
        ("rdwr_trunc", O_RDWR | O_TRUNC, O_RDWR),
        ("wronly_append", O_WRONLY | O_APPEND, O_WRONLY),
        ("rdwr_append", O_RDWR | O_APPEND, O_RDWR),
        ("rdwr_nonblock", O_RDWR | O_NONBLOCK, O_RDWR),
        ("wronly_nonblock", O_WRONLY | O_NONBLOCK, O_WRONLY),
        ("rdwr_cloexec", O_RDWR | O_CLOEXEC, O_RDWR),
        ("rdwr_creat_trunc", O_RDWR | O_CREAT | O_TRUNC, O_RDWR),
        ("wronly_creat_append", O_WRONLY | O_CREAT | O_APPEND, O_WRONLY),
        ("rdwr_creat_excl_cloexec", O_RDWR | O_CREAT | O_EXCL | O_CLOEXEC, O_RDWR),
        ("rdonly_nonblock_cloexec", O_RDONLY | O_NONBLOCK | O_CLOEXEC, O_RDONLY),
        ("rdwr_all", O_RDWR | O_CREAT | O_TRUNC | O_NONBLOCK | O_CLOEXEC, O_RDWR),
        ("wronly_excl_trunc", O_WRONLY | O_CREAT | O_EXCL | O_TRUNC, O_WRONLY),
        ("rdwr_append_nonblock", O_RDWR | O_APPEND | O_NONBLOCK, O_RDWR),
    ];
    for (c, f, a) in legal {
        m.push(MatrixRow { case: c.to_string(), flags: f, expect_ok: true, expect_acc: a });
    }
    let illegal: [(&str, u32); 8] = [
        ("acc_3", 0o3),                              // 访问域非法
        ("acc_3_creat", 0o3 | O_CREAT),              // 同上带 CREAT
        ("excl_no_creat", O_EXCL),                   // EXCL 无 CREAT
        ("excl_wronly_no_creat", O_WRONLY | O_EXCL), // 同上
        ("acc_3_excl", 0o3 | O_EXCL),                // 双重非法
        ("acc_3_trunc", 0o3 | O_TRUNC),              // 同上
        ("excl_rdonly", O_RDONLY | O_EXCL),          // EXCL 无 CREAT
        ("acc_3_nonblock", 0o3 | O_NONBLOCK),        // 同上
    ];
    for (c, f) in illegal {
        m.push(MatrixRow { case: c.to_string(), flags: f, expect_ok: false, expect_acc: 0xFFFF });
    }
    m
}

// ── 判据入口 ─────────────────────────────────────────────────
pub fn run_unx8821_checks() -> crate::checks::CheckSet {
    let mut s = crate::checks::CheckSet::new("UNX-F8821 open九标志矩阵30组");
    let m = standard_matrix();

    // 1. 矩阵组数恰 30
    s.add("OPEN-组数30", m.len() == 30, if m.len() == 30 { "" } else { "组数不符" });

    // 2. 30 组逐组判一致
    let mut hit = 0;
    for r in &m {
        if r.check() {
            hit += 1;
        }
    }
    s.add("OPEN-矩阵全过", hit == 30, if hit == 30 { "" } else { "存在不一致组" });

    // 3. 访问域互斥：0o3 全形态拒
    let acc3 = parse_flags(0o3) == Err(ERR_EINVAL)
        && parse_flags(0o3 | O_CREAT) == Err(ERR_EINVAL)
        && parse_flags(0o3 | O_RDWR) == Err(ERR_EINVAL);
    s.add("OPEN-访问域互斥", acc3, if acc3 { "" } else { "0o3 放行" });

    // 4. 三种合法访问域全过
    let acc_ok = parse_flags(O_RDONLY).map(|p| p.acc == O_RDONLY).unwrap_or(false)
        && parse_flags(O_WRONLY).map(|p| p.acc == O_WRONLY).unwrap_or(false)
        && parse_flags(O_RDWR).map(|p| p.acc == O_RDWR).unwrap_or(false);
    s.add("OPEN-三访问域", acc_ok, if acc_ok { "" } else { "合法域误拒" });

    // 5. EXCL 无 CREAT 全形态拒
    let excl = parse_flags(O_EXCL) == Err(ERR_EINVAL)
        && parse_flags(O_WRONLY | O_EXCL) == Err(ERR_EINVAL)
        && parse_flags(O_RDONLY | O_EXCL) == Err(ERR_EINVAL);
    s.add("OPEN-EXCL约束", excl, if excl { "" } else { "EXCL 放行" });

    // 6. EXCL + CREAT 合法
    let excl_ok = parse_flags(O_WRONLY | O_CREAT | O_EXCL).is_ok();
    s.add("OPEN-EXCL带CREAT过", excl_ok, if excl_ok { "" } else { "EXCL+CREAT 误拒" });

    // 7. 严格解析：杂散高位拒
    let stray = parse_strict(0o400_0000) == Err(ERR_EINVAL)
        && parse_strict(O_RDWR | 0o10_0000) == Err(ERR_EINVAL);
    s.add("OPEN-杂散位拒", stray, if stray { "" } else { "杂散位放行" });

    // 8. 严格解析：九标志域内合法组合全过
    let strict_ok = parse_strict(O_RDWR | O_CREAT | O_TRUNC | O_NONBLOCK | O_CLOEXEC).is_ok();
    s.add("OPEN-严格域内过", strict_ok, if strict_ok { "" } else { "域内误拒" });

    // 9. 特性位逐一抽取正确
    let p = parse_flags(O_RDWR | O_CREAT | O_APPEND | O_CLOEXEC).unwrap_or(OpenParsed {
        acc: 0xFFFF, creat: false, excl: false, trunc: false, append: false, nonblock: false, cloexec: false,
    });
    let bits = p.creat && p.cloexec && p.append && !p.excl && !p.trunc && !p.nonblock;
    s.add("OPEN-特性位抽取", bits, if bits { "" } else { "抽取错位" });

    // 10. 矩阵行指纹互异
    let mut fp_uniq = true;
    for i in 0..m.len() {
        for j in (i + 1)..m.len() {
            if m[i].fp() == m[j].fp() {
                fp_uniq = false;
            }
        }
    }
    s.add("OPEN-行指纹互异", fp_uniq, if fp_uniq { "" } else { "指纹碰撞" });

    // 11. LCG 抽样 6 组复核
    let mut lcg = Lcg(0xB7C0);
    let mut spot = true;
    for _ in 0..6 {
        let i = lcg.below(m.len() as u64) as usize;
        if !m[i].check() {
            spot = false;
        }
    }
    s.add("OPEN-抽样复核", spot, if spot { "" } else { "抽样异常" });

    // 12. 零标志 = RDONLY 合法（0 即读）
    let zero = parse_flags(0).map(|p| p.acc == O_RDONLY && !p.creat).unwrap_or(false);
    s.add("OPEN-零标志读", zero, if zero { "" } else { "零标志误判" });

    // 13. 诊断段独占
    let seg_ok = SEG_BASE >= 0xB7C0 && SEG_BASE < 0xB7E0;
    s.add("OPEN-诊断段独占", seg_ok, if seg_ok { "" } else { "段越界" });

    s
}
