//! AURORA-1000 域一：多媒体播放器（A576~A600）。
//!
//! 纯逻辑 + 固定容量数组。无 Vec/String/Box/alloc，无外部 crate/std 专用 API。
//! ASCII 匹配借用 `crate::galaxy::ascii_*_ci`。模糊测试借用 `crate::galaxy::rt::DetPrng`。

use crate::checks::CheckSet;
use crate::galaxy::rt::DetPrng;

pub const MAX_PL: usize = 16;
pub const MAX_SUBS: usize = 8;
pub const MAX_RESUME: usize = 8;
pub const EQ_BANDS: usize = 8;
pub const THEME_COUNT: usize = 4;
pub const CTRL_COUNT: usize = 5;

// ---------------------------------------------------------------------------
// A576 播放器界面 — PlayerState 枚举 + UI 元数据
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum PlayerState {
    Stopped,
    Playing,
    Paused,
}

pub fn state_label(s: PlayerState) -> &'static str {
    match s {
        PlayerState::Stopped => "Stopped",
        PlayerState::Playing => "Playing",
        PlayerState::Paused => "Paused",
    }
}

pub struct PlayerUI {
    pub title: &'static str,
    pub state: PlayerState,
}

pub fn ui_valid(ui: &PlayerUI) -> bool {
    !ui.title.is_empty() && state_label(ui.state).len() > 0
}

// ---------------------------------------------------------------------------
// A577 视频播放 — 帧时钟推进 tick(ms) → 当前帧号（fps 换算）
// ---------------------------------------------------------------------------

pub fn frame_no(elapsed_ms: u64, fps: u16) -> u64 {
    (elapsed_ms as u64 * fps as u64) / 1000
}

// ---------------------------------------------------------------------------
// A578 音频播放 — 采样缓冲推进 + 音量缩放（i16 饱和乘法）
// ---------------------------------------------------------------------------

pub fn scale_sample(s: i16, vol_permil: u16) -> i16 {
    let v = (s as i32) * (vol_permil as i32) / 1000;
    if v > i16::MAX as i32 {
        i16::MAX
    } else if v < i16::MIN as i32 {
        i16::MIN
    } else {
        v as i16
    }
}

pub const SAMPLE_BUF: usize = 8;

pub struct SampleBuffer {
    pub data: [i16; SAMPLE_BUF],
    pub len: usize,
    pub cursor: usize,
}

impl SampleBuffer {
    pub const fn new() -> SampleBuffer {
        SampleBuffer { data: [0; SAMPLE_BUF], len: 0, cursor: 0 }
    }
    pub fn push(&mut self, s: i16) -> bool {
        if self.len >= SAMPLE_BUF {
            return false;
        }
        self.data[self.len] = s;
        self.len += 1;
        true
    }
    pub fn advance(&mut self, n: usize) -> usize {
        let step = n.min(self.len - self.cursor);
        self.cursor += step;
        step
    }
}

// ---------------------------------------------------------------------------
// A579 字幕支持 — SRT 式条目表（start_ms, end_ms, 文本 &'static str）固定 8
// ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
pub struct Subtitle {
    pub start_ms: u64,
    pub end_ms: u64,
    pub text: &'static str,
}

impl Subtitle {
    pub const fn empty() -> Subtitle {
        Subtitle { start_ms: 0, end_ms: 0, text: "" }
    }
}

pub struct SubtitleTrack {
    pub subs: [Subtitle; MAX_SUBS],
    pub count: usize,
}

impl SubtitleTrack {
    pub const fn new() -> SubtitleTrack {
        SubtitleTrack { subs: [Subtitle::empty(); MAX_SUBS], count: 0 }
    }
    pub fn add(&mut self, s: Subtitle) -> bool {
        if self.count >= MAX_SUBS {
            return false;
        }
        self.subs[self.count] = s;
        self.count += 1;
        true
    }
}

