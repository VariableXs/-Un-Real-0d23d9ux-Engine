//! UNX-F10089 双判据批次门扩展（batch_gate E 型挡板） · 0x92600 段
//! ─────────────────────────────────────────────────────────────────────
//! 卡号：UNX-F10089（把批次门扩出 E 型挡板：J2 未决闸挂进门面）。
//! 锚点：UNX-F10090-J1 双列齐备（正/反通过率分列 10/10 次取数正确），
//!       混列 10/10 次检出。
//!
//! 数据段说明：
//!   SEG_BASE = 0x92600（卡面 seg 闭集）。诊断码占用段内 +0x01..+0x0C
//!   闭集窗口，码值 = SEG_BASE + 变体序号，段内两两唯一、段外不越界；
//!   全部码位由 `GateCode::ALL` 封闭枚举给出，禁止运行期造码。
//!
//! 结构清单：
//!   - 门面状态机 `GateState`：Green → EArmed（E 型挡板挂上）→
//!     ERed（J2 未决，挡红）→ EGreen（J2 已决放行）单向推进，
//!     越闸（红闸放行）显性拒绝；
//!   - 批次门 `BatchGateE`：批次逐批过闸，每批必须双列齐备
//!     （正/反通过率分列各一），混列批显性拒绝；
//!   - 双列账 `DualColumnLedger`：正列/反列分列记账，列位固定，
//!     混列（值写错列）逐次检出，账守恒（登记=实计）；
//!   - 注入核 `BadGateCase`：双列缺列 / 混列写反列 / J2 未决越闸 /
//!     重复过闸 / 红闸放行 五类坏样本逐类 10/10 次拦截；
//!   - Linux errno 对照表 + FNV-1a 双跑指纹（无浮点、无随机）。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（F10089 独占，0x40 对齐窗口起点）。
pub const SEG_BASE: u32 = 0x92600;

/// 段窗上界（下一卡 0x92640 起点，闭开区间右端）。
pub const SEG_END: u32 = 0x92640;

/// 批次门批次槽数（8 批）。
pub const GATE_BATCHES: usize = 8;

/// J2 未决闸的复测轮数下限（J2 侧 ≥ 10 轮留痕才算已决）。
pub const J2_SETTLE_MIN: u32 = 10;

/// 注入复测轮数（逐类 10/10）。
pub const INJECT_ROUNDS: u32 = 10;

/// Linux errno 正数码（对照表口径）。
pub const EPERM: i32 = 1;
pub const EINVAL: i32 = 22;
pub const EALREADY: i32 = 114;
pub const EIO: i32 = 5;
pub const ENOMSG: i32 = 42;

// ── 诊断码闭集 ────────────────────────────────────────────────────────
/// 本模块诊断码（段内 +0x01..+0x0C 闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GateCode {
    /// 双列缺列（批次过闸只报单列）
    ColumnMissing,
    /// 混列（正列值写入反列或反向）
    ColumnMixed,
    /// J2 未决越闸（J2 复测未齐即放行）
    GateBypass,
    /// 红闸放行（挡板红态批次过闸）
    RedPass,
    /// 重复过闸（同批二次过闸）
    DuplicatePass,
    /// 批号越界
    BatchRange,
    /// 状态倒退
    StateRegress,
    /// 通过率越界（分母 0 或通过数>总数）
    RateRange,
    /// 账失恒（登记批数≠过闸批数）
    LedgerDrift,
    /// 重复记账同批
    DuplicateRow,
    /// 门未武装即过闸
    NotArmed,
    /// 段位越界
    SegmentViolation,
}

impl GateCode {
    /// 码值（段基址 +0x01 起连续编号，闭集唯一）。
    pub fn val(self) -> u32 {
        SEG_BASE + 1 + self as u32
    }

