//! UNX-F0755 · 移交包验收核对单（六判据凭证指针） · 诊断段 0x5d60
//! 判据原文摘录：UNX-F0755-J1 核对单齐（六验收判据逐项：判据号/凭证指针/
//! 核对结论三列）且逐项凭证可解引 100%；完成时 API/软件写回：状态=已完成、
//! 结果摘要非空。纯功能代码 ≥570 行。
//!
//! 设计纪律：
//! 1. 六验收判据写死：零补丁 / 演练矩阵 / SLA 违约清零 / 接口回归 /
//!    行数守恒 / 断言全绿（判据号 1..=6，序不可换）。
//! 2. 三列结构：判据号 / 凭证指针 / 核对结论，三列齐才算行有效；
//!    凭证指针指向凭证账行，解引失败即行无效。
//! 3. 逐项凭证可解引 100%：六行全部解引成功才算核对单齐，
//!    任何悬垂指针即单据作废。
//! 4. 核对结论三态：Pass/Fail/Pending；封单要求全 Pass 且零 Pending。
//! 5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。
//! 6. 诊断段 0x5d60 为本单独占，XcCode 判别式显式写出。

use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

pub const SEG_BASE: u16 = 0x5d60;

// ── 诊断码闭集（SEG_BASE+1..+N）────────────────────────────────
const E_CRIT_BAD: u16 = SEG_BASE + 1; // 判据号越界（1..=6）
const E_CRIT_DUP: u16 = SEG_BASE + 2; // 判据号重复登记
const E_CRIT_FULL: u16 = SEG_BASE + 3; // 核对单已满（六行定容）
const E_CRIT_LACK: u16 = SEG_BASE + 4; // 六判据未齐即封单被拒
const E_PTR_ZERO: u16 = SEG_BASE + 5; // 凭证指针为 0（未挂凭证）
const E_PTR_DANGLE: u16 = SEG_BASE + 6; // 凭证指针不可解引
const E_CONCL_BAD: u16 = SEG_BASE + 7; // 核对结论值非法
const E_CONCL_TWICE: u16 = SEG_BASE + 8; // 结论重复填写
const E_PENDING_LEFT: u16 = SEG_BASE + 9; // 存在 Pending 即封单被拒
const E_FAIL_PRESENT: u16 = SEG_BASE + 10; // 存在 Fail 即封单被拒
const E_BAL: u16 = SEG_BASE + 11; // 守恒恒等式破裂（三态和≠6）
const E_SEALED: u16 = SEG_BASE + 12; // 核对单已封，拒绝再写
const E_EMPTY: u16 = SEG_BASE + 13; // 核对单为空
const E_VOUCH_EMPTY: u16 = SEG_BASE + 14; // 凭证账为空（无从解引）

/// 六验收判据号。
pub const C_ZERO_PATCH: u8 = 1; // 零补丁
pub const C_DRILL: u8 = 2; // 演练矩阵
pub const C_SLA: u8 = 3; // SLA 违约清零
pub const C_API: u8 = 4; // 接口回归
pub const C_CONSERVE: u8 = 5; // 行数守恒
pub const C_GREEN: u8 = 6; // 断言全绿
pub const CRITERIA: usize = 6;

/// 核对结论码。
pub const CL_PASS: u8 = 1;
pub const CL_FAIL: u8 = 2;
pub const CL_PENDING: u8 = 0;

/// 凭证账定容。
pub const MAX_VOUCH: usize = 16;

// ── FNV-1a 64（封单哈希口径）──────────────────────────────────
pub fn fnv1a64(data: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in data {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
    }
    h
}

/// 诊断详情：把错误码翻译成中文可操作建议。
pub fn detail(code: u16) -> &'static str {
    match code {
        E_CRIT_BAD => "判据号越界：六验收判据 1..=6",
        E_CRIT_DUP => "判据号重复：六判据各一行",
        E_CRIT_FULL => "核对单已满 6 行",
        E_CRIT_LACK => "六判据未齐：缺行不许封单",
        E_PTR_ZERO => "凭证指针为 0：先挂凭证再核",
        E_PTR_DANGLE => "凭证指针不可解引：凭证账缺行",
        E_CONCL_BAD => "核对结论值非法：0=待核 1=过 2=不过",
        E_CONCL_TWICE => "结论重复填写：一判据一结论",
        E_PENDING_LEFT => "存在待核项：逐项核完再封单",
        E_FAIL_PRESENT => "存在不过项：先整改再封单",
        E_BAL => "守恒破裂：三态计数之和必须为 6",
        E_SEALED => "核对单已封：封单后只读",
        E_EMPTY => "核对单为空",
        E_VOUCH_EMPTY => "凭证账为空：先落凭证",
        _ => "未知诊断码",
    }
}

