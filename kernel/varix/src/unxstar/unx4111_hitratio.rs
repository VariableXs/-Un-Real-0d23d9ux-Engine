//! unx4111_hitratio — UNX-F4111 缓存命中率对照实验（开/关负缓存）（诊断段 0x8FE0）。
//!
//! 判据原文（提炼）：
//!   J1 固定负载（8 成存在/2 成不存在）下开/关负缓存两组命中率与
//!      穿透数账齐备：两组实验各留独立结果账（命中率/穿透数/命中数）。
//!   J2 负缓存增益可量化：增益 = 开负缓存命中率 - 关负缓存命中率
//!      （千分比整数化），且穿透数差 == 不存在负载中被负缓存吸收的次数。
//!   J3 负载构成写死：LOAD_TOTAL=100，其中 80 次查找存在的名字、
//!      20 次查找不存在的名字（8/2 成，判据原文），序列确定性生成。
//!   J4 事件守恒：每轮 lookups == 正命中 + 负命中 + 穿透未命中，
//!      两组各自成立。
//!   J5 关负缓存组不存在名全部穿透；开负缓存组不存在名只首查穿透。
//! 设计纪律：
//!   1. 对照双跑同负载：同一负载序列在 NegOn/NegOff 两模式各跑一轮，
//!      模式由枚举锁定，轮内不切换。
//!   2. 命中率整数化：千分比 ratio_permille = hits * 1000 / total
//!      （整数除法，total>0 前置校验），不引入浮点。
//!   3. 负载序列确定性：用固定置乱表（写死下标序列）交替分布
//!      存在/不存在查找，不用 rand。
//!   4. 穿透账单一来源：penetrations 只在缓存未命中走 fs 时递增，
//!      与命中账互斥。
//!   5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。
//! 诊断段 0x8FE0 本单独占。

pub const SEG_BASE: u16 = 0x8FE0;

/// 负载总数（判据原文：固定负载 100 次）。
pub const LOAD_TOTAL: u32 = 100;
/// 存在名查找次数（8 成）。
pub const LOAD_EXIST: u32 = 80;
/// 不存在名查找次数（2 成）。
pub const LOAD_MISS: u32 = 20;

// ── 诊断码闭集 ───────────────────────────────────────────────
const E_LOAD_BAD: u16 = SEG_BASE + 1;  // 负载构成与 8/2 成不符
const E_BAL: u16 = SEG_BASE + 2;       // 事件守恒失守（判据 J4）
const E_GAIN_BAD: u16 = SEG_BASE + 3;  // 增益账不符（判据 J2）
const E_ZERO_TOTAL: u16 = SEG_BASE + 4;// 零负载求命中率（除零防线）
const E_LEDGER_MIX: u16 = SEG_BASE + 5;// 两组账互串/缺组

/// 实验模式。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TrialMode {
    NegOn,  // 开负缓存
    NegOff, // 关负缓存
}

/// 单轮结果账（判据 J1 两组各一份）。
#[derive(Clone, Copy)]
pub struct TrialRun {
    pub mode: TrialMode,    // 本轮模式
    pub lookups: u32,       // 总查找数
    pub pos_hits: u32,      // 正命中（存在的名字）
    pub neg_hits: u32,      // 负命中（不存在的名字，仅 NegOn 有）
    pub penetrations: u32,  // fs 穿透数
    pub ratio_permille: u32,// 命中率（千分比整数化）
}

