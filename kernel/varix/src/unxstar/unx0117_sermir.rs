//! UNX-F0117 · 串口远程日志只读镜像 · 诊断段 0x23F0
//! 验收锚点：UNX-F0117-J1 内核日志流镜像至串口帧序，帧账守恒、
//! 只读语义（写请求全拒）。零浮点、零 unwrap、零 expect。


use crate::alloc::{format, string::{String, ToString}, vec::Vec};

pub const SEG_BASE: u16 = 0x23F0;

// ── 诊断码闭集 ───────────────────────────────────────────────
const E_FRAME: u16 = SEG_BASE + 1; // 帧格式非法（无 STX/ETX 或段残缺）
const E_RDONLY: u16 = SEG_BASE + 2;// 镜像只读：写/删/改请求全拒
const E_SEQ: u16 = SEG_BASE + 3;   // 帧序断档（重放窗口内不连续）
const E_CRC: u16 = SEG_BASE + 4;   // 帧校验和不符
const E_LEN: u16 = SEG_BASE + 5;   // 帧载荷超长（64 字节）
const E_BAL: u16 = SEG_BASE + 6;   // 镜像账不平（收≠镜像）

pub fn detail(code: u16) -> &'static str {
    match code {
        E_FRAME => "帧格式非法：须 STX seq len payload crc ETX",
        E_RDONLY => "镜像只读：写删改请求一律拒绝",
        E_SEQ => "帧序断档：序号不连续（丢帧）",
        E_CRC => "帧校验和不符：线路噪声",
        E_LEN => "帧载荷超长：64 字节上限",
        E_BAL => "镜像账不平：镜像行数与帧数不符",
        _ => "未知诊断码",
    }
}

pub const STX: u8 = 0x02;
pub const ETX: u8 = 0x03;
pub const PAYLOAD_MAX: usize = 64;

/// FNV-1a 8 位帧校验。
pub fn crc8(data: &[u8]) -> u8 {
    let mut h: u8 = 0x37;
    for b in data {
        h ^= *b;
        h = h.rotate_left(1);
        h = h.wrapping_add(0x2B);
    }
    h
}

/// 编码帧：STX seq(2) len(1) payload crc ETX。
pub fn frame_encode(seq: u16, payload: &[u8]) -> Result<Vec<u8>, u16> {
    if payload.len() > PAYLOAD_MAX {
        return Err(E_LEN);
    }
    let mut f: Vec<u8> = Vec::new();
    f.push(STX);
    f.extend_from_slice(&seq.to_le_bytes());
    f.push(payload.len() as u8);
    f.extend_from_slice(payload);
    let mut hb = Vec::new();
    hb.extend_from_slice(&seq.to_le_bytes());
    hb.push(payload.len() as u8);
    hb.extend_from_slice(payload);
    f.push(crc8(&hb));
    f.push(ETX);
    Ok(f)
}

/// 解码帧：全格式校验，逐码回报。
pub fn frame_decode(f: &[u8]) -> Result<(u16, Vec<u8>), u16> {
    if f.len() < 6 {
        return Err(E_FRAME);
    }
    if f[0] != STX || f[f.len() - 1] != ETX {
        return Err(E_FRAME);
    }
    let len = f[3] as usize;
    if f.len() != 6 + len {
        return Err(E_FRAME);
    }
    if len > PAYLOAD_MAX {
        return Err(E_LEN);
    }
    let seq = u16::from_le_bytes([f[1], f[2]]);
    let payload = f[4..4 + len].to_vec();
    let mut hb = Vec::new();
    hb.extend_from_slice(&seq.to_le_bytes());
    hb.push(len as u8);
    hb.extend_from_slice(&payload);
    if crc8(&hb) != f[4 + len] {
        return Err(E_CRC);
    }
    Ok((seq, payload))
}

/// 镜像账：帧流 → 行镜像（只读）。
pub struct Mirror {
    pub lines: Vec<(u16, String)>,
    pub last_seq: Option<u16>,
    pub frames_seen: u64,
    pub write_blocked: u64,
}

impl Mirror {
    pub fn new() -> Mirror {
        Mirror {
            lines: Vec::new(),
            last_seq: None,
            frames_seen: 0,
            write_blocked: 0,
        }
    }

    /// 收帧入镜像：序号必须严格递增（容忍跳号则报断档）。
    pub fn ingest(&mut self, f: &[u8]) -> Result<(), u16> {
        let (seq, payload) = frame_decode(f)?;
        if let Some(last) = self.last_seq {
            if seq <= last {
                return Err(E_SEQ);
            }
            if seq != last + 1 {
                return Err(E_SEQ); // 零丢帧判据：窗口内必须连续
            }
        }
        self.last_seq = Some(seq);
        self.frames_seen += 1;
        self.lines.push((
            seq,
            String::from_utf8_lossy(&payload).to_string(),
        ));
        Ok(())
    }

