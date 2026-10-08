//! CGPU-F1922 · 显示枚举与热插拔（CGPU-M 域 · 批次 M01 · 单 02）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/CGPU Varix STAR II · 总纲与施工书.md#CGPU-F1922`
//!
//! 枚举热插拔：显示器枚举（当前连接/能力快照——枚举规范）；热插拔事件
//! 去抖（物理抖动防护——去抖规范）；枚举-事件竞态防护（序列号一致——
//! **竞态=缺陷红线**）；显示器状态机（connected/enabled/mode-set——状态
//! 机规范）；多屏同时热插拔批量处理（固定序确定性）。
//!
//! ## 要点一：枚举是能力快照不是名单
//!
//! 每台显示器枚举产出快照（display_id + EDID 指纹 + 枚举时刻序列号）：
//! 指纹对账防换屏不感知，序列号是竞态对账的基准——没有 seq 的枚举
//! 等于裸奔。
//!
//! ## 要点二：去抖窗口内只认第一个事件
//!
//! 物理抖动=连接/断开快速翻转。去抖采用 leading-edge 语义：窗口
//! DEBOUNCE_MS 内同屏后续事件一律吸收，窗外恰端点放行——窗口内
//! "聪明的合并"是暗行为，显性吸收才是纪律。
//!
//! ## 要点三：竞态=缺陷（红线）
//!
//! 枚举-事件竞态以序列号一致判定：事件 seq 必须 == 快照 seq + 1
//! （事件由总线单调分配）。seq 落后/跳变即竞态已发生——按缺陷拒绝
//! 并立案，绝不用"过期事件也凑合用"。
//!
//! ## 要点四：状态机三态单向
//!
//! connected → enabled → mode-set；非法迁移（未连接就 enable、观测段
//! 回退式跳变）显性拒绝；mode-set 允许原地重复（模式重设是常态）。
//!
//! ## 要点五：批量=固定序去重
//!
//! 多屏同时热插拔按 display_id 升序、同屏多事件取最后一条——批次序
//! 与到达序无关，重跑一致（确定性纪律与 F0002 归并同源）。
//!
//! ## 要点六：诊断码延续 0x54xx 域段
//!
//! cgm01 占 0x5401~0x5407；本单占 0x5408~0x540C；判据防自判死断言
//! 与 cgm01 码互异。

// ---------------------------------------------------------------------------
// 导入（no_std 三件套）
// ---------------------------------------------------------------------------

use alloc::vec::Vec;
use crate::cgpu::cgm01_display::VmCode;

// ---------------------------------------------------------------------------
// 一、枚举规范：能力快照
// ---------------------------------------------------------------------------

/// 显示器能力快照（枚举产出；seq 是竞态对账基准）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DisplaySnapshot {
    /// 显示器稳定标识（连接器编号派生）。
    pub display_id: u32,
    /// EDID 指纹（FNV 略化——换屏不感知即失效对账）。
    pub edid_fingerprint: u32,
    /// 枚举时刻事件总线序列号（竞态对账基准）。
    pub seq: u64,
}

/// 事件总线：热插拔事件序列号的唯一分配者（单调递增）。
#[derive(Debug)]
pub struct HpdBus {
    next_seq: u64,
}

impl HpdBus {
    /// 新总线（seq 从 1 起——0 留作"无枚举"哨兵）。
    pub fn new() -> HpdBus {
        HpdBus { next_seq: 1 }
    }

    /// 分配下一个序列号。
    pub fn take_seq(&mut self) -> u64 {
        let s = self.next_seq;
        self.next_seq += 1;
        s
    }

    /// 当前水位（枚举快照的 seq 取此值减一之前的分配序）。
    pub fn peek_next(&self) -> u64 {
        self.next_seq
    }
}

impl Default for HpdBus {
    fn default() -> Self {
        HpdBus::new()
    }
}

/// EDID 指纹（FNV-1a 32 位——判据侧独立重算同口径）。
pub const fn edid_fingerprint(bytes: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    let mut i = 0;
    while i < bytes.len() {
        h ^= bytes[i] as u32;
        h = h.wrapping_mul(0x0100_0193);
        i += 1;
    }
    h
}

/// 枚举：当前连接显示器建档（快照 seq = 分配序，逐台独立可对账）。
pub fn enumerate(displays: &[(u32, &[u8])], bus: &mut HpdBus) -> Vec<DisplaySnapshot> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < displays.len() {
        let (id, edid) = displays[i];
        out.push(DisplaySnapshot {
            display_id: id,
            edid_fingerprint: edid_fingerprint(edid),
            seq: bus.take_seq(),
        });
        i += 1;
    }
    out
}

// ---------------------------------------------------------------------------
// 二、热插拔事件与去抖规范
// ---------------------------------------------------------------------------

/// 热插拔事件类别（闭集）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HpdKind {
    /// 连接。
    Connect,
    /// 断开。
    Disconnect,
}

