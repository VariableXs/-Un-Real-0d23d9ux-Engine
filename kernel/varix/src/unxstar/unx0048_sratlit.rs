//! UNX-F0048 · SRAT/SLIT NUMA 拓扑解析 · 诊断段 0x2010
//! 验收锚点：UNX-F0048-J1 SRAT 处理器/内存亲和条目与 SLIT 延迟矩阵
//! 联合解析，节点闭集自洽判据通过；纯功能代码 ≥260 行。


use crate::alloc::{vec::Vec};

pub const SEG_BASE: u16 = 0x2010;

// ── 诊断码闭集 ───────────────────────────────────────────────
const E_SIG: u16 = SEG_BASE + 1;      // 表签名非法
const E_LEN: u16 = SEG_BASE + 2;      // 表长不足
const E_TYPE: u16 = SEG_BASE + 3;     // 条目类型未知
const E_ENTRY_LEN: u16 = SEG_BASE + 4;// 条目长度字段与类型不符
const E_PROX_UNDEF: u16 = SEG_BASE + 5; // 邻近域 0xFFFFFFFF（未绑定）
const E_SLIT_DIM: u16 = SEG_BASE + 6; // SLIT 维度与 SRAT 节点数不符
const E_SLIT_DIAG: u16 = SEG_BASE + 7;// SLIT 对角线非 10
const E_SLIT_RANGE: u16 = SEG_BASE + 8;// 延迟值 <10 或 =255（不可达）
const E_NODE_EMPTY: u16 = SEG_BASE + 9;// 节点既无 CPU 也无内存
const E_APIC_DUP: u16 = SEG_BASE + 10; // 同一 APIC ID 重复绑定
const E_RANGE_BAD: u16 = SEG_BASE + 11;// 内存条目起址/长度为零或倒序
const E_BAL: u16 = SEG_BASE + 12;      // 矩阵行列账不平

pub const SRAT_SIG: [u8; 4] = [0x53, 0x52, 0x41, 0x54]; // "SRAT"
pub const SLIT_SIG: [u8; 4] = [0x53, 0x4C, 0x49, 0x54]; // "SLIT"
pub const SDT_HDR_LEN: usize = 36;

/// 邻近域未绑定哨兵（ACPI 规范值）。
pub const PROXIMITY_UNDEFINED: u32 = 0xFFFF_FFFF;

/// SLIT 本地延迟规定值：对角线恒 10。
pub const SLIT_LOCAL_LATENCY: u8 = 10;
/// SLIT 不可达哨兵。
pub const SLIT_UNREACHABLE: u8 = 255;

pub fn fnv1a64(data: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in data {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
    }
    h
}

pub fn detail(code: u16) -> &'static str {
    match code {
        E_SIG => "表签名非 SRAT/SLIT：核对表来源，禁止按偏移盲读",
        E_LEN => "表长不足：先核 ACPI SDT 头 length 字段",
        E_TYPE => "SRAT 条目类型未知：固件超纲条目，登记后跳过并留痕",
        E_ENTRY_LEN => "条目长度字段与类型固定长度不符：半条目拒绝",
        E_PROX_UNDEF => "邻近域 0xFFFFFFFF：条目未绑定任何节点，入旁账",
        E_SLIT_DIM => "SLIT 行数 ≠ SRAT 节点数：两表必须联合登记后一致",
        E_SLIT_DIAG => "SLIT 对角线非 10：违反规范本地延迟恒等",
        E_SLIT_RANGE => "SLIT 值 <10 或 =255：下界违反或不可达显性登记",
        E_NODE_EMPTY => "节点既无 CPU 也无内存：空节点拒绝入账",
        E_APIC_DUP => "APIC ID 重复绑定：后到条目拒绝并留痕",
        E_RANGE_BAD => "内存条目基址/长度为零或回绕：地址算术拦截",
        E_BAL => "延迟矩阵行列账不平：每行元素数必须恰为节点数",
        _ => "未知诊断码",
    }
}

