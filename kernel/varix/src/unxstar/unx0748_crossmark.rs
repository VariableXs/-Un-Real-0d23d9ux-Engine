//! UNX-F0748 · 域经跨域适用标注（五域消费面） · 诊断段 0x5c80
//! 判据原文摘录：UNX-F0748-J1 跨域标注账齐（逐条适用域列举 + 外推条件列）
//! 且标注与 F0745 跨域维一致 100%、接收域零回执不导出；
//! 完成时 API/软件写回：状态=已完成、结果摘要非空。纯功能代码 ≥580 行。
//!
//! 设计纪律：
//! 1. 五域消费面写死：内存管理 / 并发调度 / 存储文件 / 引导固件 / 安全隔离，
//!    域号 1..=5，域号即标注位图位（bit0..bit4），域集封闭不扩。
//! 2. 逐条标注双列：适用域位图（列一）+ 外推条件码（列二，0..=3 闭集），
//!    双列齐才构成有效标注。
//! 3. 与 F0745 跨域维一致 100%：F0745 跨域适用分 >0 的条目，此处适用域位图
//!    必须非零；跨域分 =0 的条目位图必须为零——逐条双向核对。
//! 4. 接收域零回执不导出：导出前必须采集接收域回执（域号+回执码），
//!    目标域零回执即拒绝导出，拒绝即断言不静默。
//! 5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。
//! 6. 诊断段 0x5c80 为本单独占，XcCode 判别式显式写出。

use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

pub const SEG_BASE: u16 = 0x5c80;

// ── 诊断码闭集（SEG_BASE+1..+N）────────────────────────────────
const E_ID_DUP: u16 = SEG_BASE + 1; // 条目号重复登记
const E_ID_FULL: u16 = SEG_BASE + 2; // 标注账已满
const E_ID_NOTFOUND: u16 = SEG_BASE + 3; // 条目号不存在
const E_DOMAIN_BAD: u16 = SEG_BASE + 4; // 域号越界（1..=5 之外）
const E_DOMAIN_TWICE: u16 = SEG_BASE + 5; // 同条同域重复标注
const E_EXTRAP_BAD: u16 = SEG_BASE + 6; // 外推条件码越界（0..=3）
const E_MARK_LACK: u16 = SEG_BASE + 7; // 标注双列不齐（缺域或缺条件）
const E_CROSS_CONFLICT: u16 = SEG_BASE + 8; // 与 F0745 跨域维冲突（双向核对失败）
const E_NO_RECEIPT: u16 = SEG_BASE + 9; // 接收域零回执，导出被拒（预期拦截码）
const E_RECEIPT_DUP: u16 = SEG_BASE + 10; // 同域重复回执
const E_BAL: u16 = SEG_BASE + 11; // 守恒恒等式破裂
const E_SEALED: u16 = SEG_BASE + 12; // 标注账已封，拒绝再写
const E_EMPTY: u16 = SEG_BASE + 13; // 标注账为空
const E_NAME_EMPTY: u16 = SEG_BASE + 14; // 条目名为空

/// 标注账定容。
pub const MAX_ITEMS: usize = 24;

/// 五域消费面域号（位图位 = 域号-1）。
pub const D_MEM: u8 = 1; // 内存管理
pub const D_SCHED: u8 = 2; // 并发调度
pub const D_FS: u8 = 3; // 存储文件
pub const D_BOOT: u8 = 4; // 引导固件
pub const D_SEC: u8 = 5; // 安全隔离
pub const DOMAINS: u8 = 5;

/// 全域位图满值（bit0..bit4 全 1）。
pub const ALL_DOMAINS: u8 = 0b11111;

/// 外推条件码闭集。
pub const X_DIRECT: u8 = 0; // 直接外推：无需适配
pub const X_RENAME: u8 = 1; // 需重命名符号
pub const X_RESIZE: u8 = 2; // 需调容量参数
pub const X_REWORK: u8 = 3; // 需重写适配层（外推代价高）

// ── FNV-1a 64（封账哈希口径）──────────────────────────────────
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
        E_ID_DUP => "条目号重复：登记序自动编号",
        E_ID_FULL => "标注账已满 24 条",
        E_ID_NOTFOUND => "条目号不存在：按登记序 1..N",
        E_DOMAIN_BAD => "域号越界：五域闭集 1..=5",
        E_DOMAIN_TWICE => "同条同域重复标注：位图去重",
        E_EXTRAP_BAD => "外推条件码越界：0..=3 闭集",
        E_MARK_LACK => "标注双列不齐：域集与外推条件须同落",
        E_CROSS_CONFLICT => "与 F0745 跨域维冲突：逐条双向核对",
        E_NO_RECEIPT => "接收域零回执：先采回执再导出（预期拦截）",
        E_RECEIPT_DUP => "同域重复回执：一域一回执",
        E_BAL => "守恒破裂：标注数与回执数账不平",
        E_SEALED => "标注账已封：封账后只读",
        E_EMPTY => "标注账为空",
        E_NAME_EMPTY => "条目名为空",
        _ => "未知诊断码",
    }
}