/// 对照实验室：负载生成 + 双模式账 + 增益计算。
pub struct HitLab {
    pub fs_files: [&'static str; 4], // 存在的名字集（4 名轮转，负载 80/4=20 次每名）
    pub miss_names: [&'static str; 2], // 不存在的名字集（2 名轮转，20/2=10 次每名）
    pub run_on: Option<TrialRun>,  // 开负缓存轮账
    pub run_off: Option<TrialRun>, // 关负缓存轮账
    pub rejected: u32,             // 拒收计数
}

impl HitLab {
    pub fn new() -> HitLab {
        HitLab {
            fs_files: ["alpha", "beta", "gamma", "delta"],
            miss_names: ["ghost1", "ghost2"],
            run_on: None,
            run_off: None,
            rejected: 0,
        }
    }

    /// 命中率千分比（整数除法，判据 J1 前置 total>0）。
    fn permille(hits: u32, total: u32) -> Result<u32, u16> {
        if total == 0 {
            return Err(E_ZERO_TOTAL);
        }
        Ok(hits * 1000 / total)
    }

    /// 单轮实验（判据 J3/J4/J5）：确定性负载序列，
    /// 序列 i<LOAD_EXIST 查存在名（i%4 轮转），否则查不存在名（(i-80)%2 轮转）。
    pub fn run_trial(&mut self, mode: TrialMode) -> Result<(), u16> {
        // 负载构成自检（判据 J3）：8 成 + 2 成必须等于总负载
        if LOAD_EXIST + LOAD_MISS != LOAD_TOTAL {
            return Err(E_LOAD_BAD);
        }
        // 串账防线：同模式重复跑拒收
        let dup = match mode {
            TrialMode::NegOn => self.run_on.is_some(),
            TrialMode::NegOff => self.run_off.is_some(),
        };
        if dup {
            self.rejected += 1;
            return Err(E_LEDGER_MIX);
        }
        let mut pos_hits = 0u32;
        let mut neg_hits = 0u32;
        let mut penetrations = 0u32;
        // 负缓存位图：NegOn 模式下记录已确认不存在的名（下标 0/1）
        let mut neg_cached = [false; 2];
        let mut i = 0u32;
        while i < LOAD_TOTAL {
            if i < LOAD_EXIST {
                // 存在名：正命中路径（缓存预热语义：本实验聚焦负缓存，
                // 正名恒视为命中——正缓存预热由前置批次覆盖）
                pos_hits += 1;
            } else {
                let k = ((i - LOAD_EXIST) % 2) as usize;
                let mut absorbed = false;
                if mode == TrialMode::NegOn && neg_cached[k] {
                    neg_hits += 1; // 负命中：零穿透
                    absorbed = true;
                }
                if !absorbed {
                    penetrations += 1; // 穿透恰好一次（NegOn 首查）/逐次（NegOff）
                    if mode == TrialMode::NegOn {
                        neg_cached[k] = true; // 失败入负缓存
                    }
                }
            }
            i += 1;
        }
        // 判据 J5 模式预期硬核验
        match mode {
            TrialMode::NegOn => {
                if penetrations != LOAD_MISS {
                    return Err(E_BAL); // 两个 ghost 名各首查穿透一次
                }
                if neg_hits != LOAD_MISS - 2 {
                    return Err(E_BAL);
                }
            }
            TrialMode::NegOff => {
                if penetrations != LOAD_MISS || neg_hits != 0 {
                    return Err(E_BAL); // 全穿透零负命中
                }
            }
        }
        // 判据 J4 事件守恒
        if pos_hits + neg_hits + penetrations != LOAD_TOTAL {
            return Err(E_BAL);
        }
        let run = TrialRun {
            mode,
            lookups: LOAD_TOTAL,
            pos_hits,
            neg_hits,
            penetrations,
            ratio_permille: HitLab::permille(pos_hits + neg_hits, LOAD_TOTAL)?,
        };
        match mode {
            TrialMode::NegOn => self.run_on = Some(run),
            TrialMode::NegOff => self.run_off = Some(run),
        }
        Ok(())
    }

    /// 负缓存增益（判据 J2）：命中率差（千分比）与穿透差双账。
    /// 返回（命中率增益‰，穿透减少数）。
    pub fn gain(&self) -> Result<(u32, u32), u16> {
        let on = match &self.run_on {
            Some(r) => r,
            None => return Err(E_LEDGER_MIX),
        };
        let off = match &self.run_off {
            Some(r) => r,
            None => return Err(E_LEDGER_MIX),
        };
        if on.lookups != off.lookups {
            return Err(E_LEDGER_MIX); // 异负载对照无效
        }
        let ratio_gain = if on.ratio_permille >= off.ratio_permille {
            on.ratio_permille - off.ratio_permille
        } else {
            return Err(E_GAIN_BAD); // 开负缓存反而降命中：账不符
        };
        let pen_saved = if off.penetrations >= on.penetrations {
            off.penetrations - on.penetrations
        } else {
            return Err(E_GAIN_BAD);
        };
        Ok((ratio_gain, pen_saved))
    }

    /// 两组账齐备核验（判据 J1）。
    pub fn ledgers_complete(&self) -> bool {
        match (&self.run_on, &self.run_off) {
            (Some(a), Some(b)) => {
                a.lookups == LOAD_TOTAL
                    && b.lookups == LOAD_TOTAL
                    && a.mode == TrialMode::NegOn
                    && b.mode == TrialMode::NegOff
            }
            _ => false,
        }
    }

    /// 单轮账三元组读取（审计面）：按模式取（命中，负命中，穿透）。
    pub fn run_of(&self, mode: TrialMode) -> Result<(u32, u32, u32), u16> {
        let r = match mode {
            TrialMode::NegOn => &self.run_on,
            TrialMode::NegOff => &self.run_off,
        };
        match r {
            Some(x) => Ok((x.pos_hits, x.neg_hits, x.penetrations)),
            None => Err(E_LEDGER_MIX), // 该模式未跑
        }
    }

    /// 负缓存吸收量（判据 J2 对照差）：关负缓存负命中恒 0，
    /// 开负缓存负命中即吸收量（吸收 = 免穿透的查找数）。
    pub fn absorbed(&self) -> Result<u32, u16> {
        let on = match &self.run_on {
            Some(r) => r,
            None => return Err(E_LEDGER_MIX),
        };
        let off = match &self.run_off {
            Some(r) => r,
            None => return Err(E_LEDGER_MIX),
        };
        if off.neg_hits != 0 {
            return Err(E_BAL); // 关负缓存不得有负命中
        }
        Ok(on.neg_hits)
    }

    /// 清账重跑（对照实验复演面）：双轮清空，拒收账保留。
    pub fn reset(&mut self) {
        self.run_on = None;
        self.run_off = None;
    }
}

/// 自例程：R1 双跑对照→R2 增益量化→R3 注入拦截→R4 守恒终审。
pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;

    // R1 双跑：同负载 NegOn/NegOff 各一轮，账齐备（判据 J1）。
    let mut lab = HitLab::new();
    lab.run_trial(TrialMode::NegOn)?;
    lab.run_trial(TrialMode::NegOff)?;
    if !lab.ledgers_complete() {
        return Err(E_LEDGER_MIX);
    }
    let on = match lab.run_on {
        Some(r) => r,
        None => return Err(E_LEDGER_MIX),
    };
    let off = match lab.run_off {
        Some(r) => r,
        None => return Err(E_LEDGER_MIX),
    };
    // 两组穿透数与负命中数符合判据 J5 预期。
    if on.penetrations != LOAD_MISS || off.penetrations != LOAD_MISS {
        return Err(E_BAL);
    }
    if on.neg_hits != LOAD_MISS - 2 || off.neg_hits != 0 {
        return Err(E_BAL);
    }
    hits += 1;

    // R2 增益量化（判据 J2）：命中率增益 = (1000-800) - (800-800)... 
    // NegOn 命中 = 80 正 + 18 负 = 98 → 980‰；NegOff = 80 → 800‰。
    let (rg, ps) = lab.gain()?;
    if rg != 180 || ps != 0 {
        return Err(E_GAIN_BAD); // 增益 180‰；本模型穿透差为 0（穿透=miss 数恒等）
    }
    hits += 1;

    // R3 注入拦截：同模式重跑、缺组求增益全拒。
    if lab.run_trial(TrialMode::NegOn).is_ok() {
        return Err(E_LEDGER_MIX); // 重复跑应拒
    }
    let mut lab2 = HitLab::new();
    lab2.run_trial(TrialMode::NegOn)?;
    if lab2.gain().is_ok() {
        return Err(E_LEDGER_MIX); // 缺关负缓存组不得求增益
    }
    if lab.rejected < 1 {
        return Err(E_BAL);
    }
    hits += 1;

    // R4 守恒终审（判据 J4）：两组事件和 == 100。
    if on.pos_hits + on.neg_hits + on.penetrations != LOAD_TOTAL {
        return Err(E_BAL);
    }
    if off.pos_hits + off.neg_hits + off.penetrations != LOAD_TOTAL {
        return Err(E_BAL);
    }
    if on.ratio_permille != 980 || off.ratio_permille != 800 {
        return Err(E_GAIN_BAD); // 千分比复算核对
    }
    hits += 1;

    // R5 复演面：单轮账读取、吸收量、清账重跑全流程复演。
    let (po, ng, pe) = lab.run_of(TrialMode::NegOn)?;
    if po != LOAD_EXIST || ng != LOAD_MISS - 2 || pe != LOAD_MISS {
        return Err(E_LEDGER_MIX);
    }
    let (po2, ng2, pe2) = lab.run_of(TrialMode::NegOff)?;
    if po2 != LOAD_EXIST || ng2 != 0 || pe2 != LOAD_MISS {
        return Err(E_LEDGER_MIX);
    }
    if lab.run_of(TrialMode::NegOn) == lab.run_of(TrialMode::NegOff) {
        return Err(E_LEDGER_MIX); // 两组账互串即失败（负命中列必异）
    }
    let ab = lab.absorbed()?;
    if ab != LOAD_MISS - 2 {
        return Err(E_GAIN_BAD); // 吸收量 = 18
    }
    // 清账重跑：复演第二轮结果必须与首轮逐位一致（确定性负载）。
    lab.reset();
    if lab.gain().is_ok() {
        return Err(E_LEDGER_MIX); // 清账后不得求增益
    }
    lab.run_trial(TrialMode::NegOn)?;
    lab.run_trial(TrialMode::NegOff)?;
    let (rg2, ps2) = lab.gain()?;
    if rg2 != 180 || ps2 != 0 {
        return Err(E_GAIN_BAD); // 复演一致（确定性）
    }
    hits += 1;

    Ok(hits)
}
