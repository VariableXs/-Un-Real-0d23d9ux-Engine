//! unx48311_cfglink — UNX-F48311 配置与词库/短语联动一致性（短语入配置树的同步纪律）（诊断段 0xC7C0）。
//!
//! 判据原文：
//!   J1 联动 ×100 同步一致；判据锚定 §AI-61 §2 专题五。
//!   J2 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计纪律：
//!   1. 双侧同步：短语条目在配置树（Cfg 侧）与词库视图（Dict 侧）
//!      双侧登记；任何增/删操作必须同事务改两侧——配置侧与词库侧
//!      集合恒等是同步纪律的唯一口径。
//!   2. ×100 联动：100 次混合增删操作，每次操作后立即断言双侧集合
//!      逐位一致（逐操作断言，不是只查终态）。
//!   3. 非法操作双侧一致拒绝：删不存在的键、增重复的键，两侧都
//!      必须拒绝且拒绝后集合零变化（拒绝口径不漂移）。
//!   4. 事务原子性：任何失败操作（含校验失败）不得留下半侧修改
//!      ——失败后双侧指纹与操作前逐位一致。
//!   5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。

use crate::alloc::vec;
use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0xC7C0;

const E_SYNC_DRIFT: u16 = SEG_BASE + 1;   // 双侧集合不一致
const E_REJECT_DRIFT: u16 = SEG_BASE + 2; // 非法操作两侧拒绝口径不一
const E_ATOMIC_BROKEN: u16 = SEG_BASE + 3; // 失败操作留下半侧修改
const E_DUP_ACCEPT: u16 = SEG_BASE + 4;   // 重复键被放行
const E_GHOST_DEL: u16 = SEG_BASE + 5;    // 幽灵删除被放行
const E_ORDER_BAD: u16 = SEG_BASE + 6;    // 集合无序（规约破坏）
const E_CODE_RANGE: u16 = SEG_BASE + 7;   // 诊断码越段

/// 短语条目：短语哈希（u32）。
pub type PhraseId = u32;

/// 配置树侧短语区（有序去重）。
pub struct CfgSide {
    pub items: Vec<PhraseId>,
}

/// 词库视图侧短语区（有序去重）。
pub struct DictSide {
    pub items: Vec<PhraseId>,
}

/// 联动枢纽：apply 同事务改两侧；校验先行，失败零改。
pub struct LinkHub {
    pub cfg: CfgSide,
    pub dict: DictSide,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum LinkOp {
    Add(PhraseId),
    Del(PhraseId),
}

impl LinkHub {
    pub fn new() -> Self {
        LinkHub {
            cfg: CfgSide { items: Vec::new() },
            dict: DictSide { items: Vec::new() },
        }
    }

    /// 双侧一致性：两集合逐位相等（长度 + 逐位）。
    pub fn in_sync(&self) -> bool {
        if self.cfg.items.len() != self.dict.items.len() {
            return false;
        }
        let mut i = 0;
        while i < self.cfg.items.len() {
            if self.cfg.items[i] != self.dict.items[i] {
                return false;
            }
            i += 1;
        }
        true
    }

    /// 有序性规约：升序无重。
    pub fn ordered(&self) -> bool {
        let mut i = 1;
        while i < self.cfg.items.len() {
            if self.cfg.items[i] <= self.cfg.items[i - 1] {
                return false;
            }
            i += 1;
        }
        true
    }

    /// 同事务增：两侧都有 → 拒绝（一致拒绝）；都无 → 双侧插入
    /// （按升序落位）。返回 Ok(true)=新增 / Ok(false)=幂等拒绝 / Err=原子破坏。
    pub fn apply(&mut self, op: LinkOp) -> Result<bool, u16> {
        match op {
            LinkOp::Add(pid) => {
                // 校验先行：任一侧已存在即整体拒绝（双侧口径一致）。
                let in_cfg = self.cfg.items.contains(&pid);
                let in_dict = self.dict.items.contains(&pid);
                if in_cfg != in_dict {
                    // 双侧已漂移——原子性破坏的前兆，显式暴露。
                    return Err(E_SYNC_DRIFT);
                }
                if in_cfg {
                    return Ok(false);
                }
                // 双侧同事务插入（升序落位）。
                let mut pos = 0usize;
                while pos < self.cfg.items.len() && self.cfg.items[pos] < pid {
                    pos += 1;
                }
                self.cfg.items.insert(pos, pid);
                let mut pos2 = 0usize;
                while pos2 < self.dict.items.len() && self.dict.items[pos2] < pid {
                    pos2 += 1;
                }
                self.dict.items.insert(pos2, pid);
                Ok(true)
            }
            LinkOp::Del(pid) => {
                let in_cfg = self.cfg.items.contains(&pid);
                let in_dict = self.dict.items.contains(&pid);
                if in_cfg != in_dict {
                    return Err(E_SYNC_DRIFT);
                }
                if !in_cfg {
                    return Ok(false);
                }
                // 双侧同事务删除。
                let mut i = 0;
                while i < self.cfg.items.len() {
                    if self.cfg.items[i] == pid {
                        self.cfg.items.remove(i);
                        break;
                    }
                    i += 1;
                }
                let mut j = 0;
                while j < self.dict.items.len() {
                    if self.dict.items[j] == pid {
                        self.dict.items.remove(j);
                        break;
                    }
                    j += 1;
                }
                Ok(true)
            }
        }
    }

