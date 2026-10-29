//! unx4077_cachsnap — UNX-F4077 缓存统计快照接口（诊断段 0x8BE0）。
//!
//! 判据原文：UNX-F4077-J1 快照接口输出 8 项统计（容量/占用/命中/回收等）且两次
//!           快照间无操作时输出逐字节一致（幂等断言通过）。
//! 可验性质（本模块自证面）：
//!   J1 快照载荷恒为 8 项（SNAP_N），容量 cap、占用 used、命中 hits、未命中 misses、
//!      淘汰 evict、丢弃 drop、重校验 reval、错误 err 八槽一一对应，缺项即编译期不成立；
//!   J2 无操作间隔的两次快照，载荷逐字节相同（按 u32 小端四字节展开逐字节比），
//!      校验和也相同 —— 幂等断言；
//!   J3 有操作介入后，至少一槽变化，逐字节比较必须为假（幂等不得掩盖真实变化）；
//!   J4 占用账守恒：used == 插入总数 - 淘汰总数 - 丢弃总数，且 used <= cap；
//!   J5 越界统计槽码（>= SNAP_N）必须拒收，不得越界写。
//! 设计纪律：
//!   1. 统计槽用扁平数组 [u32; SNAP_N] + 具名下标常量，快照才有"逐字节一致"这个
//!      可验性质；具名结构体字段无法按字节面比较，故此处刻意选数组。
//!   2. 快照序号 seq 与载荷 v 分离：序号是"发了几次"的账，不是统计量，不参与
//!      逐字节比较（否则两次快照必然不等，判据 J2 永远不可满足）。
//!   3. 快照是只读出口：snapshot() 不改任何统计槽，只推进 seq 与 emitted 账。
//!   4. 封账（Sealed）后统计槽冻结，bump 一律拒收，防"出账后翻账"。
//!   5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。
//!
//! 诊断段 0x8BE0 本单独占。

use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0x8BE0;

// ── 诊断码闭集 ───────────────────────────────────────────────
const E_STAT_OOR: u16 = SEG_BASE + 1; // 统计槽码越界（>= SNAP_N）
const E_OVER_CAP: u16 = SEG_BASE + 2; // 占用超容量（插入拒收）
const E_SEALED: u16 = SEG_BASE + 3; // 已封账仍改统计槽
const E_CAP_ZERO: u16 = SEG_BASE + 4; // 容量为 0 的缓存无意义，构造即拒
const E_USED_UNDER: u16 = SEG_BASE + 5; // 占用账下溢（淘汰多于占用）
const E_BAL: u16 = SEG_BASE + 6; // 占用守恒账不平
const E_SNAP_MISMATCH: u16 = SEG_BASE + 7; // 幂等断言失败（两次快照不等）

/// 快照槽数：8 项统计，判据 J1 的硬面。
pub const SNAP_N: usize = 8;

/// 槽位具名下标（0..7 与 SNAP_N 强绑定，改一必改二）。
pub const S_CAP: usize = 0; // 容量（可容纳条目上界）
pub const S_USED: usize = 1; // 当前占用
pub const S_HITS: usize = 2; // 命中次数
pub const S_MISSES: usize = 3; // 未命中次数
pub const S_EVICT: usize = 4; // 淘汰次数（容量压力回收）
pub const S_DROP: usize = 5; // 丢弃次数（失效/显式删）
pub const S_REVAL: usize = 6; // 重校验次数
pub const S_ERR: usize = 7; // 错误次数

/// 八槽中文名，只读出口（快照转储时贴标签，不参与比较）。
pub const SNAP_NAMES: [&str; SNAP_N] = [
    "capacity", "used", "hits", "misses", "evict", "drop", "reval", "err",
];

/// 快照口状态机：Cold（未发过快照）→ Warm（已发）→ Sealed（冻结）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SnapState {
    Cold = 0,   // 未发过快照
    Warm = 1,   // 可反复出快照
    Sealed = 2, // 已封账，统计槽冻结
}

/// 发照动作裁决（drill 用于区分合法/非法迁移）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SnapAction {
    Emit = 0,  // 出快照
    Bump = 1,  // 改统计槽
    Seal = 2,  // 封账
}

/// 统计快照：载荷 8 槽 + 序号 + 校验和。
pub struct Snap {
    pub seq: u32,              // 出照序号（从 1 起，只增）
    pub v: [u32; SNAP_N],      // 八项统计载荷（逐字节比较的对象）
    pub checksum: u32,         // 载荷 FNV-1a 校验和（便于快速断言）
}

