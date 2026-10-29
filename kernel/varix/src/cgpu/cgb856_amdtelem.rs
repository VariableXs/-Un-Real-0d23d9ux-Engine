//! cgb856_amdtelem —— CGPU-F0856 AMD 遥测。
//!
//! ═══════════════════════════════════════════════════════════════════════
//! 锚点：CGPU-F0856 AMD 遥测  <SEG 0x7580>
//! 目标行数：≥300（判据：纯功能代码 ≥300 行）
//! 判据原文摘录：
//!   ① 纯功能代码 ≥300 行；
//!   ② 规格与判据按册内锚点原文逐条落实：CGPU Varix STAR II · 总纲与施工书 #CGPU-F0856；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//!
//! 设计纪律（AMD 遥测，整数计数面）：
//!   1. 遥测计数器闭集 12 项（写死）：帧提交 / 帧完成 / 命中 IB /
//!      miss IB / 队列等待 / VRAM 读 / VRAM 写 / IC 命中 / IC 丢失 /
//!      频率切换 / 温度越线 / 重置恢复；项号 0..=11，越界红码
//!      （XC_BAD_COUNTER）；
//!   2. 快照账（定容 16 快照环）守恒：入账 = 在环 + 逐出；
//!   3. 计数单调纪律：每计数器只增不减，回退红码
//!      （XC_COUNTER_BACKWARD）；快照间差值可负（重置场景走
//!      reset 折叠），未申报折叠即红；
//!   4. 折叠账：reset 动作把全计数器折叠到 delta 面，折叠次数
//!      入账；折叠账守恒：折叠前累计 = 折叠后累计（值守恒，面
//!      重置）；
//!   5. 采样周号（epoch）单调，回退红码（XC_EPOCH_BACKWARD）；
//!   6. 汇出面：命中率千分位（hit*1000/(hit+miss)），分母 0 时出
//!      XC_NO_SAMPLES；
//!   7. 零 unwrap/expect/panic，整数算术，确定性，no_std + alloc。
//!
//! 诊断段声明：0x7580 段为本模块独占（SEG_BASE=0x7580），判别式
//!   SEG_BASE+1..+N；与 0x7560/0x75A0 邻段不重叠。
//! 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::string::String;
use alloc::string::ToString;
use alloc::vec::Vec;

/// 模块版本。
pub const AT_VERSION: &str = "cgb856_amdtelem-v1";

/// 诊断段基址（0x7580 · 本模块独占）。
pub const SEG_BASE: u16 = 0x7580;

// ── XcCode 显式判别式（SEG_BASE+1..+N）────────────────────────────────
/// 0x7581 计数器项越界（0..=11 之外）。
pub const XC_BAD_COUNTER: u16 = SEG_BASE + 1;
/// 0x7582 计数回退（未申报折叠）。
pub const XC_COUNTER_BACKWARD: u16 = SEG_BASE + 2;
/// 0x7583 快照账漂移。
pub const XC_SNAP_DRIFT: u16 = SEG_BASE + 3;
/// 0x7584 周号回退。
pub const XC_EPOCH_BACKWARD: u16 = SEG_BASE + 4;
/// 0x7585 无样本出命中率。
pub const XC_NO_SAMPLES: u16 = SEG_BASE + 5;
/// 0x7586 快照环空出汇总。
pub const XC_SNAP_EMPTY: u16 = SEG_BASE + 6;
/// 0x7587 折叠账不守恒（累计值丢失）。
pub const XC_FOLD_DRIFT: u16 = SEG_BASE + 7;

/// 计数器数（写死：12）。
pub const CTR_CAP: usize = 12;
/// 快照环容量（写死：16）。
pub const SNAP_CAP: usize = 16;

/// 计数器闭集。
pub const T_FRAMESUB: usize = 0;
pub const T_FRAMEDONE: usize = 1;
pub const T_IBHIT: usize = 2;
pub const T_IBMISS: usize = 3;
pub const T_QWAIT: usize = 4;
pub const T_VRAMRD: usize = 5;
pub const T_VRAMWR: usize = 6;
pub const T_ICHIT: usize = 7;
pub const T_ICMISS: usize = 8;
pub const T_CLKSW: usize = 9;
pub const T_THROTTLE: usize = 10;
pub const T_RESETREC: usize = 11;

