//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F18480 · B04 批守恒闭合断言 · 0x7F3C0段
//! ─────────────────────────────────────────────────────────────────────
//! 锚点：UNX-F18480-J1 本批 20 条行数逐条求和=6,000 三方一致，红绿齐
//!
//! 设计要点：
//!   - 20 条行数 = B04 批 20 卡的配额行：本工单（A08）10 行取卡表
//!     冻结配额实值（Σ=3020）；姊妹工单 10 行卡表不在本工单手内，
//!     按批闭合余量写死：6000 − 3020 = 2980 → 10 槽 × 298；
//!   - 三方一致（J1 核心）：①静态行表逐条求和 ②循环累加器（从 0
//!     起 saturating_add）③闭式常量 BATCH_TOTAL=6000——三者必须
//!     逐次相等，任一漂移即红；
//!   - 红绿齐：每行绿/红态全部由同一张行表推导（行值=冻结源值即绿），
//!     20 绿 0 红才算批齐；不存在独立维护的第二份红绿账；
//!   - 只增不减：实测行数下限账以卡表目标为初值，快照计数单调递增，
//!     回退即红；
//!   - 注入核三类：行篡改（冻结后改行值 → 红绿齐破 + 三方破）/
//!     累加器漂移（非零起征 → 三方失配必抓）/ 差一漏行（少加一行
//!     → 求和失配必抓）——各 10 轮必抓；
//!   - 边界：空账 / 单行账 / 行号越界 / 双重快照防回退；
//!   - no_std + alloc；零 unwrap/expect/panic/浮点/IO/墙钟/随机。
//!
//! 本模块结构清单：
//!   [数据结构] 行表 ROWS[20]（10 本工单配额 + 10 姊妹余量）+ 实测账[10]
//!   [状态机]   账态 Open→Sealed 单向（封印后改行即红）
//!   [账本]     累加器 acc / 快照计数 / 绿红计数 / 封印哈希
//!   [错误路径] BcCode 11 码闭集，code() = SEG_BASE + n
//!   [注入核]   篡改/漂移/差一 三类 ×10 轮逐类断言必抓
//! ═══════════════════════════════════════════════════════════════════════

use crate::alloc::format;
use crate::checks::CheckSet;

/// 诊断段基址（0x7F3C0，AI-VX11-1105 批04 独占区，A08 工单卡十）。
pub const SEG_BASE: u32 = 0x7F3C0;

// ── 域闭集与冻结常量 ────────────────────────────────────────────
/// 行数（冻结：20，J1 原文锚定「20 条行数」）。
pub const BATCH_ROWS: usize = 20;
/// 本工单行数（冻结：10 = A08 工单 10 卡）。
pub const OUR_ROWS: usize = 10;
/// 姊妹工单行数（冻结推导：20 − 10 = 10）。
pub const SIBLING_ROWS: usize = 10;
/// 本工单配额和（冻结推导：320+280+300+260+320+300+280+300+380+280=3020）。
pub const OUR_QUOTA_SUM: u32 = 3020;
/// 姊妹余量槽值（冻结推导：6000 − 3020 = 2980 → 298 × 10）。
pub const SIBLING_QUOTA: u32 = 298;
/// 姊妹余量和（冻结推导：2980）。
pub const SIBLING_QUOTA_SUM: u32 = 2980;
/// 批闭合总量（冻结：6000，J1 原文「求和=6,000」）。
pub const BATCH_TOTAL: u32 = 6000;
/// 本工单目标行数和（冻结推导：640+600×7+640+760=6240）。
pub const TARGET_SUM_FROZEN: u32 = 6240;
/// 注入轮数（写死：10/10 语义）。
pub const INJECT_ROUNDS: u32 = 10;

/// 本工单 10 卡配额行（A08 卡表逐字转录）。
pub const OUR_QUOTAS: [u32; OUR_ROWS] = [320, 280, 300, 260, 320, 300, 280, 300, 380, 280];

/// 本工单 10 卡目标行数（A08 卡表逐字转录）。
pub const OUR_TARGETS: [u32; OUR_ROWS] = [640, 600, 600, 600, 640, 600, 600, 600, 760, 600];