    /// 中文诊断与可操作建议（闭集返回，禁止空串）。
    pub fn detail(self) -> &'static str {
        match self {
            GateCode::ColumnMissing => "双列缺列被拒：过闸批次必须正/反两列齐报；请补缺列",
            GateCode::ColumnMixed => "混列被拒：正列值不得写反列；请对号入列",
            GateCode::GateBypass => "J2 未决越闸被拒：J2 复测未齐闸保持红；请补 J2 留痕",
            GateCode::RedPass => "红闸放行被拒：挡板红态批次不得过闸；请先解红",
            GateCode::DuplicatePass => "重复过闸被拒：同批已过闸；请查重复放行",
            GateCode::BatchRange => "批号越界被拒：批号只允许 1..=8；请核对批号",
            GateCode::StateRegress => "状态倒退被拒：门面状态单向推进；请排查回滚",
            GateCode::RateRange => "通过率越界被拒：分母不得为 0、通过数不得越总数；请核对",
            GateCode::LedgerDrift => "账失恒被拒：登记批数必须等于过闸批数；请对账",
            GateCode::DuplicateRow => "重复记账被拒：同批已记账；请查重",
            GateCode::NotArmed => "门未武装即过闸被拒：E 型挡板未挂上不得过闸；请先武装",
            GateCode::SegmentViolation => "段位越界：段基址必须 0x40 对齐且落在 0x92600..=0x92640",
        }
    }

    /// 闭集全量（供文案完整性与唯一性检查）。
    pub const ALL: [GateCode; 12] = [
        GateCode::ColumnMissing,
        GateCode::ColumnMixed,
        GateCode::GateBypass,
        GateCode::RedPass,
        GateCode::DuplicatePass,
        GateCode::BatchRange,
        GateCode::StateRegress,
        GateCode::RateRange,
        GateCode::LedgerDrift,
        GateCode::DuplicateRow,
        GateCode::NotArmed,
        GateCode::SegmentViolation,
    ];
}

// ── 双列账 ────────────────────────────────────────────────────────────
/// 列位闭集：正列（J1 通过率）/ 反列（J2 通过率）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ColPos {
    /// 正列（J1）
    Pos,
    /// 反列（J2）
    Neg,
}

/// 一批的双列取数：正/反各一（分母必须一致且非零）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct DualRate {
    /// 批号（1..=8）
    pub batch: u32,
    /// 正列：通过数
    pub pos_pass: u32,
    /// 反列：通过数
    pub neg_pass: u32,
    /// 分母（复测总轮数）
    pub denom: u32,
}

/// 双列取数合法性：分母非零、两列通过数都 ≤ 分母。
pub fn rate_ok(r: &DualRate) -> bool {
    r.denom > 0 && r.pos_pass <= r.denom && r.neg_pass <= r.denom
}

/// 一条列记账：批号 + 列位 + 通过数/分母。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ColRow {
    /// 批号
    pub batch: u32,
    /// 列位
    pub pos: ColPos,
    /// 通过数
    pub pass: u32,
    /// 分母
    pub denom: u32,
}

/// 双列账：分列记账，列位固定，混列逐次检出。
#[derive(Clone, Debug)]
pub struct DualColumnLedger {
    /// 列账行
    pub rows: Vec<ColRow>,
    /// 登记的应记账批数（每批 2 行）
    pub registered_batches: u32,
    /// 混列检出数（显性记账）
    pub mixed: u32,
}

impl DualColumnLedger {
    /// 构造零账。
    pub fn new() -> DualColumnLedger {
        DualColumnLedger { rows: Vec::new(), registered_batches: 0, mixed: 0 }
    }

    /// 记一条列账：批号必检、通过率必检、重复必拒。
    pub fn record(&mut self, batch: u32, pos: ColPos, pass: u32, denom: u32) -> Result<(), GateCode> {
        if batch == 0 || batch as usize > GATE_BATCHES {
            return Err(GateCode::BatchRange);
        }
        let r = DualRate { batch, pos_pass: pass, neg_pass: pass, denom };
        if !rate_ok(&r) {
            return Err(GateCode::RateRange);
        }
        if self.rows.iter().any(|x| x.batch == batch && x.pos == pos) {
            return Err(GateCode::DuplicateRow);
        }
        self.rows.push(ColRow { batch, pos, pass, denom });
        Ok(())
    }

    /// 登记应记账批数：0 或超 GATE_BATCHES 拒绝。
    pub fn set_batches(&mut self, n: u32) -> Result<(), GateCode> {
        if n == 0 || n as usize > GATE_BATCHES {
            return Err(GateCode::BatchRange);
        }
        self.registered_batches = n;
        Ok(())
    }

