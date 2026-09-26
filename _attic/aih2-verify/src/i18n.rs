//! VARIX-M500 AI-20 · 全球化与作品集交付（F476~F500，M5）
//!
//! 全世界可用、作品集定稿——i18n、无障碍、发行、十年愿景。
//! 纯逻辑 + 固定容量数组（no_std），域自检 F500 汇入 `robust::run_kernel_checkup()`。

use crate::checks::CheckSet;

pub const MAX_LOCALES: usize = 8;

// ---------------------------------------------------------------------------
// F476 i18n 框架深化
// ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
pub struct StringTable {
    /// 每语言一组 key→value（value 为索引到 pooled strings 的占位实现：直接存 id）。
    pub locales: [(u16, [(u16, u16); 4], usize); MAX_LOCALES],
    pub len: usize,
}

impl StringTable {
    pub const fn new() -> StringTable {
        StringTable { locales: [(0, [(0, 0); 4], 0); MAX_LOCALES], len: 0 }
    }

    pub fn add_locale(&mut self, lang: u16) -> bool {
        if self.len >= MAX_LOCALES || self.locales[..self.len].iter().any(|&(l, _, _)| l == lang) {
            return false;
        }
        self.locales[self.len] = (lang, [(0, 0); 4], 0);
        self.len += 1;
        true
    }

    /// 查找：目标语言 → 英文兜底 → 任意第一个。
    pub fn lookup(&self, lang: u16, key: u16) -> Option<u16> {
        let find = |idx: usize| -> Option<u16> {
            let (_, kv, n) = self.locales[idx];
            kv[..n].iter().find(|&&(k, _)| k == key).map(|&(_, v)| v)
        };
        for i in 0..self.len {
            if self.locales[i].0 == lang {
                if let Some(v) = find(i) {
                    return Some(v);
                }
            }
        }
        for i in 0..self.len {
            if self.locales[i].0 == 0x0409 {
                if let Some(v) = find(i) {
                    return Some(v);
                }
            }
        }
        for i in 0..self.len {
            if let Some(v) = find(i) {
                return Some(v);
            }
        }
        None
    }

    pub fn put(&mut self, lang: u16, key: u16, val: u16) -> bool {
        for i in 0..self.len {
            if self.locales[i].0 == lang {
                let (_, kv, n) = &mut self.locales[i];
                if *n < 4 {
                    kv[*n] = (key, val);
                    *n += 1;
                    return true;
                }
                return false;
            }
        }
        false
    }
}

// ---------------------------------------------------------------------------
// F477 翻译术语库
// ---------------------------------------------------------------------------

/// 术语一致性：同一原文 key 在所有语言必须登记（缺译即不一致）。
pub fn termbase_consistent(table: &StringTable, key: u16) -> bool {
    for i in 0..table.len {
        let (_, kv, n) = table.locales[i];
        if !kv[..n].iter().any(|&(k, _)| k == key) {
            return false;
        }
    }
    table.len > 0
}

// ---------------------------------------------------------------------------
// F478 区域格式引擎
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Locale {
    Zh,
    En,
    De,
}

/// 数字千分位与货币符号（简化：返回 (符号, 分组位数)）。
pub fn locale_number(l: Locale) -> (&'static str, u8) {
    match l {
        Locale::Zh | Locale::En => ("¥", 3),
        Locale::De => ("€", 3),
    }
}

/// 日期顺序：Zh 年月日 / En 月日年 / De 日月年（返回占位序）。
pub fn locale_date_order(l: Locale) -> (u8, u8, u8) {
    match l {
        Locale::Zh => (0, 1, 2), // Y M D
        Locale::En => (1, 2, 0), // M D Y
        Locale::De => (2, 1, 0), // D M Y
    }
}

// ---------------------------------------------------------------------------
// F479 时区管家
// ---------------------------------------------------------------------------

/// 旅行调整：新时区偏移（分钟）换算本地时刻（分钟 of day，回绕）。
pub fn tz_convert(minute_of_day: i32, from_off: i32, to_off: i32) -> i32 {
    let shifted = minute_of_day + (to_off - from_off);
    shifted.rem_euclid(24 * 60)
}