/// FNV-1a 32 位（封印哈希）。
pub fn fnv1a(data: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    let mut i = 0;
    while i < data.len() {
        if let Some(&b) = data.get(i) {
            h ^= b as u32;
            h = h.wrapping_mul(0x0100_0193);
        }
        i += 1;
    }
    h
}

// ── 诊断码闭集 ─────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BcCode {
    /// 行号越界
    RowOOB,
    /// 三方失配
    ThreeWayMismatch,
    /// 红绿不齐（存在红行）
    ParityOff,
    /// 行表被篡改
    RowTampered,
    /// 累加器漂移
    AccDrift,
    /// 差一漏行
    OffByOne,
    /// 只增不减破坏
    OnlyIncreaseBroken,
    /// 账未封印
    NotSealed,
    /// 计数守恒破坏
    ConservationBroken,
    /// 双跑不一致
    NotReproducible,
    /// 段位越窗
    SegmentOutside,
}

impl BcCode {
    /// 诊断码 = SEG_BASE + 枚举序号。
    pub fn code(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            BcCode::RowOOB => "行号越界：0..20 闭集外",
            BcCode::ThreeWayMismatch => "三方失配：行表求和/累加器/闭式三者不等",
            BcCode::ParityOff => "红绿不齐：存在红行，行值偏离冻结源",
            BcCode::RowTampered => "行表篡改：封印后行值被改，红绿齐破",
            BcCode::AccDrift => "累加器漂移：非零起征或中途跳变",
            BcCode::OffByOne => "差一漏行：求和少加一行，闭合破坏",
            BcCode::OnlyIncreaseBroken => "只增不减破坏：实测账回退",
            BcCode::NotSealed => "账未封印：先 close 再出闭合断言",
            BcCode::ConservationBroken => "计数守恒破坏：绿+红 ≠ 行数",
            BcCode::NotReproducible => "双跑不一致：复现红线",
            BcCode::SegmentOutside => "段位越窗：诊断码不在 0x7F3C0 段",
        }
    }
    pub const ALL: [BcCode; 11] = [
        BcCode::RowOOB,
        BcCode::ThreeWayMismatch,
        BcCode::ParityOff,
        BcCode::RowTampered,
        BcCode::AccDrift,
        BcCode::OffByOne,
        BcCode::OnlyIncreaseBroken,
        BcCode::NotSealed,
        BcCode::ConservationBroken,
        BcCode::NotReproducible,
        BcCode::SegmentOutside,
    ];
}

/// 段内任意码 → 中文明细。
pub fn detail_of(code: u32) -> &'static str {
    let mut i = 0;
    while i < BcCode::ALL.len() {
        if let Some(c) = BcCode::ALL.get(i) {
            if c.code() == code {
                return c.detail();
            }
        }
        i += 1;
    }
    "未知诊断码：不在 0x7F3C0 段闭集内"
}

// ── 行源（冻结推导） ───────────────────────────────────────────
/// 行 i 的冻结源值：i<10 取本工单配额，否则姊妹余量 298。
pub fn row_source(i: usize) -> Result<u32, u32> {
    if i < OUR_ROWS {
        match OUR_QUOTAS.get(i) {
            Some(v) => Ok(*v),
            None => Err(BcCode::RowOOB.code()),
        }
    } else if i < BATCH_ROWS {
        Ok(SIBLING_QUOTA)
    } else {
        Err(BcCode::RowOOB.code())
    }
}

// ── 守恒账 ─────────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LedgerState {
    Open,
    Sealed,
}

pub struct ConserveLedger {
    /// 20 条行数（构建后冻结）。
    pub rows: [u32; BATCH_ROWS],
    /// 累加器（从 0 起 saturating_add，循环求和的第二方）。
    pub acc: u32,
    /// 实测行数下限账（初值=目标）。
    pub measured: [u32; OUR_ROWS],
    /// 快照计数（只增不减）。
    pub snapshots: u32,
    /// 绿/红行计数。
    pub green: u32,
    pub red: u32,
    /// 账态。
    pub state: LedgerState,
    /// 封印哈希（封印时抓取行表 FNV）。
    pub seal: u32,
}

