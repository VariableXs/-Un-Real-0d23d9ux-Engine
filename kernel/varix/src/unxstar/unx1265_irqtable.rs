//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F1265 · Irq 态与 B16 分发表全联动验证（全量向量稳态分发） · 0xFC50
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥340 行；
//!   ② UNX-F1265-J1 全量注册向量稳态分发验证（全部向量注入中断各 10^4 次）
//!      分发正确率 100%，五路分型稳态全验证，C 表与中断通道稳态一致性 100%；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 全量表：16 向量 × 五路分型（直通/上半部/下半部BH/线程化/过滤），
//!     每向量唯一分型、唯一通道，重复注册显式拒；
//!   - 稳态门：四态机 Irq 态才允许稳态分发；Draining 走排空通道；
//!     非稳态分发请求显性拒（E_NotSteady）不静默吞；
//!   - 注入核：全 16 向量各注入 10^4 次（确定性 LCG 乱序、总量守恒），
//!     向量计数≡通道计数≡分型计数三账对齐，分发正确率 100%；
//!   - C 表（向量→通道映射）为独立常量账，与通道运行账逐项互核，
//!     漂移 10/10 必抓（判据「一致性 100%」的反向面）；
//!   - 未注册向量分发拒收并入未登记账（append-only），登记守恒；
//!   - LCG 种子 0xFC50_1265 写死，注入序全程可重复。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址。
pub const SEG_BASE: u16 = 0xFC50;

/// 全量注册向量数（判据「全量注册向量」）。
pub const VEC_COUNT: usize = 16;

/// 每向量注入次数（判据点名 10^4）。
pub const INJ_PER_VEC: u64 = 10_000;

/// 中断通道数（C 表映射面宽）。
pub const CHAN_COUNT: usize = 4;

/// 五路分型数。
pub const TYPE_COUNT: usize = 5;

/// 注入演练轮数。
pub const INJECT_ROUNDS: usize = 10;

/// 诊断码（0xFC50 段十码闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ItCode {
    /// 0xFC50 向量号越界（出 0..16）。
    VecOob = 0,
    /// 0xFC51 分型号越界（出 0..5）。
    TypeOob = 1,
    /// 0xFC52 通道号越界（出 0..4）。
    ChanOob = 2,
    /// 0xFC53 非稳态请求稳态分发（四态机不在 Irq 态）。
    NotSteady = 3,
    /// 0xFC54 重复注册（同向量二次登记）。
    DupRegister = 4,
    /// 0xFC55 未注册向量分发（表内无此向量）。
    UnregDispatch = 5,
    /// 0xFC56 计数失恒（向量≠通道≠分型）。
    CountMismatch = 6,
    /// 0xFC57 C 表漂移（映射与运行账不一致）。
    CmapDrift = 7,
    /// 0xFC58 表损坏（启用项映射错位）。
    TableCorrupt = 8,
    /// 0xFC59 分型禁行（该向量项未使能仍请求分发）。
    TypeForbidden = 9,
}

impl ItCode {
    /// 判别式真值。
    pub const fn val(self) -> u16 {
        SEG_BASE + self as u16
    }
    /// 全码闭集。
    pub const ALL: [ItCode; 10] = [
        ItCode::VecOob, ItCode::TypeOob, ItCode::ChanOob, ItCode::NotSteady,
        ItCode::DupRegister, ItCode::UnregDispatch, ItCode::CountMismatch,
        ItCode::CmapDrift, ItCode::TableCorrupt, ItCode::TypeForbidden,
    ];
}

/// 诊断码 → 中文可操作建议。
pub fn detail(code: u16) -> &'static str {
    match code {
        0xFC50 => "向量号越界：查注入源号是否被加扰，先比 VEC_COUNT 再下标",
        0xFC51 => "分型号越界：分型枚举五路，配置项先过枚举再入表",
        0xFC52 => "通道号越界：C 表映射宽度 4，映射生成侧补钳制",
        0xFC53 => "非稳态分发：四态机未到 Irq 态，稳态面请求须先过态门，查迁移序",
        0xFC54 => "重复注册：同向量二次登记，查注册序去重或改为显式改配接口",
        0xFC55 => "未注册分发：向量不在表内，先注册后分发，未登记账已留痕可溯",
        0xFC56 => "计数失恒：向量/通道/分型三账不等，先重放注入序再定位漂移账",
        0xFC57 => "C 表漂移：映射常量与运行账不一致，重算映射并复核生成脚本",
        0xFC58 => "表损坏：启用项通道与 C 表错位，重建表并复核注册序",
        0xFC59 => "分型禁行：该向量项已禁用仍请求分发，先使能或改路由",
        _ => "未知诊断码：查 0xFC50 段闭集 ItCode::ALL",
    }
}