// ── 外推条件 ─────────────────────────────────────────────────
/// 外推条件码的中文名与外推代价分。
pub fn extrap_info(code: u8) -> (&'static str, u32) {
    match code {
        X_DIRECT => ("直接外推", 0),
        X_RENAME => ("重命名符号", 1),
        X_RESIZE => ("调容量参数", 2),
        X_REWORK => ("重写适配层", 3),
        _ => ("非法码", 4),
    }
}

/// F0745 跨域适用分的对侧声明：分值 → 期望最小适用域数。
/// 跨域分 0..=5 → 期望域数 = 分值（一分一域的保守口径）。
pub fn expected_domains_for_cross_score(score: u8) -> u32 {
    score as u32
}

// ── 标注条目 ─────────────────────────────────────────────────
/// 一条跨域适用标注：双列（域位图 + 外推条件）。
#[derive(Clone, Debug)]
pub struct CrossMark {
    /// 条目号（1 起连续）。
    pub item_id: u32,
    /// 条目名。
    pub name: String,
    /// 列一：适用域位图（bit0..bit4 对应域 1..=5）。
    pub domain_bits: u8,
    /// 列二：外推条件码（0..=3）。
    pub extrap: u8,
    /// 与 F0745 跨域维的核对结果（None=未核）。
    pub cross_checked: Option<bool>,
}

impl CrossMark {
    /// 双列齐备：域位图非零或显式零域（跨域分 0 时允许 0），
    /// 外推条件码在闭集。零域条目的外推码必须也是 0（直接外推无从谈起）。
    pub fn well_formed(&self) -> bool {
        if self.extrap > X_REWORK {
            return false;
        }
        if self.domain_bits & !ALL_DOMAINS != 0 {
            return false;
        }
        if self.domain_bits == 0 && self.extrap != X_DIRECT {
            return false;
        }
        true
    }

    /// 适用域数（位计数）。
    pub fn domain_count(&self) -> u32 {
        let mut n = 0u32;
        let mut b = self.domain_bits;
        while b != 0 {
            n += (b & 1) as u32;
            b >>= 1;
        }
        n
    }

    /// 指定域是否适用。
    pub fn applies(&self, domain: u8) -> bool {
        if domain == 0 || domain > DOMAINS {
            return false;
        }
        self.domain_bits & (1u8 << (domain - 1)) != 0
    }
}

// ── 接收域回执 ───────────────────────────────────────────────
/// 接收域回执行：域号 + 是否接收。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ReceiptRow {
    /// 域号（1..=5）。
    pub domain: u8,
    /// 回执码（1=接收，0=拒收）。
    pub accept: u8,
}

// ── 跨域标注账（主体）────────────────────────────────────────
/// 域经跨域适用标注账：双列标注 + F0745 跨域维核对 + 回执门控导出。
pub struct CrossMarkLedger {
    marks: [Option<CrossMark>; MAX_ITEMS],
    item_count: usize,
    /// 回执账。
    receipts: Vec<ReceiptRow>,
    /// 回执数（守恒分子之一）。
    receipt_count: usize,
    /// 是否封账。
    sealed: bool,
}

impl CrossMarkLedger {
    /// 开账。
    pub fn new() -> CrossMarkLedger {
        CrossMarkLedger {
            marks: core::array::from_fn(|_| None),
            item_count: 0,
            receipts: Vec::new(),
            receipt_count: 0,
            sealed: false,
        }
    }

    /// 登记条目（未标注态）。
    pub fn enroll(&mut self, name: &str) -> Result<u32, u16> {
        if self.sealed {
            return Err(E_SEALED);
        }
        if self.item_count >= MAX_ITEMS {
            return Err(E_ID_FULL);
        }
        if name.is_empty() {
            return Err(E_NAME_EMPTY);
        }
        let item_id = (self.item_count + 1) as u32;
        self.marks[self.item_count] = Some(CrossMark {
            item_id,
            name: name.to_string(),
            domain_bits: 0,
            extrap: 0,
            cross_checked: None,
        });
        self.item_count += 1;
        Ok(item_id)
    }