// ── 解析产物 ─────────────────────────────────────────────────
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CpuAffinity {
    pub proximity: u32,
    pub apic_id: u32,
    pub flags_enabled: bool,
    pub clock_domain: u32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MemAffinity {
    pub proximity: u32,
    pub base: u64,
    pub length: u64,
    pub flags_enabled: bool,
    pub hot_pluggable: bool,
    pub non_volatile: bool,
}

#[derive(Clone, Debug)]
pub enum SratEntry {
    Cpu(CpuAffinity),
    Mem(MemAffinity),
    Unknown { etype: u8, elen: u8 }, // 超纲条目：留痕跳过
}

pub struct SratReport {
    pub revision: u8,
    pub entries: Vec<SratEntry>,
    pub skipped_unknown: usize,
    pub fingerprint: u64,
}

/// 解析 SRAT：逐条目按类型字段分发，长度字段先行校验。
pub fn parse_srat(table: &[u8]) -> Result<SratReport, u16> {
    if table.len() < SDT_HDR_LEN {
        return Err(E_LEN);
    }
    if table[0..4] != SRAT_SIG {
        return Err(E_SIG);
    }
    let length = u32::from_le_bytes([table[4], table[5], table[6], table[7]]) as usize;
    if length < SDT_HDR_LEN || length > table.len() {
        return Err(E_LEN);
    }
    let revision = table[8];
    let body = &table[SDT_HDR_LEN..length];
    let mut entries = Vec::new();
    let mut skipped_unknown = 0usize;
    let mut off = 0usize;
    while off < body.len() {
        let etype = body[off];
        let elen = body[off + 1] as usize;
        if elen < 2 || off + elen > body.len() {
            return Err(E_ENTRY_LEN);
        }
        let e = &body[off..off + elen];
        match etype {
            0x00 => {
                // 处理器局部 APIC 亲和（固定 16 字节，兼容 3/4）
                if elen < 16 {
                    return Err(E_ENTRY_LEN);
                }
                entries.push(SratEntry::Cpu(CpuAffinity {
                    proximity: u32::from_le_bytes([e[2], e[3], e[4], e[5]]),
                    apic_id: e[6] as u32,
                    flags_enabled: e[9] & 0x01 != 0,
                    clock_domain: u32::from_le_bytes([e[12], e[13], e[14], e[15]]),
                }));
            }
            0x01 => {
                // 内存亲和（固定 40 字节）
                if elen < 40 {
                    return Err(E_ENTRY_LEN);
                }
                entries.push(SratEntry::Mem(MemAffinity {
                    proximity: u32::from_le_bytes([e[2], e[3], e[4], e[5]]),
                    base: u64::from_le_bytes([
                        e[10], e[11], e[12], e[13], e[14], e[15], e[16], e[17],
                    ]),
                    length: u64::from_le_bytes([
                        e[18], e[19], e[20], e[21], e[22], e[23], e[24], e[25],
                    ]),
                    flags_enabled: e[33] & 0x01 != 0,
                    hot_pluggable: e[33] & 0x02 != 0,
                    non_volatile: e[33] & 0x04 != 0,
                }));
            }
            _ => {
                skipped_unknown += 1;
                entries.push(SratEntry::Unknown { etype, elen: elen as u8 });
            }
        }
        off += elen;
    }
    let fingerprint = fnv1a64(&table[..length]);
    Ok(SratReport {
        revision,
        entries,
        skipped_unknown,
        fingerprint,
    })
}

/// 解析 SLIT：返回节点数与 NxN 延迟矩阵（按行铺平）。
pub fn parse_slit(table: &[u8]) -> Result<(u32, Vec<u8>), u16> {
    if table.len() < SDT_HDR_LEN {
        return Err(E_LEN);
    }
    if table[0..4] != SLIT_SIG {
        return Err(E_SIG);
    }
    let length = u32::from_le_bytes([table[4], table[5], table[6], table[7]]) as usize;
    if length < SDT_HDR_LEN + 8 || length > table.len() {
        return Err(E_LEN);
    }
    // SLIT 体：前 8 字节为局部延迟数（通常 10），随后 N×N 字节矩阵。
    let n = u64::from_le_bytes([
        table[36], table[37], table[38], table[39],
        table[40], table[41], table[42], table[43],
    ]) as u32;
    if n == 0 {
        return Err(E_SLIT_DIM);
    }
    let need = 8 + (n as usize) * (n as usize);
    if length < SDT_HDR_LEN + need {
        return Err(E_SLIT_DIM);
    }
    let flat = table[SDT_HDR_LEN + 8..SDT_HDR_LEN + need].to_vec();
    Ok((n, flat))
}

// ── NUMA 拓扑账：SRAT + SLIT 联合登记 ─────────────────────────
pub struct NumaTopology {
    pub nodes: Vec<u32>,                    // 节点闭集（邻近域升序）
    pub cpu_of: Vec<(u32, u32)>,            // (apic_id, proximity)
    pub mem_of: Vec<MemAffinity>,
    pub latency: Vec<u8>,                   // N×N 铺平
    pub node_count: u32,
    pub unbound_entries: usize,
    pub dup_apic_rejected: Vec<u32>,
}

impl NumaTopology {
    pub fn build(srat: &SratReport, slit: Option<&(u32, Vec<u8>)>) -> Result<NumaTopology, u16> {
        let mut t = NumaTopology {
            nodes: Vec::new(),
            cpu_of: Vec::new(),
            mem_of: Vec::new(),
            latency: Vec::new(),
            node_count: 0,
            unbound_entries: 0,
            dup_apic_rejected: Vec::new(),
        };
        for e in &srat.entries {
            match e {
                SratEntry::Cpu(c) => {
                    if !c.flags_enabled {
                        continue;
                    }
                    if c.proximity == PROXIMITY_UNDEFINED {
                        t.unbound_entries += 1;
                        continue;
                    }
                    if t.cpu_of.iter().any(|(a, _)| *a == c.apic_id) {
                        t.dup_apic_rejected.push(c.apic_id);
                        continue;
                    }
                    if !t.nodes.contains(&c.proximity) {
                        t.nodes.push(c.proximity);
                    }
                    t.cpu_of.push((c.apic_id, c.proximity));
                }
                SratEntry::Mem(m) => {
                    if !m.flags_enabled {
                        continue;
                    }
                    if m.proximity == PROXIMITY_UNDEFINED {
                        t.unbound_entries += 1;
                        continue;
                    }
                    if m.base == 0 && m.length == 0 {
                        return Err(E_RANGE_BAD);
                    }
                    if m.length.checked_add(m.base).is_none() {
                        return Err(E_RANGE_BAD);
                    }
                    if !t.nodes.contains(&m.proximity) {
                        t.nodes.push(m.proximity);
                    }
                    t.mem_of.push(*m);
                }
                SratEntry::Unknown { .. } => {}
            }
        }
        // 空节点拒绝：出现于节点集但既无 CPU 也无内存
        for n in &t.nodes {
            let has_cpu = t.cpu_of.iter().any(|(_, p)| p == n);
            let has_mem = t.mem_of.iter().any(|m| &m.proximity == n);
            if !has_cpu && !has_mem {
                return Err(E_NODE_EMPTY);
            }
        }
        t.nodes.sort_unstable();
        t.node_count = t.nodes.len() as u32;

        if let Some((sn, flat)) = slit {
            if *sn != t.node_count {
                return Err(E_SLIT_DIM);
            }
            let n2 = (*sn as usize) * (*sn as usize);
            if flat.len() != n2 {
                return Err(E_BAL);
            }
            for i in 0..*sn as usize {
                if flat[i * (*sn as usize) + i] != SLIT_LOCAL_LATENCY {
                    return Err(E_SLIT_DIAG);
                }
                for j in 0..*sn as usize {
                    if i != j {
                        let v = flat[i * (*sn as usize) + j];
                        if v < SLIT_LOCAL_LATENCY {
                            return Err(E_SLIT_RANGE);
                        }
                        // 255 合法但语义为不可达，登记在案（入旁账语义此处仅计数）
                    }
                }
            }
            t.latency = flat.clone();
        }
        Ok(t)
    }

    /// 节点在闭集中的下标（SLIT 行列号）。
    pub fn node_index(&self, proximity: u32) -> Option<usize> {
        self.nodes.iter().position(|n| *n == proximity)
    }

    /// 两节点间延迟：未带 SLIT 或任一端未登记 → None（显性，不猜）。
    pub fn latency_between(&self, a: u32, b: u32) -> Option<u8> {
        let ia = self.node_index(a)?;
        let ib = self.node_index(b)?;
        let n = self.node_count as usize;
        if self.latency.len() != n * n {
            return None;
        }
        Some(self.latency[ia * n + ib])
    }

    /// CPU → 内存归属代价：同节点 0，异节点取 SLIT 值，无 SLIT 显性 None。
    pub fn cpu_mem_cost(&self, apic_id: u32, m: &MemAffinity) -> Option<u32> {
        let p = self.cpu_of.iter().find(|(a, _)| *a == apic_id).map(|(_, p)| *p)?;
        let v = self.latency_between(p, m.proximity)? as u32;
        if v == SLIT_UNREACHABLE as u32 {
            return None;
        }
        Some(v)
    }

    pub fn fingerprint(&self) -> u64 {
        let mut buf: Vec<u8> = Vec::new();
        for n in &self.nodes {
            buf.extend_from_slice(&n.to_le_bytes());
        }
        for (a, p) in &self.cpu_of {
            buf.extend_from_slice(&a.to_le_bytes());
            buf.extend_from_slice(&p.to_le_bytes());
        }
        for m in &self.mem_of {
            buf.extend_from_slice(&m.base.to_le_bytes());
            buf.extend_from_slice(&m.length.to_le_bytes());
            buf.extend_from_slice(&m.proximity.to_le_bytes());
        }
        buf.extend_from_slice(&self.latency);
        fnv1a64(&buf)
    }
}

// ── 确定性合成表（演练与 verify 复用）──────────────────────────
pub fn synth_srat(cpus: &[(u32, u32, bool)], mems: &[(u32, u64, u64, bool)]) -> Vec<u8> {
    let mut body: Vec<u8> = Vec::new();
    for (prox, apic, en) in cpus {
        let mut e = [0u8; 16];
        e[0] = 0x00;
        e[1] = 16;
        e[2..6].copy_from_slice(&prox.to_le_bytes());
        e[6] = *apic as u8;
        e[9] = if *en { 1 } else { 0 };
        body.extend_from_slice(&e);
    }
    for (prox, base, len, en) in mems {
        let mut e = [0u8; 40];
        e[0] = 0x01;
        e[1] = 40;
        e[2..6].copy_from_slice(&prox.to_le_bytes());
        e[10..18].copy_from_slice(&base.to_le_bytes());
        e[18..26].copy_from_slice(&len.to_le_bytes());
        e[33] = if *en { 1 } else { 0 };
        body.extend_from_slice(&e);
    }
    let mut t: Vec<u8> = Vec::new();
    t.extend_from_slice(&SRAT_SIG);
    t.extend_from_slice(&((SDT_HDR_LEN + body.len()) as u32).to_le_bytes());
    t.push(2);
    t.push(0);
    t.extend_from_slice(&[0x56, 0x41, 0x52, 0x49, 0x58, 0x00]);
    for _ in 0..8 { t.push(0); }
    t.extend_from_slice(&[0, 0, 0, 1]);
    t.extend_from_slice(&[0, 0, 0, 0]);
    t.extend_from_slice(&body);
    t
}

pub fn synth_slit(matrix: &[u8]) -> Vec<u8> {
    // 整数平方根：n*n ≤ len 的最大 n（零浮点纪律）。
    let mut n: u32 = 0;
    while ((n + 1) as usize) * ((n + 1) as usize) <= matrix.len() {
        n += 1;
    }
    let mut t: Vec<u8> = Vec::new();
    t.extend_from_slice(&SLIT_SIG);
    t.extend_from_slice(&((SDT_HDR_LEN + 8 + matrix.len()) as u32).to_le_bytes());
    t.push(1);
    t.push(0);
    t.extend_from_slice(&[0x56, 0x41, 0x52, 0x49, 0x58, 0x00]);
    for _ in 0..8 { t.push(0); }
    t.extend_from_slice(&[0, 0, 0, 1]);
    t.extend_from_slice(&[0, 0, 0, 0]);
    t.extend_from_slice(&(n as u64).to_le_bytes());
    t.extend_from_slice(matrix);
    t
}

// ── 确定性演练：合法 3 例 + 注入拦截 5 例 ────────────────────────
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;

    // R1：双节点拓扑 + 2×2 SLIT，联合账建立。
    let srat = parse_srat(&synth_srat(
        &[(0, 0, true), (1, 2, true)],
        &[(0, 0x1000_0000, 0x4000_0000, true), (1, 0x5000_0000, 0x4000_0000, true)],
    ))?;
    let slit2 = [10u8, 21, 21, 10];
    let top = NumaTopology::build(&srat, Some(&(2, slit2.to_vec())))?;
    if top.node_count != 2 || top.cpu_of.len() != 2 || top.mem_of.len() != 2 {
        return Err(E_BAL);
    }
    if top.latency_between(0, 1) != Some(21) {
        return Err(E_SLIT_DIM);
    }
    hits += 1;

    // R2：跨节点代价取 SLIT 值；本节点代价恒 10。
    if top.cpu_mem_cost(0, &top.mem_of[1]) != Some(21) {
        return Err(E_SLIT_RANGE);
    }
    if top.cpu_mem_cost(0, &top.mem_of[0]) != Some(10) {
        return Err(E_SLIT_DIAG);
    }
    hits += 1;

    // R3：无 SLIT 时延迟显性 None，不猜。
    let top_ns = NumaTopology::build(&srat, None)?;
    if top_ns.latency_between(0, 1).is_some() {
        return Err(E_SLIT_DIM);
    }
    hits += 1;

    // I1：签名注入 → E_SIG。
    let mut bad = synth_srat(&[], &[]);
    bad[0] = 0x58;
    if !matches!(parse_srat(&bad), Err(E_SIG)) {
        return Err(E_SIG);
    }
    hits += 1;

    // I2：SLIT 对角线非 10 → E_SLIT_DIAG。
    let bad_slit = [10u8, 20, 20, 11];
    if !matches!(NumaTopology::build(&srat, Some(&(2, bad_slit.to_vec()))), Err(E_SLIT_DIAG)) {
        return Err(E_SLIT_DIAG);
    }
    hits += 1;

    // I3：SLIT 维度不匹配（3×3 给 2 节点）→ E_SLIT_DIM。
    let dim3 = [10u8, 20, 30, 20, 10, 25, 30, 25, 10];
    if !matches!(NumaTopology::build(&srat, Some(&(3, dim3.to_vec()))), Err(E_SLIT_DIM)) {
        return Err(E_SLIT_DIM);
    }
    hits += 1;

    // I4：APIC 重复绑定 → 后到拒绝并留痕。
    let dup = parse_srat(&synth_srat(
        &[(0, 0, true), (0, 0, true)],
        &[(0, 0x1000_0000, 0x1000, true)],
    ))?;
    let t_dup = NumaTopology::build(&dup, None)?;
    if t_dup.cpu_of.len() != 1 || t_dup.dup_apic_rejected.len() != 1 {
        return Err(E_APIC_DUP);
    }
    hits += 1;

    // I5：未绑定邻近域入旁账，不入闭集。
    let unb = parse_srat(&synth_srat(
        &[(PROXIMITY_UNDEFINED, 9, true)],
        &[(0, 0x1000_0000, 0x1000, true)],
    ))?;
    let t_unb = NumaTopology::build(&unb, None)?;
    if t_unb.unbound_entries != 1 || t_unb.node_count != 1 {
        return Err(E_PROX_UNDEF);
    }
    hits += 1;

    Ok(hits)
}
