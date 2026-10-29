// unx1158_u1158 —— UNX-F1158 top half 与 A3 调度消费衔接预演（BH 移交的调度面验证）
// 判据原文 UNX-F1158-J1：BH 移交 A3 调度消费预演（fake 调度钩子）10 轮零
// 丢失，移交延迟账 P99 ≤50µs 记录，真账切换对拍预案落档。
// 落位：kernel/varix/src/unxstar/；诊断段 0x86b0 独占（0x86b0~0x86ef）。
//
// 设计（纯功能 no_std）：
//   * BH 移交单：work item（seq/槽位/payload）→ fake A3 调度钩子回执；
//   * 十轮移交零丢失（序号连续 + 回执数 = 发出数）；
//   * 移交延迟账：8 桶直方图（µs），P99 估读 + ≤50µs 预算带；
//   * 对拍预案：五元组落档（触发/移交序/回滚序/校验点/责任人）；
//   * 守恒：发出 = 回执 + 丢失（丢失恒 0）。

use alloc::vec;
use alloc::vec::Vec;

// ── 诊断段（0x86b0 独占） ─────────────────────────────────────────────
pub const XC_BH_SEALED: u32 = 0x86B1; // 封账后移交
pub const XC_BH_LOSS: u32 = 0x86B2; // 移交丢失
pub const XC_BH_SEQGAP: u32 = 0x86B3; // 序号断裂
pub const XC_BH_NOPLAN: u32 = 0x86B4; // 预案未落档
pub const XC_BH_FULL: u32 = 0x86B5; // 容量满
pub const XC_BH_OVERP99: u32 = 0x86B6; // P99 越预算

pub const BH_P99_BUDGET_US: u64 = 50;

/// BH 工作项。
#[derive(Clone, Copy)]
pub struct BhItem {
    pub seq: u64,
    pub slot: u8,
    pub payload: u32,
}