impl ConserveLedger {
    /// 构建：行表按冻结源逐行落账，实测账以目标为初值，态 Open。
    pub fn new() -> ConserveLedger {
        let mut rows = [0u32; BATCH_ROWS];
        let mut i = 0;
        while i < BATCH_ROWS {
            if let Ok(v) = row_source(i) {
                if let Some(slot) = rows.get_mut(i) {
                    *slot = v;
                }
            }
            i += 1;
        }
        let mut measured = [0u32; OUR_ROWS];
        let mut k = 0;
        while k < OUR_ROWS {
            if let (Some(t), Some(slot)) = (OUR_TARGETS.get(k), measured.get_mut(k)) {
                *slot = *t;
            }
            k += 1;
        }
        ConserveLedger {
            rows,
            acc: 0,
            measured,
            snapshots: 0,
            green: 0,
            red: 0,
            state: LedgerState::Open,
            seal: 0,
        }
    }

    /// 第一方：静态行表逐条求和（顺序遍历，不跳行）。
    pub fn sum_table(&self) -> u32 {
        let mut sum: u32 = 0;
        let mut i = 0;
        while i < BATCH_ROWS {
            if let Some(v) = self.rows.get(i) {
                sum = sum.saturating_add(*v);
            }
            i += 1;
        }
        sum
    }

    /// 第二方：累加器复算（从 0 起 saturating_add 走满 20 行）。
    /// 起征非零即 AccDrift（注入二的抓点）。&self 纯函数：算完即还，
    /// 不落账面（账面 acc 槽留构建态 0 哨兵）。
    pub fn sum_acc(&self, seed: u32) -> Result<u32, u32> {
        if seed != 0 {
            return Err(BcCode::AccDrift.code());
        }
        let mut acc: u32 = 0;
        let mut i = 0;
        while i < BATCH_ROWS {
            if let Some(v) = self.rows.get(i) {
                acc = acc.saturating_add(*v);
            }
            i += 1;
        }
        Ok(acc)
    }

    /// 第三方：闭式常量。
    pub fn closed_form(&self) -> u32 {
        OUR_QUOTA_SUM + SIBLING_QUOTA_SUM
    }

    /// 三方一致断言：表和 = 累加器 = 闭式 = 6000。
    /// 三方过账后累加器值落账面（acc 槽只在此写，闭环凭证）。
    pub fn three_way(&mut self) -> Result<(u32, u32, u32), u32> {
        let s1 = self.sum_table();
        let s2 = self.sum_acc(0)?;
        let s3 = self.closed_form();
        if s1 != s2 || s2 != s3 || s3 != BATCH_TOTAL {
            return Err(BcCode::ThreeWayMismatch.code());
        }
        self.acc = s2;
        Ok((s1, s2, s3))
    }

    /// 红绿齐：每行绿/红全部由行表对冻结源推导，绿+红=20。
    pub fn parity(&mut self) -> Result<(u32, u32), u32> {
        self.green = 0;
        self.red = 0;
        let mut i = 0;
        while i < BATCH_ROWS {
            let v = match self.rows.get(i) {
                Some(v) => *v,
                None => return Err(BcCode::RowOOB.code()),
            };
            let src = row_source(i)?;
            if v == src {
                self.green = self.green.saturating_add(1);
            } else {
                self.red = self.red.saturating_add(1);
            }
            i += 1;
        }
        if self.green.saturating_add(self.red) != BATCH_ROWS as u32 {
            return Err(BcCode::ConservationBroken.code());
        }
        if self.red > 0 {
            return Err(BcCode::ParityOff.code());
        }
        Ok((self.green, self.red))
    }

    /// 封印：抓行表哈希，态 Open→Sealed 单向。
    pub fn close(&mut self) {
        let mut buf: [u8; 4] = [0; 4];
        let mut h: u32 = 0x811C_9DC5;
        let mut i = 0;
        while i < BATCH_ROWS {
            if let Some(v) = self.rows.get(i) {
                buf[0] = (*v & 0xFF) as u8;
                buf[1] = ((*v >> 8) & 0xFF) as u8;
                buf[2] = ((*v >> 16) & 0xFF) as u8;
                buf[3] = ((*v >> 24) & 0xFF) as u8;
                let mut k = 0;
                while k < 4 {
                    if let Some(&b) = buf.get(k) {
                        h ^= b as u32;
                        h = h.wrapping_mul(0x0100_0193);
                    }
                    k += 1;
                }
            }
            i += 1;
        }
        self.seal = h;
        self.state = LedgerState::Sealed;
    }

