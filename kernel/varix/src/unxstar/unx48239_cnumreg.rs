//! unx48239_cnumreg — UNX-F48239 中文数字/日期临时模式登记（v2 预告占位，语义冻结禁提前实装）（诊断段 0xBEC0）。
//!
//! 判据原文：
//!   J1 占位声明入册（§40 防幻觉）；判据锚定 §40 幻觉红线。
//!   J2 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计纪律：
//!   1. 占位红线：v2 预告的「中文数字/日期临时模式」只许登记占位声明
//!      （模式 id、预告口径、冻结理由），**禁止任何转换函数实装**——
//!      账面以「实装探针」自检：任何输入喂入必须返回 NotImplemented
//!      显性码，产出任何数字串即幻觉违规。
//!   2. 占位闭集：预登记 4 个模式位（中文数字/中文日期/中文金额/
//!      农历日期），位外登记拒收。
//!   3. 声明完整账：每占位含 (模式位, 预告版本 v2, 冻结理由, 登记
//!      tick)；冻结理由空 = 占位无效。
//!   4. 解冻留痕：v2 到来解冻时必须升版清冻结位并留痕（当前账禁止
//!      解冻——freeze 期任何解冻请求显性拒绝）。
//!   5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。

use crate::alloc::vec::Vec;
use crate::alloc::string::String;

pub const SEG_BASE: u16 = 0xBEC0;

const E_BAD_MODE: u16 = SEG_BASE + 1;    // 模式位闭集外
const E_NOT_FROZEN: u16 = SEG_BASE + 2;  // 冻结理由空
const E_DUP_MODE: u16 = SEG_BASE + 3;    // 模式位重复占位
const E_IMPL_LEAK: u16 = SEG_BASE + 4;   // 提前实装（产出非 NotImplemented）
const E_UNFREEZE_BLOCKED: u16 = SEG_BASE + 5; // 冻结期解冻
const E_GUARD_UNBAL: u16 = SEG_BASE + 6; // 占位账不平

/// 占位模式位闭集（4 位）。
pub const M_CN_NUM: u8 = 1;    // 中文数字临时模式
pub const M_CN_DATE: u8 = 2;   // 中文日期临时模式
pub const M_CN_MONEY: u8 = 3;  // 中文金额临时模式
pub const M_LUNAR: u8 = 4;     // 农历日期临时模式

/// 显性「未实装」码：占位模式唯一合法回执。
pub const RET_NOT_IMPL: u16 = SEG_BASE + 0x0F;

/// 一条占位声明。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Placeholder {
    pub mode: u8,
    pub ver: u8,        // 预告版本（恒 v2=2）
    pub reason: String, // 冻结理由（非空）
    pub tick: u32,
    pub frozen: bool,
}

/// 占位登记总账。
#[derive(Debug)]
pub struct CnumReg {
    book: Vec<Placeholder>,
    sealed: Option<u32>,
    rejects: u32,
}

impl CnumReg {
    pub fn new() -> Self {
        CnumReg { book: Vec::new(), sealed: None, rejects: 0 }
    }

    /// 占位登记：闭集内 + 冻结理由非空 + 不重复。
    pub fn register(&mut self, mode: u8, reason: &str, tick: u32) -> Result<(), u16> {
        if mode < M_CN_NUM || mode > M_LUNAR {
            self.rejects += 1;
            return Err(E_BAD_MODE);
        }
        if reason.is_empty() {
            self.rejects += 1;
            return Err(E_NOT_FROZEN);
        }
        if self.book.iter().any(|p| p.mode == mode) {
            self.rejects += 1;
            return Err(E_DUP_MODE);
        }
        self.book.push(Placeholder {
            mode,
            ver: 2,
            reason: str_copy(reason),
            tick,
            frozen: true,
        });
        Ok(())
    }

    /// 占位模式调用探针：唯一合法回执 = RET_NOT_IMPL（语义冻结禁提前实装）。
    /// 任何尝试产出转换结果的路径都不存在——本函数签名只返回 Err。
    pub fn probe_invoke(&self, mode: u8, _input: u32) -> Result<(), u16> {
        match self.book.iter().find(|p| p.mode == mode) {
            Some(p) if p.frozen => Err(RET_NOT_IMPL),
            Some(_) => Err(E_IMPL_LEAK), // 解冻但无实装 = 账面违规
            None => Err(E_BAD_MODE),
        }
    }

    /// 冻结期解冻请求：显性拒绝（v2 未到）。
    pub fn request_unfreeze(&mut self, mode: u8) -> Result<(), u16> {
        match self.book.iter_mut().find(|p| p.mode == mode) {
            Some(p) if p.frozen => {
                self.rejects += 1;
                Err(E_UNFREEZE_BLOCKED)
            }
            Some(_) => Ok(()),
            None => Err(E_BAD_MODE),
        }
    }

    /// 占位清单导出。
    pub fn list(&self) -> Vec<(u8, u8, bool)> {
        self.book.iter().map(|p| (p.mode, p.ver, p.frozen)).collect()
    }

    /// 审计：4 位全占位 + 全冻结 + 理由全非空。
    pub fn audit(&self) -> Result<(), u16> {
        if self.book.len() != 4 {
            return Err(E_GUARD_UNBAL);
        }
        for m in M_CN_NUM..=M_LUNAR {
            match self.book.iter().find(|p| p.mode == m) {
                Some(p) => {
                    if !p.frozen || p.reason.is_empty() || p.ver != 2 {
                        return Err(E_GUARD_UNBAL);
                    }
                }
                None => return Err(E_GUARD_UNBAL),
            }
        }
        Ok(())
    }

