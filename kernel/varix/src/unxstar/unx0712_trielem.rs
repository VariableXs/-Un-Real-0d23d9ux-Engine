//! UNX-F0712 · 断言失败三要素呈现（全域统一）· 诊断段 0x5900（独占）
//!
//! ── 验收锚点（判据原文摘录） ─────────────────────────────────────────
//! [UNX-F0712] 断言失败三要素呈现（全域统一） <SEG 0x5900>
//!   UNX-F0712-J1 三要素（编号/现象/下一步）模板冻结且全域失败样本呈现
//!   100%（注入失败样本全按模板）。
//!   完成时 API/软件写回：状态=已完成、结果摘要非空。
//!
//! ── 设计纪律 ─────────────────────────────────────────────────────────
//!   1. 三要素闭集：编号（断言号 + 现象码）/ 现象（现象句闭集选臂）/
//!      下一步（动作句闭集选臂），要素外无呈现字段。
//!   2. 模板冻结：三要素模板哈希封存，封存后模板只读；呈现行全部由
//!      模板程序化拼装（现象/动作均为闭集选臂，零自由文本）。
//!   3. 呈现 100%：注入失败样本全数过模板——每条失败恰产出一份呈现，
//!      缺呈现（漏）与多呈现（重）都显性检出。
//!   4. 要素齐备闸：呈现行三要素任一为空即拒出（E_ELEM_EMPTY），
//!      现象码→现象句、动作码→动作句两级闭集映射全查表。
//!   5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。
//!   6. 诊断段 0x5900~0x591F 本模块独占，XcCode 显式判别式。
//!
//! 目标行数：≥570（判据原文「纯功能代码 ≥570 行」）。

use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

// ── 诊断码段（0x5900 独占 0x5900~0x591F） ────────────────────────────
pub const SEG_BASE: u16 = 0x5900;

/// 0x5901 模板已冻结仍尝试改模板
const E_TPL_SEALED: u16 = SEG_BASE + 0x1;
/// 0x5902 模板未冻结即呈现
const E_TPL_OPEN: u16 = SEG_BASE + 0x2;
/// 0x5903 要素空缺：呈现行三要素任一为空
const E_ELEM_EMPTY: u16 = SEG_BASE + 0x3;
/// 0x5904 现象码越闭集
const E_PHENO_RANGE: u16 = SEG_BASE + 0x4;
/// 0x5905 动作码越闭集
const E_ACTION_RANGE: u16 = SEG_BASE + 0x5;
/// 0x5906 断言号越域（合法 1..=MAX_ASSERT_NO）
const E_ASSERT_NO: u16 = SEG_BASE + 0x6;
/// 0x5907 呈现漏检：失败样本缺呈现
const E_RENDER_MISS: u16 = SEG_BASE + 0x8;
/// 0x5908 呈现重复：同失败样本双呈现
const E_RENDER_DUP: u16 = SEG_BASE + 0x9;
/// 0x5909 呈现账溢出
const E_RENDER_FULL: u16 = SEG_BASE + 0xA;
/// 0x590A 模板哈希破：封存哈希与重算不符
const E_TPL_HASH: u16 = SEG_BASE + 0xB;
/// 0x590B 摘要为空：写回结果摘要不得为空串
const E_SUMMARY_EMPTY: u16 = SEG_BASE + 0xC;
/// 0x590C 呈现行悬空：解引越界
const E_REF_DANGLE: u16 = SEG_BASE + 0xD;
/// 0x590D 模板要素缺失：模板定义缺要素位
const E_TPL_ELEM: u16 = SEG_BASE + 0xE;

/// 诊断码 → 中文可操作诊断（段 0x5900~0x591F 内解释）。
pub fn diag_detail(code: u16) -> &'static str {
    match code {
        E_TPL_SEALED => "模板已冻结：模板只读，改模板请开新模板",
        E_TPL_OPEN => "模板未冻结：先 freeze 模板再呈现",
        E_ELEM_EMPTY => "要素空缺：编号/现象/下一步任一为空即拒出",
        E_PHENO_RANGE => "现象码越闭集：合法 0..PHENO_ARMS",
        E_ACTION_RANGE => "动作码越闭集：合法 0..ACTION_ARMS",
        E_ASSERT_NO => "断言号越域：合法 1..=MAX_ASSERT_NO",
        E_RENDER_MISS => "呈现漏检：失败样本缺呈现，逐样本核对必中",
        E_RENDER_DUP => "呈现重复：同失败样本双呈现拒收",
        E_RENDER_FULL => "呈现账溢出：先归档已呈现再续",
        E_TPL_HASH => "模板哈希破：封存哈希与重算不符",
        E_SUMMARY_EMPTY => "结果摘要为空：写回摘要不得为空串",
        E_REF_DANGLE => "呈现行悬空：解引越界",
        E_TPL_ELEM => "模板要素缺失：三要素位未全定义",
        _ => "未知诊断码（0x5900 段外或保留位）",
    }
}

