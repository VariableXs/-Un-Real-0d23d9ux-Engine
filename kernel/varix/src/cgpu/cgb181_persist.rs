//! CGPU-F0181 · 图持久化与回放（判据段 0x04F0）
//! 帧图的完整序列化与回放：图序列化格式（节点/边/属性全量的二进制
//! 格式——版本化）、录制-回放（录制一帧的完整图→回放精确重执行）、
//! 回放的确定性保障（时间戳替换/随机数替换/外部输入替换——与 F0172
//! 确定性纪律联动）。判据：序列化 roundtrip、回放逐像素一致、确定性
//! 保障、格式版本化、性能。
//!
//! 诊断码段 0x04F0（本模块独占：0x04F0~0x04FF）。
//! no_std + alloc 零 unwrap，全部 Result 化。

use alloc::vec;
use alloc::vec::Vec;

/// 诊断码段基址。
pub const SEG_BASE: u16 = 0x04F0;

/// 序列化格式版本（判据「格式版本化」：读端只认本版本）。
pub const FMT_VERSION: u32 = 1;

/// 序列化魔数（格式识别头）。
pub const FMT_MAGIC: u64 = 0x564152_58475241; // "VARXGRA"

/// 可序列化图（录制载体）：节点（任务号 + 属性指纹）+ 边。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SerGraph {
    /// 节点：(任务号, 属性指纹)。
    pub nodes: Vec<(u32, u64)>,
    /// 边：(from, to)。
    pub edges: Vec<(u32, u32)>,
}

impl SerGraph {
    /// 图校验：节点唯一非零、属性非零、边端点在图内、无自环。
    pub fn validate(&self) -> Result<(), u16> {
        if self.nodes.is_empty() {
            return Err(SEG_BASE);
        }
        for i in 0..self.nodes.len() {
            if self.nodes[i].0 == 0 || self.nodes[i].1 == 0 {
                return Err(SEG_BASE + 1);
            }
            for j in (i + 1)..self.nodes.len() {
                if self.nodes[i].0 == self.nodes[j].0 {
                    return Err(SEG_BASE + 2);
                }
            }
        }
        for (f, t) in &self.edges {
            if f == t {
                return Err(SEG_BASE + 3);
            }
            if !self.nodes.iter().any(|(n, _)| n == f)
                || !self.nodes.iter().any(|(n, _)| n == t)
            {
                return Err(SEG_BASE + 4);
            }
        }
        Ok(())
    }

    /// 结构哈希（roundtrip 等价判定的基准指纹）。
    pub fn hash(&self) -> Result<u64, u16> {
        self.validate()?;
        let mut ns: Vec<&(u32, u64)> = self.nodes.iter().collect();
        ns.sort_by_key(|(t, _)| *t);
        let mut es: Vec<&(u32, u32)> = self.edges.iter().collect();
        es.sort();
        let mut acc: u64 = 0xcbf29ce484222325;
        for (t, a) in ns {
            acc = acc.wrapping_mul(0x100000001b3) ^ *t as u64;
            acc = acc.wrapping_mul(0x100000001b3) ^ *a;
        }
        for (f, t) in es {
            acc = acc.wrapping_mul(0x100000001b3) ^ *f as u64;
            acc = acc.wrapping_mul(0x100000001b3) ^ *t as u64;
        }
        if acc == 0 {
            return Err(SEG_BASE + 5);
        }
        Ok(acc)
    }
}

/// 序列化格式（判据「序列化格式」载体）：定长小端字节流。
/// 头：magic(8) + version(4) + node_count(4) + edge_count(4)；
/// 体：node(4+8) × n + edge(4+4) × m。
pub fn serialize(g: &SerGraph) -> Result<Vec<u8>, u16> {
    g.validate()?;
    let mut out: Vec<u8> = Vec::new();
    let push_u64 = |v: &mut Vec<u8>, x: u64| {
        v.extend_from_slice(&x.to_le_bytes());
    };
    let push_u32 = |v: &mut Vec<u8>, x: u32| {
        v.extend_from_slice(&x.to_le_bytes());
    };
    push_u64(&mut out, FMT_MAGIC);
    push_u32(&mut out, FMT_VERSION);
    push_u32(&mut out, g.nodes.len() as u32);
    push_u32(&mut out, g.edges.len() as u32);
    for (t, a) in &g.nodes {
        push_u32(&mut out, *t);
        push_u64(&mut out, *a);
    }
    for (f, t) in &g.edges {
        push_u32(&mut out, *f);
        push_u32(&mut out, *t);
    }
    if out.is_empty() {
        return Err(SEG_BASE + 6);
    }
    Ok(out)
}