// ── 核对结论 ─────────────────────────────────────────────────
/// 核对结论三态。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Conclusion {
    /// 待核。
    Pending,
    /// 过。
    Pass,
    /// 不过。
    Fail,
}

impl Conclusion {
    /// 码（0/1/2）。
    pub fn code(self) -> u8 {
        match self {
            Conclusion::Pending => CL_PENDING,
            Conclusion::Pass => CL_PASS,
            Conclusion::Fail => CL_FAIL,
        }
    }

    /// 中文名。
    pub fn zh(self) -> &'static str {
        match self {
            Conclusion::Pending => "待核",
            Conclusion::Pass => "过",
            Conclusion::Fail => "不过",
        }
    }

    /// 码 → 结论（非法码返回 None 语义用 Err 表示）。
    pub fn from_code(c: u8) -> Result<Conclusion, u16> {
        match c {
            CL_PENDING => Ok(Conclusion::Pending),
            CL_PASS => Ok(Conclusion::Pass),
            CL_FAIL => Ok(Conclusion::Fail),
            _ => Err(E_CONCL_BAD),
        }
    }
}

// ── 凭证账行 ─────────────────────────────────────────────────
/// 凭证账一行：行号 + 凭证描述 + 凭证哈希。
#[derive(Clone, Debug)]
pub struct VoucherRow {
    /// 行号（1 起连续）。
    pub row_no: u32,
    /// 凭证描述。
    pub desc: String,
    /// 凭证哈希（内容指纹）。
    pub hash: u64,
}

// ── 核对单行（三列）──────────────────────────────────────────
/// 核对单一行：判据号 / 凭证指针 / 核对结论。
#[derive(Clone, Debug)]
pub struct CheckRow {
    /// 列一：判据号（1..=6）。
    pub crit: u8,
    /// 列二：凭证指针（凭证账行号）。
    pub voucher: u32,
    /// 列三：核对结论。
    pub concl: Conclusion,
}

// ── 验收核对单（主体）────────────────────────────────────────
/// 移交包验收核对单：凭证账 + 六行三列 + 解引核 + 封单。
pub struct CheckSheet {
    /// 凭证账。
    vouch: [Option<VoucherRow>; MAX_VOUCH],
    vouch_count: usize,
    /// 核对行（定容 6）。
    rows: [Option<CheckRow>; CRITERIA],
    row_count: usize,
    /// 三态计数。
    n_pending: u32,
    n_pass: u32,
    n_fail: u32,
    /// 是否封单。
    sealed: bool,
    /// 封单哈希。
    sealed_hash: u64,
}

impl CheckSheet {
    /// 开单。
    pub fn new() -> CheckSheet {
        CheckSheet {
            vouch: [const { None }; MAX_VOUCH],
            vouch_count: 0,
            rows: [const { None }; CRITERIA],
            row_count: 0,
            n_pending: 0,
            n_pass: 0,
            n_fail: 0,
            sealed: false,
            sealed_hash: 0,
        }
    }

    /// 落凭证：返回行号。
    pub fn put_voucher(&mut self, desc: &str, content: &[u8]) -> Result<u32, u16> {
        if self.sealed {
            return Err(E_SEALED);
        }
        if self.vouch_count >= MAX_VOUCH {
            return Err(E_BAL);
        }
        if desc.is_empty() || content.is_empty() {
            return Err(E_VOUCH_EMPTY);
        }
        let row_no = (self.vouch_count + 1) as u32;
        self.vouch[self.vouch_count] = Some(VoucherRow {
            row_no,
            desc: desc.to_string(),
            hash: fnv1a64(content),
        });
        self.vouch_count += 1;
        Ok(row_no)
    }

