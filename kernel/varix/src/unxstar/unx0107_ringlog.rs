//! UNX-F0107 · 日志环形缓冲持久化 · 诊断段 0x2350
//! 验收锚点：UNX-F0107-J1 缓冲满覆盖策略生效，重启后最近 N 条
//! 可从保留区找回。零浮点、零 unwrap、零 expect。


use alloc::format;
use alloc::vec::Vec;
use alloc::string::{String, ToString};

pub const SEG_BASE: u16 = 0x2350;

// ── 诊断码闭集 ───────────────────────────────────────────────
const E_LEN: u16 = SEG_BASE + 1;    // 记录超长（槽容量外）
const E_EMPTY: u16 = SEG_BASE + 2;  // 空环出报告 / 快照
const E_CRC: u16 = SEG_BASE + 3;    // 槽校验和不符（记录被蚀）
const E_SEQ: u16 = SEG_BASE + 4;    // 序号断档（找回账不连续）
const E_CAP: u16 = SEG_BASE + 5;    // 容量非法（零或超上限）
const E_RDONLY: u16 = SEG_BASE + 6; // 已快照，再快照被拒
const E_BAL: u16 = SEG_BASE + 7;    // 找回账不平（N 条 ≠ 有效槽数）

pub fn detail(code: u16) -> &'static str {
    match code {
        E_LEN => "记录超长：有效载荷须 ≤ 槽容量",
        E_EMPTY => "空环：无记录禁止出报告或快照",
        E_CRC => "槽校验和不符：记录被覆盖蚀或损坏",
        E_SEQ => "序号断档：找回窗内序号不连续",
        E_CAP => "容量非法：槽数须为正且不超 4096 上限",
        E_RDONLY => "已快照：一环一快照",
        E_BAL => "找回账不平：有效槽数与序号窗不符",
        _ => "未知诊断码",
    }
}

// ── 环形缓冲常量 ─────────────────────────────────────────────
pub const SLOT_PAYLOAD: usize = 24; // 每槽载荷 24 字节
pub const CAP_MAX: usize = 4096;

/// FNV-1a 8 位槽校验（记录完整性根函数）。
pub fn crc8(data: &[u8]) -> u8 {
    let mut h: u8 = 0x5A;
    for b in data {
        h ^= *b;
        h = h.rotate_left(1);
        h = h.wrapping_add(0x1B);
    }
    h
}

/// 环槽：seq + 载荷 + crc8。
#[derive(Clone)]
pub struct Slot {
    pub seq: u64,
    pub payload: Vec<u8>,
    pub crc: u8,
}

impl Slot {
    pub fn seal(seq: u64, payload: &[u8]) -> Result<Slot, u16> {
        if payload.len() > SLOT_PAYLOAD {
            return Err(E_LEN);
        }
        Ok(Slot {
            seq,
            payload: payload.to_vec(),
            crc: crc8(payload),
        })
    }

    pub fn intact(&self) -> bool {
        crc8(&self.payload) == self.crc
    }
}

/// 环形日志：满覆盖策略 + 快照找回。
pub struct RingLog {
    pub cap: usize,
    pub slots: Vec<Option<Slot>>, // 物理环
    pub head: usize,              // 下一写位置
    pub total: u64,               // 累计写入条数
    pub snapshotted: bool,
}

impl RingLog {
    pub fn new(cap: usize) -> Result<RingLog, u16> {
        if cap == 0 || cap > CAP_MAX {
            return Err(E_CAP);
        }
        Ok(RingLog {
            cap,
            slots: (0..cap).map(|_| None).collect(),
            head: 0,
            total: 0,
            snapshotted: false,
        })
    }

    /// 写一条：满则覆盖最老（head 即最老位），序号单调递增。
    pub fn push(&mut self, payload: &[u8]) -> Result<u64, u16> {
        if payload.len() > SLOT_PAYLOAD {
            return Err(E_LEN);
        }
        let seq = self.total + 1; // 序号从 1 起
        self.slots[self.head] = Some(Slot::seal(seq, payload)?);
        self.head = (self.head + 1) % self.cap;
        self.total = seq;
        Ok(seq)
    }

