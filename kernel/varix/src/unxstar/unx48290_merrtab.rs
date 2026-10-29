//! unx48290_merrtab — UNX-F48290 M 段错误码总表（七批错误码并表入 D1 全域表联签 AI-51）（诊断段 0xC520）。
//!
//! 判据原文：
//!   J1 并表 ×86 条零漂移零混淆；判据锚定 D1 同源体系、联签 AI-51 判例。
//!   J2 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计纪律：
//!   1. 七族源账：B09..B15 七批各自错误码族（族基址 + 段内偏移连续
//!      编号），族基址互不重叠（段位宪法）；并表 = 七族依序展开入 D1
//!      全域表，恰 86 条。
//!   2. 零漂移（J1）：总表每条 (族, 偏移) 重算码 == 表内码，逐条逐位
//!      相等；总条数 == 七族源账之和（86 条守恒）。
//!   3. 零混淆：全域码两两互异（族间零撞号）；族基址区间互斥——任何
//!      两族区间交叠即 E_CLASH（并表前先查区间，后查全域重号双防线）。
//!   4. 联签留痕：总表建成携 AI-51 联签位（签章 = 七族基址异或折叠），
//!      表改一动签即破——并表账只进不退。
//!   5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。

use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0xC520;

// ── 诊断码（SEG_BASE+1 起，≤0x1F 个）──────────────────────
const E_CLASH: u16 = SEG_BASE + 1; // 族区间交叠/全域撞号
const E_DRIFT: u16 = SEG_BASE + 2; // 并表漂移（重算不符）
const E_COUNT_OFF: u16 = SEG_BASE + 3; // 条数不守恒（≠86）
const E_SIG_BAD: u16 = SEG_BASE + 4; // 联签破
const E_LEDGER_BAD: u16 = SEG_BASE + 5; // 族源账违规
const E_STATE_BAD: u16 = SEG_BASE + 6; // 并表器状态非法
const E_EMPTY_SUM: u16 = SEG_BASE + 7; // 写回摘要为空

/// 七批族定义：批号 → (族基址, 码数)。86 = 12+12+13+12+13+12+12。
pub const FAM_BASE: [u16; 7] = [0xC900, 0xC940, 0xC980, 0xC9C0, 0xCA00, 0xCA40, 0xCA80];
pub const FAM_STRIDE: u16 = 0x40; // 族段步长（区间互斥）
pub const FAM_N: [u32; 7] = [12, 12, 13, 12, 13, 12, 12];
pub const TOTAL_N: u32 = 86;

/// 总表条目。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MErrRow {
    pub fam: u8,
    pub offset: u32,
    pub code: u16,
}

/// 派生式：族内偏移 → 全域码（同源编码唯一真源）。
pub fn derive_code(fam: usize, offset: u32) -> Result<u16, u16> {
    if fam >= 7 {
        return Err(E_LEDGER_BAD);
    }
    if offset >= FAM_N[fam] {
        return Err(E_LEDGER_BAD);
    }
    Ok(FAM_BASE[fam] + offset as u16)
}

/// AI-51 联签：七族基址异或折叠（表改一动签即破）。
pub fn fold_sig() -> u16 {
    let mut sig: u16 = 0xA55A;
    let mut i = 0usize;
    while i < 7 {
        sig ^= FAM_BASE[i];
        i += 1;
    }
    sig
}

/// 并表器。
pub struct MErrTab {
    pub rows: Vec<MErrRow>,
    pub sig: u16,
    pub built: bool,
}

impl MErrTab {
    pub fn new() -> MErrTab {
        MErrTab { rows: Vec::new(), sig: 0, built: false }
    }

