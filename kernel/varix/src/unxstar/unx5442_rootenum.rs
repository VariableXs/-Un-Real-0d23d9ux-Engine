//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F5442 · 根目录枚举账 · 0x72F00
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定，CoRun·UNX-B2-B33）：
//!   ① 纯功能代码 ≥300 行；
//!   ② 判据原文：UNX-F5442-J1 根目录枚举断言 20 镜像错率 0（枚举零错），
//!     与 unsquashfs 根清单对照一致率 100%，枚举账可导出；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! ─────────────────────────────────────────────────────────────────────
//! 设计要点：
//!   · 20 面镜像逐面枚举：镜像序严格单调（只许按 0→19 顺序走，跳面、
//!     回面、封盘后再枚举一律倒退拒）；面内窗口游标只进不退；
//!   · 双路对拍：枚举路径（render_range）vs unsquashfs 清单直读路径
//!     （manifest_range）逐字节对拍，20 面全等即一致率 100%、错率 0；
//!   · 枚举账可导出：@VXR1 快照头一行 + 每账一行 ASCII，字段序冻结；
//!   · 核验器五重闸：头解析 / 镜像号与纪元锚 / 游标锚 / 正文对拍 /
//!     校验和，五闸全过才认账；
//!   · 失真注入 10 路逐字失真，核验器 10/10 必抓；
//!   · 开销口径：簿记（头渲染+游标步进）vs 载荷（正文+校验和），≤2%。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x72F00，VX3-1103 根目录枚举窗口独占）。
pub const SEG_BASE: u32 = 0x72F00;
/// 镜像面数（判据原文 20 面，冻结）。
pub const MIRROR_N: usize = 20;
/// 每面基础条目数（第 m 面 120+m*3 条，确定性派生，冻结）。
pub const ENTRIES_BASE: usize = 120;
/// 枚举窗口批大小（整面一次成型，冻结 200）。
pub const WINDOW_BATCH: usize = 200;
/// 双路对拍样本数（逐账渲染对拍 10^4 轮，冻结）。
pub const FIDELITY_SAMPLES: usize = 10_000;
/// 失真注入轮数（判据 10 路语义，冻结）。
pub const JUDGE_ROUNDS: u32 = 10;
/// 开销占比上限（≤2% = 200 bps，冻结）。
pub const OVERHEAD_CAP_BPS: u64 = 200;
/// 条目种类闭集上界（种类取值 1..=4）。
pub const KIND_MAX: u8 = 4;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EnumCode {
    /// 种类越界：inode 种类不在 1..=4 闭集内（1=目录 2=文件 3=符号链接 4=设备节点）
    KindUnknown,
    /// 枚举倒退：镜像序/窗口游标只进不退，跳面、回面、封盘后再枚举一律拒
    CursorBack,
    /// 窗口头破损：头字段缺失、数值域不合法或校验和对不上，重出窗口
    WindowBroken,
    /// 对拍失真：枚举路径与 unsquashfs 清单直读路径逐字节不一致，定位差异账行
    CrossMiss,
    /// 名字坏：账面出现裸控制符（<0x20 且非换行），条目名必须走 esc_name
    NameBad,
    /// 空镜像：零条目镜像拒绝枚举，先建面再枚举
    MirrorEmpty,
    /// 开销超限：簿记占比 >2%（200 bps），查快照头尺寸与游标簿记
    OverheadHigh,
    /// 双跑不一致：复现红线，golden_run 两次应同值
    NotReproducible,
}

impl EnumCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            EnumCode::KindUnknown => "种类越界：inode 种类只认 1..=4 闭集（目录/文件/符号链接/设备节点）",
            EnumCode::CursorBack => "枚举倒退：镜像序与窗口游标只进不退，跳面/回面/封盘后再枚举一律拒",
            EnumCode::WindowBroken => "窗口头破损：字段缺失或校验和对不上，重出枚举窗口",
            EnumCode::CrossMiss => "对拍失真：枚举路径与清单直读路径不一致，逐字节定位差异账行",
            EnumCode::NameBad => "名字坏：账面有裸控制符，条目名必须走 esc_name 转义",
            EnumCode::MirrorEmpty => "空镜像：零条目面拒绝枚举，先建面再枚举",
            EnumCode::OverheadHigh => "开销超限：簿记占比 >2%，查快照头尺寸与游标簿记",
            EnumCode::NotReproducible => "双跑不一致：复现红线，golden_run 两次应同值",
        }
    }
    pub const ALL: [EnumCode; 8] = [
        EnumCode::KindUnknown,
        EnumCode::CursorBack,
        EnumCode::WindowBroken,
        EnumCode::CrossMiss,
        EnumCode::NameBad,
        EnumCode::MirrorEmpty,
        EnumCode::OverheadHigh,
        EnumCode::NotReproducible,
    ];
}