/// 热插拔事件（seq 由总线分配）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HpdEvent {
    /// 显示器标识。
    pub display_id: u32,
    /// 事件类别。
    pub kind: HpdKind,
    /// 总线序列号。
    pub seq: u64,
}

/// 去抖窗口（毫秒——物理抖动防护口径）。
pub const DEBOUNCE_MS: u32 = 30;

/// 去抖器（leading-edge：窗口内同屏后续事件一律吸收）。
#[derive(Debug)]
pub struct Debouncer {
    /// 各屏最近放行事件时刻（display_id → 毫秒；线性槽定容）。
    last: [Option<(u32, u32)>; MAX_TRACKED],
    used: usize,
}

/// 追踪屏数上限（多屏批量口径；超限 LRU 式覆盖最旧槽）。
pub const MAX_TRACKED: usize = 16;

impl Debouncer {
    /// 新去抖器。
    pub fn new() -> Debouncer {
        Debouncer { last: [None; MAX_TRACKED], used: 0 }
    }

    fn slot_of(&self, id: u32) -> Option<usize> {
        let mut i = 0;
        while i < self.used {
            if let Some((sid, _)) = self.last[i] {
                if sid == id {
                    return Some(i);
                }
            }
            i += 1;
        }
        None
    }

    /// 事件准入：窗口内同屏吸收（false），窗外恰端点放行并更新时刻。
    pub fn admit(&mut self, ev: &HpdEvent, now_ms: u32) -> bool {
        if let Some(idx) = self.slot_of(ev.display_id) {
            let last_ms = match self.last[idx] {
                Some((_, m)) => m,
                None => 0,
            };
            // 毫秒回绕安全：用差值非下溢（wrapping）。
            let delta = now_ms.wrapping_sub(last_ms);
            if delta < DEBOUNCE_MS {
                return false; // 窗口内吸收（含反向翻转与重复同向）
            }
            self.last[idx] = Some((ev.display_id, now_ms));
            return true;
        }
        // 新屏：放行并占槽；满则覆盖 0 号槽（定容纪律）。
        let slot = if self.used < MAX_TRACKED {
            let s = self.used;
            self.used += 1;
            s
        } else {
            0
        };
        self.last[slot] = Some((ev.display_id, now_ms));
        true
    }
}

impl Default for Debouncer {
    fn default() -> Self {
        Debouncer::new()
    }
}

// ---------------------------------------------------------------------------
// 三、竞态防护（红线：序列号一致）
// ---------------------------------------------------------------------------

/// 事件对快照的应用：seq 必须恰为快照 seq + 1（事件由总线单调分配）。
///
/// seq 落后（过期事件）或跳变（丢事件）均判竞态=缺陷：`ERR_RACE_SEQ_GAP`
/// 拒绝并立案——绝不用过期事件凑合。
pub fn apply_event(snap_seq: u64, ev: &HpdEvent) -> Result<(), VmCode> {
    if ev.seq != snap_seq + 1 {
        return Err(VmCode::RACE_SEQ_GAP);
    }
    Ok(())
}

/// 快照新鲜度对账：快照 seq 必须 ≥ 总线已分配水位前界（枚举晚于一切已见事件）。
pub fn snapshot_fresh(snap: &DisplaySnapshot, bus: &HpdBus) -> Result<(), VmCode> {
    if snap.seq == 0 || snap.seq >= bus.peek_next() {
        return Err(VmCode::SNAPSHOT_STALE);
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// 四、状态机规范（connected/enabled/mode-set 单向）
// ---------------------------------------------------------------------------

/// 显示器状态（官方三态闭集）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DisplayState {
    /// 已连接（未点亮）。
    Connected,
    /// 已使能（点亮中）。
    Enabled,
    /// 已设模式（时序落地）。
    ModeSet,
}

impl DisplayState {
    /// 全部三态（官方序）。
    pub const ALL: [DisplayState; 3] = [
        DisplayState::Connected,
        DisplayState::Enabled,
        DisplayState::ModeSet,
    ];

    /// 段下标。
    pub const fn index(self) -> usize {
        match self {
            DisplayState::Connected => 0,
            DisplayState::Enabled => 1,
            DisplayState::ModeSet => 2,
        }
    }

    /// 人话标签。
    pub fn label(self) -> &'static str {
        match self {
            DisplayState::Connected => "connected",
            DisplayState::Enabled => "enabled",
            DisplayState::ModeSet => "mode-set",
        }
    }
}

/// 状态迁移：单向链相邻一步放行；mode-set 允许原地重复（模式重设）；
/// 其余（跳段/回退/未连接就使能）显性拒绝。
pub fn transition(cur: DisplayState, target: DisplayState) -> Result<(), VmCode> {
    if target.index() == cur.index() + 1 {
        return Ok(());
    }
    if cur == DisplayState::ModeSet && target == DisplayState::ModeSet {
        return Ok(()); // 模式重设幂等
    }
    Err(VmCode::STATE_INVALID)
}