    /// 有效槽数（未被覆盖出局的）。
    pub fn valid_count(&self) -> usize {
        self.slots.iter().filter(|s| s.is_some()).count()
    }

    /// 按序号升序取有效记录（从最老到最新）。
    pub fn ordered(&self) -> Result<Vec<&Slot>, u16> {
        if self.total == 0 {
            return Err(E_EMPTY);
        }
        let mut out: Vec<&Slot> = Vec::new();
        let start = if self.total as usize <= self.cap {
            0
        } else {
            self.head
        };
        let n = self.valid_count();
        for i in 0..n {
            let idx = (start + i) % self.cap;
            if let Some(s) = &self.slots[idx] {
                out.push(s);
            }
        }
        Ok(out)
    }

    /// 快照到保留区：序列化全部有效槽（seq+payload+crc）。
    /// 返回保留区镜像字节账。
    pub fn snapshot(&mut self) -> Result<Vec<u8>, u16> {
        if self.snapshotted {
            return Err(E_RDONLY);
        }
        if self.total == 0 {
            return Err(E_EMPTY);
        }
        let mut img: Vec<u8> = Vec::new();
        let slots = self.ordered()?;
        let n = slots.len() as u64;
        img.extend_from_slice(&n.to_le_bytes());
        for s in slots {
            img.extend_from_slice(&s.seq.to_le_bytes());
            img.extend_from_slice(&s.crc.to_le_bytes());
            let mut p = s.payload.clone();
            p.resize(SLOT_PAYLOAD, 0);
            img.extend_from_slice(&p);
        }
        self.snapshotted = true;
        Ok(img)
    }

    /// 重启找回：从保留区镜像重建环账，逐槽校验 crc 与序号连续。
    pub fn restore(img: &[u8]) -> Result<Vec<Slot>, u16> {
        if img.len() < 8 {
            return Err(E_EMPTY);
        }
        let rd64 = |o: usize| -> u64 {
            let mut b = [0u8; 8];
            b.copy_from_slice(&img[o..o + 8]);
            u64::from_le_bytes(b)
        };
        let n = rd64(0) as usize;
        let stride = 8 + 1 + SLOT_PAYLOAD;
        if img.len() < 8 + n * stride {
            return Err(E_CRC);
        }
        let mut out: Vec<Slot> = Vec::new();
        let mut expect_seq = 0u64;
        for i in 0..n {
            let base = 8 + i * stride;
            let seq = rd64(base);
            let crc = img[base + 8];
            let payload = img[base + 9..base + 9 + SLOT_PAYLOAD].to_vec();
            let s = Slot { seq, payload, crc };
            if !s.intact() {
                return Err(E_CRC);
            }
            if expect_seq != 0 && seq != expect_seq {
                return Err(E_SEQ);
            }
            expect_seq = seq + 1;
            out.push(s);
        }
        if out.is_empty() {
            return Err(E_EMPTY);
        }
        Ok(out)
    }

    /// 找回账核：镜像条数 == 有效槽数。
    pub fn audit_recovered(&self, n_recovered: usize) -> Result<(), u16> {
        if n_recovered != self.valid_count() {
            return Err(E_BAL);
        }
        Ok(())
    }

    pub fn render(&self) -> String {
        format!(
            "环 {} 槽，写 {} 条，在环 {} 条，快照 {}",
            self.cap,
            self.total,
            self.valid_count(),
            if self.snapshotted { "已" } else { "未" }
        )
    }
}

