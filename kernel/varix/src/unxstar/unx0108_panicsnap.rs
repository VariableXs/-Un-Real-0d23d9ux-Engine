//! UNX-F0108 · panic 前日志快照 · 诊断段 0x2360
//! 验收锚点：UNX-F0108-J1 panic 前最近 N 条日志冻结进保留区，
//! 重启后三要素（位置/原因/序号）+ 日志尾可完整找回。
//! 零浮点、零 unwrap、零 expect。


use crate::alloc::{format, string::{String, ToString}, vec::Vec};

pub const SEG_BASE: u16 = 0x2360;

// ── 诊断码闭集 ───────────────────────────────────────────────
const E_TWICE: u16 = SEG_BASE + 1;  // 重复冻结（一 panic 一快照）
const E_LEN: u16 = SEG_BASE + 2;    // 位置/原因文本超长
const E_EMPTY: u16 = SEG_BASE + 3;  // 空快照出报告
const E_CRC: u16 = SEG_BASE + 4;    // 保留区镜像校验和不符
const E_SEQ: u16 = SEG_BASE + 5;    // 序号断档（找回窗不连续）
const E_BAL: u16 = SEG_BASE + 6;    // 快照账不平（冻结条数与窗不符）

pub fn detail(code: u16) -> &'static str {
    match code {
        E_TWICE => "重复冻结：panic 快照一机一次",
        E_LEN => "文本超长：位置 32 字节 / 原因 48 字节上限",
        E_EMPTY => "空快照：无日志尾禁止冻结",
        E_CRC => "镜像校验和不符：保留区被蚀",
        E_SEQ => "序号断档：找回窗内日志序号不连续",
        E_BAL => "快照账不平：窗内条数与声明不符",
        _ => "未知诊断码",
    }
}

pub const LOC_CAP: usize = 32;
pub const WHY_CAP: usize = 48;
pub const TAIL_N: usize = 8; // 冻结最近 8 条日志

/// FNV-1a 16 位镜像校验。
pub fn fnv1a16(data: &[u8]) -> u16 {
    let mut h: u16 = 0x811C;
    for b in data {
        h ^= *b as u16;
        h = h.wrapping_mul(0x0193);
    }
    h
}

/// 日志尾条目：全局序号 + 单行文本。
#[derive(Clone)]
pub struct TailLine {
    pub seq: u64,
    pub text: String,
}

/// panic 三要素。
#[derive(Clone)]
pub struct PanicTriple {
    pub loc: String,  // 位置（文件:行）
    pub why: String,  // 原因
    pub seq: u64,     // 触发时全局日志序号
}

/// 冻结器：滚动日志尾 + panic 三要素 → 保留区镜像。
pub struct PanicSnap {
    pub tail: Vec<TailLine>,
    pub triple: Option<PanicTriple>,
    pub sealed: bool,
}

impl PanicSnap {
    pub fn new() -> PanicSnap {
        PanicSnap {
            tail: Vec::new(),
            triple: None,
            sealed: false,
        }
    }

    /// 喂日志行：滚动保留最近 TAIL_N 条。
    pub fn feed(&mut self, seq: u64, text: &str) {
        self.tail.push(TailLine {
            seq,
            text: text.to_string(),
        });
        if self.tail.len() > TAIL_N {
            self.tail.remove(0);
        }
    }

    /// 冻结：三要素 + 日志尾一次性锁死，产出保留区镜像。
    /// 镜像布局：[crc:2][要素头][N×(seq:8+len:2+text)]。
    pub fn seal(&mut self, loc: &str, why: &str, seq: u64) -> Result<Vec<u8>, u16> {
        if self.sealed {
            return Err(E_TWICE);
        }
        if loc.len() > LOC_CAP || why.len() > WHY_CAP {
            return Err(E_LEN);
        }
        if self.tail.is_empty() {
            return Err(E_EMPTY);
        }
        self.triple = Some(PanicTriple {
            loc: loc.to_string(),
            why: why.to_string(),
            seq,
        });
        self.sealed = true;

        let mut img: Vec<u8> = Vec::new();
        img.extend_from_slice(&(loc.len() as u16).to_le_bytes());
        img.extend_from_slice(loc.as_bytes());
        let mut lpad = loc.len();
        while lpad < LOC_CAP {
            img.push(0);
            lpad += 1;
        }
        img.extend_from_slice(&(why.len() as u16).to_le_bytes());
        img.extend_from_slice(why.as_bytes());
        let mut wpad = why.len();
        while wpad < WHY_CAP {
            img.push(0);
            wpad += 1;
        }
        img.extend_from_slice(&seq.to_le_bytes());
        img.extend_from_slice(&(self.tail.len() as u16).to_le_bytes());
        for l in &self.tail {
            img.extend_from_slice(&l.seq.to_le_bytes());
            img.extend_from_slice(&(l.text.len() as u16).to_le_bytes());
            img.extend_from_slice(l.text.as_bytes());
        }
        let crc = fnv1a16(&img);
        let mut out = Vec::new();
        out.extend_from_slice(&crc.to_le_bytes());
        out.extend_from_slice(&img);
        Ok(out)
    }