    /// 定位。
    fn slot_of(&self, item_id: u32) -> Result<usize, u16> {
        if item_id == 0 || item_id as usize > self.item_count {
            return Err(E_ID_NOTFOUND);
        }
        Ok((item_id - 1) as usize)
    }

    /// 标注一个适用域：域号闭集 + 位图去重。
    pub fn mark_domain(&mut self, item_id: u32, domain: u8) -> Result<(), u16> {
        if self.sealed {
            return Err(E_SEALED);
        }
        if domain == 0 || domain > DOMAINS {
            return Err(E_DOMAIN_BAD);
        }
        let s = self.slot_of(item_id)?;
        let m = self.marks[s].as_mut().ok_or(E_ID_NOTFOUND)?;
        let bit = 1u8 << (domain - 1);
        if m.domain_bits & bit != 0 {
            return Err(E_DOMAIN_TWICE);
        }
        m.domain_bits |= bit;
        Ok(())
    }

    /// 设外推条件码（闭集 0..=3）。
    pub fn set_extrap(&mut self, item_id: u32, extrap: u8) -> Result<(), u16> {
        if self.sealed {
            return Err(E_SEALED);
        }
        if extrap > X_REWORK {
            return Err(E_EXTRAP_BAD);
        }
        let s = self.slot_of(item_id)?;
        let m = self.marks[s].as_mut().ok_or(E_ID_NOTFOUND)?;
        m.extrap = extrap;
        Ok(())
    }

    /// 与 F0745 跨域维一致性核对（逐条双向）：
    /// 跨域分 >0 → 位图必须非零且域数 ≥ 期望最小域数；
    /// 跨域分 =0 → 位图必须为零。核对结果落条目。
    pub fn cross_check(&mut self, item_id: u32, f0745_cross_score: u8) -> Result<bool, u16> {
        if self.sealed {
            return Err(E_SEALED);
        }
        if f0745_cross_score > 5 {
            return Err(E_EXTRAP_BAD);
        }
        let s = self.slot_of(item_id)?;
        let m = self.marks[s].as_mut().ok_or(E_ID_NOTFOUND)?;
        if !m.well_formed() {
            m.cross_checked = Some(false);
            return Ok(false);
        }
        let expect = expected_domains_for_cross_score(f0745_cross_score);
        let ok = if f0745_cross_score == 0 {
            m.domain_bits == 0
        } else {
            m.domain_bits != 0 && m.domain_count() >= expect
        };
        m.cross_checked = Some(ok);
        Ok(ok)
    }

    /// 全账一致性 100% 断言：逐条 cross_checked==Some(true)。
    pub fn cross_consistency_assert(&self) -> Result<(), u16> {
        if self.item_count == 0 {
            return Err(E_EMPTY);
        }
        let mut i = 0usize;
        while i < self.item_count {
            match &self.marks[i] {
                Some(m) => match m.cross_checked {
                    Some(true) => {}
                    _ => return Err(E_CROSS_CONFLICT),
                },
                None => return Err(E_ID_NOTFOUND),
            }
            i += 1;
        }
        Ok(())
    }

    /// 采集接收域回执：域号闭集 + 一域一回执。
    pub fn put_receipt(&mut self, domain: u8, accept: bool) -> Result<(), u16> {
        if self.sealed {
            return Err(E_SEALED);
        }
        if domain == 0 || domain > DOMAINS {
            return Err(E_DOMAIN_BAD);
        }
        let mut i = 0usize;
        while i < self.receipts.len() {
            if self.receipts[i].domain == domain {
                return Err(E_RECEIPT_DUP);
            }
            i += 1;
        }
        self.receipts.push(ReceiptRow {
            domain,
            accept: if accept { 1 } else { 0 },
        });
        self.receipt_count += 1;
        Ok(())
    }

    /// 指定域回执读取：None=零回执。
    pub fn receipt_of(&self, domain: u8) -> Option<bool> {
        let mut i = 0usize;
        while i < self.receipts.len() {
            if self.receipts[i].domain == domain {
                return Some(self.receipts[i].accept == 1);
            }
            i += 1;
        }
        None
    }

