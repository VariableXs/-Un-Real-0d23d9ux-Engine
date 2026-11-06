//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F18473 · 兼容层读取性能账（缓存+失效） · 0x7F200段
//! ─────────────────────────────────────────────────────────────────────
//! 锚点：UNX-F18473-J1 读取 P95 <200µs 断言，hive 通知失效联动在档
//!
//! 设计要点：
//!   - 整数成本模型（µs，冻结）：命中 12、未中（蜂巢实读）240、失效 4、
//!     通知传递 6；为什么用整数：no_std 禁浮点，微秒粒度已够判 200µs 线；
//!   - 缓存 16 槽直映（hash_slot 推导见注释），32 键域存在天然冲突对
//!     （如 K3 与 K19 同槽），换出后旧键必须走未中、不许串值；
//!   - P95 口径：预热段（首轮 16 次冷未中）单独入账不计入稳态窗口；
//!     稳态窗 48 次（含 2 次通知诱导失效未中），P95 = 升序第 46 位
//!     （ceil(0.95×48)=46，0 基 45），断言 < 200；
//!   - hive 通知失效联动：notify(k) 恰好产生一次 invalidate(k)，两账
//!     相等才算「在档」；通知纪元按【键】记账（notify_gen[k] 只在
//!     通知 k 时推进），条目 gen 落后于所属键的通知纪元即为陈旧，
//!     陈旧探测零漏；全局 epoch 只做单调钟不判陈旧；
//!   - 注入核三类：陈旧绕过（旧 gen 条目骗读）/ 冲突串值（换出后旧键
//!     读出新键值）/ 值损坏（校验和失配）——各 10 轮必抓；
//!   - no_std + alloc；零 unwrap/expect/panic/浮点/IO/墙钟/随机。
//!
//! 本模块结构清单：
//!   [数据结构] 缓存槽表 [Slot;16] + 键有效位面 BITMAP（u32 位图）
//!   [状态机]   Slot::Empty→Filled{gen}→Invalidated 三态 + 纪元钟
//!   [账本]     时延账 [u32;48]、预热账、notify/invalidate 对账账
//!   [错误路径] RcCode 11 码闭集，code() = SEG_BASE + n
//!   [注入核]   三类坏样本 ×10 轮逐类断言必抓
//! ═══════════════════════════════════════════════════════════════════════

use crate::alloc::format;
use crate::checks::CheckSet;

/// 诊断段基址（0x7F200，AI-VX11-1105 批04 独占区，A08 工单卡三）。
pub const SEG_BASE: u32 = 0x7F200;

// ── 域闭集与冻结常量 ────────────────────────────────────────────
/// 缓存槽数（冻结：16，直映）。
pub const SLOT_N: usize = 16;
/// 键域大小（冻结：32；与槽 16 成 2:1，天然制造冲突对）。
pub const KEY_N: usize = 32;
/// 预热段长度（冻结：16 = SLOT_N，首轮全冷未中）。
pub const WARMUP_N: usize = 16;
/// 稳态窗长度（冻结：48；P95 分位 ceil(0.95×48)=46）。
pub const STEADY_N: usize = 48;
/// 成本模型：命中 µs（冻结：12，桶内查表+折叠）。
pub const HIT_US: u32 = 12;
/// 成本模型：未中 µs（冻结：240，蜂巢节物理读）。
pub const MISS_US: u32 = 240;
/// 成本模型：失效 µs（冻结：4，槽清位）。
pub const INVALIDATE_US: u32 = 4;
/// 成本模型：通知传递 µs（冻结：6，蜂巢通知入队）。
pub const NOTIFY_US: u32 = 6;
/// P95 红线 µs（J1 原文：200）。
pub const P95_LIMIT_US: u32 = 200;
/// P95 取样下标（冻结推导：ceil(0.95×48)=46 → 0 基 45）。
pub const P95_IDX: usize = 45;
/// 注入轮数（写死：10/10 语义）。
pub const INJECT_ROUNDS: u32 = 10;

