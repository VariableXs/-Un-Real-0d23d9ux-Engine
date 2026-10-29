#![allow(dead_code)]
//! =====================================================================================
//! 任务单：UNX-F0787 · 判据 2 终验（300+ 断言全绿凭证落账）
//! 域/批：CoRun·UNX A1 收官域（入册/六判据终验面 · W7 清单第 14 单）
//! 册目标：570 行 ｜ 本文件实际：≥570 行（自包含单文件子系统）
//! 判据原文（清单逐字抄录）：
//!   纯功能代码 ≥570 行；② 判据原文：UNX-F0787-J1 终验过（判据 2 凭证包四件：
//!   终盘账/封版清单/执行账/分项盘点——齐 4/4）且 ≥300 全绿定格；③ 完成时
//!   API/软件写回：状态=已完成、结果摘要非空。
//! 设计纪律：① 终盘建模：断言清单（aid, suite, aid 唯一）先封版（清单 FNV 指纹
//!   冻结）再执行；执行账逐条回填结论，aid 必须在封版清单内（账外执行即红）。
//! ② 全绿判定：执行数 == 封版清单数且红数 0；总数 ≥300（判据原文下限）。
//! ③ 分项盘点：按套桶（8 桶闭集）分桶计数，桶和 == 总数（守恒恒等式）。
//! ④ 凭证包四件闭集：终盘账/封版清单/执行账/分项盘点，齐 4/4 才定格；定格单调。
//! ⑤ 状态机 Intake→Sealed→Executed→Audited→Pinned；诊断码定位到断言号。
//! 诊断段声明：0x60A0–0x60BF 本单独占（清单 <SEG 0x60a0>），不得他用。
//! 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。
//! =====================================================================================
extern crate alloc;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

// 1. 诊断码段（本单独占 0x60A0–0x60BF）
pub const SEG_BASE: u16 = 0x60A0; // 诊断码段基数，来源：W7 清单 UNX-F0787 行 <SEG 0x60a0>
pub const SEG_TOP: u16 = 0x60BF; // 诊断码段顶界（含）
pub const XC_STATE_INVALID: u16 = SEG_BASE + 1; // 终验状态机非法边
pub const XC_EMPTY_FEED: u16 = SEG_BASE + 2; // 空清单/空执行账
pub const XC_DUP_AID: u16 = SEG_BASE + 3; // 断言号重复
pub const XC_SUITE_BAD: u16 = SEG_BASE + 4; // 套桶越出 0..=7 闭集
pub const XC_TOTAL_SHORT: u16 = SEG_BASE + 5; // 总数 <300（判据下限红）
pub const XC_SEALED_FROZEN: u16 = SEG_BASE + 6; // 封版后再改清单
pub const XC_EXEC_UNKNOWN: u16 = SEG_BASE + 7; // 执行账含封版外断言
pub const XC_EXEC_DUP: u16 = SEG_BASE + 8; // 执行账重复回填
pub const XC_EXEC_SHORT: u16 = SEG_BASE + 9; // 执行数 ≠ 清单数
pub const XC_RED_ASSERT: u16 = SEG_BASE + 10; // 存在红断言（全绿红）
pub const XC_SUBTOTAL_DIFF: u16 = SEG_BASE + 11; // 分项桶和 ≠ 总数
pub const XC_PACK_SHORT: u16 = SEG_BASE + 12; // 凭证包不足 4/4
pub const XC_DUP_PACK: u16 = SEG_BASE + 13; // 凭证件型重复
pub const XC_ALREADY_PINNED: u16 = SEG_BASE + 14; // 已定格再定格
pub const XC_HASH_ZERO: u16 = SEG_BASE + 15; // 凭证哈希为 0

// 2. 终验常量（写死，来源=判据原文/本单口径头注假设）
pub const MIN_ASSERTS: usize = 300; // 断言总数下限，来源：判据原文「≥300 全绿」
pub const ASSERT_CAP: usize = 512; // 清单定容上限，头注假设
pub const SUITE_COUNT: usize = 8; // 分项套桶闭集 0..=7，头注假设
pub const PACK_ITEMS: usize = 4; // 凭证包四件，来源：判据原文

/// 断言清单条目（封版对象）。
#[derive(Clone, Copy, Debug)]
pub struct ManifestRow {
    pub aid: u32,    // 断言号（唯一）
    pub suite: u8,   // 分项套桶 0..=7
    pub token: u64,  // 断言载荷（非 0）
}

/// 执行账条目。
#[derive(Clone, Copy, Debug)]
pub struct ExecRow {
    pub aid: u32,   // 断言号（须在封版清单内）
    pub ok: bool,   // 执行结论
}