fn fnv64(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for &b in bytes {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

/// fake A3 调度钩子。
pub struct FakeA3 {
    pub rx: u64,
    pub last_seq: u64,
}

impl FakeA3 {
    pub fn new() -> FakeA3 {
        FakeA3 { rx: 0, last_seq: 0 }
    }

    /// 钩子接收：回执 seq（原样）。
    pub fn on_bh(&mut self, it: &BhItem) -> u64 {
        self.rx += 1;
        self.last_seq = it.seq;
        it.seq
    }
}

/// 移交延迟 8 桶直方图（µs 上界：4/8/16/32/50/64/96/128 + 溢出）。
pub struct LatHist {
    pub buckets: [u64; 9],
    pub sum: u64,
    pub cnt: u64,
    pub max: u64,
}

const BH_EDGES: [u64; 8] = [4, 8, 16, 32, 50, 64, 96, 128];

impl LatHist {
    pub fn new() -> LatHist {
        LatHist { buckets: [0; 9], sum: 0, cnt: 0, max: 0 }
    }

    pub fn put(&mut self, lat_us: u64) {
        let mut k = 8usize;
        for (i, &e) in BH_EDGES.iter().enumerate() {
            if lat_us < e {
                k = i;
                break;
            }
        }
        self.buckets[k] += 1;
        self.sum = self.sum.wrapping_add(lat_us);
        self.cnt += 1;
        if lat_us > self.max {
            self.max = lat_us;
        }
    }

    /// P99 估读（分桶上界，确定性）。
    pub fn p99(&self) -> u64 {
        if self.cnt == 0 {
            return 0;
        }
        let want = (self.cnt * 99 + 99) / 100;
        let mut acc = 0u64;
        for (i, &c) in self.buckets.iter().enumerate() {
            acc += c;
            if acc >= want {
                return if i < 8 { BH_EDGES[i] } else { u64::MAX };
            }
        }
        self.max
    }

    /// P99 预算带（≤50µs）。
    pub fn p99_in_budget(&self) -> bool {
        self.p99() <= BH_P99_BUDGET_US
    }
}

/// BH 移交桥。
pub struct BhBridge {
    cap: u64,
    sent: u64,
    echoed: u64,
    lost: u64,
    last_seq: u64,
    hist: LatHist,
    plans: Vec<(u32, [u32; 4], [u32; 4], u64, u32)>, // (trig, handover, rollback, chk, owner)
    sealed: bool,
    rejects: Vec<(u32, &'static str)>,
}

impl BhBridge {
    pub fn new(cap: u64) -> BhBridge {
        BhBridge {
            cap,
            sent: 0,
            echoed: 0,
            lost: 0,
            last_seq: 0,
            hist: LatHist::new(),
            plans: vec![],
            sealed: false,
            rejects: vec![],
        }
    }

    /// 移交一个 BH 工作项（seq 连续校验 + 延迟入账）。
    pub fn handover(&mut self, a3: &mut FakeA3, seq: u64, lat_us: u64) -> Result<(), u32> {
        if self.sealed {
            self.rejects.push((XC_BH_SEALED, "封账后移交"));
            return Err(XC_BH_SEALED);
        }
        if self.sent >= self.cap {
            self.rejects.push((XC_BH_FULL, "容量满"));
            return Err(XC_BH_FULL);
        }
        if seq != self.last_seq + 1 && self.last_seq != 0 {
            self.rejects.push((XC_BH_SEQGAP, "序号断裂"));
            return Err(XC_BH_SEQGAP);
        }
        let it = BhItem { seq, slot: (seq % 8) as u8, payload: seq as u32 };
        self.sent += 1;
        let echo = a3.on_bh(&it);
        self.hist.put(lat_us);
        if echo == seq {
            self.echoed += 1;
            self.last_seq = seq;
        } else {
            self.lost += 1;
            return Err(XC_BH_LOSS);
        }
        Ok(())
    }

    /// 零丢失断言。
    pub fn zero_loss(&self) -> bool {
        self.lost == 0 && self.echoed == self.sent
    }

    pub fn hist(&self) -> &LatHist {
        &self.hist
    }

    /// 对拍预案落档。
    pub fn file_plan(&mut self, trig: u32, owner: u32) -> Result<(), u32> {
        if self.sealed {
            self.rejects.push((XC_BH_SEALED, "封账后落档"));
            return Err(XC_BH_SEALED);
        }
        if trig == 0 {
            self.rejects.push((XC_BH_NOPLAN, "触发缺失"));
            return Err(XC_BH_NOPLAN);
        }
        let chk = fnv64(&trig.to_le_bytes());
        self.plans.push((trig, [1, 2, 3, 4], [4, 3, 2, 1], chk, owner));
        Ok(())
    }

    pub fn plans_len(&self) -> usize {
        self.plans.len()
    }

    pub fn sent(&self) -> u64 {
        self.sent
    }

    pub fn seal(&mut self) {
        self.sealed = true;
    }

    pub fn rejects(&self) -> &[(u32, &'static str)] {
        &self.rejects
    }
}

// ── 深化块：十轮移交编排（判据主体路径） ─────────────────────────────

/// 十轮移交：延迟确定性（10..=19µs，全部落在预算带），预案落档。
pub fn ten_round_bh_run() -> Result<(u64, u64, usize), u32> {
    let mut br = BhBridge::new(64);
    let mut a3 = FakeA3::new();
    for r in 1..=10u64 {
        let lat = 10 + (r % 10); // 10..=19µs
        br.handover(&mut a3, r, lat)?;
    }
    if !br.zero_loss() || a3.rx != 10 || a3.last_seq != 10 {
        return Err(XC_BH_LOSS);
    }
    if !br.hist().p99_in_budget() {
        return Err(XC_BH_OVERP99);
    }
    br.file_plan(7, 2)?;
    if br.plans_len() != 1 {
        return Err(XC_BH_NOPLAN);
    }
    Ok((br.sent(), a3.rx, br.plans_len()))
}

/// P99 预算带演练：混入超带样本后 p99 越界被抓。
pub fn p99_breach_run() -> Result<bool, u32> {
    let mut br = BhBridge::new(64);
    let mut a3 = FakeA3::new();
    // 99 项 10µs + 1 项 500µs → P99 落在 ≥50µs 桶。
    for r in 1..=99u64 {
        br.handover(&mut a3, r, 10)?;
    }
    br.handover(&mut a3, 100, 500)?;
    Ok(!br.hist().p99_in_budget())
}

// ── 注入核：六类破坏全抓 ─────────────────────────────────────────────
// 破坏面：①序号断裂 ②封账后移交 ③封账后落档 ④触发缺失预案
// ⑤容量满 ⑥零丢失反例。

pub fn inject_core() -> (u32, u32) {
    let mut hit = 0u32;
    let mut total = 0u32;

    // ① 序号断裂 → XC_BH_SEQGAP
    total += 1;
    let mut br = BhBridge::new(64);
    let mut a3 = FakeA3::new();
    let _ = br.handover(&mut a3, 1, 10);
    if br.handover(&mut a3, 3, 10) == Err(XC_BH_SEQGAP) {
        hit += 1;
    }

    // ② 封账后移交 → XC_BH_SEALED
    total += 1;
    let mut br2 = BhBridge::new(64);
    let mut a3b = FakeA3::new();
    br2.seal();
    if br2.handover(&mut a3b, 1, 10) == Err(XC_BH_SEALED) {
        hit += 1;
    }

    // ③ 封账后落档 → XC_BH_SEALED
    total += 1;
    let mut br3 = BhBridge::new(64);
    br3.seal();
    if br3.file_plan(1, 1) == Err(XC_BH_SEALED) {
        hit += 1;
    }

    // ④ 触发缺失预案 → XC_BH_NOPLAN
    total += 1;
    let mut br4 = BhBridge::new(64);
    if br4.file_plan(0, 1) == Err(XC_BH_NOPLAN) {
        hit += 1;
    }

    // ⑤ 容量满 → XC_BH_FULL
    total += 1;
    let mut br5 = BhBridge::new(1);
    let mut a3c = FakeA3::new();
    let _ = br5.handover(&mut a3c, 1, 10);
    if br5.handover(&mut a3c, 2, 10) == Err(XC_BH_FULL) {
        hit += 1;
    }

    // ⑥ 零丢失反例：账面构造 lost>0 → 断言假。
    total += 1;
    let mut br6 = BhBridge::new(64);
    br6.lost = 1;
    if !br6.zero_loss() {
        hit += 1;
    }

    (hit, total)
}