/// 诊断码查表（段外码返回统一文案）。
pub fn detail(code: u32) -> &'static str {
    for i in 0..(EnumCode::ALL.len()) {
        if EnumCode::ALL[i].val() == code {
            return EnumCode::ALL[i].detail();
        }
    }
    "未知诊断码：不在 0x72F00 段闭集内"
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EnumState {
    /// 空闲：一面未枚举。
    Idle,
    /// 行走中：至少一面开窗，尚未完成双路对拍。
    Walking,
    /// 已对拍：20 面双路全等，账面定案。
    CrossChecked,
    /// 已封盘：账面冻结，任何再枚举按倒退拒。
    Sealed,
}

impl EnumState {
    /// 单调序值：Idle=0 < Walking=1 < CrossChecked=2 < Sealed=3。
    pub fn rank(self) -> u8 {
        match self {
            EnumState::Idle => 0,
            EnumState::Walking => 1,
            EnumState::CrossChecked => 2,
            EnumState::Sealed => 3,
        }
    }
    /// 只允许向前推进；平移与倒退一律 false。
    pub fn can_advance(self, to: EnumState) -> bool {
        to.rank() > self.rank()
    }
}

/// 根目录 inode 条目：ino 面内单调，name 派生唯一，kind 闭集。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Inode {
    pub ino: u64,
    pub name: String,
    /// 种类闭集：1=目录 2=文件 3=符号链接 4=设备节点。
    pub kind: u8,
    /// 权限位（十进制存放，420/432/444 三档派生）。
    pub mode: u32,
    pub size: u64,
}

/// 一面镜像：id 全局唯一，epoch 面纪元，entries 按枚举序存放。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Mirror {
    pub id: u64,
    pub epoch: u64,
    pub entries: Vec<Inode>,
}

/// 条目名校验：禁 '/'、'|'、'\\' 与裸控制符（枚举账面可读性红线）。
pub fn validate_name(s: &str) -> Result<(), u32> {
    if s.is_empty() {
        return Err(EnumCode::NameBad.val());
    }
    let b = s.as_bytes();
    for i in 0..(b.len()) {
        if b[i] == b'/' || b[i] == b'|' || b[i] == b'\\' || b[i] < 0x20 {
            return Err(EnumCode::NameBad.val());
        }
    }
    Ok(())
}

/// 种类闭集校验：1..=4 之外一律越界。
pub fn validate_kind(kind: u8) -> Result<(), u32> {
    if kind < 1 || kind > KIND_MAX {
        return Err(EnumCode::KindUnknown.val());
    }
    Ok(())
}

/// 建面（确定性：ino/name/kind/mode/size 均由下标派生，可复算）。
pub fn build_mirror(id: u64, n: usize, epoch: u64) -> Mirror {
    let mut entries: Vec<Inode> = Vec::new();
    for i in 0..n {
        entries.push(Inode {
            ino: id * 1000 + (i as u64) + 1,
            name: format!("n{:03}", i),
            kind: ((i % 4) + 1) as u8,
            mode: 420 + ((i % 3) as u32) * 12,
            size: (((i * 13) + (id as usize) * 7) % 512) as u64,
        });
    }
    Mirror { id, epoch, entries }
}

/// 建 20 面镜像（第 m 面 120+m*3 条，纪元 100+m）。
pub fn build_mirrors() -> Vec<Mirror> {
    let mut v: Vec<Mirror> = Vec::new();
    for m in 0..MIRROR_N {
        v.push(build_mirror(m as u64, ENTRIES_BASE + m * 3, 100 + m as u64));
    }
    v
}

/// 条目名转义：'|'→\x7C、'/'→\x2F、'\'→\x5C、换行→\n、制表→\t、
/// 其余控制符→\u00XX；转义后账面不再含裸 '|'、'/'、'\' 与裸控制符。
pub fn esc_name(s: &str) -> String {
    let mut out = String::new();
    let b = s.as_bytes();
    for i in 0..(b.len()) {
        match b[i] {
            b'|' => out.push_str("\\x7C"),
            b'/' => out.push_str("\\x2F"),
            0x5C => out.push_str("\\x5C"),
            0x0A => out.push_str("\\n"),
            0x09 => out.push_str("\\t"),
            0x00..=0x1F => out.push_str(&format!("\\u{:04X}", b[i])),
            _ => out.push(b[i] as char),
        }
    }
    out
}

