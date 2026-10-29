//! unx48283_latdiag — UNX-F48283 分段账排疑入口件（「中文候选慢」按段定位工具面）（诊断段 0xC440）。
//!
//! 判据原文：
//!   J1 定位 ×10 案例段级归因正确；判据锚定 §AI-61 §5 J-1「排疑入口即本账」。
//!   J2 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计纪律：
//!   1. 管线五段闭集：输入捕获 / 组字 / 词库查 / 候选排 / 上屏渲染，
//!      段号 0..=4 封闭；「中文候选慢」疑案必须在五段内归因，越界段
//!      即工具面失守。
//!   2. 段预算表：五段各持时延预算（ms），实测超预算的段为热点段；
//!      预算表为常量账，改预算必须过账（本卡只读）。
//!   3. 段级归因（J1 主体）：每案实测五段时延，归因取超预算幅度最大
//!      的段（并列取段号小者）；×10 疑案逐案归因与预埋热点段逐一相符。
//!   4. 排疑即入账：每案归因结果落排疑账（案号、热点段、超幅 ms），
//!      账条与案例数守恒；无热点（全段在预算内）即 E_NO_FAULT 不落账。
//!   5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。

use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0xC440;

// ── 诊断码（SEG_BASE+1 起，≤0x1F 个）──────────────────────
const E_NO_FAULT: u16 = SEG_BASE + 1; // 全段在预算内（无热点）
const E_ATTRIB_OFF: u16 = SEG_BASE + 2; // 归因失守
const E_LEDGER_BAD: u16 = SEG_BASE + 3; // 排疑账不守恒
const E_BUDGET_BAD: u16 = SEG_BASE + 4; // 预算表违规
const E_STATE_BAD: u16 = SEG_BASE + 5; // 工具面状态非法
const E_EMPTY_SUM: u16 = SEG_BASE + 6; // 写回摘要为空

/// 管线五段闭集。
pub const SEG_CAPTURE: u8 = 0; // 输入捕获
pub const SEG_COMPOSE: u8 = 1; // 组字
pub const SEG_DICT: u8 = 2; // 词库查
pub const SEG_CAND: u8 = 3; // 候选排
pub const SEG_RENDER: u8 = 4; // 上屏渲染
pub const PIPE_N: usize = 5;

/// 段预算表（ms，只读账）。
pub const BUDGET_MS: [u32; PIPE_N] = [12, 20, 30, 15, 8];

/// 排疑账一条。
#[derive(Clone, Copy, Debug)]
pub struct DiagRow {
    pub case: u32,
    pub seg: u8,
    pub over_ms: u32,
}

/// 疑案：五段实测时延。
#[derive(Clone, Copy, Debug)]
pub struct LatCase {
    pub id: u32,
    pub times: [u32; PIPE_N],
}

/// 排疑入口件。
pub struct LatDiag {
    pub ledger: Vec<DiagRow>,
    pub cases_in: u32,
    pub attributed: u32,
    pub healthy: u32,
}

impl LatDiag {
    pub fn new() -> LatDiag {
        LatDiag { ledger: Vec::new(), cases_in: 0, attributed: 0, healthy: 0 }
    }

    /// 累计超幅概览：逐段汇总超幅（排疑面汇总视图）。
    pub fn overview(&self) -> Result<[u32; PIPE_N], u16> {
        if self.ledger.len() as u32 != self.attributed {
            return Err(E_LEDGER_BAD);
        }
        let mut acc = [0u32; PIPE_N];
        let mut i = 0usize;
        while i < self.ledger.len() {
            let r = self.ledger[i];
            if r.seg as usize >= PIPE_N {
                return Err(E_ATTRIB_OFF);
            }
            acc[r.seg as usize] += r.over_ms;
            i += 1;
        }
        Ok(acc)
    }

    /// 全账超幅总量。
    pub fn total_over(&self) -> Result<u32, u16> {
        let acc = self.overview()?;
        let mut t = 0u32;
        let mut s = 0usize;
        while s < PIPE_N {
            t += acc[s];
            s += 1;
        }
        Ok(t)
    }

    /// 案级归因查账：按案号回收热点段（排疑入口件的对账面）。
    pub fn lookup(&self, case: u32) -> Result<(u8, u32), u16> {
        let mut i = 0usize;
        while i < self.ledger.len() {
            if self.ledger[i].case == case {
                return Ok((self.ledger[i].seg, self.ledger[i].over_ms));
            }
            i += 1;
        }
        Err(E_ATTRIB_OFF)
    }

    /// 段预算表自检（工具面开启自检）。
    pub fn budget_audit() -> Result<(), u16> {
        let mut i = 0usize;
        while i < PIPE_N {
            if BUDGET_MS[i] == 0 {
                return Err(E_BUDGET_BAD); // 零预算段 = 预算表没配
            }
            i += 1;
        }
        Ok(())
    }

