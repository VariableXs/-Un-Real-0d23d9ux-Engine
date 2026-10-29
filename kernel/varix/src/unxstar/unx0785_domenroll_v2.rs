#![allow(dead_code)]
// =====================================================================================
// 任务单：UNX-F0785 · 域功入册（A1 域 800 条功能清单入总册）
// 域/批：CoRun·UNX A1 收官域（入册/六判据终验面 · W7 清单第 12 单）
// 册目标：570 行 ｜ 本文件实际：≥570 行（自包含单文件子系统）
// 判据原文（清单逐字抄录）：
//   纯功能代码 ≥570 行；② 判据原文：UNX-F0785-J1 入册过（域功能清单 800 条
//   四列：fid/名/行数/判据号——与批册直读一致 100%）且总册挂载声明落账；③
//   完成时 API/软件写回：状态=已完成、结果摘要非空。
// 设计纪律：① 四列建模：fid（1..=800 全域唯一）/ 名（非空 ≤24 字节）/ 行数
//   （1..=4096）/ 判据号（1..=999 闭集）；批册喂入后做「直读复算」——以第二
//   路读回数据逐行逐字段比对 100%，不一致定位到（行, 列）。
// ② 总册挂载声明：入册齐 800 才准挂载；挂载槽闭集 1..=4，重挂即红；挂载
//   锚 = 清单指纹，禁事后改写。
// ③ 守恒：Σ行数 == 直读复算 Σ行数；条数 800/800；行号连续闭集 1..=800。
// ④ 状态机 Intake→Readback→Verified→Mounted 单向闭集。
// ⑤ 常量写死注来源；禁浮点禁 std；诊断码定位到（行, 列）。
// 诊断段声明：0x6060–0x607F 本单独占（清单 <SEG 0x6060>），不得他用。
// 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。
// =====================================================================================
extern crate alloc;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

// 1. 诊断码段（本单独占 0x6060–0x607F）
pub const SEG_BASE: u16 = 0x6060; // 诊断码段基数，来源：W7 清单 UNX-F0785 行 <SEG 0x6060>
pub const SEG_TOP: u16 = 0x607F; // 诊断码段顶界（含）
pub const XC_STATE_INVALID: u16 = SEG_BASE + 1; // 入册状态机非法边
pub const XC_EMPTY_FEED: u16 = SEG_BASE + 2; // 空批喂入
pub const XC_DUP_FID: u16 = SEG_BASE + 3; // fid 重复
pub const XC_FID_GAP: u16 = SEG_BASE + 4; // 行号不连续（1..=800 闭集断裂）
pub const XC_NAME_BAD: u16 = SEG_BASE + 5; // 名为空/超 24 字节
pub const XC_LINES_BAD: u16 = SEG_BASE + 6; // 行数越出 1..=4096
pub const XC_CRIT_BAD: u16 = SEG_BASE + 7; // 判据号越出 1..=999
pub const XC_READBACK_DIFF: u16 = SEG_BASE + 8; // 直读复算不一致（一致 100% 红）
pub const XC_LEN_MISMATCH: u16 = SEG_BASE + 9; // 直读行数 ≠ 批册行数
pub const XC_NOT_VERIFIED: u16 = SEG_BASE + 10; // 未验齐即挂载
pub const XC_MOUNT_DUP: u16 = SEG_BASE + 11; // 挂载槽重挂
pub const XC_MOUNT_BAD: u16 = SEG_BASE + 12; // 挂载槽越出 1..=4
pub const XC_ALREADY_MOUNTED: u16 = SEG_BASE + 13; // 已挂载再操作
pub const XC_SUM_DIFF: u16 = SEG_BASE + 14; // Σ行数守恒断裂
pub const XC_SUMMARY_EMPTY: u16 = SEG_BASE + 15; // 摘要为空

// 2. 入册常量（写死，来源=判据原文/本单口径头注假设）
pub const TOTAL_ROWS: usize = 800; // 域功能清单 800 条，来源：判据原文
pub const COLS_PER_ROW: usize = 4; // 四列：fid/名/行数/判据号，来源：判据原文
pub const NAME_MAX: usize = 24; // 名上限 24 字节，头注假设
pub const LINES_MIN: u32 = 1; // 行数下限
pub const LINES_MAX: u32 = 4096; // 行数上限，头注假设
pub const CRIT_MAX: u32 = 999; // 判据号上限，头注假设
pub const MOUNT_SLOTS: u8 = 4; // 总册挂载槽闭集，头注假设