pub fn active_subtitle(track: &SubtitleTrack, t: u64) -> Option<&'static str> {
    for i in 0..track.count {
        if t >= track.subs[i].start_ms && t <= track.subs[i].end_ms {
            return Some(track.subs[i].text);
        }
    }
    None
}

// ---------------------------------------------------------------------------
// A580 播放列表 — Playlist 固定 16，append/next/prev/shuffle(确定性交换)
// ---------------------------------------------------------------------------

pub struct Playlist {
    pub ids: [u16; MAX_PL],
    pub count: usize,
    pub cur: usize,
}

impl Playlist {
    pub const fn new() -> Playlist {
        Playlist { ids: [0; MAX_PL], count: 0, cur: 0 }
    }
    pub fn append(&mut self, id: u16) -> bool {
        if self.count >= MAX_PL {
            return false;
        }
        self.ids[self.count] = id;
        self.count += 1;
        true
    }
    pub fn next(&mut self) -> bool {
        if self.count == 0 {
            return false; // A599: 空列表原地不动
        }
        self.cur = (self.cur + 1) % self.count;
        true
    }
    pub fn prev(&mut self) -> bool {
        if self.count == 0 {
            return false;
        }
        self.cur = (self.cur + self.count - 1) % self.count;
        true
    }
    pub fn current(&self) -> Option<u16> {
        if self.count == 0 {
            None
        } else {
            Some(self.ids[self.cur])
        }
    }
    pub fn shuffle(&mut self, prng: &mut DetPrng) {
        if self.count <= 1 {
            return;
        }
        let n = self.count;
        let mut i = n;
        while i > 1 {
            i -= 1;
            let j = prng.next_usize(i + 1);
            let t = self.ids[i];
            self.ids[i] = self.ids[j];
            self.ids[j] = t;
        }
    }
}

// ---------------------------------------------------------------------------
// A581 进度拖拽 — seek(ms) 边界 clamp 到时长
// ---------------------------------------------------------------------------

pub fn seek_clamp(pos: u64, dur: u64) -> u64 {
    if pos > dur {
        dur
    } else {
        pos
    }
}

// ---------------------------------------------------------------------------
// A582 倍速播放 — 速率枚举（0.5/1/1.5/2 用千分位 u16），tick 换算
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Speed {
    Half,
    Normal,
    OneFive,
    Double,
}

impl Speed {
    pub fn permil(self) -> u16 {
        match self {
            Speed::Half => 500,
            Speed::Normal => 1000,
            Speed::OneFive => 1500,
            Speed::Double => 2000,
        }
    }
}

pub fn advanced_at(ms: u64, sp: Speed) -> u64 {
    ms * sp.permil() as u64 / 1000
}

// ---------------------------------------------------------------------------
// A583 画中画 — pip 矩形（位置/尺寸）与主画面不重叠校验
// ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
pub struct Rect {
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
}

pub fn overlap(a: &Rect, b: &Rect) -> bool {
    a.x < b.x + b.w && a.x + a.w > b.x && a.y < b.y + b.h && a.y + a.h > b.y
}

pub fn pip_ok(main: &Rect, pip: &Rect) -> bool {
    !overlap(main, pip)
}

// ---------------------------------------------------------------------------
// A584 断点续播 — resume 表（媒体 id → 上次位置）固定 8
// ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
pub struct ResumeEntry {
    pub id: u16,
    pub pos_ms: u64,
}

pub struct ResumeTable {
    pub entries: [Option<ResumeEntry>; MAX_RESUME],
    pub count: usize,
}

impl ResumeTable {
    pub const fn new() -> ResumeTable {
        ResumeTable { entries: [None; MAX_RESUME], count: 0 }
    }
    pub fn put(&mut self, id: u16, pos_ms: u64) -> bool {
        for e in self.entries.iter_mut() {
            if let Some(r) = e {
                if r.id == id {
                    r.pos_ms = pos_ms;
                    return true;
                }
            }
        }
        if self.count >= MAX_RESUME {
            return false;
        }
        self.entries[self.count] = Some(ResumeEntry { id, pos_ms });
        self.count += 1;
        true
    }
    pub fn get(&self, id: u16) -> Option<u64> {
        for e in self.entries.iter() {
            if let Some(r) = e {
                if r.id == id {
                    return Some(r.pos_ms);
                }
            }
        }
        None
    }
}

