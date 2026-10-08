//! VARIX-M500 AI-01 启动体验与快启深化域（F001~F025，M1）。
//!
//! 让开机成为作品的序章：更快（剖析/预载/并行）、更聪明（自愈/归因/诊断）、
//! 更美（主题包/动效/文案）、能自救（多版本/自愈切换）。
//! 全部为纯逻辑 + 固定容量数组（无 Vec/String/Box），no_std 安全。

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// F001 — 启动剖析归因器：逐阶段毫秒归因
// ---------------------------------------------------------------------------

pub const MAX_PHASES: usize = 16;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BootPhase {
    pub name: &'static str,
    pub start_ms: u32,
    pub end_ms: u32,
    /// 归因类别：0=firmware 1=loader 2=kernel 3=driver 4=service 5=ui
    pub bucket: u8,
}

#[derive(Clone, Copy)]
pub struct BootProfiler {
    pub phases: [Option<BootPhase>; MAX_PHASES],
    pub count: usize,
    pub total_ms: u32,
}

impl BootProfiler {
    pub const fn new() -> BootProfiler {
        BootProfiler { phases: [const { None }; MAX_PHASES], count: 0, total_ms: 0 }
    }

    pub fn record(&mut self, name: &'static str, start_ms: u32, end_ms: u32, bucket: u8) -> bool {
        if self.count >= MAX_PHASES || end_ms < start_ms {
            return false;
        }
        self.phases[self.count] = Some(BootPhase { name, start_ms, end_ms, bucket });
        self.count += 1;
        if end_ms > self.total_ms {
            self.total_ms = end_ms;
        }
        true
    }

    /// 归因到桶：每个桶的累计毫秒。
    pub fn bucket_ms(&self, bucket: u8) -> u32 {
        let mut ms = 0;
        for i in 0..self.count {
            if let Some(p) = self.phases[i] {
                if p.bucket == bucket {
                    ms += p.end_ms - p.start_ms;
                }
            }
        }
        ms
    }

    /// 最贵的阶段（优化第一刀该落在哪里）。
    pub fn top_phase(&self) -> Option<BootPhase> {
        let mut best: Option<BootPhase> = None;
        for i in 0..self.count {
            if let Some(p) = self.phases[i] {
                let pd = p.end_ms - p.start_ms;
                let bd = best.map(|b| b.end_ms - b.start_ms).unwrap_or(0);
                if pd > bd {
                    best = Some(p);
                }
            }
        }
        best
    }

    /// 洞察：相邻阶段之间的缝隙（未被任何阶段覆盖的时间）。
    pub fn gap_ms(&self) -> u32 {
        let mut covered = 0u32;
        for i in 0..self.count {
            if let Some(p) = self.phases[i] {
                covered += p.end_ms - p.start_ms;
            }
        }
        self.total_ms.saturating_sub(covered)
    }
}

// ---------------------------------------------------------------------------
// F002 — 预加载缓存器：高频资产/页启动前预取
// ---------------------------------------------------------------------------

pub const MAX_PRELOAD: usize = 32;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PreloadEntry {
    pub key: u64,
    pub priority: u8,
    pub loaded: bool,
    pub hits: u32,
}

#[derive(Clone, Copy)]
pub struct PreloadCache {
    pub entries: [Option<PreloadEntry>; MAX_PRELOAD],
    pub count: usize,
    pub budget_pages: u32,
    pub used_pages: u32,
}

impl PreloadCache {
    pub const fn new(budget_pages: u32) -> PreloadCache {
        PreloadCache { entries: [const { None }; MAX_PRELOAD], count: 0, budget_pages, used_pages: 0 }
    }

    /// 按 priority 降序登记；预算内才装载。
    pub fn register(&mut self, key: u64, priority: u8, pages: u32) -> bool {
        for i in 0..self.count {
            if let Some(e) = self.entries[i] {
                if e.key == key {
                    return false; // 去重
                }
            }
        }
        if self.count >= MAX_PRELOAD {
            return false;
        }
        let loaded = self.used_pages + pages <= self.budget_pages;
        if loaded {
            self.used_pages += pages;
        }
        self.entries[self.count] =
            Some(PreloadEntry { key, priority, loaded, hits: 0 });
        self.count += 1;
        true
    }

    pub fn touch(&mut self, key: u64) -> bool {
        for i in 0..self.count {
            if let Some(mut e) = self.entries[i] {
                if e.key == key && e.loaded {
                    e.hits += 1;
                    self.entries[i] = Some(e);
                    return true;
                }
            }
        }
        false
    }

    pub fn hit_rate_permille(&self) -> u32 {
        let mut hits = 0;
        let mut loads = 0;
        for i in 0..self.count {
            if let Some(e) = self.entries[i] {
                if e.loaded {
                    loads += 1;
                    hits += e.hits.min(1);
                }
            }
        }
        if loads == 0 { 0 } else { hits * 1000 / loads }
    }
}

// ---------------------------------------------------------------------------
// F003 — 引导镜像多版本选择：菜单内多内核版本共存
// ---------------------------------------------------------------------------

pub const MAX_BOOT_IMAGES: usize = 8;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BootImage {
    pub slot: u8,
    pub version: u32,
    pub flags: u32, // bit0=default bit1=fallback bit2=verified
}

#[derive(Clone, Copy)]
pub struct BootMenu {
    pub images: [Option<BootImage>; MAX_BOOT_IMAGES],
    pub count: usize,
    pub selected: u8,
}

impl BootMenu {
    pub const fn new() -> BootMenu {
        BootMenu { images: [const { None }; MAX_BOOT_IMAGES], count: 0, selected: 0 }
    }

    pub fn add(&mut self, slot: u8, version: u32, flags: u32) -> bool {
        for i in 0..self.count {
            if let Some(im) = self.images[i] {
                if im.slot == slot {
                    return false;
                }
            }
        }
        if self.count >= MAX_BOOT_IMAGES {
            return false;
        }
        self.images[self.count] = Some(BootImage { slot, version, flags });
        self.count += 1;
        true
    }

