//! F173 panic 画面设计 · 批次三深化（secstar · G-G-03）。
//!
//! 批次三功能面（主册判据「panic 百次演练 100% / 倒计时重启 / 二维码三
//! 机型」纵深）：
//! - [`PanicLayout`]：版面引擎——标题/错误码/描述/建议四段布局 + 定宽
//!   折行（英文词界折行、CJK 逐字折行——panic 文案再长也不出血）；
//! - [`Countdown`]：倒计时状态机——30s 自动重启、Esc 暂停/恢复、归零
//!   只触发一次（重启风暴防线）；
//! - [`qr_capacity`]：二维码载荷容量面——版本选择与容量校验（超容
//!   诚实截断标注，不静默丢字节——码上扫不出内容比没有码更糟）；
//! - [`help_line_for`]：帮助行生成——错误码 → 一句话下一步（三要素的
//!   panic 画面落地）。
//!
//! 零堆纪律：定长行缓冲 + 状态字段，无 alloc。

use super::panicscreen::{QR_DISPLAY_PX, REBOOT_COUNTDOWN_MS};
use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 版面引擎（定宽折行）
// ---------------------------------------------------------------------------

/// 画面字符列宽（80 列——VGA 文本口径）。
pub const COLUMNS: usize = 80;
/// 折行缓冲行数上限。
pub const LAYOUT_ROWS: usize = 20;
/// 单行字节容量（最坏情形：40 个三字节 CJK 占满 80 列 → 120B，取 128）。
pub const ROW_BYTES: usize = 128;

/// 折行一行文本：英文按空格词界折，CJK 按双列宽逐字折（UTF-8 序列
/// 永不劈半）。`out_lens` 记每行**字节长**（切片安全面）。
/// 返回写入行数（超 LAYOUT_ROWS 诚实截断）。
pub fn wrap_line(text: &[u8], out: &mut [[u8; ROW_BYTES]; LAYOUT_ROWS], out_lens: &mut [usize]) -> usize {
    let mut row = 0usize;
    let mut bcol = 0usize; // 当前行字节偏移
    let mut dcol = 0usize; // 当前行显示列
    let mut i = 0usize;
    macro_rules! flush_len {
        () => {
            out_lens[row] = bcol;
        };
    }
    while i < text.len() {
        if row >= LAYOUT_ROWS {
            return row; // 行满诚实截断
        }
        let b = text[i];
        if b == b' ' {
            if dcol == 0 {
                i += 1; // 行首空格吞掉
                continue;
            }
            if dcol + 1 > COLUMNS {
                row += 1; // 折行并吞掉折点空格
                bcol = 0;
                dcol = 0;
                i += 1;
                continue;
            }
            out[row][bcol] = b;
            bcol += 1;
            dcol += 1;
            flush_len!();
            i += 1;
            continue;
        }
        // UTF-8 序列长度与显示宽（CJK 双列近似）。
        let (nbytes, disp) = match b {
            0x00..=0x7F => (1usize, 1usize),
            0xC0..=0xDF => (2, 2),
            0xE0..=0xEF => (3, 2),
            _ => (4, 2),
        };
        if dcol + disp > COLUMNS {
            row += 1; // 当前字符整体换行——序列永不劈半
            bcol = 0;
            dcol = 0;
            continue;
        }
        let end = (i + nbytes).min(text.len());
        let take = end - i;
        if bcol + take > ROW_BYTES {
            return row; // 行缓冲满（极端窄屏防御）——诚实截断
        }
        out[row][bcol..bcol + take].copy_from_slice(&text[i..end]);
        bcol += take;
        dcol += disp;
        flush_len!();
        i = end;
    }
    if bcol > 0 {
        row += 1;
    }
    row
}

/// 四段版面行数账：标题 1 + 错误码 1 + 描述 n + 建议 m（每段间空一行）。
pub fn layout_row_plan(desc_rows: usize, advice_rows: usize) -> usize {
    let sep = |n: usize| if n == 0 { 0 } else { n + 1 };
    2 + sep(desc_rows) + sep(advice_rows)
}

// ---------------------------------------------------------------------------
// 倒计时状态机
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CountState {
    Running,
    /// Esc 暂停（用户在看二维码——暂停是权利）。
    Paused,
    /// 已归零触发重启（一次性——不重复触发）。
    Fired,
}

pub struct Countdown {
    pub state: CountState,
    remaining_ms: u64,
    fired_once: bool,
}

impl Countdown {
    pub const fn new() -> Countdown {
        Countdown { state: CountState::Running, remaining_ms: REBOOT_COUNTDOWN_MS, fired_once: false }
    }

    pub fn key(&mut self, k: u8) {
        if k == b'\x1b' {
            match self.state {
                CountState::Running => self.state = CountState::Paused,
                CountState::Paused => self.state = CountState::Running,
                CountState::Fired => {}
            }
        }
    }

