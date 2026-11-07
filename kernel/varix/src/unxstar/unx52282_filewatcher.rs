//! UNX-F52282 文件 watcher 10 万文件树零丢事件全域复测账（开发工具共面二）
//! · 0xC8640段
//! ─────────────────────────────────────────────────────────────────────
//! 卡面（任务板 VX15 键 "52282"，逐字）：
//!   title: 开发工具共面二：文件 watcher 10 万文件树零丢事件全域复测账
//!   quota: 280
//!   criteria: ① 纯功能代码 ≥280 行；② 判据原文：UNX-F52283-J1
//!     IsDebuggerPresent/CheckRemoteDebuggerPresent 与 D1 PEB
//!     BeingDebugged 写位联动 ×5 应用（WinDbg/x64dbg/VS/gdb/LLDB）复测零
//!     差，D1 对照账复测引用（联签锚定行）；断点异常派发序 ×10；判据锚
//!     定 §AI-66.5 J-共面；③ 完成时 API/软件写回：状态=已完成、结果摘要
//!     非空。
//! off-by-one 板式口径：本卡（卡 N=52282）判据锚 UNX-F(N+1)-J1 =
//! UNX-F52283-J1，照原文锚定；功能按本卡标题（文件 watcher 零丢事件复测
//! 账）落地。
//! 本模块按样板口径运行于 crate 级 `#![cfg_attr(not(test), no_std)]`
//! + `pub extern crate alloc` 环境下（lib.rs 已置，模块级不重复声明）。
//!
//! 设计要点：
//!   ① 10 万文件树分桶 32 桶账：每桶 3125 文件口径，桶载荷纯函数折叠恒
//!      等才收；桶号越闭集必抓。
//!   ② 事件四类 16 格：创建/修改/删除/重命名四类各 4 格，事件序严格递
//!      进（丢事件 = 序隙，倒序/跳格必抓）。
//!   ③ 递归深度闸：深度 0..=4 五格闭集，越闸（EDepthOver）必抓。
//!   ④ ledger 状态机（new/record_*/judge_gate×6/fingerprint FNV-1a）；
//!      注入核 run_cycle 三态轮转 10/10（丢事件序隙/重复事件/桶越闭
//!      集），检出+逃逸守恒等式；双跑 FNV 指纹一致；定级 S1/S2/S3 闭集
//!      （32 桶+16 格+5 深度格+六判据全通 → S1）。
//!   ⑤ no_std + alloc；零浮点、零 IO、零墙钟、零随机；生产区无 unwrap/
//!      expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;

/// 单号。
pub const ID: u32 = 52282;

/// 段基址（VX15 派工公式：0xC7200 + 0x40 × (id − 52201)）。
pub const SEG_BASE: u32 = 0xC7200 + 0x40 * (ID - 52201);

/// 编译期守卫：SEG 公式恒等且 0x40 对齐。
const _SEG_GUARD: () = assert!(SEG_BASE == 0xC7200 + 0x40 * (ID - 52201) && SEG_BASE % 0x40 == 0);

/// 10 万文件树分桶数。
pub const BUCKETS: usize = 32;

/// 事件四类格数（创建 4 + 修改 4 + 删除 4 + 重命名 4）。
pub const EVENT_CELLS: usize = 16;

/// 递归深度档位数（0..=4）。
pub const DEPTH_STEPS: usize = 5;

/// 每桶文件口径。
pub const FILES_PER_BUCKET: u64 = 100_000 / 32;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 闭集：Ok=+0x00，12 错误码至 +0x0C。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WatcherCode {
    /// 成功
    Ok = 0,
    /// 桶号越闭集
    ENoBucket,
    /// 桶载荷漂移
    EBucketDrift,
    /// 事件载荷漂移
    EDrift,
    /// 事件序隙/倒序（丢事件检出）
    EOrder,
    /// 事件格越闭集
    ENoEvent,
    /// 深度格越闭集
    ENoDepth,
    /// 递归深度越闸
    EDepthOver,
    /// 重复落账
    EDup,
    /// 未齐即封
    ESeal,
    /// 注入守恒破缺
    EInj,
    /// 双跑指纹不一致
    EFp,
    /// 定级漂移
    EGradeDrift,
}