    /// 凭证解引：行号 → 凭证行。
    pub fn deref_voucher(&self, row_no: u32) -> Result<&VoucherRow, u16> {
        if self.vouch_count == 0 {
            return Err(E_VOUCH_EMPTY);
        }
        if row_no == 0 || row_no as usize > self.vouch_count {
            return Err(E_PTR_DANGLE);
        }
        match &self.vouch[(row_no - 1) as usize] {
            Some(v) => Ok(v),
            None => Err(E_PTR_DANGLE),
        }
    }

    /// 登记核对行：判据号合法 + 未重复 + 凭证可解引。
    pub fn add_row(&mut self, crit: u8, voucher: u32) -> Result<(), u16> {
        if self.sealed {
            return Err(E_SEALED);
        }
        if crit == 0 || crit as usize > CRITERIA {
            return Err(E_CRIT_BAD);
        }
        if voucher == 0 {
            return Err(E_PTR_ZERO);
        }
        let idx = (crit - 1) as usize;
        if self.rows[idx].is_some() {
            return Err(E_CRIT_DUP);
        }
        // 凭证可解引（登记即核）。
        self.deref_voucher(voucher)?;
        self.rows[idx] = Some(CheckRow {
            crit,
            voucher,
            concl: Conclusion::Pending,
        });
        self.row_count += 1;
        self.n_pending += 1;
        Ok(())
    }

    /// 填结论：Pending → Pass/Fail（一次定案）。
    pub fn set_conclusion(&mut self, crit: u8, pass: bool) -> Result<(), u16> {
        if self.sealed {
            return Err(E_SEALED);
        }
        if crit == 0 || crit as usize > CRITERIA {
            return Err(E_CRIT_BAD);
        }
        let idx = (crit - 1) as usize;
        let r = self.rows[idx].as_mut().ok_or(E_CRIT_BAD)?;
        if r.concl != Conclusion::Pending {
            return Err(E_CONCL_TWICE);
        }
        r.concl = if pass { Conclusion::Pass } else { Conclusion::Fail };
        self.n_pending -= 1;
        if pass {
            self.n_pass += 1;
        } else {
            self.n_fail += 1;
        }
        Ok(())
    }

    /// 三态守恒：pending + pass + fail == 6。
    pub fn conserved(&self) -> bool {
        self.n_pending + self.n_pass + self.n_fail == CRITERIA as u32
    }

    /// 逐项凭证可解引 100% 断言：六行全挂凭证且全可解引。
    pub fn deref_all_assert(&self) -> Result<(), u16> {
        if self.row_count != CRITERIA {
            return Err(E_CRIT_LACK);
        }
        let mut c = 1u8;
        while c as usize <= CRITERIA {
            let r = self.rows[(c - 1) as usize].as_ref().ok_or(E_CRIT_LACK)?;
            self.deref_voucher(r.voucher)?;
            c += 1;
        }
        Ok(())
    }

    /// 核对单齐断言：六行齐 + 三态守恒 + 全 Pass + 零 Fail + 零 Pending
    /// + 解引 100%。
    pub fn complete_assert(&self) -> Result<(), u16> {
        if self.row_count == 0 {
            return Err(E_EMPTY);
        }
        if !self.conserved() {
            return Err(E_BAL);
        }
        self.deref_all_assert()?;
        if self.n_pending > 0 {
            return Err(E_PENDING_LEFT);
        }
        if self.n_fail > 0 {
            return Err(E_FAIL_PRESENT);
        }
        if self.n_pass != CRITERIA as u32 {
            return Err(E_CRIT_LACK);
        }
        Ok(())
    }

    /// 封单：齐断言过 → 封单哈希。
    pub fn seal(&mut self) -> Result<u64, u16> {
        if self.sealed {
            return Err(E_SEALED);
        }
        self.complete_assert()?;
        let mut buf: Vec<u8> = Vec::new();
        let mut c = 1u8;
        while c as usize <= CRITERIA {
            let r = self.rows[(c - 1) as usize].as_ref().ok_or(E_CRIT_LACK)?;
            buf.push(r.crit);
            buf.extend_from_slice(r.voucher.to_le_bytes().as_slice());
            buf.push(r.concl.code());
            c += 1;
        }
        self.sealed_hash = fnv1a64(&buf);
        self.sealed = true;
        Ok(self.sealed_hash)
    }