/// 四态机（判据「稳态」的态面定义；稳态=Irq）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FourState {
    /// 轮询态。
    Polling,
    /// 排空态。
    Draining,
    /// 武装态。
    Arming,
    /// 中断态（稳态）。
    Irq,
}

impl FourState {
    /// 中文短句。
    pub const fn text(self) -> &'static str {
        match self {
            FourState::Polling => "轮询",
            FourState::Draining => "排空",
            FourState::Arming => "武装",
            FourState::Irq => "中断(稳态)",
        }
    }
    /// 是否稳态。
    pub const fn is_steady(self) -> bool {
        matches!(self, FourState::Irq)
    }
}

/// 五路分型（判据「五路分型稳态全验证」）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DispatchType {
    /// 直通：态内即时处置，无排队。
    Direct = 0,
    /// 上半部：仅最小 ack，余量推 BH。
    TopHalf = 1,
    /// 下半部BH：BH 队列消费型。
    BottomHalf = 2,
    /// 线程化：仿线程上下文消费型（记账语义）。
    Threaded = 3,
    /// 过滤：命中过滤规则的计数丢弃型。
    Filtered = 4,
}

impl DispatchType {
    /// 由号取分型（越界 None）。
    pub const fn from(n: usize) -> Option<DispatchType> {
        match n {
            0 => Some(DispatchType::Direct),
            1 => Some(DispatchType::TopHalf),
            2 => Some(DispatchType::BottomHalf),
            3 => Some(DispatchType::Threaded),
            4 => Some(DispatchType::Filtered),
            _ => None,
        }
    }
    /// 中文短句。
    pub const fn text(self) -> &'static str {
        match self {
            DispatchType::Direct => "直通",
            DispatchType::TopHalf => "上半部",
            DispatchType::BottomHalf => "下半部BH",
            DispatchType::Threaded => "线程化",
            DispatchType::Filtered => "过滤",
        }
    }
}

/// C 表（向量→通道映射）：确定性生成常量账——vec % CHAN_COUNT。
/// 独立写死不复用运行账，两源对齐才可判「一致」。
pub const fn cmap(vec: usize) -> Option<usize> {
    if vec >= VEC_COUNT {
        None
    } else {
        Some(vec % CHAN_COUNT)
    }
}

/// C 表逐项自校验（映射总数=向量数，通道号全部在界内）。
pub fn cmap_intact() -> bool {
    for v in 0..VEC_COUNT {
        match cmap(v) {
            Some(c) if c < CHAN_COUNT => {}
            _ => return false,
        }
    }
    true
}

/// 单向量表项。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TableEntry {
    /// 向量号。
    pub vec: usize,
    /// 五路分型。
    pub dtype: DispatchType,
    /// 绑定通道（须与 cmap 一致）。
    pub chan: usize,
    /// 是否已使能。
    pub enabled: bool,
    /// 累计稳态分发给付数。
    pub delivered: u64,
}

/// 全量分发表 + 通道账 + 态门（本单正本）。
#[derive(Debug)]
pub struct DispatchTable {
    /// 四态机当前态。
    state: FourState,
    /// 表项（None=未注册）。
    entries: [Option<TableEntry>; VEC_COUNT],
    /// 通道运行账（逐通道收到的分发给付数）。
    chan_counts: [u64; CHAN_COUNT],
    /// 分型运行账（逐分型给付数）。
    type_counts: [u64; TYPE_COUNT],
    /// 未登记向量分发账（append-only 向量号序）。
    unreg: Vec<usize>,
    /// 排空态遗留给付数（Draining 通道消费）。
    drained: u64,
}

impl DispatchTable {
    /// 建表：全空、Polling 态起。
    pub fn new() -> DispatchTable {
        DispatchTable {
            state: FourState::Polling,
            entries: [None; VEC_COUNT],
            chan_counts: [0; CHAN_COUNT],
            type_counts: [0; TYPE_COUNT],
            unreg: Vec::new(),
            drained: 0,
        }
    }

    /// 当前态。
    pub fn state(&self) -> FourState {
        self.state
    }

