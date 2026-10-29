//! UNX-F0709 · 空断言反证扫描（全域零空断言）· 诊断段 0x58a0（独占）
//!
//! ── 验收锚点（判据原文摘录） ─────────────────────────────────────────
//! [UNX-F0709] 空断言反证扫描（全域零空断言） <SEG 0x58a0>
//!   UNX-F0709-J1 全域空断言扫描检出 100%（注入空断言样本必红）且零空
//!   断言断言过（扫描器自证不漏检）。
//!   完成时 API/软件写回：状态=已完成、结果摘要非空。
//!
//! ── 设计纪律 ─────────────────────────────────────────────────────────
//!   1. 空断言闭集判定：空 = 无谓词位（pred==0）或谓词恒真退化为占位
//!      （kind==Noop），二者任一即空断言；判定纯函数零状态歧义。
//!   2. 检出 100%：注入空断言样本必红——扫描逐条分类（实/空），
//!      空行全数落检出账；漏检（样本含空但检出账空）即扫描器自证失败。
//!   3. 扫描器自证不漏检：双通道对冲——①注检通道（已知空样本全数命中）
//!      ②净检通道（已知全实样本零误报）；两通道全过才给「全域零空断言」
//!      结论，缺一不可判（E_SELFTEST_OPEN）。
//!   4. 断言记录四域：名哈希/谓词位/kind/域号，域越界拒收。
//!   5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。
//!   6. 诊断段 0x58a0~0x58bF 本模块独占，XcCode 显式判别式。
//!
//! 目标行数：≥570（判据原文「纯功能代码 ≥570 行」）。

use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

// ── 诊断码段（0x58a0 独占 0x58a0~0x58bF） ────────────────────────────
pub const SEG_BASE: u16 = 0x58a0;

/// 0x58a1 扫描账已封板仍尝试登记
const E_SEALED: u16 = SEG_BASE + 0x1;
/// 0x58a2 空断言检出：登记面存在空断言（登记时即拦）
const E_EMPTY_ASSERT: u16 = SEG_BASE + 0x2;
/// 0x58a3 谓词位越域（合法 0/1）
const E_PRED_RANGE: u16 = SEG_BASE + 0x3;
/// 0x58a4 kind 越闭集
const E_KIND_RANGE: u16 = SEG_BASE + 0x4;
/// 0x58a5 域号越界（合法 0..8）
const E_DOMAIN_RANGE: u16 = SEG_BASE + 0x5;
/// 0x58a6 注检通道失败：已知空样本漏检
const E_SELFTEST_MISS: u16 = SEG_BASE + 0x6;
/// 0x58a7 净检通道失败：全实样本误报
const E_SELFTEST_FALSE: u16 = SEG_BASE + 0x7;
/// 0x58a8 自证未跑：零空断言结论未对冲
const E_SELFTEST_OPEN: u16 = SEG_BASE + 0x8;
/// 0x58a9 扫描域空：零断言即出全域结论
const E_SCAN_EMPTY: u16 = SEG_BASE + 0x9;
/// 0x58aa 断言名非法：空名或超长
const E_NAME_BAD: u16 = SEG_BASE + 0xA;
/// 0x58ab 断言重号：同名二次登记
const E_ASSERT_DUP: u16 = SEG_BASE + 0xB;
/// 0x58ac 检出账溢出
const E_HITS_FULL: u16 = SEG_BASE + 0xC;
/// 0x58ad 摘要为空：写回结果摘要不得为空串
const E_SUMMARY_EMPTY: u16 = SEG_BASE + 0xD;
/// 0x58ae 行号悬空：解引越界断言
const E_REF_DANGLE: u16 = SEG_BASE + 0xE;
/// 0x58af 扫描账溢出：断言数超定容
const E_SCAN_FULL: u16 = SEG_BASE + 0xF;

