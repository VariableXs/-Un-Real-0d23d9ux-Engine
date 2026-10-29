//! unx48289_mexplic — UNX-F48289 M 段异常显性化终检（全批 error 路径三要素抽查）（诊断段 0xC500）。
//!
//! 判据原文：
//!   J1 抽查 ×20 三要素全齐零裸异常码；判据锚定 体验十三·补。
//!   J2 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计纪律：
//!   1. 显性化三要素：错误路径必须同时登记 ①错误码（同源体系码）、
//!      ②用户话术码（面向用户的可读解释）、③处理建议码（下一步动作
//!      引导）——缺任一即裸异常码（E_BARE_CODE）。
//!   2. 抽查口径（J1）：全批 error 路径登记册按固定步长抽 20 条
//!      （每第 k 条抽一），逐条三要素在位核验；抽不满 20 即登记面
//!      短量（E_SAMPLE_SHORT）。
//!   3. 码位闭集：错误码域 [0xC000,0xD000)、话术码域 [0xE000,0xF000)、
//!      建议码域 [0xF000,0x10000)——三码互串（域错位）即显性化混淆。
//!   4. 终检双账：抽查明细账（路径号、三码）+ 结论账（抽到数、全齐数、
//!      裸码数）；裸码数恒 0 为绿，>0 逐条定位。
//!   5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。

use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0xC500;

// ── 诊断码（SEG_BASE+1 起，≤0x1F 个）──────────────────────
const E_BARE_CODE: u16 = SEG_BASE + 1; // 裸异常码（三要素缺位）
const E_SAMPLE_SHORT: u16 = SEG_BASE + 2; // 抽样短量
const E_DOMAIN_MIX: u16 = SEG_BASE + 3; // 三码域互串
const E_LEDGER_BAD: u16 = SEG_BASE + 4; // 终检账不闭合
const E_STATE_BAD: u16 = SEG_BASE + 5; // 终检器状态非法
const E_EMPTY_SUM: u16 = SEG_BASE + 6; // 写回摘要为空

/// 三要素码域（互斥区间）。
pub const DOM_CODE_LO: u16 = 0xC000; // 错误码域
pub const DOM_CODE_HI: u16 = 0xD000;
pub const DOM_UTTR_LO: u16 = 0xE000; // 话术码域
pub const DOM_UTTR_HI: u16 = 0xF000;
pub const DOM_ADVICE_LO: u16 = 0xF000; // 建议码域
pub const DOM_ADVICE_HI: u32 = 0x1_0000;

/// 抽样口径。
pub const SAMPLE_N: u32 = 20;

/// error 路径登记一条。
#[derive(Clone, Copy, Debug)]
pub struct ErrPath {
    pub path_id: u32,
    pub code: u16,   // 错误码
    pub uttr: u16,   // 用户话术码
    pub advice: u16, // 处理建议码
}

/// 终检器。
pub struct ExplicCheck {
    pub paths: Vec<ErrPath>,
    pub sampled: Vec<ErrPath>,
    pub bare_count: u32,
    pub rounds: u32,
    pub green_rounds: u32,
}

impl ExplicCheck {
    pub fn new() -> ExplicCheck {
        ExplicCheck {
            paths: Vec::new(),
            sampled: Vec::new(),
            bare_count: 0,
            rounds: 0,
            green_rounds: 0,
        }
    }

    /// error 路径登记：三码域各归其位（域错位即拒收）。
    pub fn register(&mut self, path_id: u32, code: u16, uttr: u16, advice: u16) -> Result<(), u16> {
        if code < DOM_CODE_LO || code >= DOM_CODE_HI {
            return Err(E_DOMAIN_MIX);
        }
        if uttr < DOM_UTTR_LO || uttr >= DOM_UTTR_HI {
            return Err(E_DOMAIN_MIX);
        }
        if advice < DOM_ADVICE_LO || (advice as u32) >= DOM_ADVICE_HI {
            return Err(E_DOMAIN_MIX);
        }
        // 路径号不重。
        let mut i = 0usize;
        while i < self.paths.len() {
            if self.paths[i].path_id == path_id {
                return Err(E_LEDGER_BAD);
            }
            i += 1;
        }
        self.paths.push(ErrPath { path_id, code, uttr, advice });
        Ok(())
    }

