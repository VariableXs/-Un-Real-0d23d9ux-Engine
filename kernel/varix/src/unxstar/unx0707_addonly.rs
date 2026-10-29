//! UNX-F0707 · 只增不减纪律账（断言清单 diff 断言）· 诊断段 0x5860（独占）
//!
//! ── 验收锚点（判据原文摘录） ─────────────────────────────────────────
//! [UNX-F0707] 只增不减纪律账（断言清单 diff 断言） <SEG 0x5860>
//!   UNX-F0707-J1 纪律账落簿（基线清单快照 + 变更 diff 仅新增断言过），
//!   删改注入检出 100% 且删改即 P0 红账。
//!   完成时 API/软件写回：状态=已完成、结果摘要非空。
//!
//! ── 设计纪律 ─────────────────────────────────────────────────────────
//!   1. 基线快照：断言清单落定即拍 FNV 快照哈希 + 名序表；快照封存后
//!      基线只读（E_BASELINE_RDONLY）。
//!   2. diff 三臂闭集：Added（新增，唯一合法臂）/ Removed（删，P0 红账）/
//!      Modified（改，P0 红账）——臂外无变更语义，删改注入检出 100%。
//!   3. P0 红账：任何 Removed/Modified 臂即写红账行（P0 级 + 明细），
//!      红账非空即纪律破（E_DISCIPLINE_BROKEN），放行通道关闭。
//!   4. diff 算法确定性：名序表逐名对拍（双向：基线→新表 + 新表→基线），
//!      无状态歧义；同输入两次 diff 必同结果（幂等）。
//!   5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。
//!   6. 诊断段 0x5860~0x587F 本模块独占，XcCode 显式判别式。
//!
//! 目标行数：≥580（判据原文「纯功能代码 ≥580 行」）。

use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

// ── 诊断码段（0x5860 独占 0x5860~0x587F） ────────────────────────────
pub const SEG_BASE: u16 = 0x5860;

/// 0x5861 基线未封存即 diff
const E_NO_BASELINE: u16 = SEG_BASE + 0x1;
/// 0x5862 基线已封存仍尝试改基线
const E_BASELINE_RDONLY: u16 = SEG_BASE + 0x2;
/// 0x5863 删注入检出：基线断言在新表缺失
const E_REMOVE_HIT: u16 = SEG_BASE + 0x3;
/// 0x5864 改注入检出：同名断言语义位变更
const E_MODIFY_HIT: u16 = SEG_BASE + 0x4;
/// 0x5865 纪律破：红账非零即放行拦截
const E_DISCIPLINE_BROKEN: u16 = SEG_BASE + 0x5;
/// 0x5866 空新表：diff 目标清单为空
const E_EMPTY_TARGET: u16 = SEG_BASE + 0x6;
/// 0x5867 断言名非法：空名或超长
const E_NAME_BAD: u16 = SEG_BASE + 0x7;
/// 0x5868 断言重号：同语义位重复登记
const E_ASSERT_DUP: u16 = SEG_BASE + 0x8;
/// 0x5869 语义位越域：kind/sev 位越闭集
const E_SEMANTIC_RANGE: u16 = SEG_BASE + 0x9;
/// 0x586A 红账溢出：红账行数超定容
const E_RED_FULL: u16 = SEG_BASE + 0xA;
/// 0x586B 摘要为空：写回结果摘要不得为空串
const E_SUMMARY_EMPTY: u16 = SEG_BASE + 0xC;
/// 0x586C 断言账溢出：清单条目超定容
const E_LIST_FULL: u16 = SEG_BASE + 0xB;
/// 0x586D diff 幂等破：同输入两次 diff 结果漂移
const E_DIFF_IDEM: u16 = SEG_BASE + 0xD;
/// 0x586E 基线空：零断言即拍快照
const E_BASELINE_EMPTY: u16 = SEG_BASE + 0xE;