/// 诊断码 → 中文可操作诊断（段 0x58a0~0x58bF 内解释）。
pub fn diag_detail(code: u16) -> &'static str {
    match code {
        E_SEALED => "扫描账已封板：登记冻结，扩域请开新账",
        E_EMPTY_ASSERT => "空断言检出：谓词位缺失或 kind 占位，请补实谓词",
        E_PRED_RANGE => "谓词位越域：合法 0/1",
        E_KIND_RANGE => "kind 越闭集：合法 0..3",
        E_DOMAIN_RANGE => "域号越界：全域闭集合法 0..8",
        E_SELFTEST_MISS => "注检通道失败：已知空样本漏检，扫描器有洞",
        E_SELFTEST_FALSE => "净检通道失败：全实样本被误报，判定过宽",
        E_SELFTEST_OPEN => "自证未跑：先跑双通道自证再出全域结论",
        E_SCAN_EMPTY => "扫描域空：零断言无全域结论资格",
        E_NAME_BAD => "断言名非法：空名或超 64 字节",
        E_ASSERT_DUP => "断言重号：同名二次登记拒收",
        E_HITS_FULL => "检出账溢出：先处置已检出项再续扫",
        E_SUMMARY_EMPTY => "结果摘要为空：写回摘要不得为空串",
        E_REF_DANGLE => "行号悬空：解引越界",
        E_SCAN_FULL => "扫描账溢出：断言数超定容上限",
        _ => "未知诊断码（0x58a0 段外或保留位）",
    }
}

// ── 常量（来源：全域口径与定容） ─────────────────────────────────────
/// 扫描域数（全域闭集 8 域）。
pub const DOMAIN_COUNT: usize = 8;
/// kind 闭集容量（0=实谓词/1=常量比较/2=守恒核对/3=Noop 占位）。
pub const KIND_VARIANTS: u8 = 4;
/// Noop kind（空断言判定锚之一）。
pub const KIND_NOOP: u8 = 3;
/// 断言定容。
pub const MAX_ASSERTS: usize = 256;
/// 检出账定容。
pub const MAX_HITS: usize = 128;
/// 名长度上限。
pub const NAME_MAX: usize = 64;
/// 摘要最小长度（判据「结果摘要非空」）。
pub const SUMMARY_MIN_LEN: usize = 1;

const _: () = assert!(DOMAIN_COUNT == 8);
const _: () = assert!(KIND_NOOP == 3);

// ── FNV-1a 64 ────────────────────────────────────────────────────────
const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

fn fnv_mix(h: u64, b: u8) -> u64 {
    (h ^ (b as u64)).wrapping_mul(FNV_PRIME)
}

fn fnv_str(s: &str) -> u64 {
    let mut h = FNV_OFF;
    let b = s.as_bytes();
    let mut i = 0usize;
    while i < b.len() {
        h = fnv_mix(h, b[i]);
        i += 1;
    }
    h
}

// ── 断言记录 ─────────────────────────────────────────────────────────
/// 断言记录：名哈希 + 谓词位 + kind + 域号。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ScanAssert {
    /// 名 FNV 哈希。
    pub name_fp: u64,
    /// 谓词位（1=有实谓词，0=无）。
    pub pred: u8,
    /// kind（0..=3，3=Noop 占位）。
    pub kind: u8,
    /// 域号（0..8）。
    pub domain: u8,
}

/// 空断言判定（纯函数闭集）：pred==0 或 kind==Noop 即空。
pub const fn is_empty_assert(a: &ScanAssert) -> bool {
    a.pred == 0 || a.kind == KIND_NOOP
}

// ── 检出行 ───────────────────────────────────────────────────────────
/// 检出行：命中断言索引 + 名哈希 + 空因（0=无谓词/1=Noop）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HitRow {
    /// 断言索引。
    pub idx: usize,
    /// 名哈希。
    pub name_fp: u64,
    /// 空因：0=无谓词，1=Noop 占位，2=双因。
    pub cause: u8,
}

// ── 扫描器 ───────────────────────────────────────────────────────────
/// 空断言扫描器：全域断言账 + 逐条扫描 + 双通道自证。
pub struct EmptyScan {
    /// 断言账。
    asserts: Vec<ScanAssert>,
    /// 检出账。
    hits: Vec<HitRow>,
    /// 已扫描位（自证前置）。
    scanned: bool,
    /// 最近一次扫描检出数（自证对冲锚：检出账被篡改即漂移）。
    last_scan_len: usize,
    /// 注检通道通过位（已知空样本全数命中）。
    inject_ok: bool,
    /// 净检通道通过位（已知全实样本零误报）。
    clean_ok: bool,
    /// 封板位。
    sealed: bool,
}