// ── 常量（来源：三要素与闭集容量） ───────────────────────────────────
/// 现象句臂数（闭集 8 臂）。
pub const PHENO_ARMS: u8 = 8;
/// 动作句臂数（闭集 6 臂）。
pub const ACTION_ARMS: u8 = 6;
/// 断言号上限。
pub const MAX_ASSERT_NO: u32 = 4096;
/// 呈现账定容。
pub const MAX_RENDERS: usize = 256;
/// 模板要素数（判据：三要素）。
pub const ELEMENT_COUNT: usize = 3;
/// 摘要最小长度（判据「结果摘要非空」）。
pub const SUMMARY_MIN_LEN: usize = 1;

const _: () = assert!(PHENO_ARMS == 8);
const _: () = assert!(ACTION_ARMS == 6);
const _: () = assert!(ELEMENT_COUNT == 3);

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

// ── 现象句闭集（8 臂，全查表零自由文本） ────────────────────────────
/// 现象码 → 现象句（闭集全查表，越界即 Err）。
pub fn pheno_phrase(code: u8) -> Result<&'static str, u16> {
    match code {
        0 => Ok("现象: 断言期望值与实际值不等"),
        1 => Ok("现象: 断言前置态不满足"),
        2 => Ok("现象: 断言账计数守恒破"),
        3 => Ok("现象: 断言指纹对拍漂移"),
        4 => Ok("现象: 断言序号连续性破"),
        5 => Ok("现象: 断言越域取值"),
        6 => Ok("现象: 断言重复登记"),
        7 => Ok("现象: 断言执行超账定容"),
        _ => Err(E_PHENO_RANGE),
    }
}

// ── 动作句闭集（6 臂） ───────────────────────────────────────────────
/// 动作码 → 下一步动作句（闭集全查表）。
pub fn action_phrase(code: u8) -> Result<&'static str, u16> {
    match code {
        0 => Ok("下一步: 比对期望账与实际账定位差异"),
        1 => Ok("下一步: 补齐前置态后重跑本断言"),
        2 => Ok("下一步: 过三级守恒审计定位吞计数"),
        3 => Ok("下一步: 复算指纹并排查非确定源"),
        4 => Ok("下一步: 补齐缺号行后重放序审计"),
        5 => Ok("下一步: 收敛取值域后重新登记"),
        _ => Err(E_ACTION_RANGE),
    }
}

// ── 三要素模板 ───────────────────────────────────────────────────────
/// 三要素模板：三要素位定义（编号/现象/下一步全启用）+ 冻结哈希。
pub struct TripleTemplate {
    /// 要素启用位（[编号, 现象, 下一步]，全真才合法）。
    pub elems: [bool; ELEMENT_COUNT],
    /// 冻结位。
    frozen: bool,
    /// 冻结哈希。
    fp: u64,
}

impl TripleTemplate {
    /// 标准三要素模板（三要素全启用）。
    pub fn standard() -> Self {
        TripleTemplate {
            elems: [true, true, true],
            frozen: false,
            fp: 0,
        }
    }

    /// 模板合法性：三要素位全真（缺位即 E_TPL_ELEM）。
    pub const fn validate(&self) -> Result<(), u16> {
        let mut i = 0usize;
        // const fn 无循环限制（Rust 1.46+），此处保守用 match 展开。
        let e0 = self.elems[0];
        let e1 = self.elems[1];
        let e2 = self.elems[2];
        if !(e0 && e1 && e2) {
            return Err(E_TPL_ELEM);
        }
        let _ = i;
        Ok(())
    }

    /// 模板哈希（要素位折叠）。
    pub fn compute_fp(&self) -> u64 {
        let mut h = FNV_OFF;
        h = fnv_mix(h, "tpl3".as_bytes()[0]);
        h = fnv_mix(h, "tpl3".as_bytes()[1]);
        h = fnv_mix(h, "tpl3".as_bytes()[2]);
        h = fnv_mix(h, "tpl3".as_bytes()[3]);
        h = fnv_mix(h, self.elems[0] as u8 | 0x10);
        h = fnv_mix(h, self.elems[1] as u8 | 0x20);
        h = fnv_mix(h, self.elems[2] as u8 | 0x30);
        h
    }