// ── 确定性演练：合法 4 例 + 注入拦截 5 例 ────────────────────────
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;

    // R1：4 槽环写 6 条 → 满覆盖生效，在环 4 条为第 3~6 号。
    let mut r = RingLog::new(4)?;
    for i in 1..=6u64 {
        r.push(format!("rec-{}", i).as_bytes())?;
    }
    if r.total != 6 || r.valid_count() != 4 {
        return Err(E_BAL);
    }
    let ord = r.ordered()?;
    if ord[0].seq != 3 || ord[3].seq != 6 {
        return Err(E_SEQ);
    }
    hits += 1;

    // R2：快照 → 模拟重启 → 保留区找回最近 4 条，crc 全过、序号连续。
    let img = r.snapshot()?;
    let rec = RingLog::restore(&img)?;
    if rec.len() != 4 {
        return Err(E_BAL);
    }
    r.audit_recovered(rec.len())?;
    if rec[0].seq != 3 || rec[3].seq != 6 {
        return Err(E_SEQ);
    }
    hits += 1;

    // R3：不满环（2/4）快照找回 → 恰 2 条。
    let mut r3 = RingLog::new(4)?;
    r3.push(b"a")?;
    r3.push(b"bb")?;
    let img3 = r3.snapshot()?;
    let rec3 = RingLog::restore(&img3)?;
    if rec3.len() != 2 || rec3[1].seq != 2 {
        return Err(E_BAL);
    }
    hits += 1;

    // R4：渲染账含容量与在环数。
    let rep = r.render();
    if !rep.contains("环 4 槽") || !rep.contains("写 6 条") || !rep.contains("在环 4 条") {
        return Err(E_BAL);
    }
    hits += 1;

    // I1：容量非法（零 / 超上限）→ E_CAP。
    if RingLog::new(0).is_ok() {
        return Err(E_CAP);
    }
    if RingLog::new(CAP_MAX + 1).is_ok() {
        return Err(E_CAP);
    }
    hits += 1;

    // I2：记录超长（25 字节 > 槽容量）→ E_LEN（push 与 seal 两路）。
    let mut i2 = RingLog::new(2)?;
    let long = [0u8; SLOT_PAYLOAD + 1];
    if !matches!(i2.push(&long), Err(E_LEN)) {
        return Err(E_LEN);
    }
    if !matches!(Slot::seal(1, &long), Err(E_LEN)) {
        return Err(E_LEN);
    }
    hits += 1;

    // I3：空环出报告/快照 → E_EMPTY；空镜像找回 → E_EMPTY。
    let i3 = RingLog::new(2)?;
    if !matches!(i3.ordered(), Err(E_EMPTY)) {
        return Err(E_EMPTY);
    }
    let mut i3b = RingLog::new(2)?;
    if !matches!(i3b.snapshot(), Err(E_EMPTY)) {
        return Err(E_EMPTY);
    }
    if !matches!(RingLog::restore(&[]), Err(E_EMPTY)) {
        return Err(E_EMPTY);
    }
    hits += 1;

    // I4：镜像槽位蚀坏 → E_CRC。
    let mut i4 = RingLog::new(2)?;
    i4.push(b"good")?;
    let mut img4 = i4.snapshot()?;
    let n = img4.len();
    img4[n - 1] ^= 0xFF; // 蚀最后一槽载荷
    if !matches!(RingLog::restore(&img4), Err(E_CRC)) {
        return Err(E_CRC);
    }
    hits += 1;

    // I5：镜像序号断档 → E_SEQ；重复快照 → E_RDONLY。
    let mut i5 = RingLog::new(4)?;
    i5.push(b"x")?;
    i5.push(b"y")?;
    let mut img5 = i5.snapshot()?;
    // 篡改第二槽 seq（8 字节小端在 base=8+stride 处），蚀出断档。
    let stride = 8 + 1 + SLOT_PAYLOAD;
    let base2 = 8 + stride;
    img5[base2] ^= 0x01; // seq 2 → 3，断档
    if !matches!(RingLog::restore(&img5), Err(E_SEQ)) {
        return Err(E_SEQ);
    }
    if !matches!(i5.snapshot(), Err(E_RDONLY)) {
        return Err(E_RDONLY);
    }
    hits += 1;

    Ok(hits)
}