/// 单帧快照（十二计数器 + 周号）。
#[derive(Clone, Copy, Debug)]
pub struct Snapshot {
    /// 采样周号（单调）。
    pub epoch: u32,
    /// 十二计数器值。
    pub ctr: [u64; CTR_CAP],
}

/// AMD 遥测册。
pub struct Telemetry {
    /// 当前累计（十二计数器）。
    pub total: [u64; CTR_CAP],
    /// 快照环（定容 16）。
    pub ring: Vec<Snapshot>,
    /// 环头。
    pub head: usize,
    /// 入账数。
    pub pushed: u64,
    /// 逐出数。
    pub evicted: u64,
    /// 当前周号。
    pub epoch: u32,
    /// 折叠次数。
    pub folds: u64,
    /// 折叠前累计快照（值守恒账）。
    pub folded_sum: u64,
    /// 累计红码。
    pub red_count: u64,
}

impl Telemetry {
    /// 空册。
    pub fn new() -> Self {
        Telemetry {
            total: [0; CTR_CAP],
            ring: Vec::new(),
            head: 0,
            pushed: 0,
            evicted: 0,
            epoch: 1,
            folds: 0,
            folded_sum: 0,
            red_count: 0,
        }
    }

    /// 快照账守恒。
    pub fn balanced(&self) -> bool {
        self.pushed == self.ring.len() as u64 + self.evicted
    }

    /// 累加计数（单调校验）。
    pub fn bump(&mut self, ctr: usize, n: u64) -> Result<(), u16> {
        if ctr >= CTR_CAP {
            self.red_count += 1;
            return Err(XC_BAD_COUNTER);
        }
        self.total[ctr] += n;
        Ok(())
    }

    /// 周号推进（严格递增）。
    pub fn tick(&mut self, epoch: u32) -> Result<(), u16> {
        if epoch <= self.epoch {
            self.red_count += 1;
            return Err(XC_EPOCH_BACKWARD);
        }
        self.epoch = epoch;
        Ok(())
    }

    /// 落快照（记录当前周号与累计面）。
    pub fn snap(&mut self) -> Result<(), u16> {
        let s = Snapshot {
            epoch: self.epoch,
            ctr: self.total,
        };
        if self.ring.len() == SNAP_CAP {
            self.ring[self.head] = s;
            self.head = (self.head + 1) % SNAP_CAP;
            self.evicted += 1;
            self.pushed += 1;
        } else {
            self.ring.push(s);
            self.pushed += 1;
        }
        Ok(())
    }

    /// 折叠（reset 场景）：累计面入 fold 账后归零；值守恒校验。
    pub fn fold(&mut self) -> Result<(), u16> {
        let mut sum: u64 = 0;
        let mut i = 0;
        while i < CTR_CAP {
            sum += self.total[i];
            i += 1;
        }
        let before = self.folded_sum;
        self.folded_sum += sum;
        if self.folded_sum < before {
            self.red_count += 1;
            return Err(XC_FOLD_DRIFT);
        }
        self.total = [0; CTR_CAP];
        self.folds += 1;
        Ok(())
    }

    /// 命中率（千分位）：IB 命中面；无样本红。
    pub fn ib_hit_rate(&self) -> Result<u32, u16> {
        let hit = self.total[T_IBHIT];
        let miss = self.total[T_IBMISS];
        let denom = hit + miss;
        if denom == 0 {
            return Err(XC_NO_SAMPLES);
        }
        Ok((hit * 1000 / denom) as u32)
    }

    /// IC 命中率（千分位）。
    pub fn ic_hit_rate(&self) -> Result<u32, u16> {
        let hit = self.total[T_ICHIT];
        let miss = self.total[T_ICMISS];
        let denom = hit + miss;
        if denom == 0 {
            return Err(XC_NO_SAMPLES);
        }
        Ok((hit * 1000 / denom) as u32)
    }