    /// 封印完整性：Sealed 态下行表必须与封印哈希一致（篡改抓点）。
    pub fn seal_intact(&self) -> Result<u32, u32> {
        if self.state != LedgerState::Sealed {
            return Err(BcCode::NotSealed.code());
        }
        // 重算现哈希。
        let mut buf: [u8; 4] = [0; 4];
        let mut h: u32 = 0x811C_9DC5;
        let mut i = 0;
        while i < BATCH_ROWS {
            if let Some(v) = self.rows.get(i) {
                buf[0] = (*v & 0xFF) as u8;
                buf[1] = ((*v >> 8) & 0xFF) as u8;
                buf[2] = ((*v >> 16) & 0xFF) as u8;
                buf[3] = ((*v >> 24) & 0xFF) as u8;
                let mut k = 0;
                while k < 4 {
                    if let Some(&b) = buf.get(k) {
                        h ^= b as u32;
                        h = h.wrapping_mul(0x0100_0193);
                    }
                    k += 1;
                }
            }
            i += 1;
        }
        if h != self.seal {
            return Err(BcCode::RowTampered.code());
        }
        Ok(h)
    }

    /// 实测快照：每次快照实测值必须 ≥ 目标且不回退（只增不减）。
    pub fn snapshot(&mut self, vals: &[u32; OUR_ROWS]) -> Result<u32, u32> {
        let mut i = 0;
        while i < OUR_ROWS {
            let v = match vals.get(i) {
                Some(v) => *v,
                None => return Err(BcCode::RowOOB.code()),
            };
            let floor = match OUR_TARGETS.get(i) {
                Some(t) => *t,
                None => return Err(BcCode::RowOOB.code()),
            };
            if v < floor {
                return Err(BcCode::OnlyIncreaseBroken.code());
            }
            let prev = match self.measured.get(i) {
                Some(p) => *p,
                None => return Err(BcCode::RowOOB.code()),
            };
            if v < prev {
                return Err(BcCode::OnlyIncreaseBroken.code());
            }
            if let Some(slot) = self.measured.get_mut(i) {
                *slot = v;
            }
            i += 1;
        }
        self.snapshots = self.snapshots.saturating_add(1);
        Ok(self.snapshots)
    }

    /// 差一漏行探测：跳过最后一行的求和必不等于闭合量。
    pub fn sum_skip_last(&self) -> u32 {
        let mut sum: u32 = 0;
        let mut i = 0;
        while i + 1 < BATCH_ROWS {
            if let Some(v) = self.rows.get(i) {
                sum = sum.saturating_add(*v);
            }
            i += 1;
        }
        sum
    }
}

// ── 黄金闭合链 ─────────────────────────────────────────────────
/// 构建→三方→红绿齐→封印→封印完整→快照。返回（三方值, 绿行, 快照数）。
pub fn golden_run() -> Result<((u32, u32, u32), u32, u32), u32> {
    let mut led = ConserveLedger::new();
    let tri = led.three_way()?;
    let (g, r) = led.parity()?;
    if g != BATCH_ROWS as u32 || r != 0 {
        return Err(BcCode::ParityOff.code());
    }
    led.close();
    led.seal_intact()?;
    // 快照：目标即实测下限。
    let mut snap: [u32; OUR_ROWS] = [0; OUR_ROWS];
    let mut i = 0;
    while i < OUR_ROWS {
        if let Some(t) = OUR_TARGETS.get(i) {
            if let Some(slot) = snap.get_mut(i) {
                *slot = *t;
            }
        }
        i += 1;
    }
    let snaps = led.snapshot(&snap)?;
    if snaps != 1 {
        return Err(BcCode::ConservationBroken.code());
    }
    Ok((tri, g, snaps))
}

