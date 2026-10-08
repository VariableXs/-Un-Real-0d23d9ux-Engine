//! 深化层 · F588 图片粘贴为文件（回炉补深主册【设计要点】未实装机制）。
//!
//! 补深四条（判据唯一源：主册 F588 节）：
//! ① **命名引擎**——epoch 毫秒→「截图 YYYY-MM-DD_HHMM」戳（F521 同规）
//!   + 占用名账上的重名递增（复用 ibase::dedupe_name 唯一实现）；
//! ② **PNG 落盘两段提交账**——写临时名（.part）→ 完成 → 正名，中断
//!   即弃置（abort 零副作用、临时名不入占用账）；
//! ③ **格式栈守恒账**——粘贴取的是剪贴板视图，内容不消费（可重复粘贴）；
//! ④ 三场景分派复用基础件 [`PasteImg`]（分派语义不重写，只对账）。

use crate::checks::CheckSet;
use crate::istar::ibase::{dedupe_name, ISTAR_DOMAIN};
use crate::istar::pasteimg::{PasteImg, PasteOutcome, PasteScene, IMAGE_EXT};

use alloc::string::String;
use alloc::vec::Vec;

// --- ① 命名引擎 ------------------------------------------------------------

/// 手写数字追加（禁 format!——递归除 10 逐位压栈）。
fn push_num(s: &mut String, v: u64) {
    if v >= 10 {
        push_num(s, v / 10);
    }
    s.push((b'0' + (v % 10) as u8) as char);
}

/// 两位补零追加（月/日/时/分定宽）。
fn push2(s: &mut String, v: u32) {
    if v < 10 {
        s.push('0');
    }
    push_num(s, v as u64);
}

/// 儒略日数 → (年, 月, 日)（Hinnant civil_from_days，无查表）。
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64; // [0, 146096]
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

/// epoch 毫秒 → 命名戳「截图 2026-09-25_1430」（F521 同规形制唯一实现）。
pub fn stamp_from_epoch(ms: u64) -> String {
    let days = (ms / 86_400_000) as i64;
    let (y, mo, d) = civil_from_days(days);
    let hh = (ms / 3_600_000) % 24;
    let mi = (ms / 60_000) % 60;
    let mut s = String::from("截图 ");
    push_num(&mut s, y as u64);
    s.push('-');
    push2(&mut s, mo);
    s.push('-');
    push2(&mut s, d);
    s.push('_');
    push2(&mut s, hh as u32);
    push2(&mut s, mi as u32);
    s
}

/// 命名引擎：落点目录的占用名账 + 重名递增（dedupe_name 复用）。
pub struct NamingEngine {
    taken: Vec<String>,
}

impl NamingEngine {
    pub fn new() -> NamingEngine {
        NamingEngine { taken: Vec::new() }
    }

    /// 领名：重名自动「 (2)」「 (3)」递增；领到即入占用账。
    /// 递增上限耗尽（dedupe_name 诚实 None）不领不占。
    pub fn claim(&mut self, stamp: &str) -> Option<String> {
        let refs: Vec<&str> = self.taken.iter().map(|s| s.as_str()).collect();
        let name = dedupe_name(stamp, &refs)?;
        self.taken.push(name.clone());
        Some(name)
    }

    pub fn taken_count(&self) -> usize {
        self.taken.len()
    }
}

impl Default for NamingEngine {
    fn default() -> Self {
        Self::new()
    }
}

// --- ② PNG 落盘两段提交 ------------------------------------------------------

/// 两段提交状态机（Idle→Writing→Committed / Aborted）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SavePhase {
    Idle,
    /// 写临时名中：(临时名, 预解析正名)。
    Writing { temp: String, dest: String },
    Committed { dest: String },
    Aborted,
}

/// PNG 落盘两段提交账：写临时名 → 完成 → 正名。
pub struct TwoPhaseSave {
    phase: SavePhase,
    committed: Vec<String>,
}

impl TwoPhaseSave {
    pub fn new() -> TwoPhaseSave {
        TwoPhaseSave { phase: SavePhase::Idle, committed: Vec::new() }
    }

    /// 第一段：开临时名写盘（「.」前缀 +「.part」后缀——中断产物不冒充
    /// 成品；正名此刻预解析，临时名不占正名位）。
    pub fn begin(&mut self, stamp: &str, taken: &[&str]) -> Option<String> {
        if !matches!(self.phase, SavePhase::Idle) {
            return None;
        }
        let dest = dedupe_name(stamp, taken)?;
        let mut temp = String::from(".");
        temp.push_str(&dest);
        temp.push_str(IMAGE_EXT);
        temp.push_str(".part");
        self.phase = SavePhase::Writing { temp: temp.clone(), dest: dest.clone() };
        Some(temp)
    }

    /// 第二段：写完正名（临时名退场，正名入已提交账）。
    pub fn commit(&mut self) -> Option<String> {
        if !matches!(self.phase, SavePhase::Writing { .. }) {
            return None;
        }
        let dest = match core::mem::replace(&mut self.phase, SavePhase::Idle) {
            SavePhase::Writing { dest, .. } => dest,
            _ => return None,
        };
        self.committed.push(dest.clone());
        self.phase = SavePhase::Committed { dest: dest.clone() };
        Some(dest)
    }

    /// 中断弃置：临时名作废、零副作用（已提交账纹丝不动）。
    pub fn abort(&mut self) -> bool {
        if matches!(self.phase, SavePhase::Writing { .. }) {
            self.phase = SavePhase::Aborted;
            true
        } else {
            false
        }
    }

    pub fn phase(&self) -> &SavePhase {
        &self.phase
    }