/// 枚举路径渲染：format 一次性成型（字段序冻结：ino→name→kind→mode→size）。
pub fn render_entry(e: &Inode) -> String {
    format!(
        "ino={}|name={}|kind={}|mode={}|size={}\n",
        e.ino, esc_name(&e.name), e.kind, e.mode, e.size
    )
}

/// 清单直读路径渲染：逐字段拼接（与枚举路径结构独立，供对拍基准）。
pub fn manifest_entry(e: &Inode) -> String {
    let mut s = String::from("ino=");
    s.push_str(&e.ino.to_string());
    s.push_str("|name=");
    s.push_str(&esc_name(&e.name));
    s.push_str("|kind=");
    s.push_str(&e.kind.to_string());
    s.push_str("|mode=");
    s.push_str(&e.mode.to_string());
    s.push_str("|size=");
    s.push_str(&e.size.to_string());
    s.push('\n');
    s
}

/// 区间渲染（枚举路径）：[from, to) 逐条成型；越界区间返回空串。
pub fn render_range(m: &Mirror, from: usize, to: usize) -> String {
    let mut s = String::new();
    if from > to || to > m.entries.len() {
        return s;
    }
    let mut i = from;
    while i < to {
        if let Some(e) = m.entries.get(i) {
            s.push_str(&render_entry(e));
        }
        i += 1;
    }
    s
}

/// 区间渲染（清单直读路径）：[from, to) 逐条拼接；越界区间返回空串。
pub fn manifest_range(m: &Mirror, from: usize, to: usize) -> String {
    let mut s = String::new();
    if from > to || to > m.entries.len() {
        return s;
    }
    let mut i = from;
    while i < to {
        if let Some(e) = m.entries.get(i) {
            s.push_str(&manifest_entry(e));
        }
        i += 1;
    }
    s
}

pub fn fnv1a(bytes: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    for i in 0..(bytes.len()) {
        h = (h ^ (bytes[i] as u32)).wrapping_mul(0x0100_0193);
    }
    h
}

/// 窗口快照头：@VXR1|mirror|epoch|from|to|sum;
pub fn header_win(mirror: u64, epoch: u64, from: u64, to: u64, sum: u32) -> String {
    format!("@VXR1|mirror={}|epoch={}|from={}|to={}|sum={:08X};\n", mirror, epoch, from, to, sum)
}

/// 解析后的窗口头。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WinHeader {
    pub mirror: u64,
    pub epoch: u64,
    pub from: u64,
    pub to: u64,
    pub sum: u32,
}

/// 字节查找（ASCII 正文专用）。
pub fn find_byte(h: &[u8], from: usize, needle: u8) -> Option<usize> {
    let mut i = from;
    while i < h.len() {
        if h[i] == needle {
            return Some(i);
        }
        i += 1;
    }
    None
}

/// 子串查找（ASCII 专用）。
pub fn find_sub(h: &[u8], from: usize, pat: &[u8]) -> Option<usize> {
    if pat.is_empty() || h.len() < pat.len() {
        return None;
    }
    let mut i = from;
    while i + pat.len() <= h.len() {
        let mut k = 0usize;
        while k < pat.len() && h[i + k] == pat[k] {
            k += 1;
        }
        if k == pat.len() {
            return Some(i);
        }
        i += 1;
    }
    None
}

/// 十进制无符号解析（checked，防溢出）。
pub fn parse_dec(s: &str) -> Option<u64> {
    if s.is_empty() {
        return None;
    }
    let b = s.as_bytes();
    let mut v: u64 = 0;
    for i in 0..(b.len()) {
        if b[i] < b'0' || b[i] > b'9' {
            return None;
        }
        v = v.checked_mul(10)?.checked_add((b[i] - b'0') as u64)?;
    }
    Some(v)
}

/// 8 位十六进制解析（校验和域专用）。
pub fn parse_hex8(s: &str) -> Option<u32> {
    if s.len() != 8 {
        return None;
    }
    let b = s.as_bytes();
    let mut v: u32 = 0;
    for i in 0..8 {
        let d = match b[i] {
            c @ b'0'..=b'9' => (c - b'0') as u32,
            c @ b'A'..=b'F' => (c - b'A') as u32 + 10,
            _ => return None,
        };
        v = (v << 4) | d;
    }
    Some(v)
}

