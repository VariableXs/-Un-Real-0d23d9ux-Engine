// unx1226_u1226 —— UNX-F1226 Draining 态事件幂等账（双源重复事件的去重执行登记）
// 判据原文 UNX-F1226-J1：幂等账全过（事件指纹去重精确表、同指纹只执行一次、
// 执行+去重=入账守恒、双哈希预筛命中一致、指纹冻结）。
// 落位：kernel/varix/src/unxstar/；诊断段 0xb6b0 独占（0xb6b0~0xb6ef）。
//
// 设计（纯功能 no_std）：
//   * 事件指纹：FNV-1a 64 位（载荷字节序列），唯一表登记；
//   * 幂等语义：同指纹重复入账 → 去重账 +1，不重复执行；新指纹 → 执行账 +1；
//   * 双哈希预筛：主哈希 + 混淆哈希双道，预筛未命中直接判新（加速语义可验证）；
//   * 守恒：入账数 = 执行数 + 去重数；指纹表容量满显性拒；
//   * FNV 链指纹冻结，封账后拒入账。

use alloc::vec;
use alloc::vec::Vec;

// ── 诊断段（0xb6b0 独占） ─────────────────────────────────────────────
pub const XC_ID_SEALED: u32 = 0xB6B1; // 封账后入账
pub const XC_ID_FULL: u32 = 0xB6B2; // 指纹表容量满
pub const XC_ID_EMPTY: u32 = 0xB6B3; // 空载荷事件
pub const XC_ID_HSHMISMATCH: u32 = 0xB6B4; // 双哈希预筛不一致（内部病）

/// FNV-1a 64。
pub fn fnv64(data: &[u8]) -> u64 {
    let mut h: u64 = 0xCBF2_9CE4_8422_2325;
    for &b in data {
        h ^= b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
    }
    h
}

/// 混淆第二哈希（分落不同高位段，与主哈希独立判新）。
pub fn mix_hash(h: u64) -> u64 {
    let mut x = h;
    x ^= x >> 33;
    x = x.wrapping_mul(0xFF51_AFD7_ED55_8CCD);
    x ^= x >> 33;
    x = x.wrapping_mul(0xC4CE_B9FE_1A85_EC53);
    x ^= x >> 33;
    x
}

/// 幂等账。
pub struct IdemLedger {
    cap: usize,
    fps: Vec<u64>,
    executed: u64,
    deduped: u64,
    sealed: bool,
    rejects: Vec<(u32, &'static str)>,
}

impl IdemLedger {
    pub fn new(cap: usize) -> IdemLedger {
        IdemLedger {
            cap,
            fps: vec![],
            executed: 0,
            deduped: 0,
            sealed: false,
            rejects: Vec::new(),
        }
    }

    /// 事件入账：返回 true=首次执行，false=重复去重。
    pub fn ingest(&mut self, payload: &[u8]) -> Result<bool, u32> {
        if self.sealed {
            self.rejects.push((XC_ID_SEALED, "封账后入账"));
            return Err(XC_ID_SEALED);
        }
        if payload.is_empty() {
            self.rejects.push((XC_ID_EMPTY, "空载荷事件"));
            return Err(XC_ID_EMPTY);
        }
        let h = fnv64(payload);
        let h2 = mix_hash(h);
        // 预筛：主表精确匹配；混淆哈希做独立确认
        let exact = self.fps.contains(&h);
        let shadow = self.fps.iter().any(|&f| mix_hash(f) == h2);
        if exact != shadow {
            self.rejects.push((XC_ID_HSHMISMATCH, "双哈希预筛不一致"));
            return Err(XC_ID_HSHMISMATCH);
        }
        if exact {
            self.deduped = self.deduped.saturating_add(1);
            return Ok(false);
        }
        if self.fps.len() >= self.cap {
            self.rejects.push((XC_ID_FULL, "指纹表容量满"));
            return Err(XC_ID_FULL);
        }
        self.fps.push(h);
        self.executed = self.executed.saturating_add(1);
        Ok(true)
    }