/// 诊断码 → 中文可操作诊断（段 0x5860~0x587F 内解释）。
pub fn diag_detail(code: u16) -> &'static str {
    match code {
        E_NO_BASELINE => "基线未封存：先拍基线快照再 diff",
        E_BASELINE_RDONLY => "基线只读：快照封存后基线不可改，变更走 diff",
        E_REMOVE_HIT => "删注入检出：基线断言在新表缺失，P0 红账",
        E_MODIFY_HIT => "改注入检出：同名断言语义位变更，P0 红账",
        E_DISCIPLINE_BROKEN => "纪律破：红账非零，只增不减被破坏，放行拦截",
        E_EMPTY_TARGET => "新表为空：diff 目标清单零条目无意义",
        E_NAME_BAD => "断言名非法：空名或超 64 字节",
        E_ASSERT_DUP => "断言重号：同语义位重复登记拒收",
        E_SEMANTIC_RANGE => "语义位越域：kind 0..4 / sev 0..3",
        E_RED_FULL => "红账溢出：红账行数超定容，先处置再续",
        E_SUMMARY_EMPTY => "结果摘要为空：写回摘要不得为空串",
        E_LIST_FULL => "清单溢出：条目数超定容上限",
        E_DIFF_IDEM => "diff 幂等破：同输入两次结果漂移，非确定源",
        E_BASELINE_EMPTY => "基线空：零断言清单无快照资格",
        _ => "未知诊断码（0x5860 段外或保留位）",
    }
}

// ── 常量（来源：纪律口径与定容） ─────────────────────────────────────
/// 断言名长度上限（字节）。
pub const NAME_MAX: usize = 64;
/// 语义 kind 闭集容量（0..4）。
pub const KIND_VARIANTS: u8 = 4;
/// 严重级闭集容量（0..3：0=P0 红/1=P1/2=P2）。
pub const SEV_VARIANTS: u8 = 3;
/// 清单定容。
pub const MAX_LIST: usize = 256;
/// 红账定容。
pub const MAX_RED: usize = 64;
/// 摘要最小长度（判据「结果摘要非空」）。
pub const SUMMARY_MIN_LEN: usize = 1;

const _: () = assert!(NAME_MAX == 64);
const _: () = assert!(KIND_VARIANTS == 4);
const _: () = assert!(SEV_VARIANTS == 3);

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
/// 断言记录：名哈希（身份锚）+ 语义位（kind/sev，Modified 检出锚）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AssertRecord {
    /// 名 FNV 哈希（身份）。
    pub name_fp: u64,
    /// 语义 kind（0..4）。
    pub kind: u8,
    /// 严重级 sev（0..3）。
    pub sev: u8,
}

impl AssertRecord {
    /// 语义位合法性。
    pub const fn semantic_ok(&self) -> bool {
        self.kind < KIND_VARIANTS && self.sev < SEV_VARIANTS
    }

    /// 语义指纹（Modified 对拍锚：名不变语义变即命中）。
    pub const fn semantic_fp(&self) -> u32 {
        (self.kind as u32) | ((self.sev as u32) << 8) | 0xA0A0_0000
    }
}

// ── diff 臂闭集 ──────────────────────────────────────────────────────
/// 变更 diff 三臂闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DiffArm {
    /// 新增：唯一合法臂。
    Added,
    /// 删：P0 红账。
    Removed,
    /// 改：P0 红账。
    Modified,
}

impl DiffArm {
    /// 臂号。
    pub const fn id(self) -> u8 {
        match self {
            DiffArm::Added => 0,
            DiffArm::Removed => 1,
            DiffArm::Modified => 2,
        }
    }

    /// P0 红账臂判定：Removed/Modified 即红。
    pub const fn is_red(self) -> bool {
        match self {
            DiffArm::Added => false,
            DiffArm::Removed => true,
            DiffArm::Modified => true,
        }
    }
}

// ── diff 行 ──────────────────────────────────────────────────────────
/// 一行 diff 结果：臂 + 涉事断言名哈希 + 语义位（Modified 时为旧值）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DiffRow {
    /// diff 臂。
    pub arm: DiffArm,
    /// 涉事断言名哈希。
    pub name_fp: u64,
    /// 基线侧语义指纹（Added 臂为 0）。
    pub base_sev_fp: u32,
}

// ── 红账行 ───────────────────────────────────────────────────────────
/// P0 红账行：级 P0 + 涉事名哈希 + 臂号。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RedRow {
    /// 级别恒 P0（0）。
    pub level: u8,
    /// 涉事断言名哈希。
    pub name_fp: u64,
    /// 臂号（1=删 2=改）。
    pub arm: u8,
}

// ── 只增不减纪律账 ───────────────────────────────────────────────────
/// 纪律账：基线清单 + 快照封存 + diff 引擎 + P0 红账 + 放行闸。
pub struct AddOnlyLedger {
    /// 基线断言清单（名序表，登记序）。
    baseline: Vec<AssertRecord>,
    /// 基线快照哈希（封存位锚）。
    baseline_fp: u64,
    /// 基线封存位。
    baseline_sealed: bool,
    /// 红账。
    reds: Vec<RedRow>,
    /// 最近一次 diff 结果（幂等对拍锚）。
    last_diff: Vec<DiffRow>,
    /// 已 diff 位。
    diffed: bool,
    /// 放行位（纪律全过才置位）。
    released: bool,
}