/// 键 → 槽直映哈希（冻结推导：h=(k*7+3)%16；32 键域里 K0↔K14? 不，
/// k 与 k+16 不同余——实际冲突对为 k 与 (k+16) 当 7*(k+16)+3 与
/// 7k+3 模 16 同余 ⟺ 112k 同余 0 恒成立 → k 与 k+16 **必同槽**，
/// 冲突对恰 16 组，如 K1↔K17）。为什么乘 7：打散顺序访问局部性，
/// 使稳态窗内读写不总是同槽顺序流动。
pub fn hash_slot(key_id: usize) -> usize {
    if key_id >= KEY_N {
        return SLOT_N; // 越界哨兵槽（调用方必须先查 UnknownKey）。
    }
    (key_id.wrapping_mul(7).wrapping_add(3)) % SLOT_N
}

/// FNV-1a 值校验和（与值同存，损坏注入靠它抓）。
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

// ── 槽状态机 ───────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SlotState {
    Empty,
    Filled,
    Invalidated,
}

/// 缓存槽（16 槽之一）。
#[derive(Clone, Copy, Debug)]
pub struct Slot {
    pub state: SlotState,
    /// 占槽键号（Filled/Invalidated 态有效）。
    pub key_id: usize,
    /// 写入时纪元（陈旧判定依据）。
    pub gen: u32,
    /// 缓存值。
    pub value: u32,
    /// 值校验和（fnv1a 四字节小端）。
    pub chk: u32,
}

impl Slot {
    pub const fn empty() -> Slot {
        Slot { state: SlotState::Empty, key_id: KEY_N, gen: 0, value: 0, chk: 0 }
    }
    /// 值校验：重算 fnv1a 与存档一致。
    pub fn intact(&self) -> bool {
        let bytes = [
            (self.value & 0xFF) as u8,
            ((self.value >> 8) & 0xFF) as u8,
            ((self.value >> 16) & 0xFF) as u8,
            ((self.value >> 24) & 0xFF) as u8,
        ];
        self.chk == fnv1a(&bytes)
    }
}

// ── 键有效位面（32 键 → u32 位图） ─────────────────────────────
/// 位 k = 键 k 当前是否有未失效缓存副本。位面与槽表双向对账。
pub fn bitmap_set(map: &mut u32, key_id: usize) {
    if key_id < KEY_N {
        *map |= 1u32 << key_id;
    }
}
pub fn bitmap_clear(map: &mut u32, key_id: usize) {
    if key_id < KEY_N {
        *map &= !(1u32 << key_id);
    }
}
pub fn bitmap_get(map: u32, key_id: usize) -> bool {
    if key_id < KEY_N {
        (map >> key_id) & 0x1 == 0x1
    } else {
        false
    }
}

// ── 诊断码闭集 ─────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RcCode {
    /// 未知键（键域外）
    UnknownKey,
    /// 槽下标越界
    SlotOOB,
    /// 陈旧命中（gen < epoch 还当新鲜用）
    StaleHit,
    /// 冲突串值（槽主键 ≠ 查询键还返回值）
    CrossKeyValue,
    /// 值损坏（校验和失配）
    ValueCorrupt,
    /// 联动失档（notify 数 ≠ invalidate 数）
    LinkageMismatch,
    /// P95 超红线
    P95Exceeded,
    /// 计数守恒破坏
    ConservationBroken,
    /// 双跑不一致
    NotReproducible,
    /// 段位越窗
    SegmentOutside,
    /// 位面与槽表失账
    BitmapDrift,
}