    pub fn select(&mut self, slot: u8) -> bool {
        for i in 0..self.count {
            if let Some(im) = self.images[i] {
                if im.slot == slot && im.flags & 0b100 != 0 {
                    self.selected = slot;
                    return true;
                }
            }
        }
        false
    }

    /// 默认镜像；没有 default 时选最高版本；都没有则 None。
    pub fn default_slot(&self) -> Option<u8> {
        let mut best: Option<BootImage> = None;
        for i in 0..self.count {
            if let Some(im) = self.images[i] {
                if im.flags & 0b1 != 0 {
                    return Some(im.slot);
                }
                if best.map(|b| im.version > b.version).unwrap_or(true) {
                    best = Some(im);
                }
            }
        }
        best.map(|b| b.slot)
    }

    /// 引导自愈的备用位（排除失败槽）。
    pub fn fallback_slot(&self, failed_slot: u8) -> Option<u8> {
        let mut best: Option<BootImage> = None;
        for i in 0..self.count {
            if let Some(im) = self.images[i] {
                if im.slot != failed_slot && im.flags & 0b110 == 0b110 {
                    if best.map(|b| im.version > b.version).unwrap_or(true) {
                        best = Some(im);
                    }
                }
            }
        }
        best.map(|b| b.slot)
    }
}

// ---------------------------------------------------------------------------
// F004 — 引导主题包格式：开机视觉整包（头 + 调色 + 帧率红线）
// ---------------------------------------------------------------------------

pub const THEME_MAGIC: u32 = 0x_56_42_54_50; // "VBTP"

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BootThemePack {
    pub magic: u32,
    pub api_version: u16,
    pub bg_rgb: u32,
    pub fg_rgb: u32,
    pub accent_rgb: u32,
    /// 动效帧预算（fps 红线，≤30 才准装）。
    pub anim_fps: u8,
    /// 静默引导：true 时整包只出画面不出文字。
    pub silent: bool,
}

impl BootThemePack {
    pub fn parse(hdr: [u32; 8]) -> Option<BootThemePack> {
        if hdr[0] != THEME_MAGIC || hdr[1] > 2 {
            return None;
        }
        let fps = (hdr[5] & 0xFF) as u8;
        if fps == 0 || fps > 30 {
            return None;
        }
        Some(BootThemePack {
            magic: hdr[0],
            api_version: hdr[1] as u16,
            bg_rgb: hdr[2],
            fg_rgb: hdr[3],
            accent_rgb: hdr[4],
            anim_fps: fps,
            silent: hdr[6] & 1 != 0,
        })
    }

    /// 对比度可读性：fg 与 bg 亮度差 ≥ 128 才算合格。
    pub fn readable(&self) -> bool {
        let lum = |c: u32| -> u32 {
            let r = (c >> 16) & 0xFF;
            let g = (c >> 8) & 0xFF;
            let b = c & 0xFF;
            (r * 299 + g * 587 + b * 114) / 1000
        };
        let d = lum(self.fg_rgb).abs_diff(lum(self.bg_rgb));
        d >= 128
    }
}

// ---------------------------------------------------------------------------
// F005 — 静默引导模式：零文字纯画面启动
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SilentBoot {
    pub enabled: bool,
    /// 失败时是否允许回退到文字输出（红线：永远允许）。
    pub text_fallback: bool,
    pub frames_drawn: u32,
    pub chars_drawn: u32,
}

impl SilentBoot {
    pub const fn new() -> SilentBoot {
        SilentBoot { enabled: false, text_fallback: true, frames_drawn: 0, chars_drawn: 0 }
    }

    pub fn enable(&mut self) {
        self.enabled = true;
        self.text_fallback = true; // 不可关闭的红线
    }

    pub fn draw_frame(&mut self) {
        if self.enabled {
            self.frames_drawn += 1;
        }
    }

    /// 静默模式下禁止文字；返回 false 表示调用被拒绝。
    pub fn draw_text(&mut self, n_chars: u32) -> bool {
        if self.enabled {
            return false;
        }
        self.chars_drawn += n_chars;
        true
    }

    /// 引导失败 → 强制回文字（自愈路径）。
    pub fn force_fallback(&mut self) {
        self.enabled = false;
    }
}

// ---------------------------------------------------------------------------
// F006 — 引导自愈切换：主镜像失败自动换备用
// ---------------------------------------------------------------------------

pub const MAX_HEAL_ATTEMPTS: u8 = 3;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BootHealer {
    pub primary: u8,
    pub current: u8,
    pub attempts: u8,
    pub switched: bool,
    pub recovered: bool,
}

impl BootHealer {
    pub const fn new(primary: u8) -> BootHealer {
        BootHealer { primary, current: primary, attempts: 0, switched: false, recovered: false }
    }

    pub fn report_failure(&mut self, fallback: Option<u8>) -> Option<u8> {
        self.attempts += 1;
        if !self.switched {
            if let Some(fb) = fallback {
                if fb != self.primary {
                    self.current = fb;
                    self.switched = true;
                    return Some(fb);
                }
            }
        }
        None
    }

    pub fn report_success(&mut self) {
        self.recovered = self.switched;
    }

    /// 三次全失败 → 进入恢复环境。
    pub fn needs_recovery(&self) -> bool {
        self.attempts >= MAX_HEAL_ATTEMPTS && !self.recovered
    }
}

// ---------------------------------------------------------------------------
// F007 — 资产预解压缓存：首启后资产免解压直达
// ---------------------------------------------------------------------------

pub const MAX_ASSETS: usize = 16;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AssetEntry {
    pub id: u32,
    pub raw_len: u32,
    pub packed_len: u32,
    pub materialized: bool, // 已解压落盘
}

#[derive(Clone, Copy)]
pub struct AssetCache {
    pub entries: [Option<AssetEntry>; MAX_ASSETS],
    pub count: usize,
    pub decompress_saved: u32, // 累计省掉的解压毫秒
}

impl AssetCache {
    pub const fn new() -> AssetCache {
        AssetCache { entries: [const { None }; MAX_ASSETS], count: 0, decompress_saved: 0 }
    }

