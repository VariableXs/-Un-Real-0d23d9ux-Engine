//! UNX-F0151 · 写盘工具对接口（只列清单，写盘行为在 L1）· 诊断段 0x25B0
//!
//! 判据原文（UNX-F0151-J1）：内核侧只产出「写盘指令清单」（目标、
//! 偏移、长度、校验），真实写盘行为在 L1 执行；清单账守恒
//! （Σ长度 = Σ区间），区间重叠即红（红线：防跨区误写）。
//!
//! 设计要点：写盘清单册（WritePlan）——每条指令含目标盘标 + 偏移 +
//! 长度 + 内容 CRC8；区间账：同盘指令两两不相交；导出规范化
//! 清单文本。零浮点、零 unwrap/expect。

use crate::alloc::{format, string::{String, ToString}, vec::Vec};

pub const SEG_BASE: u16 = 0x25C0;

const E_RANGE: u16 = SEG_BASE + 1; // 偏移/长度非法（零长或溢出）
const E_OVERLAP: u16 = SEG_BASE + 2; // 同盘区间重叠（红线）
const E_BAL: u16 = SEG_BASE + 3; // 清单账守恒被破坏
const E_SEAL: u16 = SEG_BASE + 4; // 册已封拒写
const E_DISK: u16 = SEG_BASE + 5; // 盘标越纲

/// 内容 CRC8（多项式 0x07）。
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

/// 写盘指令。
#[derive(Clone)]
pub struct WriteCmd {
    pub disk: u8, // 盘标（0~3）
    pub offset: u64, // 字节偏移
    pub len: u64,
    pub payload_crc: u8,
    pub tag: String, // 语义标签（如 "stage2"）
}

impl WriteCmd {
    /// 指令终点（开区间右端）。
    pub fn end(&self) -> u64 {
        self.offset + self.len
    }

    /// 与他指令同盘相交判定。
    pub fn overlaps(&self, other: &WriteCmd) -> bool {
        self.disk == other.disk && self.offset < other.end() && other.offset < self.end()
    }
}

/// 写盘清单册。
pub struct WritePlan {
    pub cmds: Vec<WriteCmd>,
    pub sealed: bool,
}

impl WritePlan {
    pub fn new() -> Result<Self, u16> {
        Ok(WritePlan { cmds: Vec::new(), sealed: false })
    }

    /// 追加指令（零长/盘标越纲/重叠即拒）。
    pub fn add(
        &mut self,
        disk: u8,
        offset: u64,
        payload: &[u8],
        tag: &str,
    ) -> Result<(), u16> {
        if self.sealed {
            return Err(E_SEAL);
        }
        if disk > 3 {
            return Err(E_DISK);
        }
        if payload.is_empty() || offset == u64::MAX {
            return Err(E_RANGE);
        }
        let candidate = WriteCmd {
            disk,
            offset,
            len: payload.len() as u64,
            payload_crc: crc8(payload),
            tag: tag.to_string(),
        };
        for c in &self.cmds {
            if candidate.overlaps(c) {
                return Err(E_OVERLAP); // 红线：跨区误写防护
            }
        }
        self.cmds.push(candidate);
        Ok(())
    }

    /// 封册。
    pub fn seal(&mut self) {
        self.sealed = true;
    }

    /// 清单账守恒：同盘区间两两不交（追加时已拦，此处全量复核）。
    pub fn audit(&self) -> Result<(), u16> {
        for i in 0..self.cmds.len() {
            for j in (i + 1)..self.cmds.len() {
                if self.cmds[i].overlaps(&self.cmds[j]) {
                    return Err(E_OVERLAP);
                }
            }
        }
        Ok(())
    }