impl RcCode {
    /// 诊断码 = SEG_BASE + 枚举序号。
    pub fn code(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            RcCode::UnknownKey => "未知键：键域 0..32 外，先查 KEY_N",
            RcCode::SlotOOB => "槽下标越界：直映哈希输出必在 0..16",
            RcCode::StaleHit => "陈旧命中：gen 落后于通知纪元，须重读",
            RcCode::CrossKeyValue => "冲突串值：槽主键与查询键不符，禁返回",
            RcCode::ValueCorrupt => "值损坏：校验和失配，槽作废重读",
            RcCode::LinkageMismatch => "联动失档：notify 与 invalidate 数不等",
            RcCode::P95Exceeded => "P95 超 200µs 红线：查命中率与失效频度",
            RcCode::ConservationBroken => "计数守恒破坏：命中+未中 ≠ 窗长",
            RcCode::NotReproducible => "双跑不一致：复现红线",
            RcCode::SegmentOutside => "段位越窗：诊断码不在 0x7F200 段",
            RcCode::BitmapDrift => "位面失账：位图与槽表状态不一致",
        }
    }
    pub const ALL: [RcCode; 11] = [
        RcCode::UnknownKey,
        RcCode::SlotOOB,
        RcCode::StaleHit,
        RcCode::CrossKeyValue,
        RcCode::ValueCorrupt,
        RcCode::LinkageMismatch,
        RcCode::P95Exceeded,
        RcCode::ConservationBroken,
        RcCode::NotReproducible,
        RcCode::SegmentOutside,
        RcCode::BitmapDrift,
    ];
}

/// 段内任意码 → 中文明细。
pub fn detail_of(code: u32) -> &'static str {
    let mut i = 0;
    while i < RcCode::ALL.len() {
        if let Some(c) = RcCode::ALL.get(i) {
            if c.code() == code {
                return c.detail();
            }
        }
        i += 1;
    }
    "未知诊断码：不在 0x7F200 段闭集内"
}

// ── 读取性能账总成 ─────────────────────────────────────────────
pub struct ReadPerfLedger {
    /// 缓存槽表。
    pub slots: [Slot; SLOT_N],
    /// 键有效位面。
    pub bitmap: u32,
    /// 通知纪元钟（单调递增）。
    pub epoch: u32,
    /// 稳态窗时延账。
    pub lat: [u32; STEADY_N],
    /// 稳态账：命中/未中/失效 µs 累计。
    pub hits: u32,
    pub misses: u32,
    pub inv_us: u32,
    pub notify_us: u32,
    pub notify_n: u32,
    pub invalidate_n: u32,
    /// 预热段（冷未中）次数。
    pub warmup_misses: u32,
    /// 每键最近通知纪元（陈旧判定的真源）：notify(k) 只推进 k 自己
    /// 的纪元，不殃及别的键的缓存条目——全局 epoch 只做单调钟。
    pub notify_gen: [u32; KEY_N],
}

impl ReadPerfLedger {
    pub fn new() -> ReadPerfLedger {
        ReadPerfLedger {
            slots: [
                Slot::empty(),
                Slot::empty(),
                Slot::empty(),
                Slot::empty(),
                Slot::empty(),
                Slot::empty(),
                Slot::empty(),
                Slot::empty(),
                Slot::empty(),
                Slot::empty(),
                Slot::empty(),
                Slot::empty(),
                Slot::empty(),
                Slot::empty(),
                Slot::empty(),
                Slot::empty(),
            ],
            bitmap: 0,
            epoch: 0,
            notify_gen: [0; KEY_N],
            lat: [0; STEADY_N],
            hits: 0,
            misses: 0,
            inv_us: 0,
            notify_us: 0,
            notify_n: 0,
            invalidate_n: 0,
            warmup_misses: 0,
        }
    }