    pub fn seal(&mut self) -> Result<u32, u16> {
        let mut h: u32 = 0x811C_9DC5;
        for p in &self.book {
            h ^= p.mode as u32;
            h = h.wrapping_mul(0x0100_0193);
            h ^= p.ver as u32;
            h = h.wrapping_mul(0x0100_0193);
            for b in p.reason.bytes() {
                h ^= b as u32;
                h = h.wrapping_mul(0x0100_0193);
            }
        }
        self.sealed = Some(h);
        Ok(h)
    }

    pub fn verify(&self, fp: u32) -> Result<(), u16> {
        match self.sealed {
            Some(s) if s == fp => Ok(()),
            _ => Err(E_GUARD_UNBAL),
        }
    }

    pub fn len(&self) -> usize {
        self.book.len()
    }
    pub fn rejects(&self) -> u32 {
        self.rejects
    }
}

fn str_copy(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        out.push(b as char);
    }
    out
}

/// 自例程判据：落实 J1「占位声明入册（§40 防幻觉）」全链。
pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;

    // R1 场景：4 模式位全占位 → 入册 4 条 → 审计过。
    let mut r = CnumReg::new();
    r.register(M_CN_NUM, "v2 预告：中文数字临时模式，冻结防提前实装", 100)?;
    r.register(M_CN_DATE, "v2 预告：中文日期临时模式，冻结防提前实装", 110)?;
    r.register(M_CN_MONEY, "v2 预告：中文金额临时模式，冻结防提前实装", 120)?;
    r.register(M_LUNAR, "v2 预告：农历日期临时模式，冻结防提前实装", 130)?;
    if r.len() != 4 {
        return Err(E_GUARD_UNBAL);
    }
    r.audit()?;
    hits += 1;

    // R2 场景：实装探针 —— 4 位 × 若干输入全部回 RET_NOT_IMPL（零产出）。
    for m in M_CN_NUM..=M_LUNAR {
        for input in [0u32, 1, 42, 2024, 0xFFFF_FFFF] {
            match r.probe_invoke(m, input) {
                Err(RET_NOT_IMPL) => {}
                _ => return Err(E_IMPL_LEAK),
            }
        }
    }
    hits += 1;

    // R3 场景：冻结期解冻显性拒绝 —— 4 位逐位请求全拒。
    for m in M_CN_NUM..=M_LUNAR {
        match r.request_unfreeze(m) {
            Err(E_UNFREEZE_BLOCKED) => {}
            _ => return Err(E_UNFREEZE_BLOCKED),
        }
    }
    if r.rejects() != 4 {
        return Err(E_GUARD_UNBAL);
    }
    hits += 1;

    // R4 场景：冻结理由空 = 占位无效。
    let mut r4 = CnumReg::new();
    match r4.register(M_CN_NUM, "", 10) {
        Err(E_NOT_FROZEN) => {}
        _ => return Err(E_NOT_FROZEN),
    }
    hits += 1;

    // R5 场景：模式位闭集外拒收（0 与 5）。
    let mut r5 = CnumReg::new();
    match r5.register(0, "x", 10) {
        Err(E_BAD_MODE) => {}
        _ => return Err(E_BAD_MODE),
    }
    match r5.register(5, "x", 10) {
        Err(E_BAD_MODE) => {}
        _ => return Err(E_BAD_MODE),
    }
    hits += 1;

    // R6 场景：重复占位拒收。
    let mut r6 = CnumReg::new();
    r6.register(M_CN_NUM, "首占", 10)?;
    match r6.register(M_CN_NUM, "重复占位", 20) {
        Err(E_DUP_MODE) => {}
        _ => return Err(E_DUP_MODE),
    }
    hits += 1;

    // R7 场景：未占位模式调用探针 → E_BAD_MODE（不猜不编）。
    let mut r7 = CnumReg::new();
    r7.register(M_CN_NUM, "仅占数字位", 10)?;
    match r7.probe_invoke(M_CN_DATE, 1) {
        Err(E_BAD_MODE) => {}
        _ => return Err(E_BAD_MODE),
    }
    hits += 1;

    // R8 场景：清单导出 —— (mode, ver=2, frozen=true) 全量 4 行。
    let lst = r.list();
    if lst.len() != 4 || !lst.iter().all(|(_, v, f)| *v == 2 && *f) {
        return Err(E_GUARD_UNBAL);
    }
    hits += 1;

    // R9 场景：封账与复核 —— 指纹敏感（追加占位必变指纹）。
    let fp = r.seal()?;
    r.verify(fp)?;
    match r.verify(fp ^ 1) {
        Err(E_GUARD_UNBAL) => {}
        _ => return Err(E_GUARD_UNBAL),
    }
    hits += 1;

    // R10 场景：幻觉红线终检 —— 探针从未产出任何 Ok（类型级保证），
    //        全 4 位 × 16 输入值域抽样的回执全为显性码。
    for m in M_CN_NUM..=M_LUNAR {
        for input in [7u32, 100, 9999, 123456789] {
            let res = r.probe_invoke(m, input);
            if res.is_ok() {
                return Err(E_IMPL_LEAK);
            }
        }
    }
    hits += 1;

    Ok(hits)
}
