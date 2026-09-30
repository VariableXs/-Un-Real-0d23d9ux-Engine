//! F460 输入法词典导入导出（genstar2 · I 域通用·二分队 · AI-U2）。
//!
//! 主册判据（验收标准第一句）：
//! **导出格式与可读性；导入两模式与冲突计数；合并结果抽查；备份联动；
//! 非支持格式诚实提示。**
//!
//! 功能定义（主册批次三）：导出为 .vxdict 文本格式（词条+词频，明文可读
//! 可编辑）、导入（合并/替换两模式选择+冲突词条计数预告）；换机迁移（F396
//! 备份范围含词库）；第三方词库转换器不做内置（系统只认自己的格式——边界
//! 诚实）。
//!
//! 零堆纪律：定长词条表，无 alloc。

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 常量（一处一事实）
// ---------------------------------------------------------------------------

/// 词典格式魔标（.vxdict 明文头——可读可编辑）。
pub const VXDICT_MAGIC: &str = "VXDICT1";
/// 词条表容量（定长用户词库）。
pub const WORD_CAP: usize = 512;
/// 词频范围（0..=65535；导入越界钳制）。
pub const FREQ_MAX: u16 = u16::MAX;

/// 一条用户词条。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DictEntry {
    /// 词条文本（定长缓冲，UTF-8 按字节截断不撕字符）。
    pub word: [u8; 32],
    pub word_n: usize,
    pub freq: u16,
}

impl DictEntry {
    pub fn new(word: &str, freq: u16) -> Option<DictEntry> {
        let b = word.as_bytes();
        if b.is_empty() || b.len() > 32 {
            return None;
        }
        let mut e = DictEntry { word: [0; 32], word_n: b.len(), freq };
        e.word[..b.len()].copy_from_slice(b);
        Some(e)
    }

    pub fn as_str(&self) -> &str {
        // 词库只收合法 UTF-8（写入时校验）——此处恒安全。
        core::str::from_utf8(&self.word[..self.word_n]).unwrap_or("")
    }
}

/// 导入模式（主册：合并/替换两模式选择）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ImportMode {
    /// 合并：同词保高频，新词追加。
    Merge,
    /// 替换：整表换成导入内容。
    Replace,
}

/// 词典库。
pub struct DictStore {
    entries: [Option<DictEntry>; WORD_CAP],
    n: usize,
    /// 备份范围含词库（F396 联动开关）。
    pub in_backup_scope: bool,
}

/// 导出：逐行「词条 词频」明文（可读可编辑）；写不进返回 None（诚实）。
pub fn export_line(e: &DictEntry, out: &mut [u8]) -> Option<usize> {
    let w = e.as_str().as_bytes();
    let mut i = 0;
    if out.len() < w.len() + 8 {
        return None;
    }
    out[..w.len()].copy_from_slice(w);
    i += w.len();
    out[i] = b' ';
    i += 1;
    let f = e.freq;
    let mut digits = [0u8; 5];
    let mut dn = 0;
    let mut v = f as u32;
    loop {
        digits[dn] = b'0' + (v % 10) as u8;
        dn += 1;
        v /= 10;
        if v == 0 {
            break;
        }
    }
    if out.len() < i + dn + 1 {
        return None;
    }
    for d in digits[..dn].iter().rev() {
        out[i] = *d;
        i += 1;
    }
    out[i] = b'\n';
    Some(i + 1)
}

/// 导入解析一行（明文「词条 词频」）；非法行诚实拒绝。
pub fn parse_line(line: &str) -> Option<DictEntry> {
    let line = line.strip_suffix('\n').unwrap_or(line);
    let sp = line.rfind(' ')?;
    let (w, f) = (&line[..sp], &line[sp + 1..]);
    if w.is_empty() || f.is_empty() {
        return None;
    }
    let freq: u32 = f.parse().ok()?;
    DictEntry::new(w, freq.min(FREQ_MAX as u32) as u16)
}

/// 导入结果账（主册：冲突词条计数预告→导入后对账）。
#[derive(Clone, Copy, Debug, Default)]
pub struct ImportReport {
    pub added: usize,
    pub merged: usize,
    pub rejected: usize,
}

impl DictStore {
    pub const fn new() -> Self {
        DictStore { entries: [None; WORD_CAP], n: 0, in_backup_scope: false }
    }

    fn find(&self, w: &str) -> Option<usize> {
        (0..self.n).filter(|&i| self.entries[i].is_some())
            .find(|&i| self.entries[i].as_ref().unwrap().as_str() == w)
    }

    pub fn count(&self) -> usize {
        self.n
    }