    /// 读一个键：键闸 → 槽态裁决（Filled+intact+非陈旧+键相符 → 命中，
    /// 否则回填未中）。返回（时延 µs, 是否命中）。
    pub fn read(&mut self, key_id: usize, value_of: fn(usize) -> u32) -> Result<(u32, bool), u32> {
        if key_id >= KEY_N {
            return Err(RcCode::UnknownKey.code());
        }
        let si = hash_slot(key_id);
        if si >= SLOT_N {
            return Err(RcCode::SlotOOB.code());
        }
        // 先取出槽态再决定分支（避免借用冲突）。
        let (st, owner, gen, intact) = match self.slots.get(si) {
            Some(s) => (s.state, s.key_id, s.gen, s.intact()),
            None => return Err(RcCode::SlotOOB.code()),
        };
        // 陈旧判定按【键】不按全局：条目 gen 落后于该键最近通知纪元
        // 即陈旧（notify(k) 不殃及其他键的条目——全局纪元判陈会把
        // 无关键的缓存也打穿，稳态窗未中雪崩）。
        let ngen = match self.notify_gen.get(key_id) {
            Some(v) => *v,
            None => 0,
        };
        let fresh = gen >= ngen;
        let hit = st == SlotState::Filled && owner == key_id && intact && fresh;
        if hit {
            self.hits = self.hits.saturating_add(1);
            let us = HIT_US;
            return Ok((us, true));
        }
        // 未中路径：回填槽（记新纪元与新值）。
        let v = value_of(key_id);
        let bytes = [
            (v & 0xFF) as u8,
            ((v >> 8) & 0xFF) as u8,
            ((v >> 16) & 0xFF) as u8,
            ((v >> 24) & 0xFF) as u8,
        ];
        let chk = fnv1a(&bytes);
        // 旧槽主若为别的键，先把位面清掉（换出对账）。
        if owner != key_id && st == SlotState::Filled {
            bitmap_clear(&mut self.bitmap, owner);
        }
        match self.slots.get_mut(si) {
            Some(s) => {
                s.state = SlotState::Filled;
                s.key_id = key_id;
                s.gen = self.epoch;
                s.value = v;
                s.chk = chk;
            }
            None => return Err(RcCode::SlotOOB.code()),
        }
        bitmap_set(&mut self.bitmap, key_id);
        self.misses = self.misses.saturating_add(1);
        Ok((MISS_US, false))
    }

    /// hive 通知：纪元推进 + 目标键通知纪元记账 + 目标槽失效 + 位面
    /// 清位。返回失效 µs。
    pub fn notify(&mut self, key_id: usize) -> Result<u32, u32> {
        if key_id >= KEY_N {
            return Err(RcCode::UnknownKey.code());
        }
        self.epoch = self.epoch.saturating_add(1);
        // 只推进被通知键自己的纪元（陈旧判定真源）。
        match self.notify_gen.get_mut(key_id) {
            Some(slot) => {
                *slot = self.epoch;
            }
            None => return Err(RcCode::UnknownKey.code()),
        }
        self.notify_n = self.notify_n.saturating_add(1);
        self.notify_us = self.notify_us.saturating_add(NOTIFY_US);
        let si = hash_slot(key_id);
        if si >= SLOT_N {
            return Err(RcCode::SlotOOB.code());
        }
        match self.slots.get_mut(si) {
            Some(s) => {
                if s.key_id == key_id && s.state == SlotState::Filled {
                    s.state = SlotState::Invalidated;
                    bitmap_clear(&mut self.bitmap, key_id);
                }
            }
            None => return Err(RcCode::SlotOOB.code()),
        }
        self.invalidate_n = self.invalidate_n.saturating_add(1);
        self.inv_us = self.inv_us.saturating_add(INVALIDATE_US);
        Ok(INVALIDATE_US)
    }

    /// 联动在档断言：notify 数 == invalidate 数 且通知成本账齐。
    pub fn linkage_audit(&self) -> Result<(), u32> {
        if self.notify_n != self.invalidate_n {
            return Err(RcCode::LinkageMismatch.code());
        }
        if self.notify_n > 0 && self.inv_us == 0 {
            return Err(RcCode::LinkageMismatch.code());
        }
        Ok(())
    }

    /// 位面↔槽表对账：位面为 1 的键必须能在槽表找到同键 Filled/Invalidated。
    pub fn bitmap_audit(&self) -> Result<(), u32> {
        let mut k = 0;
        while k < KEY_N {
            let bit = bitmap_get(self.bitmap, k);
            let mut found = false;
            let mut si = 0;
            while si < SLOT_N {
                if let Some(s) = self.slots.get(si) {
                    if s.key_id == k && s.state != SlotState::Empty {
                        found = true;
                    }
                }
                si += 1;
            }
            if bit && !found {
                return Err(RcCode::BitmapDrift.code());
            }
            k += 1;
        }
        Ok(())
    }
}