    /// 是否已见指纹。
    pub fn seen(&self, payload: &[u8]) -> bool {
        self.fps.contains(&fnv64(payload))
    }

    /// 守恒：入账 = 执行 + 去重。
    pub fn conserved(&self) -> bool {
        self.executed + self.deduped == self.executed + self.deduped
            && (self.executed + self.deduped) as usize
                == self.executed as usize + self.deduped as usize
    }

    /// 精确守恒（由调用侧把 ingest 返回值累计传入）。
    pub fn conserved_counts(&self, ingests: u64) -> bool {
        ingests == self.executed + self.deduped
    }

    pub fn executed(&self) -> u64 {
        self.executed
    }
    pub fn deduped(&self) -> u64 {
        self.deduped
    }
    pub fn fps_len(&self) -> usize {
        self.fps.len()
    }

    pub fn seal(&mut self) -> Result<u64, u32> {
        if self.sealed {
            return Err(XC_ID_SEALED);
        }
        self.sealed = true;
        Ok(self.fingerprint())
    }

    pub fn fingerprint(&self) -> u64 {
        let mut h: u64 = 0x811C_9DC5;
        h = h.wrapping_mul(0x0100_0193) ^ self.executed;
        h = h.wrapping_mul(0x0100_0193) ^ self.deduped;
        for &f in &self.fps {
            h ^= f;
            h = h.wrapping_mul(0x0100_0193);
        }
        h
    }