    /// 重启找回：镜像 → 三要素 + 日志尾，逐项校验。
    pub fn restore(img: &[u8]) -> Result<(PanicTriple, Vec<TailLine>), u16> {
        if img.len() < 2 + LOC_CAP + 2 + WHY_CAP + 8 + 2 {
            return Err(E_CRC);
        }
        let stored_crc = u16::from_le_bytes([img[0], img[1]]);
        let body = &img[2..];
        if fnv1a16(body) != stored_crc {
            return Err(E_CRC);
        }
        let rd16 = |o: usize| -> u16 {
            u16::from_le_bytes([body[o], body[o + 1]])
        };
        let rd64 = |o: usize| -> u64 {
            let mut b = [0u8; 8];
            b.copy_from_slice(&body[o..o + 8]);
            u64::from_le_bytes(b)
        };
        let loc_len = rd16(0) as usize;
        if loc_len > LOC_CAP {
            return Err(E_LEN);
        }
        let loc = String::from_utf8_lossy(&body[2..2 + loc_len]).to_string();
        let wbase = 2 + LOC_CAP;
        let why_len = rd16(wbase) as usize;
        if why_len > WHY_CAP {
            return Err(E_LEN);
        }
        let why =
            String::from_utf8_lossy(&body[wbase + 2..wbase + 2 + why_len]).to_string();
        let sbase = wbase + 2 + WHY_CAP;
        let seq = rd64(sbase);
        let n = rd16(sbase + 8) as usize;
        let mut p = sbase + 10;
        let mut tail: Vec<TailLine> = Vec::new();
        let mut expect = 0u64;
        for _ in 0..n {
            let lseq = rd64(p);
            let tlen = rd16(p + 8) as usize;
            let text = String::from_utf8_lossy(&body[p + 10..p + 10 + tlen]).to_string();
            if expect != 0 && lseq != expect {
                return Err(E_SEQ);
            }
            expect = lseq + 1;
            tail.push(TailLine { seq: lseq, text });
            p += 10 + tlen;
        }
        if tail.is_empty() {
            return Err(E_EMPTY);
        }
        Ok((
            PanicTriple {
                loc,
                why,
                seq,
            },
            tail,
        ))
    }

    /// 快照账核：找回条数 ≤ TAIL_N 且序号尾接 panic 序号前缘。
    pub fn audit(&self, recovered: usize, triple_seq: u64) -> Result<(), u16> {
        if recovered > TAIL_N {
            return Err(E_BAL);
        }
        if let Some(first) = self.tail.first() {
            if triple_seq < first.seq {
                return Err(E_BAL);
            }
        }
        Ok(())
    }

    pub fn render(&self) -> Result<String, u16> {
        match &self.triple {
            None => Err(E_EMPTY),
            Some(t) => Ok(format!(
                "panic@{}: {}（日志序 {}），尾 {} 条",
                t.loc,
                t.why,
                t.seq,
                self.tail.len()
            )),
        }
    }
}