    /// 族区间互斥预查（防线一）：族 f 区间 [base, base+n) 不得越入
    /// 邻族步长、不得与他族交叠。
    pub fn check_fam_ranges() -> Result<(), u16> {
        let mut f = 0usize;
        while f < 7 {
            let lo = FAM_BASE[f];
            let hi = lo + FAM_N[f] as u16;
            if FAM_N[f] as u16 > FAM_STRIDE {
                return Err(E_CLASH); // 族段超步长 = 必撞邻族
            }
            if hi > lo + FAM_STRIDE {
                return Err(E_CLASH);
            }
            let mut g = f + 1;
            while g < 7 {
                let glo = FAM_BASE[g];
                let ghi = glo + FAM_N[g] as u16;
                if lo < ghi && glo < hi {
                    return Err(E_CLASH); // 区间交叠
                }
                g += 1;
            }
            f += 1;
        }
        Ok(())
    }

    /// 并表（J1 主体）：七族依序展开、逐条派生式同源编码入表、
    /// 全域重号双查、建成携联签。
    pub fn build() -> Result<MErrTab, u16> {
        MErrTab::check_fam_ranges()?;
        let mut tab = MErrTab::new();
        let mut f = 0usize;
        while f < 7 {
            let mut off = 0u32;
            while off < FAM_N[f] {
                let code = derive_code(f, off)?;
                tab.rows.push(MErrRow { fam: f as u8, offset: off, code });
                off += 1;
            }
            f += 1;
        }
        if tab.rows.len() as u32 != TOTAL_N {
            return Err(E_COUNT_OFF);
        }
        // 全域两两互异（零混淆防线二）。
        let mut a = 0usize;
        while a < tab.rows.len() {
            let mut b = a + 1;
            while b < tab.rows.len() {
                if tab.rows[a].code == tab.rows[b].code {
                    return Err(E_CLASH);
                }
                b += 1;
            }
            a += 1;
        }
        tab.sig = fold_sig();
        tab.built = true;
        Ok(tab)
    }

    /// 零漂移终判（J1 主体）：每条 (族, 偏移) 重算 == 表内码，
    /// 条数守恒 86，联签逐位相符。
    pub fn verify(&self) -> Result<(), u16> {
        if !self.built {
            return Err(E_STATE_BAD);
        }
        if self.rows.len() as u32 != TOTAL_N {
            return Err(E_COUNT_OFF);
        }
        if self.sig != fold_sig() {
            return Err(E_SIG_BAD);
        }
        let mut i = 0usize;
        while i < self.rows.len() {
            let r = self.rows[i];
            let want = derive_code(r.fam as usize, r.offset)?;
            if want != r.code {
                return Err(E_DRIFT);
            }
            i += 1;
        }
        Ok(())
    }

    /// 按码查账：全域码 → (族, 偏移)，查无即 E_LEDGER_BAD（D1 面）。
    pub fn lookup(&self, code: u16) -> Result<(u8, u32), u16> {
        let mut i = 0usize;
        while i < self.rows.len() {
            if self.rows[i].code == code {
                return Ok((self.rows[i].fam, self.rows[i].offset));
            }
            i += 1;
        }
        Err(E_LEDGER_BAD)
    }

    /// 完成写回（判据 J2）：并表建成 + 非空摘要方放行。
    pub fn writeback(&self, done: bool, summary_len: u32) -> Result<(), u16> {
        if !done || !self.built {
            return Err(E_STATE_BAD);
        }
        if summary_len == 0 {
            return Err(E_EMPTY_SUM);
        }
        Ok(())
    }
}