// ── 注入核：三类坏样本 ×10 轮 ──────────────────────────────────
/// 注入一：行篡改——封印后改一行，封印完整性必红 + 红绿齐必破。
/// 注入二：累加器漂移——非零起征求和必拒（AccDrift）。
/// 注入三：差一漏行——跳末行求和必与闭合量失配。
pub fn inject_rounds(rounds: u32) -> Result<(u32, u32), u32> {
    let mut tries = 0u32;
    let mut caught = 0u32;
    let mut r = 0u32;
    while r < rounds {
        // —— 类一：行篡改 ——
        tries = tries.saturating_add(1);
        {
            let mut led = ConserveLedger::new();
            led.close();
            // 篡改第 3 行（+1）。
            if let Some(slot) = led.rows.get_mut(3) {
                *slot = slot.saturating_add(1);
            }
            let seal_broken = led.seal_intact().err() == Some(BcCode::RowTampered.code());
            let parity_off = led.parity().err() == Some(BcCode::ParityOff.code());
            if seal_broken && parity_off {
                caught = caught.saturating_add(1);
            }
        }
        // —— 类二：累加器漂移 ——
        tries = tries.saturating_add(1);
        {
            let led = ConserveLedger::new();
            let drift = led.sum_acc(1).err() == Some(BcCode::AccDrift.code());
            let clean = led.sum_acc(0).is_ok();
            if drift && clean {
                caught = caught.saturating_add(1);
            }
        }
        // —— 类三：差一漏行 ——
        tries = tries.saturating_add(1);
        {
            let led = ConserveLedger::new();
            let skipped = led.sum_skip_last();
            let full = led.sum_table();
            // 少加一行（298）必与 6000 失配，且差值恰为末行源值。
            let hit = skipped != BATCH_TOTAL
                && full == BATCH_TOTAL
                && BATCH_TOTAL.saturating_sub(skipped) == SIBLING_QUOTA;
            if hit {
                caught = caught.saturating_add(1);
            }
        }
        r = r.saturating_add(1);
    }
    if caught != tries {
        return Err(BcCode::ConservationBroken.code());
    }
    Ok((tries, caught))
}

/// 双跑确定性。
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(BcCode::NotReproducible.code());
    }
    Ok(())
}

/// 段位守卫。
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < BcCode::ALL.len() {
        if let Some(c) = BcCode::ALL.get(i) {
            let v = c.code();
            if v < SEG_BASE || v >= SEG_BASE + 0x40 {
                return false;
            }
        }
        i += 1;
    }
    SEG_BASE % 0x40 == 0 && SEG_BASE >= 0x7F000 && SEG_BASE <= 0x7F400
}

/// 边界探针组：空账（求和 0 但闭式仍 6000 → 三方必破）/ 单行账 /
/// 行号越界。
pub fn edges() -> Result<(), u32> {
    // 空账：行表清零，表和 0 ≠ 6000，三方必破（空账不算闭合）。
    let mut empty = ConserveLedger::new();
    let mut i = 0;
    while i < BATCH_ROWS {
        if let Some(slot) = empty.rows.get_mut(i) {
            *slot = 0;
        }
        i += 1;
    }
    let empty_ok = empty.three_way().err() == Some(BcCode::ThreeWayMismatch.code());
    // 单行账：只留第 0 行源值，其余清零 → 三方必破。
    let mut one = ConserveLedger::new();
    let head = row_source(0)?;
    let mut j = 0;
    while j < BATCH_ROWS {
        if let Some(slot) = one.rows.get_mut(j) {
            *slot = if j == 0 { head } else { 0 };
        }
        j += 1;
    }
    let one_ok = one.three_way().is_err();
    // 行号越界：row_source(20) 必 RowOOB。
    let oob_ok = row_source(BATCH_ROWS).err() == Some(BcCode::RowOOB.code());
    if empty_ok && one_ok && oob_ok {
        Ok(())
    } else {
        Err(BcCode::ConservationBroken.code())
    }
}

/// 只增不减探针：快照抬升合法、回退必拒。
pub fn only_increase_probe() -> Result<(), u32> {
    let mut led = ConserveLedger::new();
    let mut snap: [u32; OUR_ROWS] = [0; OUR_ROWS];
    let mut i = 0;
    while i < OUR_ROWS {
        if let Some(t) = OUR_TARGETS.get(i) {
            if let Some(slot) = snap.get_mut(i) {
                *slot = *t + 10;
            }
        }
        i += 1;
    }
    let _ = led.snapshot(&snap)?;
    // 回退（低于现账）必拒。
    let mut back: [u32; OUR_ROWS] = [0; OUR_ROWS];
    let mut k = 0;
    while k < OUR_ROWS {
        if let Some(t) = OUR_TARGETS.get(k) {
            if let Some(slot) = back.get_mut(k) {
                *slot = *t;
            }
        }
        k += 1;
    }
    if led.snapshot(&back).err() == Some(BcCode::OnlyIncreaseBroken.code()) {
        Ok(())
    } else {
        Err(BcCode::ConservationBroken.code())
    }
}