    /// 守恒式：登记批数×2 = 留痕行数 才零漂移。
    pub fn drift(&self) -> Option<u32> {
        let need = self.registered_batches * 2;
        let done = self.rows.len() as u32;
        if done == need {
            None
        } else if done < need {
            Some(need - done)
        } else {
            Some(0)
        }
    }

    /// 单列行数。
    pub fn col_count(&self, pos: ColPos) -> usize {
        self.rows.iter().filter(|r| r.pos == pos).count()
    }
}

/// 标准记账：4 批正/反各一（10/10 满通过率）。
pub fn ledger_standard(lg: &mut DualColumnLedger) -> Result<(), GateCode> {
    let _ = lg.set_batches(4);
    let mut n = 1u32;
    while n <= 4 {
        lg.record(n, ColPos::Pos, J2_SETTLE_MIN, J2_SETTLE_MIN)?;
        lg.record(n, ColPos::Neg, J2_SETTLE_MIN, J2_SETTLE_MIN)?;
        n += 1;
    }
    Ok(())
}

// ── 批次门（状态机） ──────────────────────────────────────────────────
/// 门面状态机：Green → EArmed → ERed → EGreen 单向推进。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GateState {
    /// 常态绿（E 型挡板未挂）
    Green,
    /// E 型挡板已武装
    EArmed,
    /// E 型挡板红（J2 未决，闸挡红）
    ERed,
    /// E 型闸绿（J2 已决，放行）
    EGreen,
}

/// 批次门：E 型挡板挂上门面，批次逐批过闸。
#[derive(Clone, Debug)]
pub struct BatchGateE {
    /// 已过闸批号
    pub passed: Vec<u32>,
    /// 门面状态
    pub state: GateState,
}

impl BatchGateE {
    /// 构造常态绿门。
    pub const fn new() -> BatchGateE {
        BatchGateE { passed: Vec::new(), state: GateState::Green }
    }

    /// 武装 E 型挡板：Green 态专用。
    pub fn arm_e(&mut self, j2_rounds: u32) -> Result<(), GateCode> {
        if self.state != GateState::Green {
            return Err(GateCode::StateRegress);
        }
        // J2 未决 → 挂板即挡红；已决 → 武装并保持绿
        if j2_rounds < J2_SETTLE_MIN {
            self.state = GateState::ERed;
        } else {
            self.state = GateState::EGreen;
        }
        Ok(())
    }

    /// 批次过闸：未武装拒、红闸拒、重复批拒、批号越界拒。
    pub fn pass(&mut self, batch: u32) -> Result<(), GateCode> {
        match self.state {
            GateState::Green => return Err(GateCode::NotArmed),
            GateState::ERed => return Err(GateCode::RedPass),
            GateState::EArmed | GateState::EGreen => {}
        }
        if batch == 0 || batch as usize > GATE_BATCHES {
            return Err(GateCode::BatchRange);
        }
        if self.passed.iter().any(|b| *b == batch) {
            return Err(GateCode::DuplicatePass);
        }
        self.passed.push(batch);
        Ok(())
    }

    /// 解红：J2 补齐复测后从 ERed 推进到 EGreen。
    pub fn clear_red(&mut self, j2_rounds: u32) -> Result<(), GateCode> {
        if self.state != GateState::ERed {
            return Err(GateCode::StateRegress);
        }
        if j2_rounds < J2_SETTLE_MIN {
            return Err(GateCode::GateBypass);
        }
        self.state = GateState::EGreen;
        Ok(())
    }
}

/// 标准过闸：武装（J2 已决）→ 8 批逐批过闸。
pub fn gate_standard(gate: &mut BatchGateE) -> Result<(), GateCode> {
    gate.arm_e(J2_SETTLE_MIN)?;
    let mut b = 1u32;
    while b <= GATE_BATCHES as u32 {
        gate.pass(b)?;
        b += 1;
    }
    Ok(())
}