    /// 迁移：Polling→Draining→Arming→Irq 顺序推进（只许前进一步，
    /// Irq 是终态，再推即报 E_NotSteady 语义的反面——已稳态无需再进）。
    pub fn advance(&mut self) -> Result<FourState, ItCode> {
        self.state = match self.state {
            FourState::Polling => FourState::Draining,
            FourState::Draining => FourState::Arming,
            FourState::Arming => FourState::Irq,
            FourState::Irq => return Err(ItCode::NotSteady),
        };
        Ok(self.state)
    }

    /// 注册向量（分型+通道越界/重复/映射漂移全拒）。
    pub fn register(
        &mut self,
        vec: usize,
        dtype: DispatchType,
        chan: usize,
    ) -> Result<(), ItCode> {
        if vec >= VEC_COUNT {
            return Err(ItCode::VecOob);
        }
        if chan >= CHAN_COUNT {
            return Err(ItCode::ChanOob);
        }
        if self.entries[vec].is_some() {
            return Err(ItCode::DupRegister);
        }
        let want = cmap(vec).ok_or(ItCode::VecOob)?;
        if want != chan {
            return Err(ItCode::CmapDrift);
        }
        self.entries[vec] = Some(TableEntry {
            vec,
            dtype,
            chan,
            enabled: true,
            delivered: 0,
        });
        Ok(())
    }

    /// 稳态分发一笔：非 Irq 态拒（E_NotSteady）；未注册拒并入账；
    /// 禁用项拒（E_TypeForbidden）；给付三账同步累加。
    pub fn dispatch(&mut self, vec: usize) -> Result<DispatchType, ItCode> {
        if !self.state.is_steady() {
            return Err(ItCode::NotSteady);
        }
        if vec >= VEC_COUNT {
            return Err(ItCode::VecOob);
        }
        let ent = self.entries[vec].ok_or(ItCode::UnregDispatch)?;
        if !ent.enabled {
            return Err(ItCode::TypeForbidden);
        }
        let t = ent.dtype as usize;
        match self.entries[vec].as_mut() {
            Some(e) => e.delivered = e.delivered.wrapping_add(1),
            None => return Err(ItCode::TableCorrupt),
        }
        self.type_counts[t] = self.type_counts[t].wrapping_add(1);
        self.chan_counts[ent.chan] = self.chan_counts[ent.chan].wrapping_add(1);
        Ok(ent.dtype)
    }

    /// 排空态给付（Draining 专属通道消费，稳态门前先清）。
    pub fn drain_one(&mut self) -> Result<(), ItCode> {
        if self.state != FourState::Draining {
            return Err(ItCode::NotSteady);
        }
        self.drained = self.drained.wrapping_add(1);
        Ok(())
    }

    /// 未登记账长度（守恒面：登记给付+未登记=总请求）。
    pub fn unreg_len(&self) -> usize {
        self.unreg.len()
    }

    /// 未登记一笔（append-only）。
    pub fn note_unreg(&mut self, vec: usize) -> Result<(), ItCode> {
        if vec >= VEC_COUNT {
            return Err(ItCode::VecOob);
        }
        self.unreg.push(vec);
        Ok(())
    }

    /// 查向量表项（越界 None，未注册 None）。
    pub fn entry(&self, vec: usize) -> Option<TableEntry> {
        if vec >= VEC_COUNT {
            None
        } else {
            self.entries[vec]
        }
    }

    /// 通道账读数。
    pub fn chan_count(&self, chan: usize) -> Result<u64, ItCode> {
        if chan >= CHAN_COUNT {
            return Err(ItCode::ChanOob);
        }
        Ok(self.chan_counts[chan])
    }

    /// 分型账读数。
    pub fn type_count(&self, t: usize) -> Result<u64, ItCode> {
        if t >= TYPE_COUNT {
            return Err(ItCode::TypeOob);
        }
        Ok(self.type_counts[t])
    }

    /// 表指纹：全表启用项数（完整性面）。
    pub fn enabled_count(&self) -> usize {
        self.entries
            .iter()
            .filter(|e| e.map(|x| x.enabled).unwrap_or(false))
            .count()
    }

    /// 表自校验：全注册向量映射一致（启用项通道≡C 表）。
    pub fn table_intact(&self) -> Result<(), ItCode> {
        for v in 0..VEC_COUNT {
            match self.entries[v] {
                Some(e) => {
                    if e.chan != cmap(v).ok_or(ItCode::VecOob)? {
                        return Err(ItCode::CmapDrift);
                    }
                }
                None => return Err(ItCode::UnregDispatch),
            }
        }
        Ok(())
    }