// ---------------------------------------------------------------------------
// 五、批量处理（多屏同时热插拔：固定序去重）
// ---------------------------------------------------------------------------

/// 批量规划：按 display_id 升序、同屏多事件取最后一条（确定性纪律）。
pub fn plan_batch(mut events: Vec<HpdEvent>) -> Vec<HpdEvent> {
    // 插入排序按 display_id 升序（定容多屏，n 小；无共享态）。
    let n = events.len();
    let mut i = 1;
    while i < n {
        let key = events[i];
        let mut j = i;
        while j > 0 && events[j - 1].display_id > key.display_id {
            events[j] = events[j - 1];
            j -= 1;
        }
        events[j] = key;
        i += 1;
    }
    // 同屏去重：保留最后一条（seq 最大者——总线单调保证）。
    let mut out: Vec<HpdEvent> = Vec::new();
    let mut k = 0;
    while k < n {
        let id = events[k].display_id;
        let mut last = k;
        let mut m = k + 1;
        while m < n && events[m].display_id == id {
            if events[m].seq > events[last].seq {
                last = m;
            }
            m += 1;
        }
        out.push(events[last]);
        k = m;
    }
    out
}

// ---------------------------------------------------------------------------
// 六、错误契约（0x54xx 域段；cgm01 占 0x5401~0x5407，本单 0x5408~）
// ---------------------------------------------------------------------------

impl VmCode {
    /// 竞态：事件 seq 与快照不一致（竞态=缺陷红线）。
    pub const RACE_SEQ_GAP: VmCode = VmCode(0x5408);
    /// 状态机非法迁移。
    pub const STATE_INVALID: VmCode = VmCode(0x5409);
    /// 快照过期（seq 与总线水位矛盾）。
    pub const SNAPSHOT_STALE: VmCode = VmCode(0x540A);
    /// 批量内同屏冲突（去重后仍含同屏——防御性拒绝）。
    pub const BATCH_DUP: VmCode = VmCode(0x540B);
    /// 枚举缺屏（归并时快照缺失——确定性归并前置）。
    pub const ENUM_MISSING: VmCode = VmCode(0x540C);
}

/// 本单诊断码清单（判据侧点名防漂移）。
pub const M02_CODES: [VmCode; 5] = [
    VmCode::RACE_SEQ_GAP,
    VmCode::STATE_INVALID,
    VmCode::SNAPSHOT_STALE,
    VmCode::BATCH_DUP,
    VmCode::ENUM_MISSING,
];

// ---------------------------------------------------------------------------
// 七、测试支撑（回归用例与断言）
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 去抖窗口恰端点() {
        let mut d = Debouncer::new();
        let ev_c = HpdEvent { display_id: 1, kind: HpdKind::Connect, seq: 1 };
        let ev_d = HpdEvent { display_id: 1, kind: HpdKind::Disconnect, seq: 2 };
        assert!(d.admit(&ev_c, 0));
        assert!(!d.admit(&ev_d, DEBOUNCE_MS - 1)); // 窗口内吸收
        assert!(d.admit(&ev_d, DEBOUNCE_MS)); // 恰端点放行
    }

    #[test]
    fn 竞态序列号恰一() {
        let ev = HpdEvent { display_id: 1, kind: HpdKind::Connect, seq: 5 };
        assert_eq!(apply_event(4, &ev), Ok(()));
        assert_eq!(apply_event(5, &ev), Err(VmCode::RACE_SEQ_GAP));
        assert_eq!(apply_event(3, &ev), Err(VmCode::RACE_SEQ_GAP));
    }

    #[test]
    fn 状态机单向链() {
        assert_eq!(transition(DisplayState::Connected, DisplayState::Enabled), Ok(()));
        assert_eq!(transition(DisplayState::Enabled, DisplayState::ModeSet), Ok(()));
        assert_eq!(transition(DisplayState::Connected, DisplayState::ModeSet), Err(VmCode::STATE_INVALID));
        assert_eq!(transition(DisplayState::ModeSet, DisplayState::ModeSet), Ok(()));
    }

    #[test]
    fn 批量固定序去重() {
        let evs = vec![
            HpdEvent { display_id: 7, kind: HpdKind::Connect, seq: 3 },
            HpdEvent { display_id: 2, kind: HpdKind::Connect, seq: 4 },
            HpdEvent { display_id: 7, kind: HpdKind::Disconnect, seq: 9 },
        ];
        let out = plan_batch(evs);
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].display_id, 2);
        assert_eq!(out[1].display_id, 7);
        assert_eq!(out[1].kind, HpdKind::Disconnect);
        assert_eq!(out[1].seq, 9);
    }

    #[test]
    fn 指纹独立可算() {
        let fp = edid_fingerprint(&[0x01, 0x02, 0x03]);
        assert_ne!(fp, 0);
        assert_eq!(fp, edid_fingerprint(&[0x01, 0x02, 0x03]));
        assert_ne!(fp, edid_fingerprint(&[0x01, 0x02, 0x04]));
    }
}