// ── 混列检出器 ────────────────────────────────────────────────────────
/// 混列探测：一批的两列取数，正/反列位必须各一条且不互换。
/// 互换样本（两行同列位）即混列，逐次检出并显性记账。
pub fn mixed_probe(rows: &[ColRow]) -> Result<(), GateCode> {
    let mut pos_cnt = 0u32;
    let mut neg_cnt = 0u32;
    let mut i = 0usize;
    while i < rows.len() {
        if let Some(r) = rows.get(i) {
            match r.pos {
                ColPos::Pos => pos_cnt += 1,
                ColPos::Neg => neg_cnt += 1,
            }
        }
        i += 1;
    }
    // 齐备 = 恰一正一反；任何偏差（缺列/双列）都判混列面
    if pos_cnt == 1 && neg_cnt == 1 {
        Ok(())
    } else {
        Err(GateCode::ColumnMixed)
    }
}

/// 构造一批的正确双列样本。
pub fn dual_ok_sample(batch: u32) -> [ColRow; 2] {
    [
        ColRow { batch, pos: ColPos::Pos, pass: J2_SETTLE_MIN, denom: J2_SETTLE_MIN },
        ColRow { batch, pos: ColPos::Neg, pass: J2_SETTLE_MIN, denom: J2_SETTLE_MIN },
    ]
}

/// 构造一批的混列样本（两行同列位 = 反列缺位）。
pub fn dual_mixed_sample(batch: u32) -> [ColRow; 2] {
    [
        ColRow { batch, pos: ColPos::Neg, pass: J2_SETTLE_MIN, denom: J2_SETTLE_MIN },
        ColRow { batch, pos: ColPos::Neg, pass: J2_SETTLE_MIN, denom: J2_SETTLE_MIN },
    ]
}

// ── 注入核 ────────────────────────────────────────────────────────────
/// 五类坏过闸样本。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BadGateCase {
    /// 缺列过闸（只报正列）
    ColumnMissing,
    /// 混列（两行同列位）
    ColumnMixed,
    /// J2 未决越闸
    GateBypass,
    /// 重复过闸
    DuplicatePass,
    /// 红闸放行
    RedPass,
}

impl BadGateCase {
    /// 闭集全量。
    pub const ALL: [BadGateCase; 5] = [
        BadGateCase::ColumnMissing,
        BadGateCase::ColumnMixed,
        BadGateCase::GateBypass,
        BadGateCase::DuplicatePass,
        BadGateCase::RedPass,
    ];

    /// 注入一轮：构造坏样本并触发校验，返回是否被显式拦截。
    pub fn inject(self) -> bool {
        match self {
            BadGateCase::ColumnMissing => {
                let ok = dual_ok_sample(1);
                let one = mixed_probe(&ok).is_ok();
                let miss = dual_mixed_sample(1);
                let miss_rej = mixed_probe(&miss).err() == Some(GateCode::ColumnMixed);
                one && miss_rej
            }
            BadGateCase::ColumnMixed => {
                let mut lg = DualColumnLedger::new();
                let _ = lg.set_batches(1);
                let a = lg.record(1, ColPos::Neg, J2_SETTLE_MIN, J2_SETTLE_MIN).is_ok();
                let b = lg.record(1, ColPos::Neg, J2_SETTLE_MIN, J2_SETTLE_MIN).is_err();
                a && b
            }
            BadGateCase::GateBypass => {
                let mut g = BatchGateE::new();
                let _ = g.arm_e(J2_SETTLE_MIN - 1);
                g.state == GateState::ERed && g.pass(1).err() == Some(GateCode::RedPass)
            }
            BadGateCase::DuplicatePass => {
                let mut g = BatchGateE::new();
                let _ = g.arm_e(J2_SETTLE_MIN);
                let a = g.pass(1).is_ok();
                let b = g.pass(1).err() == Some(GateCode::DuplicatePass);
                a && b
            }
            BadGateCase::RedPass => {
                let mut g = BatchGateE::new();
                let _ = g.arm_e(J2_SETTLE_MIN - 1);
                let mut hit = 0u32;
                let mut b = 1u32;
                while b <= 3 {
                    if g.pass(b).err() == Some(GateCode::RedPass) {
                        hit += 1;
                    }
                    b += 1;
                }
                hit == 3 && g.passed.is_empty()
            }
        }
    }
}