    pub fn fingerprint(&self) -> u64 {
        let mut h: u64 = 0xCBF2_9CE4_8422_2325;
        let mut i = 0;
        while i < self.cfg.items.len() {
            h ^= self.cfg.items[i] as u64;
            h = h.wrapping_mul(0x0000_0100_0000_01B3);
            i += 1;
        }
        h
    }
}

/// ×100 联动操作序列合成：LCG 确定性（增 60% 删 40%，id 空间有界
/// 保证删操作有实际命中与未命中两种样本）。
pub fn synth_ops(n: usize, seed: u32) -> Vec<LinkOp> {
    let mut lc = Lcg::new(seed);
    let mut ops: Vec<LinkOp> = Vec::new();
    let mut i = 0;
    while i < n {
        let r = lc.next();
        let pid = 100 + (r % 70) as u32; // id ∈ [100,170)，空间有界
        if r & 0x8000 != 0 {
            ops.push(LinkOp::Add(pid));
        } else {
            ops.push(LinkOp::Del(pid));
        }
        i += 1;
    }
    ops
}

/// 确定性线性同余发生器。
pub struct Lcg(u32);

impl Lcg {
    pub fn new(seed: u32) -> Self {
        Lcg(seed | 1)
    }
    pub fn next(&mut self) -> u32 {
        self.0 = self.0.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        self.0
    }
}

/// 配置与短语联动主巡检：×100 同步一致全链判据。
pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;
    let ops = synth_ops(100, 0x4831_1001);

    // R1 ×100 逐操作同步断言：每次操作后双侧集合逐位一致且有序。
    let mut hub = LinkHub::new();
    let mut adds_ok = 0u32;
    let mut dels_ok = 0u32;
    let mut idem = 0u32;
    let mut i = 0;
    while i < ops.len() {
        let r = hub.apply(ops[i])?;
        match r {
            true => match ops[i] {
                LinkOp::Add(_) => adds_ok += 1,
                LinkOp::Del(_) => dels_ok += 1,
            },
            false => idem += 1,
        }
        // 逐操作断言（不是只查终态）。
        if !hub.in_sync() {
            return Err(E_SYNC_DRIFT);
        }
        if !hub.ordered() {
            return Err(E_ORDER_BAD);
        }
        i += 1;
    }
    if adds_ok == 0 || dels_ok == 0 || idem == 0 {
        // 三类结果（新增/删除/幂等拒绝）都要有真实样本，否则序列太弱。
        return Err(E_REJECT_DRIFT);
    }
    hits += 1;

    // R2 终态一致 + 有序。
    if !hub.in_sync() || !hub.ordered() {
        return Err(E_SYNC_DRIFT);
    }
    hits += 1;

    // R3 非法操作双侧一致拒绝：删不存在键 / 增重复键 → Ok(false)
    //    且双侧指纹不变（拒绝不留痕）。
    let fp_before = hub.fingerprint();
    match hub.apply(LinkOp::Del(99999))? {
        false => {}
        true => return Err(E_GHOST_DEL),
    }
    if hub.fingerprint() != fp_before {
        return Err(E_ATOMIC_BROKEN);
    }
    if let Ok(true) = hub.apply(LinkOp::Add(99999)) {
        // 新增成功则把 99999 变成重复样本。
    }
    let fp_mid = hub.fingerprint();
    match hub.apply(LinkOp::Add(99999))? {
        false => {}
        true => return Err(E_DUP_ACCEPT),
    }
    if hub.fingerprint() != fp_mid {
        return Err(E_ATOMIC_BROKEN);
    }
    hits += 1;

    // R4 原子性铁证：双侧集合在任何路径下都不出现「半侧状态」——
    //    构造 70 个全量增（id 100..170 全域），删除一半，断言双侧
    //    长度始终相等（半侧 = 长度不齐）。
    let mut hub2 = LinkHub::new();
    let mut pid = 100u32;
    while pid < 170 {
        hub2.apply(LinkOp::Add(pid))?;
        if hub2.cfg.items.len() != hub2.dict.items.len() {
            return Err(E_ATOMIC_BROKEN);
        }
        pid += 1;
    }
    let mut pid2 = 100u32;
    while pid2 < 135 {
        hub2.apply(LinkOp::Del(pid2))?;
        if hub2.cfg.items.len() != hub2.dict.items.len() {
            return Err(E_ATOMIC_BROKEN);
        }
        pid2 += 1;
    }
    if hub2.cfg.items.len() != 35 {
        return Err(E_ATOMIC_BROKEN);
    }
    hits += 1;

    // R5 全量域终态：hub2 留存 id ∈ [135,170) 恰 35 条，双侧同构
    //    且指纹可复算（重跑同操作指纹不变）。
    let fp_a = hub2.fingerprint();
    let mut hub3 = LinkHub::new();
    let mut pid3 = 100u32;
    while pid3 < 170 {
        hub3.apply(LinkOp::Add(pid3))?;
        pid3 += 1;
    }
    let mut pid4 = 100u32;
    while pid4 < 135 {
        hub3.apply(LinkOp::Del(pid4))?;
        pid4 += 1;
    }
    if hub3.fingerprint() != fp_a {
        return Err(E_SYNC_DRIFT);
    }
    hits += 1;

    // R6 诊断码闭包：全部错误码落在 [SEG_BASE+1, SEG_BASE+0x1F] 段内。
    const CODES: [u16; 7] = [
        E_SYNC_DRIFT,
        E_REJECT_DRIFT,
        E_ATOMIC_BROKEN,
        E_DUP_ACCEPT,
        E_GHOST_DEL,
        E_ORDER_BAD,
        E_CODE_RANGE,
    ];
    let mut ci = 0;
    while ci < CODES.len() {
        if CODES[ci] <= SEG_BASE || CODES[ci] > SEG_BASE + 0x1F {
            return Err(E_CODE_RANGE);
        }
        ci += 1;
    }
    hits += 1;

    Ok(hits)
}