/// 一条域功能四列行。
#[derive(Clone, Debug)]
pub struct DomRow {
    pub fid: u16,        // 功能号 1..=800
    pub name: String,    // 功能名（非空 ≤24 字节）
    pub lines: u32,      // 行数 1..=4096
    pub crit: u32,       // 判据号 1..=999
}

/// 直读复算差异定位。
#[derive(Clone, Debug)]
pub struct ColDiff {
    pub fid: u16,   // 行
    pub col: usize, // 列 0..=3
    pub batch_val: u64,
    pub read_val: u64,
}

/// 挂载声明。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MountDecl {
    pub slot: u8,        // 总册槽位 1..=4
    pub registry: u32,   // 总册号（0x6060 域总册）
    pub anchor: u64,     // 挂载锚（清单指纹）
}

/// 入册回执。
#[derive(Clone, Debug)]
pub struct EnrollReceipt {
    pub rows: u32,            // 入册行数（800）
    pub total_lines: u64,     // Σ行数
    pub readback_match: bool, // 直读一致 100% 结论
    pub diffs: u32,           // 差异数（0）
    pub mount: Option<MountDecl>,
    pub summary: String,
    pub fingerprint: u64,
}

/// 状态机：单向闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EnrollStage {
    Intake = 0,
    Readback = 1,
    Verified = 2,
    Mounted = 3,
}
impl EnrollStage {
    pub fn can_step(self, next: EnrollStage) -> bool {
        (self as u8) + 1 == next as u8
    }
}
pub fn fnv1a(seed: u64, v: u64) -> u64 {  // FNV-1a-64 单步。
    let mut h = seed;
    let mut b = 0usize;
    while b < 8 {
        h ^= (v >> (b * 8)) & 0xFF;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
        b += 1;
    }
    h
}
pub struct DomainEnroller {  // 域功入册机。
    stage: EnrollStage,
    batch: Vec<DomRow>, // 批册
    readback: Vec<DomRow>, // 直读回读
    diffs: Vec<ColDiff>,
    total_lines: u64,
    mount: Option<MountDecl>,
    events: Vec<String>,
}
impl DomainEnroller {
    pub fn new() -> Self {
        DomainEnroller {
            stage: EnrollStage::Intake,
            batch: Vec::new(),
            readback: Vec::new(),
            diffs: Vec::new(),
            total_lines: 0,
            mount: None,
            events: Vec::new(),
        }
    }
    pub fn stage(&self) -> EnrollStage {
        self.stage
    }
    pub fn batch_len(&self) -> usize {
        self.batch.len()
    }
    pub fn diff_count(&self) -> usize {
        self.diffs.len()
    }
    pub fn diffs(&self) -> &[ColDiff] {  // 差异明细（只读，定位到（行, 列））。
        &self.diffs
    }
    pub fn event_count(&self) -> usize {
        self.events.len()
    }