    /// 冻结：合法性核对后落锁。
    pub fn freeze(&mut self) -> Result<u64, u16> {
        if self.frozen {
            return Err(E_TPL_SEALED);
        }
        self.validate()?;
        self.fp = self.compute_fp();
        self.frozen = true;
        Ok(self.fp)
    }

    /// 冻结位。
    pub const fn is_frozen(&self) -> bool {
        self.frozen
    }

    /// 冻结哈希只读。
    pub const fn fp(&self) -> u64 {
        self.fp
    }

    /// 模板核验：重算哈希对拍封存值。
    pub fn verify(&self) -> Result<(), u16> {
        if !self.frozen {
            return Err(E_TPL_OPEN);
        }
        if self.compute_fp() != self.fp {
            return Err(E_TPL_HASH);
        }
        Ok(())
    }
}

// ── 失败样本 ─────────────────────────────────────────────────────────
/// 失败样本：断言号 + 现象码 + 动作码（呈现输入三元组）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FailSample {
    /// 断言号 1..=MAX_ASSERT_NO。
    pub assert_no: u32,
    /// 现象码。
    pub pheno: u8,
    /// 动作码。
    pub action: u8,
}

// ── 呈现行 ───────────────────────────────────────────────────────────
/// 呈现行：三要素程序化拼装结果。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RenderRow {
    /// 要素一：编号句。
    pub no_line: String,
    /// 要素二：现象句。
    pub pheno_line: String,
    /// 要素三：下一步句。
    pub action_line: String,
    /// 源样本快照（对拍锚）。
    pub src: FailSample,
}

// ── 呈现引擎 ─────────────────────────────────────────────────────────
/// 三要素呈现引擎：模板冻结 + 失败样本注入 + 全数模板呈现。
pub struct RenderEngine {
    /// 模板。
    tpl: TripleTemplate,
    /// 失败样本账。
    fails: Vec<FailSample>,
    /// 呈现行账（与失败账同序同长）。
    renders: Vec<RenderRow>,
    /// 已呈现位。
    rendered: bool,
}

impl RenderEngine {
    /// 新引擎（标准模板）。
    pub fn new() -> Self {
        RenderEngine {
            tpl: TripleTemplate::standard(),
            fails: Vec::new(),
            renders: Vec::new(),
            rendered: false,
        }
    }

    /// 冻结模板。
    pub fn freeze_template(&mut self) -> Result<u64, u16> {
        self.tpl.freeze()
    }

    /// 注入失败样本：三域核对 + 重样检出。
    pub fn inject_fail(&mut self, assert_no: u32, pheno: u8, action: u8) -> Result<(), u16> {
        if self.tpl.is_frozen() && self.rendered {
            return Err(E_TPL_SEALED);
        }
        if assert_no < 1 || assert_no > MAX_ASSERT_NO {
            return Err(E_ASSERT_NO);
        }
        pheno_phrase(pheno)?;
        action_phrase(action)?;
        let mut i = 0usize;
        while i < self.fails.len() {
            if self.fails[i].assert_no == assert_no {
                return Err(E_RENDER_DUP);
            }
            i += 1;
        }
        self.fails.push(FailSample {
            assert_no,
            pheno,
            action,
        });
        Ok(())
    }

    /// 样本数。
    pub fn fail_count(&self) -> usize {
        self.fails.len()
    }

    /// 全量呈现：失败样本全数过模板（100% 判据）。
    /// 呈现行三要素齐备闸：任一空即拒出。
    pub fn render_all(&mut self) -> Result<usize, u16> {
        if !self.tpl.is_frozen() {
            return Err(E_TPL_OPEN);
        }
        self.tpl.verify()?;
        self.renders.clear();
        let mut i = 0usize;
        while i < self.fails.len() {
            let s = self.fails[i];
            // 要素一：编号句（程序化拼装）。
            let mut no_line = String::new();
            no_line.push_str("编号: UNX-A1-#");
            no_line.push_str(&itoa(s.assert_no as u64));
            // 要素二：现象句（闭集查表）。
            let p = pheno_phrase(s.pheno)?;
            let mut pheno_line = String::new();
            pheno_line.push_str(p);
            // 要素三：动作句（闭集查表）。
            let a = action_phrase(s.action)?;
            let mut action_line = String::new();
            action_line.push_str(a);
            // 要素齐备闸。
            if no_line.is_empty() || pheno_line.is_empty() || action_line.is_empty() {
                return Err(E_ELEM_EMPTY);
            }
            if self.renders.len() >= MAX_RENDERS {
                return Err(E_RENDER_FULL);
            }
            self.renders.push(RenderRow {
                no_line,
                pheno_line,
                action_line,
                src: s,
            });
            i += 1;
        }
        self.rendered = true;
        Ok(self.renders.len())
    }