// ── 确定性演练：合法 3 例 + 注入拦截 5 例 ────────────────────────
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;

    // R1：喂 12 条日志 → 冻结 → 找回：尾恰 8 条（序 5~12），三要素全对。
    let mut p = PanicSnap::new();
    for i in 1..=12u64 {
        p.feed(i, &format!("log-line-{}", i));
    }
    let img = p.seal("main.rs:42", "unwrap on None", 12)?;
    let (tri, tail) = PanicSnap::restore(&img)?;
    if tri.loc != "main.rs:42" || tri.why != "unwrap on None" || tri.seq != 12 {
        return Err(E_BAL);
    }
    if tail.len() != TAIL_N || tail[0].seq != 5 || tail[7].seq != 12 {
        return Err(E_SEQ);
    }
    p.audit(tail.len(), tri.seq)?;
    hits += 1;

    // R2：不满窗（3 条）也冻结找回 → 恰 3 条；渲染账可读。
    let mut p2 = PanicSnap::new();
    p2.feed(1, "boot");
    p2.feed(2, "mmu on");
    p2.feed(3, "oops");
    let img2 = p2.seal("mmu.rs:7", "page fault", 3)?;
    let (_tri2, tail2) = PanicSnap::restore(&img2)?;
    if tail2.len() != 3 || tail2[2].text != "oops" {
        return Err(E_SEQ);
    }
    let rep = p2.render()?;
    if !rep.contains("mmu.rs:7") || !rep.contains("尾 3 条") {
        return Err(E_BAL);
    }
    hits += 1;

    // R3：滚动窗语义——第 9 条进窗后第 1 条出局（TAIL_N=8）。
    let mut p3 = PanicSnap::new();
    for i in 1..=9u64 {
        p3.feed(i, "x");
    }
    if p3.tail[0].seq != 2 || p3.tail.len() != 8 {
        return Err(E_BAL);
    }
    hits += 1;

    // I1：重复冻结 → E_TWICE。
    let mut i1 = PanicSnap::new();
    i1.feed(1, "a");
    i1.seal("a.rs:1", "w", 1)?;
    if !matches!(i1.seal("b.rs:2", "x", 2), Err(E_TWICE)) {
        return Err(E_TWICE);
    }
    hits += 1;

    // I2：文本超长 → E_LEN（位置 33 / 原因 49 两路）。
    let mut i2 = PanicSnap::new();
    i2.feed(1, "a");
    let long_loc = "x".repeat(LOC_CAP + 1);
    let long_why = "y".repeat(WHY_CAP + 1);
    if !matches!(i2.seal(&long_loc, "w", 1), Err(E_LEN)) {
        return Err(E_LEN);
    }
    if !matches!(i2.seal("a.rs:1", &long_why, 1), Err(E_LEN)) {
        return Err(E_LEN);
    }
    hits += 1;

    // I3：空日志尾冻结 → E_EMPTY；空镜像找回 → E_CRC。
    let mut i3 = PanicSnap::new();
    if !matches!(i3.seal("a.rs:1", "w", 1), Err(E_EMPTY)) {
        return Err(E_EMPTY);
    }
    if !matches!(PanicSnap::restore(&[]), Err(E_CRC)) {
        return Err(E_CRC);
    }
    hits += 1;

    // I4：镜像蚀一字节 → E_CRC（crc 头与体蚀两路同拦）。
    let mut i4 = PanicSnap::new();
    i4.feed(1, "alive");
    let mut img4 = i4.seal("a.rs:9", "boom", 1)?;
    let n = img4.len();
    img4[n - 1] ^= 0xFF; // 体蚀
    if !matches!(PanicSnap::restore(&img4), Err(E_CRC)) {
        return Err(E_CRC);
    }
    img4[n - 1] ^= 0xFF;
    img4[0] ^= 0xFF; // crc 头蚀
    if !matches!(PanicSnap::restore(&img4), Err(E_CRC)) {
        return Err(E_CRC);
    }
    hits += 1;

    // I5：镜像内日志序号断档 → E_SEQ；未冻结出报告 → E_EMPTY。
    let mut i5 = PanicSnap::new();
    i5.feed(1, "one");
    i5.feed(2, "two");
    let mut img5 = i5.seal("s.rs:1", "w", 2)?;
    // 尾第一条 seq 在镜像偏移 2+2+32+2+48+8+2 处起；蚀其最低字节 → 断档。
    let seq_off = 2 + 2 + LOC_CAP + 2 + WHY_CAP + 8 + 2;
    img5[seq_off] ^= 0x01; // seq 1 → 0，第二条 expect 1 ≠ 0 → 断档
    if !matches!(PanicSnap::restore(&img5), Err(E_SEQ)) {
        return Err(E_SEQ);
    }
    let i5b = PanicSnap::new();
    if !matches!(i5b.render(), Err(E_EMPTY)) {
        return Err(E_EMPTY);
    }
    hits += 1;

    Ok(hits)
}