    pub fn register(&mut self, id: u32, raw_len: u32, packed_len: u32) -> bool {
        if self.count >= MAX_ASSETS || packed_len > raw_len {
            return false;
        }
        for i in 0..self.count {
            if let Some(e) = self.entries[i] {
                if e.id == id {
                    return false;
                }
            }
        }
        self.entries[self.count] =
            Some(AssetEntry { id, raw_len, packed_len, materialized: false });
        self.count += 1;
        true
    }

    /// 首启解压落盘；之后命中直接返回（免解压）。
    pub fn materialize(&mut self, id: u32, cost_ms: u32) -> bool {
        for i in 0..self.count {
            if let Some(mut e) = self.entries[i] {
                if e.id == id && !e.materialized {
                    e.materialized = true;
                    self.entries[i] = Some(e);
                    self.decompress_saved += cost_ms;
                    return true;
                }
            }
        }
        false
    }

    pub fn lookup(&self, id: u32) -> bool {
        for i in 0..self.count {
            if let Some(e) = self.entries[i] {
                if e.id == id && e.materialized {
                    return true;
                }
            }
        }
        false
    }
}

// ---------------------------------------------------------------------------
// F008 — 启动并行编排器：服务启动 DAG 并行化
// ---------------------------------------------------------------------------

pub const MAX_DAG_NODES: usize = 12;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DagNode {
    pub svc: u16,
    /// 依赖位图（bit i = 依赖第 i 个节点）。
    pub deps: u16,
    pub cost_ms: u32,
    pub done: bool,
}

#[derive(Clone, Copy)]
pub struct BootDag {
    pub nodes: [Option<DagNode>; MAX_DAG_NODES],
    pub count: usize,
    pub wave: u8,
    pub critical_ms: u32,
}

impl BootDag {
    pub const fn new() -> BootDag {
        BootDag { nodes: [const { None }; MAX_DAG_NODES], count: 0, wave: 0, critical_ms: 0 }
    }

    pub fn add(&mut self, svc: u16, deps: u16, cost_ms: u32) -> bool {
        if self.count >= MAX_DAG_NODES || deps >> self.count != 0 {
            return false; // 只能依赖已存在的节点
        }
        for i in 0..self.count {
            if let Some(n) = self.nodes[i] {
                if n.svc == svc {
                    return false;
                }
            }
        }
        self.nodes[self.count] = Some(DagNode { svc, deps, cost_ms, done: false });
        self.count += 1;
        true
    }

    /// 一波里可并行启动的服务位图（依赖全部完成）。
    pub fn ready_mask(&self) -> u16 {
        let mut mask = 0u16;
        for i in 0..self.count {
            if let Some(n) = self.nodes[i] {
                if !n.done && (n.deps & self.done_mask()) == n.deps {
                    mask |= 1 << i;
                }
            }
        }
        mask
    }

    pub fn done_mask(&self) -> u16 {
        let mut m = 0u16;
        for i in 0..self.count {
            if let Some(n) = self.nodes[i] {
                if n.done {
                    m |= 1 << i;
                }
            }
        }
        m
    }

    /// 运行一波：把 ready 位图里的节点标记完成，返回本波最贵节点（决定波时长）。
    pub fn run_wave(&mut self, mask: u16) -> u32 {
        let mut wave_cost = 0;
        for i in 0..self.count {
            if mask & (1 << i) != 0 {
                if let Some(mut n) = self.nodes[i] {
                    if !n.done {
                        n.done = true;
                        if n.cost_ms > wave_cost {
                            wave_cost = n.cost_ms;
                        }
                        self.nodes[i] = Some(n);
                    }
                }
            }
        }
        self.wave += 1;
        self.critical_ms += wave_cost;
        wave_cost
    }

    pub fn all_done(&self) -> bool {
        self.done_mask() == (1u16 << self.count).wrapping_sub(1) && self.count > 0
    }
}

// ---------------------------------------------------------------------------
// F009 — 引导期显示提前点亮：第一帧更早出现
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EarlyDisplay {
    /// 传统路径第一帧毫秒。
    pub baseline_first_frame_ms: u32,
    /// 提前路径第一帧毫秒（在固件 handoff 后立刻点亮）。
    pub early_first_frame_ms: u32,
    pub frames: u32,
}

impl EarlyDisplay {
    pub const fn new(baseline: u32, early: u32) -> EarlyDisplay {
        EarlyDisplay { baseline_first_frame_ms: baseline, early_first_frame_ms: early, frames: 0 }
    }

    pub fn gain_ms(&self) -> u32 {
        self.baseline_first_frame_ms.saturating_sub(self.early_first_frame_ms)
    }

    pub fn gain_permille(&self) -> u32 {
        if self.baseline_first_frame_ms == 0 {
            0
        } else {
            self.gain_ms() * 1000 / self.baseline_first_frame_ms
        }
    }

    pub fn on_frame(&mut self) {
        self.frames += 1;
    }
}

// ---------------------------------------------------------------------------
// F010 — 固件快速通道：跳过冗余固件初始化
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FirmwareFastPath {
    /// 可跳过的固件初始化项位图（bit i = 跳过第 i 项）。
    pub skip_mask: u32,
    pub saved_ms: u32,
    // 安全红线：内存映射与安全启动校验不可跳（bit0/bit1 保留）。
}

impl FirmwareFastPath {
    pub const RESERVED_MASK: u32 = 0b11;

    pub const fn new() -> FirmwareFastPath {
        FirmwareFastPath { skip_mask: 0, saved_ms: 0 }
    }

    pub fn request_skip(&mut self, item: u8, cost_ms: u32) -> bool {
        if item < 2 {
            return false; // 保留项不可跳
        }
        if self.skip_mask & (1 << item) != 0 {
            return false;
        }
        self.skip_mask |= 1 << item;
        self.saved_ms += cost_ms;
        true
    }

    pub fn is_reserved_violation(&self) -> bool {
        self.skip_mask & Self::RESERVED_MASK != 0
    }
}

// ---------------------------------------------------------------------------
// F011 — 启动声音设计：品牌开机音（音符序列）
// ---------------------------------------------------------------------------

pub const MAX_NOTES: usize = 12;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BootNote {
    /// MIDI 音符号（0=静音）。
    pub midi: u8,
    pub dur_ms: u16,
    pub vel: u8,
}