    /// 导出条目到指定域：域必须适用 + 域回执必须接收（零回执不导出）。
    pub fn export_to(&self, item_id: u32, domain: u8) -> Result<(String, u8), u16> {
        if self.item_count == 0 {
            return Err(E_EMPTY);
        }
        if domain == 0 || domain > DOMAINS {
            return Err(E_DOMAIN_BAD);
        }
        let s = self.slot_of(item_id)?;
        let m = self.marks[s].as_ref().ok_or(E_ID_NOTFOUND)?;
        if !m.applies(domain) {
            return Err(E_DOMAIN_BAD);
        }
        // 零回执不导出（核心拦截）。
        match self.receipt_of(domain) {
            Some(true) => {}
            Some(false) => return Err(E_NO_RECEIPT),
            None => return Err(E_NO_RECEIPT),
        }
        Ok((m.name.clone(), m.extrap))
    }

    /// 回执守恒：回执账行数 == 回执计数，且域号互异。
    pub fn receipts_conserved(&self) -> bool {
        self.receipts.len() == self.receipt_count
            && self.receipts.len() <= DOMAINS as usize
    }

    /// 条目数。
    pub fn len(&self) -> usize {
        self.item_count
    }

    /// 封账：一致性 100% + 全条目 well_formed + 回执守恒后冻结。
    pub fn seal(&mut self) -> Result<u64, u16> {
        if self.sealed {
            return Err(E_SEALED);
        }
        self.cross_consistency_assert()?;
        if !self.receipts_conserved() {
            return Err(E_BAL);
        }
        let mut buf: Vec<u8> = Vec::new();
        let mut i = 0usize;
        while i < self.item_count {
            if let Some(m) = &self.marks[i] {
                buf.extend_from_slice(m.item_id.to_le_bytes().as_slice());
                buf.extend_from_slice(m.name.as_bytes());
                buf.push(0x1f);
                buf.push(m.domain_bits);
                buf.push(m.extrap);
            }
            i += 1;
        }
        let mut j = 0usize;
        while j < self.receipts.len() {
            buf.push(self.receipts[j].domain);
            buf.push(self.receipts[j].accept);
            j += 1;
        }
        self.sealed = true;
        Ok(fnv1a64(&buf))
    }

    /// 标注账行渲染：双列逐条。
    pub fn audit_lines(&self) -> Vec<String> {
        let mut lines: Vec<String> = Vec::new();
        lines.push("=== 域经跨域适用标注 · 五域消费面 ===".to_string());
        let mut i = 0usize;
        while i < self.item_count {
            if let Some(m) = &self.marks[i] {
                let mut line = String::from("X");
                line.push_str(&itoa(m.item_id as u64));
                line.push('|');
                line.push_str(&m.name);
                line.push_str("|域位图 ");
                line.push_str(&itoa(m.domain_bits as u64));
                line.push_str("|外推 ");
                let (zh, _) = extrap_info(m.extrap);
                line.push_str(zh);
                line.push_str("|核 ");
                line.push_str(match m.cross_checked {
                    Some(true) => "一致",
                    Some(false) => "冲突",
                    None => "未核",
                });
                lines.push(line);
            }
            i += 1;
        }
        lines
    }

    /// 五域热度盘点：域号 → 适用条目数（消费面负载取数口）。
    pub fn domain_tally(&self) -> [u32; 6] {
        let mut tally = [0u32; 6];
        let mut i = 0usize;
        while i < self.item_count {
            if let Some(m) = &self.marks[i] {
                let mut d = 1u8;
                while d <= DOMAINS {
                    if m.applies(d) {
                        tally[d as usize] += 1;
                    }
                    d += 1;
                }
            }
            i += 1;
        }
        tally
    }

    /// 未标注条目清单：已登记但双列未齐的条目号（账面健康度取数口）。
    pub fn unmarked_items(&self) -> Vec<u32> {
        let mut out: Vec<u32> = Vec::new();
        let mut i = 0usize;
        while i < self.item_count {
            if let Some(m) = &self.marks[i] {
                // 双列齐 = 域列已落（含显式零域）且核对已完成。
                if m.cross_checked.is_none() {
                    out.push(m.item_id);
                }
            }
            i += 1;
        }
        out
    }

    /// 回执汇总行渲染。
    pub fn receipt_lines(&self) -> Vec<String> {
        let mut lines: Vec<String> = Vec::new();
        lines.push("=== 接收域回执账 ===".to_string());
        let mut i = 0usize;
        while i < self.receipts.len() {
            let r = &self.receipts[i];
            let mut line = String::from("R|域");
            line.push_str(&itoa(r.domain as u64));
            line.push('|');
            line.push_str(if r.accept == 1 { "接收" } else { "拒收" });
            lines.push(line);
            i += 1;
        }
        lines
    }
}