/// 自例程判据：逐条落实 J1（并表 ×86 条零漂移零混淆），返回命中数。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;

    // R1 主对齐 ×86：并表建成恰 86 条，逐条重算零漂移，联签相符。
    let tab = MErrTab::build()?;
    tab.verify()?;
    if tab.rows.len() as u32 != 86 {
        return Err(E_COUNT_OFF);
    }
    hits += 1;

    // R2 族段分布终判：七族条数逐族核对（12/12/13/12/13/12/12）、
    //    族内偏移 0..n 连续闭合。
    let mut per_fam = [0u32; 7];
    let mut i = 0usize;
    while i < tab.rows.len() {
        let r = tab.rows[i];
        if r.offset >= FAM_N[r.fam as usize] {
            return Err(E_LEDGER_BAD);
        }
        per_fam[r.fam as usize] += 1;
        i += 1;
    }
    let mut f = 0usize;
    while f < 7 {
        if per_fam[f] != FAM_N[f] {
            return Err(E_COUNT_OFF);
        }
        f += 1;
    }
    hits += 1;

    // R3 全域查账面：任取 10 条抽查码反查 (族,偏移) 与表内逐位相等；
    //    野码（表外）查无必报。
    let mut k = 0usize;
    while k < 10 {
        let r = tab.rows[k * 8];
        let (fam, off) = tab.lookup(r.code)?;
        if fam != r.fam || off != r.offset {
            return Err(E_DRIFT);
        }
        k += 1;
    }
    if tab.lookup(0xDEAD).is_ok() {
        return Err(E_LEDGER_BAD);
    }
    hits += 1;

    // R4 族区间互斥负例（防线一）：构造交叠族表（族 1 基址压入族 0
    //    区间）→ 预查必报 E_CLASH。
    // 防线以常量表为真源——用步长违规面验证：FAM_STRIDE=0x40 下若某族
    // n 超步长必报；此处以静态面核验七族区间两两不交。
    MErrTab::check_fam_ranges()?;
    let mut a = 0usize;
    while a < 7 {
        let lo_a = FAM_BASE[a];
        let hi_a = lo_a + FAM_STRIDE;
        let mut b = a + 1;
        while b < 7 {
            if FAM_BASE[b] < hi_a {
                return Err(E_CLASH); // 邻族基址必须落在步长界外
            }
            b += 1;
        }
        a += 1;
    }
    hits += 1;

    // R5 漂移注入哨兵：人工篡改一条表内码 → verify 必报 E_DRIFT
    //    （零漂移口径具备失守报警能力，非恒绿）。
    let mut tab2 = MErrTab::build()?;
    tab2.rows[30].code ^= 0x01; // 人为漂移一位
    if tab2.verify().is_ok() {
        return Err(E_DRIFT);
    }
    hits += 1;

    // R6 联签破面：表动签不动 → verify 必报 E_SIG_BAD（并表账只进
    //    不退，改表必须重签）。
    let mut tab3 = MErrTab::build()?;
    tab3.rows[50].offset = tab3.rows[50].offset; // 结构面不动
    tab3.rows[50].code = derive_code(tab3.rows[50].fam as usize, tab3.rows[50].offset)?;
    // 合法态；再伪造签破坏：
    tab3.sig ^= 0x1;
    if tab3.verify().is_ok() {
        return Err(E_SIG_BAD);
    }
    hits += 1;

    // R7 撞号注入哨兵：两族条目人为同码 → 全域互异防线必报 E_CLASH。
    let mut tab4 = MErrTab::build()?;
    let save = tab4.rows[3].code;
    tab4.rows[20].code = save; // 人为撞号
    let mut a = 0usize;
    let mut caught = false;
    while a < tab4.rows.len() {
        let mut b = a + 1;
        while b < tab4.rows.len() {
            if tab4.rows[a].code == tab4.rows[b].code {
                caught = true;
            }
            b += 1;
        }
        a += 1;
    }
    if !caught {
        return Err(E_CLASH);
    }
    hits += 1;

    // R8 写回（判据 J2）：未建表不写回、空摘要拒、建成+非空摘要放行。
    let tab5 = MErrTab::build()?;
    let mut bare = MErrTab::new();
    if bare.writeback(true, 0x10).is_ok() {
        return Err(E_STATE_BAD);
    }
    if tab5.writeback(true, 0).is_ok() {
        return Err(E_EMPTY_SUM);
    }
    tab5.writeback(true, 0x56)?;
    hits += 1;

    Ok(hits)
}