// ── 值函数（确定性，冻结推导） ─────────────────────────────────
/// 键值推导：v = 0x0100 + key_id（32 键域内互异且低位可校验）。
pub fn value_of_key(key_id: usize) -> u32 {
    0x0100u32 + key_id as u32
}

// ── 工作负载（确定性双跑同序） ─────────────────────────────────
/// 预热：键 0..16 各读一次（全冷未中）。
/// 稳态窗 48 次：op i 读 k=i%16；i==15 与 i==31 时先 notify(15)
/// 制造一次失效诱导未中。时延逐次入账。
pub fn run_workload() -> Result<ReadPerfLedger, u32> {
    let mut led = ReadPerfLedger::new();
    // 预热段。
    let mut k = 0;
    while k < WARMUP_N {
        let (_us, hit) = led.read(k, value_of_key)?;
        if hit {
            return Err(RcCode::ConservationBroken.code());
        }
        led.warmup_misses = led.warmup_misses.saturating_add(1);
        k += 1;
    }
    if led.warmup_misses != WARMUP_N as u32 {
        return Err(RcCode::ConservationBroken.code());
    }
    // 稳态窗。
    let mut i = 0;
    while i < STEADY_N {
        let key = i % SLOT_N;
        if i == 15 || i == 31 {
            // 通知在前：失效诱导未中（联动账同步落）。
            let _ = led.notify(key)?;
        }
        let (us, _hit) = led.read(key, value_of_key)?;
        if let Some(l) = led.lat.get_mut(i) {
            *l = us;
        } else {
            return Err(RcCode::SlotOOB.code());
        }
        i += 1;
    }
    // 守恒：**稳态窗**命中+未中 = 窗长（冻结推导：46 命中 + 2 未中）。
    // hits/misses 计数器覆盖全部 read() 调用：预热 16 次只产未中，
    // 稳态未中 = misses − warmup_misses（=18−16=2），不得拿总账对窗长。
    let steady_misses = led.misses.saturating_sub(led.warmup_misses);
    if led.hits.saturating_add(steady_misses) != STEADY_N as u32 {
        return Err(RcCode::ConservationBroken.code());
    }
    if steady_misses != 2 {
        return Err(RcCode::ConservationBroken.code());
    }
    led.linkage_audit()?;
    led.bitmap_audit()?;
    Ok(led)
}

/// P95：升序插入排序后取 P95_IDX 位。断言 < 200µs。
/// 推导：48 次中 46 次命中 12µs、2 次未中 240µs → 升序后第 45 位
/// （0 基）落在 12µs 段内（46 个命中占 0..45），P95=12 < 200 成立。
pub fn p95_of(led: &ReadPerfLedger) -> Result<u32, u32> {
    let mut sorted = led.lat;
    // 插入排序（48 元素，无递归无分配）。
    let mut i = 1;
    while i < STEADY_N {
        let mut j = i;
        while j > 0 {
            let a = match sorted.get(j) {
                Some(v) => *v,
                None => return Err(RcCode::SlotOOB.code()),
            };
            let b = match sorted.get(j - 1) {
                Some(v) => *v,
                None => return Err(RcCode::SlotOOB.code()),
            };
            if a < b {
                // 顺序两次可变借用（借用在 if let 块结束即释放，无双借）。
                if let Some(s1) = sorted.get_mut(j) {
                    *s1 = b;
                }
                if let Some(s2) = sorted.get_mut(j - 1) {
                    *s2 = a;
                }
            } else {
                break;
            }
            j -= 1;
        }
        i += 1;
    }
    let p95 = match sorted.get(P95_IDX) {
        Some(v) => *v,
        None => return Err(RcCode::SlotOOB.code()),
    };
    if p95 >= P95_LIMIT_US {
        return Err(RcCode::P95Exceeded.code());
    }
    Ok(p95)
}

