//! UNX-F7861 · 备份执行总纲（五阶段流水线宣言）账 · 诊断段 0x9D00
//! 验收锚点：UNX-F7861-J1 五阶段冻结——①清单载入（B33 产物）→②分块读取→
//! ③校验和计算→④传输落仓→⑤会话落账；阶段状态机 ×1（严格线性，仅允许
//! i→i+1）；跳阶段注入 ×10 拒收；阶段倒序注入 ×10 拒收。


use alloc::vec::Vec;

pub const SEG_BASE: u16 = 0x9D00;

/// 五阶段。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BkPhase {
    LoadList,
    ReadChunks,
    Checksum,
    Transfer,
    Session,
}

impl BkPhase {
    pub fn as_str(self) -> &'static str {
        match self {
            BkPhase::LoadList => "load_list",
            BkPhase::ReadChunks => "read_chunks",
            BkPhase::Checksum => "checksum",
            BkPhase::Transfer => "transfer",
            BkPhase::Session => "session",
        }
    }
    pub fn all() -> [BkPhase; 5] {
        [BkPhase::LoadList, BkPhase::ReadChunks, BkPhase::Checksum, BkPhase::Transfer, BkPhase::Session]
    }
    pub fn index(self) -> usize {
        match self {
            BkPhase::LoadList => 0,
            BkPhase::ReadChunks => 1,
            BkPhase::Checksum => 2,
            BkPhase::Transfer => 3,
            BkPhase::Session => 4,
        }
    }
}

/// 五阶段流水线状态机：严格线性。
pub struct BackupPipeline {
    pub cur: usize,       // 0..5，5 = 完成
    pub rejections: u32,  // 拒收计数
    pub completed: u32,
}

impl BackupPipeline {
    pub fn new() -> Self {
        BackupPipeline { cur: 0, rejections: 0, completed: 0 }
    }

    /// 步进：仅允许 cur → cur（重入同段）或 cur+1；其它（跳段/倒序）拒收。
    pub fn step(&mut self, target: BkPhase) -> bool {
        let t = target.index();
        if t == self.cur || t == self.cur + 1 {
            if t == self.cur + 1 {
                self.cur = t;
                if self.cur == 5 {
                    self.completed += 1;
                }
            }
            true
        } else {
            self.rejections += 1;
            false
        }
    }

    pub fn done(&self) -> bool {
        self.cur == 5
    }
}

/// 分块读取 + 校验和账（阶段②③的实际载荷）。
pub struct ChunkLedger {
    pub chunks: Vec<(u32, u32)>, // (块号, 校验和)
}

impl ChunkLedger {
    pub fn new() -> Self {
        ChunkLedger { chunks: Vec::new() }
    }
    pub fn add(&mut self, chunk: u32, data: u64) {
        let mut h: u32 = 0x811C_9DC5;
        for b in data.to_le_bytes().iter() {
            h ^= *b as u32;
            h = h.wrapping_mul(0x0100_0193);
        }
        self.chunks.push((chunk, h));
    }
    pub fn verify(&self, chunk: u32, data: u64) -> bool {
        let mut h: u32 = 0x811C_9DC5;
        for b in data.to_le_bytes().iter() {
            h ^= *b as u32;
            h = h.wrapping_mul(0x0100_0193);
        }
        self.chunks.iter().any(|(c, f)| *c == chunk && *f == h)
    }
}