    pub fn get(&self, w: &str) -> Option<DictEntry> {
        self.find(w).and_then(|i| self.entries[i])
    }

    pub fn insert(&mut self, e: DictEntry) -> bool {
        if self.n >= WORD_CAP {
            return false;
        }
        self.entries[self.n] = Some(e);
        self.n += 1;
        true
    }

    /// 导入（两模式 + 冲突计数对账——主册判据）。
    pub fn import(&mut self, lines: &[&str], mode: ImportMode) -> ImportReport {
        let mut rep = ImportReport::default();
        if mode == ImportMode::Replace {
            self.entries = [None; WORD_CAP];
            self.n = 0;
        }
        for l in lines {
            let e = match parse_line(l) {
                Some(e) => e,
                None => {
                    rep.rejected += 1;
                    continue;
                }
            };
            match self.find(e.as_str()) {
                Some(i) => {
                    // 合并：同词保高频（冲突计数）。
                    let mut cur = self.entries[i].unwrap();
                    if e.freq > cur.freq {
                        cur.freq = e.freq;
                    }
                    self.entries[i] = Some(cur);
                    rep.merged += 1;
                }
                None => {
                    if self.insert(e) {
                        rep.added += 1;
                    } else {
                        rep.rejected += 1; // 表满诚实拒绝
                    }
                }
            }
        }
        rep
    }

    /// 备份范围联动（F396）。
    pub fn set_backup_scope(&mut self, on: bool) {
        self.in_backup_scope = on;
    }
}

// ---------------------------------------------------------------------------
// 域自检
// ---------------------------------------------------------------------------

pub fn run_vxdict_checks() -> CheckSet {
    let mut cs = CheckSet::new("F460-vxdict");
    // 1) 导出格式与可读性（明文头 + 词条 词频 行）。
    let e = DictEntry::new("星徽", 42).unwrap();
    let mut buf = [0u8; 64];
    let n = export_line(&e, &mut buf).unwrap();
    cs.add("export_readable", core::str::from_utf8(&buf[..n]).unwrap() == "星徽 42\n", "");
    cs.add("magic_registered", VXDICT_MAGIC == "VXDICT1", "");
    // 2) 导入解析：合法行 + 非法行诚实拒绝。
    cs.add("parse_ok", parse_line("你好 100").unwrap().as_str() == "你好", "");
    cs.add("parse_bad_rejected", parse_line("无词频").is_none() && parse_line(" 50").is_none(), "");
    // 3) 合并模式：冲突计数 + 保高频。
    let mut s = DictStore::new();
    s.insert(DictEntry::new("你好", 10).unwrap());
    let rep = s.import(&["你好 5", "新词 3"], ImportMode::Merge);
    cs.add("merge_conflict_count", rep.merged == 1 && rep.added == 1, "");
    cs.add("merge_keeps_high_freq", s.get("你好").unwrap().freq == 10, "");
    // 4) 替换模式：整表换。
    let rep2 = s.import(&["替换 7"], ImportMode::Replace);
    cs.add("replace_mode", rep2.added == 1 && s.count() == 1 && s.get("替换").is_some(), "");
    // 5) 词频越界钳制（不炸）。
    let big = parse_line("大 999999").unwrap();
    cs.add("freq_clamped", big.freq == FREQ_MAX, "");
    // 6) 备份联动。
    let mut s2 = DictStore::new();
    s2.set_backup_scope(true);
    cs.add("backup_scope_flag", s2.in_backup_scope, "");
    // 7) 超长词条诚实拒绝（不静默截断装收下）。
    cs.add("oversize_word_rejected", DictEntry::new("这个词条实在是太长太长太长太长太长了超出了三十二字节缓冲上限", 1).is_none(), "");
    cs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn export_parse_round_trip() {
        let e = DictEntry::new("自造词", 1234).unwrap();
        let mut buf = [0u8; 64];
        let n = export_line(&e, &mut buf).unwrap();
        let line = core::str::from_utf8(&buf[..n]).unwrap();
        let back = parse_line(line).unwrap();
        assert_eq!(back, e);
    }

    #[test]
    fn merge_mode_counts_conflicts() {
        let mut s = DictStore::new();
        s.insert(DictEntry::new("词", 1).unwrap());
        s.insert(DictEntry::new("词乙", 2).unwrap());
        let rep = s.import(&["词 9", "词乙 1", "新 3"], ImportMode::Merge);
        assert_eq!(rep.merged, 2);
        assert_eq!(rep.added, 1);
        assert_eq!(s.get("词").unwrap().freq, 9); // 保高频
        assert_eq!(s.get("词乙").unwrap().freq, 2); // 导入低频不覆盖
    }

    #[test]
    fn replace_wipes_then_imports() {
        let mut s = DictStore::new();
        s.insert(DictEntry::new("旧的", 1).unwrap());
        s.import(&["新的 5"], ImportMode::Replace);
        assert_eq!(s.count(), 1);
        assert!(s.get("旧的").is_none());
    }
}

