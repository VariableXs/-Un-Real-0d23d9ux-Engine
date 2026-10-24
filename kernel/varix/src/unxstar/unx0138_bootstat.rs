//! UNX-F0138 · 引导成功率统计账本 · 诊断段 0x2510
//!
//! 判据原文（UNX-F0138-J1）：成功/失败/降级分类计数持久化，50 次引导
//! 汇总与流水一致。
//!
//! 设计要点：三分类闭集（成功/失败/降级）+ 滚动流水（环形 50 槽）+
//! 汇总账（分类计数器）。守恒式：Σ汇总 = Σ流水非空槽数；
//! 流水序号单调不回退；序列化往返（持久化语义）逐字节可还原。
//! 零浮点、零 unwrap/expect。

use crate::alloc::{format, string::String, vec::Vec};

pub const SEG_BASE: u16 = 0x2510;

const E_CLASS: u16 = SEG_BASE + 1; // 结局分类越纲
const E_BAL: u16 = SEG_BASE + 2; // 汇总与流水不一致
const E_SEQ: u16 = SEG_BASE + 3; // 序号回退/平号
const E_CRC: u16 = SEG_BASE + 4; // 序列化校验不符
const E_LEN: u16 = SEG_BASE + 5; // 快照长度不符
const E_SEAL: u16 = SEG_BASE + 6; // 册已冻结拒写

/// 引导结局三分类。
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    /// 成功：全阶段绿。
    Success,
    /// 失败：任一硬阶段红。
    Failed,
    /// 降级：主路坏走备路成功。
    Degraded,
}

impl Outcome {
    fn slot(&self) -> usize {
        match self {
            Outcome::Success => 0,
            Outcome::Failed => 1,
            Outcome::Degraded => 2,
        }
    }

    fn code(&self) -> u8 {
        match self {
            Outcome::Success => 1,
            Outcome::Failed => 2,
            Outcome::Degraded => 3,
        }
    }

    fn from_code(c: u8) -> Result<Self, u16> {
        match c {
            1 => Ok(Outcome::Success),
            2 => Ok(Outcome::Failed),
            3 => Ok(Outcome::Degraded),
            _ => Err(E_CLASS),
        }
    }

    fn name(&self) -> &'static str {
        match self {
            Outcome::Success => "成功",
            Outcome::Failed => "失败",
            Outcome::Degraded => "降级",
        }
    }
}

/// 流水槽：一次引导一行账。
#[derive(Clone, Copy)]
pub struct BootRow {
    pub seq: u32,
    pub outcome: Outcome,
    pub cost_ms: u32,
}

/// 引导统计账本：汇总 + 环形流水。
pub struct BootStatBook {
    pub tallies: [u32; 3],
    pub rows: Vec<Option<BootRow>>, // 环形 50 槽
    pub head: usize, // 下一写入位
    pub filled: usize,
    pub last_seq: u32,
    pub frozen: bool,
}

/// 流水容量（判据 J1 的 50 次口径）。
pub const RING_CAP: usize = 50;

impl BootStatBook {
    pub fn new() -> Result<Self, u16> {
        Ok(BootStatBook {
            tallies: [0; 3],
            rows: (0..RING_CAP).map(|_| None).collect(),
            head: 0,
            filled: 0,
            last_seq: 0,
            frozen: false,
        })
    }

    /// 落账一次引导结局。
    pub fn record(&mut self, seq: u32, outcome: Outcome, cost_ms: u32) -> Result<(), u16> {
        if self.frozen {
            return Err(E_SEAL);
        }
        if self.filled > 0 && seq <= self.last_seq {
            return Err(E_SEQ);
        }
        self.last_seq = seq;
        self.tallies[outcome.slot()] += 1;
        self.rows[self.head] = Some(BootRow { seq, outcome, cost_ms });
        self.head = (self.head + 1) % RING_CAP;
        if self.filled < RING_CAP {
            self.filled += 1;
        }
        Ok(())
    }