    /// 只读闸：任何写语义请求拒绝并计数。
    pub fn request_write(&mut self, _op: &str) -> Result<(), u16> {
        self.write_blocked += 1;
        Err(E_RDONLY)
    }

    /// 镜像账守恒：行数 == 收帧数。
    pub fn audit(&self) -> Result<(), u16> {
        if self.lines.len() as u64 != self.frames_seen {
            return Err(E_BAL);
        }
        Ok(())
    }

    /// 序号连续核：镜像内 seq 逐行 +1。
    pub fn seq_audit(&self) -> Result<(), u16> {
        for w in self.lines.windows(2) {
            if w[1].0 != w[0].0 + 1 {
                return Err(E_SEQ);
            }
        }
        Ok(())
    }

    pub fn render(&self) -> Result<String, u16> {
        if self.lines.is_empty() {
            return Err(E_BAL);
        }
        Ok(format!(
            "镜像 {} 帧（首#{} 末#{}），写请求拒 {} 次",
            self.frames_seen,
            self.lines[0].0,
            self.lines[self.lines.len() - 1].0,
            self.write_blocked
        ))
    }
}

// ── 确定性演练：合法 3 例 + 注入拦截 5 例 ────────────────────────
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;

    // R1：编解码往返——三帧（序 1~3）→ 镜像 3 行，账全绿。
    let mut m = Mirror::new();
    for i in 1..=3u16 {
        let f = frame_encode(i, format!("klog-{}", i).as_bytes())?;
        m.ingest(&f)?;
    }
    m.audit()?;
    m.seq_audit()?;
    hits += 1;

    // R2：只读闸——写/删/改三请求全拒并计数。
    if m.request_write("write") != Err(E_RDONLY) {
        return Err(E_RDONLY);
    }
    if m.request_write("delete") != Err(E_RDONLY) {
        return Err(E_RDONLY);
    }
    if m.request_write("update") != Err(E_RDONLY) {
        return Err(E_RDONLY);
    }
    if m.write_blocked != 3 {
        return Err(E_BAL);
    }
    let rep = m.render()?;
    if !rep.contains("镜像 3 帧") || !rep.contains("拒 3 次") {
        return Err(E_BAL);
    }
    hits += 1;

    // R3：载荷边界——恰 64 字节过，65 拒（编码与解码两路）。
    let edge = [0x41u8; PAYLOAD_MAX];
    let f = frame_encode(4, &edge)?;
    m.ingest(&f)?;
    let over = [0x42u8; PAYLOAD_MAX + 1];
    if frame_encode(9, &over) != Err(E_LEN) {
        return Err(E_LEN);
    }
    hits += 1;

    // I1：帧残缺（去 STX / 去 ETX / 截短）→ E_FRAME。
    let good = frame_encode(1, b"x")?;
    let mut bad1 = good.clone();
    bad1[0] = 0x00;
    if frame_decode(&bad1) != Err(E_FRAME) {
        return Err(E_FRAME);
    }
    let mut bad2 = good.clone();
    let n2 = bad2.len();
    bad2[n2 - 1] = 0x00;
    if frame_decode(&bad2) != Err(E_FRAME) {
        return Err(E_FRAME);
    }
    if frame_decode(&good[..6]) != Err(E_FRAME) {
        return Err(E_FRAME);
    }
    hits += 1;

    // I2：CRC 蚀 → E_CRC。
    let mut bad3 = frame_encode(1, b"noise")?;
    let n3 = bad3.len();
    bad3[n3 - 2] ^= 0xFF;
    if frame_decode(&bad3) != Err(E_CRC) {
        return Err(E_CRC);
    }
    hits += 1;

    // I3：帧序断档（跳号）→ E_SEQ；倒序重放 → E_SEQ。
    let mut m3 = Mirror::new();
    m3.ingest(&frame_encode(1, b"a")?)?;
    if m3.ingest(&frame_encode(3, b"c")?) != Err(E_SEQ) {
        return Err(E_SEQ);
    }
    if m3.ingest(&frame_encode(1, b"a")?) != Err(E_SEQ) {
        return Err(E_SEQ);
    }
    hits += 1;

    // I4：解码端载荷超长声明（len=65 但帧长匹配构造）→ E_LEN。
    let mut bad4: Vec<u8> = Vec::new();
    bad4.push(STX);
    bad4.extend_from_slice(&1u16.to_le_bytes());
    bad4.push(PAYLOAD_MAX as u8 + 1);
    bad4.extend_from_slice(&[0u8; PAYLOAD_MAX + 1]);
    bad4.push(0);
    bad4.push(ETX);
    if frame_decode(&bad4) != Err(E_LEN) {
        return Err(E_LEN);
    }
    hits += 1;

    // I5：镜像账破（行账与帧数不符）→ E_BAL。
    let mut m5 = Mirror::new();
    m5.ingest(&frame_encode(1, b"z")?)?;
    m5.frames_seen = 5; // 人为破账
    if m5.audit() != Err(E_BAL) {
        return Err(E_BAL);
    }
    hits += 1;

    Ok(hits)
}