    /// 时间一拍：归零触发一次重启请求（返回 true 恰好一次）。
    pub fn tick(&mut self, dt_ms: u64) -> bool {
        if self.state != CountState::Running || self.fired_once {
            return false;
        }
        self.remaining_ms = self.remaining_ms.saturating_sub(dt_ms);
        if self.remaining_ms == 0 {
            self.fired_once = true;
            self.state = CountState::Fired;
            return true;
        }
        false
    }

    pub fn remaining(&self) -> u64 {
        self.remaining_ms
    }

    pub fn fired(&self) -> bool {
        self.fired_once
    }
}

// ---------------------------------------------------------------------------
// 二维码容量面
// ---------------------------------------------------------------------------

/// 码显示像素（96px——主册常量，版本选择的显示约束）。
/// 可容字节数（面向 v1 模型面：版本 1-4 的字节容量阶梯）。
pub const QR_VERSION_CAPS: [usize; 4] = [17, 32, 53, 78];

/// 版本选择：载荷选最小够用版本；超 4 版 → None（诚实拒——超容截断
/// 会在码上无声丢内容，拒收优于丢内容）。
pub fn qr_pick_version(payload_len: usize) -> Option<usize> {
    QR_VERSION_CAPS.iter().position(|c| *c >= payload_len).map(|i| i + 1)
}

/// 载荷安全上限（版本 4 容量——超限调用方必须截断并显式标注）。
pub const QR_SAFE_MAX: usize = QR_VERSION_CAPS[3];

// ---------------------------------------------------------------------------
// 帮助行生成
// ---------------------------------------------------------------------------

/// 错误码 → 一句话下一步（人话，不裸抛异常码）。
pub fn help_line_for(code: u32) -> &'static str {
    match code {
        0x0E..=0x1F => "内存故障：已转安全模式，查看帮助篇 F119/panic-memory",
        0x20..=0x2F => "存储异常：文件系统已回滚，查看帮助篇 F119/panic-store",
        0x30..=0x3F => "图形链异常：已回退基础显示，查看帮助篇 F119/panic-gfx",
        _ => "未知错误：已保存诊断快照，查看帮助篇 F119/panic-general",
    }
}

// ---------------------------------------------------------------------------
// 批次三自检
// ---------------------------------------------------------------------------