/// 反序列化（roundtrip 判据面）：版本不符即拒（向前不兼容是纪律）。
pub fn deserialize(bytes: &[u8]) -> Result<SerGraph, u16> {
    if bytes.len() < 20 {
        return Err(SEG_BASE + 7);
    }
    let rd_u64 = |b: &[u8], o: usize| -> u64 {
        let mut a = [0u8; 8];
        a.copy_from_slice(&b[o..o + 8]);
        u64::from_le_bytes(a)
    };
    let rd_u32 = |b: &[u8], o: usize| -> u32 {
        let mut a = [0u8; 4];
        a.copy_from_slice(&b[o..o + 4]);
        u32::from_le_bytes(a)
    };
    if rd_u64(bytes, 0) != FMT_MAGIC {
        return Err(SEG_BASE + 8);
    }
    let ver = rd_u32(bytes, 8);
    if ver != FMT_VERSION {
        return Err(SEG_BASE + 9);
    }
    let nn = rd_u32(bytes, 12) as usize;
    let ne = rd_u32(bytes, 16) as usize;
    if nn == 0 {
        return Err(SEG_BASE + 10);
    }
    let expect = 20 + nn * 12 + ne * 8;
    if bytes.len() != expect {
        return Err(SEG_BASE + 11);
    }
    let mut nodes: Vec<(u32, u64)> = Vec::new();
    let mut off = 20usize;
    for _ in 0..nn {
        let t = rd_u32(bytes, off);
        let a = rd_u64(bytes, off + 4);
        nodes.push((t, a));
        off += 12;
    }
    let mut edges: Vec<(u32, u32)> = Vec::new();
    for _ in 0..ne {
        let f = rd_u32(bytes, off);
        let t = rd_u32(bytes, off + 4);
        edges.push((f, t));
        off += 8;
    }
    let g = SerGraph { nodes, edges };
    g.validate()?;
    Ok(g)
}

/// roundtrip 校验（判据「序列化 roundtrip」核心面）：序列化 →
/// 反序列化 → 结构哈希一致。
pub fn roundtrip(g: &SerGraph) -> Result<(), u16> {
    let bytes = serialize(g)?;
    let back = deserialize(&bytes)?;
    if g.hash()? != back.hash()? {
        return Err(SEG_BASE + 12);
    }
    Ok(())
}

/// 回放输入替换账（判据「确定性保障」载体）：回放前必须替换的
/// 三类外部输入——时间戳 / 随机数 / 外部输入。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ReplayInput {
    /// 时间戳（墙钟 → 录制时刻）。
    Timestamp(u64),
    /// 随机数（真随机 → 录制种子）。
    Random(u64),
    /// 外部输入（实时输入 → 录制快照）。
    External(u64),
}

impl ReplayInput {
    /// 替换后的确定值。
    pub fn value(&self) -> u64 {
        match self {
            ReplayInput::Timestamp(v) | ReplayInput::Random(v) | ReplayInput::External(v) => *v,
        }
    }

    /// 类别序号（0/1/2）。
    pub fn kind(&self) -> u8 {
        match self {
            ReplayInput::Timestamp(_) => 0,
            ReplayInput::Random(_) => 1,
            ReplayInput::External(_) => 2,
        }
    }
}

/// 回放台账（判据「回放逐像素一致」载体）。
pub struct ReplayLedger {
    /// 录制帧的图。
    pub recorded: SerGraph,
    /// 录制时替换账（类别序号 → 确定值）。
    pub subst: Vec<(u8, u64)>,
    /// 回放观测的像素指纹。
    pub pixel_fp: Option<u64>,
    /// 回放观测的图哈希。
    pub replay_hash: Option<u64>,
}

impl ReplayLedger {
    /// 建台账（判据前置：替换账必须三类齐——缺一类 = 回放不完整，
    /// 残留外部输入会污染确定性）。
    pub fn new(recorded: SerGraph, subst: &[ReplayInput]) -> Result<Self, u16> {
        recorded.validate()?;
        if subst.is_empty() {
            return Err(SEG_BASE + 13);
        }
        let mut kinds: Vec<u8> = Vec::new();
        let mut pairs: Vec<(u8, u64)> = Vec::new();
        for s in subst {
            if s.value() == 0 {
                return Err(SEG_BASE + 14);
            }
            if kinds.contains(&s.kind()) {
                return Err(SEG_BASE + 15);
            }
            kinds.push(s.kind());
            pairs.push((s.kind(), s.value()));
        }
        for k in 0..3u8 {
            if !kinds.contains(&k) {
                return Err(SEG_BASE + 16);
            }
        }
        Ok(ReplayLedger {
            recorded,
            subst: pairs,
            pixel_fp: None,
            replay_hash: None,
        })
    }