impl AddOnlyLedger {
    /// 新纪律账。
    pub fn new() -> Self {
        AddOnlyLedger {
            baseline: Vec::new(),
            baseline_fp: 0,
            baseline_sealed: false,
            reds: Vec::new(),
            last_diff: Vec::new(),
            diffed: false,
            released: false,
        }
    }

    /// 基线登记（封存前）。
    pub fn enroll(&mut self, name: &str, kind: u8, sev: u8) -> Result<(), u16> {
        if self.baseline_sealed {
            return Err(E_BASELINE_RDONLY);
        }
        if self.baseline.len() >= MAX_LIST {
            return Err(E_LIST_FULL);
        }
        if name.is_empty() || name.len() > NAME_MAX {
            return Err(E_NAME_BAD);
        }
        if kind >= KIND_VARIANTS || sev >= SEV_VARIANTS {
            return Err(E_SEMANTIC_RANGE);
        }
        let fp = fnv_str(name);
        let mut i = 0usize;
        while i < self.baseline.len() {
            if self.baseline[i].name_fp == fp {
                return Err(E_ASSERT_DUP);
            }
            i += 1;
        }
        self.baseline.push(AssertRecord { name_fp: fp, kind, sev });
        Ok(())
    }

    /// 拍基线快照：哈希封存，基线转只读。
    pub fn seal_baseline(&mut self) -> Result<u64, u16> {
        if self.baseline_sealed {
            return Err(E_BASELINE_RDONLY);
        }
        if self.baseline.is_empty() {
            return Err(E_BASELINE_EMPTY);
        }
        self.baseline_fp = self.baseline_fingerprint();
        self.baseline_sealed = true;
        Ok(self.baseline_fp)
    }

    /// 基线指纹。
    fn baseline_fingerprint(&self) -> u64 {
        let mut h = FNV_OFF;
        let mut i = 0usize;
        while i < self.baseline.len() {
            let r = self.baseline[i];
            let mut shift = 56i32;
            while shift >= 0 {
                h = fnv_mix(h, ((r.name_fp >> shift) & 0xFF) as u8);
                shift -= 8;
            }
            h = fnv_mix(h, r.kind | 0x10);
            h = fnv_mix(h, r.sev | 0x20);
            i += 1;
        }
        h
    }

    /// 基线封存位。
    pub const fn baseline_sealed(&self) -> bool {
        self.baseline_sealed
    }

    /// 基线条数。
    pub fn baseline_len(&self) -> usize {
        self.baseline.len()
    }