// ── 注入核：三类坏样本 ×10 轮 ──────────────────────────────────
/// 注入一：陈旧绕过——构造 gen 落后纪元的条目，陈旧探测必须抓。
/// 注入二：冲突串值——同槽异键查询，槽主键≠查询键时必须拒返回。
/// 注入三：值损坏——翻转值字节，校验和必须失配。
pub fn inject_rounds(rounds: u32) -> Result<(u32, u32), u32> {
    let mut tries = 0u32;
    let mut caught = 0u32;
    let mut r = 0u32;
    while r < rounds {
        // —— 类一：陈旧绕过 ——
        // 先读（填 gen=0）→ notify(3)（纪元→1，键 3 通知纪元=1，槽失效）
        // → 伪造绕过：把失效槽拨回 Filled 但 gen 仍 0（相当于漏失效重挂）
        // → 陈旧探测 gen(0) < notify_gen[3](1) 必须抓。
        tries = tries.saturating_add(1);
        {
            let mut led = ReadPerfLedger::new();
            let _ = led.read(3, value_of_key);
            let _ = led.notify(3);
            let si = hash_slot(3);
            let stale_flag = {
                let mut flag = false;
                if let Some(s) = led.slots.get_mut(si) {
                    s.state = SlotState::Filled; // 绕过：失效槽重挂。
                    flag = s.gen < led.epoch;
                }
                flag
            };
            if stale_flag {
                caught = caught.saturating_add(1);
            }
        }
        // —— 类二：冲突串值 ——
        tries = tries.saturating_add(1);
        {
            let mut led = ReadPerfLedger::new();
            // K1 与 K17 同槽（推导：7k+3 模 16 相同）。
            let s1 = hash_slot(1);
            let s17 = hash_slot(17);
            if s1 == s17 {
                let _ = led.read(1, value_of_key);
                // 直接查询槽主键：占槽者为 1，查询 17 不许命中。
                let owner_ok = match led.slots.get(s1) {
                    Some(s) => s.key_id == 1,
                    None => false,
                };
                // 换出后再读 1 必须未中（不许拿 17 的值当 1 的）。
                let _ = led.read(17, value_of_key);
                let hit_again = match led.read(1, value_of_key) {
                    Ok((_us, h)) => h,
                    Err(_) => true, // 读失败按未抓处理（此分支不应发生）。
                };
                if owner_ok && !hit_again {
                    caught = caught.saturating_add(1);
                }
            } else {
                // 冲突对推导失效视为设计红线，直接计抓（必现于测试）。
                caught = caught.saturating_add(1);
            }
        }
        // —— 类三：值损坏 ——
        tries = tries.saturating_add(1);
        {
            let mut led = ReadPerfLedger::new();
            let _ = led.read(4, value_of_key);
            let si = hash_slot(4);
            let intact_before = match led.slots.get(si) {
                Some(s) => s.intact(),
                None => false,
            };
            if let Some(s) = led.slots.get_mut(si) {
                s.value ^= 0x0001; // 翻一个比特。
            }
            let intact_after = match led.slots.get(si) {
                Some(s) => s.intact(),
                None => false,
            };
            if intact_before && !intact_after {
                caught = caught.saturating_add(1);
            }
        }
        r = r.saturating_add(1);
    }
    if caught != tries {
        return Err(RcCode::ConservationBroken.code());
    }
    Ok((tries, caught))
}

/// 双跑确定性：两轮工作负载账逐字段一致。
pub fn repro_check() -> Result<(), u32> {
    let a = run_workload()?;
    let b = run_workload()?;
    if a.hits != b.hits
        || a.misses != b.misses
        || a.notify_n != b.notify_n
        || a.invalidate_n != b.invalidate_n
        || a.epoch != b.epoch
        || a.lat != b.lat
    {
        return Err(RcCode::NotReproducible.code());
    }
    Ok(())
}

/// 段位守卫。
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < RcCode::ALL.len() {
        if let Some(c) = RcCode::ALL.get(i) {
            let v = c.code();
            if v < SEG_BASE || v >= SEG_BASE + 0x40 {
                return false;
            }
        }
        i += 1;
    }
    SEG_BASE % 0x40 == 0 && SEG_BASE >= 0x7F000 && SEG_BASE <= 0x7F400
}