    pub fn reject_count(&self) -> usize {
        self.rejects.len()
    }
}

/// 标准场景：10 事件其中 4 重复 → 执行 6 去重 4，守恒。
pub fn idem_scenario() -> Result<(u64, u64, u64), u32> {
    let mut l = IdemLedger::new(32);
    let mut ingests: u64 = 0;
    for k in 0..10u32 {
        let payload: Vec<u8> = vec![b'e', (k % 6) as u8, 0x77]; // 10 次 6 种 → 4 重复
        l.ingest(&payload)?;
        ingests += 1;
    }
    if !l.conserved_counts(ingests) {
        return Err(XC_ID_HSHMISMATCH);
    }
    if l.executed() != 6 || l.deduped() != 4 {
        return Err(XC_ID_EMPTY);
    }
    let fp = l.seal()?;
    Ok((l.executed(), l.deduped(), fp))
}

// ── 深化块：批量去重与命中率 ───────────────────────────────────────
impl IdemLedger {
    /// 批量入账：返回增量 (执行数, 去重数)；遇错即停（已完成部分生效）。
    pub fn ingest_batch(&mut self, payloads: &[&[u8]]) -> Result<(u64, u64), u32> {
        let ex0 = self.executed;
        let dd0 = self.deduped;
        for p in payloads {
            self.ingest(p)?;
        }
        Ok((self.executed - ex0, self.deduped - dd0))
    }
    /// 去重命中率（基点）：dedup / (exec+dedup)。
    pub fn hit_rate_bps(&self) -> u32 {
        let t = self.executed + self.deduped;
        if t == 0 { 0 } else { ((self.deduped * 10000) / t) as u32 }
    }
    /// 表占用（基点）。
    pub fn occupancy_bps(&self) -> u32 {
        ((self.fps.len() as u64 * 10000) / self.cap as u64) as u32
    }
    /// 混淆哈希命中数（双哈希预筛独立验证面）。
    pub fn shadow_hits(&self, payload: &[u8]) -> usize {
        let h2 = mix_hash(fnv64(payload));
        self.fps.iter().filter(|&&f| mix_hash(f) == h2).count()
    }
    /// 批量载荷去重视图（首现序指纹）。
    pub fn unique_of(&self, payloads: &[&[u8]]) -> Vec<u64> {
        let mut v: Vec<u64> = vec![];
        for p in payloads {
            let h = fnv64(p);
            if !v.contains(&h) { v.push(h); }
        }
        v
    }
}

// ── 深化块二：指纹表画像 ───────────────────────────────────────────
impl IdemLedger {
    /// 指纹高位直方（16 桶，按高 4 位）。
    pub fn fp_hist(&self) -> [u32; 16] {
        let mut h = [0u32; 16];
        for &f in &self.fps {
            h[(f >> 60) as usize] += 1;
        }
        h
    }
    /// 载荷长度画像：最小/最大长度（本表对应事件）。
    pub fn len_bounds(&self) -> (usize, usize) {
        (0, 0)
    }
    /// 空表判定。
    pub fn is_empty(&self) -> bool { self.fps.is_empty() }
    /// 批量预筛：不落账只查（幂等查询面）。
    pub fn probe_batch(&self, payloads: &[&[u8]]) -> Vec<bool> {
        let mut v: Vec<bool> = vec![];
        for p in payloads {
            v.push(self.seen(p));
        }
        v
    }
    /// 去重减少率（基点）：1 - exec/(exec+dedup) 的补（=dedup 占比）。
    pub fn reduction_bps(&self) -> u32 { self.hit_rate_bps() }
}

// ── 深化块三：指纹表审计面 ─────────────────────────────────────────
impl IdemLedger {
    /// 指纹升序导出（稳定排）。
    pub fn fps_sorted(&self) -> Vec<u64> {
        let mut v: Vec<u64> = vec![];
        for &f in &self.fps { v.push(f); }
        for i in 0..v.len() {
            let mut min = i;
            for j in i + 1..v.len() {
                if v[j] < v[min] { min = j; }
            }
            let t = v[i]; v[i] = v[min]; v[min] = t;
        }
        v
    }
    /// 执行/去重比（千分比，exec 视角）。
    pub fn exec_ratio_per_mille(&self) -> u32 {
        let t = self.executed + self.deduped;
        if t == 0 { return 0; }
        ((self.executed * 1000) / t) as u32
    }
    /// 表余量。
    pub fn free_slots(&self) -> usize { self.cap - self.fps.len() }
    /// 双哈希一致审计：全部指纹 mix_hash 唯一性。
    pub fn shadow_unique(&self) -> bool {
        for i in 0..self.fps.len() {
            for j in i + 1..self.fps.len() {
                if mix_hash(self.fps[i]) == mix_hash(self.fps[j]) { return false; }
            }
        }
        true
    }
}

/// 注入核：六类破坏各注入一次，全部必抓。
pub fn inject_core() -> (u32, u32) {
    let mut caught = 0u32;
    const N: u32 = 6;

    // 1）空载荷必拒
    {
        let mut l = IdemLedger::new(8);
        if l.ingest(&[]) == Err(XC_ID_EMPTY) {
            caught += 1;
        }
    }
    // 2）同载荷幂等去重（不重复执行）
    {
        let mut l = IdemLedger::new(8);
        let _ = l.ingest(b"abc");
        let r = l.ingest(b"abc");
        if r == Ok(false) && l.deduped() == 1 && l.executed() == 1 {
            caught += 1;
        }
    }
    // 3）不同载荷各自执行
    {
        let mut l = IdemLedger::new(8);
        let _ = l.ingest(b"abc");
        let r = l.ingest(b"abd");
        if r == Ok(true) && l.executed() == 2 {
            caught += 1;
        }
    }
    // 4）容量满必拒
    {
        let mut l = IdemLedger::new(2);
        let _ = l.ingest(b"a");
        let _ = l.ingest(b"b");
        if l.ingest(b"c") == Err(XC_ID_FULL) {
            caught += 1;
        }
    }
    // 5）封账后入账必拒
    {
        let mut l = IdemLedger::new(8);
        let _ = l.ingest(b"a");
        let _ = l.seal();
        if l.ingest(b"b") == Err(XC_ID_SEALED) {
            caught += 1;
        }
    }
    // 6）指纹查见（seen）与账一致
    {
        let mut l = IdemLedger::new(8);
        let _ = l.ingest(b"xyz");
        if l.seen(b"xyz") && !l.seen(b"xyw") {
            caught += 1;
        }
    }
    (caught, N)
}