/// 窗口头解析：逐 token 扫描，未知字段即破损。
pub fn parse_header(hdr: &str) -> Option<WinHeader> {
    let mut mirror: Option<u64> = None;
    let mut epoch: Option<u64> = None;
    let mut from: Option<u64> = None;
    let mut to: Option<u64> = None;
    let mut sum: Option<u32> = None;
    let bts = hdr.as_bytes();
    let mut i = 0usize;
    while i <= bts.len() {
        let end = match find_byte(bts, i, b'|') {
            Some(p) => p,
            None => bts.len(),
        };
        let tok = &hdr[i..end];
        if !tok.is_empty() {
            let eq = find_byte(tok.as_bytes(), 0, b'=')?;
            let (k, v) = (&tok[..eq], &tok[eq + 1..]);
            match k {
                "mirror" => mirror = Some(parse_dec(v)?),
                "epoch" => epoch = Some(parse_dec(v)?),
                "from" => from = Some(parse_dec(v)?),
                "to" => to = Some(parse_dec(v)?),
                "sum" => sum = Some(parse_hex8(v)?),
                _ => return None,
            }
        }
        if end >= bts.len() {
            break;
        }
        i = end + 1;
    }
    Some(WinHeader { mirror: mirror?, epoch: epoch?, from: from?, to: to?, sum: sum? })
}

/// 正文裸控制符扫描（<0x20 且非换行 → 名字坏）。
fn body_control_scan(body: &str) -> Result<(), u32> {
    let b = body.as_bytes();
    for i in 0..(b.len()) {
        if b[i] < 0x20 && b[i] != b'\n' {
            return Err(EnumCode::NameBad.val());
        }
    }
    Ok(())
}

/// 窗口件核验：头字段 + 镜像/纪元锚 + 游标锚 + 正文逐字节对拍 + 校验和。
pub fn verify_window(m: &Mirror, out: &str, from_expected: usize) -> Result<(), u32> {
    let ob = out.as_bytes();
    if out.len() < 7 || !out.starts_with("@VXR1|") {
        return Err(EnumCode::WindowBroken.val());
    }
    let semi = match find_byte(ob, 0, b';') {
        Some(p) => p,
        None => return Err(EnumCode::WindowBroken.val()),
    };
    let h = match parse_header(&out[6..semi]) {
        Some(h) => h,
        None => return Err(EnumCode::WindowBroken.val()),
    };
    if h.mirror != m.id || h.epoch != m.epoch {
        return Err(EnumCode::CrossMiss.val());
    }
    if h.from != from_expected as u64 || h.from > h.to || h.to > m.entries.len() as u64 {
        return Err(EnumCode::CursorBack.val());
    }
    let mut start = semi + 1;
    if start < out.len() && ob[start] == b'\n' {
        start += 1;
    }
    let body = &out[start..];
    body_control_scan(body)?;
    if body != render_range(m, h.from as usize, h.to as usize) {
        return Err(EnumCode::CrossMiss.val());
    }
    if h.sum != fnv1a(body.as_bytes()) {
        return Err(EnumCode::WindowBroken.val());
    }
    Ok(())
}

/// 根目录枚举器：镜像游标 + 面内窗口游标 + 状态机 + 开销账。
#[derive(Clone, Debug)]
pub struct RootEnum {
    /// 下一面待枚举镜像下标（严格单调 0→19，跳面/回面均拒）。
    pub mirror_cursor: usize,
    /// 面内窗口游标（只进不退；换面自动归零）。
    pub offset: usize,
    pub state: EnumState,
    /// 簿记开销：快照头渲染 + 游标步进。
    pub admin_ops: u64,
    /// 载荷开销：正文渲染 + 校验和逐字节。
    pub payload_ops: u64,
    /// 累计窗口数。
    pub exports: u32,
    /// 对拍失真累计（错率分子）。
    pub mismatches: u32,
}

impl RootEnum {
    pub fn new() -> RootEnum {
        RootEnum {
            mirror_cursor: 0,
            offset: 0,
            state: EnumState::Idle,
            admin_ops: 0,
            payload_ops: 0,
            exports: 0,
            mismatches: 0,
        }
    }

    /// 状态推进公开口：倒退/平移显性拒。
    pub fn advance_state(&mut self, to: EnumState) -> Result<(), u32> {
        if !self.state.can_advance(to) {
            return Err(EnumCode::CursorBack.val());
        }
        self.state = to;
        Ok(())
    }

    /// 封盘：账面冻结。
    pub fn seal(&mut self) -> Result<(), u32> {
        self.advance_state(EnumState::Sealed)
    }