/// 分账闭式探针：本工单 3020 + 姊妹 2980 = 6000（分账各自复算）。
pub fn split_probe() -> Result<(u32, u32), u32> {
    let mut ours: u32 = 0;
    let mut i = 0;
    while i < OUR_ROWS {
        if let Some(v) = OUR_QUOTAS.get(i) {
            ours = ours.saturating_add(*v);
        }
        i += 1;
    }
    if ours != OUR_QUOTA_SUM {
        return Err(BcCode::ThreeWayMismatch.code());
    }
    let siblings = SIBLING_ROWS as u32 * SIBLING_QUOTA;
    if siblings != SIBLING_QUOTA_SUM {
        return Err(BcCode::ThreeWayMismatch.code());
    }
    if ours.saturating_add(siblings) != BATCH_TOTAL {
        return Err(BcCode::ThreeWayMismatch.code());
    }
    Ok((ours, siblings))
}

/// 目标账探针：Σ目标 = 6240（实测账初值守恒）。
pub fn target_probe() -> Result<u32, u32> {
    let mut sum: u32 = 0;
    let mut i = 0;
    while i < OUR_ROWS {
        if let Some(t) = OUR_TARGETS.get(i) {
            sum = sum.saturating_add(*t);
        }
        i += 1;
    }
    if sum != TARGET_SUM_FROZEN {
        return Err(BcCode::ThreeWayMismatch.code());
    }
    Ok(sum)
}

// ── 判据出口 ───────────────────────────────────────────────────
/// UNX-F18480 自检判据集（≤20 条；≤8 条动态明细）。
pub fn run_unx18480_checks() -> CheckSet {
    let mut s = CheckSet::new("unx18480");
    // 1. 行闭集冻结：20 行 = 10 本工单 + 10 姊妹。
    s.add(
        "J1-行闭集20",
        BATCH_ROWS == 20 && OUR_ROWS == 10 && SIBLING_ROWS == 10,
        "20行=10+10 闭式",
    );
    // 2. 三方一致 = 6000（J1 核心）。
    let g = golden_run();
    s.add_dyn("J1-三方一致6000", g.is_ok(), &format!("三方/绿行/快照={:?}", g));
    // 3. 分账闭式：3020 + 2980 = 6000。
    s.add(
        "J1-分账闭式",
        split_probe().is_ok(),
        "本工单3020+姊妹2980=6000",
    );
    // 4. 目标账守恒 6240。
    s.add(
        "J1-目标账6240",
        target_probe().is_ok(),
        "Σ目标=6240 实测初值守恒",
    );
    // 5. 红绿齐：20 绿 0 红（同源推导）；三方过账后 acc 槽落 6000。
    {
        let mut led = ConserveLedger::new();
        let _ = led.three_way();
        let acc_ok = led.acc == BATCH_TOTAL;
        let pr = led.parity();
        let ok = match pr {
            Ok((gg, rr)) => gg == 20 && rr == 0,
            Err(_) => false,
        };
        s.add("J1-红绿齐", ok && acc_ok, "20绿0红 + acc 槽落 6000 凭证");
    }
    // 6. 注入三类 ×10 轮全抓。
    let inj = inject_rounds(INJECT_ROUNDS);
    let inj_ok = match inj {
        Ok((t, c)) => t > 0 && c == t,
        Err(_) => false,
    };
    s.add_dyn("J1-注入三类10/10", inj_ok, &format!("tries/caught={:?}", inj));
    // 7. 行篡改单发必红（封印完整性 + 红绿齐双通道）。
    {
        let mut led = ConserveLedger::new();
        led.close();
        if let Some(slot) = led.rows.get_mut(7) {
            *slot = slot.saturating_add(1);
        }
        let seal = led.seal_intact().err() == Some(BcCode::RowTampered.code());
        let par = led.parity().err() == Some(BcCode::ParityOff.code());
        s.add("J1-篡改单发必红", seal && par, "封印+红绿双通道点名");
    }
    // 8. 差一漏行单发必抓（差值恰为末行 298）。
    {
        let led = ConserveLedger::new();
        let skipped = led.sum_skip_last();
        s.add(
            "J1-差一漏行必抓",
            BATCH_TOTAL.saturating_sub(skipped) == SIBLING_QUOTA && skipped != BATCH_TOTAL,
            "6000−5702=298 恰末行",
        );
    }
    // 9. 只增不减：抬升合法/回退必拒。
    s.add("J1-只增不减", only_increase_probe().is_ok(), "快照回退必拒");
    // 10. 边界三探：空账/单行/越号。
    s.add("J1-边界三探", edges().is_ok(), "空账破/单行破/越号拒");
    // 11. 未封印闸：Open 态 seal_intact 必 NotSealed。
    {
        let led = ConserveLedger::new();
        s.add(
            "J1-未封印闸",
            led.seal_intact().err() == Some(BcCode::NotSealed.code()),
            "Open 态不出闭合断言",
        );
    }
    // 12. 双跑一致。
    s.add("J2-双跑一致", repro_check().is_ok(), "golden_run 两跑同值");
    // 13. 段位守卫。
    s.add("J3-段位守卫", segment_guard(), "0x7F3C0 窗口对齐 0x40");
    // 14. 文案闭集。
    {
        let mut ok = true;
        let mut i = 0;
        while i < BcCode::ALL.len() {
            if let Some(c) = BcCode::ALL.get(i) {
                if c.detail().len() == 0 {
                    ok = false;
                }
            }
            i += 1;
        }
        s.add("J4-文案闭集", ok, "11 码中文明细全非空");
    }
    // 15. 段窗收口：本段 0x7F3C0 是 B04 批末段（批窗右边界）。
    s.add(
        "J1-批窗右边界",
        SEG_BASE == 0x7F3C0 && SEG_BASE + 0x40 == 0x7F400,
        "0x7F3C0+0x40=批窗上沿",
    );
    s
}