// ---------------------------------------------------------------------------
// F480 多历法预留
// ---------------------------------------------------------------------------

/// 农历预留：公历→农历仅提供接口占位（固定映射表头两项）。
pub fn lunar_probe(gregorian_ymd: (u16, u8, u8)) -> Option<(u16, u8)> {
    match gregorian_ymd {
        (2026, 2, 17) => Some((4724, 1)), // 春节示例
        (2026, 9, 25) => Some((4724, 8)),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// F481 朗读引擎接口
// ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
pub struct TtsQueue {
    items: [u16; 4],
    len: usize,
    speaking: usize,
}

impl TtsQueue {
    pub const fn new() -> TtsQueue {
        TtsQueue { items: [0; 4], len: 0, speaking: 0 }
    }

    pub fn enqueue(&mut self, text_id: u16) -> bool {
        if self.len >= 4 {
            return false;
        }
        self.items[self.len] = text_id;
        self.len += 1;
        true
    }

    pub fn next(&mut self) -> Option<u16> {
        if self.speaking >= self.len {
            return None;
        }
        let v = self.items[self.speaking];
        self.speaking += 1;
        Some(v)
    }
}

// ---------------------------------------------------------------------------
// F482 系统字幕
// ---------------------------------------------------------------------------

/// 字幕时间轴：给定播放时刻返回应显示的字幕 id。
pub fn caption_at(cues: &[(u32, u32, u16)], t_ms: u32) -> Option<u16> {
    cues.iter().find(|&&(start, end, _)| t_ms >= start && t_ms < end).map(|&(_, _, id)| id)
}

// ---------------------------------------------------------------------------
// F483 高对比主题
// ---------------------------------------------------------------------------

/// WCAG 对比度（近似 8bit 亮度），>= 4.5 通过（以 45/10 表示）。
pub fn contrast_ratio(fg: u8, bg: u8) -> u32 {
    let lum = |c: u8| -> u32 { (c as u32 * 299 + c as u32 * 587 + c as u32 * 114) / 1000 };
    let a = lum(fg);
    let b = lum(bg);
    let (hi, lo) = if a > b { (a, b) } else { (b, a) };
    (hi * 100 + 50) / (lo * 70 + 50).max(1)
}

pub fn high_contrast_ok(fg: u8, bg: u8) -> bool {
    contrast_ratio(fg, bg) >= 45
}

// ---------------------------------------------------------------------------
// F484 单手模式
// ---------------------------------------------------------------------------

/// 单手可达性：把顶部控件下拉到拇指区（返回新 y）。
pub fn one_hand_shift(y: u16, screen_h: u16) -> u16 {
    let thumb = screen_h * 3 / 4;
    if y < thumb {
        thumb
    } else {
        y
    }
}

// ---------------------------------------------------------------------------
// F485 演示者工具
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Presenter {
    pub show_notes: bool,
    pub show_timer: bool,
    pub blank_screen: bool,
}

/// 演示者模式：B 键黑屏、N 键切备注。
pub fn presenter_key(p: Presenter, key: u8) -> Presenter {
    match key {
        b'B' => Presenter { blank_screen: !p.blank_screen, ..p },
        b'N' => Presenter { show_notes: !p.show_notes, ..p },
        _ => p,
    }
}

// ---------------------------------------------------------------------------
// F486 无线投屏预留
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CastState {
    Off,
    Searching,
    Connected(u16),
}

/// 投屏状态机占位。
pub fn cast_step(state: CastState, sink: Option<u16>) -> CastState {
    match (state, sink) {
        (CastState::Off, Some(s)) => CastState::Connected(s),
        (CastState::Connected(_), None) => CastState::Off,
        (s, _) => s,
    }
}

// ---------------------------------------------------------------------------
// F487 发行渠道
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Channel {
    Nightly,
    Beta,
    Stable,
}

/// 渠道晋升：只允许从低到高。
pub fn channel_promote(from: Channel, to: Channel) -> Channel {
    if to > from {
        to
    } else {
        from
    }
}

// ---------------------------------------------------------------------------
// F488 更新器 v2
// ---------------------------------------------------------------------------

/// 差分更新：应用补丁块到旧版本；块序号必须连续。
pub fn delta_apply(old: &mut [u8], patches: &[(u16, u8)]) -> bool {
    let mut expect = 0u16;
    for &(idx, val) in patches {
        if idx != expect {
            return false;
        }
        if (idx as usize) < old.len() {
            old[idx as usize] = val;
        }
        expect += 1;
    }
    true
}

// ---------------------------------------------------------------------------
// F489 全产物验签
// ---------------------------------------------------------------------------

/// 产物验签：全部产物须带有效签名（sig != 0）。
pub fn verify_artifacts(sigs: &[u32]) -> bool {
    sigs.iter().all(|&s| s != 0)
}

// ---------------------------------------------------------------------------
// F490 镜像工厂
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ImageKind {
    Iso,
    Usb,
    Netboot,
}

/// 一键多形态：从母镜像产出 N 种形态清单。
pub fn image_variants(out: &mut [ImageKind]) -> usize {
    let kinds = [ImageKind::Iso, ImageKind::Usb, ImageKind::Netboot];
    let n = out.len().min(kinds.len());
    out[..n].copy_from_slice(&kinds[..n]);
    n
}

// ---------------------------------------------------------------------------
// F491 演示镜像
// ---------------------------------------------------------------------------

/// 演示镜像预算：容量限额 + 预装演示应用数。
pub fn demo_image_ok(size_mb: u32, limit_mb: u32, demo_apps: u8, min_apps: u8) -> bool {
    size_mb <= limit_mb && demo_apps >= min_apps
}

// ---------------------------------------------------------------------------
// F492 截图管线
// ---------------------------------------------------------------------------

/// 截图管线：抓帧→编码→落盘，三步状态。
pub fn screenshot_steps(frame_ok: bool, encode_ok: bool, write_ok: bool) -> bool {
    frame_ok && encode_ok && write_ok
}

// ---------------------------------------------------------------------------
// F493 宣传叙事脚本
// ---------------------------------------------------------------------------

/// 叙事脚本节拍：钩子→演示→特性→号召，缺一不可。
pub fn narrative_complete(beats: [bool; 4]) -> bool {
    beats.iter().all(|&b| b)
}

// ---------------------------------------------------------------------------
// F494 官网素材包
// ---------------------------------------------------------------------------

/// 素材包清单完整性：截图+视频+图标+文案。
pub fn site_pack_complete(has: [bool; 4]) -> bool {
    has.iter().all(|&b| b)
}

// ---------------------------------------------------------------------------
// F495 社区手册
// ---------------------------------------------------------------------------

/// 手册目录：章节覆盖率（已写/应有 >= 90% 才算齐）。
pub fn manual_coverage(written: u8, total: u8) -> bool {
    if total == 0 {
        return false;
    }
    (written as u32) * 100 >= (total as u32) * 90
}

// ---------------------------------------------------------------------------
// F496 版本公告模板
// ---------------------------------------------------------------------------

/// 公告模板填充：三段（亮点/修复/升级说明）齐全。
pub fn release_note_ok(sections: [bool; 3]) -> bool {
    sections.iter().all(|&b| b)
}

// ---------------------------------------------------------------------------
// F497 贡献者荣誉墙
// ---------------------------------------------------------------------------

/// 按提交数排序取前三名 id（稳定：同分取先登记者）。
pub fn contributor_top3(contribs: &[(u16, u32)], out: &mut [u16; 3]) -> usize {
    let n = contribs.len().min(3);
    let mut used = [false; 16];
    for slot in out[..n].iter_mut() {
        let mut best: Option<(usize, u32)> = None;
        for (i, &(_id, commits)) in contribs.iter().enumerate() {
            if used[i] {
                continue;
            }
            match best {
                Some((_, bc)) if commits <= bc => {}
                _ => best = Some((i, commits)),
            }
        }
        if let Some((i, _)) = best {
            used[i] = true;
            *slot = contribs[i].0;
        } else {
            *slot = 0;
        }
    }
    n
}

// ---------------------------------------------------------------------------
// F498 公开路线图
// ---------------------------------------------------------------------------

/// 路线图里程碑：按季度排布且不重排。
pub fn roadmap_milestones(quarters: &[u8]) -> bool {
    for w in quarters.windows(2) {
        if w[0] > w[1] {
            return false;
        }
    }
    !quarters.is_empty()
}

// ---------------------------------------------------------------------------
// F499 十年愿景
// ---------------------------------------------------------------------------

/// 愿景支柱：内核/桌面/生态/社区四柱齐全。
pub fn vision_pillars(pillars: [bool; 4]) -> bool {
    pillars.iter().all(|&b| b)
}

// ---------------------------------------------------------------------------
// F500 交付域自检
// ---------------------------------------------------------------------------

pub fn run_i18n_checks() -> CheckSet {
    let mut set = CheckSet::new("m5-i18n");

    // F476
    let mut t = StringTable::new();
    let a1 = t.add_locale(0x0409); // en-US
    let a2 = t.add_locale(0x0804); // zh-CN
    let dup = t.add_locale(0x0804);
    let p1 = t.put(0x0409, 1, 100);
    let p2 = t.put(0x0804, 1, 200);
    let zh = t.lookup(0x0804, 1);
    let fallback = t.lookup(0x0407, 1); // de 未登记 → 英文兜底
    set.add(
        "F476 i18n framework",
        a1 && a2 && !dup && p1 && p2 && zh == Some(200) && fallback == Some(100),
        "locale + fallback chain",
    );

    // F477
    let t_ok = termbase_consistent(&t, 1);
    let _ = t.put(0x0804, 2, 300);
    let t_bad = termbase_consistent(&t, 2);
    set.add("F477 termbase", t_ok && !t_bad, "missing translation detected");

    // F478
    let (sym_zh, _) = locale_number(Locale::Zh);
    let (sym_de, _) = locale_number(Locale::De);
    let d_zh = locale_date_order(Locale::Zh);
    set.add(
        "F478 locale formats",
        sym_zh == "¥" && sym_de == "€" && d_zh == (0, 1, 2),
        "currency + date order",
    );

    // F479
    let t1 = tz_convert(600, 0, 480); // UTC 10:00 → UTC+8 = 18:00
    let wrap = tz_convert(1380, 480, 0); // 23:00 (+8) → UTC 15:00
    set.add("F479 timezone", t1 == 1080 && wrap == 900, "offset + wraparound");

    // F480
    let l1 = lunar_probe((2026, 2, 17));
    let l0 = lunar_probe((2026, 5, 5));
    set.add("F480 lunar stub", l1 == Some((4724, 1)) && l0.is_none(), "placeholder map");

    // F481
    let mut q = TtsQueue::new();
    q.enqueue(7);
    q.enqueue(9);
    let n1 = q.next();
    let n2 = q.next();
    let n3 = q.next();
    set.add("F481 tts interface", n1 == Some(7) && n2 == Some(9) && n3.is_none(), "speech queue");

    // F482
    let cues = [(0u32, 1000u32, 1u16), (1000, 2000, 2)];
    let c1 = caption_at(&cues, 500);
    let c2 = caption_at(&cues, 1500);
    let c3 = caption_at(&cues, 2500);
    set.add("F482 captions", c1 == Some(1) && c2 == Some(2) && c3.is_none(), "cue timing");

    // F483
    let good = high_contrast_ok(0, 255);
    let bad = high_contrast_ok(128, 128);
    set.add("F483 high contrast", good && !bad, "wcag gate");

    // F484
    let y1 = one_hand_shift(100, 800);
    let y2 = one_hand_shift(700, 800);
    set.add("F484 one-hand mode", y1 == 600 && y2 == 700, "thumb zone shift");

    // F485
    let p0 = Presenter { show_notes: false, show_timer: true, blank_screen: false };
    let p1 = presenter_key(p0, b'B');
    let p2 = presenter_key(p1, b'B');
    let p3 = presenter_key(p0, b'N');
    set.add(
        "F485 presenter tools",
        p1.blank_screen && !p2.blank_screen && p3.show_notes && !p1.show_notes,
        "key toggles",
    );

    // F486
    let c1 = cast_step(CastState::Off, Some(5));
    let c2 = cast_step(c1, None);
    let c3 = cast_step(CastState::Off, None);
    set.add(
        "F486 wireless cast stub",
        c1 == CastState::Connected(5) && c2 == CastState::Off && c3 == CastState::Off,
        "cast state machine",
    );

    // F487
    let up = channel_promote(Channel::Nightly, Channel::Stable);
    let stay = channel_promote(Channel::Stable, Channel::Beta);
    set.add("F487 release channel", up == Channel::Stable && stay == Channel::Stable, "promote only forward");

    // F488
    let mut old = [0u8; 4];
    let ok = delta_apply(&mut old, &[(0, 1), (1, 2)]);
    let mut old2 = [0u8; 4];
    let bad = delta_apply(&mut old2, &[(1, 1)]);
    set.add("F488 delta updater", ok && old[0] == 1 && old[1] == 2 && !bad, "ordered patches");

    // F489
    set.add("F489 artifact verify", verify_artifacts(&[1, 2, 3]) && !verify_artifacts(&[1, 0, 3]), "all signed");

    // F490
    let mut kinds = [ImageKind::Iso; 3];
    let n = image_variants(&mut kinds);
    set.add(
        "F490 image factory",
        n == 3 && kinds[1] == ImageKind::Usb && kinds[2] == ImageKind::Netboot,
        "multi-form output",
    );

    // F491
    set.add(
        "F491 demo image",
        demo_image_ok(400, 512, 6, 5) && !demo_image_ok(600, 512, 6, 5) && !demo_image_ok(400, 512, 3, 5),
        "budget + app floor",
    );

    // F492
    set.add(
        "F492 screenshot pipeline",
        screenshot_steps(true, true, true) && !screenshot_steps(true, false, true),
        "three-step chain",
    );

    // F493
    set.add(
        "F493 narrative script",
        narrative_complete([true, true, true, true]) && !narrative_complete([true, false, true, true]),
        "all beats",
    );

    // F494
    set.add(
        "F494 site pack",
        site_pack_complete([true, true, true, true]) && !site_pack_complete([true, true, true, false]),
        "asset completeness",
    );

    // F495
    set.add(
        "F495 community manual",
        manual_coverage(9, 10) && !manual_coverage(8, 10) && !manual_coverage(0, 0),
        "90% coverage",
    );

    // F496
    set.add(
        "F496 release note",
        release_note_ok([true, true, true]) && !release_note_ok([true, false, true]),
        "three sections",
    );

    // F497
    let contribs = [(1u16, 30u32), (2, 99), (3, 30), (4, 50)];
    let mut top = [0u16; 3];
    let n = contributor_top3(&contribs, &mut top);
    set.add("F497 contributor wall", n == 3 && top[0] == 2 && top[1] == 4 && top[2] == 1, "top3 stable");

    // F498
    set.add(
        "F498 roadmap",
        roadmap_milestones(&[1, 2, 2, 3]) && !roadmap_milestones(&[3, 1]) && !roadmap_milestones(&[]),
        "ordered milestones",
    );

    // F499
    set.add(
        "F499 ten-year vision",
        vision_pillars([true, true, true, true]) && !vision_pillars([true, true, false, true]),
        "four pillars",
    );

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f476_fallback_chain() {
        let mut t = StringTable::new();
        t.add_locale(0x0804);
        t.put(0x0804, 5, 55);
        assert_eq!(t.lookup(0x0409, 5), Some(55)); // 兜底到唯一语言
        assert_eq!(t.lookup(0x0804, 6), None);
    }

    #[test]
    fn f481_queue_full() {
        let mut q = TtsQueue::new();
        for i in 0..4 {
            assert!(q.enqueue(i));
        }
        assert!(!q.enqueue(9));
    }

    #[test]
    fn f500_i18n_self_test_passes() {
        let set = run_i18n_checks();
        if !set.all_passed() {
            let mut buf = [0u8; 1024];
            let n = set.render(&mut buf);
            panic!("m5-i18n self-test:\n{}", core::str::from_utf8(&buf[..n]).unwrap());
        }
        assert!(set.len() >= 24);
    }
}