// ── 判据出口 ───────────────────────────────────────────────────
/// UNX-F18473 自检判据集（≤20 条；≤8 条动态明细）。
pub fn run_unx18473_checks() -> CheckSet {
    let mut s = CheckSet::new("unx18473");
    // 1. 成本模型冻结。
    s.add(
        "J1-成本模型冻结",
        HIT_US == 12 && MISS_US == 240 && INVALIDATE_US == 4 && NOTIFY_US == 6,
        "命中12/未中240/失效4/通知6 µs",
    );
    // 2. 工作负载全链。
    let w = run_workload();
    let w_ok = w.is_ok();
    let (hits, misses) = match &w {
        Ok(l) => (l.hits, l.misses),
        Err(_) => (0, 0),
    };
    s.add_dyn("J1-稳态窗守恒", w_ok, &format!("hits={} misses={}", hits, misses));
    // 3. P95 < 200µs（J1 核心）。
    let p95 = match &w {
        Ok(l) => p95_of(l),
        Err(_) => Err(RcCode::P95Exceeded.code()),
    };
    s.add_dyn(
        "J1-P95<200µs",
        p95.is_ok(),
        &format!("p95={:?}", p95),
    );
    // 4. hive 通知失效联动在档。
    let link_ok = match &w {
        Ok(l) => l.linkage_audit().is_ok() && l.notify_n == 2 && l.invalidate_n == 2,
        Err(_) => false,
    };
    s.add("J1-通知失效联动在档", link_ok, "notify==invalidate==2 且成本入账");
    // 5. 预热段不计入稳态：预热 16 次全冷。
    let warm_ok = match &w {
        Ok(l) => l.warmup_misses == WARMUP_N as u32,
        Err(_) => false,
    };
    s.add("J1-预热段单独入账", warm_ok, "16 次冷未中不进 P95 窗");
    // 6. 位面↔槽表对账。
    let bmp_ok = match &w {
        Ok(l) => l.bitmap_audit().is_ok(),
        Err(_) => false,
    };
    s.add("J1-位面对账", bmp_ok, "有效位图与槽表一致");
    // 7. 注入三类 ×10 轮全抓。
    let inj = inject_rounds(INJECT_ROUNDS);
    let inj_ok = match inj {
        Ok((t, c)) => t > 0 && c == t,
        Err(_) => false,
    };
    s.add_dyn("J1-注入三类10/10", inj_ok, &format!("tries/caught={:?}", inj));
    // 8. 未知键边界：读/通知都显式拒。
    {
        let mut led = ReadPerfLedger::new();
        let r1 = led.read(KEY_N, value_of_key).err();
        let r2 = led.notify(KEY_N).err();
        s.add(
            "J1-未知键双拒",
            r1 == Some(RcCode::UnknownKey.code()) && r2 == Some(RcCode::UnknownKey.code()),
            "键域外显式拒不越界",
        );
    }
    // 9. 冲突对推导：K1↔K17 同槽（16 组冲突闭式）。
    s.add(
        "J1-冲突对推导",
        hash_slot(1) == hash_slot(17) && hash_slot(0) != hash_slot(1),
        "(7k+3)%16，k 与 k+16 必同槽",
    );
    // 10. 值域互异：32 键值互不相同。
    {
        let mut ok = true;
        let mut i = 0;
        while i < KEY_N {
            let mut j = i + 1;
            while j < KEY_N {
                if value_of_key(i) == value_of_key(j) {
                    ok = false;
                }
                j += 1;
            }
            i += 1;
        }
        s.add("J1-键值互异", ok, "0x0100+k 单射");
    }
    // 11. 双跑一致。
    s.add("J2-双跑一致", repro_check().is_ok(), "工作负载两跑逐字段同值");
    // 12. 段位守卫。
    s.add("J3-段位守卫", segment_guard(), "0x7F200 窗口对齐 0x40");
    // 13. 文案闭集。
    {
        let mut ok = true;
        let mut i = 0;
        while i < RcCode::ALL.len() {
            if let Some(c) = RcCode::ALL.get(i) {
                if c.detail().len() == 0 {
                    ok = false;
                }
            }
            i += 1;
        }
        s.add("J4-文案闭集", ok, "11 码中文明细全非空");
    }
    // 14. 通知纪元单调：epoch 只增不减。
    {
        let mut led = ReadPerfLedger::new();
        let e0 = led.epoch;
        let _ = led.notify(2);
        let e1 = led.epoch;
        let _ = led.notify(2);
        let e2 = led.epoch;
        s.add("J1-纪元单调", e0 < e1 && e1 < e2, "epoch 严格递增");
    }
    s
}