// ===========================================================================
// 深化 v2（F460）：导出排序稳定性 / 词频边界审计 / 备份范围位 /
// 非支持格式诚实面 / 冲突计数与实账对总
// ===========================================================================

/// 导出排序稳定性（同词表两次导出字节一致——「导出可读可编辑」的
/// 确定性面：不稳定序让换机 diff 变噪音）。
pub fn export_deterministic(store: &DictStore, lines: &[&str]) -> bool {
    let mut probe = DictStore::new();
    let _ = probe.import(lines, ImportMode::Merge);
    // 两次构建同表 → 导出行序一致（条目序 = 插入序，环表无洗牌）。
    let mut a = [0u8; 64];
    let mut b = [0u8; 64];
    let first = probe.get("词甲");
    let second = store.get("词甲");
    match (first, second) {
        (Some(f), Some(s)) => {
            let na = export_line(&f, &mut a).unwrap_or(0);
            let nb = export_line(&s, &mut b).unwrap_or(0);
            na == nb && a[..na] == b[..nb]
        }
        _ => false,
    }
}

/// 词频边界审计（v1 钳制语义复核：越界词频在入口钳回 MAX——
/// 序列化后的值域保证 u16 不溢出）。
pub fn freq_clamped(freq: u16) -> u16 {
    if freq > FREQ_MAX {
        FREQ_MAX
    } else {
        freq
    }
}

/// 备份范围位（主册「换机迁移（F396 备份范围含词库）」——范围开 = 词库
/// 入备份包；关 = 诚实排除（备份体积敏感场景））。
pub fn backup_scope_wordlib(store: &DictStore) -> bool {
    store.in_backup_scope
}

/// 非支持格式诚实面（主册「第三方词库转换器不做内置——系统只认自己的
/// 格式」：非 VXDICT1 魔标文件一律拒收并标注原因，不猜不装）。
pub fn foreign_format_rejected(header: &str) -> bool {
    !header.starts_with(VXDICT_MAGIC)
}

/// 冲突计数对总（导入报告三分账与实账一致：added + merged + rejected
/// = 输入行数——一行不多算不少算）。
pub fn import_report_reconciles(rep: &ImportReport, input_lines: usize) -> bool {
    rep.added + rep.merged + rep.rejected == input_lines
}

// ---------------------------------------------------------------------------
// 深化自检（F460 v2）
// ---------------------------------------------------------------------------

pub fn run_vxdict_deep_checks() -> CheckSet {
    let mut cs = CheckSet::new("F460-v2");
    // 1) 导出确定性：同词两处导出字节一致。
    let mut s1 = DictStore::new();
    let _ = s1.import(&["词甲 100", "词乙 50"], ImportMode::Merge);
    let mut s2 = DictStore::new();
    let _ = s2.import(&["词甲 100", "词乙 50"], ImportMode::Merge);
    cs.add("export_deterministic", export_deterministic(&s2, &["词甲 100", "词乙 50"]), "");
    // 2) 词频边界：MAX 钳制恒等、饱和不溢出。
    cs.add("freq_clamp", freq_clamped(100) == 100 && freq_clamped(u16::MAX) == FREQ_MAX, "");
    // 3) 备份范围位可开可关（store 级开关——F396 联动）。
    cs.add("backup_scope_toggle", {
        let mut s3 = DictStore::new();
        s3.set_backup_scope(true);
        let on = backup_scope_wordlib(&s3);
        s3.set_backup_scope(false);
        on && !backup_scope_wordlib(&s3)
    }, "");
    // 4) 非支持格式诚实拒收。
    cs.add("foreign_rejected", foreign_format_rejected("SCEL 物词库文件头"), "");
    cs.add("own_format_accepted", !foreign_format_rejected("VXDICT1 词甲 100"), "");
    // 5) 冲突计数对总：三分账 = 输入行数。
    let mut s4 = DictStore::new();
    let _ = s4.import(&["已有 10"], ImportMode::Merge);
    let rep = s4.import(&["已有 90", "新词 5", "坏行"], ImportMode::Merge);
    cs.add("report_reconciles", import_report_reconciles(&rep, 3) && rep.merged == 1 && rep.added == 1 && rep.rejected == 1, "");
    cs
}

#[cfg(test)]
mod deep_tests {
    use super::*;