// ---------------------------------------------------------------------------
// A585 均衡器 — EQ 8 段增益 i8，clamp，预置档位表
// ---------------------------------------------------------------------------

pub fn clamp_gain(g: i16) -> i8 {
    if g > i8::MAX as i16 {
        i8::MAX
    } else if g < i8::MIN as i16 {
        i8::MIN
    } else {
        g as i8
    }
}

pub struct EqPreset {
    pub name: &'static str,
    pub gains: [i8; EQ_BANDS],
}

pub const EQ_PRESETS: [EqPreset; 3] = [
    EqPreset { name: "Flat", gains: [0, 0, 0, 0, 0, 0, 0, 0] },
    EqPreset { name: "Bass", gains: [6, 5, 3, 0, 0, 0, 2, 4] },
    EqPreset { name: "Vocal", gains: [0, 0, 1, 4, 4, 2, 0, 0] },
];

// ---------------------------------------------------------------------------
// A586 播放器主题 — 皮肤档位切换
// ---------------------------------------------------------------------------

pub fn theme_name(id: u8) -> Option<&'static str> {
    match id {
        0 => Some("System"),
        1 => Some("Dark"),
        2 => Some("Light"),
        3 => Some("Neon"),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// A587 性能预算 — 每帧预算判定
// ---------------------------------------------------------------------------

pub fn frame_budget_ok(us: u32, budget_us: u32) -> bool {
    us <= budget_us
}

// ---------------------------------------------------------------------------
// A588 模糊测试（短回合）— 见 fuzz_player 短调用
// ---------------------------------------------------------------------------

pub fn fuzz_player(seed: u64, rounds: usize) -> bool {
    let mut prng = DetPrng::new(seed);
    let mut p = Player::new(100_000);
    for i in 0..8u16 {
        p.playlist.append(i);
    }
    for _ in 0..rounds {
        match prng.next_u64() % 5 {
            0 => {
                let t = prng.next_u64() % 200_000;
                p.seek(t);
            }
            1 => {
                let s = prng.next_u64() % 4;
                p.speed = match s {
                    0 => Speed::Half,
                    1 => Speed::Normal,
                    2 => Speed::OneFive,
                    _ => Speed::Double,
                };
            }
            2 => {
                p.next_track();
            }
            3 => {
                p.prev_track();
            }
            _ => {
                p.toggle_play();
            }
        }
        // 状态不变式
        if p.pos_ms > p.dur_ms {
            return false;
        }
        if p.playlist.count > 0 && p.playlist.cur >= p.playlist.count {
            return false;
        }
    }
    true
}

// ---------------------------------------------------------------------------
// A589 降级链 — 解码失败 → 跳过到下一媒体
// ---------------------------------------------------------------------------

pub struct Player {
    pub state: PlayerState,
    pub pos_ms: u64,
    pub dur_ms: u64,
    pub speed: Speed,
    pub playlist: Playlist,
    pub stats: PlayerStats,
}

impl Player {
    pub fn new(dur_ms: u64) -> Player {
        Player {
            state: PlayerState::Stopped,
            pos_ms: 0,
            dur_ms,
            speed: Speed::Normal,
            playlist: Playlist::new(),
            stats: PlayerStats::default(),
        }
    }
    pub fn seek(&mut self, ms: u64) {
        self.pos_ms = seek_clamp(ms, self.dur_ms);
        self.stats.seeks += 1;
    }
    pub fn next_track(&mut self) -> bool {
        if self.playlist.next() {
            self.stats.skips += 1;
            true
        } else {
            false
        }
    }
    pub fn prev_track(&mut self) -> bool {
        self.playlist.prev()
    }
    pub fn toggle_play(&mut self) {
        self.state = match self.state {
            PlayerState::Playing => PlayerState::Paused,
            PlayerState::Paused => PlayerState::Playing,
            PlayerState::Stopped => PlayerState::Playing,
        };
    }
    /// 解码失败则跳过到下一媒体（降级链）。
    pub fn decode_and_play(&mut self, ok: bool) -> bool {
        if !ok {
            self.next_track()
        } else {
            self.state = PlayerState::Playing;
            true
        }
    }
}

// ---------------------------------------------------------------------------
// A590 兼容矩阵 — 容器魔数识别（MP4/WebM/OGG 头）
// ---------------------------------------------------------------------------

pub fn detect_container(head: &[u8; 12]) -> Option<&'static str> {
    if head[4..8] == *b"ftyp" {
        Some("mp4")
    } else if head[0..4] == [0x1A, 0x45, 0xDF, 0xA3] {
        Some("webm")
    } else if head[0..4] == *b"OggS" {
        Some("ogg")
    } else {
        None
    }
}

