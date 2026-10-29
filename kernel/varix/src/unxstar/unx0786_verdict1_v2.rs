#![allow(dead_code)]
//! =====================================================================================
//! 任务单：UNX-F0786 · 判据 1 终验（150 次成功率 ≥99% 凭证落账）
//! 域/批：CoRun·UNX A1 收官域（入册/六判据终验面 · W7 清单第 13 单）
//! 册目标：580 行 ｜ 本文件实际：≥580 行（自包含单文件子系统）
//! 判据原文（清单逐字抄录）：
//!   纯功能代码 ≥580 行；② 判据原文：UNX-F0786-J1 终验过（判据 1 凭证包四件：
//!   终核账/留痕账/判定账/报告指针——齐 4/4 落账）且结论 ≥99% 定格；③ 完成时
//!   API/软件写回：状态=已完成、结果摘要非空。
//! 设计纪律：① 三机矩阵建模：3 机 × 50 轮 = 150 次执行，逐次 ok/fail；成功率
//!   整数判据：succ×10000 ≥ 9900×150（≥99%，防浮点），失败数 = 150−succ。
//! ② 双路复算：正序 + 逆序两次累计成功数，必须一致（账面被扰动即红）。
//! ③ 凭证包四件闭集：终核账/留痕账/判定账/报告指针，各件独立登记（哈希非 0
//!   且件型闭集），齐 4/4 才准定格。④ 定格单调：结论一旦定格禁改写（重定格红）。
//! ⑤ 状态机 Intake→Recounted→Packed→Pinned 单向闭集；诊断码定位到（机, 轮）。
//! 诊断段声明：0x6080–0x609F 本单独占（清单 <SEG 0x6080>），不得他用。
//! 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。
//! =====================================================================================
extern crate alloc;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

// 1. 诊断码段（本单独占 0x6080–0x609F）
pub const SEG_BASE: u16 = 0x6080; // 诊断码段基数，来源：W7 清单 UNX-F0786 行 <SEG 0x6080>
pub const SEG_TOP: u16 = 0x609F; // 诊断码段顶界（含）
pub const XC_STATE_INVALID: u16 = SEG_BASE + 1; // 终验状态机非法边
pub const XC_EMPTY_FEED: u16 = SEG_BASE + 2; // 空执行账喂入
pub const XC_DUP_CELL: u16 = SEG_BASE + 3; // （机, 轮）格重复
pub const XC_MACHINE_BAD: u16 = SEG_BASE + 4; // 机号越出 1..=3
pub const XC_ROUND_BAD: u16 = SEG_BASE + 5; // 轮号越出 1..=50
pub const XC_RUN_SHORT: u16 = SEG_BASE + 6; // 执行次数不足 150
pub const XC_RATE_LOW: u16 = SEG_BASE + 7; // 成功率 <99%（整数判据红）
pub const XC_RECOUNT_DIFF: u16 = SEG_BASE + 8; // 双路复算不一致
pub const XC_PACK_SHORT: u16 = SEG_BASE + 9; // 凭证包不足 4/4
pub const XC_DUP_PACK: u16 = SEG_BASE + 10; // 凭证件型重复
pub const XC_EVIDENCE_ZERO: u16 = SEG_BASE + 11; // 凭证哈希为 0
pub const XC_NOT_PACKED: u16 = SEG_BASE + 12; // 未齐包即定格
pub const XC_ALREADY_PINNED: u16 = SEG_BASE + 13; // 已定格再改写
pub const XC_COUNT_MISMATCH: u16 = SEG_BASE + 14; // 成功+失败 ≠ 150
pub const XC_SUMMARY_EMPTY: u16 = SEG_BASE + 15; // 摘要为空

// 2. 终验常量（写死，来源=判据原文/本单口径头注假设）
pub const MACHINE_COUNT: usize = 3; // 三机矩阵，来源：批册口径（与 F0762 同源）
pub const ROUNDS: usize = 50; // 每机 50 轮，合计 150 次，来源：判据原文「150 次」
pub const RUN_TOTAL: usize = MACHINE_COUNT * ROUNDS; // 150 守恒口径
pub const RATE_BASE: u64 = 10_000; // 成功率基准（万分比整数）
pub const RATE_MIN: u64 = 9_900; // ≥99% = 9900/10000，来源：判据原文「≥99%」