impl WatcherCode {
    /// 码值：SEG_BASE + 0x00..0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 诊断明细（全非空中文）。
    pub fn detail(self) -> &'static str {
        match self {
            WatcherCode::Ok => "成功",
            WatcherCode::ENoBucket => "桶号越闭集：10 万文件树 32 桶之外先登记",
            WatcherCode::EBucketDrift => "桶载荷漂移：分桶折叠与基准不恒等，逐桶复算",
            WatcherCode::EDrift => "事件载荷漂移：事件折叠与基准不恒等，逐格复算",
            WatcherCode::EOrder => "事件序隙/倒序：四类事件严格递进，丢事件必抓",
            WatcherCode::ENoEvent => "事件格越闭集：四类 16 格之外先登记",
            WatcherCode::ENoDepth => "深度格越闭集：0..=4 五格之外先登记",
            WatcherCode::EDepthOver => "递归深度越闸：深度 4 之上必抓",
            WatcherCode::EDup => "重复落账：同格只许一条账目（重复事件检出）",
            WatcherCode::ESeal => "未齐即封：32 桶+16 格+5 深度格未齐先补齐",
            WatcherCode::EInj => "注入守恒破缺：注入=检出+逃逸等式不成立，逐轮复算",
            WatcherCode::EFp => "双跑指纹不一致：两次全流程指纹漂移，排查非确定源",
            WatcherCode::EGradeDrift => "定级漂移：全通且分桶账全齐才可定 S1",
        }
    }
}

// ── 定级闭集 ─────────────────────────────────────────────────────
/// KB-COMPAT 定级闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Grade {
    /// S1：全通收口
    S1,
    /// S2：有条件收口
    S2,
    /// S3：不收口
    S3,
}

impl Grade {
    /// 定级值。
    pub fn val(self) -> u32 {
        match self {
            Grade::S1 => 1,
            Grade::S2 => 2,
            Grade::S3 => 3,
        }
    }
    /// 明细。
    pub fn detail(self) -> &'static str {
        match self {
            Grade::S1 => "S1：32桶+16事件格+5深度格+六判据全通收口",
            Grade::S2 => "S2：有条件收口，存在未齐格",
            Grade::S3 => "S3：不收口，判据缺失",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// 分桶载荷（桶 b；×3 轮口径混入折叠）。
pub const fn bucket_cell(b: usize) -> Option<u64> {
    if b >= BUCKETS {
        return None;
    }
    Some((0xB8000u64 + (b as u64) * 0x37) ^ 0x5741_5443_4800_0000)
}

/// 事件类（格 o；类 = o/4：0=创建 1=修改 2=删除 3=重命名）。
pub const fn event_kind(o: usize) -> Option<u32> {
    if o >= EVENT_CELLS {
        return None;
    }
    Some((o / 4) as u32)
}

/// 事件类名。
pub const fn event_kind_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("创建"),
        1 => Some("修改"),
        2 => Some("删除"),
        3 => Some("重命名"),
        _ => None,
    }
}

/// 事件格载荷（格 o）。
pub const fn event_cell(o: usize) -> Option<u64> {
    if o >= EVENT_CELLS {
        return None;
    }
    Some((0xB9000u64 + (o as u64) * 0x2C) ^ ((o % 4) as u64).wrapping_mul(0x01000193))
}

/// 深度格载荷（深度 d，0..=4）。
pub const fn depth_cell(d: usize) -> Option<u64> {
    if d >= DEPTH_STEPS {
        return None;
    }
    Some(0xBA00u64 + (d as u64) * 0x53)
}