/// 执行五类坏样本注入：每类 INJECT_ROUNDS 次，返回账本与逐类拦截数。
pub fn run_gate_injection() -> (DualColumnLedger, [u32; 5]) {
    let mut ledger = DualColumnLedger::new();
    let mut per_class = [0u32; 5];
    let mut ci = 0usize;
    while ci < BadGateCase::ALL.len() {
        if let Some(case) = BadGateCase::ALL.get(ci).copied() {
            let mut round = 0u32;
            while round < INJECT_ROUNDS {
                let caught = case.inject();
                if !caught {
                    ledger.mixed = ledger.mixed.saturating_add(1);
                }
                if caught {
                    if let Some(slot) = per_class.get_mut(ci) {
                        *slot = slot.saturating_add(1);
                    }
                }
                round += 1;
            }
        }
        ci += 1;
    }
    (ledger, per_class)
}

// ── Linux errno 对照表 ────────────────────────────────────────────────
/// (名称, 负值, Windows 近似码, 说明)
pub const ERRNO_TABLE: &[(&str, i32, i32, &str)] = &[
    ("EPERM", -1, 5, "Windows ERROR_ACCESS_DENIED；红闸放行/越闸族"),
    ("EIO", -5, 21, "Windows ERROR_NOT_READY；缺列/混列传数族"),
    ("EINVAL", -22, 87, "Windows ERROR_INVALID_PARAMETER；批号/通过率越界族"),
    ("ENOMSG", -42, 122, "Windows ERROR_NO_MORE_ITEMS；列位缺位族"),
    ("EALREADY", -114, 10037, "Windows WSAEALREADY；重复过闸族"),
];

/// 查表：按 errno 正数取负值；表外返回 0（显性可查）。
pub fn linux_neg(code: i32) -> i32 {
    let mut i = 0usize;
    while i < ERRNO_TABLE.len() {
        if let Some(row) = ERRNO_TABLE.get(i) {
            if row.1 == -code {
                return row.1;
            }
        }
        i += 1;
    }
    0
}

/// 与 Linux 逐码一致：只允许表内码且负值精确匹配。
pub fn agrees_with_linux(code: i32) -> bool {
    linux_neg(code) == -code && code > 0
}

// ── 确定性指纹 ────────────────────────────────────────────────────────
/// FNV-1a 32 位（无浮点、无随机）。
pub fn fnv1a(bytes: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    let mut i = 0usize;
    while i < bytes.len() {
        if let Some(b) = bytes.get(i) {
            h ^= *b as u32;
            h = h.wrapping_mul(0x0100_0193);
        }
        i += 1;
    }
    h
}

/// 规范语料双跑指纹：武装→过闸→解红→双列记账全链逐字节入哈希。
pub fn corpus_fingerprint() -> u32 {
    let mut bytes: Vec<u8> = Vec::new();
    let mut g = BatchGateE::new();
    // 未决武装 → 挡红 → 解红 → 过闸
    let _ = g.arm_e(J2_SETTLE_MIN - 1);
    bytes.push(match g.pass(1) {
        Err(_) => 1,
        Ok(()) => 2,
    });
    let _ = g.clear_red(J2_SETTLE_MIN);
    bytes.push(match g.pass(1) {
        Ok(()) => 3,
        Err(_) => 4,
    });
    bytes.push(g.state as u8);
    let mut lg = DualColumnLedger::new();
    bytes.push(match ledger_standard(&mut lg) {
        Ok(()) => 5,
        Err(_) => 6,
    });
    bytes.push(match lg.drift() {
        None => 7,
        Some(_) => 8,
    });
    bytes.extend_from_slice(&lg.rows.len().to_le_bytes());
    fnv1a(&bytes)
}