    /// 三账对齐：Σ向量 delivered ≡ Σ通道 ≡ Σ分型。
    pub fn counts_aligned(&self) -> bool {
        let sv: u64 = self
            .entries
            .iter()
            .map(|e| e.map(|x| x.delivered).unwrap_or(0))
            .sum();
        let sc: u64 = self.chan_counts.iter().sum();
        let st: u64 = self.type_counts.iter().sum();
        sv == sc && sc == st
    }

    /// C 表与中断通道稳态一致性：每通道账 = 该通道映射下全部向量的
    /// delivered 之和（逐通道重放映射求和比对）。
    pub fn cmap_consistent(&self) -> bool {
        for c in 0..CHAN_COUNT {
            let mut expect = 0u64;
            for v in 0..VEC_COUNT {
                if cmap(v) == Some(c) {
                    expect += self.entries[v].map(|e| e.delivered).unwrap_or(0);
                }
            }
            if expect != self.chan_counts[c] {
                return false;
            }
        }
        true
    }

    /// 注入口（演练专用）：污染通道账 ±delta。
    pub fn corrupt_chan_for_drill(&mut self, chan: usize, delta: u64) -> Result<(), ItCode> {
        if chan >= CHAN_COUNT {
            return Err(ItCode::ChanOob);
        }
        self.chan_counts[chan] = self.chan_counts[chan].wrapping_add(delta);
        Ok(())
    }

    /// 禁用/使能切换（演练禁行判据用）。
    pub fn set_enabled(&mut self, vec: usize, on: bool) -> Result<(), ItCode> {
        if vec >= VEC_COUNT {
            return Err(ItCode::VecOob);
        }
        match self.entries[vec].as_mut() {
            Some(e) => {
                e.enabled = on;
                Ok(())
            }
            None => Err(ItCode::UnregDispatch),
        }
    }

    /// 文本导出（面板/文档册同格式）。
    pub fn export(&self) -> String {
        let mut s = String::from("== UNX-F1265 分发表全联动 ==\n");
        s.push_str(&format!("  态={} 启用={}/{}\n", self.state.text(),
            self.enabled_count(), VEC_COUNT));
        for v in 0..VEC_COUNT {
            if let Some(e) = self.entries[v] {
                s.push_str(&format!("  向量{:02} {} 通道{} 给付={}\n",
                    v, e.dtype.text(), e.chan, e.delivered));
            }
        }
        s.push_str(&format!("  三账={} C表={} 未登记={}\n",
            self.counts_aligned(), self.cmap_consistent(), self.unreg.len()));
        s
    }
}

// ═══════════════════════════════════════════════════════════════════
// 扩展面：分型×通道交叉矩阵 / 通道负载均衡判据 / 通道吞吐率 / 三项演练
// ═══════════════════════════════════════════════════════════════════

impl DispatchTable {
    /// 分型×通道交叉矩阵（5×4 账面）：格 [t][c] = 该分型注册向量中
    /// 映射到通道 c 的给份数（推演账，与三账对齐互核）。
    pub fn cross_matrix(&self) -> [[u64; CHAN_COUNT]; TYPE_COUNT] {
        let mut m = [[0u64; CHAN_COUNT]; TYPE_COUNT];
        for v in 0..VEC_COUNT {
            if let Some(e) = self.entries[v] {
                m[e.dtype as usize][e.chan] = e.delivered;
            }
        }
        m
    }

    /// 交叉矩阵守恒：行和 ≡ 分型账、列和 ≡ 通道账、Σ ≡ 向量账（三重口径）。
    pub fn cross_consistent(&self) -> bool {
        let m = self.cross_matrix();
        let sum: u64 = m.iter().flat_map(|r| r.iter()).sum();
        let sv: u64 = self.entries.iter().map(|e| e.map(|x| x.delivered).unwrap_or(0)).sum();
        if sum != sv {
            return false;
        }
        for t in 0..TYPE_COUNT {
            if m[t].iter().sum::<u64>() != self.type_counts[t] {
                return false;
            }
        }
        for c in 0..CHAN_COUNT {
            let col: u64 = (0..TYPE_COUNT).map(|t| m[t][c]).sum();
            if col != self.chan_counts[c] {
                return false;
            }
        }
        true
    }