    /// 段级归因（J1 主体）：取超预算幅度最大的段（并列取段号小者）。
    pub fn attribute(&mut self, case: &LatCase) -> Result<(u8, u32), u16> {
        self.cases_in += 1;
        // 段数闭集防线的伴随自检：实测段数必为五段。
        let mut best: i64 = -1;
        let mut best_seg: u8 = 0xFF;
        let mut over: u32 = 0;
        let mut s = 0usize;
        while s < PIPE_N {
            let t = case.times[s] as i64;
            let b = BUDGET_MS[s] as i64;
            if t > b {
                let o = (t - b) as u32;
                // 并列取段号小者（先到先得，严格大于才替换）。
                if (o as i64) > best {
                    best = o as i64;
                    best_seg = s as u8;
                    over = o;
                }
            }
            s += 1;
        }
        if best_seg == 0xFF {
            self.healthy += 1;
            return Err(E_NO_FAULT);
        }
        self.ledger.push(DiagRow { case: case.id, seg: best_seg, over_ms: over });
        self.attributed += 1;
        Ok((best_seg, over))
    }

    /// 排疑账审计：账条数=归因数、案号单调不重、热点段闭集、超幅>0。
    pub fn audit(&self) -> Result<(), u16> {
        if self.ledger.len() as u32 != self.attributed {
            return Err(E_LEDGER_BAD);
        }
        if self.cases_in != self.attributed + self.healthy {
            return Err(E_LEDGER_BAD);
        }
        let mut prev: u32 = 0;
        let mut i = 0usize;
        while i < self.ledger.len() {
            let r = self.ledger[i];
            if r.seg as usize >= PIPE_N {
                return Err(E_ATTRIB_OFF);
            }
            if r.over_ms == 0 {
                return Err(E_ATTRIB_OFF);
            }
            if i > 0 && r.case <= prev {
                return Err(E_LEDGER_BAD); // 案号须单调不重
            }
            prev = r.case;
            i += 1;
        }
        Ok(())
    }

    /// 案例生成器（工具面配套）：确定性预埋热点——案 i 热点段 = i%5，
    /// 超幅 = 3+i，其余段压在预算内。
    pub fn plant_case(i: u32) -> LatCase {
        let hot = (i % 5) as usize;
        let mut times = [0u32; PIPE_N];
        let mut s = 0usize;
        while s < PIPE_N {
            if s == hot {
                times[s] = BUDGET_MS[s] + 3 + i;
            } else {
                times[s] = BUDGET_MS[s] - 1 - (i % (BUDGET_MS[s] - 1));
            }
            s += 1;
        }
        LatCase { id: 1 + i, times }
    }

    /// ×N 覆盖核验（J1 硬口径）。
    pub fn coverage(&self, want: u32) -> Result<(), u16> {
        if self.attributed < want {
            return Err(E_ATTRIB_OFF);
        }
        Ok(())
    }

    /// 完成写回（判据 J2）：有归因 + 非空摘要方放行。
    pub fn writeback(&self, done: bool, summary_len: u32) -> Result<(), u16> {
        if !done || self.attributed == 0 {
            return Err(E_STATE_BAD);
        }
        if summary_len == 0 {
            return Err(E_EMPTY_SUM);
        }
        Ok(())
    }
}