    /// 封单哈希。
    pub fn sealed_hash(&self) -> u64 {
        self.sealed_hash
    }

    /// 三态计数。
    pub fn tally(&self) -> (u32, u32, u32) {
        (self.n_pending, self.n_pass, self.n_fail)
    }

    /// 行数。
    pub fn row_count(&self) -> usize {
        self.row_count
    }

    /// 凭证数。
    pub fn voucher_count(&self) -> usize {
        self.vouch_count
    }

    /// 核对单行渲染：三列逐行。
    pub fn sheet_lines(&self) -> Vec<String> {
        let mut lines: Vec<String> = Vec::new();
        lines.push("=== 移交包验收核对单 · 六判据 ===".to_string());
        let mut c = 1u8;
        while c as usize <= CRITERIA {
            match &self.rows[(c - 1) as usize] {
                Some(r) => {
                    let mut line = String::from("判据");
                    line.push_str(&itoa(r.crit as u64));
                    line.push_str("|凭证P");
                    line.push_str(&itoa(r.voucher as u64));
                    line.push('|');
                    line.push_str(r.concl.zh());
                    lines.push(line);
                }
                None => {
                    let mut line = String::from("判据");
                    line.push_str(&itoa(c as u64));
                    line.push_str("|缺行");
                    lines.push(line);
                }
            }
            c += 1;
        }
        lines
    }

    /// 凭证复用检测：六行凭证指针必须互异（一判据一凭证）。
    pub fn voucher_unique_assert(&self) -> Result<(), u16> {
        if self.row_count != CRITERIA {
            return Err(E_CRIT_LACK);
        }
        let mut a = 0usize;
        while a < CRITERIA {
            let ra = self.rows[a].as_ref().ok_or(E_CRIT_LACK)?;
            let mut b = a + 1;
            while b < CRITERIA {
                let rb = self.rows[b].as_ref().ok_or(E_CRIT_LACK)?;
                if ra.voucher == rb.voucher {
                    return Err(E_CRIT_DUP);
                }
                b += 1;
            }
            a += 1;
        }
        Ok(())
    }

    /// 三态计数行渲染。
    pub fn tally_lines(&self) -> Vec<String> {
        let mut lines: Vec<String> = Vec::new();
        lines.push("=== 核对结论三态 ===".to_string());
        lines.push("待核: ".to_string() + &itoa(self.n_pending as u64));
        lines.push("过: ".to_string() + &itoa(self.n_pass as u64));
        lines.push("不过: ".to_string() + &itoa(self.n_fail as u64));
        lines
    }

    /// 凭证账行渲染。
    pub fn voucher_lines(&self) -> Vec<String> {
        let mut lines: Vec<String> = Vec::new();
        lines.push("=== 凭证账 ===".to_string());
        let mut i = 0usize;
        while i < self.vouch_count {
            if let Some(v) = &self.vouch[i] {
                let mut line = String::from("P");
                line.push_str(&itoa(v.row_no as u64));
                line.push('|');
                line.push_str(&v.desc);
                line.push_str("|哈希");
                line.push_str(&hex64(v.hash));
                lines.push(line);
            }
            i += 1;
        }
        lines
    }

    /// 判据号 → 凭证指针查询（消费方解引入口）。
    pub fn voucher_of(&self, crit: u8) -> Result<u32, u16> {
        if crit == 0 || crit as usize > CRITERIA {
            return Err(E_CRIT_BAD);
        }
        match &self.rows[(crit - 1) as usize] {
            Some(r) => Ok(r.voucher),
            None => Err(E_CRIT_LACK),
        }
    }
}

/// u64 转 16 位十六进制字符串（确定性小写）。
fn hex64(v: u64) -> String {
    let digits = b"0123456789abcdef";
    let mut s = String::new();
    let mut i = 0usize;
    while i < 16 {
        let shift = 60 - (i as u32) * 4;
        let nib = ((v >> shift) & 0xF) as usize;
        s.push(digits[nib] as char);
        i += 1;
    }
    s
}