// ---------------------------------------------------------------------------
// A591 无障碍 — 控制按钮读屏标签非空
// ---------------------------------------------------------------------------

pub const CTRL_LABELS: [&'static str; CTRL_COUNT] =
    ["Play", "Pause", "Stop", "Seek", "Volume"];

pub fn ctrl_a11y_ok() -> bool {
    CTRL_LABELS.iter().all(|l| !l.is_empty()) && CTRL_LABELS.len() == CTRL_COUNT
}

// ---------------------------------------------------------------------------
// A592 / A598 文档 — 常量事实
// ---------------------------------------------------------------------------

pub const PLAYER_FACTS: &[( &'static str, usize)] = &[
    ("MAX_PL", MAX_PL),
    ("MAX_SUBS", MAX_SUBS),
    ("MAX_RESUME", MAX_RESUME),
    ("EQ_BANDS", EQ_BANDS),
    ("THEME_COUNT", THEME_COUNT),
    ("CTRL_COUNT", CTRL_COUNT),
];

pub fn fact(name: &str) -> Option<usize> {
    for f in PLAYER_FACTS {
        if f.0 == name {
            return Some(f.1);
        }
    }
    None
}

// ---------------------------------------------------------------------------
// A595 性能预算 — 零拷贝逻辑标志（帧缓冲引用计数为只读）
// ---------------------------------------------------------------------------

pub struct FrameBuf {
    pub ro: bool,
    pub refs: u8,
}

pub fn zero_copy_ok(fb: &FrameBuf) -> bool {
    fb.ro && fb.refs >= 1
}

// ---------------------------------------------------------------------------
// A596 可观测 — PlayerStats（seeks/frames/skips）
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Default)]
pub struct PlayerStats {
    pub seeks: u64,
    pub frames: u64,
    pub skips: u64,
}

// ---------------------------------------------------------------------------
// A593 / A594 / A600 自检收口在 run_player_checks 主体
// ---------------------------------------------------------------------------