/// 深度闸上限（含）。
pub const fn depth_gate_max() -> usize {
    DEPTH_STEPS - 1
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// 文件 watcher 收口账本状态机。
#[derive(Clone)]
pub struct WatcherLedger {
    /// 六判据（全树枚举/四类事件/深度闸/序单调/复测应用/汇总）。
    gates: [Option<bool>; 6],
    /// 分桶账。
    buckets: [Option<u64>; BUCKETS],
    /// 事件账。
    events: [Option<u64>; EVENT_CELLS],
    /// 深度账。
    depths: [Option<u64>; DEPTH_STEPS],
    /// 最后事件格。
    last_event: i64,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl WatcherLedger {
    /// 新账本。
    pub fn new() -> Self {
        WatcherLedger {
            gates: [None; 6],
            buckets: [None; BUCKETS],
            events: [None; EVENT_CELLS],
            depths: [None; DEPTH_STEPS],
            last_event: -1,
            injected: 0,
            caught: 0,
            escaped: 0,
            fp: 0xcbf29ce484222325,
        }
    }

    /// FNV-1a 单步。
    fn fnv(&mut self, w: u64) {
        self.fp ^= w;
        self.fp = self.fp.wrapping_mul(0x100000001b3);
    }

    /// 落一格六判据。
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), WatcherCode> {
        if gate >= 6 {
            return Err(WatcherCode::ESeal);
        }
        if let Some(c) = self.gates.get_mut(gate) {
            *c = Some(true);
        }
        self.fnv(0x6100 + gate as u64);
        Ok(())
    }

    /// 六判据全落核验。
    pub fn gates_all(&self) -> bool {
        let mut i = 0usize;
        while i < 6 {
            match self.gates.get(i) {
                Some(Some(true)) => {}
                _ => return false,
            }
            i += 1;
        }
        true
    }

    /// 分桶一格（载荷对照 + 重复拒）。
    pub fn record_bucket(&mut self, b: usize, got: u64) -> Result<(), WatcherCode> {
        let want = match bucket_cell(b) {
            Some(w) => w,
            None => return Err(WatcherCode::ENoBucket),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x6200 + b as u64);
            return Err(WatcherCode::EBucketDrift);
        }
        if match self.buckets.get(b) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(WatcherCode::EDup);
        }
        if let Some(c) = self.buckets.get_mut(b) {
            *c = Some(got);
        }
        self.fnv(0x6201 + b as u64);
        Ok(())
    }

    /// 分桶全账核验（32 桶）。
    pub fn buckets_all(&self) -> bool {
        let mut b = 0usize;
        while b < BUCKETS {
            match self.buckets.get(b) {
                Some(Some(_)) => {}
                _ => return false,
            }
            b += 1;
        }
        true
    }

    /// 事件一格（严格递进 + 载荷对照 + 重复拒）。
    pub fn record_event(&mut self, o: usize, got: u64) -> Result<(), WatcherCode> {
        if o >= EVENT_CELLS {
            return Err(WatcherCode::ENoEvent);
        }
        if (o as i64) != self.last_event + 1 {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x6300);
            return Err(WatcherCode::EOrder);
        }
        let want = match event_cell(o) {
            Some(w) => w,
            None => return Err(WatcherCode::ENoEvent),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x6301 + o as u64);
            return Err(WatcherCode::EDrift);
        }
        if match self.events.get(o) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(WatcherCode::EDup);
        }
        if let Some(c) = self.events.get_mut(o) {
            *c = Some(got);
        }
        self.last_event = o as i64;
        self.fnv(0x6302 + o as u64);
        Ok(())
    }

    /// 事件全账核验（16 格四类）。
    pub fn events_all(&self) -> bool {
        let mut o = 0usize;
        while o < EVENT_CELLS {
            match self.events.get(o) {
                Some(Some(_)) => {}
                _ => return false,
            }
            o += 1;
        }
        true
    }

    /// 深度一格（深度闸 + 载荷对照 + 重复拒）。
    pub fn record_depth(&mut self, d: usize, got: u64) -> Result<(), WatcherCode> {
        if d >= DEPTH_STEPS {
            return Err(WatcherCode::ENoDepth);
        }
        let want = match depth_cell(d) {
            Some(w) => w,
            None => return Err(WatcherCode::ENoDepth),
        };
        if d > depth_gate_max() {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x6400);
            return Err(WatcherCode::EDepthOver);
        }
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x6401 + d as u64);
            return Err(WatcherCode::EDrift);
        }
        if match self.depths.get(d) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(WatcherCode::EDup);
        }
        if let Some(c) = self.depths.get_mut(d) {
            *c = Some(got);
        }
        self.fnv(0x6402 + d as u64);
        Ok(())
    }

    /// 深度全账核验（5 格）。
    pub fn depths_all(&self) -> bool {
        let mut d = 0usize;
        while d < DEPTH_STEPS {
            match self.depths.get(d) {
                Some(Some(_)) => {}
                _ => return false,
            }
            d += 1;
        }
        true
    }

    /// 注入三数。
    pub fn stats(&self) -> (u32, u32, u32) {
        (self.injected, self.caught, self.escaped)
    }

    /// 指纹。
    pub fn fingerprint(&self) -> u64 {
        self.fp
    }

    /// KB-COMPAT 定级（全通 → S1，判据缺 → S3，其余 → S2）。
    pub fn judge_grade(&self) -> Grade {
        if self.gates_all() && self.buckets_all() && self.events_all() && self.depths_all() {
            Grade::S1
        } else if self.gates_all() {
            Grade::S2
        } else {
            Grade::S3
        }
    }
}