    pub fn path_count(&self) -> usize {
        self.paths.len()
    }

    /// 三要素在位核验（单条）。
    pub fn check_path(p: &ErrPath) -> Result<(), u16> {
        if p.code == 0 || p.uttr == 0 || p.advice == 0 {
            return Err(E_BARE_CODE);
        }
        if p.code < DOM_CODE_LO || p.code >= DOM_CODE_HI {
            return Err(E_DOMAIN_MIX);
        }
        if p.uttr < DOM_UTTR_LO || p.uttr >= DOM_UTTR_HI {
            return Err(E_DOMAIN_MIX);
        }
        if p.advice < DOM_ADVICE_LO || (p.advice as u32) >= DOM_ADVICE_HI {
            return Err(E_DOMAIN_MIX);
        }
        Ok(())
    }

    /// 抽样终检（J1 主体）：固定步长抽 20 条逐条核验；抽不满 20 报短量；
    /// 裸码逐条定位入账。
    pub fn sample_check(&mut self) -> Result<(), u16> {
        self.rounds += 1;
        self.sampled.clear();
        self.bare_count = 0;
        if self.path_count() < SAMPLE_N as usize {
            return Err(E_SAMPLE_SHORT);
        }
        // 固定步长：每 (n/20) 条抽一，抽足 20。
        let step = self.path_count() / SAMPLE_N as usize;
        let mut s = 0u32;
        while s < SAMPLE_N {
            let idx = s as usize * step;
            let p = self.paths[idx];
            match ExplicCheck::check_path(&p) {
                Ok(()) => {}
                Err(E_BARE_CODE) => {
                    self.bare_count += 1;
                }
                Err(e) => return Err(e),
            }
            self.sampled.push(p);
            s += 1;
        }
        if self.sampled.len() as u32 != SAMPLE_N {
            return Err(E_SAMPLE_SHORT);
        }
        if self.bare_count == 0 {
            self.green_rounds += 1;
            Ok(())
        } else {
            Err(E_BARE_CODE)
        }
    }

    /// 终检账审计：抽样账条数=20、抽样路径号单调递增（步长采样不回访）、
    /// 结论账闭合（全齐数+裸码数=抽到数）。
    pub fn audit(&self) -> Result<(), u16> {
        if self.sampled.len() as u32 != SAMPLE_N {
            return Err(E_SAMPLE_SHORT);
        }
        let mut i = 1usize;
        while i < self.sampled.len() {
            if self.sampled[i].path_id <= self.sampled[i - 1].path_id {
                return Err(E_LEDGER_BAD);
            }
            i += 1;
        }
        let ok = self.sampled.len() as u32 - self.bare_count;
        if ok + self.bare_count != SAMPLE_N {
            return Err(E_LEDGER_BAD);
        }
        if self.green_rounds > self.rounds {
            return Err(E_STATE_BAD);
        }
        Ok(())
    }

    /// 完成写回（判据 J2）：终检绿 + 非空摘要方放行。
    pub fn writeback(&self, done: bool, summary_len: u32) -> Result<(), u16> {
        if !done || self.green_rounds == 0 {
            return Err(E_STATE_BAD);
        }
        if summary_len == 0 {
            return Err(E_EMPTY_SUM);
        }
        Ok(())
    }
}