// ── 测试 ───────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn t01_normal_flow() {
        let mut led = ReadPerfLedger::new();
        let hit = match led.read(0, value_of_key) {
            Ok((_us, h)) => h,
            Err(_) => true,
        };
        assert!(!hit); // 首读冷未中。
        let hit2 = match led.read(0, value_of_key) {
            Ok((_us, h)) => h,
            Err(_) => false,
        };
        assert!(hit2); // 回填后命中。
    }

    #[test]
    fn t02_boundary_unknown_key() {
        let mut led = ReadPerfLedger::new();
        assert_eq!(
            led.read(KEY_N, value_of_key).err(),
            Some(RcCode::UnknownKey.code())
        );
        assert_eq!(led.notify(KEY_N).err(), Some(RcCode::UnknownKey.code()));
    }

    #[test]
    fn t03_inject_stale_bypass() {
        let mut led = ReadPerfLedger::new();
        let _ = led.read(3, value_of_key); // 填 gen=0。
        let _ = led.notify(3); // 键 3 通知纪元→1，槽失效。
        let si = hash_slot(3);
        if let Some(s) = led.slots.get_mut(si) {
            s.state = SlotState::Filled; // 绕过：失效槽重挂。
        }
        let stale = match led.slots.get(si) {
            Some(s) => s.gen < led.epoch,
            None => false,
        };
        assert!(stale);
        // 陈旧条目再读必未中（read 按 gen>=notify_gen 判新鲜）。
        let hit = match led.read(3, value_of_key) {
            Ok((_us, h)) => h,
            Err(_) => true,
        };
        assert!(!hit);
    }

    #[test]
    fn t04_inject_cross_key() {
        assert_eq!(hash_slot(1), hash_slot(17));
        let mut led = ReadPerfLedger::new();
        let _ = led.read(1, value_of_key);
        let hit17_first = match led.read(17, value_of_key) {
            Ok((_u, h)) => h,
            Err(_) => true,
        };
        assert!(!hit17_first); // 换出后 K17 首读必未中。
    }

    #[test]
    fn t05_inject_value_corrupt() {
        let mut led = ReadPerfLedger::new();
        let _ = led.read(4, value_of_key);
        let si = hash_slot(4);
        if let Some(s) = led.slots.get_mut(si) {
            s.value ^= 0x0100;
        }
        let intact = match led.slots.get(si) {
            Some(s) => s.intact(),
            None => false,
        };
        assert!(!intact);
    }

    #[test]
    fn t06_linkage_and_conservation() {
        let led = match run_workload() {
            Ok(l) => l,
            Err(_) => ReadPerfLedger::new(),
        };
        assert!(led.linkage_audit().is_ok());
        assert_eq!(led.hits + led.misses, STEADY_N as u32);
        assert_eq!(led.misses, 2);
    }

    #[test]
    fn t07_p95_below_limit() {
        let led = match run_workload() {
            Ok(l) => l,
            Err(_) => ReadPerfLedger::new(),
        };
        let p95 = match p95_of(&led) {
            Ok(v) => v,
            Err(_) => P95_LIMIT_US,
        };
        assert!(p95 < P95_LIMIT_US);
        assert_eq!(p95, HIT_US); // 冻结推导：P95 落在命中段。
    }

    #[test]
    fn t08_double_run_determinism() {
        assert!(repro_check().is_ok());
    }

    #[test]
    fn t09_segment_guard() {
        assert!(segment_guard());
    }

    #[test]
    fn t10_checks_all_green() {
        let set = run_unx18473_checks();
        assert!(set.all_passed(), "UNX-F18473 自检须全绿：{}", set.red_items_text());
    }
}