/// FNV-1a 32 位。
pub fn fnv1a(data: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    for b in data {
        h ^= *b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

/// 确定性 LCG。
pub fn lcg_next(seed: &mut u64) -> u64 {
    *seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
    *seed
}

// ── 注入核（0x9D00 段 5 项）────────────────────────────────────────────

pub fn inject_core_5() -> Vec<(&'static str, bool)> {
    let mut seed: u64 = 0x7861_9D00;
    let mut ok = true;
    for _ in 0..64 {
        if lcg_next(&mut seed) == 0 {
            ok = false;
        }
    }
    let mut out: Vec<(&'static str, bool)> = Vec::new();
    out.push(("LCG-确定性-64轮不炸", ok));
    out.push(("五阶段-枚举全5", BkPhase::all().len() == 5));
    out.push(("状态机-跳段拒", !BackupPipeline::new().step(BkPhase::Transfer)));
    out.push(("状态机-倒序拒", { let mut p = BackupPipeline::new(); p.step(BkPhase::ReadChunks); !p.step(BkPhase::LoadList) }));
    out.push(("FNV-空串锚", fnv1a(b"") == 0x811C_9DC5));
    out
}

// ── 判据聚合 ────────────────────────────────────────────────────────────

/// run_unx7861_checks：本单判据聚合（诊断段 0x9D00）。
pub fn run_unx7861_checks() -> crate::checks::CheckSet {
    let mut s = crate::checks::CheckSet::new("UNX-F7861 备份执行五阶段流水线");

    for (name, ok) in inject_core_5() {
        s.add(&name, ok, if ok { "" } else { "注入核失败" });
    }

    // 全程线性走完：0→1→2→3→4
    let mut p = BackupPipeline::new();
    let mut all_ok = true;
    for ph in BkPhase::all().iter() {
        if !p.step(*ph) {
            all_ok = false;
        }
    }
    let walk_ok = all_ok && p.done() && p.completed == 1;
    s.add("线性-五阶段全程走完", walk_ok, if walk_ok { "" } else { "线性路径漂移" });

    // 同段重入合法
    let mut p2 = BackupPipeline::new();
    let re1 = p2.step(BkPhase::LoadList);
    let re2 = p2.step(BkPhase::LoadList);
    s.add("重入-同段可重复", re1 && re2, if re1 && re2 { "" } else { "重入误拒" });

    // 跳阶段注入 ×10 拒收
    let skips = [BkPhase::Checksum, BkPhase::Transfer, BkPhase::Session, BkPhase::ReadChunks, BkPhase::Checksum, BkPhase::Transfer, BkPhase::Session, BkPhase::Transfer, BkPhase::Session, BkPhase::Session];
    let mut p3 = BackupPipeline::new();
    let mut rej = 0u32;
    for t in skips.iter() {
        if !p3.step(*t) {
            rej += 1;
        }
    }
    s.add("跳段-10/10次拒收", rej == 10 && p3.rejections == 10, if rej == 10 { "" } else { "跳段漏网" });

    // 阶段倒序注入 ×10 拒收
    let mut p4 = BackupPipeline::new();
    for ph in BkPhase::all().iter() {
        let _ = p4.step(*ph);
    }
    // p4 已完成（cur=5）；倒序再走全部拒
    let revs = [BkPhase::Session, BkPhase::Transfer, BkPhase::Checksum, BkPhase::ReadChunks, BkPhase::LoadList, BkPhase::Session, BkPhase::Transfer, BkPhase::Checksum, BkPhase::ReadChunks, BkPhase::LoadList];
    let mut rev_rej = 0u32;
    for t in revs.iter() {
        if !p4.step(*t) {
            rev_rej += 1;
        }
    }
    s.add("倒序-完成态倒序注入10/10拒", rev_rej == 10, if rev_rej == 10 { "" } else { "倒序漏网" });

    // 中途倒序拒：走到阶段②后回阶段①拒
    let mut p5 = BackupPipeline::new();
    p5.step(BkPhase::LoadList);
    p5.step(BkPhase::ReadChunks);
    s.add("倒序-中途回退拒", !p5.step(BkPhase::LoadList), if !p5.step(BkPhase::LoadList) { "" } else { "中途倒序漏网" });

    // 分块读取 + 校验和：8 块写入后逐块验证
    let mut cl = ChunkLedger::new();
    let mut seed: u64 = 0x7861_0008;
    let mut datas: Vec<u64> = Vec::new();
    for c in 0..8u32 {
        let d = lcg_next(&mut seed);
        datas.push(d);
        cl.add(c, d);
    }
    let mut verify_ok = true;
    for (c, d) in datas.iter().enumerate() {
        if !cl.verify(c as u32, *d) {
            verify_ok = false;
        }
    }
    s.add("校验-8块逐块验证全过", verify_ok && cl.chunks.len() == 8, if verify_ok { "" } else { "校验链缺口" });

    // 校验错账检出：单块篡改
    s.add("校验-块3篡改检出", !cl.verify(3, datas[3] ^ 1), if !cl.verify(3, datas[3] ^ 1) { "" } else { "篡改漏检" });

    // 阶段名导出
    let names = BkPhase::all().iter().map(|p| p.as_str()).collect::<Vec<&str>>().join(",");
    s.add("导出-五阶段名序一致", names == "load_list,read_chunks,checksum,transfer,session", if names == "load_list,read_chunks,checksum,transfer,session" { "" } else { "名序漂移" });

    // FNV 阶段指纹稳定
    let h1 = fnv1a(names.as_bytes());
    let h2 = fnv1a(b"load_list,read_chunks,checksum,transfer,session");
    s.add("FNV-阶段指纹稳定", h1 == h2, if h1 == h2 { "" } else { "指纹不稳" });

    s
}