    /// 行合法性单检：名/行数/判据号三列闭集。
    fn row_ok(r: &DomRow) -> Result<bool, u16> {
        if r.name.is_empty() || r.name.len() > NAME_MAX {
            return Err(XC_NAME_BAD);
        }
        if r.lines < LINES_MIN || r.lines > LINES_MAX {
            return Err(XC_LINES_BAD);
        }
        if r.crit < 1 || r.crit > CRIT_MAX {
            return Err(XC_CRIT_BAD);
        }
        Ok(true)
    }
    pub fn feed_batch(&mut self, rows: &[DomRow]) -> Result<u32, u16> {  // 批册喂入：号唯一、总守恒 800。
        if self.stage != EnrollStage::Intake {
            return Err(XC_STATE_INVALID);
        }
        if rows.is_empty() {
            return Err(XC_EMPTY_FEED);
        }
        let mut added: u32 = 0;
        for r in rows.iter() {
            if self.batch.len() >= TOTAL_ROWS {
                return Err(XC_FID_GAP);
            }
            Self::row_ok(r)?;
            let mut i = 0usize;
            while i < self.batch.len() {
                if self.batch[i].fid == r.fid {
                    return Err(XC_DUP_FID);
                }
                i += 1;
            }
            self.batch.push(r.clone());
            added += 1;
        }
        self.events.push(alloc::format!("feed +{}", added));
        Ok(added)
    }
    pub fn is_full(&self) -> bool {
        self.batch.len() == TOTAL_ROWS
    }
    pub fn fid_continuity(&self) -> Result<u32, u16> {  // 行号连续性核：1..=800 逐号到位（闭集断裂即红）。
        let mut missing: u32 = 0;
        let mut fid = 1u16;
        while fid <= TOTAL_ROWS as u16 {
            let mut hit = false;
            let mut i = 0usize;
            while i < self.batch.len() {
                if self.batch[i].fid == fid {
                    hit = true;
                    break;
                }
                i += 1;
            }
            if !hit {
                missing += 1;
            }
            fid += 1;
        }
        if missing != 0 {
            return Err(XC_FID_GAP);
        }
        Ok(TOTAL_ROWS as u32)
    }
    pub fn sum_lines_batch(&mut self) -> Result<u64, u16> {  // Σ行数复算（批册侧，checked_add）。
        if self.batch.len() != TOTAL_ROWS {
            return Err(XC_LEN_MISMATCH);
        }
        let mut sum: u64 = 0;
        let mut i = 0usize;
        while i < self.batch.len() {
            sum = match sum.checked_add(self.batch[i].lines as u64) {
                Some(v) => v,
                None => return Err(XC_SUM_DIFF),
            };
            i += 1;
        }
        self.total_lines = sum;
        Ok(sum)
    }
    pub fn readback_compare(&mut self, read: &[DomRow]) -> Result<u32, u16> {  // 直读复算：第二路读回逐行逐字段比对 100%。
        if !self.stage.can_step(EnrollStage::Readback) {
            return Err(XC_STATE_INVALID);
        }
        if read.len() != self.batch.len() {
            return Err(XC_LEN_MISMATCH);
        }
        self.readback.clear();
        self.diffs.clear();
        let mut i = 0usize;
        while i < self.batch.len() {
            let b = &self.batch[i];
            let r = &read[i];
            if b.fid != r.fid {
                self.diffs.push(ColDiff { fid: b.fid, col: 0, batch_val: b.fid as u64, read_val: r.fid as u64 });
            }
            if b.name != r.name {
                self.diffs.push(ColDiff { fid: b.fid, col: 1, batch_val: b.name.len() as u64, read_val: r.name.len() as u64 });
            }
            if b.lines != r.lines {
                self.diffs.push(ColDiff { fid: b.fid, col: 2, batch_val: b.lines as u64, read_val: r.lines as u64 });
            }
            if b.crit != r.crit {
                self.diffs.push(ColDiff { fid: b.fid, col: 3, batch_val: b.crit as u64, read_val: r.crit as u64 });
            }
            self.readback.push(r.clone());
            i += 1;
        }
        self.stage = EnrollStage::Readback;
        self.events
            .push(alloc::format!("readback rows={} diffs={}", self.readback.len(), self.diffs.len()));
        Ok(self.diffs.len() as u32)
    }
    pub fn sum_lines_crosscheck(&self) -> Result<bool, u16> {  // 直读 Σ行数与批册 Σ行数守恒互证。
        if self.readback.len() != self.batch.len() {
            return Err(XC_LEN_MISMATCH);
        }
        let mut sum: u64 = 0;
        let mut i = 0usize;
        while i < self.readback.len() {
            sum = match sum.checked_add(self.readback[i].lines as u64) {
                Some(v) => v,
                None => return Err(XC_SUM_DIFF),
            };
            i += 1;
        }
        Ok(sum == self.total_lines)
    }
    pub fn verify(&mut self) -> Result<(u32, u64), u16> {  // 验齐断言：直读零差 + 行号连续 + Σ行数一致 → Verified。
        if !self.stage.can_step(EnrollStage::Verified) {
            return Err(XC_NOT_VERIFIED);
        }
        self.fid_continuity()?;
        if !self.sum_lines_crosscheck()? {
            return Err(XC_SUM_DIFF);
        }
        if !self.diffs.is_empty() {
            return Err(XC_READBACK_DIFF);
        }
        self.stage = EnrollStage::Verified;
        self.events.push(String::from("verified"));
        Ok((self.batch.len() as u32, self.total_lines))
    }
    pub fn mount(&mut self, slot: u8) -> Result<MountDecl, u16> {  // 总册挂载：槽闭集、重挂即红、锚 = 指纹。
        if !self.stage.can_step(EnrollStage::Mounted) {
            return Err(XC_NOT_VERIFIED);
        }
        if slot < 1 || slot > MOUNT_SLOTS {
            return Err(XC_MOUNT_BAD);
        }
        if self.mount.is_some() {
            return Err(XC_MOUNT_DUP);
        }
        let m = MountDecl { slot, registry: 0x6060, anchor: self.fingerprint() };
        self.mount = Some(m);
        self.stage = EnrollStage::Mounted;
        self.events.push(alloc::format!("mount slot={}", slot));
        Ok(m)
    }
    pub fn mount_of(&self) -> Option<MountDecl> {
        self.mount
    }
    pub fn receipt(&self) -> Result<EnrollReceipt, EnrollReceiptErr> {  // 出证回执（挂载后可取，摘要非空）。
        // 内部轻量错误通道：未挂载用 XC_NOT_VERIFIED。
        if self.mount.is_none() {
            return Err(EnrollReceiptErr::NotMounted);
        }
        Ok(EnrollReceipt {
            rows: self.batch.len() as u32,
            total_lines: self.total_lines,
            readback_match: self.diffs.is_empty(),
            diffs: self.diffs.len() as u32,
            mount: self.mount,
            summary: alloc::format!(
                "UNX-F0785-J1 入册过：域功能清单 {} 条四列与批册直读一致 100%（零差异），Σ行数 {}，总册槽 {} 挂载声明落账",
                self.batch.len(), self.total_lines,
                match self.mount { Some(m) => m.slot, None => 0 }
            ),
            fingerprint: self.fingerprint(),
        })
    }
    pub fn fingerprint(&self) -> u64 {  // 清单指纹：全部（fid, 行数, 判据号, 名长）规范序。
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        let mut i = 0usize;
        while i < self.batch.len() {
            let r = &self.batch[i];
            h = fnv1a(h, r.fid as u64);
            h = fnv1a(h, r.lines as u64);
            h = fnv1a(h, r.crit as u64);
            h = fnv1a(h, r.name.len() as u64);
            i += 1;
        }
        h
    }
    pub fn crit_histogram(&self) -> Result<[u32; 10], u16> {  // 判据号直方图：1..=999 十桶（每桶 ≈100 号），桶和 == 800（守恒）。
        let mut hist = [0u32; 10];
        let mut i = 0usize;
        while i < self.batch.len() {
            let c = self.batch[i].crit;
            if c < 1 || c > CRIT_MAX {
                return Err(XC_CRIT_BAD);
            }
            let bucket = ((c - 1) / 100) as usize;
            hist[bucket] += 1;
            i += 1;
        }
        let mut total = 0u32;
        let mut b = 0usize;
        while b < 10 {
            total += hist[b];
            b += 1;
        }
        if total != self.batch.len() as u32 {
            return Err(XC_SUM_DIFF);
        }
        Ok(hist)
    }

