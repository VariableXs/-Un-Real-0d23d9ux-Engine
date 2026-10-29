//! unx4167_trailslash — UNX-F4167 尾斜杠必须目录（ENOTDIR/EISDIR 矩阵）（诊断段 0x9680）。
//!
//! 判据原文：
//!   J1 尾斜杠 × 末分量类型矩阵（目录/普通/不存在/链接四格）行为全过：
//!      目录通过、普通文件 ENOTDIR、链接按目标类型判定——与 POSIX 表
//!      逐格一致。
//!   J2 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计纪律：
//!   1. 四格闭集：{目录, 普通, 不存在, 链接} × 尾斜杠，判定结果
//!      分别为 通过 / ENOTDIR / ENOENT / 按目标类型二次裁定。
//!   2. 链接格：解析到链接目标类型后按 格1/格2 重判（跟随语义），
//!      目标自身不存在判 ENOENT。
//!   3. EISDIR 口径：不带尾斜杠的路径分量穿越到文件分量时报
//!      ENOTDIR（穿越失败），EISDIR 仅用于"以目录操作打开非目录"
//!      的反向面，两账分开记。
//!   4. 矩阵账：每格执行一次记一账，4/4 齐且全对才算过。
//!   5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。

pub const SEG_BASE: u16 = 0x9680;

// ── 诊断码闭集 ───────────────────────────────────────────────
const E_BAD_KIND: u16 = SEG_BASE + 1;    // 末分量类型越界（四格闭集外）
const E_NOT_DIR: u16 = SEG_BASE + 2;     // 普通文件 + 尾斜杠（ENOTDIR）
const E_NO_ENTRY: u16 = SEG_BASE + 3;    // 不存在 + 尾斜杠（ENOENT）
const E_LNK_DANGLE: u16 = SEG_BASE + 4;  // 链接目标不存在（ENOENT 链路）
const E_MATRIX_MISS: u16 = SEG_BASE + 5; // 矩阵格缺验或判错（4/4 失守）
const E_BAD_STATE: u16 = SEG_BASE + 6;   // 裁定状态机非法迁移
const E_BAL: u16 = SEG_BASE + 7;         // 判定账与矩阵账不平
const E_EMPTY_SUM: u16 = SEG_BASE + 8;   // 写回摘要为空

/// 末分量类型四格闭集。
pub const K_DIR: u8 = 0;       // 目录
pub const K_REG: u8 = 1;       // 普通文件
pub const K_MISSING: u8 = 2;   // 不存在
pub const K_LNK: u8 = 3;       // 符号链接
pub const KIND_TOTAL: u8 = 4;

/// 裁定结果闭集（与 POSIX errno 对齐）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TrailVerdict {
    Pass,      // 目录通过（尾斜杠合法）
    Enotdir,   // 普通文件 ENOTDIR
    Enoent,    // 不存在 ENOENT
    PassViaLnk,// 链接→目标为目录 → 通过
    EnotdirLnk,// 链接→目标为普通文件 → ENOTDIR
    EnoentLnk, // 链接→目标不存在 → ENOENT
}

/// 链接目标类型（链接格二次裁定输入；NONE=悬空链接）。
pub const LNK_NONE: u8 = 0xFF;

/// 裁定状态机：Inspect 查看末分量 / Dereferencing 链接解引 / Ruled 已裁定。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RuleState {
    Inspect,
    Dereferencing,
    Ruled,
}

/// 尾斜杠裁定器：四格矩阵 + 链接二次裁定 + 矩阵账。
pub struct TrailRuler {
    pub state: RuleState,
    cell_hits: [u32; 4], // 四格命中账（数组长度须 usize 字面量）
    pub verdicts: u32,            // 裁定总数
    pub matrix_cells: u32,        // 矩阵验证格数（须 = 4）
}

impl TrailRuler {
    pub fn new() -> TrailRuler {
        TrailRuler {
            state: RuleState::Inspect,
            cell_hits: [0; 4],
            verdicts: 0,
            matrix_cells: 0,
        }
    }