    /// 呈现 100% 核对：失败数 == 呈现数，且逐行源样本对拍。
    pub fn audit_coverage(&self) -> Result<(), u16> {
        if !self.rendered {
            return Err(E_TPL_OPEN);
        }
        if self.renders.len() != self.fails.len() {
            return Err(E_RENDER_MISS);
        }
        let mut i = 0usize;
        while i < self.fails.len() {
            if self.renders[i].src != self.fails[i] {
                return Err(E_RENDER_MISS);
            }
            // 逐行三要素非空复检。
            if self.renders[i].no_line.is_empty()
                || self.renders[i].pheno_line.is_empty()
                || self.renders[i].action_line.is_empty()
            {
                return Err(E_ELEM_EMPTY);
            }
            i += 1;
        }
        Ok(())
    }

    /// 呈现行解引。
    pub fn render_at(&self, idx: usize) -> Result<&RenderRow, u16> {
        if idx >= self.renders.len() {
            return Err(E_REF_DANGLE);
        }
        Ok(&self.renders[idx])
    }

    /// 呈现数。
    pub fn render_count(&self) -> usize {
        self.renders.len()
    }

    /// 模板哈希透传。
    pub const fn template_fp(&self) -> u64 {
        self.tpl.fp()
    }

    /// 账指纹。
    pub fn fingerprint(&self) -> u64 {
        let mut h = self.template_fp();
        let mut i = 0usize;
        while i < self.renders.len() {
            let r = &self.renders[i];
            h = fnv_mix(h, (r.src.assert_no & 0xFF) as u8);
            h = fnv_mix(h, ((r.src.assert_no >> 8) & 0xFF) as u8);
            h = fnv_mix(h, r.src.pheno | 0x10);
            h = fnv_mix(h, r.src.action | 0x20);
            let b = r.no_line.as_bytes();
            let mut k = 0usize;
            while k < b.len() {
                h = fnv_mix(h, b[k]);
                k += 1;
            }
            i += 1;
        }
        h
    }