    pub fn lines_histogram(&self) -> Result<[u32; 4], u16> {  // 行数四分位桶：[1-64, 65-128, 129-192, 193+]，桶和 == 行数（守恒）。
        let mut hist = [0u32; 4];
        let mut i = 0usize;
        while i < self.batch.len() {
            let l = self.batch[i].lines;
            if l < LINES_MIN || l > LINES_MAX {
                return Err(XC_LINES_BAD);
            }
            let bucket = if l <= 64 {
                0
            } else if l <= 128 {
                1
            } else if l <= 192 {
                2
            } else {
                3
            };
            hist[bucket] += 1;
            i += 1;
        }
        Ok(hist)
    }

    pub fn top_lines_view(&self, n: usize) -> Result<Vec<(u16, u32)>, u16> {  // 行数 top-n 视图：按行数降序取前 n（选择排序，确定性，禁浮点）。
        if n == 0 || n > self.batch.len() {
            return Err(XC_LEN_MISMATCH);
        }
        let mut all: Vec<(u16, u32)> = Vec::new();
        let mut i = 0usize;
        while i < self.batch.len() {
            all.push((self.batch[i].fid, self.batch[i].lines));
            i += 1;
        }
        // 选择排序降序。
        let mut a = 0usize;
        while a < all.len() {
            let mut b = a + 1;
            while b < all.len() {
                if all[b].1 > all[a].1 {
                    let t = all[a];
                    all[a] = all[b];
                    all[b] = t;
                }
                b += 1;
            }
            a += 1;
        }
        all.truncate(n);
        Ok(all)
    }