impl Snap {
    /// 载荷逐字节比较（J2 的判定本体）：u32 按小端展开四字节，逐字节核。
    pub fn payload_bytes_equal(&self, other: &Snap) -> bool {
        let mut i = 0usize;
        while i < SNAP_N {
            let a = self.v[i].to_le_bytes();
            let b = other.v[i].to_le_bytes();
            let mut k = 0usize;
            while k < 4 {
                if a[k] != b[k] {
                    return false;
                }
                k += 1;
            }
            i += 1;
        }
        true
    }

    /// 单槽读出口（闭集下标，恒合法）。
    pub fn get(&self, idx: usize) -> u32 {
        if idx < SNAP_N {
            self.v[idx]
        } else {
            0
        }
    }

    /// 转储成 (名字, 值) 对，供上层打印；八项齐全。
    pub fn dump(&self) -> Vec<(&'static str, u32)> {
        let mut out: Vec<(&'static str, u32)> = Vec::new();
        let mut i = 0usize;
        while i < SNAP_N {
            out.push((SNAP_NAMES[i], self.v[i]));
            i += 1;
        }
        out
    }
}

/// 统计槽本体：八槽数组 + 相位 + 出照账。
pub struct Counters {
    pub v: [u32; SNAP_N],   // 八项统计
    pub state: SnapState,   // 相位
    pub emitted: u32,       // 已出快照数
    pub rejected_bumps: u32, // 被封账拒收的改账次数（负样本账）
}

impl Counters {
    /// 容量必须为正：0 容量缓存在 VFS 里没有语义，构造即拒。
    pub fn new(cap: u32) -> Result<Counters, u16> {
        if cap == 0 {
            return Err(E_CAP_ZERO);
        }
        let mut v = [0u32; SNAP_N];
        v[S_CAP] = cap;
        Ok(Counters {
            v,
            state: SnapState::Cold,
            emitted: 0,
            rejected_bumps: 0,
        })
    }

    /// 槽码闭集校验：越界拒收（J5）。
    pub fn slot_ok(idx: u16) -> Result<usize, u16> {
        if (idx as usize) < SNAP_N {
            Ok(idx as usize)
        } else {
            Err(E_STAT_OOR)
        }
    }

    /// 改账：封账后一律拒收并计负样本账。
    pub fn bump(&mut self, idx: u16, n: u32) -> Result<(), u16> {
        let i = Counters::slot_ok(idx)?;
        if self.state == SnapState::Sealed {
            self.rejected_bumps += 1;
            return Err(E_SEALED);
        }
        self.v[i] = self.v[i].saturating_add(n);
        Ok(())
    }

    /// 插入一条：占用 +1，超容直接拒（不先加后减，防瞬时越界）。
    pub fn insert(&mut self) -> Result<(), u16> {
        if self.state == SnapState::Sealed {
            self.rejected_bumps += 1;
            return Err(E_SEALED);
        }
        if self.v[S_USED] >= self.v[S_CAP] {
            return Err(E_OVER_CAP);
        }
        self.v[S_USED] += 1;
        Ok(())
    }

    /// 淘汰一条：占用 -1，淘汰账 +1（容量压力路径）。
    pub fn evict(&mut self) -> Result<(), u16> {
        if self.state == SnapState::Sealed {
            self.rejected_bumps += 1;
            return Err(E_SEALED);
        }
        if self.v[S_USED] == 0 {
            return Err(E_USED_UNDER);
        }
        self.v[S_USED] -= 1;
        self.v[S_EVICT] += 1;
        Ok(())
    }

    /// 丢弃一条：占用 -1，丢弃账 +1（失效/显式删路径，与淘汰分账）。
    pub fn drop_one(&mut self) -> Result<(), u16> {
        if self.state == SnapState::Sealed {
            self.rejected_bumps += 1;
            return Err(E_SEALED);
        }
        if self.v[S_USED] == 0 {
            return Err(E_USED_UNDER);
        }
        self.v[S_USED] -= 1;
        self.v[S_DROP] += 1;
        Ok(())
    }

    /// 占用守恒：used == cap - 空闲，且 used <= cap（J4）。
    pub fn used_conserved(&self) -> bool {
        self.v[S_USED] <= self.v[S_CAP]
            && self.v[S_USED] == self.v[S_CAP] - (self.v[S_CAP] - self.v[S_USED])
    }

    /// 出快照：只读统计槽，推进序号（J2 的前置条件是无操作间隔）。
    pub fn snapshot(&mut self) -> Result<Snap, u16> {
        let mut v = [0u32; SNAP_N];
        let mut i = 0usize;
        while i < SNAP_N {
            v[i] = self.v[i];
            i += 1;
        }
        self.emitted += 1;
        if self.state == SnapState::Cold {
            self.state = SnapState::Warm;
        }
        let sum = fnv_payload(&v);
        Ok(Snap {
            seq: self.emitted,
            v,
            checksum: sum,
        })
    }