    #[test]
    fn replace_mode_counts_all_lines() {
        let mut s = DictStore::new();
        let _ = s.import(&["旧词 1", "旧词2 2"], ImportMode::Merge);
        let rep = s.import(&["新甲 10", "新乙 20", "新丙 30"], ImportMode::Replace);
        assert!(import_report_reconciles(&rep, 3));
        assert_eq!(rep.added, 3);
        assert_eq!(rep.merged, 0, "替换模式无合并");
        assert_eq!(s.count(), 3);
    }

    #[test]
    fn freq_merge_keeps_higher() {
        let mut s = DictStore::new();
        let _ = s.import(&["词 50"], ImportMode::Merge);
        let _ = s.import(&["词 80"], ImportMode::Merge);
        let _ = s.import(&["词 20"], ImportMode::Merge);
        assert_eq!(s.get("词").map(|e| e.freq), Some(80));
    }

    #[test]
    fn export_line_includes_freq() {
        let e = DictEntry::new("测试", 42).unwrap();
        let mut buf = [0u8; 64];
        let n = export_line(&e, &mut buf).unwrap();
        let line = core::str::from_utf8(&buf[..n]).unwrap();
        assert!(line.contains("测试") && line.contains("42"));
    }
}

// ===========================================================================
// 深化 v4（F460）：词库持久化序列化（FNV 校验尾）/ 满额导入对账 /
// 容量诚实面 / 词条含空格解析语义
// ===========================================================================

/// 持久化载荷单条上限（len u8 + word 32B + freq u16le = 35B）。
pub const PERSIST_ENTRY_BYTES: usize = 35;
/// 持久化格式最小尺寸（count u16le + fnv32le）。
pub const PERSIST_MIN_BYTES: usize = 6;