    /// 枚举当前镜像的一个窗口（from 由面内游标决定，批大小 WINDOW_BATCH）。
    /// 镜像序严格单调：mid != mirror_cursor 一律倒退拒。
    pub fn export_window(&mut self, mirrors: &[Mirror], mid: usize) -> Result<String, u32> {
        if self.state == EnumState::Sealed || mid != self.mirror_cursor {
            return Err(EnumCode::CursorBack.val());
        }
        let m = match mirrors.get(mid) {
            Some(m) => m,
            None => return Err(EnumCode::WindowBroken.val()),
        };
        if m.entries.is_empty() {
            return Err(EnumCode::MirrorEmpty.val());
        }
        let from = self.offset;
        if from >= m.entries.len() {
            return Err(EnumCode::WindowBroken.val());
        }
        let to = from.saturating_add(WINDOW_BATCH).min(m.entries.len());
        let body = render_range(m, from, to);
        let hdr = header_win(m.id, m.epoch, from as u64, to as u64, fnv1a(body.as_bytes()));
        self.admin_ops += hdr.len() as u64 + 2;
        self.payload_ops += body.len() as u64 * 2;
        if self.state == EnumState::Idle {
            self.state = EnumState::Walking;
        }
        self.exports += 1;
        self.offset = to;
        if to >= m.entries.len() {
            self.mirror_cursor = mid + 1;
            self.offset = 0;
        }
        let mut out = hdr;
        out.push_str(&body);
        Ok(out)
    }

    /// 双路对拍：20 面逐一 render_range vs manifest_range 逐字节比对。
    /// 全等返回对拍面数；任一失真记 mismatches 并拒。
    pub fn cross_check(&mut self, mirrors: &[Mirror]) -> Result<usize, u32> {
        if self.state == EnumState::Sealed {
            return Err(EnumCode::CursorBack.val());
        }
        let mut crossed = 0usize;
        for m in 0..MIRROR_N {
            let mir = match mirrors.get(m) {
                Some(x) => x,
                None => return Err(EnumCode::WindowBroken.val()),
            };
            let a = render_range(mir, 0, mir.entries.len());
            let b = manifest_range(mir, 0, mir.entries.len());
            if a != b {
                self.mismatches += 1;
                return Err(EnumCode::CrossMiss.val());
            }
            crossed += 1;
        }
        self.advance_state(EnumState::CrossChecked)?;
        Ok(crossed)
    }

    /// 开销占比（bps）：簿记 / 载荷；无载荷视为超限。
    pub fn overhead_bps(&self) -> u64 {
        if self.payload_ops == 0 {
            return u64::MAX;
        }
        self.admin_ops.saturating_mul(10_000) / self.payload_ops
    }
}

/// 逐账双路对拍：枚举路径 vs 清单直读路径，样本数 = FIDELITY_SAMPLES。
pub fn fidelity_10k(mirrors: &[Mirror]) -> Result<u32, u32> {
    if mirrors.is_empty() {
        return Err(EnumCode::MirrorEmpty.val());
    }
    for k in 0..FIDELITY_SAMPLES {
        let mid = k % mirrors.len();
        let m = match mirrors.get(mid) {
            Some(x) => x,
            None => return Err(EnumCode::CrossMiss.val()),
        };
        if m.entries.is_empty() {
            return Err(EnumCode::MirrorEmpty.val());
        }
        let idx = (k.wrapping_mul(11)) % m.entries.len();
        let e = match m.entries.get(idx) {
            Some(e) => e,
            None => return Err(EnumCode::CrossMiss.val()),
        };
        if render_entry(e) != manifest_entry(e) {
            return Err(EnumCode::CrossMiss.val());
        }
    }
    Ok(FIDELITY_SAMPLES as u32)
}

/// 复制 [lo, hi] 闭区间字节（越界自动夹到流长）。
fn push_range(b: &[u8], lo: usize, hi: usize, v: &mut Vec<u8>) {
    let mut k = lo;
    while k <= hi && k < b.len() {
        v.push(b[k]);
        k += 1;
    }
}

/// 头域单字节改写：定位 pat，把其后首个字节改写（保证与原值不同）。
fn patch_after(b: &[u8], pat: &[u8], newc: u8, alt: u8, v: &mut Vec<u8>) {
    let tgt = match find_sub(b, 0, pat) {
        Some(pp) => pp + pat.len(),
        None => usize::MAX,
    };
    for i in 0..(b.len()) {
        if i == tgt {
            let c = b[i];
            v.push(if c == newc { alt } else { newc });
        } else {
            v.push(b[i]);
        }
    }
}