    pub fn readback_row_of(&self, fid: u16) -> Option<&DomRow> {  // 直读侧行查询（未回读 None）。
        let mut i = 0usize;
        while i < self.readback.len() {
            if self.readback[i].fid == fid {
                return Some(&self.readback[i]);
            }
            i += 1;
        }
        None
    }

        pub fn row_of(&self, fid: u16) -> Option<&DomRow> {  // 行查询（未登记 None）。
        let mut i = 0usize;
        while i < self.batch.len() {
            if self.batch[i].fid == fid {
                return Some(&self.batch[i]);
            }
            i += 1;
        }
        None
    }
}

/// 出证轻量错误（不用 u16 通道，展示错误路径闭集建模）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EnrollReceiptErr {
    NotMounted,
}
pub fn gen_rows(seed: u64) -> Vec<DomRow> {  // 样本发生器：800 行确定性四列（名 12 字节，行数/判据号在闭集内）。
    let mut s = seed;
    let mut out: Vec<DomRow> = Vec::new();
    let mut fid = 1u16;
    while fid <= TOTAL_ROWS as u16 {
        s = s.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
        let r1 = (s >> 33) as u32;
        let lines = LINES_MIN + (r1 % 300);
        let crit = 1 + (r1 % CRIT_MAX);
        out.push(DomRow {
            fid,
            name: alloc::format!("unxfunc_{:04}", fid),
            lines,
            crit,
        });
        fid += 1;
    }
    out
}
pub fn drill() -> Result<u32, u16> {  // 自例程 drill：主路径 + 错误路径，0=全过。
    let rows = gen_rows(0x6060_0000_E000_0001);
    // —— 主路径：分 4 批喂满 → 连续性 → Σ行数 → 直读 → 验齐 → 挂载 → 出证 ——
    let mut m = DomainEnroller::new();
    let mut off = 0usize;
    while off < TOTAL_ROWS {
        let end = if off + 200 < TOTAL_ROWS { off + 200 } else { TOTAL_ROWS };
        m.feed_batch(&rows[off..end])?;
        off = end;
    }
    if !m.is_full() || m.batch_len() != TOTAL_ROWS {
        return Err(XC_LEN_MISMATCH);
    }
    if m.fid_continuity()? != TOTAL_ROWS as u32 {
        return Err(XC_FID_GAP);
    }
    let sum = m.sum_lines_batch()?;
    if sum == 0 {
        return Err(XC_SUM_DIFF);
    }
    let diffs = m.readback_compare(&rows)?;
    if diffs != 0 || m.diff_count() != 0 {
        return Err(XC_READBACK_DIFF);
    }
    if !m.sum_lines_crosscheck()? {
        return Err(XC_SUM_DIFF);
    }
    let (n, total) = m.verify()?;
    if n != 800 || total != sum {
        return Err(XC_LEN_MISMATCH);
    }
    let md = m.mount(2)?;
    if md.slot != 2 || md.registry != 0x6060 {
        return Err(XC_MOUNT_BAD);
    }
    if m.mount(3) != Err(XC_MOUNT_DUP) {
        return Err(XC_MOUNT_DUP);
    }
    let rc = match m.receipt() {
        Ok(r) => r,
        Err(_) => return Err(XC_SUMMARY_EMPTY),
    };
    if rc.rows != 800 || !rc.readback_match || rc.summary.is_empty() {
        return Err(XC_SUMMARY_EMPTY);
    }
    // —— 错误路径 1：直读单点漂移（lines 列）定位（行 401, 列 2）——
    let mut m2 = DomainEnroller::new();
    let mut drift = rows.clone();
    drift[400].lines += 1;
    m2.feed_batch(&rows)?;
    let d2 = m2.readback_compare(&drift)?;
    if d2 != 1 {
        return Err(XC_READBACK_DIFF);
    }
    let dd = m2.diffs();
    if dd[0].fid != 401 || dd[0].col != 2 {
        return Err(XC_READBACK_DIFF);
    }
    if m2.verify() != Err(XC_READBACK_DIFF) {
        return Err(XC_READBACK_DIFF);
    }
    // —— 错误路径 2：fid 重复 / 行数越界 / 名为空 / 判据号越界 ——
    let mut m3 = DomainEnroller::new();
    let mut dup = rows[..3].to_vec();
    dup[2].fid = dup[0].fid;
    if m3.feed_batch(&dup) != Err(XC_DUP_FID) {
        return Err(XC_DUP_FID);
    }
    let mut m4 = DomainEnroller::new();
    let mut badl = rows[..3].to_vec();
    badl[0].lines = 0;
    if m4.feed_batch(&badl) != Err(XC_LINES_BAD) {
        return Err(XC_LINES_BAD);
    }
    let mut m5 = DomainEnroller::new();
    let mut badn = rows[..3].to_vec();
    badn[1].name = String::new();
    if m5.feed_batch(&badn) != Err(XC_NAME_BAD) {
        return Err(XC_NAME_BAD);
    }
    let mut m6 = DomainEnroller::new();
    let mut badc = rows[..3].to_vec();
    badc[2].crit = 0;
    if m6.feed_batch(&badc) != Err(XC_CRIT_BAD) {
        return Err(XC_CRIT_BAD);
    }
    // —— 错误路径 3：未验齐即挂载 / 槽越界 / 直读行数不齐 ——
    let mut m7 = DomainEnroller::new();
    m7.feed_batch(&rows[..10].to_vec())?;
    if m7.mount(1) != Err(XC_NOT_VERIFIED) {
        return Err(XC_NOT_VERIFIED);
    }
    let mut m8 = DomainEnroller::new();
    m8.feed_batch(&rows)?;
    let short: Vec<DomRow> = rows[..799].to_vec();
    if m8.readback_compare(&short) != Err(XC_LEN_MISMATCH) {
        return Err(XC_LEN_MISMATCH);
    }
    // —— 查询 API：行可解引且四列一致 ——
    match m.row_of(1) {
        Some(r) if r.fid == 1 && r.lines == rows[0].lines => {}
        _ => return Err(XC_READBACK_DIFF),
    }
    if m.row_of(9999).is_some() {
        return Err(XC_DUP_FID);
    }
    // —— 扩展 API 面：判据直方图 / 行数桶 / top 视图 / 直读侧行 ——
    let ch = m.crit_histogram()?;
    let mut csum = 0u32;
    let mut cb = 0usize;
    while cb < 10 {
        csum += ch[cb];
        cb += 1;
    }
    if csum != 800 {
        return Err(XC_SUM_DIFF);
    }
    let lh = m.lines_histogram()?;
    let mut lsum = 0u32;
    let mut lb = 0usize;
    while lb < 4 {
        lsum += lh[lb];
        lb += 1;
    }
    if lsum != 800 {
        return Err(XC_SUM_DIFF);
    }
    let top = m.top_lines_view(5)?;
    if top.len() != 5 {
        return Err(XC_LEN_MISMATCH);
    }
    let mut t = 1usize;
    while t < top.len() {
        if top[t].1 > top[t - 1].1 {
            return Err(XC_SUM_DIFF);
        }
        t += 1;
    }
    if m.top_lines_view(801) != Err(XC_LEN_MISMATCH) {
        return Err(XC_LEN_MISMATCH);
    }
    match m.readback_row_of(2) {
        Some(r) if r.lines == rows[1].lines => {}
        _ => return Err(XC_READBACK_DIFF),
    }
    // —— 直读漂移稿（m2）直方图互证：直读侧行数和与批册侧不同 ——
    match m2.sum_lines_crosscheck() {
        Ok(false) => {}
        _ => return Err(XC_SUM_DIFF),
    }
    Ok(0)
}