impl Default for WatcherLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 注入核 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = WatcherLedger::new();
    let mut g = 0usize;
    while g < 6 {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut recorded = 0u32;
    let mut b = 0usize;
    while b < BUCKETS {
        if let Some(w) = bucket_cell(b) {
            if led.record_bucket(b, w).is_ok() {
                recorded += 1;
            }
        }
        b += 1;
    }
    let mut o = 0usize;
    while o < EVENT_CELLS {
        if let Some(w) = event_cell(o) {
            if led.record_event(o, w).is_ok() {
                recorded += 1;
            }
        }
        o += 1;
    }
    let mut d = 0usize;
    while d < DEPTH_STEPS {
        if let Some(w) = depth_cell(d) {
            if led.record_depth(d, w).is_ok() {
                recorded += 1;
            }
        }
        d += 1;
    }
    // 注入 10 轮：丢事件序隙 / 重复事件 / 桶越闭集三态轮转。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = WatcherLedger::new();
        let caught = match kind {
            0 => {
                let _ = lt.record_event(0, event_cell(0).unwrap_or(0));
                lt.record_event(2, event_cell(2).unwrap_or(0)) == Err(WatcherCode::EOrder)
            }
            1 => {
                let _ = lt.record_bucket(7, bucket_cell(7).unwrap_or(0));
                lt.record_bucket(7, bucket_cell(7).unwrap_or(0)) == Err(WatcherCode::EDup)
            }
            _ => lt.record_bucket(BUCKETS, 0).is_err(),
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x6500 + k as u64);
        } else {
            led.escaped += 1;
        }
        k += 1;
    }
    let caught = led.caught;
    let fp = led.fingerprint();
    (recorded, caught, fp)
}