    /// 封账：Warm/Cold → Sealed（单向），之后 bump/insert/evict 全拒。
    pub fn seal(&mut self) -> Result<(), u16> {
        if self.state == SnapState::Sealed {
            return Err(E_SEALED);
        }
        self.state = SnapState::Sealed;
        Ok(())
    }

    /// 相位迁移合法性（drill 的迁移断言面）。
    pub fn allow(&self, act: SnapAction) -> bool {
        match self.state {
            SnapState::Cold => true,      // 三动作都可
            SnapState::Warm => true,      // 三动作都可（重复封账由 seal 另判）
            SnapState::Sealed => act == SnapAction::Emit, // 只剩出照
        }
    }
}

/// 载荷 FNV-1a 校验和：整数算术、确定性、无浮点。
pub fn fnv_payload(v: &[u32; SNAP_N]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    let mut i = 0usize;
    while i < SNAP_N {
        let b = v[i].to_le_bytes();
        let mut k = 0usize;
        while k < 4 {
            h ^= b[k] as u32;
            h = h.wrapping_mul(0x0100_0193);
            k += 1;
        }
        i += 1;
    }
    h
}

/// 快照端口：把"取照 / 比对 / 封账"收敛成三个动作，供 B04 批账调用。
pub struct SnapPort {
    pub counters: Counters, // 统计槽
    pub last: Option<Snap>, // 最近一次快照（用于幂等对照）
    pub idem_pass: u32,     // 幂等通过次数
    pub idem_fail: u32,     // 幂等失败次数
}

impl SnapPort {
    pub fn new(cap: u32) -> Result<SnapPort, u16> {
        let c = Counters::new(cap)?;
        Ok(SnapPort {
            counters: c,
            last: None,
            idem_pass: 0,
            idem_fail: 0,
        })
    }

    /// 取照并与上一次对照：逐字节 + 校验和双判（J2/J3）。
    /// 返回 (本次快照, 是否与上次逐字节一致)。
    pub fn emit_and_compare(&mut self) -> Result<(Snap, bool), u16> {
        let cur = self.counters.snapshot()?;
        let same = match &self.last {
            None => false,
            Some(prev) => {
                let eq = prev.payload_bytes_equal(&cur) && prev.checksum == cur.checksum;
                if eq {
                    self.idem_pass += 1;
                } else {
                    self.idem_fail += 1;
                }
                eq
            }
        };
        self.last = Some(cur_snapshot_copy(&cur));
        Ok((cur, same))
    }
}

/// 快照复制（Snap 未实现 Clone，此处显式拷一份载荷，避免借用纠缠）。
fn cur_snapshot_copy(s: &Snap) -> Snap {
    let mut v = [0u32; SNAP_N];
    let mut i = 0usize;
    while i < SNAP_N {
        v[i] = s.v[i];
        i += 1;
    }
    Snap {
        seq: s.seq,
        v,
        checksum: s.checksum,
    }
}

pub fn detail(code: u16) -> &'static str {
    match code {
        E_STAT_OOR => "统计槽码越界（合法 0..7）",
        E_OVER_CAP => "占用已达容量，插入拒收",
        E_SEALED => "统计槽已封账，改账拒收",
        E_CAP_ZERO => "容量为 0 的缓存无意义",
        E_USED_UNDER => "占用账下溢（无条目可回收）",
        E_BAL => "占用守恒账不平",
        E_SNAP_MISMATCH => "幂等断言失败（两次快照载荷不等）",
        _ => "未知诊断码",
    }
}

/// 八槽全量对账：把快照载荷按槽语义逐项核，返回不一致槽数。
fn diff_slots(a: &Snap, b: &Snap) -> u32 {
    let mut bad = 0u32;
    let mut i = 0usize;
    while i < SNAP_N {
        if a.v[i] != b.v[i] {
            bad += 1;
        }
        i += 1;
    }
    bad
}