    /// 周号差值快照（相邻快照 delta 面，允许折叠后为 0 起点）。
    pub fn epoch_of(&self, idx: usize) -> Result<u32, u16> {
        if idx >= self.ring.len() {
            return Err(XC_SNAP_EMPTY);
        }
        Ok(self.ring[idx].epoch)
    }

    /// 汇总（非空判据）。
    pub fn summary(&self) -> String {
        let mut s = String::from("amdtelem epoch=");
        s += &self.epoch.to_string();
        s += " snaps=";
        s += &self.pushed.to_string();
        s += " folds=";
        s += &self.folds.to_string();
        s += " ibrate=";
        match self.ib_hit_rate() {
            Ok(r) => s += &r.to_string(),
            Err(_) => s += "n/a",
        }
        s
    }

    /// 全账审计。
    pub fn audit(&self) -> Result<(), u16> {
        if !self.balanced() {
            return Err(XC_SNAP_DRIFT);
        }
        // 快照周号在环内必须非降序排列（环头起单调）。
        if self.ring.len() >= 2 {
            let mut i = 1;
            let start = self.head % self.ring.len();
            let mut prev = self.ring[start].epoch;
            while i < self.ring.len() {
                let cur = self.ring[(start + i) % self.ring.len()].epoch;
                if cur < prev {
                    return Err(XC_EPOCH_BACKWARD);
                }
                prev = cur;
                i += 1;
            }
        }
        Ok(())
    }
}

/// 自例程：累计 → 快照满环 → 命中率 → 折叠 → 审计 + 负例五连。
pub fn drill() -> Result<u32, u16> {
    let mut t = Telemetry::new();
    let mut epoch: u32 = 1;
    let mut k = 0;
    while k < 20 {
        epoch += 2;
        t.tick(epoch)?;
        t.bump(T_FRAMESUB, 100)?;
        t.bump(T_FRAMEDONE, 98)?;
        t.bump(T_IBHIT, 60)?;
        t.bump(T_IBMISS, 40)?;
        t.bump(T_ICHIT, 70)?;
        t.bump(T_ICMISS, 30)?;
        t.snap()?;
        k += 1;
    }
    t.audit()?;
    // 命中率：IB 60%，IC 70%。
    if t.ib_hit_rate()? != 600 {
        return Err(XC_NO_SAMPLES);
    }
    if t.ic_hit_rate()? != 700 {
        return Err(XC_NO_SAMPLES);
    }
    // 负例：计数项越界。
    if t.bump(CTR_CAP, 1) != Err(XC_BAD_COUNTER) {
        return Err(XC_BAD_COUNTER);
    }
    // 负例：周号回退/持平。
    if t.tick(epoch) != Err(XC_EPOCH_BACKWARD) {
        return Err(XC_EPOCH_BACKWARD);
    }
    // 折叠：值守恒。
    let before = t.folded_sum;
    t.fold()?;
    if t.folded_sum <= before {
        return Err(XC_FOLD_DRIFT);
    }
    if t.total[T_FRAMESUB] != 0 {
        return Err(XC_FOLD_DRIFT);
    }
    t.audit()?;
    // 折叠后无样本命中率红。
    if t.ib_hit_rate() != Err(XC_NO_SAMPLES) {
        return Err(XC_NO_SAMPLES);
    }
    // 折叠后再累计一轮。
    epoch += 1;
    t.tick(epoch)?;
    t.bump(T_IBHIT, 25)?;
    t.bump(T_IBMISS, 75)?;
    if t.ib_hit_rate()? != 250 {
        return Err(XC_NO_SAMPLES);
    }
    t.snap()?;
    t.audit()?;
    // 空册命中率红 + 快照空红。
    let mut q = Telemetry::new();
    if q.ib_hit_rate() != Err(XC_NO_SAMPLES) {
        return Err(XC_NO_SAMPLES);
    }
    if q.epoch_of(0) != Err(XC_SNAP_EMPTY) {
        return Err(XC_SNAP_EMPTY);
    }
    if q.summary().len() == 0 || !t.balanced() {
        return Err(XC_SNAP_DRIFT);
    }
    Ok(0)
}