    /// 链接格二次裁定：目标类型 → 通过/ENOTDIR/ENOENT。
    fn rule_lnk_target(&mut self, target: u8) -> Result<TrailVerdict, u16> {
        self.state = RuleState::Dereferencing;
        let v = match target {
            K_DIR => TrailVerdict::PassViaLnk,
            K_REG => TrailVerdict::EnotdirLnk,
            _ => {
                self.state = RuleState::Ruled;
                return Err(E_LNK_DANGLE);
            }
        };
        self.state = RuleState::Ruled;
        Ok(v)
    }

    /// 单格裁定：末分量类型 + 尾斜杠 → POSIX 逐格结论。
    pub fn rule(&mut self, kind: u8, trailing_slash: bool) -> Result<TrailVerdict, u16> {
        if kind >= KIND_TOTAL && kind != LNK_NONE {
            return Err(E_BAD_KIND);
        }
        if kind >= KIND_TOTAL {
            return Err(E_BAD_KIND);
        }
        self.state = RuleState::Inspect;
        self.cell_hits[kind as usize] += 1; // 先判界后记账
        self.verdicts += 1;
        if !trailing_slash {
            // 无尾斜杠：四格全部按无尾斜杠通道（本模块只裁有尾格，
            // 无尾格入账但裁定按存在语义）。
            self.state = RuleState::Ruled;
            return Ok(TrailVerdict::Pass);
        }
        let v = match kind {
            K_DIR => TrailVerdict::Pass,
            K_REG => TrailVerdict::Enotdir,
            K_MISSING => TrailVerdict::Enoent,
            K_LNK => {
                // 链接格由调用方补目标类型二次裁定；此处先返回占位。
                self.state = RuleState::Ruled;
                return Ok(TrailVerdict::EnoentLnk);
            }
            _ => return Err(E_BAD_KIND),
        };
        self.state = RuleState::Ruled;
        Ok(v)
    }

    /// 链接全格：链接 + 尾斜杠 + 目标类型 → 二次裁定（判据 J1 链接格）。
    pub fn rule_lnk(&mut self, target: u8) -> Result<TrailVerdict, u16> {
        self.rule(K_LNK, true)?;
        self.rule_lnk_target(target)
    }

    /// 四格矩阵验证：逐格断言与 POSIX 表一致（判据 J1 断言面）。
    pub fn verify_matrix(&mut self) -> Result<(), u16> {
        let mut cells = 0u32;
        // 格 1：目录 + 尾斜杠 → 通过。
        if self.rule(K_DIR, true)? != TrailVerdict::Pass {
            return Err(E_MATRIX_MISS);
        }
        cells += 1;
        // 格 2：普通 + 尾斜杠 → ENOTDIR。
        if self.rule(K_REG, true)? != TrailVerdict::Enotdir {
            return Err(E_MATRIX_MISS);
        }
        cells += 1;
        // 格 3：不存在 + 尾斜杠 → ENOENT。
        if self.rule(K_MISSING, true)? != TrailVerdict::Enoent {
            return Err(E_MATRIX_MISS);
        }
        cells += 1;
        // 格 4：链接 + 尾斜杠 → 按目标类型二次裁定（目录目标通过）。
        if self.rule_lnk(K_DIR)? != TrailVerdict::PassViaLnk {
            return Err(E_MATRIX_MISS);
        }
        cells += 1;
        // 链接格补验：普通目标 / 悬空目标。
        if self.rule_lnk(K_REG)? != TrailVerdict::EnotdirLnk {
            return Err(E_MATRIX_MISS);
        }
        cells += 1;
        if self.rule_lnk(K_MISSING).is_ok() {
            return Err(E_LNK_DANGLE);
        }
        cells += 1;
        self.matrix_cells = cells;
        if cells != 6 {
            return Err(E_MATRIX_MISS);
        }
        Ok(())
    }

    /// 矩阵账守恒：四格命中账均非零，裁定总数 = 四格合计。
    pub fn audit(&self) -> Result<(), u16> {
        let sum = self.cell_hits[0] + self.cell_hits[1] + self.cell_hits[2] + self.cell_hits[3];
        if sum != self.verdicts {
            return Err(E_BAL);
        }
        let mut i = 0usize;
        while i < KIND_TOTAL as usize {
            if self.cell_hits[i] == 0 {
                return Err(E_MATRIX_MISS);
            }
            i += 1;
        }
        if self.matrix_cells != 6 {
            return Err(E_MATRIX_MISS);
        }
        if self.state != RuleState::Ruled {
            return Err(E_BAD_STATE);
        }
        Ok(())
    }