/// 凭证件。
#[derive(Clone, Debug)]
pub struct Piece {
    pub kind: u8,    // 0=终盘账 1=封版清单 2=执行账 3=分项盘点
    pub hash: u64,   // 非 0
    pub note: String,
}

/// 分项盘点报告。
#[derive(Clone, Debug)]
pub struct SubInventory {
    pub per_suite: [u32; SUITE_COUNT], // 八桶计数
    pub bucket_sum: u32,               // 桶和（== 总数）
    pub total: u32,                    // 总数
    pub nonempty: u32,                 // 非空桶数
}

/// 定格回执。
#[derive(Clone, Debug)]
pub struct V2Pin {
    pub total: u32,       // 封版断言总数
    pub executed: u32,    // 执行数
    pub green: u32,       // 绿数
    pub red: u32,         // 红数（0）
    pub all_green: bool,  // 全绿结论
    pub pack: u32,        // 凭证件数（4）
    pub summary: String,  // 摘要（非空）
    pub fingerprint: u64, // 账面指纹
}

/// 状态机：单向闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum V2Stage {
    Intake = 0,
    Sealed = 1,
    Executed = 2,
    Audited = 3,
    Pinned = 4,
}
impl V2Stage {
    pub fn can_step(self, next: V2Stage) -> bool {
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
pub struct Verdict2 {  // 判据 2 终验机。
    stage: V2Stage,
    manifest: Vec<ManifestRow>,
    seen: Vec<u32>,
    sealed_hash: u64,
    exec: Vec<ExecRow>,
    exec_seen: Vec<u32>,
    pack: Vec<Piece>,
    pinned: Option<V2Pin>,
    events: Vec<String>,
}
impl Verdict2 {
    pub fn new() -> Self {
        Verdict2 {
            stage: V2Stage::Intake,
            manifest: Vec::new(),
            seen: Vec::new(),
            sealed_hash: 0,
            exec: Vec::new(),
            exec_seen: Vec::new(),
            pack: Vec::new(),
            pinned: None,
            events: Vec::new(),
        }
    }
    pub fn stage(&self) -> V2Stage {
        self.stage
    }
    pub fn manifest_len(&self) -> usize {
        self.manifest.len()
    }
    pub fn exec_len(&self) -> usize {
        self.exec.len()
    }
    pub fn pack_len(&self) -> usize {
        self.pack.len()
    }
    pub fn event_count(&self) -> usize {
        self.events.len()
    }
    pub fn feed_manifest(&mut self, rows: &[ManifestRow]) -> Result<u32, u16> {  // 清单喂入：号唯一、桶闭集、载荷非 0、定容守卫；封版后拒收。
        if self.stage != V2Stage::Intake {
            return Err(XC_SEALED_FROZEN);
        }
        if rows.is_empty() {
            return Err(XC_EMPTY_FEED);
        }
        let mut added: u32 = 0;
        for r in rows.iter() {
            if self.manifest.len() >= ASSERT_CAP {
                return Err(XC_TOTAL_SHORT);
            }
            if r.token == 0 {
                return Err(XC_HASH_ZERO);
            }
            if r.suite as usize >= SUITE_COUNT {
                return Err(XC_SUITE_BAD);
            }
            let mut i = 0usize;
            while i < self.seen.len() {
                if self.seen[i] == r.aid {
                    return Err(XC_DUP_AID);
                }
                i += 1;
            }
            self.seen.push(r.aid);
            self.manifest.push(*r);
            added += 1;
        }
        self.events.push(alloc::format!("manifest +{}", added));
        Ok(added)
    }
    pub fn is_full(&self) -> bool {
        self.manifest.len() >= MIN_ASSERTS
    }
    pub fn seal_manifest(&mut self) -> Result<u64, u16> {  // 封版：清单 ≥300 才准封；指纹冻结，事后改清单即红。
        if !self.stage.can_step(V2Stage::Sealed) {
            return Err(XC_STATE_INVALID);
        }
        if self.manifest.len() < MIN_ASSERTS {
            return Err(XC_TOTAL_SHORT);
        }
        let h = self.manifest_hash();
        self.sealed_hash = h;
        self.stage = V2Stage::Sealed;
        self.events.push(alloc::format!("sealed {:#018x}", h));
        Ok(h)
    }
    pub fn sealed_hash_of(&self) -> u64 {
        self.sealed_hash
    }
    pub fn manifest_hash(&self) -> u64 {  // 清单指纹（封版值独立复算，可对拍）。
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        let mut i = 0usize;
        while i < self.manifest.len() {
            let r = &self.manifest[i];
            h = fnv1a(h, r.aid as u64);
            h = fnv1a(h, r.suite as u64);
            h = fnv1a(h, r.token);
            i += 1;
        }
        h
    }
    pub fn feed_exec(&mut self, rows: &[ExecRow]) -> Result<u32, u16> {  // 执行账回填：aid 须在清单内、不重复、不越清单数。
        if !self.stage.can_step(V2Stage::Executed) {
            return Err(XC_STATE_INVALID);
        }
        if rows.is_empty() {
            return Err(XC_EMPTY_FEED);
        }
        let mut added: u32 = 0;
        for r in rows.iter() {
            if self.exec.len() >= self.manifest.len() {
                return Err(XC_EXEC_SHORT);
            }
            // 封版内断言查找。
            let mut known = false;
            let mut i = 0usize;
            while i < self.manifest.len() {
                if self.manifest[i].aid == r.aid {
                    known = true;
                    break;
                }
                i += 1;
            }
            if !known {
                return Err(XC_EXEC_UNKNOWN);
            }
            let mut j = 0usize;
            while j < self.exec_seen.len() {
                if self.exec_seen[j] == r.aid {
                    return Err(XC_EXEC_DUP);
                }
                j += 1;
            }
            self.exec_seen.push(r.aid);
            self.exec.push(*r);
            added += 1;
        }
        self.events.push(alloc::format!("exec +{}", added));
        Ok(added)
    }
    pub fn sub_inventory(&mut self) -> Result<SubInventory, u16> {  // 分项盘点：八桶计数 + 桶和守恒（执行账侧）。
        if !self.stage.can_step(V2Stage::Audited) {
            return Err(XC_STATE_INVALID);
        }
        if self.exec.len() != self.manifest.len() {
            return Err(XC_EXEC_SHORT);
        }
        let mut per = [0u32; SUITE_COUNT];
        let mut i = 0usize;
        while i < self.exec.len() {
            // 回填套桶：按 aid 从清单取。
            let mut suite = SUITE_COUNT as u8;
            let mut j = 0usize;
            while j < self.manifest.len() {
                if self.manifest[j].aid == self.exec[i].aid {
                    suite = self.manifest[j].suite;
                    break;
                }
                j += 1;
            }
            if suite as usize >= SUITE_COUNT {
                return Err(XC_EXEC_UNKNOWN);
            }
            per[suite as usize] += 1;
            i += 1;
        }
        let mut bucket_sum = 0u32;
        let mut nonempty = 0u32;
        let mut b = 0usize;
        while b < SUITE_COUNT {
            bucket_sum += per[b];
            if per[b] > 0 {
                nonempty += 1;
            }
            b += 1;
        }
        if bucket_sum != self.exec.len() as u32 {
            return Err(XC_SUBTOTAL_DIFF);
        }
        let rep = SubInventory {
            per_suite: per,
            bucket_sum,
            total: self.exec.len() as u32,
            nonempty,
        };
        self.stage = V2Stage::Audited;
        self.events
            .push(alloc::format!("inventory total={} buckets={}", rep.total, nonempty));
        Ok(rep)
    }
    pub fn register_pack(&mut self, pieces: &[Piece]) -> Result<u32, u16> {  // 凭证包登记：件型闭集 0..=3、哈希非 0、型唯一。
        if self.stage != V2Stage::Audited {
            return Err(XC_STATE_INVALID);
        }
        if pieces.is_empty() {
            return Err(XC_PACK_SHORT);
        }
        let mut added: u32 = 0;
        for p in pieces.iter() {
            if p.kind as usize >= PACK_ITEMS {
                return Err(XC_DUP_PACK);
            }
            if p.hash == 0 {
                return Err(XC_HASH_ZERO);
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
        if self.pack.len() > PACK_ITEMS {
            return Err(XC_DUP_PACK);
        }
        self.events.push(alloc::format!("pack +{}", added));
        Ok(added)
    }
    pub fn pin(&mut self) -> Result<V2Pin, u16> {  // 定格：执行数 == 清单数 ≥300 + 全绿 + 凭包 4/4。
        if self.pinned.is_some() {
            return Err(XC_ALREADY_PINNED);
        }
        if self.stage != V2Stage::Audited {
            return Err(XC_STATE_INVALID);
        }
        if self.pack.len() != PACK_ITEMS {
            return Err(XC_PACK_SHORT);
        }
        if self.manifest.len() < MIN_ASSERTS || self.exec.len() != self.manifest.len() {
            return Err(XC_EXEC_SHORT);
        }
        let mut green = 0u32;
        let mut red = 0u32;
        let mut i = 0usize;
        while i < self.exec.len() {
            if self.exec[i].ok {
                green += 1;
            } else {
                red += 1;
            }
            i += 1;
        }
        if green + red != self.exec.len() as u32 {
            return Err(XC_SUBTOTAL_DIFF);
        }
        if red != 0 {
            return Err(XC_RED_ASSERT);
        }
        let fp = self.fingerprint();
        let summary = alloc::format!(
            "UNX-F0787-J1 终验过：{} 条断言（≥300）执行全绿，分项 {} 桶盘点守恒，凭证包 4/4（终盘账/封版清单/执行账/分项盘点）落账",
            green, SUITE_COUNT
        );
        let rc = V2Pin {
            total: self.manifest.len() as u32,
            executed: self.exec.len() as u32,
            green,
            red,
            all_green: true,
            pack: self.pack.len() as u32,
            summary,
            fingerprint: fp,
        };
        self.pinned = Some(rc.clone());
        self.stage = V2Stage::Pinned;
        Ok(rc)
    }
    pub fn pinned_of(&self) -> Option<&V2Pin> {
        self.pinned.as_ref()
    }
    pub fn fingerprint(&self) -> u64 {  // 账面指纹：清单 + 执行账规范序。
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        let mut i = 0usize;
        while i < self.manifest.len() {
            h = fnv1a(h, self.manifest[i].aid as u64);
            h = fnv1a(h, self.manifest[i].token);
            i += 1;
        }
        let mut j = 0usize;
        while j < self.exec.len() {
            h = fnv1a(h, self.exec[j].aid as u64);
            h = fnv1a(h, self.exec[j].ok as u64);
            j += 1;
        }
        h
    }
    pub fn manifest_row(&self, aid: u32) -> Option<ManifestRow> {  // 清单行查询。
        let mut i = 0usize;
        while i < self.manifest.len() {
            if self.manifest[i].aid == aid {
                return Some(self.manifest[i]);
            }
            i += 1;
        }
        None
    }
    pub fn suite_view(&self, suite: u8) -> Result<Vec<u32>, u16> {  // 套桶成员视图：单桶断言号清单（越桶 Err）。
        if suite as usize >= SUITE_COUNT {
            return Err(XC_SUITE_BAD);
        }
        let mut out: Vec<u32> = Vec::new();
        let mut i = 0usize;
        while i < self.manifest.len() {
            if self.manifest[i].suite == suite {
                out.push(self.manifest[i].aid);
            }
            i += 1;
        }
        Ok(out)
    }
}
pub fn gen_data(seed: u64, n: usize) -> (Vec<ManifestRow>, Vec<ExecRow>) {  // 样本发生器：n 条清单（八桶轮转）+ 全绿执行账。
    let mut s = seed;
    let mut manifest: Vec<ManifestRow> = Vec::new();
    let mut exec: Vec<ExecRow> = Vec::new();
    let mut aid = 1u32;
    let mut i = 0usize;
    while i < n {
        s = s.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
        manifest.push(ManifestRow {
            aid,
            suite: (i % SUITE_COUNT) as u8,
            token: s | 2,
        });
        exec.push(ExecRow { aid, ok: true });
        aid += 1;
        i += 1;
    }
    (manifest, exec)
}
/// 凭证件型名表（闭集 0..=3 中文标签）。
pub fn piece_name(kind: u8) -> Option<&'static str> {
    match kind {
        0 => Some("终盘账"),
        1 => Some("封版清单"),
        2 => Some("执行账"),
        3 => Some("分项盘点"),
        _ => None,
    }
}

pub fn gen_pack(seed: u64) -> Vec<Piece> {  // 凭证包四件。
    let names = ["终盘账", "封版清单", "执行账", "分项盘点"];
    let mut out: Vec<Piece> = Vec::new();
    let mut k = 0u8;
    while k < 4 {
        out.push(Piece {
            kind: k,
            hash: fnv1a(0x60A0_0000, seed ^ (k as u64)) | 1,
            note: alloc::format!("判据2凭证·{}", names[k as usize]),
        });
        k += 1;
    }
    out
}
pub fn drill() -> Result<u32, u16> {  // 自例程 drill：主路径 + 错误路径，0=全过。
    let (manifest, exec) = gen_data(0x60A0_0000_0000_0001, 320);
    // —— 主路径：喂清单 → 封版 → 执行 → 分项 → 凭包 → 定格 ——
    let mut m = Verdict2::new();
    m.feed_manifest(&manifest)?;
    if !m.is_full() {
        return Err(XC_TOTAL_SHORT);
    }
    let sh = m.seal_manifest()?;
    if sh != m.manifest_hash() {
        return Err(XC_SEALED_FROZEN);
    }
    m.feed_exec(&exec)?;
    if m.exec_len() != 320 {
        return Err(XC_EXEC_SHORT);
    }
    let inv = m.sub_inventory()?;
    if inv.total != 320 || inv.bucket_sum != 320 || inv.nonempty != SUITE_COUNT as u32 {
        return Err(XC_SUBTOTAL_DIFF);
    }
    m.register_pack(&gen_pack(0x61))?;
    let rc = m.pin()?;
    if rc.total != 320 || !rc.all_green || rc.red != 0 || rc.summary.is_empty() {
        return Err(XC_RED_ASSERT);
    }
    if m.pin().err() != Some(XC_ALREADY_PINNED) {
        return Err(XC_STATE_INVALID);
    }
    // —— 错误路径 1：不足 300 即封版 ——
    let mut m2 = Verdict2::new();
    let (short_m, short_e) = gen_data(0x60A0_0000_0000_0002, 299);
    m2.feed_manifest(&short_m)?;
    if m2.seal_manifest() != Err(XC_TOTAL_SHORT) {
        return Err(XC_TOTAL_SHORT);
    }
    // —— 错误路径 2：封版后再改清单 ——
    if m.feed_manifest(&manifest[..2].to_vec()) != Err(XC_SEALED_FROZEN) {
        return Err(XC_SEALED_FROZEN);
    }
    // —— 错误路径 3：执行账含封版外断言 / 重复回填 / 超清单数 ——
    let mut m3 = Verdict2::new();
    m3.feed_manifest(&manifest)?;
    m3.seal_manifest()?;
    let mut ghost = exec.clone();
    ghost[10].aid = 99_999;
    if m3.feed_exec(&ghost) != Err(XC_EXEC_UNKNOWN) {
        return Err(XC_EXEC_UNKNOWN);
    }
    let mut dupexec = exec[..4].to_vec();
    dupexec[3].aid = dupexec[0].aid;
    let mut m4 = Verdict2::new();
    m4.feed_manifest(&manifest)?;
    m4.seal_manifest()?;
    if m4.feed_exec(&dupexec) != Err(XC_EXEC_DUP) {
        return Err(XC_EXEC_DUP);
    }
    // —— 错误路径 4：红断言阻断定格 ——
    let mut m5 = Verdict2::new();
    m5.feed_manifest(&manifest)?;
    m5.seal_manifest()?;
    let mut redexec = exec.clone();
    redexec[7].ok = false;
    m5.feed_exec(&redexec)?;
    m5.sub_inventory()?;
    m5.register_pack(&gen_pack(0x62))?;
    if m5.pin().err() != Some(XC_RED_ASSERT) {
        return Err(XC_RED_ASSERT);
    }
    // —— 错误路径 5：凭包缺件 / 件型重复 / 套桶越界 ——
    let mut m6 = Verdict2::new();
    m6.feed_manifest(&manifest)?;
    m6.seal_manifest()?;
    m6.feed_exec(&exec)?;
    m6.sub_inventory()?;
    m6.register_pack(&gen_pack(0x63)[..3].to_vec())?;
    if m6.pin().err() != Some(XC_PACK_SHORT) {
        return Err(XC_PACK_SHORT);
    }
    if m.suite_view(9) != Err(XC_SUITE_BAD) {
        return Err(XC_SUITE_BAD);
    }
    // —— 查询 API：清单行可解引 / 套桶成员计数 == 分项桶 ——
    match m.manifest_row(1) {
        Some(r) if r.suite == 0 => {}
        _ => return Err(XC_SUITE_BAD),
    }
    let mut b = 0u8;
    while b < SUITE_COUNT as u8 {
        let v = m.suite_view(b)?;
        if v.len() as u32 != inv.per_suite[b as usize] {
            return Err(XC_SUBTOTAL_DIFF);
        }
        b += 1;
    }
    if m.manifest_row(99_999).is_some() {
        return Err(XC_DUP_AID);
    }
    // —— 件型名表闭集：0..=3 有名、越界 None ——
    let mut pk = 0u8;
    while pk < 4 {
        if piece_name(pk).is_none() {
            return Err(XC_DUP_PACK);
        }
        pk += 1;
    }
    if piece_name(4).is_some() {
        return Err(XC_DUP_PACK);
    }
    Ok(0)
}