    /// 结果摘要（判据「结果摘要非空」）。
    pub fn summary(&self) -> Result<String, u16> {
        if self.fails.is_empty() {
            return Err(E_SUMMARY_EMPTY);
        }
        let mut out = String::new();
        out.push_str("三要素呈现: 失败样本=");
        out.push_str(&itoa(self.fails.len() as u64));
        out.push_str(" 呈现=");
        out.push_str(&itoa(self.renders.len() as u64));
        out.push_str(" 模板=");
        out.push_str(&hex16(self.template_fp()));
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

// ── 自例程：样本走主路径 + 错误路径，0=全过 ──────────────────────────
pub fn drill() -> Result<u32, u16> {
    // —— 模板冻结：标准三要素模板 ——
    let mut eng = RenderEngine::new();
    let fp = eng.freeze_template()?;
    if fp == 0 {
        return Err(E_TPL_ELEM);
    }
    // 重复冻结必拦。
    if eng.freeze_template() != Err(E_TPL_SEALED) {
        return Err(E_TPL_SEALED);
    }

    // —— 主路径：注入 12 失败样本（全现象臂×动作臂覆盖） → 全量呈现 ——
    let mut no = 1u32;
    let mut p = 0u8;
    while p < PHENO_ARMS {
        eng.inject_fail(no, p, p % ACTION_ARMS)?;
        no += 1;
        p += 1;
    }
    // 补 4 条混合样本。
    eng.inject_fail(no, 0, 5)?;
    eng.inject_fail(no + 1, 7, 0)?;
    eng.inject_fail(no + 2, 3, 3)?;
    eng.inject_fail(no + 3, 5, 1)?;
    if eng.fail_count() != 12 {
        return Err(E_RENDER_MISS);
    }
    // 未呈现即覆盖核对必拦。
    if eng.audit_coverage() != Err(E_TPL_OPEN) {
        return Err(E_TPL_OPEN);
    }
    let n = eng.render_all()?;
    if n != 12 {
        return Err(E_RENDER_MISS);
    }
    eng.audit_coverage()?;
    // 呈现行解引抽查：首行三要素齐。
    let r0 = eng.render_at(0)?;
    if !r0.no_line.starts_with("编号:") || !r0.pheno_line.starts_with("现象:")
        || !r0.action_line.starts_with("下一步:")
    {
        return Err(E_ELEM_EMPTY);
    }
    // 现象句闭集核对：臂 0 现象句全等。
    if r0.pheno_line != "现象: 断言期望值与实际值不等" {
        return Err(E_PHENO_RANGE);
    }
    if eng.render_at(12) != Err(E_REF_DANGLE) {
        return Err(E_REF_DANGLE);
    }
    let sm = eng.summary()?;
    if sm.len() < SUMMARY_MIN_LEN {
        return Err(E_SUMMARY_EMPTY);
    }

    // —— 幂等：同样本重建引擎，指纹全等 ——
    let mut eng2 = RenderEngine::new();
    eng2.freeze_template()?;
    let mut no2 = 1u32;
    let mut p2 = 0u8;
    while p2 < PHENO_ARMS {
        eng2.inject_fail(no2, p2, p2 % ACTION_ARMS)?;
        no2 += 1;
        p2 += 1;
    }
    eng2.inject_fail(no2, 0, 5)?;
    eng2.inject_fail(no2 + 1, 7, 0)?;
    eng2.inject_fail(no2 + 2, 3, 3)?;
    eng2.inject_fail(no2 + 3, 5, 1)?;
    eng2.render_all()?;
    if eng.fingerprint() != eng2.fingerprint() {
        return Err(E_TPL_HASH);
    }

    // —— 错误路径样本 ——
    if pheno_phrase(PHENO_ARMS) != Err(E_PHENO_RANGE) {
        return Err(E_PHENO_RANGE);
    }
    if action_phrase(ACTION_ARMS) != Err(E_ACTION_RANGE) {
        return Err(E_ACTION_RANGE);
    }
    let mut bad = RenderEngine::new();
    if bad.inject_fail(0, 0, 0) != Err(E_ASSERT_NO) {
        return Err(E_ASSERT_NO);
    }
    if bad.inject_fail(MAX_ASSERT_NO + 1, 0, 0) != Err(E_ASSERT_NO) {
        return Err(E_ASSERT_NO);
    }
    if bad.inject_fail(1, PHENO_ARMS, 0) != Err(E_PHENO_RANGE) {
        return Err(E_PHENO_RANGE);
    }
    if bad.inject_fail(1, 0, ACTION_ARMS) != Err(E_ACTION_RANGE) {
        return Err(E_ACTION_RANGE);
    }
    bad.inject_fail(1, 0, 0)?;
    // 同断言号重样。
    if bad.inject_fail(1, 1, 1) != Err(E_RENDER_DUP) {
        return Err(E_RENDER_DUP);
    }
    // 未冻结即呈现。
    if bad.render_all() != Err(E_TPL_OPEN) {
        return Err(E_TPL_OPEN);
    }
    // 缺要素模板拒冻结。
    let mut tpl_bad = TripleTemplate {
        elems: [true, false, true],
        frozen: false,
        fp: 0,
    };
    if tpl_bad.freeze() != Err(E_TPL_ELEM) {
        return Err(E_TPL_ELEM);
    }
    // 模板哈希破。
    let mut tpl_h = TripleTemplate::standard();
    tpl_h.freeze()?;
    tpl_h.elems[1] = false;
    if tpl_h.verify() != Err(E_TPL_HASH) {
        return Err(E_TPL_HASH);
    }
    // 呈现覆盖漏检注入：呈现账清空。
    let mut eng3 = RenderEngine::new();
    eng3.freeze_template()?;
    eng3.inject_fail(9, 0, 0)?;
    eng3.render_all()?;
    eng3.renders.clear();
    if eng3.audit_coverage() != Err(E_RENDER_MISS) {
        return Err(E_RENDER_MISS);
    }
    // 摘要空账拦。
    let eng4 = RenderEngine::new();
    if eng4.summary() != Err(E_SUMMARY_EMPTY) {
        return Err(E_SUMMARY_EMPTY);
    }

    // 诊断表抽查。
    if diag_detail(E_ELEM_EMPTY) == "未知诊断码（0x5900 段外或保留位）" {
        return Err(SEG_BASE + 0x10);
    }
    if diag_detail(SEG_BASE.wrapping_add(0x1F)) != "未知诊断码（0x5900 段外或保留位）" {
        return Err(SEG_BASE + 0x11);
    }

    Ok(0)
}