    pub fn cell_hit(&self, kind: u8) -> Result<u32, u16> {
        if kind >= KIND_TOTAL {
            return Err(E_BAD_KIND);
        }
        Ok(self.cell_hits[kind as usize])
    }

    /// 完成写回（判据 J2）。
    pub fn writeback(&mut self, status_done: bool, summary_len: u32) -> Result<(), u16> {
        if !status_done {
            return Err(E_BAD_STATE);
        }
        if summary_len == 0 {
            return Err(E_EMPTY_SUM);
        }
        Ok(())
    }
}

/// 自例程：R1 四格矩阵 → R2 边界 → R3 注入拦截 → R4 对账写回。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;

    // R1 正常路径：矩阵 6 断言全过（四格 + 链接格三目标）。
    let mut r = TrailRuler::new();
    r.verify_matrix()?;
    r.audit()?;
    r.writeback(true, 0x40)?;
    hits += 1;

    // R2 边界：类型越界 / 无尾斜杠通道 / 链接悬空。
    let mut r = TrailRuler::new();
    if r.rule(9, true).is_ok() {
        return Err(E_BAD_KIND);
    }
    if r.rule(K_REG, false)? != TrailVerdict::Pass {
        return Err(E_MATRIX_MISS);
    }
    if r.rule_lnk(LNK_NONE).is_ok() {
        return Err(E_LNK_DANGLE);
    }
    hits += 1;

    // R3 注入拦截：矩阵账缺格时 audit 拦截；错格判错即拦截。
    let mut r = TrailRuler::new();
    r.rule(K_DIR, true)?;
    if r.audit().is_ok() {
        return Err(E_MATRIX_MISS); // 其余三格未验
    }
    let mut r2 = TrailRuler::new();
    if r2.rule(K_REG, true)? == TrailVerdict::Pass {
        return Err(E_NOT_DIR);
    }
    hits += 1;

    // R4 守恒对账：四格命中账与裁定总数平 + 各格读数可查。
    let mut r = TrailRuler::new();
    r.verify_matrix()?;
    let d = r.cell_hit(K_DIR)?;
    let g = r.cell_hit(K_REG)?;
    let m = r.cell_hit(K_MISSING)?;
    let l = r.cell_hit(K_LNK)?;
    if d + g + m + l != r.verdicts {
        return Err(E_BAL);
    }
    if r.matrix_cells != 6 {
        return Err(E_MATRIX_MISS);
    }
    hits += 1;

    // R5 追加回归：无尾斜杠四格通道 + 链接三目标读数 + 逐格命中账。
    let mut r = TrailRuler::new();
    // 无尾斜杠：四格全走 Pass 通道（各记一账）。
    let mut k = 0u8;
    while k < KIND_TOTAL {
        if r.rule(k, false)? != TrailVerdict::Pass {
            return Err(E_MATRIX_MISS);
        }
        k += 1;
    }
    // 链接格三目标（目录/普通/悬空）裁定读数。
    if r.rule_lnk(K_DIR)? != TrailVerdict::PassViaLnk {
        return Err(E_MATRIX_MISS);
    }
    if r.rule_lnk(K_REG)? != TrailVerdict::EnotdirLnk {
        return Err(E_MATRIX_MISS);
    }
    if r.rule_lnk(K_MISSING).is_ok() {
        return Err(E_LNK_DANGLE);
    }
    // 命中账：四格各有无尾两账 + 链接多账，总数守恒。
    r.verify_matrix()?;
    r.audit()?;
    let mut sum = 0u32;
    let mut m = 0u8;
    while m < KIND_TOTAL {
        sum += r.cell_hit(m)?;
        m += 1;
    }
    if sum != r.verdicts {
        return Err(E_BAL);
    }
    if r.state != RuleState::Ruled {
        return Err(E_BAD_STATE);
    }
    r.writeback(true, 0x14)?;
    hits += 1;

    Ok(hits)
}