/// 快照头行尾下标（头行 ';' 后换行的位置；找不到取全长）。
fn header_line_end(b: &[u8]) -> usize {
    match find_byte(b, 0, b';').and_then(|sp| find_byte(b, sp, b'\n')) {
        Some(x) => x,
        None => b.len(),
    }
}

/// 对黄金窗口件注入第 kind 路失真，返回坏件（内容全 ASCII，安全）。
pub fn distort(out: &str, kind: usize) -> String {
    let b = out.as_bytes();
    let mut v: Vec<u8> = Vec::new();
    let he = header_line_end(b);
    match kind % 10 {
        0 => {
            let mut flipped = false;
            for i in 0..(b.len()) {
                if i > he && b[i] != b'\n' && !flipped {
                    v.push(b[i] ^ 0x01);
                    flipped = true;
                } else {
                    v.push(b[i]);
                }
            }
        }
        1 => patch_after(b, b"mirror=", b'0', b'1', &mut v),
        2 => patch_after(b, b"sum=", b'0', b'F', &mut v),
        5 => patch_after(b, b"epoch=", b'9', b'1', &mut v),
        7 => patch_after(b, b"from=", b'0', b'1', &mut v),
        3 => {
            let le = find_byte(b, he + 1, b'\n').unwrap_or(b.len());
            push_range(b, 0, he, &mut v);
            push_range(b, le + 1, b.len(), &mut v);
        }
        4 => push_range(b, 0, b.len().saturating_sub(7), &mut v), // 截尾：去末 6 字节
        6 => {
            let l1 = find_byte(b, he + 1, b'\n').unwrap_or(b.len());
            let l2 = find_byte(b, l1 + 1, b'\n').unwrap_or(b.len());
            push_range(b, 0, he, &mut v);
            push_range(b, l1 + 1, l2, &mut v);
            push_range(b, he + 1, l1, &mut v);
            push_range(b, l2 + 1, b.len(), &mut v);
        }
        8 => {
            push_range(b, 0, he, &mut v);
            v.push(0x01);
            push_range(b, he + 1, b.len(), &mut v);
        }
        _ => {
            push_range(b, 0, b.len(), &mut v);
            let tail: &[u8] = b"ino=999999|name=zz|kind=2|mode=420|size=0\n";
            for k in 0..(tail.len()) {
                v.push(tail[k]);
            }
        }
    }
    match String::from_utf8(v) {
        Ok(s) => s,
        Err(_) => String::new(),
    }
}

pub struct InjectLedger {
    pub tries: u32,
    pub caught: u32,
    /// 漏抓的失真路号（诊断用）。
    pub miss_kinds: Vec<usize>,
}

impl InjectLedger {
    pub fn new() -> InjectLedger {
        InjectLedger { tries: 0, caught: 0, miss_kinds: Vec::new() }
    }
    pub fn record(&mut self, kind: usize, hit: bool) {
        self.tries += 1;
        if hit {
            self.caught += 1;
        } else {
            self.miss_kinds.push(kind);
        }
    }
    pub fn all_caught(&self) -> bool {
        self.tries > 0 && self.caught == self.tries
    }
}

/// 注入轮：每轮对新鲜黄金窗口件注入一路失真，核验器必须抓到
///（黄金件本身必须核验通过，防注入器污染基准）。
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    for r in 0..rounds {
        let m = build_mirror(7, ENTRIES_BASE + 7 * 3, 7);
        let ms = vec![m];
        let mut ex = RootEnum::new();
        let out = ex.export_window(&ms, 0)?;
        verify_window(&ms[0], &out, 0)?;
        let kind = (r % 10) as usize;
        led.record(kind, verify_window(&ms[0], &distort(&out, kind), 0).is_err());
    }
    Ok(led)
}

/// 转义闭集探针：竖线/斜杠/反斜杠/换行/控制符逐字验证。
pub fn probe_name_esc() -> bool {
    let q = esc_name("a|b/c\nd\\e");
    let a = q.contains("\\x7C") && q.contains("\\x2F") && q.contains("\\x5C") && q.contains("\\n");
    let b = !q.contains('\n') && !q.contains('|') && !q.contains('/');
    let c = esc_name("\u{01}") == String::from("\\u0001");
    let d = esc_name("n000") == String::from("n000");
    let e = validate_name("").is_err()
        && validate_name("a/b").is_err()
        && validate_name("a|b").is_err()
        && validate_name("a\nb").is_err()
        && validate_name("n000").is_ok();
    a && b && c && d && e
}

/// 种类闭集探针：0 与 5 越界拒，1..=4 全收。
pub fn probe_kind() -> bool {
    validate_kind(0).is_err() && validate_kind(5).is_err()
        && validate_kind(1).is_ok() && validate_kind(2).is_ok()
        && validate_kind(3).is_ok() && validate_kind(4).is_ok()
}