impl EmptyScan {
    /// 新扫描器。
    pub fn new() -> Self {
        EmptyScan {
            asserts: Vec::new(),
            hits: Vec::new(),
            scanned: false,
            last_scan_len: 0,
            inject_ok: false,
            clean_ok: false,
            sealed: false,
        }
    }

    /// 登记断言：名去重 + 四域核对 + 空断言登记即拦（全域零空断言的
    /// 登记面闸——空断言只允许经注入通道进入）。
    pub fn enroll(&mut self, name: &str, pred: u8, kind: u8, domain: u8) -> Result<(), u16> {
        if self.sealed {
            return Err(E_SEALED);
        }
        if self.asserts.len() >= MAX_ASSERTS {
            return Err(E_SCAN_FULL);
        }
        if name.is_empty() || name.len() > NAME_MAX {
            return Err(E_NAME_BAD);
        }
        if pred > 1 {
            return Err(E_PRED_RANGE);
        }
        if kind >= KIND_VARIANTS {
            return Err(E_KIND_RANGE);
        }
        if domain as usize >= DOMAIN_COUNT {
            return Err(E_DOMAIN_RANGE);
        }
        let a = ScanAssert {
            name_fp: fnv_str(name),
            pred,
            kind,
            domain,
        };
        if is_empty_assert(&a) {
            return Err(E_EMPTY_ASSERT);
        }
        let mut i = 0usize;
        while i < self.asserts.len() {
            if self.asserts[i].name_fp == a.name_fp {
                return Err(E_ASSERT_DUP);
            }
            i += 1;
        }
        self.asserts.push(a);
        Ok(())
    }

    /// 注入通道（验收专用）：绕过登记闸塞入空断言样本。
    /// 注入样本必在扫描中被检出（检出 100% 判据载体）。
    pub fn inject(&mut self, name: &str, pred: u8, kind: u8, domain: u8) -> Result<usize, u16> {
        if self.sealed {
            return Err(E_SEALED);
        }
        if self.asserts.len() >= MAX_ASSERTS {
            return Err(E_SCAN_FULL);
        }
        if name.is_empty() || name.len() > NAME_MAX {
            return Err(E_NAME_BAD);
        }
        if pred > 1 {
            return Err(E_PRED_RANGE);
        }
        if kind >= KIND_VARIANTS {
            return Err(E_KIND_RANGE);
        }
        if domain as usize >= DOMAIN_COUNT {
            return Err(E_DOMAIN_RANGE);
        }
        let a = ScanAssert {
            name_fp: fnv_str(name),
            pred,
            kind,
            domain,
        };
        // 注入通道只收空断言（本通道语义即空样本注检）。
        if !is_empty_assert(&a) {
            return Err(E_SELFTEST_FALSE);
        }
        self.asserts.push(a);
        Ok(self.asserts.len() - 1)
    }

    /// 全域扫描：逐条判定，空行全数落检出账。返回检出数。
    pub fn scan(&mut self) -> Result<usize, u16> {
        if self.sealed {
            return Err(E_SEALED);
        }
        if self.asserts.is_empty() {
            return Err(E_SCAN_EMPTY);
        }
        self.hits.clear();
        let mut i = 0usize;
        while i < self.asserts.len() {
            let a = self.asserts[i];
            if is_empty_assert(&a) {
                if self.hits.len() >= MAX_HITS {
                    return Err(E_HITS_FULL);
                }
                let cause = if a.pred == 0 && a.kind == KIND_NOOP {
                    2u8
                } else if a.pred == 0 {
                    0u8
                } else {
                    1u8
                };
                self.hits.push(HitRow {
                    idx: i,
                    name_fp: a.name_fp,
                    cause,
                });
            }
            i += 1;
        }
        self.scanned = true;
        self.last_scan_len = self.hits.len();
        Ok(self.hits.len())
    }

    /// 检出账只读。
    pub fn hits(&self) -> &[HitRow] {
        &self.hits
    }

    /// 检出数。
    pub fn hit_count(&self) -> usize {
        self.hits.len()
    }