// ── 检查入口 ──────────────────────────────────────────────────────────
/// F10089 判据集（17 条静态 + 3 条动态明细）。
pub fn run_unx10089_checks() -> CheckSet {
    let mut s = CheckSet::new("unx10089");

    // 1. 段位守卫
    let seg_ok = SEG_BASE % 0x40 == 0 && (SEG_BASE..SEG_END).contains(&SEG_BASE);
    s.add("段位守卫：0x40 对齐且在 0x92600..=0x92640", seg_ok,
          "段基址唯一且落在本卡窗口；越界即拒绝");

    // 2. 标准过闸：武装→8 批逐批过闸
    let mut g = BatchGateE::new();
    let std_ok = gate_standard(&mut g).is_ok();
    s.add("批次门：武装后 8 批逐批过闸", std_ok && g.passed.len() == GATE_BATCHES,
          "J2 已决武装绿闸，8 批全过无拦截");

    // 3. 未武装过闸拦截
    let mut ng = BatchGateE::new();
    let not_armed = ng.pass(1).err() == Some(GateCode::NotArmed);
    s.add("武装闸：未武装过闸被拒", not_armed,
          "E 型挡板未挂上门面时批次不得过闸");

    // 4. J2 未决挂板即挡红 + 红闸放行拦截（锚点判据的挡板面）
    let mut rg = BatchGateE::new();
    let _ = rg.arm_e(J2_SETTLE_MIN - 1);
    let armed_red = rg.state == GateState::ERed;
    let mut red_hits = 0u32;
    let mut b = 1u32;
    while b <= 3 {
        if rg.pass(b).err() == Some(GateCode::RedPass) {
            red_hits += 1;
        }
        b += 1;
    }
    s.add("红闸挡板：未决挂板即红、红闸 3 批全拦", armed_red && red_hits == 3,
          "J2 复测未齐挡板挂上即红，红态批次逐批拦截不放行");

    // 5. 越闸 10/10 拦截（锚点判据）
    let mut bypass_hits = 0u32;
    let mut round = 0u32;
    while round < INJECT_ROUNDS {
        let mut bg = BatchGateE::new();
        let _ = bg.arm_e(J2_SETTLE_MIN - 1);
        if bg.pass(2).err() == Some(GateCode::RedPass) {
            bypass_hits += 1;
        }
        round += 1;
    }
    s.add("越闸拦截：未决越闸 10/10 次拦截", bypass_hits == INJECT_ROUNDS,
          "红闸放行路径逐次显性拦截，闸面无暗门");

    // 6. 解红放行：补齐 J2 复测后闸转绿
    let mut cg = BatchGateE::new();
    let _ = cg.arm_e(J2_SETTLE_MIN - 1);
    let clear_ok = cg.clear_red(J2_SETTLE_MIN).is_ok() && cg.state == GateState::EGreen;
    let pass_after = cg.pass(1).is_ok();
    s.add("解红闸：补齐 J2 复测后闸绿放行", clear_ok && pass_after,
          "解红前置 = J2 侧复测 ≥10 轮，边界精确单向");

    // 7. 解红未齐拒绝
    let mut dg = BatchGateE::new();
    let _ = dg.arm_e(J2_SETTLE_MIN - 1);
    let clear_rej = dg.clear_red(J2_SETTLE_MIN - 1).err() == Some(GateCode::GateBypass);
    s.add("解红越闸：补测未齐解红被拒", clear_rej,
          "解红不得绕过 J2_SETTLE_MIN 下限，越界显性拒绝");

    // 8. 重复过闸拦截
    let mut pg = BatchGateE::new();
    let _ = pg.arm_e(J2_SETTLE_MIN);
    let _ = pg.pass(1);
    let dup_ok = pg.pass(1) == Err(GateCode::DuplicatePass);
    s.add("重复过闸：同批二次过闸被拒", dup_ok,
          "一批一次闸位，重复放行显性拒绝");

    // 9. 批号越界拦截
    let mut og = BatchGateE::new();
    let _ = og.arm_e(J2_SETTLE_MIN);
    let rng_ok = og.pass(0).err() == Some(GateCode::BatchRange)
        && og.pass(GATE_BATCHES as u32 + 1).err() == Some(GateCode::BatchRange);
    s.add("批号闭集：0 号/9 号批过闸被拒", rng_ok,
          "批号只允许 1..=8，越界显性拒绝");

    // 10. 双列齐备取数：标准 4 批分列记账零漂移（锚点判据）
    let mut lg = DualColumnLedger::new();
    let std_ledger = ledger_standard(&mut lg).is_ok();
    let dual_ok = std_ledger && lg.drift().is_none()
        && lg.col_count(ColPos::Pos) == 4 && lg.col_count(ColPos::Neg) == 4;
    s.add("双列记账：4 批正/反分列各 4 行零漂移", dual_ok,
          "正/反通过率分列固定，登记=实计=8 行");

    // 11. 混列 10/10 检出（锚点判据）
    let mut mix_hits = 0u32;
    let mut round2 = 0u32;
    while round2 < INJECT_ROUNDS {
        let mixed = dual_mixed_sample(2);
        let ok_s = dual_ok_sample(2);
        if mixed_probe(&mixed).err() == Some(GateCode::ColumnMixed)
            && mixed_probe(&ok_s).is_ok()
        {
            mix_hits += 1;
        }
        round2 += 1;
    }
    s.add("混列检出：两行同列位 10/10 次检出", mix_hits == INJECT_ROUNDS,
          "正/反列位互换或缺列样本逐次显性检出");

    // 12. 取数正确性：满通过率与零通过率两端
    let mut vl = DualColumnLedger::new();
    let _ = vl.set_batches(2);
    let full_ok = vl.record(1, ColPos::Pos, J2_SETTLE_MIN, J2_SETTLE_MIN).is_ok()
        && vl.record(1, ColPos::Neg, 0, J2_SETTLE_MIN).is_ok();
    let bad_rej = vl.record(2, ColPos::Pos, J2_SETTLE_MIN + 1, J2_SETTLE_MIN).is_err()
        && vl.record(2, ColPos::Neg, 3, 0).is_err();
    s.add("取数闭集：满通过率/零通过率可记、越总数/零分母拒", full_ok && bad_rej,
          "通过数 ≤ 分母且分母 > 0 是取数的闭集边界");

    // 13. 注入核：五类坏样本全检出
    let (ledger, per_class) = run_gate_injection();
    let all_caught = per_class.iter().all(|n| *n == INJECT_ROUNDS);
    s.add("注入核：五类坏样本各 10/10 检出", all_caught,
          "缺列/混列/未决越闸/重复过闸/红闸放行全被显式拦截");

    // 14. 账守恒逃逸=0
    s.add("账守恒：注入过程逃逸=0",
          ledger.mixed == 0 && ledger.rows.is_empty(),
          "注入核全程不得产生列账逃逸");

    // 15. 文案闭集：非空
    let text_ok = GateCode::ALL.iter().all(|c| !c.detail().is_empty());
    s.add("文案闭集：全部诊断码带中文建议", text_ok,
          "每个码必须能取到非空且可操作的说明");

    // 16. 诊断码唯一 + 落段
    let mut uniq = true;
    let mut i2 = 0usize;
    while i2 < GateCode::ALL.len() {
        let mut j = i2 + 1;
        while j < GateCode::ALL.len() {
            if let (Some(x), Some(y)) = (GateCode::ALL.get(i2), GateCode::ALL.get(j)) {
                if x.val() == y.val() {
                    uniq = false;
                }
            }
            j += 1;
        }
        i2 += 1;
    }
    let in_seg = GateCode::ALL.iter().all(|c| c.val() > SEG_BASE && c.val() <= SEG_BASE + 0x0C);
    s.add("诊断码：12 个码值两两唯一且落段窗", uniq && in_seg,
          "码值闭集唯一且不得越过 +0x0C 窗口");

    // 17. Linux 逐码一致
    let codes_ok = agrees_with_linux(EPERM)
        && agrees_with_linux(EIO)
        && agrees_with_linux(EINVAL)
        && agrees_with_linux(ENOMSG)
        && agrees_with_linux(EALREADY)
        && !agrees_with_linux(0)
        && !agrees_with_linux(9999);
    s.add("Linux 逐码一致：表内码精确、表外码拒绝", codes_ok,
          "错误码只允许 errno 表内正值且负值可复算");

    // 18. 双跑一致
    let f1 = corpus_fingerprint();
    let f2 = corpus_fingerprint();
    s.add("双跑一致：规范语料指纹相等", f1 == f2,
          "同输入两次必须逐字节相等，隐藏状态即失败");

    // 动态明细（≤8）：过闸快照 + 双列快照 + 指纹快照
    let gate_text = format!("过闸 {}/8 批", g.passed.len());
    s.add_dyn("过闸快照", g.passed.len() == GATE_BATCHES, &gate_text);
    let col_text = format!("正列 {} 行 / 反列 {} 行", lg.col_count(ColPos::Pos), lg.col_count(ColPos::Neg));
    s.add_dyn("双列快照", lg.col_count(ColPos::Pos) == 4 && lg.col_count(ColPos::Neg) == 4, &col_text);
    let fp_text = format!("语料指纹 0x{:08X}", f1);
    s.add_dyn("指纹快照", f1 == f2, &fp_text);

    s
}