/// 自例程判据：逐条落实 J1（定位 ×10 案例段级归因正确），返回命中数。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;

    // R1 主对齐 ×10：确定性预埋 ×10 疑案（热点段轮转五段各两案），
    //    归因段与预埋段逐一相符、超幅逐案相符。
    LatDiag::budget_audit()?;
    let mut ld = LatDiag::new();
    let mut i = 0u32;
    while i < 10 {
        let case = LatDiag::plant_case(i);
        let planted = (i % 5) as u8;
        let want_over = 3 + i;
        let (seg, over) = ld.attribute(&case)?;
        if seg != planted {
            return Err(E_ATTRIB_OFF);
        }
        if over != want_over {
            return Err(E_ATTRIB_OFF);
        }
        i += 1;
    }
    ld.coverage(10)?;
    if ld.attributed != 10 || ld.healthy != 0 {
        return Err(E_LEDGER_BAD);
    }
    ld.audit()?;
    hits += 1;

    // R2 段位分布终判：×10 预埋下五段各恰好归因 2 案（轮转均匀性，
    //    归因器不得偏段）。
    let mut per_seg = [0u32; PIPE_N];
    let mut i = 0usize;
    while i < ld.ledger.len() {
        per_seg[ld.ledger[i].seg as usize] += 1;
        i += 1;
    }
    let mut s = 0usize;
    while s < PIPE_N {
        if per_seg[s] != 2 {
            return Err(E_ATTRIB_OFF);
        }
        s += 1;
    }
    hits += 1;

    // R3 健康案负例：五段全在预算内 → E_NO_FAULT、不落账、健康计数+1。
    let mut ld2 = LatDiag::new();
    let healthy_case = LatCase { id: 1, times: BUDGET_MS };
    if ld2.attribute(&healthy_case).is_ok() {
        return Err(E_NO_FAULT);
    }
    if ld2.attributed != 0 || ld2.ledger.len() != 0 || ld2.healthy != 1 {
        return Err(E_LEDGER_BAD);
    }
    ld2.audit()?;
    hits += 1;

    // R4 并列取小段：两段同超幅 → 归因取段号小者（确定性口径）。
    let mut ld3 = LatDiag::new();
    let mut times = BUDGET_MS;
    times[1] += 9; // 组字超 9ms
    times[3] += 9; // 候选排同超 9ms
    let tie_case = LatCase { id: 2, times };
    let (seg, over) = ld3.attribute(&tie_case)?;
    if seg != SEG_COMPOSE || over != 9 {
        return Err(E_ATTRIB_OFF); // 并列必须取小段号 1
    }
    hits += 1;

    // R5 多热点取最大：组字超 4、词库查超 11 → 归因词库查（幅度大者）。
    let mut ld4 = LatDiag::new();
    let mut times = BUDGET_MS;
    times[1] += 4;
    times[2] += 11;
    let multi_case = LatCase { id: 3, times };
    let (seg, over) = ld4.attribute(&multi_case)?;
    if seg != SEG_DICT || over != 11 {
        return Err(E_ATTRIB_OFF);
    }
    hits += 1;

    // R6 预算表负例防线：预算账只读——归因器按常量表跑，任何段实测
    //    超预算必归因；恰好压线（= 预算）不算超（开界口径）。
    let mut ld5 = LatDiag::new();
    let mut times = BUDGET_MS;
    times[0] = BUDGET_MS[0]; // 恰压线
    times[4] = BUDGET_MS[4] + 1; // 渲染超 1ms
    let edge_case = LatCase { id: 4, times };
    let (seg, over) = ld5.attribute(&edge_case)?;
    if seg != SEG_RENDER || over != 1 {
        return Err(E_ATTRIB_OFF);
    }
    hits += 1;

    // R7 案号单调不重防线：乱序案号入账 → 审计必报 E_LEDGER_BAD。
    let mut ld6 = LatDiag::new();
    let mut times = BUDGET_MS;
    times[2] += 5;
    ld6.attribute(&LatCase { id: 5, times })?;
    ld6.attribute(&LatCase { id: 5, times })?; // 重案号人为入账
    if ld6.audit().is_ok() {
        return Err(E_LEDGER_BAD);
    }
    hits += 1;

    // R8 概览账终判：逐段累计超幅与预埋超幅逐段求和逐位相等、
    //    全账总量守恒（超幅账双口径闭合）。
    let mut want_acc = [0u32; PIPE_N];
    let mut i = 0u32;
    while i < 10 {
        want_acc[(i % 5) as usize] += 3 + i;
        i += 1;
    }
    let acc = ld.overview()?;
    let mut s = 0usize;
    while s < PIPE_N {
        if acc[s] != want_acc[s] {
            return Err(E_LEDGER_BAD);
        }
        s += 1;
    }
    // 总量与逐段和口径对拍。
    let mut sum_acc = 0u32;
    let mut s = 0usize;
    while s < PIPE_N {
        sum_acc += acc[s];
        s += 1;
    }
    if ld.total_over()? != sum_acc {
        return Err(E_LEDGER_BAD);
    }
    hits += 1;

    // R9 案级查账面：×10 案逐案按案号回收 (热点段, 超幅) 与归因结果
    //    逐位相等（排疑入口的对账闭环）。
    let mut i = 0u32;
    while i < 10 {
        let case = LatDiag::plant_case(i);
        let (seg0, over0) = ld.lookup(case.id)?;
        if seg0 != (i % 5) as u8 || over0 != 3 + i {
            return Err(E_ATTRIB_OFF);
        }
        i += 1;
    }
    if ld.lookup(999).is_ok() {
        return Err(E_ATTRIB_OFF); // 查无此案必报
    }
    hits += 1;

    // R10 重放对拍：同批案例重喂新账本，账本逐条逐位相同（归因器
    //    确定性铁证，同输入必同归因）。
    let mut ld2 = LatDiag::new();
    let mut i = 0u32;
    while i < 10 {
        let case = LatDiag::plant_case(i);
        ld2.attribute(&case)?;
        i += 1;
    }
    if ld2.ledger.len() != ld.ledger.len() {
        return Err(E_LEDGER_BAD);
    }
    let mut i = 0usize;
    while i < ld.ledger.len() {
        let a = ld.ledger[i];
        let b = ld2.ledger[i];
        if a.case != b.case || a.seg != b.seg || a.over_ms != b.over_ms {
            return Err(E_ATTRIB_OFF);
        }
        i += 1;
    }
    hits += 1;

    // R11 写回（判据 J2）：零归因不写回、空摘要拒、有账+非空摘要放行。
    let mut ld7 = LatDiag::new();
    if ld7.writeback(true, 0x10).is_ok() {
        return Err(E_STATE_BAD);
    }
    let case = LatDiag::plant_case(0);
    ld7.attribute(&case)?;
    if ld7.writeback(true, 0).is_ok() {
        return Err(E_EMPTY_SUM);
    }
    ld7.writeback(true, 0x20)?;
    hits += 1;

    Ok(hits)
}