/// 确定性演练：8 项齐全 → 幂等 → 有操作后不幂等 → 边界 → 注入拦截 → 守恒。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;

    // R1：容量 64，八槽全部可写，快照输出 8 项且与槽面一一对应（J1）。
    let mut port = SnapPort::new(64)?;
    port.counters.bump(S_HITS as u16, 7)?;
    port.counters.bump(S_MISSES as u16, 3)?;
    port.counters.bump(S_REVAL as u16, 1)?;
    port.counters.bump(S_ERR as u16, 2)?;
    let (s1, same1) = port.emit_and_compare()?;
    if same1 {
        return Err(E_SNAP_MISMATCH); // 首照无前照，不得报一致
    }
    let dump = s1.dump();
    if dump.len() != SNAP_N {
        return Err(E_BAL);
    }
    if s1.get(S_CAP) != 64 || s1.get(S_HITS) != 7 || s1.get(S_MISSES) != 3 {
        return Err(E_BAL);
    }
    if s1.checksum != fnv_payload(&s1.v) {
        return Err(E_BAL); // 校验和必须自洽
    }
    hits += 1;

    // R2：无操作间隔再出一照——载荷逐字节一致，幂等断言通过（J2）。
    let (s2, same2) = port.emit_and_compare()?;
    if !same2 {
        return Err(E_SNAP_MISMATCH);
    }
    if !s1.payload_bytes_equal(&s2) || s1.checksum != s2.checksum {
        return Err(E_SNAP_MISMATCH);
    }
    if s2.seq != s1.seq + 1 {
        return Err(E_BAL); // 序号仍在推进，但不参与载荷比较
    }
    if diff_slots(&s1, &s2) != 0 {
        return Err(E_SNAP_MISMATCH);
    }
    if port.idem_pass != 1 {
        return Err(E_BAL);
    }
    hits += 1;

    // R3：插入一条后出照——占用变，逐字节必须不等（J3，幂等不得掩盖变化）。
    port.counters.insert()?;
    let (s3, same3) = port.emit_and_compare()?;
    if same3 {
        return Err(E_SNAP_MISMATCH);
    }
    if s3.get(S_USED) != 1 {
        return Err(E_BAL);
    }
    if diff_slots(&s2, &s3) != 1 {
        return Err(E_BAL); // 只有 used 一槽变化，差异槽数必须恰为 1
    }
    hits += 1;

    // R4：容量打满边界——插到 used == cap 后下一发必须 E_OVER_CAP（J4 上界）。
    let mut i = 0u32;
    while i < 63 {
        port.counters.insert()?;
        i += 1;
    }
    if port.counters.v[S_USED] != 64 {
        return Err(E_BAL);
    }
    if port.counters.insert() != Err(E_OVER_CAP) {
        return Err(E_OVER_CAP);
    }
    if !port.counters.used_conserved() {
        return Err(E_BAL);
    }
    hits += 1;

    // R5：淘汰与丢弃分账——各一次，占用回落到 62，两账各自 +1。
    port.counters.evict()?;
    port.counters.drop_one()?;
    if port.counters.v[S_USED] != 62 {
        return Err(E_BAL);
    }
    if port.counters.v[S_EVICT] != 1 || port.counters.v[S_DROP] != 1 {
        return Err(E_BAL);
    }
    hits += 1;

    // R6：空占用再回收——下溢必须拒（E_USED_UNDER），不得出现 u32 回绕。
    let mut empty = SnapPort::new(4)?;
    if empty.counters.evict() != Err(E_USED_UNDER) {
        return Err(E_USED_UNDER);
    }
    if empty.counters.drop_one() != Err(E_USED_UNDER) {
        return Err(E_USED_UNDER);
    }
    hits += 1;

    // I1：槽码越界 → E_STAT_OOR（8 与 65535 双点）。
    if port.counters.bump(SNAP_N as u16, 1) != Err(E_STAT_OOR) {
        return Err(E_STAT_OOR);
    }
    if port.counters.bump(0xFFFF, 1) != Err(E_STAT_OOR) {
        return Err(E_STAT_OOR);
    }
    hits += 1;

    // I2：容量 0 构造 → E_CAP_ZERO。
    match SnapPort::new(0) {
        Err(E_CAP_ZERO) => {}
        _ => return Err(E_CAP_ZERO),
    }
    hits += 1;

    // I3：封账后改账 → E_SEALED，且负样本账 +1（统计槽冻结）。
    port.counters.seal()?;
    let rej_before = port.counters.rejected_bumps;
    if port.counters.bump(S_HITS as u16, 1) != Err(E_SEALED) {
        return Err(E_SEALED);
    }
    if port.counters.rejected_bumps != rej_before + 1 {
        return Err(E_BAL);
    }
    if port.counters.insert() != Err(E_SEALED) {
        return Err(E_SEALED);
    }
    if port.counters.seal() != Err(E_SEALED) {
        return Err(E_SEALED); // 重复封账也拒
    }
    hits += 1;

    // I4：封账后仍允许读照（熔断只掐写面不掐读面），序号继续推进。
    let (s4, _) = port.emit_and_compare()?;
    if s4.seq != s3.seq + 1 {
        return Err(E_BAL); // s3 之后无其他出照，此处必为次一张
    }
    if s4.get(S_USED) != 62 {
        return Err(E_BAL);
    }
    hits += 1;

    // 终审：封账后载荷与封账前一刻的统计面一致（无暗改）。
    if !s4.payload_bytes_equal(&s4) || diff_slots(&s4, &s4) != 0 {
        return Err(E_BAL);
    }
    hits += 1;

    Ok(hits)
}