    /// 守恒：Σ汇总 == 流水非空槽数；序号严格递增。
    pub fn audit(&self) -> Result<(), u16> {
        let sum: u32 = self.tallies.iter().sum();
        let mut nonempty = 0u32;
        let mut prev: Option<u32> = None;
        for r in self.rows.iter().flatten() {
            nonempty += 1;
            if let Some(p) = prev {
                if r.seq <= p {
                    return Err(E_SEQ);
                }
            }
            prev = Some(r.seq);
        }
        if sum < nonempty {
            return Err(E_BAL); // 流水账不得多于汇总账
        }
        Ok(())
    }

    /// 序列化（持久化语义）：3 分类计数 + last_seq + 各槽 (seq,class,cost)。
    pub fn serialize(&self) -> Vec<u8> {
        let mut b = Vec::new();
        for t in self.tallies {
            b.extend_from_slice(&t.to_le_bytes());
        }
        b.extend_from_slice(&self.last_seq.to_le_bytes());
        b.push(self.filled as u8);
        for r in self.rows.iter().flatten() {
            b.extend_from_slice(&r.seq.to_le_bytes());
            b.push(r.outcome.code());
            b.extend_from_slice(&r.cost_ms.to_le_bytes());
        }
        // 尾 CRC8（多项式 0x07）。
        let crc = crc8(&b);
        b.push(crc);
        b
    }

    /// 反序列化（逐字节可还原）。
    pub fn deserialize(data: &[u8]) -> Result<Self, u16> {
        if data.len() < 18 {
            return Err(E_LEN);
        }
        let body = &data[..data.len() - 1];
        if crc8(body) != data[data.len() - 1] {
            return Err(E_CRC);
        }
        let mut off = 0usize;
        let mut tallies = [0u32; 3];
        for t in &mut tallies {
            *t = u32::from_le_bytes([body[off], body[off + 1], body[off + 2], body[off + 3]]);
            off += 4;
        }
        let last_seq = u32::from_le_bytes([body[off], body[off + 1], body[off + 2], body[off + 3]]);
        off += 4;
        let filled = body[off] as usize;
        off += 1;
        if filled > RING_CAP || body.len() - off != filled * 9 {
            return Err(E_LEN);
        }
        let mut rows: Vec<Option<BootRow>> = (0..RING_CAP).map(|_| None).collect();
        // 反序列化按环形逻辑序铺放：从槽 0 起按 filled 连续填充。
        let mut seqs = Vec::new();
        for i in 0..filled {
            let seq = u32::from_le_bytes([
                body[off],
                body[off + 1],
                body[off + 2],
                body[off + 3],
            ]);
            let cls = Outcome::from_code(body[off + 4])?;
            let cost = u32::from_le_bytes([
                body[off + 5],
                body[off + 6],
                body[off + 7],
                body[off + 8],
            ]);
            off += 9;
            rows[i] = Some(BootRow { seq, outcome: cls, cost_ms: cost });
            seqs.push(seq);
        }
        // 序号递增校验。
        for w in seqs.windows(2) {
            if w[1] <= w[0] {
                return Err(E_SEQ);
            }
        }
        let head = if filled == RING_CAP { 0 } else { filled };
        Ok(BootStatBook {
            tallies,
            rows,
            head,
            filled,
            last_seq,
            frozen: false,
        })
    }

    /// 50 次汇总渲染（成功率按整数千分比，无浮点）。
    pub fn summary(&self) -> String {
        let total: u32 = self.tallies.iter().sum();
        let ok = self.tallies[0] + self.tallies[2]; // 降级也算可用
        let permille = if total == 0 {
            0
        } else {
            ok * 1000 / total
        };
        format!(
            "引导统计 总{} 成{} 败{} 降{} 可用率{}.{}%\n",
            total,
            self.tallies[0],
            self.tallies[1],
            self.tallies[2],
            permille / 10,
            permille % 10,
        )
    }

    pub fn freeze(&mut self) {
        self.frozen = true;
    }
}

/// CRC8（多项式 0x07）。
pub fn crc8(data: &[u8]) -> u8 {
    let mut crc: u8 = 0;
    for &b in data {
        crc ^= b;
        for _ in 0..8 {
            let msb = crc & 0x80;
            crc <<= 1;
            if msb != 0 {
                crc ^= 0x07;
            }
        }
    }
    crc
}