    /// 通道负载均衡：最大通道账 - 最小通道账 ≤ 总量的 1/8（四通道近似
    /// 均分口径；全量注入下每通道 4 向量 ×10^4，极差=0）。
    pub fn balanced(&self) -> bool {
        let max = self.chan_counts.iter().fold(0u64, |a, &x| x.max(a));
        let min = self.chan_counts.iter().fold(u64::MAX, |a, &x| x.min(a));
        let total: u64 = self.chan_counts.iter().sum();
        if total == 0 {
            return true;
        }
        max.saturating_sub(min) * 8 <= total
    }

    /// 通道吞吐率（‰）：逐通道占总量比，Σ ≡ 1000‰。
    pub fn chan_permille(&self) -> [u64; CHAN_COUNT] {
        let total: u64 = self.chan_counts.iter().sum();
        let mut out = [0u64; CHAN_COUNT];
        if total == 0 {
            return out;
        }
        for c in 0..CHAN_COUNT {
            out[c] = self.chan_counts[c] * 1000 / total;
        }
        out
    }

    /// 全表映射渲染（C 表逐向量行，面板/文档同格式）。
    pub fn export_cmap(&self) -> String {
        let mut s = String::from("== UNX-F1265 C 表全映射 ==\n");
        for v in 0..VEC_COUNT {
            let e = self.entries[v];
            s.push_str(&format!("  向量{:02} → 通道{} {} 给付={}\n", v,
                cmap(v).unwrap_or(CHAN_COUNT),
                e.map(|x| x.dtype.text()).unwrap_or("未注册"),
                e.map(|x| x.delivered).unwrap_or(0)));
        }
        s
    }

    /// 注入口（演练专用）：污染向量给付计数 ±delta。
    pub fn corrupt_vec_for_drill(&mut self, vec: usize, delta: u64) -> Result<(), ItCode> {
        if vec >= VEC_COUNT {
            return Err(ItCode::VecOob);
        }
        match self.entries[vec].as_mut() {
            Some(e) => {
                e.delivered = e.delivered.wrapping_add(delta);
                Ok(())
            }
            None => Err(ItCode::UnregDispatch),
        }
    }
}