    /// diff 引擎：基线 vs 新表，三臂分类。
    /// 双向对拍：① 基线逐条在新表找——缺失即 Removed；同名语义位变即
    /// Modified；② 新表逐条在基线找——缺失即 Added。
    /// 删改臂写红账；返回红账行数。
    pub fn diff(&mut self, target: &[AssertRecord]) -> Result<usize, u16> {
        if !self.baseline_sealed {
            return Err(E_NO_BASELINE);
        }
        if target.is_empty() {
            return Err(E_EMPTY_TARGET);
        }
        // 新表自身去重前置核对。
        let mut a = 0usize;
        while a < target.len() {
            if !target[a].semantic_ok() {
                return Err(E_SEMANTIC_RANGE);
            }
            let mut b = a + 1;
            while b < target.len() {
                if target[a].name_fp == target[b].name_fp {
                    return Err(E_ASSERT_DUP);
                }
                b += 1;
            }
            a += 1;
        }
        let mut rows: Vec<DiffRow> = Vec::new();
        // ① 基线→新表：Removed / Modified 检出。
        let mut i = 0usize;
        while i < self.baseline.len() {
            let base = self.baseline[i];
            let mut found = false;
            let mut j = 0usize;
            while j < target.len() {
                if target[j].name_fp == base.name_fp {
                    found = true;
                    if target[j].kind != base.kind || target[j].sev != base.sev {
                        rows.push(DiffRow {
                            arm: DiffArm::Modified,
                            name_fp: base.name_fp,
                            base_sev_fp: base.semantic_fp(),
                        });
                    }
                    break;
                }
                j += 1;
            }
            if !found {
                rows.push(DiffRow {
                    arm: DiffArm::Removed,
                    name_fp: base.name_fp,
                    base_sev_fp: base.semantic_fp(),
                });
            }
            i += 1;
        }
        // ② 新表→基线：Added 检出。
        let mut k = 0usize;
        while k < target.len() {
            let cur = target[k];
            let mut found = false;
            let mut m = 0usize;
            while m < self.baseline.len() {
                if self.baseline[m].name_fp == cur.name_fp {
                    found = true;
                    break;
                }
                m += 1;
            }
            if !found {
                rows.push(DiffRow {
                    arm: DiffArm::Added,
                    name_fp: cur.name_fp,
                    base_sev_fp: 0,
                });
            }
            k += 1;
        }
        // 红账落簿：红臂逐行写 P0。
        let mut reds_new: Vec<RedRow> = Vec::new();
        let mut r = 0usize;
        while r < rows.len() {
            if rows[r].arm.is_red() {
                if self.reds.len() + reds_new.len() >= MAX_RED {
                    return Err(E_RED_FULL);
                }
                reds_new.push(RedRow {
                    level: 0,
                    name_fp: rows[r].name_fp,
                    arm: rows[r].arm.id(),
                });
            }
            r += 1;
        }
        // 幂等对拍：与上次 diff 结果逐行比对（同输入必同结果）。
        if self.diffed {
            if self.last_diff.len() != rows.len() {
                return Err(E_DIFF_IDEM);
            }
            let mut d = 0usize;
            while d < rows.len() {
                if self.last_diff[d] != rows[d] {
                    return Err(E_DIFF_IDEM);
                }
                d += 1;
            }
        }
        self.last_diff = rows;
        self.diffed = true;
        let mut x = 0usize;
        while x < reds_new.len() {
            self.reds.push(reds_new[x]);
            x += 1;
        }
        // 放行闸：本次 diff 零红臂才允许置位（红账非空即拦）。
        if reds_new.is_empty() && !self.released {
            self.released = true;
        }
        if !reds_new.is_empty() {
            self.released = false;
        }
        Ok(reds_new.len())
    }

    /// 红账行数。
    pub fn red_count(&self) -> usize {
        self.reds.len()
    }

    /// 红账只读。
    pub fn reds(&self) -> &[RedRow] {
        &self.reds
    }

    /// 放行位。
    pub const fn released(&self) -> bool {
        self.released
    }

    /// 纪律总审计：红账零行 + diff 已跑 + 基线封存。
    pub fn audit_discipline(&self) -> Result<(), u16> {
        if !self.baseline_sealed {
            return Err(E_NO_BASELINE);
        }
        if !self.diffed {
            return Err(E_NO_BASELINE);
        }
        if !self.reds.is_empty() {
            return Err(E_DISCIPLINE_BROKEN);
        }
        if !self.released {
            return Err(E_DISCIPLINE_BROKEN);
        }
        Ok(())
    }

    /// 最近 diff 只读。
    pub fn last_diff(&self) -> &[DiffRow] {
        &self.last_diff
    }

    /// 账指纹。
    pub fn fingerprint(&self) -> u64 {
        let mut h = FNV_OFF;
        h = fnv_mix(h, (self.baseline_fp & 0xFF) as u8);
        h = fnv_mix(h, ((self.baseline_fp >> 8) & 0xFF) as u8);
        h = fnv_mix(h, self.baseline_sealed as u8 | 0x10);
        h = fnv_mix(h, self.released as u8 | 0x20);
        let mut i = 0usize;
        while i < self.reds.len() {
            h = fnv_mix(h, self.reds[i].arm | 0x30);
            let mut shift = 56i32;
            while shift >= 0 {
                h = fnv_mix(h, ((self.reds[i].name_fp >> shift) & 0xFF) as u8);
                shift -= 8;
            }
            i += 1;
        }
        h
    }