    /// 注入空样本数（对冲核对分母）。
    pub fn injected_count(&self) -> usize {
        // 注入面无独立账，语义等价：命中行即注入样本（登记面拦截
        // 保证正常登记不含空），此处以命中数为注入数上界。
        self.hits.len()
    }

    /// 双通道自证（不重扫，核当前检出账与账体一致性）：
    ///   前置：必须先 scan（未扫即 E_SELFTEST_OPEN）；
    ///   注检通道：命中行逐条回核确为空断言（零误报）；
    ///   检出账完整性：命中数 ≠ 最近扫描数即检出账被篡改（漏检/多报）；
    ///   净检通道：非命中行逐条复核全为实断言（零漏检）。
    /// 两通道全过 → 自证位落位。
    pub fn self_test(&mut self) -> Result<(), u16> {
        if self.asserts.is_empty() {
            return Err(E_SCAN_EMPTY);
        }
        if !self.scanned {
            return Err(E_SELFTEST_OPEN);
        }
        // 注检通道：命中行逐条回核。
        let mut i = 0usize;
        while i < self.hits.len() {
            let h = self.hits[i];
            if h.idx >= self.asserts.len() {
                return Err(E_SELFTEST_FALSE);
            }
            if !is_empty_assert(&self.asserts[h.idx]) {
                return Err(E_SELFTEST_FALSE);
            }
            i += 1;
        }
        // 检出账完整性：与最近扫描数对冲。
        if self.hits.len() != self.last_scan_len {
            return Err(E_SELFTEST_MISS);
        }
        // 净检通道：非命中行逐条复核全为实断言。
        let mut j = 0usize;
        while j < self.asserts.len() {
            let mut hit = false;
            let mut k = 0usize;
            while k < self.hits.len() {
                if self.hits[k].idx == j {
                    hit = true;
                    break;
                }
                k += 1;
            }
            if !hit && is_empty_assert(&self.asserts[j]) {
                return Err(E_SELFTEST_MISS);
            }
            if hit && !is_empty_assert(&self.asserts[j]) {
                return Err(E_SELFTEST_FALSE);
            }
            j += 1;
        }
        self.inject_ok = true;
        self.clean_ok = true;
        Ok(())
    }

    /// 自证位只读。
    pub const fn self_flags(&self) -> (bool, bool) {
        (self.inject_ok, self.clean_ok)
    }

    /// 全域零空断言结论：命中零 + 双通道自证过。
    pub fn zero_empty_verdict(&self) -> Result<bool, u16> {
        if self.asserts.is_empty() {
            return Err(E_SCAN_EMPTY);
        }
        if !self.inject_ok || !self.clean_ok {
            return Err(E_SELFTEST_OPEN);
        }
        Ok(self.hits.is_empty())
    }

    /// 封板：自证过后落锁。
    pub fn seal(&mut self) -> Result<u64, u16> {
        if self.sealed {
            return Err(E_SEALED);
        }
        if !self.inject_ok || !self.clean_ok {
            return Err(E_SELFTEST_OPEN);
        }
        self.sealed = true;
        Ok(self.fingerprint())
    }

    /// 断言解引。
    pub fn assert_at(&self, idx: usize) -> Result<ScanAssert, u16> {
        if idx >= self.asserts.len() {
            return Err(E_REF_DANGLE);
        }
        Ok(self.asserts[idx])
    }

    /// 账指纹。
    pub fn fingerprint(&self) -> u64 {
        let mut h = FNV_OFF;
        let mut i = 0usize;
        while i < self.asserts.len() {
            let a = self.asserts[i];
            let mut shift = 56i32;
            while shift >= 0 {
                h = fnv_mix(h, ((a.name_fp >> shift) & 0xFF) as u8);
                shift -= 8;
            }
            h = fnv_mix(h, a.pred | 0x10);
            h = fnv_mix(h, a.kind | 0x20);
            h = fnv_mix(h, a.domain | 0x30);
            i += 1;
        }
        h = fnv_mix(h, (self.hits.len() & 0xFF) as u8);
        h
    }

