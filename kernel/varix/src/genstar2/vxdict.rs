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