    /// 结果摘要（判据「结果摘要非空」）。
    pub fn summary(&self) -> Result<String, u16> {
        if self.baseline.is_empty() {
            return Err(E_SUMMARY_EMPTY);
        }
        let mut out = String::new();
        out.push_str("只增不减纪律账: 基线=");
        out.push_str(&itoa(self.baseline.len() as u64));
        out.push_str(" 红账=");
        out.push_str(&itoa(self.reds.len() as u64));
        out.push_str(" 放行=");
        out.push_str(&itoa(self.released as u64));
        out.push_str(" 基线指纹=");
        out.push_str(&hex16(self.baseline_fp));
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

// ── 基线构造工具（确定性） ───────────────────────────────────────────
fn rec(name: &str, kind: u8, sev: u8) -> AssertRecord {
    AssertRecord {
        name_fp: fnv_str(name),
        kind,
        sev,
    }
}

// ── 自例程：样本走主路径 + 错误路径，0=全过 ──────────────────────────
pub fn drill() -> Result<u32, u16> {
    // —— 主路径：基线 8 条 → 封存 → 新表 = 基线 + 2 新增 → diff 零红 ——
    let mut led = AddOnlyLedger::new();
    let mut i = 0usize;
    while i < 8 {
        let nm = base_name(i);
        led.enroll(nm.as_str(), (i % 4) as u8, (i % 3) as u8)?;
        i += 1;
    }
    let fp = led.seal_baseline()?;
    if fp == 0 {
        return Err(E_BASELINE_EMPTY);
    }
    if !led.baseline_sealed() {
        return Err(E_BASELINE_RDONLY);
    }
    // 封存后改基线必拦。
    if led.enroll("late", 0, 0) != Err(E_BASELINE_RDONLY) {
        return Err(E_BASELINE_RDONLY);
    }
    // 新表：基线全留 + 2 新增 → 仅 Added 臂 → 零红账 → 放行。
    let mut target: Vec<AssertRecord> = Vec::new();
    i = 0;
    while i < 8 {
        target.push(rec(&base_name(i), (i % 4) as u8, (i % 3) as u8));
        i += 1;
    }
    target.push(rec("new_a", 0, 0));
    target.push(rec("new_b", 1, 2));
    let reds = led.diff(&target)?;
    if reds != 0 {
        return Err(E_DISCIPLINE_BROKEN);
    }
    if led.red_count() != 0 {
        return Err(E_DISCIPLINE_BROKEN);
    }
    if !led.released() {
        return Err(E_DISCIPLINE_BROKEN);
    }
    led.audit_discipline()?;
    // diff 幂等：同输入重跑零漂移。
    let mut target2: Vec<AssertRecord> = Vec::new();
    i = 0;
    while i < 8 {
        target2.push(rec(&base_name(i), (i % 4) as u8, (i % 3) as u8));
        i += 1;
    }
    target2.push(rec("new_a", 0, 0));
    target2.push(rec("new_b", 1, 2));
    let reds2 = led.diff(&target2)?;
    if reds2 != 0 {
        return Err(E_DISCIPLINE_BROKEN);
    }
    let sm = led.summary()?;
    if sm.len() < SUMMARY_MIN_LEN {
        return Err(E_SUMMARY_EMPTY);
    }

    // —— 删注入检出 100%：新表缺 1 条 → Removed 红账必中 ——
    let mut led2 = AddOnlyLedger::new();
    i = 0;
    while i < 8 {
        led2.enroll(&base_name(i), 0, 0)?;
        i += 1;
    }
    led2.seal_baseline()?;
    let mut drop_one: Vec<AssertRecord> = Vec::new();
    i = 1; // 跳过 base_0。
    while i < 8 {
        drop_one.push(rec(&base_name(i), 0, 0));
        i += 1;
    }
    drop_one.push(rec("keep", 0, 0));
    let r1 = led2.diff(&drop_one)?;
    if r1 != 1 {
        return Err(E_REMOVE_HIT);
    }
    if led2.red_count() != 1 {
        return Err(E_REMOVE_HIT);
    }
    if led2.released() {
        return Err(E_DISCIPLINE_BROKEN);
    }
    if led2.audit_discipline() != Err(E_DISCIPLINE_BROKEN) {
        return Err(E_DISCIPLINE_BROKEN);
    }
    // 红账臂号核对：Removed=1。
    if led2.reds()[0].arm != DiffArm::Removed.id() {
        return Err(E_REMOVE_HIT);
    }

    // —— 改注入检出 100%：同名语义位变更 → Modified 红账必中 ——
    let mut led3 = AddOnlyLedger::new();
    i = 0;
    while i < 4 {
        led3.enroll(&base_name(i), 0, 0)?;
        i += 1;
    }
    led3.seal_baseline()?;
    let mut mod_one: Vec<AssertRecord> = Vec::new();
    i = 0;
    while i < 4 {
        let sev = if i == 2 { 1 } else { 0 };
        mod_one.push(rec(&base_name(i), 0, sev));
        i += 1;
    }
    let r2 = led3.diff(&mod_one)?;
    if r2 != 1 {
        return Err(E_MODIFY_HIT);
    }
    if led3.reds()[0].arm != DiffArm::Modified.id() {
        return Err(E_MODIFY_HIT);
    }

    // —— 删+改混合注入：两条红臂同时检出 ——
    let mut led4 = AddOnlyLedger::new();
    i = 0;
    while i < 6 {
        led4.enroll(&base_name(i), 0, 0)?;
        i += 1;
    }
    led4.seal_baseline()?;
    let mut mixed: Vec<AssertRecord> = Vec::new();
    i = 1;
    while i < 6 {
        let kind = if i == 3 { 2 } else { 0 };
        mixed.push(rec(&base_name(i), kind, 0));
        i += 1;
    }
    mixed.push(rec("add_x", 0, 0));
    let r3 = led4.diff(&mixed)?;
    if r3 != 2 {
        return Err(E_DISCIPLINE_BROKEN);
    }
    if led4.red_count() != 2 {
        return Err(E_RED_FULL);
    }

    // —— 错误路径样本 ——
    let mut led5 = AddOnlyLedger::new();
    let t1 = [rec("x", 0, 0)];
    if led5.diff(&t1) != Err(E_NO_BASELINE) {
        return Err(E_NO_BASELINE);
    }
    if led5.seal_baseline() != Err(E_BASELINE_EMPTY) {
        return Err(E_BASELINE_EMPTY);
    }
    led5.enroll("a", 0, 0)?;
    led5.seal_baseline()?;
    let t0: Vec<AssertRecord> = Vec::new();
    if led5.diff(&t0) != Err(E_EMPTY_TARGET) {
        return Err(E_EMPTY_TARGET);
    }
    let tbad = [rec("x", 9, 0)];
    if led5.diff(&tbad) != Err(E_SEMANTIC_RANGE) {
        return Err(E_SEMANTIC_RANGE);
    }
    let tbad2 = [rec("dup", 0, 0), rec("dup", 1, 1)];
    if led5.diff(&tbad2) != Err(E_ASSERT_DUP) {
        return Err(E_ASSERT_DUP);
    }
    // 幂等破注入：同账两次不同输入。
    let mut led6 = AddOnlyLedger::new();
    led6.enroll("m", 0, 0)?;
    led6.seal_baseline()?;
    let ta = [rec("m", 0, 0), rec("n", 0, 0)];
    led6.diff(&ta)?;
    let tb = [rec("m", 0, 0), rec("o", 0, 0)];
    if led6.diff(&tb) != Err(E_DIFF_IDEM) {
        return Err(E_DIFF_IDEM);
    }
    // 名非法。
    let mut led7 = AddOnlyLedger::new();
    if led7.enroll("", 0, 0) != Err(E_NAME_BAD) {
        return Err(E_NAME_BAD);
    }
    let long = [b'x'; NAME_MAX + 1];
    let long_name = core::str::from_utf8(&long);
    match long_name {
        Ok(s) => {
            if led7.enroll(s, 0, 0) != Err(E_NAME_BAD) {
                return Err(E_NAME_BAD);
            }
        }
        Err(_) => {
            // 非 UTF8 场景不适用（字面量全 ASCII，恒为 Ok），占位核。
            if long.len() != NAME_MAX + 1 {
                return Err(E_NAME_BAD);
            }
        }
    }
    // 重号登记。
    led7.enroll("k", 0, 0)?;
    if led7.enroll("k", 1, 1) != Err(E_ASSERT_DUP) {
        return Err(E_ASSERT_DUP);
    }
    // 语义位越域。
    if led7.enroll("k2", 4, 0) != Err(E_SEMANTIC_RANGE) {
        return Err(E_SEMANTIC_RANGE);
    }
    if led7.enroll("k3", 0, 3) != Err(E_SEMANTIC_RANGE) {
        return Err(E_SEMANTIC_RANGE);
    }

    // 诊断表抽查。
    if diag_detail(E_REMOVE_HIT) == "未知诊断码（0x5860 段外或保留位）" {
        return Err(SEG_BASE + 0x10);
    }
    if diag_detail(SEG_BASE.wrapping_add(0x1F)) != "未知诊断码（0x5860 段外或保留位）" {
        return Err(SEG_BASE + 0x11);
    }

    Ok(0)
}

fn base_name(i: usize) -> String {
    let mut s = String::new();
    s.push_str("base_");
    s.push_str(&itoa(i as u64));
    s
}