    pub fn committed_count(&self) -> usize {
        self.committed.len()
    }
}

impl Default for TwoPhaseSave {
    fn default() -> Self {
        Self::new()
    }
}

// --- ③ 格式栈守恒账 ----------------------------------------------------------

/// 剪贴板守恒账：粘贴取视图不消费内容（可重复粘贴）。
pub struct ClipboardHold {
    is_image: bool,
    paste_views: u32,
}

impl ClipboardHold {
    pub fn new(is_image: bool) -> ClipboardHold {
        ClipboardHold { is_image, paste_views: 0 }
    }

    /// 取一次粘贴视图（有位图格式才取得到；内容不因取而消耗）。
    pub fn paste_view(&mut self) -> bool {
        if self.is_image {
            self.paste_views += 1;
            true
        } else {
            false
        }
    }

    /// 守恒判据：取过视图后内容仍在（可再粘）。
    pub fn content_retained(&self) -> bool {
        self.is_image
    }

    pub fn view_count(&self) -> u32 {
        self.paste_views
    }
}

// --- 深化自检 ---------------------------------------------------------------

pub fn run_f588_deep_checks() -> CheckSet {
    let mut cs = CheckSet::new(ISTAR_DOMAIN);

    // 1) 命名戳：epoch 毫秒 →「截图 2026-09-25_1430」（F521 同规形制）。
    cs.add("stamp f521 format", stamp_from_epoch(1_790_346_600_000) == "截图 2026-09-25_1430", "");

    // 2) 补零定宽：1 月 5 日 09:05 → 01-05_0905（不定宽即错名）。
    cs.add("stamp zero padded", stamp_from_epoch(1_767_603_900_000) == "截图 2026-01-05_0905", "");

    // 3) 重名递增：同戳连领 → 基名→(2)→(3)（dedupe_name 语义复用）。
    let mut eng = NamingEngine::new();
    let n1 = eng.claim("截图 2026-09-25_1430");
    let n2 = eng.claim("截图 2026-09-25_1430");
    let n3 = eng.claim("截图 2026-09-25_1430");
    cs.add(
        "duplicate increments sequence",
        n1.as_deref() == Some("截图 2026-09-25_1430")
            && n2.as_deref() == Some("截图 2026-09-25_1430 (2)")
            && n3.as_deref() == Some("截图 2026-09-25_1430 (3)")
            && eng.taken_count() == 3,
        "",
    );

    // 4) 两段提交：临时名隐藏 .part、与正名不同；commit 正名入账。
    let mut tp = TwoPhaseSave::new();
    let temp = tp.begin("截图 2026-09-25_1430", &[]);
    let committed = tp.commit();
    let want = SavePhase::Committed { dest: String::from("截图 2026-09-25_1430") };
    cs.add(
        "two phase commit renames",
        temp.as_deref() == Some(".截图 2026-09-25_1430.png.part")
            && committed.as_deref() == Some("截图 2026-09-25_1430")
            && tp.committed_count() == 1
            && *tp.phase() == want,
        "",
    );

    // 5) 中断弃置：abort 后已提交账零增量（零副作用）。
    let mut tp2 = TwoPhaseSave::new();
    let _ = tp2.begin("截图 2026-09-25_1440", &[]);
    let aborted = tp2.abort();
    cs.add(
        "abort zero side effects",
        aborted && tp2.committed_count() == 0 && *tp2.phase() == SavePhase::Aborted,
        "",
    );

    // 6) 状态机纪律：未 begin 不得 commit/abort（跳步诚实拒绝）。
    let mut tp3 = TwoPhaseSave::new();
    cs.add("no skip step", tp3.commit().is_none() && !tp3.abort(), "");

    // 7) 格式栈守恒：同戳粘两次两张、内容仍在（可重复粘贴）。
    let mut hold = ClipboardHold::new(true);
    let mut eng2 = NamingEngine::new();
    let v1 = hold.paste_view();
    let f1 = if v1 { eng2.claim("截图 2026-09-25_1450") } else { None };
    let v2 = hold.paste_view();
    let f2 = if v2 { eng2.claim("截图 2026-09-25_1450") } else { None };
    cs.add(
        "clipboard conserved on paste",
        v1 && v2 && hold.content_retained() && hold.view_count() == 2
            && f1.is_some() && f2.as_deref() == Some("截图 2026-09-25_1450 (2)"),
        "",
    );

    // 8) 三场景分派复用基础件：目录落 PNG、对话框落引用（分派不重写）。
    let mut p = PasteImg::new();
    let o_folder = p.paste(PasteScene::Folder, true, "截图 2026-09-25_1500", &[]);
    let o_dialog = p.paste(PasteScene::FileDialog, true, "截图 2026-09-25_1500", &[]);
    cs.add(
        "scene dispatch via base",
        o_folder == PasteOutcome::Saved {
            dir: String::from("当前目录"),
            name: String::from("截图 2026-09-25_1500.png"),
        } && o_dialog == PasteOutcome::Reference {
            name: String::from("截图 2026-09-25_1500.png"),
        },
        "",
    );

    cs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn epoch_zero_epoch() {
        assert_eq!(stamp_from_epoch(0), "截图 1970-01-01_0000");
    }

    #[test]
    fn commit_without_begin_none() {
        let mut tp = TwoPhaseSave::new();
        assert!(tp.commit().is_none());
    }

    #[test]
    fn non_image_hold_denied() {
        let mut h = ClipboardHold::new(false);
        assert!(!h.paste_view());
        assert!(!h.content_retained());
        assert_eq!(h.view_count(), 0);
    }
}