/// 自例程判据：逐条落实 J1（抽查 ×20 三要素全齐零裸异常码），返回命中数。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;

    // R1 主对齐 ×20：登记 40 条 error 路径（步长 2 抽 20），三要素
    //    全齐，抽检全绿、零裸码。
    let mut ec = ExplicCheck::new();
    let mut i = 0u32;
    while i < 40 {
        ec.register(1 + i, 0xC100 + i as u16, 0xE100 + i as u16, 0xF100 + i as u16)?;
        i += 1;
    }
    if ec.path_count() != 40 {
        return Err(E_LEDGER_BAD);
    }
    ec.sample_check()?;
    if ec.bare_count != 0 {
        return Err(E_BARE_CODE);
    }
    ec.audit()?;
    if ec.sampled.len() != 20 {
        return Err(E_SAMPLE_SHORT);
    }
    hits += 1;

    // R2 抽样定步复核：抽样路径号必为 1,3,5,…,39（步长 2 确定采样），
    //    逐条三码域各归其位。
    let mut i = 0usize;
    while i < ec.sampled.len() {
        let p = ec.sampled[i];
        if p.path_id != (2 * i + 1) as u32 {
            return Err(E_LEDGER_BAD);
        }
        if p.code < DOM_CODE_LO || p.uttr < DOM_UTTR_LO || p.advice < DOM_ADVICE_LO {
            return Err(E_DOMAIN_MIX);
        }
        i += 1;
    }
    hits += 1;

    // R3 裸码负例：话术缺位（uttr=0）登记 —— 域检查先拦（0 不在话术
    //    域）→ E_DOMAIN_MIX；构造域内零值不可达，改以绕域值（错误码
    //    塞进话术域）验证混淆防线。
    let mut ec2 = ExplicCheck::new();
    if ec2.register(1, 0xE999, 0xE100, 0xF100).is_ok() {
        return Err(E_DOMAIN_MIX); // 错误码塞话术域 = 域错位
    }
    if ec2.register(1, 0xC100, 0xC200, 0xF100).is_ok() {
        return Err(E_DOMAIN_MIX); // 话术位塞错误码
    }
    if ec2.register(1, 0xC100, 0xE100, 0xC300).is_ok() {
        return Err(E_DOMAIN_MIX); // 建议位塞错误码
    }
    if ec2.path_count() != 0 {
        return Err(E_LEDGER_BAD);
    }
    hits += 1;

    // R4 短量负例：只登记 19 条 → E_SAMPLE_SHORT（抽不满 20 不出终检）。
    let mut ec3 = ExplicCheck::new();
    let mut i = 0u32;
    while i < 19 {
        ec3.register(1 + i, 0xC100 + i as u16, 0xE100 + i as u16, 0xF100 + i as u16)?;
        i += 1;
    }
    if ec3.sample_check().is_ok() {
        return Err(E_SAMPLE_SHORT);
    }
    hits += 1;

    // R5 路径号重号拒收：同 path_id 二次登记 → E_LEDGER_BAD。
    let mut ec4 = ExplicCheck::new();
    ec4.register(1, 0xC101, 0xE101, 0xF101)?;
    if ec4.register(1, 0xC102, 0xE102, 0xF102).is_ok() {
        return Err(E_LEDGER_BAD);
    }
    if ec4.path_count() != 1 {
        return Err(E_LEDGER_BAD);
    }
    hits += 1;

    // R6 空登记面：零路径直接抽检 → E_SAMPLE_SHORT（先短量后裸码口径）。
    let mut ec5 = ExplicCheck::new();
    if ec5.sample_check().is_ok() {
        return Err(E_SAMPLE_SHORT);
    }
    if ec5.rounds != 1 {
        return Err(E_STATE_BAD);
    }
    hits += 1;

    // R7 绿轮与结论账闭合：连续两轮全绿 → green_rounds=2、bare_count
    //    每轮清账、结论账 (20,0) 闭合。
    ec.sample_check()?;
    ec.sample_check()?;
    if ec.green_rounds != 3 {
        return Err(E_STATE_BAD); // R1 一轮 + 本轮两轮
    }
    if ec.bare_count != 0 {
        return Err(E_BARE_CODE);
    }
    ec.audit()?;
    hits += 1;

    // R8 写回（判据 J2）：零绿轮不写回、空摘要拒、终检绿+非空摘要放行。
    let mut ec6 = ExplicCheck::new();
    if ec6.writeback(true, 0x10).is_ok() {
        return Err(E_STATE_BAD);
    }
    let mut i = 0u32;
    while i < 20 {
        ec6.register(1 + i, 0xC200 + i as u16, 0xE200 + i as u16, 0xF200 + i as u16)?;
        i += 1;
    }
    ec6.sample_check()?;
    if ec6.writeback(true, 0).is_ok() {
        return Err(E_EMPTY_SUM);
    }
    ec6.writeback(true, 0x14)?;
    hits += 1;

    Ok(hits)
}