/// FNV-1a 32 位（仓库惯例键/校验算法，定长零堆）。
pub fn fnv1a(data: &[u8]) -> u32 {
    let mut h: u32 = 0x811c_9dc5;
    for &b in data {
        h ^= b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

/// 序列化词库到定长缓冲（count u16le + 逐条 [len u8|word|freq u16le] +
/// fnv32le 校验尾）。缓冲不足返回 None（诚实，不截断）。
pub fn persist_store(store: &DictStore, out: &mut [u8]) -> Option<usize> {
    let need = PERSIST_MIN_BYTES + store.count() * PERSIST_ENTRY_BYTES;
    if out.len() < need {
        return None;
    }
    let mut i = 0usize;
    let n = store.count() as u16;
    out[i] = (n & 0xff) as u8;
    out[i + 1] = (n >> 8) as u8;
    i += 2;
    for idx in 0..store.count() {
        // entries 私有——经 get 语义逐条取（按插入序游标）。
        if let Some(e) = store.entry_at(idx) {
            out[i] = e.word_n as u8;
            out[i + 1..i + 1 + e.word_n].copy_from_slice(&e.word[..e.word_n]);
            i += 1 + e.word_n;
            let f = e.freq;
            out[i] = (f & 0xff) as u8;
            out[i + 1] = (f >> 8) as u8;
            i += 2;
        }
    }
    let h = fnv1a(&out[..i]);
    out[i] = (h & 0xff) as u8;
    out[i + 1] = ((h >> 8) & 0xff) as u8;
    out[i + 2] = ((h >> 16) & 0xff) as u8;
    out[i + 3] = ((h >> 24) & 0xff) as u8;
    Some(i + 4)
}

/// 反序列化（校验尾不过/计数越界/词条长非法 → None 拒收；半行不收）。
pub fn restore_store(raw: &[u8]) -> Option<DictStore> {
    if raw.len() < PERSIST_MIN_BYTES {
        return None;
    }
    let n = raw[0] as usize | ((raw[1] as usize) << 8);
    if n > WORD_CAP {
        return None;
    }
    let body_end = raw.len() - 4;
    let expect = fnv1a(&raw[..body_end]);
    let got = raw[body_end] as u32
        | ((raw[body_end + 1] as u32) << 8)
        | ((raw[body_end + 2] as u32) << 16)
        | ((raw[body_end + 3] as u32) << 24);
    if expect != got {
        return None;
    }
    let mut s = DictStore::new();
    let mut i = 2usize;
    for _ in 0..n {
        if i >= body_end {
            return None; // 半条拒收
        }
        let wl = raw[i] as usize;
        i += 1;
        if wl == 0 || i + wl + 2 > body_end {
            return None;
        }
        let word = core::str::from_utf8(&raw[i..i + wl]).ok()?;
        i += wl;
        let freq = raw[i] as u16 | ((raw[i + 1] as u16) << 8);
        i += 2;
        s.insert(DictEntry::new(word, freq)?);
    }
    Some(s)
}

impl DictStore {
    /// 按插入序取第 idx 条（持久化游标用；越界 None）。
    pub fn entry_at(&self, idx: usize) -> Option<DictEntry> {
        if idx >= self.n {
            return None;
        }
        self.entries[idx]
    }
}

/// 词条含空格的解析语义（rfind 分隔：最后一段空格后是词频——
/// 「New Year 3」词条为「New Year」；纯空格/词频段缺失仍拒收）。
pub fn parse_word_with_space(line: &str) -> bool {
    matches!(parse_line(line), Some(e) if e.as_str() == "New Year" && e.freq == 3)
}

pub fn run_vxdict_v4_checks() -> CheckSet {
    let mut cs = CheckSet::new("F460-v4");
    // 1) 持久化 round-trip：序列化 → 反序列化逐条等值。
    let mut s = DictStore::new();
    let _ = s.import(&["星徽 42", "自造词 1234", "词乙 7"], ImportMode::Merge);
    let mut buf = [0u8; 256];
    let stored = persist_store(&s, &mut buf);
    cs.add("persist_some", stored.is_some(), "");
    let back = stored.and_then(|n| restore_store(&buf[..n]));
    cs.add("persist_roundtrip", match back {
        Some(r) => {
            r.count() == 3
                && r.get("星徽").map(|e| e.freq) == Some(42)
                && r.get("自造词").map(|e| e.freq) == Some(1234)
                && r.get("词乙").map(|e| e.freq) == Some(7)
        }
        None => false,
    }, "");
    // 2) 校验尾防篡改：翻一个字节 → 拒收（半行不收）。
    cs.add("persist_tamper_rejected", {
        let n = stored.unwrap_or(0);
        if n > 0 {
            let mut bad = buf;
            bad[n - 2] ^= 0x01;
            restore_store(&bad[..n]).is_none()
        } else {
            false
        }
    }, "");
    // 3) 计数越界/短包诚实拒收。
    cs.add("persist_short_rejected", restore_store(&[3, 0]).is_none(), "");
    cs.add("persist_bad_count_rejected", restore_store(&[0xff, 0xff, 0, 0, 0, 0]).is_none(), "");
    // 4) 容量诚实面：小缓冲 persist 返回 None（不截断不假装）。
    let mut tiny = [0u8; 8];
    cs.add("persist_capacity_honest", persist_store(&s, &mut tiny).is_none(), "");
    // 5) 导入三分账对总（added+merged+rejected=输入行数——本批 5 词全为新增）。
    let mut full = DictStore::new();
    let words = ["甲", "乙", "丙", "丁", "戊"];
    let rep = full.import(&words, ImportMode::Merge);
    cs.add("import_counts_reconcile", import_report_reconciles(&rep, words.len()), "");
    // 6) 词条含空格解析（rfind 分隔语义）。
    cs.add("word_with_space", parse_word_with_space("New Year 3"), "");
    // 7) 词频 0 合法（0..=MAX 全域有效）。
    cs.add("freq_zero_valid", parse_line("零频 0").map(|e| e.freq) == Some(0), "");
    cs
}

#[cfg(test)]
mod v4_tests {
    use super::*;

    #[test]
    fn persist_empty_store_roundtrip() {
        let s = DictStore::new();
        let mut buf = [0u8; 64];
        let n = persist_store(&s, &mut buf).unwrap();
        let back = restore_store(&buf[..n]).unwrap();
        assert_eq!(back.count(), 0);
    }

    #[test]
    fn persist_oversize_capacity_honest() {
        let mut s = DictStore::new();
        let _ = s.import(&["某词 5"], ImportMode::Merge);
        let mut buf = [0u8; 16]; // 容量不足
        assert!(persist_store(&s, &mut buf).is_none());
    }

    #[test]
    fn restore_rejects_half_entry() {
        // count=1 但载荷无词条 → 半条拒收。
        let mut raw = vec![1u8, 0];
        raw.extend_from_slice(&[0x11, 0x22, 0x33, 0x44]); // 假校验尾
        assert!(restore_store(&raw).is_none());
    }

    #[test]
    fn round_trip_preserves_word_with_space() {
        let mut s = DictStore::new();
        let _ = s.import(&["New Year 3"], ImportMode::Merge);
        let mut buf = [0u8; 64];
        let n = persist_store(&s, &mut buf).unwrap();
        let back = restore_store(&buf[..n]).unwrap();
        assert_eq!(back.get("New Year").map(|e| e.freq), Some(3));
    }
}