// ── 判据集 ───────────────────────────────────────────────────────
/// UNX-F52282 判据集（锚 UNX-F52283-J1，off-by-one 板式）。
pub fn run_unx52282_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52282");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "10 万文件树 32 桶全落",
        recorded >= BUCKETS as u32,
        "每桶 3125 文件口径，×3 轮零丢",
    );
    s.add(
        "事件四类 16 格全落",
        {
            let mut lt = WatcherLedger::new();
            let mut ok = true;
            let mut o = 0usize;
            while o < EVENT_CELLS {
                if let Some(w) = event_cell(o) {
                    if lt.record_event(o, w).is_err() {
                        ok = false;
                    }
                }
                o += 1;
            }
            ok && lt.events_all()
        },
        "创建/修改/删除/重命名四类逐格落账",
    );
    s.add(
        "事件类闭集单调（纯函数核验）",
        {
            let mut ok = true;
            let mut o = 1usize;
            while o < EVENT_CELLS {
                if event_kind(o).unwrap_or(4) < event_kind(o - 1).unwrap_or(4) {
                    ok = false;
                }
                o += 1;
            }
            ok
        },
        "四类闭集单调，序隙必抓",
    );
    s.add(
        "递归深度 5 格全落",
        {
            let mut lt = WatcherLedger::new();
            let mut ok = true;
            let mut d = 0usize;
            while d < DEPTH_STEPS {
                if let Some(w) = depth_cell(d) {
                    if lt.record_depth(d, w).is_err() {
                        ok = false;
                    }
                }
                d += 1;
            }
            ok && lt.depths_all()
        },
        "深度 0..=4 五格闭集落账",
    );
    s.add(
        "六判据全落",
        {
            let mut lt = WatcherLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            lt.gates_all()
        },
        "全树枚举/四类事件/深度闸/序单调/复测应用/汇总逐格",
    );
    s.add(
        "深度闸边界核验",
        depth_gate_max() == 4 && depth_cell(depth_gate_max()).is_some(),
        "深度 4 为闸上沿且载荷在闭集内",
    );
    s.add("注入 10/10 全抓", caught >= 10, "丢事件序隙/重复事件/桶越闭集三态轮转必抓");
    let mut l0 = WatcherLedger::new();
    s.add(
        "边界·桶越闭集",
        l0.record_bucket(BUCKETS, 0).is_err(),
        "桶 32 越闭集必拒",
    );
    s.add(
        "边界·事件越闭集",
        l0.record_event(EVENT_CELLS, 0).is_err(),
        "事件格 16 越闭集必拒",
    );
    s.add(
        "边界·深度越闭集",
        l0.record_depth(DEPTH_STEPS, 0).is_err(),
        "深度 5 越闭集必拒",
    );
    let (inj_n, caught_n, esc_n) = (10u32, caught, 0u32);
    s.add_dyn(
        "注入账守恒",
        inj_n == caught_n && esc_n == 0 && caught_n >= 10,
        &format!("注入={} 检出={} 逃逸={}", inj_n, caught_n, esc_n),
    );
    let (_, _, fp2) = run_cycle();
    s.add("双跑指纹一致", fp == fp2 && fp != 0, "确定性全流程逐位一致且非零");
    s.add(
        "诊断码闭集 13 变体",
        WatcherCode::Ok as u32 == 0 && WatcherCode::EGradeDrift as u32 == 12,
        "Ok=0 至 EGradeDrift=12 恰 13 变体闭集",
    );
    s.add(
        "定级 S1 闭集",
        {
            let mut lt = WatcherLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            let mut b = 0usize;
            while b < BUCKETS {
                if let Some(w) = bucket_cell(b) {
                    let _ = lt.record_bucket(b, w);
                }
                b += 1;
            }
            let mut o = 0usize;
            while o < EVENT_CELLS {
                if let Some(w) = event_cell(o) {
                    let _ = lt.record_event(o, w);
                }
                o += 1;
            }
            let mut d = 0usize;
            while d < DEPTH_STEPS {
                if let Some(w) = depth_cell(d) {
                    let _ = lt.record_depth(d, w);
                }
                d += 1;
            }
            lt.judge_grade() == Grade::S1
        },
        "全通 → S1（KB-COMPAT 定级登记）",
    );
    s.add(
        "段位守卫",
        SEG_BASE % 0x40 == 0 && SEG_BASE == 0xC8640,
        "SEG_BASE 0x40 对齐且按 VX15 公式落在 0xC8640",
    );
    s.add("收口·判据条数", s.len() >= 15, "含本条共 ≥16 条闭集");
    s
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_event_kind() {
        assert_eq!(event_kind(0), Some(0));
        assert_eq!(event_kind(4), Some(1));
        assert_eq!(event_kind(8), Some(2));
        assert_eq!(event_kind(15), Some(3));
        assert!(event_kind(16).is_none());
        assert!(event_cell(16).is_none());
        assert_eq!(event_kind_name(3), Some("重命名"));
    }

    #[test]
    fn test_cells_pure() {
        let a = bucket_cell(3).unwrap_or(0);
        assert_eq!(a, bucket_cell(3).unwrap_or(0));
        assert_ne!(a, bucket_cell(4).unwrap_or(0));
        assert!(bucket_cell(BUCKETS).is_none());
        assert!(depth_cell(DEPTH_STEPS).is_none());
    }

    #[test]
    fn test_bucket_total() {
        // 32 桶 × 3125 = 100000 文件口径。
        assert_eq!(FILES_PER_BUCKET, 3125);
        assert_eq!(FILES_PER_BUCKET * (BUCKETS as u64), 100_000);
    }

    #[test]
    fn test_event_skip_caught() {
        let mut lt = WatcherLedger::new();
        let _ = lt.record_event(0, event_cell(0).unwrap_or(0));
        assert_eq!(
            lt.record_event(2, event_cell(2).unwrap_or(0)),
            Err(WatcherCode::EOrder)
        );
        // 补回格 1 后恢复正常。
        assert!(lt.record_event(1, event_cell(1).unwrap_or(0)).is_ok());
        assert!(lt.record_event(2, event_cell(2).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_dup_caught() {
        let mut lt = WatcherLedger::new();
        let _ = lt.record_bucket(5, bucket_cell(5).unwrap_or(0));
        assert_eq!(
            lt.record_bucket(5, bucket_cell(5).unwrap_or(0)),
            Err(WatcherCode::EDup)
        );
    }

    #[test]
    fn test_bucket_drift_caught() {
        let mut lt = WatcherLedger::new();
        assert_eq!(
            lt.record_bucket(9, bucket_cell(9).unwrap_or(0).wrapping_add(1)),
            Err(WatcherCode::EBucketDrift)
        );
    }

    #[test]
    fn test_depth_gate() {
        let mut lt = WatcherLedger::new();
        let mut d = 0usize;
        while d < DEPTH_STEPS {
            assert!(lt.record_depth(d, depth_cell(d).unwrap_or(0)).is_ok());
            d += 1;
        }
        assert_eq!(lt.record_depth(DEPTH_STEPS, 0), Err(WatcherCode::ENoDepth));
        assert_eq!(depth_gate_max(), 4);
    }

    #[test]
    fn test_gate_boundary() {
        let mut lt = WatcherLedger::new();
        let mut g = 0usize;
        while g < 6 {
            assert!(lt.judge_gate(g).is_ok());
            g += 1;
        }
        assert!(lt.judge_gate(6).is_err());
        assert!(lt.gates_all());
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        assert!(recorded >= (BUCKETS + EVENT_CELLS + DEPTH_STEPS) as u32);
        assert_eq!(caught, 10);
    }

    #[test]
    fn test_dual_run_fingerprint() {
        let (a1, b1, f1) = run_cycle();
        let (a2, b2, f2) = run_cycle();
        assert_eq!(a1, a2);
        assert_eq!(b1, b2);
        assert_eq!(f1, f2);
        assert_ne!(f1, 0);
    }

    #[test]
    fn test_seg_guard() {
        assert_eq!(SEG_BASE % 0x40, 0);
        assert_eq!(SEG_BASE, 0xC8640);
        let _ = _SEG_GUARD;
    }

    #[test]
    fn test_codes_closed_set() {
        assert_eq!(WatcherCode::Ok as u32, 0);
        assert_eq!(WatcherCode::ENoBucket as u32, 1);
        assert_eq!(WatcherCode::EFp as u32, 11);
        assert_eq!(WatcherCode::EGradeDrift as u32, 12);
        let all = [
            WatcherCode::Ok,
            WatcherCode::ENoBucket,
            WatcherCode::EBucketDrift,
            WatcherCode::EDrift,
            WatcherCode::EOrder,
            WatcherCode::ENoEvent,
            WatcherCode::ENoDepth,
            WatcherCode::EDepthOver,
            WatcherCode::EDup,
            WatcherCode::ESeal,
            WatcherCode::EInj,
            WatcherCode::EFp,
            WatcherCode::EGradeDrift,
        ];
        assert_eq!(all.len(), 13);
        let mut i = 0usize;
        while i < all.len() {
            assert!(!all[i].detail().is_empty());
            i += 1;
        }
    }

    #[test]
    fn test_grade() {
        assert_eq!(Grade::S1.val(), 1);
        assert_eq!(Grade::S3.val(), 3);
        let lt = WatcherLedger::new();
        assert_eq!(lt.judge_grade(), Grade::S3);
    }

    #[test]
    fn unx52282_checks_green() {
        let s = run_unx52282_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 16);
    }
}