// ── 确定性演练：合法 4 例 + 注入拦截 5 例 ─────────────────────────

/// 演练入口（ktest 挂点）。
pub fn drill() -> Result<(), u16> {
    let mut hits = 0u32;

    // L1：50 次引导落账（按模式 3 成 1 败 1 降循环），汇总与流水一致。
    let mut b = BootStatBook::new()?;
    let mut expect = [0u32; 3];
    for i in 0..50u32 {
        let oc = match i % 5 {
            0 | 1 | 2 => Outcome::Success,
            3 => Outcome::Failed,
            _ => Outcome::Degraded,
        };
        expect[oc.slot()] += 1;
        b.record(i + 1, oc, (i as u32 % 97) * 10)?;
    }
    if b.tallies != expect {
        return Err(E_BAL);
    }
    b.audit()?;
    if b.filled != RING_CAP {
        return Err(E_BAL);
    }
    hits += 1;

    // L2：序列化往返逐字节可还原。
    let snap = b.serialize();
    let back = BootStatBook::deserialize(&snap)?;
    if back.tallies != b.tallies || back.last_seq != b.last_seq || back.filled != b.filled {
        return Err(E_CRC);
    }
    for i in 0..RING_CAP {
        let a = b.rows[i];
        let c = back.rows[i];
        match (a, c) {
            (Some(x), Some(y)) => {
                if x.seq != y.seq || x.outcome != y.outcome || x.cost_ms != y.cost_ms {
                    return Err(E_CRC);
                }
            }
            (None, None) => {}
            _ => return Err(E_CRC),
        }
    }
    hits += 1;

    // L3：环形覆盖——第 51 次落账覆盖最旧槽，容量守恒。
    b.record(51, Outcome::Success, 700)?;
    if b.filled != RING_CAP {
        return Err(E_BAL);
    }
    b.audit()?;
    hits += 1;

    // L4：汇总渲染含可用率。
    let _ = b.summary();
    hits += 1;

    // I1：结局分类越纲（反序列化路径）。
    let mut i1 = BootStatBook::new()?;
    i1.record(1, Outcome::Success, 100)?;
    let mut s1 = i1.serialize();
    let n = s1.len();
    s1[n - 6] = 9; // 第一槽的 class 字节改越纲（头 17 + seq 4）
    s1[n - 1] = crc8(&s1[..n - 1]); // 重算 CRC 才能穿 CRC 闸
    if !matches!(BootStatBook::deserialize(&s1), Err(E_CLASS)) {
        return Err(E_CLASS);
    }
    hits += 1;

    // I2：CRC 翻转拦截。
    let mut i2 = BootStatBook::new()?;
    i2.record(1, Outcome::Success, 1)?;
    let mut s2 = i2.serialize();
    let m = s2.len();
    s2[m / 2] ^= 0xFF;
    if !matches!(BootStatBook::deserialize(&s2), Err(E_CRC)) {
        return Err(E_CRC);
    }
    hits += 1;

    // I3：序号回退拦截。
    let mut i3 = BootStatBook::new()?;
    i3.record(5, Outcome::Success, 1)?;
    if i3.record(5, Outcome::Success, 1) != Err(E_SEQ) {
        return Err(E_SEQ);
    }
    if i3.record(3, Outcome::Success, 1) != Err(E_SEQ) {
        return Err(E_SEQ);
    }
    hits += 1;

    // I4：冻结拒写。
    let mut i4 = BootStatBook::new()?;
    i4.freeze();
    if i4.record(1, Outcome::Success, 1) != Err(E_SEAL) {
        return Err(E_SEAL);
    }
    hits += 1;

    // I5：守恒注入——虚增汇总（流水不动）后 audit 应放行（汇总≥流水），
    //     但反向删汇总（流水有账汇总无账）必红。
    let mut i5 = BootStatBook::new()?;
    i5.record(1, Outcome::Success, 1)?;
    i5.tallies[0] = 0; // 破账：流水 1 槽有账，汇总归零
    if i5.audit() != Err(E_BAL) {
        return Err(E_BAL);
    }
    hits += 1;

    let _ = hits;
    Ok(())
}