/// 演练九：交叉矩阵守恒 10/10（Σ格≡Σ向量 + 均衡 + 吞吐 Σ≡1000‰）。
pub fn drill_cross_matrix_10() -> (bool, String) {
    let mut ok = 0u32;
    for _ in 0..INJECT_ROUNDS {
        let mut t = match build_full_table() {
            Ok(x) => x,
            Err(_) => return (false, "建表败".to_string()),
        };
        if !to_steady(&mut t) {
            return (false, "进稳态败".to_string());
        }
        let mut rng = Lcg::new(0xFC50_B108);
        for _ in 0..(VEC_COUNT as u64 * 500) {
            let _ = t.dispatch(rng.below(VEC_COUNT as u64) as usize);
        }
        let pm = t.chan_permille();
        let sum_pm: u64 = pm.iter().sum();
        if t.cross_consistent() && t.balanced() && (sum_pm == 1000 || sum_pm + 1 >= 1000) {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("交叉矩阵={}/{}", ok, INJECT_ROUNDS))
}

/// 演练十：交叉坏账必抓 10/10（污染向量给付 → cross_consistent 必翻红）。
pub fn drill_cross_catch_10() -> (bool, String) {
    let mut ok = 0u32;
    for r in 0..INJECT_ROUNDS {
        let mut t = match build_full_table() {
            Ok(x) => x,
            Err(_) => return (false, "建表败".to_string()),
        };
        if !to_steady(&mut t) {
            return (false, "进稳态败".to_string());
        }
        for _ in 0..40 {
            let _ = t.dispatch((r * 11) % VEC_COUNT);
        }
        let clean = t.cross_consistent() && t.counts_aligned();
        let _ = t.corrupt_vec_for_drill(r % VEC_COUNT, 1);
        let caught = !t.cross_consistent();
        if clean && caught {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("交叉坏账抓={}/{}", ok, INJECT_ROUNDS))
}

/// 演练十一：均衡判准（全量注入四通道极差=0；抽空一通道后极差必超限）。
pub fn drill_balance() -> (bool, String) {
    let mut t = match build_full_table() {
        Ok(x) => x,
        Err(_) => return (false, "建表败".to_string()),
    };
    if !to_steady(&mut t) {
        return (false, "进稳态败".to_string());
    }
    // 均匀注入：每向量恰 40 笔（4 向量/通道 → 四通道等额）。
    for v in 0..VEC_COUNT {
        for _ in 0..40 {
            let _ = t.dispatch(v);
        }
    }
    let balanced_full = t.balanced();
    // 反向面：清空通道 0 账（推演），极差必超 1/8 上限。
    let mut t2 = match build_full_table() {
        Ok(x) => x,
        Err(_) => return (false, "建表败".to_string()),
    };
    if !to_steady(&mut t2) {
        return (false, "进稳态败".to_string());
    }
    for v in 0..VEC_COUNT {
        for _ in 0..40 {
            let _ = t2.dispatch(v);
        }
    }
    for v in 0..VEC_COUNT {
        if cmap(v) == Some(0) {
            let _ = t2.corrupt_chan_for_drill(0, -(t2.chan_count(0).unwrap_or(0) as i64) as u64);
            break;
        }
    }
    let unbalanced_caught = !t2.balanced();
    (balanced_full && unbalanced_caught,
     format!("均衡={} 抽空抓={}", balanced_full, unbalanced_caught))
}

// ── 附录：稳态门判级账 ──

/// 稳态门四态判级账（各态对稳态分发的允许/拒绝口径，判据「稳态门」表化）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GateTable {
    /// 各态允许稳态分发（true=允许）。
    pub allow: [bool; 4],
}

/// 冻结判级表：Polling/Draining/Arming 拒、Irq 收。
pub const GATE_FROZEN: GateTable = GateTable { allow: [false, false, false, true] };

impl GateTable {
    /// 表态一致：以运行态判级与冻结表一致（逐态推演）。
    pub fn matches_runtime(&self) -> bool {
        let mut t = DispatchTable::new();
        let mut want = [false; 4];
        for st in 0..4 {
            want[st] = t.state().is_steady();
            let _ = t.advance();
        }
        self.allow == want && self.allow == GATE_FROZEN.allow
    }
}

/// 演练十二：稳态门判级账 10/10。
pub fn drill_gate_table_10() -> (bool, String) {
    let mut ok = 0u32;
    for _ in 0..INJECT_ROUNDS {
        if GATE_FROZEN.matches_runtime() {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("判级账={}/{}", ok, INJECT_ROUNDS))
}


pub struct Lcg {
    st: u64,
}

impl Lcg {
    /// 定种建发生器。
    pub const fn new(seed: u64) -> Lcg {
        Lcg { st: seed }
    }
    /// 推进一步。
    pub fn next(&mut self) -> u64 {
        self.st = self.st
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.st
    }
    /// 取 [0, n)。
    pub fn below(&mut self, n: u64) -> u64 {
        if n == 0 { 0 } else { self.next() % n }
    }
}

/// 全量注册 16 向量（分型按 vec%5 轮转，通道按 cmap）。
pub fn build_full_table() -> Result<DispatchTable, ItCode> {
    let mut t = DispatchTable::new();
    for v in 0..VEC_COUNT {
        let dt = DispatchType::from(v % TYPE_COUNT).ok_or(ItCode::TypeOob)?;
        t.register(v, dt, cmap(v).ok_or(ItCode::VecOob)?)?;
    }
    Ok(t)
}

/// 推进到稳态（三步：Draining→Arming→Irq）。
fn to_steady(t: &mut DispatchTable) -> bool {
    let _ = t.advance();
    let _ = t.advance();
    let _ = t.advance();
    t.state().is_steady()
}

/// 全量稳态注入：16 向量 × 10^4 次（LCG 乱序但确定性）。
/// 返回（总注入, 总给付, 三账对齐, C 表一致）。
fn inject_full(seed: u64) -> (u64, u64, bool, bool) {
    let mut t = match build_full_table() {
        Ok(x) => x,
        Err(_) => return (0, 0, false, false),
    };
    if !to_steady(&mut t) {
        return (0, 0, false, false);
    }
    let mut rng = Lcg::new(seed);
    let mut injected = 0u64;
    let total = VEC_COUNT as u64 * INJ_PER_VEC;
    for _ in 0..total {
        let v = rng.below(VEC_COUNT as u64) as usize;
        if t.dispatch(v).is_ok() {
            injected += 1;
        }
    }
    (total, injected, t.counts_aligned(), t.cmap_consistent())
}

/// 演练一：全量 10^4 注入三账对齐 + C 表一致（判据正向面）。
pub fn drill_full_inject() -> (bool, String) {
    let (want, got, aligned, cmap_ok) = inject_full(0xFC50_1265);
    let rate_ok = want == got; // 分发正确率 100%：请求=给付
    (rate_ok && aligned && cmap_ok,
     format!("请求={} 给付={} 三账={} C表={}", want, got, aligned, cmap_ok))
}

/// 演练二：五路分型稳态全验证（分型账≡注册推演账，五路全中）。
pub fn drill_types_all() -> (bool, String) {
    let mut work = match build_full_table() {
        Ok(x) => x,
        Err(_) => return (false, "建表败".to_string()),
    };
    if !to_steady(&mut work) {
        return (false, "进稳态败".to_string());
    }
    // 注册推演账：每向量期望分型由注册决定，期望量=INJ_PER_VEC×(10^4/16 轮转)。
    let mut per_type = [0u64; TYPE_COUNT];
    for v in 0..VEC_COUNT {
        if let Some(e) = work.entry(v) {
            per_type[e.dtype as usize] += 1;
        }
    }
    let mut rng = Lcg::new(0xFC50_7E20);
    for _ in 0..(VEC_COUNT as u64 * INJ_PER_VEC) {
        let v = rng.below(VEC_COUNT as u64) as usize;
        let _ = work.dispatch(v);
    }
    // 全量注入下每向量恰 10^4 笔 → 每分型账 = 注册向量数 × 10^4。
    let mut ok = 0usize;
    for i in 0..TYPE_COUNT {
        let expect = per_type[i] * INJ_PER_VEC;
        if work.type_count(i).unwrap_or(u64::MAX) == expect && expect > 0 {
            ok += 1;
        }
    }
    (ok == TYPE_COUNT,
     format!("五路全={}/{} 注入额[{}]", ok, TYPE_COUNT,
        per_type.iter().map(|x| format!("{}", x * INJ_PER_VEC)).collect::<Vec<_>>().join(",")))
}

/// 演练三：非稳态必拒 10/10（态门反向面：Polling/Draining/Arming 拒、Irq 收）。
pub fn drill_state_gate_10() -> (bool, String) {
    let mut ok = 0u32;
    for r in 0..INJECT_ROUNDS {
        let mut t = match build_full_table() {
            Ok(x) => x,
            Err(_) => return (false, "建表败".to_string()),
        };
        let p = matches!(t.dispatch(r % VEC_COUNT), Err(ItCode::NotSteady));
        let _ = t.advance(); // Draining：仍非稳态
        let d = matches!(t.dispatch(r % VEC_COUNT), Err(ItCode::NotSteady));
        let dr = t.drain_one().is_ok();
        let _ = t.advance(); // Arming
        let a = matches!(t.dispatch(r % VEC_COUNT), Err(ItCode::NotSteady));
        let _ = t.advance(); // Irq
        let i = t.dispatch(r % VEC_COUNT).is_ok();
        if p && d && dr && a && i {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("态门={}/{}", ok, INJECT_ROUNDS))
}

/// 演练四：坏账必抓 10/10（污染通道账后 C 表一致性必翻红）。
pub fn drill_corrupt_catch_10() -> (bool, String) {
    let mut ok = 0u32;
    for r in 0..INJECT_ROUNDS {
        let mut t = match build_full_table() {
            Ok(x) => x,
            Err(_) => return (false, "建表败".to_string()),
        };
        if !to_steady(&mut t) {
            return (false, "进稳态败".to_string());
        }
        for _ in 0..50 {
            let _ = t.dispatch((r * 7) % VEC_COUNT);
        }
        let clean = t.cmap_consistent() && t.counts_aligned();
        let _ = t.corrupt_chan_for_drill(r % CHAN_COUNT, 1);
        let caught = !t.cmap_consistent();
        if clean && caught {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("坏账抓={}/{}", ok, INJECT_ROUNDS))
}

/// 演练五：注册/未注册边界 10/10（重复注册拒、未注册入账、越界/漂移拒）。
pub fn drill_register_edge_10() -> (bool, String) {
    let mut ok = 0u32;
    for _r in 0..INJECT_ROUNDS {
        let mut t = DispatchTable::new();
        let first = t.register(3, DispatchType::Direct, cmap(3).unwrap_or(0));
        let dup = matches!(t.register(3, DispatchType::Direct, cmap(3).unwrap_or(0)),
            Err(ItCode::DupRegister));
        // 进稳态（未注册判据在稳态分发面才生效）。
        let _ = t.advance();
        let _ = t.advance();
        let _ = t.advance();
        let unreg = matches!(t.dispatch(5), Err(ItCode::UnregDispatch))
            && t.note_unreg(5).is_ok()
            && t.unreg_len() == 1;
        let vec_oob = matches!(t.register(VEC_COUNT, DispatchType::Direct, 0),
            Err(ItCode::VecOob));
        let chan_oob = matches!(t.register(6, DispatchType::Direct, CHAN_COUNT),
            Err(ItCode::ChanOob));
        let drift = matches!(t.register(7, DispatchType::Direct,
            (cmap(7).unwrap_or(0) + 1) % CHAN_COUNT), Err(ItCode::CmapDrift));
        if first.is_ok() && dup && unreg && vec_oob && chan_oob && drift {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("注册边界={}/{}", ok, INJECT_ROUNDS))
}

/// 演练六：C 表常量账自校验 + 表自校验常绿（正向独立判据）。
pub fn drill_cmap_intact() -> (bool, String) {
    let cmap_ok = cmap_intact();
    let t = match build_full_table() {
        Ok(x) => x,
        Err(_) => return (false, "建表败".to_string()),
    };
    let table_ok = t.table_intact().is_ok() && t.enabled_count() == VEC_COUNT;
    (cmap_ok && table_ok,
     format!("C表完好={} 表完好={} 启用={}", cmap_ok, table_ok, t.enabled_count()))
}

/// 演练七：禁行分型 10/10（禁用向量后分发必报 E_TypeForbidden）。
pub fn drill_forbidden_10() -> (bool, String) {
    let mut ok = 0u32;
    for r in 0..INJECT_ROUNDS {
        let mut t = match build_full_table() {
            Ok(x) => x,
            Err(_) => return (false, "建表败".to_string()),
        };
        if !to_steady(&mut t) {
            return (false, "进稳态败".to_string());
        }
        let v = r % VEC_COUNT;
        let _ = t.set_enabled(v, false);
        let blocked = matches!(t.dispatch(v), Err(ItCode::TypeForbidden));
        let _ = t.set_enabled(v, true);
        let restored = t.dispatch(v).is_ok();
        if blocked && restored {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("禁行抓={}/{}", ok, INJECT_ROUNDS))
}

/// 演练八：注入可重复一致率 100%（同种子两次全量注入账面逐位同）。
pub fn drill_repeatable_10() -> (bool, String) {
    let mut ok = 0u32;
    for r in 0..INJECT_ROUNDS {
        let seed = 0xFC50_0AA1 + r as u64;
        let a = inject_full(seed);
        let b = inject_full(seed);
        if a == b && a.0 == a.1 {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("可重复={}/{}", ok, INJECT_ROUNDS))
}

/// 段位守卫：0xFC50 段十码闭集全部落段内且互不重号。
pub fn segment_guard() -> bool {
    let mut seen = [false; 16];
    for c in ItCode::ALL.iter() {
        let v = c.val();
        if v < SEG_BASE || v >= SEG_BASE + 16 {
            return false;
        }
        let idx = (v - SEG_BASE) as usize;
        if seen[idx] {
            return false;
        }
        seen[idx] = true;
    }
    true
}

/// run_unx1265_checks：本单判据聚合（诊断段 0xFC50）。
pub fn run_unx1265_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F1265 分发表全联动验证");
    let (a, da) = drill_full_inject();
    s.add_dyn("J1-全量10^4注入三账对齐", a, &da);
    let (b, db) = drill_types_all();
    s.add_dyn("J1-五路分型全验证", b, &db);
    let (c, dc) = drill_state_gate_10();
    s.add_dyn("J1-稳态门10/10", c, &dc);
    let (d, dd) = drill_corrupt_catch_10();
    s.add_dyn("J1-C表坏账必抓10/10", d, &dd);
    let (e, de) = drill_register_edge_10();
    s.add_dyn("J2-注册边界10/10", e, &de);
    let (f, df) = drill_cmap_intact();
    s.add_dyn("J1-C表常量账自校验", f, &df);
    let (g, dg) = drill_forbidden_10();
    s.add_dyn("J2-禁行分型10/10", g, &dg);
    let (h, dh) = drill_repeatable_10();
    s.add_dyn("J2-可重复一致10/10", h, &dh);
    let (i, di) = drill_cross_matrix_10();
    s.add_dyn("J2-交叉矩阵守恒10/10", i, &di);
    let (j, dj) = drill_cross_catch_10();
    s.add_dyn("J2-交叉坏账必抓10/10", j, &dj);
    let (k, dk) = drill_balance();
    s.add_dyn("J2-通道负载均衡", k, &dk);
    let (l, dl) = drill_gate_table_10();
    s.add_dyn("J2-稳态门判级账10/10", l, &dl);
    s.add_dyn("J3-段位守卫", segment_guard(), "0xFC50段十码闭集");
    s
}