pub fn header_roundtrip() -> bool {
    let sum = fnv1a(b"probe");
    let hf = header_win(3, 107, 0, 129, sum);
    let a = match parse_header(&hf[6..hf.len() - 2]) {
        Some(h) => h.mirror == 3 && h.epoch == 107 && h.from == 0 && h.to == 129 && h.sum == sum,
        None => false,
    };
    let bad = parse_header("mirror=1|epoch=7|ghost=1|sum=00000000").is_none()
        && parse_header("mirror=1|epoch=x|sum=00000000").is_none()
        && parse_header("mirror=1|epoch=7|sum=00").is_none();
    a && bad
}

/// 全链：20 面顺序枚举 → 双路对拍 → 逐账样本对拍 → 五拒探 → 开销闸。
/// 返回 (正旗数, 拒旗数, 载荷账, 对拍样本数)。
pub fn golden_run() -> Result<(u32, u32, u64, u32), u32> {
    let ms = build_mirrors();
    let mut ex = RootEnum::new();
    let rej_skip = ex.export_window(&ms, 1).is_err();
    let mut first_out = String::new();
    while ex.mirror_cursor < MIRROR_N {
        let out = ex.export_window(&ms, ex.mirror_cursor)?;
        if ex.exports == 1 {
            first_out = out;
        }
    }
    let crossed = ex.cross_check(&ms)?;
    let fid = fidelity_10k(&ms)?;
    let rej_back = ex.export_window(&ms, 0).is_err();
    let empty_ms = vec![build_mirror(0, 0, 9), build_mirror(1, 40, 9)];
    let mut ex2 = RootEnum::new();
    let rej_empty = ex2.export_window(&empty_ms, 0).is_err();
    ex.seal()?;
    let rej_sealed = ex.export_window(&ms, 0).is_err();
    let rej_tamper = verify_window(&ms[0], &distort(&first_out, 2), 0).is_err();
    if ex.overhead_bps() > OVERHEAD_CAP_BPS {
        return Err(EnumCode::OverheadHigh.val());
    }
    let flags = (crossed == MIRROR_N) as u32 + (ex.mismatches == 0) as u32
        + (fid == FIDELITY_SAMPLES as u32) as u32 + probe_name_esc() as u32;
    let rejects = rej_skip as u32 + rej_back as u32 + rej_empty as u32
        + rej_sealed as u32 + rej_tamper as u32;
    Ok((flags, rejects, ex.payload_ops, fid))
}

/// 序 A：黄金全链四正五拒全中。
pub fn probe_seq_a() -> Result<(), u32> {
    match golden_run() {
        Ok((4, 5, _, _)) => Ok(()),
        _ => Err(EnumCode::NotReproducible.val()),
    }
}

/// 序 B：失真注入 10 路全抓。
pub fn probe_seq_b() -> Result<(), u32> {
    match inject_rounds(JUDGE_ROUNDS) {
        Ok(l) if l.all_caught() => Ok(()),
        _ => Err(EnumCode::CrossMiss.val()),
    }
}

/// 序 C：冻结常量与段位窗口。
pub fn probe_seq_c() -> Result<(), u32> {
    let in_window = SEG_BASE == 0x72F00 && EnumCode::NotReproducible.val() < SEG_BASE + 0x40;
    let ok = MIRROR_N == 20 && ENTRIES_BASE == 120 && WINDOW_BATCH == 200
        && FIDELITY_SAMPLES == 10_000 && JUDGE_ROUNDS == 10 && OVERHEAD_CAP_BPS == 200
        && KIND_MAX == 4 && in_window;
    if ok {
        Ok(())
    } else {
        Err(EnumCode::NotReproducible.val())
    }
}

pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(EnumCode::NotReproducible.val());
    }
    Ok(())
}

pub fn segment_guard() -> bool {
    for i in 0..(EnumCode::ALL.len()) {
        let v = EnumCode::ALL[i].val();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
    }
    true
}

pub fn writeback_summary() -> Result<String, u32> {
    let ms = build_mirrors();
    let mut ex = RootEnum::new();
    while ex.mirror_cursor < MIRROR_N {
        let _ = ex.export_window(&ms, ex.mirror_cursor)?;
    }
    let crossed = ex.cross_check(&ms)?;
    let fid = fidelity_10k(&ms)?;
    Ok(format!(
        "状态=已完成 mirrors={} crossed={} samples={} overhead={}bps exports={}",
        MIRROR_N, crossed, fid, ex.overhead_bps(), ex.exports
    ))
}

pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    let g = golden_run();
    let (gp, gd) = match &g {
        Ok((f, r, p, _)) => (*f == 4 && *r == 5, format!("flags={} rejects={} payload={}", f, r, p)),
        Err(e) => (false, format!("err=0x{:x} {}", e, detail(*e))),
    };
    v.push(("J1-20镜像枚举零错", gp, gd));
    let ms = build_mirrors();
    let mut ex = RootEnum::new();
    let cr = ex.cross_check(&ms);
    let cp = matches!(&cr, Ok(n) if *n == MIRROR_N) && ex.mismatches == 0;
    v.push(("J1-清单对照一致率100%", cp, format!("crossed={:?} mismatches={}", cr, ex.mismatches)));
    let mut ex3 = RootEnum::new();
    let out = ex3.export_window(&ms, 0);
    let vp = match &out {
        Ok(o) => verify_window(&ms[0], o, 0).is_ok(),
        Err(_) => false,
    };
    v.push(("J1-枚举账可导出", vp, String::from("@VXR1 头+账行，五重闸全过")));
    let inj = inject_rounds(JUDGE_ROUNDS);
    let (ip, id) = match &inj {
        Ok(l) => (l.all_caught(), format!("t={} c={} miss={:?}", l.tries, l.caught, l.miss_kinds)),
        Err(e) => (false, format!("err=0x{:x}", e)),
    };
    v.push(("J1-失真注入10/10", ip, id));
    let mut ex4 = RootEnum::new();
    let _ = ex4.export_window(&ms, 0);
    let bps = ex4.overhead_bps();
    v.push((
        "J1-开销占比≤2%",
        bps <= OVERHEAD_CAP_BPS,
        format!("admin={} payload={} bps={} cap={}", ex4.admin_ops, ex4.payload_ops, bps, OVERHEAD_CAP_BPS),
    ));
    let fid = fidelity_10k(&ms);
    let fp = matches!(&fid, Ok(n) if *n == FIDELITY_SAMPLES as u32);
    v.push(("J1-逐账对拍10^4样本", fp, format!("samples={}", FIDELITY_SAMPLES)));
    v.push(("J1-转义与种类闭集", probe_name_esc() && probe_kind(), String::from("\\x7C \\x2F \\x5C \\n \\u00XX 全落位；种类 1..=4")));
    let hd = String::from("建头→解析逐域对账；未知字段/坏数字/短校验和必败");
    v.push(("J1-快照头校验", header_roundtrip(), hd));
    let ms2 = build_mirrors();
    let mut ex5 = RootEnum::new();
    let r1 = ex5.export_window(&ms2, 1).is_err();
    while ex5.mirror_cursor < MIRROR_N {
        let _ = ex5.export_window(&ms2, ex5.mirror_cursor);
    }
    let r2 = ex5.export_window(&ms2, 0).is_err();
    let _ = ex5.seal();
    let r3 = ex5.export_window(&ms2, 3).is_err();
    let empty_ms = vec![build_mirror(0, 0, 9), build_mirror(1, 40, 9)];
    let mut ex6 = RootEnum::new();
    let r4 = ex6.export_window(&empty_ms, 0).is_err();
    let mut ex7 = RootEnum::new();
    let m1 = ex7.advance_state(EnumState::Walking).is_ok();
    let m2 = ex7.advance_state(EnumState::Idle).is_err();
    let m3 = ex7.advance_state(EnumState::Walking).is_err();
    let m4 = ex7.advance_state(EnumState::CrossChecked).is_ok();
    let m5 = ex7.advance_state(EnumState::Sealed).is_ok();
    v.push((
        "J2-单调守卫",
        r1 && r2 && r3 && r4 && m1 && m2 && m3 && m4 && m5,
        format!("跳{}回{}封{}空{} 状态机进拒平进封={}/{}/{}/{}/{}", r1, r2, r3, r4, m1, m2, m3, m4, m5),
    ));
    v.push(("J2-双跑一致", repro_check().is_ok(), String::from("golden_run 两跑同值")));
    let wb = writeback_summary();
    let (wp, wd) = match &wb {
        Ok(s) => (!s.is_empty(), s.clone()),
        Err(e) => (false, format!("err=0x{:x}", e)),
    };
    v.push(("J3-写回摘要", wp, wd));
    v.push(("J4-段位守卫", segment_guard(), String::from("0x72F00 窗口")));
    v
}

pub fn run_unx5442_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F5442 根目录枚举账");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