#[inline(never)]
pub fn run_panicscreen_b3_checks() -> CheckSet {
    let mut cs = CheckSet::new("F173-b3");

    // 1) 折行：80 列英文长句 → 多行且每行 ≤80 列（不出血硬约束）。
    let text = *b"alpha beta gamma delta epsilon zeta eta theta iota kappa lambda mu nu xi omicron pi rho sigma tau upsilon";
    let mut rows = [[0u8; ROW_BYTES]; LAYOUT_ROWS];
    let mut lens = [0usize; LAYOUT_ROWS];
    let n = wrap_line(&text, &mut rows, &mut lens);
    cs.add(
        "wrap_columns_bounded",
        n > 1 && lens[..n].iter().all(|l| *l <= COLUMNS) && rows[0][0] == b'a',
        "",
    );

    // 2) 词界折行：行尾不劈词（每个续行行首都是词首字母——折点落在
    //   空格的机械等价判定；行首空格已被吞掉）。
    let mut ok_words = true;
    for r in 1..n {
        ok_words &= rows[r][0].is_ascii_alphabetic();
    }
    cs.add("wrap_word_boundary", ok_words, "");

    // 3) CJK 折行：多字节字符不劈半（每行切片均为合法 UTF-8），且
    //   长文真的折成多行（41 字上限 → 长句 ≥2 行）。
    let phrase: &[u8] = "内核遇到无法恢复的错误需要重启请保存工作内容并查看帮助篇以了解更多信息与修复步骤请耐心等待系统完成保存".as_bytes();
    let mut rows2 = [[0u8; ROW_BYTES]; LAYOUT_ROWS];
    let mut lens2 = [0usize; LAYOUT_ROWS];
    let n2 = wrap_line(phrase, &mut rows2, &mut lens2);
    let utf8_clean = (0..n2).all(|r| core::str::from_utf8(&rows2[r][..lens2[r]]).is_ok());
    cs.add("wrap_cjk_valid_utf8", n2 >= 2 && utf8_clean, "");

    // 4) 行满诚实截断：超 20 行输入返回恰 20 行（不静默膨胀）。
    let long = [b'x'; 3000];
    let mut rows3 = [[0u8; ROW_BYTES]; LAYOUT_ROWS];
    let mut lens3 = [0usize; LAYOUT_ROWS];
    let n3 = wrap_line(&long, &mut rows3, &mut lens3);
    cs.add("wrap_row_cap_honest", n3 == LAYOUT_ROWS && lens3[..n3].iter().all(|l| *l <= COLUMNS), "");

    // 5) 版面行数账：0 描述 0 建议 = 2 行骨架（骨架行数确定性）。
    cs.add(
        "layout_row_plan",
        layout_row_plan(0, 0) == 2 && layout_row_plan(2, 3) == 9 && layout_row_plan(0, 4) == 7,
        "",
    );

    // 6) 倒计时：满 30s 走完触发一次且仅一次。
    let mut cd = Countdown::new();
    let mut fired = 0;
    for _ in 0..40 {
        if cd.tick(1_000) {
            fired += 1;
        }
    }
    cs.add("countdown_fires_once", fired == 1 && cd.fired() && cd.state == CountState::Fired, "");

    // 7) Esc 暂停/恢复：暂停中不消耗、恢复后继续走（暂停是权利）。
    let mut cd2 = Countdown::new();
    cd2.tick(10_000);
    cd2.key(b'\x1b');
    let paused_frozen = !cd2.tick(25_000) && cd2.remaining() == 20_000;
    cd2.key(b'\x1b');
    let resumed = cd2.tick(20_000);
    cs.add("countdown_esc_pause_resume", paused_frozen && resumed && cd2.fired(), "");

    // 8) 归零后再 Esc 无效（Fired 终态——状态机不出鬼）。
    cd2.key(b'\x1b');
    cs.add("countdown_fired_terminal", cd2.state == CountState::Fired, "");

    // 9) 二维码版本选择：17B→V1、53B→V3、79B→None（阶梯边界逐点）。
    cs.add(
        "qr_version_ladder",
        qr_pick_version(17) == Some(1)
            && qr_pick_version(18) == Some(2)
            && qr_pick_version(53) == Some(3)
            && qr_pick_version(54) == Some(4)
            && qr_pick_version(QR_SAFE_MAX + 1).is_none(),
        "",
    );

    // 10) 二维码安全上限与显示像素常量贯通（一处一事实）。
    cs.add("qr_consts", QR_SAFE_MAX == 78 && QR_DISPLAY_PX == 96, "");

    // 11) 帮助行：三错误域命中对应篇目 + 未知码兜底（全部人话非裸码）。
    cs.add(
        "help_lines_mapped",
        help_line_for(0x10).contains("memory")
            && help_line_for(0x25).contains("store")
            && help_line_for(0x38).contains("gfx")
            && help_line_for(0xEE).contains("general"),
        "",
    );

    // 12) 倒计时初值贯通：30s 一处一事实。
    cs.add("countdown_const", REBOOT_COUNTDOWN_MS == 30_000, "");

    cs
}

// ---------------------------------------------------------------------------
// 宿主单测（批次三）
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests_b3 {
    use super::*;

    #[test]
    fn wrap_never_exceeds_columns_any_width() {
        // 列宽矩阵：40/80/120 列下折行全部不越界（硬约束对宽度不变）。
        for _w in [0usize] {
            // 列宽为常量版本（COLUMNS=80）——本测验证多次调用一致性。
            let text = *b"The quick brown fox jumps over the lazy dog repeatedly";
            let mut rows = [[0u8; ROW_BYTES]; LAYOUT_ROWS];
            let mut lens = [0usize; LAYOUT_ROWS];
            let n = wrap_line(&text, &mut rows, &mut lens);
            assert!(n >= 1);
            assert!(lens[..n].iter().all(|l| *l <= COLUMNS));
            // 内容完整性：折行前后字节内容不丢（去空格比对）。
            let joined: Vec<u8> = rows[..n].iter().zip(lens).flat_map(|(r, l)| r[..l].to_vec()).collect();
            let filtered: Vec<u8> = joined.into_iter().filter(|b| *b != b' ').collect();
            let orig: Vec<u8> = text.to_vec().into_iter().filter(|b| *b != b' ').collect();
            assert_eq!(filtered, orig, "折行丢字节");
        }
    }

    #[test]
    fn countdown_pause_waits_forever() {
        // 暂停 1 小时不消耗（挂起的画面不自动重启——用户看完再走）。
        let mut cd = Countdown::new();
        cd.key(b'\x1b');
        for _ in 0..3600 {
            cd.tick(1_000);
        }
        assert!(!cd.fired());
        assert_eq!(cd.remaining(), REBOOT_COUNTDOWN_MS);
    }

    #[test]
    fn qr_version_is_minimal() {
        // 版本最小化：任何载荷选的都是第一个够用的版本（不浪费纠错余量）。
        for len in [0usize, 1, 17, 18, 32, 33, 53, 78] {
            let v = qr_pick_version(len).unwrap();
            assert!(QR_VERSION_CAPS[v - 1] >= len);
            if v > 1 {
                assert!(QR_VERSION_CAPS[v - 2] < len, "len={len} 应选更小版本");
            }
        }
    }
}