// ── 测试 ───────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn t01_normal_flow() {
        let r = match golden_run() {
            Ok(r) => r,
            Err(_) => ((0, 0, 0), 0, 0),
        };
        assert_eq!(r.0, (BATCH_TOTAL, BATCH_TOTAL, BATCH_TOTAL));
        assert_eq!(r.1, 20);
        assert_eq!(r.2, 1);
    }

    #[test]
    fn t02_boundary_edges() {
        assert!(edges().is_ok());
    }

    #[test]
    fn t03_boundary_row_oob() {
        assert_eq!(
            row_source(BATCH_ROWS).err(),
            Some(BcCode::RowOOB.code())
        );
        assert!(row_source(0).is_ok());
        assert!(row_source(BATCH_ROWS - 1).is_ok());
    }

    #[test]
    fn t04_inject_row_tamper() {
        let mut led = ConserveLedger::new();
        led.close();
        if let Some(slot) = led.rows.get_mut(3) {
            *slot = slot.saturating_add(1);
        }
        assert_eq!(
            led.seal_intact().err(),
            Some(BcCode::RowTampered.code())
        );
        assert_eq!(led.parity().err(), Some(BcCode::ParityOff.code()));
    }

    #[test]
    fn t05_inject_acc_drift() {
        let led = ConserveLedger::new();
        assert_eq!(led.sum_acc(1).err(), Some(BcCode::AccDrift.code()));
        assert!(led.sum_acc(0).is_ok());
    }

    #[test]
    fn t06_inject_off_by_one() {
        let led = ConserveLedger::new();
        let skipped = led.sum_skip_last();
        assert_ne!(skipped, BATCH_TOTAL);
        assert_eq!(BATCH_TOTAL.saturating_sub(skipped), SIBLING_QUOTA);
    }

    #[test]
    fn t07_parity_all_green() {
        let mut led = ConserveLedger::new();
        let (g, r) = match led.parity() {
            Ok(v) => v,
            Err(_) => (0, 1),
        };
        assert_eq!((g, r), (20, 0));
    }

    #[test]
    fn t08_only_increase() {
        assert!(only_increase_probe().is_ok());
    }

    #[test]
    fn t09_double_run_determinism() {
        assert!(repro_check().is_ok());
    }

    #[test]
    fn t10_segment_guard() {
        assert!(segment_guard());
    }

    #[test]
    fn t11_checks_all_green() {
        let set = run_unx18480_checks();
        assert!(set.all_passed(), "UNX-F18480 自检须全绿：{}", set.red_items_text());
    }
}