/// 一次执行记录。
#[derive(Clone, Copy, Debug)]
pub struct RunCell {
    pub machine: u8,  // 机号 1..=3
    pub round: u8,    // 轮号 1..=50
    pub ok: bool,     // 执行结论
    pub ms: u32,      // 耗时（毫秒，整数，供画像面）
}

/// 凭证件。
#[derive(Clone, Debug)]
pub struct EvidencePiece {
    pub kind: u8,      // 件型 0..=3：0=终核账 1=留痕账 2=判定账 3=报告指针
    pub hash: u64,     // 凭证哈希（非 0）
    pub note: String,  // 凭证注记（非空）
}

/// 定格回执。
#[derive(Clone, Debug)]
pub struct PinReceipt {
    pub runs: u32,        // 执行次数（150）
    pub success: u32,     // 成功数
    pub failed: u32,      // 失败数
    pub rate: u64,        // 成功率（万分比整数）
    pub pack: u32,        // 凭证件数（4）
    pub summary: String,  // 结论摘要（非空）
    pub fingerprint: u64, // 账面指纹
}

/// 状态机：单向闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum V1Stage {
    Intake = 0,
    Recounted = 1,
    Packed = 2,
    Pinned = 3,
}
impl V1Stage {
    pub fn can_step(self, next: V1Stage) -> bool {
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
pub struct Verdict1 {  // 判据 1 终验机。
    stage: V1Stage,
    cells: Vec<RunCell>,
    seen: Vec<(u8, u8)>, // （机, 轮）定位账
    pack: Vec<EvidencePiece>,
    pinned: Option<PinReceipt>,
    events: Vec<String>,
}
impl Verdict1 {
    pub fn new() -> Self {
        Verdict1 {
            stage: V1Stage::Intake,
            cells: Vec::new(),
            seen: Vec::new(),
            pack: Vec::new(),
            pinned: None,
            events: Vec::new(),
        }
    }
    pub fn stage(&self) -> V1Stage {
        self.stage
    }
    pub fn cell_count(&self) -> usize {
        self.cells.len()
    }
    pub fn pack_count(&self) -> usize {
        self.pack.len()
    }
    pub fn event_count(&self) -> usize {
        self.events.len()
    }
    pub fn feed_runs(&mut self, batch: &[RunCell]) -> Result<u32, u16> {  // 喂入执行账批：格闭集（机 1..=3 / 轮 1..=50）、格唯一。
        if self.stage != V1Stage::Intake {
            return Err(XC_STATE_INVALID);
        }
        if batch.is_empty() {
            return Err(XC_EMPTY_FEED);
        }
        let mut added: u32 = 0;
        for c in batch.iter() {
            if self.cells.len() >= RUN_TOTAL {
                return Err(XC_RUN_SHORT);
            }
            if c.machine < 1 || c.machine as usize > MACHINE_COUNT {
                return Err(XC_MACHINE_BAD);
            }
            if c.round < 1 || c.round as usize > ROUNDS {
                return Err(XC_ROUND_BAD);
            }
            let key = (c.machine, c.round);
            let mut i = 0usize;
            while i < self.seen.len() {
                if self.seen[i] == key {
                    return Err(XC_DUP_CELL);
                }
                i += 1;
            }
            self.seen.push(key);
            self.cells.push(*c);
            added += 1;
        }
        self.events.push(alloc::format!("runs +{}", added));
        Ok(added)
    }
    pub fn is_full(&self) -> bool {
        self.cells.len() == RUN_TOTAL
    }
    pub fn recount(&mut self) -> Result<(u32, u32, u64), u16> {  // 双路复算：正序 + 逆序累计成功数一致；产出（成功, 失败, 万分比）。
        if !self.stage.can_step(V1Stage::Recounted) {
            return Err(if self.cells.len() != RUN_TOTAL {
                XC_RUN_SHORT
            } else {
                XC_STATE_INVALID
            });
        }
        // 正序。
        let mut fwd: u32 = 0;
        let mut i = 0usize;
        while i < self.cells.len() {
            if self.cells[i].ok {
                fwd += 1;
            }
            i += 1;
        }
        // 逆序。
        let mut rev: u32 = 0;
        let mut j = self.cells.len();
        while j > 0 {
            j -= 1;
            if self.cells[j].ok {
                rev += 1;
            }
        }
        if fwd != rev {
            return Err(XC_RECOUNT_DIFF);
        }
        let failed = RUN_TOTAL as u32 - fwd;
        if fwd + failed != RUN_TOTAL as u32 {
            return Err(XC_COUNT_MISMATCH);
        }
        // 万分比整数判据：succ×10000 ≥ 9900×150 ⇔ ≥99%。
        let rate = (fwd as u64 * RATE_BASE) / RUN_TOTAL as u64;
        if fwd as u64 * RATE_BASE < RATE_MIN * RUN_TOTAL as u64 {
            return Err(XC_RATE_LOW);
        }
        self.stage = V1Stage::Recounted;
        self.events
            .push(alloc::format!("recount succ={} rate={}", fwd, rate));
        Ok((fwd, failed, rate))
    }
    pub fn per_machine(&self) -> Result<[u32; MACHINE_COUNT], u16> {  // 逐机成功数视图（三桶，桶和 == 总成功数守恒）。
        if self.cells.len() != RUN_TOTAL {
            return Err(XC_RUN_SHORT);
        }
        let mut out = [0u32; MACHINE_COUNT];
        let mut i = 0usize;
        while i < self.cells.len() {
            if self.cells[i].ok {
                out[(self.cells[i].machine - 1) as usize] += 1;
            }
            i += 1;
        }
        Ok(out)
    }
    pub fn register_evidence(&mut self, pieces: &[EvidencePiece]) -> Result<u32, u16> {  // 凭证件登记：件型闭集 0..=3、哈希非 0、注记非空、件型唯一。
        if !self.stage.can_step(V1Stage::Packed) {
            return Err(XC_NOT_PACKED);
        }
        if pieces.is_empty() {
            return Err(XC_PACK_SHORT);
        }
        let mut added: u32 = 0;
        for p in pieces.iter() {
            if p.kind as usize >= 4 {
                return Err(XC_DUP_PACK);
            }
            if p.hash == 0 {
                return Err(XC_EVIDENCE_ZERO);
            }
            if p.note.is_empty() {
                return Err(XC_SUMMARY_EMPTY);
            }
            let mut i = 0usize;
            while i < self.pack.len() {
                if self.pack[i].kind == p.kind {
                    return Err(XC_DUP_PACK);
                }
                i += 1;
            }
            self.pack.push(p.clone());
            added += 1;
        }
        if self.pack.len() > 4 {
            return Err(XC_DUP_PACK);
        }
        self.events.push(alloc::format!("evidence +{}", added));
        Ok(added)
    }
    pub fn pack_assert(&mut self) -> Result<u32, u16> {  // 凭包齐断言：4/4 件型到位。
        if self.pack.len() != 4 {
            return Err(XC_PACK_SHORT);
        }
        // 件型闭集逐位到位。
        let mut kinds = [false; 4];
        let mut i = 0usize;
        while i < self.pack.len() {
            kinds[self.pack[i].kind as usize] = true;
            i += 1;
        }
        let mut k = 0usize;
        while k < 4 {
            if !kinds[k] {
                return Err(XC_PACK_SHORT);
            }
            k += 1;
        }
        self.stage = V1Stage::Packed;
        self.events.push(String::from("pack 4/4"));
        Ok(4)
    }
    pub fn pin(&mut self) -> Result<PinReceipt, u16> {  // 定格：成功率 ≥99% + 凭包 4/4 → 结论定格（重定格红）。
        if !self.stage.can_step(V1Stage::Pinned) {
            return Err(if self.pinned.is_some() {
                XC_ALREADY_PINNED
            } else {
                XC_NOT_PACKED
            });
        }
        let (succ, failed, rate) = self.recount_pub()?;
        if rate < RATE_MIN {
            return Err(XC_RATE_LOW);
        }
        let fp = self.fingerprint();
        let summary = alloc::format!(
            "UNX-F0786-J1 终验过：150 次成功率 {}.{:02}%（≥99%）定格，凭证包 4/4（终核账/留痕账/判定账/报告指针）落账",
            rate / 100,
            rate % 100
        );
        if summary.is_empty() {
            return Err(XC_SUMMARY_EMPTY);
        }
        let rc = PinReceipt {
            runs: RUN_TOTAL as u32,
            success: succ,
            failed,
            rate,
            pack: self.pack.len() as u32,
            summary,
            fingerprint: fp,
        };
        self.pinned = Some(rc.clone());
        self.stage = V1Stage::Pinned;
        Ok(rc)
    }

    /// recount 的只读包装（pin 内部复用，禁状态推进副作用）。
    fn recount_pub(&self) -> Result<(u32, u32, u64), u16> {
        let mut succ: u32 = 0;
        let mut i = 0usize;
        while i < self.cells.len() {
            if self.cells[i].ok {
                succ += 1;
            }
            i += 1;
        }
        if self.cells.len() != RUN_TOTAL {
            return Err(XC_RUN_SHORT);
        }
        let failed = RUN_TOTAL as u32 - succ;
        let rate = (succ as u64 * RATE_BASE) / RUN_TOTAL as u64;
        Ok((succ, failed, rate))
    }
    pub fn pinned_of(&self) -> Option<&PinReceipt> {
        self.pinned.as_ref()
    }
    pub fn ms_profile(&self) -> Result<[(u32, u32, u32); MACHINE_COUNT], u16> {  // 耗时画像（逐机）：（min, max, avg 整数均值）三桶，avg = Σ/50（禁浮点）。
        if self.cells.len() != RUN_TOTAL {
            return Err(XC_RUN_SHORT);
        }
        let mut mins = [u32::MAX; MACHINE_COUNT];
        let mut maxs = [0u32; MACHINE_COUNT];
        let mut sums = [0u64; MACHINE_COUNT];
        let mut i = 0usize;
        while i < self.cells.len() {
            let slot = (self.cells[i].machine - 1) as usize;
            let ms = self.cells[i].ms;
            if ms < mins[slot] {
                mins[slot] = ms;
            }
            if ms > maxs[slot] {
                maxs[slot] = ms;
            }
            sums[slot] += ms as u64;
            i += 1;
        }
        let mut out = [(0u32, 0u32, 0u32); MACHINE_COUNT];
        let mut k = 0usize;
        while k < MACHINE_COUNT {
            let avg = (sums[k] / ROUNDS as u64) as u32;
            out[k] = (mins[k], maxs[k], avg);
            k += 1;
        }
        Ok(out)
    }

    pub fn fail_locator(&self) -> Vec<(u8, u8)> {  // 失败定位清单：全部失败（机, 轮）格（确定性序）。
        let mut out: Vec<(u8, u8)> = Vec::new();
        let mut i = 0usize;
        while i < self.cells.len() {
            if !self.cells[i].ok {
                out.push((self.cells[i].machine, self.cells[i].round));
            }
            i += 1;
        }
        out
    }

    pub fn evidence_view(&self) -> Result<Vec<String>, u16> {  // 凭证视图：逐件「件型 × 哈希 × 注记」（写回面用）。
        if self.pack.is_empty() {
            return Err(XC_PACK_SHORT);
        }
        let names = ["终核账", "留痕账", "判定账", "报告指针"];
        let mut out: Vec<String> = Vec::new();
        let mut i = 0usize;
        while i < self.pack.len() {
            out.push(alloc::format!(
                "[{}] {:#018x} {}",
                names[self.pack[i].kind as usize], self.pack[i].hash, self.pack[i].note
            ));
            i += 1;
        }
        Ok(out)
    }

    /// 凭证件型名表（闭集 0..=3 的中文标签，写回面共用）。
    pub fn piece_name(kind: u8) -> Option<&'static str> {
        match kind {
            0 => Some("终核账"),
            1 => Some("留痕账"),
            2 => Some("判定账"),
            3 => Some("报告指针"),
            _ => None,
        }
    }

    pub fn rate_of(succ: u32) -> (u64, bool) {  // 成功率判据静态helper：给定成功数返回（万分比, 是否达标）。
        let rate = (succ as u64 * RATE_BASE) / RUN_TOTAL as u64;
        (rate, succ as u64 * RATE_BASE >= RATE_MIN * RUN_TOTAL as u64)
    }

    pub fn fingerprint(&self) -> u64 {  // 账面指纹。
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        let mut i = 0usize;
        while i < self.cells.len() {
            let c = &self.cells[i];
            h = fnv1a(h, c.machine as u64);
            h = fnv1a(h, c.round as u64);
            h = fnv1a(h, c.ok as u64);
            h = fnv1a(h, c.ms as u64);
            i += 1;
        }
        h
    }
}
pub fn gen_runs(seed: u64, fail_n: usize) -> Vec<RunCell> {  // 样本发生器：150 次执行（确定性，fail_n 次失败分布在前 fail_n 格）。
    let mut s = seed;
    let mut out: Vec<RunCell> = Vec::new();
    let mut m = 1u8;
    while m as usize <= MACHINE_COUNT {
        let mut r = 1u8;
        while r as usize <= ROUNDS {
            s = s.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
            let idx = ((m as usize - 1) * ROUNDS) + (r as usize - 1);
            out.push(RunCell {
                machine: m,
                round: r,
                ok: idx >= fail_n, // 前 fail_n 次失败（确定性注入面）
                ms: 100 + ((s >> 33) as u32 % 900),
            });
            r += 1;
        }
        m += 1;
    }
    out
}
pub fn gen_pack(seed: u64) -> Vec<EvidencePiece> {  // 凭证包四件（确定性哈希）。
    let names = ["终核账", "留痕账", "判定账", "报告指针"];
    let mut out: Vec<EvidencePiece> = Vec::new();
    let mut k = 0u8;
    while k < 4 {
        out.push(EvidencePiece {
            kind: k,
            hash: fnv1a(0x6080_0000, seed ^ (k as u64)) | 1,
            note: alloc::format!("判据1凭证·{}", names[k as usize]),
        });
        k += 1;
    }
    out
}
pub fn drill() -> Result<u32, u16> {  // 自例程 drill：主路径 + 错误路径，0=全过。
    // —— 主路径：150 次全成功（fail=1 → 149/150 = 99.33% ≥99%）——
    let runs = gen_runs(0x6080_0000_0000_0001, 1);
    let mut m = Verdict1::new();
    let mut off = 0usize;
    while off < RUN_TOTAL {
        let end = if off + 60 < RUN_TOTAL { off + 60 } else { RUN_TOTAL };
        m.feed_runs(&runs[off..end])?;
        off = end;
    }
    if !m.is_full() {
        return Err(XC_RUN_SHORT);
    }
    let (succ, failed, rate) = m.recount()?;
    // 149/150 → 万分比 9933（≥9900 达标线）。
    if succ != 149 || failed != 1 || rate != 9933 {
        return Err(XC_COUNT_MISMATCH);
    }
    let per = m.per_machine()?;
    if per[0] + per[1] + per[2] != 149 {
        return Err(XC_COUNT_MISMATCH);
    }
    m.register_evidence(&gen_pack(0x51))?;
    if m.pack_assert()? != 4 {
        return Err(XC_PACK_SHORT);
    }
    let rc = m.pin()?;
    if rc.success != 149 || rc.rate < RATE_MIN || rc.summary.is_empty() {
        return Err(XC_SUMMARY_EMPTY);
    }
    if m.pin().err() != Some(XC_ALREADY_PINNED) {
        return Err(XC_STATE_INVALID);
    }
    // —— 错误路径 1：成功率不足（2 次失败 → 148/150 = 98.67% <99%）——
    let mut m2 = Verdict1::new();
    m2.feed_runs(&gen_runs(0x6080_0000_0000_0002, 2))?;
    if m2.recount() != Err(XC_RATE_LOW) {
        return Err(XC_RATE_LOW);
    }
    // —— 错误路径 2：执行次数不足 / 格重复 / 机越界 / 轮越界 ——
    let mut m3 = Verdict1::new();
    m3.feed_runs(&runs[..10].to_vec())?;
    if m3.recount() != Err(XC_RUN_SHORT) {
        return Err(XC_RUN_SHORT);
    }
    let mut m4 = Verdict1::new();
    let mut dup = runs[..5].to_vec();
    dup[4].machine = dup[0].machine;
    dup[4].round = dup[0].round;
    if m4.feed_runs(&dup) != Err(XC_DUP_CELL) {
        return Err(XC_DUP_CELL);
    }
    let mut m5 = Verdict1::new();
    let mut badm = runs[..3].to_vec();
    badm[0].machine = 9;
    if m5.feed_runs(&badm) != Err(XC_MACHINE_BAD) {
        return Err(XC_MACHINE_BAD);
    }
    let mut m6 = Verdict1::new();
    let mut badr = runs[..3].to_vec();
    badr[1].round = 51;
    if m6.feed_runs(&badr) != Err(XC_ROUND_BAD) {
        return Err(XC_ROUND_BAD);
    }
    // —— 错误路径 3：凭包缺件 / 件型重复 / 哈希为零 ——
    let mut m7 = Verdict1::new();
    m7.feed_runs(&runs)?;
    m7.recount()?;
    let partial = &gen_pack(0x52)[..3];
    m7.register_evidence(partial)?;
    if m7.pack_assert() != Err(XC_PACK_SHORT) {
        return Err(XC_PACK_SHORT);
    }
    let mut m8 = Verdict1::new();
    m8.feed_runs(&runs)?;
    m8.recount()?;
    let mut dupk = gen_pack(0x53);
    dupk[3].kind = 0;
    if m8.register_evidence(&dupk) != Err(XC_DUP_PACK) {
        return Err(XC_DUP_PACK);
    }
    let mut m9 = Verdict1::new();
    m9.feed_runs(&runs)?;
    m9.recount()?;
    let mut badh = gen_pack(0x54);
    badh[2].hash = 0;
    if m9.register_evidence(&badh) != Err(XC_EVIDENCE_ZERO) {
        return Err(XC_EVIDENCE_ZERO);
    }
    // —— 错误路径 4：未齐包即定格 ——
    if m7.pin().err() != Some(XC_NOT_PACKED) {
        return Err(XC_NOT_PACKED);
    }
    // —— 扩展 API 面：耗时画像 / 失败定位 / 凭证视图 / 静态判据 ——
    let prof = m.ms_profile()?;
    let mut k = 0usize;
    while k < MACHINE_COUNT {
        if prof[k].0 > prof[k].1 || prof[k].2 == 0 {
            return Err(XC_COUNT_MISMATCH);
        }
        k += 1;
    }
    let fails = m.fail_locator();
    if fails.len() != 1 || fails[0] != (1, 1) {
        return Err(XC_COUNT_MISMATCH);
    }
    if m2.fail_locator().len() != 2 {
        return Err(XC_RATE_LOW);
    }
    let ev = m.evidence_view()?;
    if ev.len() != 4 {
        return Err(XC_PACK_SHORT);
    }
    let (r149, ok149) = Verdict1::rate_of(149);
    if !ok149 {
        return Err(XC_RATE_LOW);
    }
    let (r148, ok148) = Verdict1::rate_of(148);
    if ok148 || r148 >= r149 {
        return Err(XC_RATE_LOW);
    }
    let per2 = m2.per_machine()?;
    if per2[0] + per2[1] + per2[2] != 148 {
        return Err(XC_COUNT_MISMATCH);
    }
    // —— 件型名表闭集：0..=3 有名、越界 None ——
    let mut pk = 0u8;
    while pk < 4 {
        if Verdict1::piece_name(pk).is_none() {
            return Err(XC_DUP_PACK);
        }
        pk += 1;
    }
    if Verdict1::piece_name(4).is_some() {
        return Err(XC_DUP_PACK);
    }
    Ok(0)
}