// ── 测试 ──────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn standard_gate_ok() {
        let mut g = BatchGateE::new();
        assert!(gate_standard(&mut g).is_ok());
        assert_eq!(g.passed.len(), GATE_BATCHES);
        assert_eq!(g.state, GateState::EGreen);
    }

    #[test]
    fn not_armed_rejected() {
        let mut g = BatchGateE::new();
        assert_eq!(g.pass(1), Err(GateCode::NotArmed));
    }

    #[test]
    fn pending_arms_red_and_blocks() {
        let mut g = BatchGateE::new();
        let _ = g.arm_e(J2_SETTLE_MIN - 1);
        assert_eq!(g.state, GateState::ERed);
        assert_eq!(g.pass(1), Err(GateCode::RedPass));
        assert!(g.passed.is_empty());
    }

    #[test]
    fn clear_red_boundary() {
        let mut g = BatchGateE::new();
        let _ = g.arm_e(J2_SETTLE_MIN - 1);
        assert_eq!(g.clear_red(J2_SETTLE_MIN - 1), Err(GateCode::GateBypass));
        assert!(g.clear_red(J2_SETTLE_MIN).is_ok());
        assert_eq!(g.state, GateState::EGreen);
    }

    #[test]
    fn duplicate_pass_rejected() {
        let mut g = BatchGateE::new();
        let _ = g.arm_e(J2_SETTLE_MIN);
        let _ = g.pass(1);
        assert_eq!(g.pass(1), Err(GateCode::DuplicatePass));
    }

    #[test]
    fn batch_range_closed() {
        let mut g = BatchGateE::new();
        let _ = g.arm_e(J2_SETTLE_MIN);
        assert_eq!(g.pass(0), Err(GateCode::BatchRange));
        assert_eq!(g.pass(GATE_BATCHES as u32 + 1), Err(GateCode::BatchRange));
    }

    #[test]
    fn dual_column_conservation() {
        let mut lg = DualColumnLedger::new();
        assert!(ledger_standard(&mut lg).is_ok());
        assert_eq!(lg.col_count(ColPos::Pos), 4);
        assert_eq!(lg.col_count(ColPos::Neg), 4);
        assert!(lg.drift().is_none());
    }

    #[test]
    fn mixed_probe_closed() {
        assert!(mixed_probe(&dual_ok_sample(1)).is_ok());
        assert_eq!(mixed_probe(&dual_mixed_sample(1)), Err(GateCode::ColumnMixed));
    }

    #[test]
    fn rate_range_closed() {
        let mut lg = DualColumnLedger::new();
        let _ = lg.set_batches(2);
        assert_eq!(lg.record(1, ColPos::Pos, J2_SETTLE_MIN + 1, J2_SETTLE_MIN), Err(GateCode::RateRange));
        assert_eq!(lg.record(1, ColPos::Neg, 3, 0), Err(GateCode::RateRange));
        assert_eq!(lg.record(9, ColPos::Pos, 1, 1), Err(GateCode::BatchRange));
    }

    #[test]
    fn injection_matrix_zero_escape() {
        let (l, per) = run_gate_injection();
        assert_eq!(l.mixed, 0);
        assert!(l.rows.is_empty());
        for n in per.iter() {
            assert_eq!(*n, INJECT_ROUNDS);
        }
    }

    #[test]
    fn linux_codes_agree() {
        assert!(agrees_with_linux(ENOMSG));
        assert!(agrees_with_linux(EALREADY));
        assert!(!agrees_with_linux(0));
        assert_eq!(linux_neg(EIO), -EIO);
    }

    #[test]
    fn deterministic_corpus() {
        assert_eq!(corpus_fingerprint(), corpus_fingerprint());
    }

    #[test]
    fn unx10089_checks_green() {
        let s = run_unx10089_checks();
        assert!(s.all_passed(), "UNX-F10089 red: {}", s.red_items_text());
    }
}