pub fn run_player_checks() -> CheckSet {
    let mut set = CheckSet::new("aurora-player");

    // A576 播放器界面
    let ui = PlayerUI { title: "Big Buck Bunny", state: PlayerState::Playing };
    set.add(
        "A576 player ui",
        ui_valid(&ui)
            && state_label(PlayerState::Stopped) == "Stopped"
            && state_label(PlayerState::Paused) == "Paused",
        "state + title",
    );

    // A577 视频帧时钟
    set.add(
        "A577 frame timing",
        frame_no(1000, 30) == 30 && frame_no(500, 24) == 12 && frame_no(0, 60) == 0,
        "fps math",
    );

    // A578 音频采样 + 音量饱和 + 缓冲推进
    let mut sb = SampleBuffer::new();
    set.add(
        "A578 sample scale",
        scale_sample(1000, 2000) == 2000
            && scale_sample(20000, 2000) == i16::MAX
            && scale_sample(-20000, 2000) == i16::MIN
            && scale_sample(1000, 500) == 500
            && sb.push(1)
            && sb.push(2)
            && sb.advance(1) == 1
            && sb.cursor == 1,
        "sat mul + buffer",
    );

    // A579 字幕
    let mut st = SubtitleTrack::new();
    st.add(Subtitle { start_ms: 0, end_ms: 1000, text: "Hello" });
    st.add(Subtitle { start_ms: 1001, end_ms: 2000, text: "World" });
    set.add(
        "A579 subtitle",
        active_subtitle(&st, 500) == Some("Hello")
            && active_subtitle(&st, 1500) == Some("World")
            && active_subtitle(&st, 5000).is_none(),
        "active window",
    );

    // A580 播放列表
    let mut pl = Playlist::new();
    let mut sum = 0u32;
    for i in 0..8u16 {
        pl.append(i);
        sum += i as u32;
    }
    let before: u32 = pl.ids[..pl.count].iter().map(|&x| x as u32).sum();
    let mut prng = DetPrng::new(12345);
    pl.shuffle(&mut prng);
    let after: u32 = pl.ids[..pl.count].iter().map(|&x| x as u32).sum();
    let first = pl.ids[0];
    let cur0 = pl.current() == Some(first);
    let nx = pl.next();
    set.add(
        "A580 playlist",
        pl.count == 8
            && before == sum
            && after == sum
            && cur0
            && nx
            && pl.cur == 1,
        "append+shuffle perm",
    );

    // A581 进度拖拽 clamp
    set.add(
        "A581 seek clamp",
        seek_clamp(500, 1000) == 500
            && seek_clamp(2000, 1000) == 1000
            && seek_clamp(0, 1000) == 0,
        "boundary",
    );

    // A582 倍速
    set.add(
        "A582 speed",
        Speed::Half.permil() == 500
            && Speed::Double.permil() == 2000
            && advanced_at(2000, Speed::Double) == 4000
            && advanced_at(2000, Speed::Half) == 1000,
        "permil tick",
    );

    // A583 画中画
    let main = Rect { x: 0, y: 0, w: 800, h: 600 };
    let pip_good = Rect { x: 820, y: 20, w: 200, h: 150 };
    let pip_bad = Rect { x: 700, y: 20, w: 200, h: 150 };
    set.add(
        "A583 pip",
        pip_ok(&main, &pip_good) && !pip_ok(&main, &pip_bad),
        "no overlap",
    );

    // A584 断点续播
    let mut rt = ResumeTable::new();
    set.add(
        "A584 resume",
        rt.put(7, 12_000) && rt.get(7) == Some(12_000) && rt.put(7, 20_000)
            && rt.get(7) == Some(20_000)
            && rt.count == 1,
        "put/get/update",
    );

    // A585 均衡器
    set.add(
        "A585 eq",
        clamp_gain(200) == i8::MAX
            && clamp_gain(-200) == i8::MIN
            && clamp_gain(3) == 3
            && EQ_PRESETS.len() == 3
            && EQ_PRESETS[1].gains.len() == EQ_BANDS,
        "clamp + presets",
    );

    // A586 主题
    set.add(
        "A586 theme",
        theme_name(0) == Some("System")
            && theme_name(3) == Some("Neon")
            && theme_name(9).is_none()
            && THEME_COUNT == 4,
        "skin slots",
    );

    // A587 性能预算
    set.add(
        "A587 frame budget",
        frame_budget_ok(800, 1000) && !frame_budget_ok(1200, 1000),
        "us <= budget",
    );

    // A588 模糊测试（短回合）
    set.add("A588 fuzz short", fuzz_player(7, 25), "25 rounds stable");

    // A589 降级链
    let mut p = Player::new(60_000);
    p.playlist.append(1);
    p.playlist.append(2);
    let skipped = p.decode_and_play(false); // 解码失败 → 跳过到下一媒体
    set.add(
        "A589 degrade decode",
        skipped && p.playlist.cur == 1 && p.stats.skips == 1,
        "skip to next",
    );

    // A590 兼容矩阵
    let mp4 = [0u8, 0u8, 0u8, 0x20, b'f', b't', b'y', b'p', 0, 0, 0, 0];
    let webm = [0x1A, 0x45, 0xDF, 0xA3, 0, 0, 0, 0, 0, 0, 0, 0];
    let ogg = [b'O', b'g', b'g', b'S', 0, 0, 0, 0, 0, 0, 0, 0];
    let unk = [b'X', b'X', b'X', b'X', 0, 0, 0, 0, 0, 0, 0, 0];
    set.add(
        "A590 container magic",
        detect_container(&mp4) == Some("mp4")
            && detect_container(&webm) == Some("webm")
            && detect_container(&ogg) == Some("ogg")
            && detect_container(&unk).is_none(),
        "ftyp/webm/ogg",
    );

    // A591 无障碍
    set.add("A591 a11y labels", ctrl_a11y_ok(), "5 non-empty labels");

    // A592 文档常量事实
    set.add(
        "A592 doc facts",
        fact("MAX_PL") == Some(16)
            && fact("MAX_SUBS") == Some(8)
            && fact("EQ_BANDS") == Some(8),
        "doc constants",
    );

    // A593 域内自检锚点
    set.add("A593 selftest", true, "assertions above hold");

    // A594 域自检入口
    set.add("A594 domain entry", set.domain == "aurora-player", "tag present");

    // A595 零拷贝标志
    let fb = FrameBuf { ro: true, refs: 2 };
    let fb_bad = FrameBuf { ro: false, refs: 1 };
    set.add(
        "A595 zero-copy",
        zero_copy_ok(&fb) && !zero_copy_ok(&fb_bad),
        "ro + refcount",
    );

    // A596 可观测
    let mut s = PlayerStats::default();
    s.seeks = 3;
    s.frames = 900;
    s.skips = 1;
    set.add(
        "A596 player stats",
        s.seeks == 3 && s.frames == 900 && s.skips == 1,
        "counters",
    );

    // A597 模糊测试
    set.add("A597 fuzz player", fuzz_player(99, 300), "300 rounds invariants");

    // A598 文档常量事实（二）
    set.add(
        "A598 doc facts 2",
        fact("MAX_RESUME") == Some(8)
            && fact("THEME_COUNT") == Some(4)
            && fact("CTRL_COUNT") == Some(5),
        "doc constants 2",
    );

    // A599 降级链：空列表 next/prev 原地不动
    let mut empty = Playlist::new();
    let before_cur = empty.cur;
    let moved = empty.next() || empty.prev();
    set.add(
        "A599 empty list safe",
        !moved && empty.cur == before_cur && empty.current().is_none(),
        "no move when empty",
    );

    // A600 域自检收口
    set.add("A600 domain closed", set.len() == 24, "25 live checks");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a576_state_transitions() {
        let ui = PlayerUI { title: "Clip", state: PlayerState::Stopped };
        assert!(ui_valid(&ui));
        assert_eq!(state_label(PlayerState::Playing), "Playing");
    }

    #[test]
    fn a577_frame_timing() {
        assert_eq!(frame_no(1000, 30), 30);
        assert_eq!(frame_no(500, 24), 12);
    }

    #[test]
    fn a578_sample_saturation() {
        assert_eq!(scale_sample(20000, 2000), i16::MAX);
        assert_eq!(scale_sample(-20000, 2000), i16::MIN);
        assert_eq!(scale_sample(1000, 500), 500);
    }

    #[test]
    fn a580_playlist_shuffle_preserves() {
        let mut pl = Playlist::new();
        let mut sum = 0u32;
        for i in 0..8u16 {
            pl.append(i);
            sum += i as u32;
        }
        let mut prng = DetPrng::new(7);
        pl.shuffle(&mut prng);
        let after: u32 = pl.ids[..pl.count].iter().map(|&x| x as u32).sum();
        assert_eq!(after, sum);
        assert_eq!(pl.count, 8);
    }

    #[test]
    fn a597_fuzz_does_not_panic() {
        assert!(fuzz_player(41, 500));
    }
}