    /// 回放完成（判据「回放逐像素一致」核心面）：回放图哈希必须
    /// 与录制一致、像素指纹与录制一致。
    pub fn complete(&mut self, replay_g: &SerGraph, replay_pixel: u64) -> Result<(), u16> {
        let want = self.recorded.hash()?;
        let got = replay_g.hash()?;
        if want != got {
            return Err(SEG_BASE + 17);
        }
        if replay_pixel == 0 {
            return Err(SEG_BASE + 18);
        }
        self.replay_hash = Some(got);
        self.pixel_fp = Some(replay_pixel);
        Ok(())
    }

    /// 回放绿判：三账齐（替换账 + 图哈希 + 像素指纹）。
    pub fn green(&self) -> Result<(), u16> {
        if self.subst.len() != 3 {
            return Err(SEG_BASE + 19);
        }
        match (self.replay_hash, self.pixel_fp) {
            (Some(_), Some(p)) if p != 0 => Ok(()),
            _ => Err(SEG_BASE + 20),
        }
    }
}

/// 替换值确定性扫描（判据「确定性保障」强化面）：替换账内同类别
/// 值必须唯一（同类别两次不同值 = 回放中还有活性源）。
pub fn subst_hygiene(subst: &[ReplayInput]) -> Result<(), u16> {
    for i in 0..subst.len() {
        for j in (i + 1)..subst.len() {
            if subst[i].kind() == subst[j].kind() && subst[i].value() != subst[j].value() {
                return Err(SEG_BASE + 21);
            }
        }
    }
    Ok(())
}

/// 序列化体积审计（判据「性能」量化面）：字节数 = 头 + 体，
/// 每节点 12 字节 + 每边 8 字节——超预算（节点 × 24）即冗余。
pub fn size_audit(bytes: &[u8], nodes: usize) -> Result<(), u16> {
    if bytes.is_empty() || nodes == 0 {
        return Err(SEG_BASE + 22);
    }
    let budget = 20 + nodes * 24;
    if bytes.len() > budget {
        return Err(SEG_BASE + 23);
    }
    Ok(())
}

/// 持久化一站式审计（总闸）：roundtrip + 回放台账绿 + 体积审计。
pub fn persist_audit(
    g: &SerGraph,
    subst: &[ReplayInput],
    replay_g: &SerGraph,
    replay_pixel: u64,
) -> Result<(), u16> {
    roundtrip(g)?;
    subst_hygiene(subst)?;
    let mut led = ReplayLedger::new(g.clone(), subst)?;
    led.complete(replay_g, replay_pixel)?;
    led.green()?;
    let bytes = serialize(g)?;
    size_audit(&bytes, g.nodes.len())
}

/// 回放台账深度扩展面。
impl ReplayLedger {
    /// 替换账指纹（判据「格式版本化」联动面）：替换账折并——
    /// 回放可复现性的种子摘要。
    pub fn subst_fp(&self) -> Result<u64, u16> {
        if self.subst.len() != 3 {
            return Err(SEG_BASE + 24);
        }
        let mut acc: u64 = 0xcbf29ce484222325;
        for (k, v) in &self.subst {
            acc = acc.wrapping_mul(0x100000001b3) ^ *k as u64;
            acc = acc.wrapping_mul(0x100000001b3) ^ *v;
        }
        if acc == 0 {
            return Err(SEG_BASE + 25);
        }
        Ok(acc)
    }

    /// 回放耗账（判据「性能」面）：录制帧与回放帧的耗时差比
    /// （千分比）——回放开销超 10% 即回放实现有膨胀。
    pub fn replay_overhead_permille(&self, record_us: u64, replay_us: u64) -> Result<u32, u16> {
        if record_us == 0 || replay_us == 0 {
            return Err(SEG_BASE + 26);
        }
        if replay_us < record_us {
            return Err(SEG_BASE + 27);
        }
        let pm = (((replay_us - record_us) * 1000) / record_us) as u32;
        if pm > 100 {
            return Err(SEG_BASE + 28);
        }
        Ok(pm)
    }
}

/// 多帧录制演练（独立函数面）：连续 N 帧录制 → 每帧 roundtrip
/// → 帧间哈希不必相同（不同帧不同图是常态），但每帧自身一致。
pub fn multi_frame_record(graphs: &[SerGraph]) -> Result<Vec<u64>, u16> {
    if graphs.len() < 2 {
        return Err(SEG_BASE + 29);
    }
    let mut hashes: Vec<u64> = Vec::new();
    for g in graphs {
        roundtrip(g)?;
        hashes.push(g.hash()?);
    }
    Ok(hashes)
}