/// 极简无符号转十进制。
fn itoa(mut v: u64) -> String {
    if v == 0 {
        return String::from("0");
    }
    let mut buf = [0u8; 20];
    let mut n = 0usize;
    while v > 0 {
        buf[n] = b'0' + (v % 10) as u8;
        v /= 10;
        n += 1;
    }
    s_impl(&buf, n)
}

/// 十进制渲染实现（itoa 的可复用核心）。
fn s_impl(buf: &[u8; 20], n: usize) -> String {
    let mut s = String::new();
    let mut i = n;
    while i > 0 {
        i -= 1;
        s.push(buf[i] as char);
    }
    s
}

/// u64 转十进制（供外部渲染复用）。
pub fn render_u64(v: u64) -> String {
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
    s_impl(&buf, n)
}

// ── 自例程 ───────────────────────────────────────────────────
/// 演练：双列标注 + F0745 跨域维一致 100% + 零回执不导出。返回 0 = 全过。
pub fn drill() -> Result<u32, u16> {
    let mut lg = CrossMarkLedger::new();
    // 两条样本：跨域分 3 与跨域分 0。
    let a = lg.enroll("定容账本模式")?;
    let b = lg.enroll("引导期独占技巧")?;
    // a：适用内存/调度/安全三域，外推=重命名。
    lg.mark_domain(a, D_MEM)?;
    lg.mark_domain(a, D_SCHED)?;
    lg.mark_domain(a, D_SEC)?;
    lg.set_extrap(a, X_RENAME)?;
    // b：零域（跨域分 0 对应），外推=直接。
    lg.set_extrap(b, X_DIRECT)?;
    // 与 F0745 跨域维核对：a 分 3、b 分 0。
    if !lg.cross_check(a, 3)? {
        return Err(SEG_BASE);
    }
    if !lg.cross_check(b, 0)? {
        return Err(SEG_BASE);
    }
    lg.cross_consistency_assert()?;
    // 零回执不导出：先拒。
    match lg.export_to(a, D_MEM) {
        Err(c) if c == E_NO_RECEIPT => {}
        _ => return Err(SEG_BASE),
    }
    // 采内存域回执（拒收）→ 仍拒；改收 → 放行。
    lg.put_receipt(D_MEM, false)?;
    match lg.export_to(a, D_MEM) {
        Err(c) if c == E_NO_RECEIPT => {}
        _ => return Err(SEG_BASE),
    }
    match lg.put_receipt(D_MEM, true) {
        Err(c) if c == E_RECEIPT_DUP => {}
        _ => return Err(SEG_BASE),
    }
    lg.put_receipt(D_SCHED, true)?;
    let (name, xp) = lg.export_to(a, D_SCHED)?;
    if name != "定容账本模式" || xp != X_RENAME {
        return Err(SEG_BASE);
    }
    // 不适用域导出拒绝（域未标注）。
    match lg.export_to(a, D_FS) {
        Err(c) if c == E_DOMAIN_BAD => {}
        _ => return Err(SEG_BASE),
    }
    if !lg.receipts_conserved() {
        return Err(SEG_BASE);
    }
    // 五域热度与未标注清单取数口。
    let dt = lg.domain_tally();
    if dt[D_MEM as usize] != 1 || dt[D_SCHED as usize] != 1 || dt[D_FS as usize] != 0 {
        return Err(SEG_BASE);
    }
    let unmarked = lg.unmarked_items();
    if !unmarked.is_empty() {
        return Err(SEG_BASE);
    }
    let rl = lg.receipt_lines();
    if rl.len() != 3 {
        return Err(SEG_BASE);
    }
    let hash = lg.seal()?;
    if hash == 0 {
        return Err(SEG_BASE);
    }
    // 封账后标注拒绝。
    match lg.mark_domain(a, D_BOOT) {
        Err(c) if c == E_SEALED => {}
        _ => return Err(SEG_BASE),
    }
    // 错误路径：外推码越界。
    let mut lg2 = CrossMarkLedger::new();
    let x = lg2.enroll("越界样本")?;
    match lg2.set_extrap(x, 4) {
        Err(c) if c == E_EXTRAP_BAD => {}
        _ => return Err(SEG_BASE),
    }
    // 一致性核对冲突样本：跨域分 2 但只标 1 域。
    let y = lg2.enroll("冲突样本")?;
    lg2.mark_domain(y, D_BOOT)?;
    match lg2.cross_check(y, 2) {
        Ok(false) => {}
        _ => return Err(SEG_BASE),
    }
    match lg2.cross_consistency_assert() {
        Err(c) if c == E_CROSS_CONFLICT => {}
        _ => return Err(SEG_BASE),
    }
    Ok(0)
}