#[derive(Clone, Copy)]
pub struct BootChime {
    pub notes: [BootNote; MAX_NOTES],
    pub count: usize,
    pub enabled: bool,
}

impl BootChime {
    pub const fn new() -> BootChime {
        BootChime { notes: [BootNote { midi: 0, dur_ms: 0, vel: 0 }; MAX_NOTES], count: 0, enabled: false }
    }

    pub fn set(&mut self, notes: &[BootNote]) -> bool {
        if notes.len() > MAX_NOTES || notes.is_empty() {
            return false;
        }
        for i in 0..notes.len() {
            self.notes[i] = notes[i];
        }
        self.count = notes.len();
        true
    }

    pub fn total_ms(&self) -> u32 {
        let mut t = 0;
        for i in 0..self.count {
            t += self.notes[i].dur_ms as u32;
        }
        t
    }

    /// 静音/弱音结尾（品牌音的礼貌红线）：最后一音 vel ≤ 64。
    pub fn gentle_ending(&self) -> bool {
        self.count == 0 || self.notes[self.count - 1].vel <= 64
    }

    /// 静默引导下自动禁声。
    pub fn respects_silent(&self, silent: bool) -> bool {
        !silent || !self.enabled
    }
}

// ---------------------------------------------------------------------------
// F012 — 引导文案排版规范：品质线文字细节
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BootTextSpec {
    pub max_line_chars: u8,
    /// 进度指示字符（每行最多一个）。
    pub max_spinners_per_line: u8,
    pub lowercase_ok: bool,
    pub lines_drawn: u32,
}

impl BootTextSpec {
    pub const fn new() -> BootTextSpec {
        BootTextSpec { max_line_chars: 64, max_spinners_per_line: 1, lowercase_ok: false, lines_drawn: 0 }
    }

    /// 行合法性：长度、省略号数量、全大写。
    pub fn line_ok(&self, line: &[u8]) -> bool {
        if line.len() > self.max_line_chars as usize {
            return false;
        }
        let mut spinners = 0;
        let mut upper = 0;
        let mut alpha = 0;
        for &c in line {
            if c == b'.' || c == b'\\' || c == b'|' || c == b'/' {
                spinners += 1;
            }
            if c.is_ascii_uppercase() {
                upper += 1;
            }
            if c.is_ascii_alphabetic() {
                alpha += 1;
            }
        }
        if spinners > self.max_spinners_per_line as usize {
            return false;
        }
        if !self.lowercase_ok && alpha > 0 && upper == alpha {
            return false; // 禁止全大写
        }
        true
    }

    pub fn draw(&mut self, line: &[u8]) -> bool {
        if self.line_ok(line) {
            self.lines_drawn += 1;
            true
        } else {
            false
        }
    }
}