/// 极简无符号转十进制。
fn itoa(v: u64) -> String {
    if v == 0 {
        return String::from("0");
    }
    let mut buf = [0u8; 20];
    let mut n = 0usize;
    let mut x = v;
    while x > 0 {
        buf[n] = b'0' + (x % 10) as u8;
        x /= 10;
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

/// 判据号 → 判据名（核对单表头消费口）。
pub const CRITERION_NAMES: [&str; 6] = [
    "零补丁",
    "演练矩阵",
    "SLA 违约清零",
    "接口回归",
    "行数守恒",
    "断言全绿",
];

/// 判据号 → 判据名（越界拒绝）。
pub fn criterion_name(crit: u8) -> Result<&'static str, u16> {
    if crit == 0 || crit as usize > CRITERION_NAMES.len() {
        return Err(E_CRIT_BAD);
    }
    Ok(CRITERION_NAMES[(crit - 1) as usize])
}

// ── 自例程 ───────────────────────────────────────────────────
/// 演练：六判据三列 + 解引 100% + 全 Pass 封单 + 错误路径。返回 0 = 全过。
pub fn drill() -> Result<u32, u16> {
    let mut cs = CheckSheet::new();
    // 落六张凭证。
    let mut p = [0u32; CRITERIA];
    let mut i = 0usize;
    while i < CRITERIA {
        let desc = match criterion_name((i + 1) as u8) {
            Ok(n) => n.to_string(),
            Err(_) => String::from("?"),
        };
        p[i] = cs.put_voucher(&desc, &[b'V', (i + 1) as u8, 0x9])?;
        i += 1;
    }
    if cs.voucher_count() != 6 {
        return Err(SEG_BASE);
    }
    // 六行登记（判据号→凭证指针）。
    i = 0;
    while i < CRITERIA {
        cs.add_row((i + 1) as u8, p[i])?;
        i += 1;
    }
    if cs.row_count() != 6 {
        return Err(SEG_BASE);
    }
    // 结论全 Pass。
    i = 0;
    while i < CRITERIA {
        cs.set_conclusion((i + 1) as u8, true)?;
        i += 1;
    }
    let (pd, ps, fl) = cs.tally();
    if pd != 0 || ps != 6 || fl != 0 {
        return Err(SEG_BASE);
    }
    cs.complete_assert()?;
    cs.voucher_unique_assert()?;
    // 取数口：判据名、凭证指针查询、行渲染。
    if criterion_name(1)? != "零补丁" {
        return Err(SEG_BASE);
    }
    if cs.voucher_of(3)? != p[2] {
        return Err(SEG_BASE);
    }
    let tl = cs.tally_lines();
    if tl.len() != 4 {
        return Err(SEG_BASE);
    }
    let vl = cs.voucher_lines();
    if vl.len() != 7 {
        return Err(SEG_BASE);
    }
    let sl = cs.sheet_lines();
    if sl.len() != 7 {
        return Err(SEG_BASE);
    }
    let h = cs.seal()?;
    if h == 0 {
        return Err(SEG_BASE);
    }
    // 封单后写拒绝。
    match cs.set_conclusion(1, true) {
        Err(c) if c == E_SEALED => {}
        _ => return Err(SEG_BASE),
    }
    // 错误路径：悬垂凭证。
    let mut cs2 = CheckSheet::new();
    let v = cs2.put_voucher("唯一凭证", b"v")?;
    match cs2.add_row(C_ZERO_PATCH, v + 5) {
        Err(c) if c == E_PTR_DANGLE => {}
        _ => return Err(SEG_BASE),
    }
    // 错误路径：零指针。
    match cs2.add_row(C_DRILL, 0) {
        Err(c) if c == E_PTR_ZERO => {}
        _ => return Err(SEG_BASE),
    }
    // 错误路径：存在 Fail 封单被拒。
    let mut cs3 = CheckSheet::new();
    let mut q = [0u32; CRITERIA];
    let mut i = 0usize;
    while i < CRITERIA {
        q[i] = cs3.put_voucher("凭证", &[b'W', (i + 1) as u8])?;
        cs3.add_row((i + 1) as u8, q[i])?;
        i += 1;
    }
    let mut j = 0usize;
    while j < CRITERIA {
        cs3.set_conclusion((j + 1) as u8, j != 4)?; // 判据 5 = Fail。
        j += 1;
    }
    match cs3.complete_assert() {
        Err(c) if c == E_FAIL_PRESENT => {}
        _ => return Err(SEG_BASE),
    }
    Ok(0)
}