    /// 导出规范化清单文本（按盘标 + 偏移升序）。
    pub fn export_sorted(&self) -> Vec<String> {
        let mut idx: Vec<usize> = (0..self.cmds.len()).collect();
        idx.sort_by(|&a, &b| {
            self.cmds[a]
                .disk
                .cmp(&self.cmds[b].disk)
                .then(self.cmds[a].offset.cmp(&self.cmds[b].offset))
        });
        let mut out = Vec::new();
        for i in idx {
            let c = &self.cmds[i];
            out.push(format!(
                "盘{} @{} +{} crc{:02X} {}",
                c.disk, c.offset, c.len, c.payload_crc, c.tag
            ));
        }
        out
    }
}

/// 汇总账：Σ长度按盘分账。
pub fn bytes_by_disk(plan: &WritePlan) -> Result<[u64; 4], u16> {
    plan.audit()?;
    let mut acc = [0u64; 4];
    for c in &plan.cmds {
        acc[c.disk as usize] += c.len;
    }
    Ok(acc)
}

// ── 确定性演练：合法 4 例 + 注入拦截 4 例 ─────────────────────────

/// 演练入口（ktest 挂点）。
pub fn drill() -> Result<(), u16> {
    let mut hits = 0u32;

    // L1：多盘多区指令追加 → 审计绿 → 导出有序。
    let mut p = WritePlan::new()?;
    p.add(0, 0x200, b"STAGE2-LOADER", "stage2")?;
    p.add(0, 0x1000, b"KERNEL-BIN", "kernel")?;
    p.add(1, 0x0, b"CFG", "config")?;
    p.audit()?;
    let exp = p.export_sorted();
    if exp.len() != 3 || !exp[0].contains("config") {
        return Err(E_BAL);
    }
    hits += 1;

    // L2：分盘守恒账——盘 0 两段合计、盘 1 一段。
    let acc = bytes_by_disk(&p)?;
    if acc[0] != ("STAGE2-LOADER".len() + "KERNEL-BIN".len()) as u64 || acc[1] != 3 {
        return Err(E_BAL);
    }
    hits += 1;

    // L3：同盘相邻不重叠（紧贴边界）合法。
    let mut p3 = WritePlan::new()?;
    p3.add(2, 0, b"AAAA", "a")?;
    p3.add(2, 4, b"BBBB", "b")?;
    p3.audit()?;
    hits += 1;

    // L4：CRC 指纹按内容定（同内容同 CRC）。
    let mut p4 = WritePlan::new()?;
    p4.add(0, 0, b"XX", "x")?;
    p4.add(1, 0, b"XX", "x2")?;
    if p4.cmds[0].payload_crc != p4.cmds[1].payload_crc {
        return Err(E_BAL);
    }
    hits += 1;

    // I1：同盘重叠拒（部分交叠）。
    let mut i1 = WritePlan::new()?;
    i1.add(0, 100, b"0123456789", "a")?;
    if i1.add(0, 105, b"0123456789", "b") != Err(E_OVERLAP) {
        return Err(E_OVERLAP);
    }
    hits += 1;

    // I2：同盘完全包含区间拒。
    if i1.add(0, 102, b"01", "c") != Err(E_OVERLAP) {
        return Err(E_OVERLAP);
    }
    hits += 1;

    // I3：零长与盘标越纲双拒。
    let mut i3 = WritePlan::new()?;
    if i3.add(0, 0, b"", "z") != Err(E_RANGE) {
        return Err(E_RANGE);
    }
    if i3.add(9, 0, b"x", "z") != Err(E_DISK) {
        return Err(E_DISK);
    }
    hits += 1;

    // I4：封册拒写 + 账注入（手工造重叠 audit 必红）。
    let mut i4 = WritePlan::new()?;
    i4.add(0, 0, b"A", "a")?;
    i4.seal();
    if i4.add(0, 8, b"B", "b") != Err(E_SEAL) {
        return Err(E_SEAL);
    }
    i4.sealed = false;
    i4.add(0, 8, b"B", "b")?;
    i4.cmds[1].offset = 0; // 破账：造重叠
    if i4.audit() != Err(E_OVERLAP) {
        return Err(E_OVERLAP);
    }
    hits += 1;

    let _ = hits;
    Ok(())
}