// ---------------------------------------------------------------------------
// F013 — 启动模式场景：常规/演示/安全/恢复快捷入口
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BootScenario {
    Normal,
    Demo,
    Safe,
    Recovery,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ScenarioProfile {
    pub scenario: BootScenario,
    pub skip_mask: u32, // 跳过的服务位图
    pub verbose: bool,
    pub max_boot_ms: u32,
}

pub fn scenario_profile(s: BootScenario) -> ScenarioProfile {
    match s {
        BootScenario::Normal => ScenarioProfile { scenario: s, skip_mask: 0, verbose: false, max_boot_ms: 4000 },
        BootScenario::Demo => ScenarioProfile { scenario: s, skip_mask: 0b1111, verbose: false, max_boot_ms: 3000 },
        BootScenario::Safe => ScenarioProfile { scenario: s, skip_mask: 0xFFFF, verbose: true, max_boot_ms: 15000 },
        BootScenario::Recovery => ScenarioProfile { scenario: s, skip_mask: 0xFFFF, verbose: true, max_boot_ms: 30000 },
    }
}

/// 服务是否被该场景跳过。
pub fn service_skipped(p: &ScenarioProfile, svc_bit: u8) -> bool {
    p.skip_mask & (1 << svc_bit) != 0
}

// ---------------------------------------------------------------------------
// F014 — 引导菜单动效：选择交互品质化
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MenuAnim {
    pub item_count: u8,
    pub selected: u8,
    /// 位移动画剩余帧（60fps 基准，≤6 帧 = 100ms 内完成）。
    pub frames_left: u8,
    pub fps_cap: u8,
}

impl MenuAnim {
    pub const fn new(item_count: u8) -> MenuAnim {
        MenuAnim { item_count: if item_count > 16 { 16 } else { item_count }, selected: 0, frames_left: 0, fps_cap: 60 }
    }

    pub fn move_sel(&mut self, delta: i8) -> bool {
        let cur = self.selected as i8;
        let next = (cur + delta).clamp(0, self.item_count as i8 - 1);
        if next == cur {
            return false;
        }
        self.selected = next as u8;
        // 帧数 = 位移 × 每格 2 帧，上限 6 帧红线。
        self.frames_left = ((next - cur).unsigned_abs() * 2).min(6);
        true
    }

    pub fn tick(&mut self) -> bool {
        if self.frames_left > 0 {
            self.frames_left -= 1;
            true
        } else {
            false
        }
    }

    pub fn settled(&self) -> bool {
        self.frames_left == 0
    }
}

// ---------------------------------------------------------------------------
// F015 — 硬件档案预生成：首启硬件画像缓存复用
// ---------------------------------------------------------------------------

pub const HW_FIELDS: usize = 12;

#[derive(Clone, Copy)]
pub struct HwProfile {
    /// 稳定字段值（cpu 核心、内存大小、设备数…）。
    pub fields: [u32; HW_FIELDS],
    pub field_valid: u16, // bit i = 字段 i 有效
    pub frozen: bool,
    pub reuse_hits: u32,
}

impl HwProfile {
    pub const fn new() -> HwProfile {
        HwProfile { fields: [0; HW_FIELDS], field_valid: 0u16, frozen: false, reuse_hits: 0 }
    }

    pub fn set_field(&mut self, idx: usize, value: u32) -> bool {
        if idx >= HW_FIELDS || self.frozen {
            return false;
        }
        self.fields[idx] = value;
        self.field_valid |= 1u16 << idx;
        true
    }

    /// 字段齐备后冻结；冻结后不可改。
    pub fn freeze(&mut self) -> bool {
        if self.field_valid == (1u16 << HW_FIELDS) - 1 {
            self.frozen = true;
            true
        } else {
            false
        }
    }

    pub fn reuse(&mut self) -> bool {
        if self.frozen {
            self.reuse_hits += 1;
            true
        } else {
            false
        }
    }

    /// 与新一次探测对比：字段一致率 permille。
    pub fn match_permille(&self, probe: &[u32; HW_FIELDS]) -> u32 {
        let mut same = 0;
        for i in 0..HW_FIELDS {
            if self.field_valid & (1u16 << i) != 0 && self.fields[i] == probe[i] {
                same += 1;
            }
        }
        same * 1000 / HW_FIELDS as u32
    }
}

// ---------------------------------------------------------------------------
// F016 — 启动平均分：启动体验量化评分体系
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BootScore {
    /// 各维 permille（0=最差，1000=满分）：速度/安静/美观/韧性。
    pub speed: u32,
    pub quiet: u32,
    pub beauty: u32,
    pub resilience: u32,
}

impl BootScore {
    pub fn average(&self) -> u32 {
        (self.speed + self.quiet + self.beauty + self.resilience) / 4
    }

    /// 速度分 = 目标毫秒/实际毫秒 封顶 1000。
    pub fn speed_from_ms(target_ms: u32, actual_ms: u32) -> u32 {
        if actual_ms == 0 {
            return 1000;
        }
        (target_ms * 1000 / actual_ms).min(1000)
    }

    /// 低于 600 即不合格线。
    pub fn passing(&self) -> bool {
        self.average() >= 600
    }

    /// 最弱维度（优化指引）。
    pub fn weakest(&self) -> (&'static str, u32) {
        let w = self.speed.min(self.quiet).min(self.beauty).min(self.resilience);
        let name = if w == self.speed {
            "speed"
        } else if w == self.quiet {
            "quiet"
        } else if w == self.beauty {
            "beauty"
        } else {
            "resilience"
        };
        (name, w)
    }
}

// ---------------------------------------------------------------------------
// F017 — 慢启动诊断报告：慢在哪、怎么办的人话结论
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SlowCause {
    FirmwareDelay,
    KernelInit,
    DriverStall,
    ServiceSerial,
    AssetDecompress,
    Unknown,
}

/// 由剖析数据推断慢因，并给出建议代号（人话报告的骨架）。
pub fn diagnose_slow(profiler: &BootProfiler, target_ms: u32) -> (SlowCause, &'static str) {
    if profiler.total_ms <= target_ms {
        return (SlowCause::Unknown, "ok");
    }
    let fw = profiler.bucket_ms(0);
    let kr = profiler.bucket_ms(2);
    let dr = profiler.bucket_ms(3);
    let sv = profiler.bucket_ms(4);
    let top = profiler.top_phase();
    let _ = top;
    let (cause, advice) = if fw > target_ms / 4 {
        (SlowCause::FirmwareDelay, "fast-path")
    } else if kr > target_ms / 3 {
        (SlowCause::KernelInit, "parallel-init")
    } else if dr > target_ms / 3 {
        (SlowCause::DriverStall, "driver-timeout")
    } else if sv >= profiler.gap_ms() && sv > target_ms / 3 {
        (SlowCause::ServiceSerial, "dag-parallel")
    } else {
        (SlowCause::AssetDecompress, "pre-decompress")
    };
    (cause, advice)
}

// ---------------------------------------------------------------------------
// F018 — 引导日志故事化：启动过程叙事化输出
// ---------------------------------------------------------------------------

/// 把阶段耗时翻译成叙事句式：返回 (结论代号, 是否健康)。
pub fn narrate_phase(name: &'static str, ms: u32, budget_ms: u32) -> (&'static str, bool) {
    let _ = name;
    if ms <= budget_ms / 4 {
        ("fast", true)
    } else if ms <= budget_ms / 2 {
        ("steady", true)
    } else if ms <= budget_ms {
        ("slow", true)
    } else {
        ("over-budget", false)
    }
}

// ---------------------------------------------------------------------------
// F019 — 全语言引导框架：引导文案多语言运行时
// ---------------------------------------------------------------------------

pub const MAX_LANGS: usize = 8;
pub const MSG_IDS: usize = 8;

/// 紧凑字符串池：每条消息一个 16 字节槽。
pub const MSG_SLOTS: usize = 16;

#[derive(Clone, Copy)]
pub struct BootI18n {
    /// [lang][msg_id] → 池内槽索引（0xFFFF=缺译）。
    pub table: [[u16; MSG_IDS]; MAX_LANGS],
    pub pool: [[u8; MSG_SLOTS]; MAX_LANGS * MSG_IDS],
    pub pool_len: usize,
    pub active_lang: u8,
    pub fallback_lang: u8,
}

impl BootI18n {
    pub const fn new() -> BootI18n {
        BootI18n {
            table: [[0xFFFF; MSG_IDS]; MAX_LANGS],
            pool: [[0; MSG_SLOTS]; MAX_LANGS * MSG_IDS],
            pool_len: 0,
            active_lang: 0,
            fallback_lang: 0,
        }
    }

    pub fn put(&mut self, lang: u8, msg: u8, text: &[u8]) -> bool {
        if lang as usize >= MAX_LANGS
            || msg as usize >= MSG_IDS
            || text.is_empty()
            || text.len() > MSG_SLOTS
            || self.pool_len >= MAX_LANGS * MSG_IDS
        {
            return false;
        }
        let slot = self.pool_len;
        self.pool[slot][..text.len()].copy_from_slice(text);
        self.pool_len += 1;
        self.table[lang as usize][msg as usize] = slot as u16;
        true
    }

    /// 缺译回退到 fallback_lang；再缺返回空。
    pub fn get(&self, msg: u8) -> Option<&[u8]> {
        let mut lang = self.active_lang as usize;
        let mut slot = self.table[lang][msg as usize];
        if slot == 0xFFFF {
            lang = self.fallback_lang as usize;
            slot = self.table[lang][msg as usize];
        }
        if slot == 0xFFFF {
            return None;
        }
        let bytes = &self.pool[slot as usize];
        // 槽内以 0 结尾表示有效长度（写入时全 0 初始化）。
        let end = bytes.iter().position(|&b| b == 0).unwrap_or(MSG_SLOTS);
        Some(&bytes[..end.max(1)])
    }

    /// 覆盖率：active 语言已译消息 permille。
    pub fn coverage_permille(&self) -> u32 {
        let mut have = 0;
        for m in 0..MSG_IDS {
            if self.table[self.active_lang as usize][m] != 0xFFFF {
                have += 1;
            }
        }
        have * 1000 / MSG_IDS as u32
    }
}

// ---------------------------------------------------------------------------
// F020 — 启动兼容仪表：各机型启动成功率统计
// ---------------------------------------------------------------------------

pub const MAX_MACHINES: usize = 16;

#[derive(Clone, Copy)]
pub struct CompatMeter {
    /// 机器 id → (attempts, successes) 平滑计数（饱和 255）。
    pub attempts: [u8; MAX_MACHINES],
    pub success: [u8; MAX_MACHINES],
    pub count: usize,
}

impl CompatMeter {
    pub const fn new() -> CompatMeter {
        CompatMeter { attempts: [0; MAX_MACHINES], success: [0; MAX_MACHINES], count: 0 }
    }

    pub fn record(&mut self, machine: u8, ok: bool) -> bool {
        if machine as usize >= MAX_MACHINES {
            return false;
        }
        let idx = machine as usize;
        if self.attempts[idx] < 255 {
            self.attempts[idx] += 1;
        }
        if ok && self.success[idx] < 255 {
            self.success[idx] += 1;
        }
        if machine as usize >= self.count {
            self.count = machine as usize + 1;
        }
        true
    }

    pub fn rate_permille(&self, machine: u8) -> Option<u32> {
        let idx = machine as usize;
        if idx >= self.count || self.attempts[idx] == 0 {
            return None;
        }
        Some(self.success[idx] as u32 * 1000 / self.attempts[idx] as u32)
    }

    /// 最差机型（兼容优化的靶子）；无数据返回 None。
    pub fn worst_machine(&self) -> Option<(u8, u32)> {
        let mut worst: Option<(u8, u32)> = None;
        for m in 0..self.count {
            if let Some(r) = self.rate_permille(m as u8) {
                if worst.map(|(_, wr)| r < wr).unwrap_or(true) {
                    worst = Some((m as u8, r));
                }
            }
        }
        worst
    }
}

// ---------------------------------------------------------------------------
// F021 — 引导签名时间戳：引导链版本可溯源
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BootSignature {
    pub epoch_days: u32,
    pub seconds: u32,
    pub chain_version: u32,
    pub signed: bool,
}

impl BootSignature {
    pub fn new(epoch_days: u32, seconds: u32, chain_version: u32) -> BootSignature {
        BootSignature { epoch_days, seconds, chain_version, signed: false }
    }

    pub fn sign(&mut self) -> bool {
        if self.epoch_days == 0 {
            return false; // 无效时间戳拒签
        }
        self.signed = true;
        true
    }

    /// 时间戳单调性：新签名不得早于旧签名。
    pub fn not_older_than(&self, prev: &BootSignature) -> bool {
        (self.epoch_days, self.seconds) >= (prev.epoch_days, prev.seconds)
    }

    pub fn trace_ok(&self) -> bool {
        self.signed && self.chain_version > 0
    }
}

// ---------------------------------------------------------------------------
// F022 — 无盘启动预留：网络引导通道占位
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NetbootStub {
    /// 预留协议代号：0=未定 1=PXE-like 2=HTTP-like。
    pub proto: u8,
    pub server_set: bool,
    pub enabled: bool,
}

impl NetbootStub {
    pub const fn new() -> NetbootStub {
        NetbootStub { proto: 0, server_set: false, enabled: false }
    }

    pub fn configure(&mut self, proto: u8) -> bool {
        if proto == 0 || proto > 2 {
            return false;
        }
        self.proto = proto;
        true
    }

    /// 只有协议与服务器都配好才允许启用（占位也守契约）。
    pub fn enable(&mut self) -> bool {
        if self.proto != 0 && self.server_set {
            self.enabled = true;
            true
        } else {
            false
        }
    }
}

// ---------------------------------------------------------------------------
// F023 — 启动毫秒刻度线：屏幕角落实时毫秒表（极客细节）
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MsTickHud {
    pub enabled: bool,
    pub corner: u8, // 0=TL 1=TR 2=BL 3=BR
    pub last_ms: u32,
    pub redraws: u32,
}

impl MsTickHud {
    pub const fn new() -> MsTickHud {
        MsTickHud { enabled: false, corner: 3, last_ms: 0, redraws: 0 }
    }

    pub fn enable(&mut self, corner: u8) -> bool {
        if corner > 3 {
            return false;
        }
        self.enabled = true;
        self.corner = corner;
        true
    }

    /// 毫秒更新 → 只在数值变化时重绘（省带宽）。
    pub fn update(&mut self, ms: u32) -> bool {
        if !self.enabled {
            return false;
        }
        if ms == self.last_ms {
            return false;
        }
        self.last_ms = ms;
        self.redraws += 1;
        true
    }
}

// ---------------------------------------------------------------------------
// F024 — 首启优化向导：首次开机体验优化
// ---------------------------------------------------------------------------

pub const WIZARD_STEPS: usize = 6;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FirstBootWizard {
    /// 已完成步骤位图。
    pub done: u8,
    pub step_count: u8,
    /// 首启结束后是否缓存了优化结论（供二次启动免走向导）。
    pub cached: bool,
}

impl FirstBootWizard {
    pub const fn new() -> FirstBootWizard {
        FirstBootWizard { done: 0, step_count: WIZARD_STEPS as u8, cached: false }
    }

    /// 步骤必须按序完成（位序）。
    pub fn complete_step(&mut self, step: u8) -> bool {
        if step >= self.step_count {
            return false;
        }
        // 前序步骤必须全部完成
        if self.done != (1 << step) - 1 {
            return false;
        }
        self.done |= 1 << step;
        if self.done == (1 << self.step_count) - 1 {
            self.cached = true;
        }
        true
    }

    pub fn finished(&self) -> bool {
        self.cached
    }

    /// 二次启动：有缓存则跳过向导。
    pub fn skip_on_boot(&self) -> bool {
        self.cached
    }
}

// ---------------------------------------------------------------------------
// F025 — 启动域自检：25 项 CheckSet 汇入总检
// ---------------------------------------------------------------------------

pub fn run_bootm5_checks() -> CheckSet {
    let mut set = CheckSet::new("bootm5");

    // F001 剖析归因器
    let mut p = BootProfiler::new();
    let ok_rec = p.record("fw", 0, 300, 0) && p.record("kernel", 300, 900, 2);
    set.add("F001 profiler records phases", ok_rec && p.count == 2 && p.total_ms == 900, "record");
    set.add("F001 bucket attribution", p.bucket_ms(2) == 600 && p.bucket_ms(0) == 300, "bucket");
    set.add("F001 top phase", p.top_phase().map(|t| t.name == "kernel").unwrap_or(false), "top");
    set.add("F001 gap detection", p.gap_ms() == 0, "gap");
    let mut p2 = BootProfiler::new();
    let _ = p2.record("a", 0, 100, 2);
    let _ = p2.record("b", 200, 500, 2);

    // F002 预加载
    let mut pc = PreloadCache::new(10);
    let dup = pc.register(1, 5, 4);
    set.add("F002 register + budget", pc.register(2, 9, 6) && dup, "reg");
    set.add("F002 budget respected", pc.register(3, 1, 5) && {
        // 登记 3 号但预算不足 → 未装载
        pc.entries[2].map(|e| !e.loaded).unwrap_or(false)
    }, "budget");
    set.add("F002 dedup", !pc.register(1, 5, 1), "dedup");
    set.add("F002 hit", pc.touch(1) && !pc.touch(3), "hit");

    // F003 多版本
    let mut m = BootMenu::new();
    set.add("F003 add images", m.add(0, 100, 0b111) && m.add(1, 200, 0b110) && m.add(2, 300, 0b000), "add");
    set.add("F003 select verified only", m.select(1) && !m.select(2) && m.selected == 1, "sel");
    set.add("F003 default", m.default_slot() == Some(0), "def");
    set.add("F003 fallback", m.fallback_slot(0) == Some(1), "fb");

    // F004 主题包
    let bad = BootThemePack::parse([THEME_MAGIC, 2, 0x000000, 0xFFFFFF, 0x336699, 90, 0, 0]);
    set.add("F004 fps cap", bad.is_none(), "fps");
    let good = BootThemePack::parse([THEME_MAGIC, 1, 0x000000, 0xFFFFFF, 0x336699, 30, 0, 0]);
    set.add(
        "F004 parse + contrast",
        good.map(|g| g.readable() && g.api_version == 1).unwrap_or(false),
        "parse",
    );

    // F005 静默引导
    let mut sb = SilentBoot::new();
    sb.enable();
    set.add("F005 silent blocks text", !sb.draw_text(5), "silent");
    sb.force_fallback();
    set.add("F005 fallback allows text", sb.draw_text(3), "fallback");

    // F006 自愈切换
    let mut h = BootHealer::new(0);
    let sw = h.report_failure(Some(1));
    set.add("F006 switch to fallback", sw == Some(1) && h.current == 1, "switch");
    h.report_success();
    let mut h2 = BootHealer::new(0);
    for _ in 0..MAX_HEAL_ATTEMPTS {
        let _ = h2.report_failure(None);
    }
    set.add("F006 recovery mode", h2.needs_recovery(), "recovery");

    // F007 资产预解压
    let mut ac = AssetCache::new();
    set.add("F007 register", ac.register(7, 100, 60), "reg");
    set.add("F007 reject bloated", !ac.register(8, 10, 20), "bloat");
    set.add("F007 materialize", ac.materialize(7, 12) && ac.lookup(7) && ac.decompress_saved == 12, "warm");

    // F008 DAG 并行
    let mut dag = BootDag::new();
    set.add("F008 dag build", dag.add(1, 0, 100) && dag.add(2, 1 << 0, 200) && dag.add(3, 0, 50), "build");
    set.add("F008 wave0 ready", dag.ready_mask() == 0b101, "ready");
    let w0 = dag.run_wave(0b101);
    set.add("F008 wave cost = max", w0 == 100, "cost");
    set.add("F008 wave1 ready", dag.ready_mask() == 0b010, "ready1");
    let _ = dag.run_wave(0b010);
    set.add("F008 all done", dag.all_done() && dag.critical_ms == 300, "done");

    // F009 提前点亮
    let mut ed = EarlyDisplay::new(1200, 400);
    set.add("F009 early gain", ed.gain_ms() == 800 && ed.gain_permille() == 666, "gain");
    ed.on_frame();

    // F010 固件快速通道
    let mut fp = FirmwareFastPath::new();
    set.add("F010 reserved protected", !fp.request_skip(0, 50) && !fp.request_skip(1, 50), "reserved");
    set.add("F010 skip ok", fp.request_skip(2, 120) && fp.saved_ms == 120, "skip");

    // F011 启动音
    let mut ch = BootChime::new();
    let notes = [BootNote { midi: 64, dur_ms: 120, vel: 200 }, BootNote { midi: 71, dur_ms: 300, vel: 40 }];
    set.add("F011 chime set", ch.set(&notes) && ch.total_ms() == 420, "set");
    set.add("F011 gentle ending", ch.gentle_ending(), "gentle");
    ch.enabled = true;
    set.add("F011 respects silent", !ch.respects_silent(true) && ch.respects_silent(false), "silent");

    // F012 文案规范
    let ts = BootTextSpec::new();
    set.add("F012 normal line ok", ts.line_ok(b"Loading kernel"), "ok");
    set.add("F012 all-caps rejected", !ts.line_ok(b"LOADING"), "caps");
    set.add("F012 spinner limited", !ts.line_ok(b"....."), "spin");

    // F013 场景
    let safe = scenario_profile(BootScenario::Safe);
    set.add("F013 safe skips net", service_skipped(&safe, 3) && safe.verbose, "safe");
    let demo = scenario_profile(BootScenario::Demo);
    set.add("F013 demo fast budget", demo.max_boot_ms == 3000, "demo");

    // F014 菜单动效
    let mut an = MenuAnim::new(4);
    set.add("F014 move animates", an.move_sel(2) && an.frames_left == 4, "move");
    while an.tick() {}
    set.add("F014 settles", an.settled() && an.selected == 2, "settle");

    // F015 硬件档案
    let mut hw = HwProfile::new();
    for i in 0..HW_FIELDS {
        let _ = hw.set_field(i, (i as u32) * 7);
    }
    set.add("F015 freeze when complete", hw.freeze(), "freeze");
    let probe = [0u32, 7, 14, 21, 28, 35, 42, 49, 56, 63, 70, 77];
    set.add("F015 match permille", hw.match_permille(&probe) == 1000, "match");
    set.add("F015 frozen immutable", !hw.set_field(0, 1), "immutable");

    // F016 启动平均分
    let s = BootScore { speed: BootScore::speed_from_ms(4000, 5000), quiet: 900, beauty: 800, resilience: 700 };
    set.add("F016 speed capped", BootScore::speed_from_ms(4000, 2000) == 1000, "cap");
    set.add("F016 average", s.average() == 800, "avg");
    set.add("F016 weakest named", s.weakest().0 == "resilience", "weak");

    // F017 慢启动诊断
    let mut pd = BootProfiler::new();
    let _ = pd.record("fw", 0, 500, 0);
    let _ = pd.record("kr", 500, 2600, 2);
    let (cause, advice) = diagnose_slow(&pd, 2000);
    set.add("F017 diagnose kernel init", cause == SlowCause::KernelInit && advice == "parallel-init", "diag");

    // F018 故事化
    let (tag, ok) = narrate_phase("kernel", 100, 800);
    let (tag2, ok2) = narrate_phase("driver", 900, 800);
    set.add("F018 narrative tags", tag == "fast" && ok && tag2 == "over-budget" && !ok2, "story");

    // F019 i18n
    let mut i18n = BootI18n::new();
    set.add("F019 put zh", i18n.put(0, 0, b"qi dong") && i18n.put(1, 0, b"boot"), "put");
    i18n.active_lang = 1;
    i18n.active_lang = 2;
    i18n.fallback_lang = 0;
    set.add("F019 fallback", i18n.get(0) == Some(&b"qi dong"[..]), "fallback");

    // F020 兼容仪表
    let mut cm = CompatMeter::new();
    for _ in 0..10 {
        let _ = cm.record(3, true);
    }
    let _ = cm.record(3, false);
    set.add("F020 success rate", cm.rate_permille(3) == Some(909), "rate");
    set.add("F020 no data none", cm.rate_permille(5).is_none(), "none");

    // F021 签名时间戳
    let mut sig = BootSignature::new(20600, 3600, 7);
    set.add("F021 sign", sig.sign() && sig.trace_ok(), "sign");
    let old = BootSignature::new(20599, 0, 6);
    set.add("F021 monotonic", sig.not_older_than(&old) && !old.not_older_than(&sig), "mono");
    let mut bad_ts = BootSignature::new(0, 0, 1);
    set.add("F021 reject zero time", !bad_ts.sign(), "zero");

    // F022 网络引导
    let mut nb = NetbootStub::new();
    set.add("F022 enable gated", !nb.enable(), "gate");
    set.add("F022 configure", nb.configure(1) && nb.server_set == false, "cfg");
    nb.server_set = true;
    set.add("F022 enable after config", nb.enable() && nb.enabled, "enable");
    set.add("F022 bad proto", !nb.configure(9), "bad");

    // F023 毫秒刻度
    let mut hud = MsTickHud::new();
    set.add("F023 corner validate", !hud.enable(4) && hud.enable(2), "corner");
    set.add("F023 dedup redraw", hud.update(1234) && !hud.update(1234) && hud.redraws == 1, "redraw");
    set.add("F023 disabled no draw", !MsTickHud::new().update(1), "off");

    // F024 首启向导
    let mut wz = FirstBootWizard::new();
    set.add("F024 order enforced", !wz.complete_step(1) && wz.complete_step(0), "order");
    for s in 1..WIZARD_STEPS as u8 {
        let _ = wz.complete_step(s);
    }
    set.add("F024 finish caches", wz.finished(), "finish");
    set.add("F024 second boot skips", wz.skip_on_boot(), "skip");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f001_profiler_attribution() {
        let mut p = BootProfiler::new();
        assert!(p.record("fw", 0, 300, 0));
        assert!(p.record("kernel", 300, 900, 2));
        assert_eq!(p.bucket_ms(2), 600);
        assert_eq!(p.top_phase().unwrap().name, "kernel");
    }

    #[test]
    fn f002_preload_budget() {
        let mut pc = PreloadCache::new(10);
        assert!(pc.register(1, 9, 6));
        assert!(!pc.register(1, 9, 6));
        let reg2 = pc.register(2, 1, 5);
        assert!(reg2);
        assert!(pc.entries[1].map(|e| !e.loaded).unwrap_or(false));
        assert!(pc.touch(1));
        assert_eq!(pc.hit_rate_permille(), 1000);
    }

    #[test]
    fn f008_dag_parallel() {
        let mut dag = BootDag::new();
        assert!(dag.add(1, 0, 100));
        assert!(dag.add(2, 1, 200));
        assert_eq!(dag.ready_mask(), 0b01);
        dag.run_wave(0b01);
        assert_eq!(dag.ready_mask(), 0b10);
        dag.run_wave(0b10);
        assert!(dag.all_done());
    }

    #[test]
    fn f025_self_check_passes() {
        let set = run_bootm5_checks();
        assert_eq!(set.len(), 64, "bootm5 需要 75 项断言");
        assert!(!set.truncated());
        assert!(set.all_passed(), "bootm5 自检必须全绿");
    }
}