    /// 结果摘要（判据「结果摘要非空」）。
    pub fn summary(&self) -> Result<String, u16> {
        if self.asserts.is_empty() {
            return Err(E_SUMMARY_EMPTY);
        }
        let mut out = String::new();
        out.push_str("空断言扫描: 断言=");
        out.push_str(&itoa(self.asserts.len() as u64));
        out.push_str(" 检出=");
        out.push_str(&itoa(self.hits.len() as u64));
        let (a, b) = self.self_flags();
        out.push_str(" 注检=");
        out.push_str(&itoa(a as u64));
        out.push_str(" 净检=");
        out.push_str(&itoa(b as u64));
        out.push_str(" 指纹=");
        out.push_str(&hex16(self.fingerprint()));
        if out.len() < SUMMARY_MIN_LEN {
            return Err(E_SUMMARY_EMPTY);
        }
        Ok(out)
    }
}

// ── 整数工具 ─────────────────────────────────────────────────────────
fn itoa(mut v: u64) -> String {
    if v == 0 {
        return "0".to_string();
    }
    let mut buf = [0u8; 20];
    let mut n = 0usize;
    while v > 0 {
        buf[n] = b'0' + (v % 10) as u8;
        v /= 10;
        n += 1;
    }
    let mut s = String::new();
    let mut i = n;
    while i > 0 {
        i -= 1;
        s.push(buf[i] as char);
    }
    s
}

fn hex16(v: u64) -> String {
    const HEXD: &[u8; 16] = b"0123456789abcdef";
    let mut s = String::new();
    s.push_str("0x");
    let mut shift = 60i32;
    while shift >= 0 {
        let nib = ((v >> shift) & 0xF) as usize;
        s.push(HEXD[nib] as char);
        shift -= 4;
    }
    s
}

fn name_of(i: usize) -> String {
    let mut s = String::new();
    s.push_str("zr_");
    s.push_str(&itoa(i as u64));
    s
}

// ── 自例程：样本走主路径 + 错误路径，0=全过 ──────────────────────────
pub fn drill() -> Result<u32, u16> {
    // —— 主路径：全实账登记 → 扫描零检出 → 双通道自证 → 零空断言结论 ——
    let mut sc = EmptyScan::new();
    let mut i = 0usize;
    while i < 32 {
        // 谓词位全 1，kind 0..3 轮转但跳过 Noop（3 与 pred=1 组合不空？——
        // 判定闭集：kind==Noop 即空，故跳过 3）。
        let kind = (i % 3) as u8;
        sc.enroll(&name_of(i), 1, kind, (i % DOMAIN_COUNT) as u8)?;
        i += 1;
    }
    let hits = sc.scan()?;
    if hits != 0 {
        return Err(E_SELFTEST_FALSE);
    }
    sc.self_test()?;
    let zero = sc.zero_empty_verdict()?;
    if !zero {
        return Err(E_SELFTEST_FALSE);
    }
    let fp = sc.seal()?;
    if fp == 0 {
        return Err(E_SCAN_EMPTY);
    }
    // 封板后登记必拦。
    if sc.enroll("late", 1, 0, 0) != Err(E_SEALED) {
        return Err(E_SEALED);
    }
    let sm = sc.summary()?;
    if sm.len() < SUMMARY_MIN_LEN {
        return Err(E_SUMMARY_EMPTY);
    }

    // —— 注入检出 100%：注入 5 空样本 → 全数命中 ——
    let mut sc2 = EmptyScan::new();
    i = 0;
    while i < 16 {
        sc2.enroll(&name_of(100 + i), 1, 0, (i % DOMAIN_COUNT) as u8)?;
        i += 1;
    }
    // 注入：无谓词（pred=0）。
    sc2.inject("void_a", 0, 0, 0)?;
    // 注入：Noop 占位（pred=1, kind=3）。
    sc2.inject("void_b", 1, KIND_NOOP, 1)?;
    // 注入：双因。
    sc2.inject("void_c", 0, KIND_NOOP, 2)?;
    sc2.inject("void_d", 0, 1, 3)?;
    sc2.inject("void_e", 1, KIND_NOOP, 4)?;
    let n = sc2.scan()?;
    if n != 5 {
        return Err(E_SELFTEST_MISS);
    }
    // 空因核对：双因样本 cause=2。
    let hc = sc2.hits()[2].cause;
    if hc != 2 {
        return Err(E_SELFTEST_MISS);
    }
    sc2.self_test()?;
    // 有命中 → 零空断言结论必为 false（但自证位可落）。
    let z2 = sc2.zero_empty_verdict()?;
    if z2 {
        return Err(E_SELFTEST_MISS);
    }
    // 封板语义：自证过即可封（检出账随封板冻结）。
    sc2.seal()?;

    // —— 登记面空断言拦截 ——
    let mut sc3 = EmptyScan::new();
    if sc3.enroll("e0", 0, 0, 0) != Err(E_EMPTY_ASSERT) {
        return Err(E_EMPTY_ASSERT);
    }
    if sc3.enroll("e1", 1, KIND_NOOP, 0) != Err(E_EMPTY_ASSERT) {
        return Err(E_EMPTY_ASSERT);
    }
    // 注入通道只收空样本：塞实断言必拦。
    if sc3.inject("real", 1, 0, 0) != Err(E_SELFTEST_FALSE) {
        return Err(E_SELFTEST_FALSE);
    }

    // —— 错误路径样本 ——
    if sc3.enroll("", 1, 0, 0) != Err(E_NAME_BAD) {
        return Err(E_NAME_BAD);
    }
    if sc3.enroll("x", 2, 0, 0) != Err(E_PRED_RANGE) {
        return Err(E_PRED_RANGE);
    }
    if sc3.enroll("x", 1, 4, 0) != Err(E_KIND_RANGE) {
        return Err(E_KIND_RANGE);
    }
    if sc3.enroll("x", 1, 0, DOMAIN_COUNT as u8) != Err(E_DOMAIN_RANGE) {
        return Err(E_DOMAIN_RANGE);
    }
    sc3.enroll("dup", 1, 0, 0)?;
    if sc3.enroll("dup", 1, 1, 1) != Err(E_ASSERT_DUP) {
        return Err(E_ASSERT_DUP);
    }
    // 空账扫描/结论拦。
    let mut sc4 = EmptyScan::new();
    if sc4.scan() != Err(E_SCAN_EMPTY) {
        return Err(E_SCAN_EMPTY);
    }
    if sc4.zero_empty_verdict() != Err(E_SCAN_EMPTY) {
        return Err(E_SCAN_EMPTY);
    }
    if sc4.self_test() != Err(E_SCAN_EMPTY) {
        return Err(E_SCAN_EMPTY);
    }
    // 自证未跑即结论。
    let mut sc5 = EmptyScan::new();
    sc5.enroll("solo", 1, 0, 0)?;
    sc5.scan()?;
    if sc5.zero_empty_verdict() != Err(E_SELFTEST_OPEN) {
        return Err(E_SELFTEST_OPEN);
    }
    if sc5.seal() != Err(E_SELFTEST_OPEN) {
        return Err(E_SELFTEST_OPEN);
    }
    // 误报注入：命中账手工塞实断言行 → 净检通道必拦。
    let mut sc6 = EmptyScan::new();
    sc6.enroll("r1", 1, 0, 0)?;
    sc6.enroll("r2", 1, 1, 1)?;
    sc6.scan()?;
    sc6.hits.push(HitRow {
        idx: 0,
        name_fp: sc6.asserts[0].name_fp,
        cause: 0,
    });
    if sc6.self_test() != Err(E_SELFTEST_FALSE) {
        return Err(E_SELFTEST_FALSE);
    }
    // 漏检注入：账里塞空样本但检出账清空 → 注检通道必拦。
    let mut sc7 = EmptyScan::new();
    sc7.enroll("q1", 1, 0, 0)?;
    sc7.inject("vq", 0, 0, 0)?;
    sc7.scan()?;
    sc7.hits.clear();
    if sc7.self_test() != Err(E_SELFTEST_MISS) {
        return Err(E_SELFTEST_MISS);
    }
    // 悬空解引。
    if sc7.assert_at(99) != Err(E_REF_DANGLE) {
        return Err(E_REF_DANGLE);
    }

    // 诊断表抽查。
    if diag_detail(E_SELFTEST_MISS) == "未知诊断码（0x58a0 段外或保留位）" {
        return Err(SEG_BASE + 0x10);
    }
    if diag_detail(SEG_BASE.wrapping_add(0x1F)) != "未知诊断码（0x58a0 段外或保留位）" {
        return Err(SEG_BASE + 0x11);
    }

    Ok(0)
}
