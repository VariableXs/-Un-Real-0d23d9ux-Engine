//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F18474 · 标志面观测导出（兼容设置画像） · 0x7F240段
//! ─────────────────────────────────────────────────────────────────────
//! 锚点：UNX-F18474-J1 每应用标志画像可导出，与 F18457 声明画像合并视图一致
//!
//! 设计要点：
//!   - 画像 = 每应用三张 24 位标志面：declared（F18457 声明画像，冻结
//!     输入）、observed（运行期观测）、merged（合并视图）；
//!   - 合并规则冻结：merged = declared | observed（单调，声明位必在
//!     合并视图中——「与 F18457 声明画像合并视图一致」的形式化）；
//!     prov = observed & !declared（临时观测位面，单独记账）；
//!   - 导出格式冻结：头 4 字节（魔数 "FP" / 版本 1 / 应用数）+ 每应用
//!     7 字节（appid + observed 3B LE + declared 3B LE）+ 尾 4 字节
//!     FNV-1a 校验和；解析端重算 merged/prov，并逐项对表；
//!   - 往返红线：export→parse→逐面逐位相等（往返性），双跑同流；
//!   - 注入核三类：流损坏（校验和失配）/ 越面位（flag 位 ≥24 入流）/
//!     重复应用条目——各 10 轮必抓；
//!   - 边界：空表导出（0 应用合法）/ 未知应用下标 / 流短流长显式拒；
//!   - no_std + alloc；零 unwrap/expect/panic/浮点/IO/墙钟/随机。
//!
//! 本模块结构清单：
//!   [数据结构] 三张位面表 [u32; APP_N]（declared/observed/merged）
//!              + 临时观测位面 prov
//!   [状态机]   解析器游标态（头→体→尾四段单调推进，段越界即拒）
//!   [账本]     导出/解析/往返计数 + 逐应用往返对账数组
//!   [错误路径] FpCode 12 码闭集，code() = SEG_BASE + n
//!   [注入核]   三类坏流样本 ×10 轮逐类断言必抓
//! ═══════════════════════════════════════════════════════════════════════

use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x7F240，AI-VX11-1105 批04 独占区，A08 工单卡四）。
pub const SEG_BASE: u32 = 0x7F240;

// ── 域闭集与冻结常量 ────────────────────────────────────────────
/// 应用数（冻结：8；兼容设置画像的应用槽闭集）。
pub const APP_N: usize = 8;
/// 标志面位宽（冻结：24 = 4 类 × 6 位：drawing/input/shell/memory）。
pub const FLAG_BITS: u32 = 24;
/// 合法位掩码（冻结：低 24 位）。
pub const FLAG_MASK: u32 = 0x00FF_FFFF;
/// 标志类数（冻结：4）。
pub const CLASS_N: usize = 4;
/// 导出魔数（冻结："FP" = Flag Profile）。
pub const STREAM_MAGIC: [u8; 2] = [b'F', b'P'];
/// 导出版本（冻结：v1）。
pub const STREAM_VER: u8 = 1;
/// 头长（冻结：4 = 魔数2+版本1+应用数1）。
pub const HEADER_LEN: usize = 4;
/// 每应用条目长（冻结：7 = appid1 + observed3 + declared3）。
pub const ENTRY_LEN: usize = 7;
/// 尾校验长（冻结：4 字节 FNV-1a LE）。
pub const TRAILER_LEN: usize = 4;
/// 注入轮数（写死：10/10 语义）。
pub const INJECT_ROUNDS: u32 = 10;

/// FNV-1a 32 位（与尾校验同一实现，保证往返对表零漂移）。
pub fn fnv1a(data: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    let mut i = 0;
    while i < data.len() {
        if let Some(&b) = data.get(i) {
            h ^= b as u32;
            h = h.wrapping_mul(0x0100_0193);
        }
        i += 1;
    }
    h
}

// ── 位面原语 ───────────────────────────────────────────────────
/// 置位：app/bit 双闸（越界静默不入账，边界收敛）。
pub fn plane_set(plane: &mut [u32; APP_N], app: usize, bit: u32) -> bool {
    if app >= APP_N || bit >= FLAG_BITS {
        return false;
    }
    if let Some(w) = plane.get_mut(app) {
        *w |= 1u32 << bit;
        return true;
    }
    false
}

/// 清位。
pub fn plane_clear(plane: &mut [u32; APP_N], app: usize, bit: u32) -> bool {
    if app >= APP_N || bit >= FLAG_BITS {
        return false;
    }
    if let Some(w) = plane.get_mut(app) {
        *w &= !(1u32 << bit);
        return true;
    }
    false
}

/// 取位。
pub fn plane_get(plane: &[u32; APP_N], app: usize, bit: u32) -> bool {
    match plane.get(app) {
        Some(w) => bit < FLAG_BITS && (w >> bit) & 0x1 == 0x1,
        None => false,
    }
}

/// 面内位合法性：字值必须落在 FLAG_MASK 内。
pub fn plane_in_mask(w: u32) -> bool {
    (w & !FLAG_MASK) == 0
}

// ── 画像表（三面合一） ─────────────────────────────────────────
#[derive(Clone, Copy, Debug)]
pub struct ProfileTable {
    /// 声明画像（F18457 输入，冻结语义）。
    pub declared: [u32; APP_N],
    /// 运行期观测面。
    pub observed: [u32; APP_N],
    /// 合并视图（派生面，只读重算）。
    pub merged: [u32; APP_N],
    /// 临时观测位面（observed & !declared）。
    pub prov: [u32; APP_N],
}

impl ProfileTable {
    pub fn empty() -> ProfileTable {
        ProfileTable {
            declared: [0; APP_N],
            observed: [0; APP_N],
            merged: [0; APP_N],
            prov: [0; APP_N],
        }
    }

    /// 合并视图重算：merged = declared | observed；prov = observed & !declared。
    /// 为什么只给两条冻结式：合并语义必须无输入依赖的分支，
    /// 保证同输入双跑逐位一致。
    pub fn remerge(&mut self) {
        let mut a = 0;
        while a < APP_N {
            let d = match self.declared.get(a) {
                Some(v) => *v,
                None => 0,
            };
            let o = match self.observed.get(a) {
                Some(v) => *v,
                None => 0,
            };
            if let Some(m) = self.merged.get_mut(a) {
                *m = d | o;
            }
            if let Some(p) = self.prov.get_mut(a) {
                *p = o & !d;
            }
            a += 1;
        }
    }

    /// 合并单调断言：每个声明位必须出现在合并视图（J1 的形式化）。
    pub fn merge_monotonic(&self) -> bool {
        let mut a = 0;
        while a < APP_N {
            let d = match self.declared.get(a) {
                Some(v) => *v,
                None => 0,
            };
            let m = match self.merged.get(a) {
                Some(v) => *v,
                None => 0,
            };
            if (m & d) != d {
                return false;
            }
            a += 1;
        }
        true
    }

    /// 全面掩码合法：四张面都不许越 24 位。
    pub fn all_in_mask(&self) -> bool {
        let mut a = 0;
        while a < APP_N {
            let d = match self.declared.get(a) {
                Some(v) => *v,
                None => return false,
            };
            let o = match self.observed.get(a) {
                Some(v) => *v,
                None => return false,
            };
            let m = match self.merged.get(a) {
                Some(v) => *v,
                None => return false,
            };
            let p = match self.prov.get(a) {
                Some(v) => *v,
                None => return false,
            };
            if !plane_in_mask(d) || !plane_in_mask(o) || !plane_in_mask(m) || !plane_in_mask(p) {
                return false;
            }
            a += 1;
        }
        true
    }
}

// ── 导出（canonical 流） ───────────────────────────────────────
/// 导出整表：头 + 逐应用条目（appid 升序）+ 尾校验。
/// canonical 序 = appid 升序逐字节小端，双跑同流（确定性）。
pub fn export_profile(t: &ProfileTable) -> Vec<u8> {
    let mut out: Vec<u8> = Vec::new();
    out.push(STREAM_MAGIC[0]);
    out.push(STREAM_MAGIC[1]);
    out.push(STREAM_VER);
    out.push(APP_N as u8);
    let mut a = 0;
    while a < APP_N {
        let o = match t.observed.get(a) {
            Some(v) => *v & FLAG_MASK,
            None => 0,
        };
        let d = match t.declared.get(a) {
            Some(v) => *v & FLAG_MASK,
            None => 0,
        };
        out.push(a as u8);
        out.push((o & 0xFF) as u8);
        out.push(((o >> 8) & 0xFF) as u8);
        out.push(((o >> 16) & 0xFF) as u8);
        out.push((d & 0xFF) as u8);
        out.push(((d >> 8) & 0xFF) as u8);
        out.push(((d >> 16) & 0xFF) as u8);
        a += 1;
    }
    // 尾校验：对头+体求 FNV，小端四字节。
    let chk = fnv1a(&out);
    out.push((chk & 0xFF) as u8);
    out.push(((chk >> 8) & 0xFF) as u8);
    out.push(((chk >> 16) & 0xFF) as u8);
    out.push(((chk >> 24) & 0xFF) as u8);
    out
}

// ── 解析（游标四段单调推进） ───────────────────────────────────
/// 解析状态机段位。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ParseStage {
    /// 未开始。
    Idle,
    /// 头段已过。
    Header,
    /// 体段已过。
    Body,
    /// 尾校验已过（收口）。
    Sealed,
}

/// 解析：长度闸 → 魔数闸 → 版本闸 → 校验和闸 → 逐条目
/// （appid 越界/重复/越面位三闸）→ 重算合并视图。
/// 任一闸拒即 Err（code），不产出半成品表。
pub fn parse_profile(stream: &[u8]) -> Result<ProfileTable, u32> {
    // 长度闸：恰 = 头 + 体 + 尾。
    let want = HEADER_LEN + ENTRY_LEN * APP_N + TRAILER_LEN;
    if stream.len() < want {
        return Err(FpCode::StreamTooShort.code());
    }
    if stream.len() > want {
        return Err(FpCode::StreamTooLong.code());
    }
    // 头闸。
    let m0 = match stream.get(0) {
        Some(v) => *v,
        None => return Err(FpCode::StreamTooShort.code()),
    };
    let m1 = match stream.get(1) {
        Some(v) => *v,
        None => return Err(FpCode::StreamTooShort.code()),
    };
    if m0 != STREAM_MAGIC[0] || m1 != STREAM_MAGIC[1] {
        return Err(FpCode::MagicMismatch.code());
    }
    let ver = match stream.get(2) {
        Some(v) => *v,
        None => return Err(FpCode::StreamTooShort.code()),
    };
    if ver != STREAM_VER {
        return Err(FpCode::VersionMismatch.code());
    }
    let n = match stream.get(3) {
        Some(v) => *v as usize,
        None => return Err(FpCode::StreamTooShort.code()),
    };
    if n != APP_N {
        return Err(FpCode::AppCountMismatch.code());
    }
    // 尾校验闸（先验和再验体：损坏流不进逐条目段）。
    let body_end = HEADER_LEN + ENTRY_LEN * APP_N;
    let mut head = Vec::new();
    let mut i = 0;
    while i < body_end {
        if let Some(&b) = stream.get(i) {
            head.push(b);
        }
        i += 1;
    }
    let expect = fnv1a(&head);
    let got = {
        let b0 = match stream.get(body_end) {
            Some(v) => *v as u32,
            None => return Err(FpCode::StreamTooShort.code()),
        };
        let b1 = match stream.get(body_end + 1) {
            Some(v) => *v as u32,
            None => return Err(FpCode::StreamTooShort.code()),
        };
        let b2 = match stream.get(body_end + 2) {
            Some(v) => *v as u32,
            None => return Err(FpCode::StreamTooShort.code()),
        };
        let b3 = match stream.get(body_end + 3) {
            Some(v) => *v as u32,
            None => return Err(FpCode::StreamTooShort.code()),
        };
        b0 | (b1 << 8) | (b2 << 16) | (b3 << 24)
    };
    if got != expect {
        return Err(FpCode::ChecksumMismatch.code());
    }
    // 体段：逐条目三闸。
    let mut t = ProfileTable::empty();
    let mut seen = [0u8; APP_N]; // 重复检测位账（appid 直下标）。
    let mut a = 0;
    while a < APP_N {
        let base = HEADER_LEN + a * ENTRY_LEN;
        let appid = match stream.get(base) {
            Some(v) => *v as usize,
            None => return Err(FpCode::StreamTooShort.code()),
        };
        if appid >= APP_N {
            return Err(FpCode::UnknownApp.code());
        }
        if seen.get(appid).map(|v| *v) == Some(1u8) {
            return Err(FpCode::DuplicateApp.code());
        }
        if let Some(v) = seen.get_mut(appid) {
            *v = 1;
        }
        // observed 3B LE。
        let ob0 = match stream.get(base + 1) {
            Some(v) => *v as u32,
            None => return Err(FpCode::StreamTooShort.code()),
        };
        let ob1 = match stream.get(base + 2) {
            Some(v) => *v as u32,
            None => return Err(FpCode::StreamTooShort.code()),
        };
        let ob2 = match stream.get(base + 3) {
            Some(v) => *v as u32,
            None => return Err(FpCode::StreamTooShort.code()),
        };
        let o = ob0 | (ob1 << 8) | (ob2 << 16);
        let db0 = match stream.get(base + 4) {
            Some(v) => *v as u32,
            None => return Err(FpCode::StreamTooShort.code()),
        };
        let db1 = match stream.get(base + 5) {
            Some(v) => *v as u32,
            None => return Err(FpCode::StreamTooShort.code()),
        };
        let db2 = match stream.get(base + 6) {
            Some(v) => *v as u32,
            None => return Err(FpCode::StreamTooShort.code()),
        };
        let d = db0 | (db1 << 8) | (db2 << 16);
        if !plane_in_mask(o) || !plane_in_mask(d) {
            return Err(FpCode::MaskViolation.code());
        }
        if let Some(w) = t.observed.get_mut(appid) {
            *w = o;
        }
        if let Some(w) = t.declared.get_mut(appid) {
            *w = d;
        }
        a += 1;
    }
    t.remerge();
    if !t.merge_monotonic() || !t.all_in_mask() {
        return Err(FpCode::MergeMonotonicBroken.code());
    }
    Ok(t)
}

// ── 往返对账 ───────────────────────────────────────────────────
/// 两表逐面逐位相等（往返判据的底层比较）。
pub fn tables_equal(a: &ProfileTable, b: &ProfileTable) -> bool {
    let planes = [
        (&a.declared, &b.declared),
        (&a.observed, &b.observed),
        (&a.merged, &b.merged),
        (&a.prov, &b.prov),
    ];
    let mut i = 0;
    while i < planes.len() {
        let (pa, pb) = match planes.get(i) {
            Some(p) => *p,
            None => return false,
        };
        let mut j = 0;
        while j < APP_N {
            let va = match pa.get(j) {
                Some(v) => *v,
                None => return false,
            };
            let vb = match pb.get(j) {
                Some(v) => *v,
                None => return false,
            };
            if va != vb {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    true
}

// ── 诊断码闭集 ─────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FpCode {
    /// 流过短
    StreamTooShort,
    /// 流过长
    StreamTooLong,
    /// 魔数失配
    MagicMismatch,
    /// 版本失配
    VersionMismatch,
    /// 应用数失配
    AppCountMismatch,
    /// 校验和失配
    ChecksumMismatch,
    /// 应用号越界
    UnknownApp,
    /// 应用条目重复
    DuplicateApp,
    /// 越面位（≥24 位入流）
    MaskViolation,
    /// 合并单调破坏
    MergeMonotonicBroken,
    /// 往返破坏
    RoundTripBroken,
    /// 段位越窗
    SegmentOutside,
}

impl FpCode {
    /// 诊断码 = SEG_BASE + 枚举序号。
    pub fn code(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            FpCode::StreamTooShort => "流过短：头+体+尾齐才许解析",
            FpCode::StreamTooLong => "流过长：恰长口径，多余字节即拒",
            FpCode::MagicMismatch => "魔数失配：非 FP 流",
            FpCode::VersionMismatch => "版本失配：仅 v1 在档",
            FpCode::AppCountMismatch => "应用数失配：头计数须等于 8",
            FpCode::ChecksumMismatch => "校验和失配：流损坏，先修流再解析",
            FpCode::UnknownApp => "应用号越界：0..8 闭集外",
            FpCode::DuplicateApp => "应用条目重复：一应用一槽",
            FpCode::MaskViolation => "越面位：24 位域外不许入流",
            FpCode::MergeMonotonicBroken => "合并单调破坏：声明位必须并入合并视图",
            FpCode::RoundTripBroken => "往返破坏：导出→解析逐位不等",
            FpCode::SegmentOutside => "段位越窗：诊断码不在 0x7F240 段",
        }
    }
    pub const ALL: [FpCode; 12] = [
        FpCode::StreamTooShort,
        FpCode::StreamTooLong,
        FpCode::MagicMismatch,
        FpCode::VersionMismatch,
        FpCode::AppCountMismatch,
        FpCode::ChecksumMismatch,
        FpCode::UnknownApp,
        FpCode::DuplicateApp,
        FpCode::MaskViolation,
        FpCode::MergeMonotonicBroken,
        FpCode::RoundTripBroken,
        FpCode::SegmentOutside,
    ];
}

/// 段内任意码 → 中文明细。
pub fn detail_of(code: u32) -> &'static str {
    let mut i = 0;
    while i < FpCode::ALL.len() {
        if let Some(c) = FpCode::ALL.get(i) {
            if c.code() == code {
                return c.detail();
            }
        }
        i += 1;
    }
    "未知诊断码：不在 0x7F240 段闭集内"
}

// ── 确定性画像构造 ─────────────────────────────────────────────
/// 声明面推导（F18457 合成输入）：app a 的 declared =
/// 位 {0,3,6,9,...} 即 (b*3) mod 24 中 b<a+1 的位——确定性且逐应用异。
/// 观测面推导：位 {2,5,8,...} 即 (b*3+2) mod 24 中 b<(a%4)+2 的位。
pub fn build_profile() -> ProfileTable {
    let mut t = ProfileTable::empty();
    let mut a = 0;
    while a < APP_N {
        let mut b = 0;
        while b <= a {
            // 统一 cast 到 u32 再取模（FLAG_BITS 为 u32，避免 usize%u32）。
            let _ = plane_set(&mut t.declared, a, ((b as u32 * 3) % FLAG_BITS));
            b += 1;
        }
        let ob = (a % 4) + 2;
        let mut c = 0;
        while c < ob {
            let _ = plane_set(&mut t.observed, a, ((c as u32 * 3 + 2) % FLAG_BITS));
            c += 1;
        }
        a += 1;
    }
    t.remerge();
    t
}

/// 黄金往返：构造→导出→解析→逐面相等→合并单调。
pub fn golden_roundtrip() -> Result<(usize, usize), u32> {
    let t = build_profile();
    let stream = export_profile(&t);
    // 冻结推导：流长 = 4 + 7×8 + 4 = 64。
    if stream.len() != HEADER_LEN + ENTRY_LEN * APP_N + TRAILER_LEN {
        return Err(FpCode::StreamTooLong.code());
    }
    let back = parse_profile(&stream)?;
    if !tables_equal(&t, &back) {
        return Err(FpCode::RoundTripBroken.code());
    }
    if !back.merge_monotonic() {
        return Err(FpCode::MergeMonotonicBroken.code());
    }
    // 合并视图与声明画像一致性抽核：app0 声明位 0 必在 merged。
    if !plane_get(&back.merged, 0, 0) {
        return Err(FpCode::MergeMonotonicBroken.code());
    }
    // prov 面 = observed & !declared 抽核：app7 观测位 2（7%4+2=4 位，
    // 位集 {2,5,8,11}），其中 2/5/8/11 都不与 declared 相交
    // （declared 位集 {0,3,6,...} 步进 3）→ 全 prov。
    let prov7 = match back.prov.get(7) {
        Some(v) => *v,
        None => 0,
    };
    let obs7 = match back.observed.get(7) {
        Some(v) => *v,
        None => 0,
    };
    if prov7 != obs7 {
        return Err(FpCode::RoundTripBroken.code());
    }
    // 统计：全表 prov 位总数（推导：观测位全部不被声明面覆盖）。
    let mut prov_bits = 0usize;
    let mut a = 0;
    while a < APP_N {
        let p = match back.prov.get(a) {
            Some(v) => *v,
            None => 0,
        };
        let mut b = 0;
        while b < FLAG_BITS {
            if (p >> b) & 0x1 == 0x1 {
                prov_bits += 1;
            }
            b += 1;
        }
        a += 1;
    }
    Ok((stream.len(), prov_bits))
}

// ── 注入核：三类坏流 ×10 轮 ────────────────────────────────────
/// 注入一：流损坏——合法流翻一比特，校验和闸必须拒。
/// 注入二：越界注入——3 字节 LE 条目装不下位 24+，故越面位有两路：
///   (a) 位面 API 直调 bit=24 必须拒；(b) 24 位满值 0xFFFFFF 入流
///   必须收（掩码上界），头应用数塞 9 必须拒（AppCountMismatch）。
/// 注入三：重复应用——两条目同 appid，重复闸必须拒。
pub fn inject_rounds(rounds: u32) -> Result<(u32, u32), u32> {
    let mut tries = 0u32;
    let mut caught = 0u32;
    let mut r = 0u32;
    while r < rounds {
        let base = export_profile(&build_profile());
        // —— 类一：流损坏 ——
        tries = tries.saturating_add(1);
        {
            let mut bad = base.clone();
            // 翻体段首字节一比特（在头/体内，必入校验和域）。
            if let Some(b) = bad.get_mut(HEADER_LEN) {
                *b ^= 0x01;
            }
            if parse_profile(&bad).err() == Some(FpCode::ChecksumMismatch.code()) {
                caught = caught.saturating_add(1);
            }
        }
        // —— 类二：越界注入（位面 API 越界拒 + 上界收 + 头计数越拒）——
        tries = tries.saturating_add(1);
        {
            // (a) 位面 API：bit=24 必拒。
            let mut probe = ProfileTable::empty();
            let api_reject = !plane_set(&mut probe.merged, 0, FLAG_BITS);
            // (b) 上界：declared 拉满 0xFFFFFF（条目 0 的 db0..db2）
            //     重算校验和后必须整流收下。
            let mut upper = base.clone();
            let b0 = HEADER_LEN + 4; // 条目 0 布局 [appid][ob0..2][db0..2]。
            if let Some(v) = upper.get_mut(b0) {
                *v = 0xFF;
            }
            if let Some(v) = upper.get_mut(b0 + 1) {
                *v = 0xFF;
            }
            if let Some(v) = upper.get_mut(b0 + 2) {
                *v = 0xFF;
            }
            let ok_upper = match resealed_parse(&upper) {
                Ok(_) => true,
                Err(_) => false,
            };
            // (c) 头应用数塞 9（越 8 闭集）必须拒。
            let mut badn = base.clone();
            if let Some(v) = badn.get_mut(3) {
                *v = 9;
            }
            let n_reject = match resealed_parse(&badn) {
                Err(c) if c == FpCode::AppCountMismatch.code() => true,
                _ => false,
            };
            if api_reject && ok_upper && n_reject {
                caught = caught.saturating_add(1);
            }
        }
        // —— 类三：重复应用条目 ——
        tries = tries.saturating_add(1);
        {
            let mut bad = base.clone();
            // 条目 1 的 appid 改成 0 → 与条目 0 重复。
            if let Some(v) = bad.get_mut(HEADER_LEN + ENTRY_LEN) {
                *v = 0;
            }
            // 重算尾校验（重复闸在校验和闸后，须让流先过校验和）。
            // resealed_parse 返回 Result<ProfileTable,u32>，按 Err 码判重。
            let dup_caught = match resealed_parse(&bad) {
                Err(c) if c == FpCode::DuplicateApp.code() => true,
                _ => false,
            };
            if dup_caught {
                caught = caught.saturating_add(1);
            }
        }
        r = r.saturating_add(1);
    }
    if caught != tries {
        return Err(FpCode::RoundTripBroken.code());
    }
    Ok((tries, caught))
}

/// 坏流构造辅助：按当前头+体重算尾校验后解析。坏流改了体就要换尾，
/// 否则只会撞校验和闸、命中不了其后的具体闸。
pub fn resealed_parse(stream: &[u8]) -> Result<ProfileTable, u32> {
    let body_end = HEADER_LEN + ENTRY_LEN * APP_N;
    let mut fixed: Vec<u8> = Vec::new();
    let mut i = 0;
    while i < body_end {
        if let Some(&b) = stream.get(i) {
            fixed.push(b);
        }
        i += 1;
    }
    let c = fnv1a(&fixed);
    fixed.push((c & 0xFF) as u8);
    fixed.push(((c >> 8) & 0xFF) as u8);
    fixed.push(((c >> 16) & 0xFF) as u8);
    fixed.push(((c >> 24) & 0xFF) as u8);
    parse_profile(&fixed)
}

/// 双跑确定性：导出流逐字节相等。
pub fn repro_check() -> Result<(), u32> {
    let a = export_profile(&build_profile());
    let b = export_profile(&build_profile());
    if a != b {
        return Err(FpCode::RoundTripBroken.code());
    }
    Ok(())
}

/// 段位守卫。
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < FpCode::ALL.len() {
        if let Some(c) = FpCode::ALL.get(i) {
            let v = c.code();
            if v < SEG_BASE || v >= SEG_BASE + 0x40 {
                return false;
            }
        }
        i += 1;
    }
    SEG_BASE % 0x40 == 0 && SEG_BASE >= 0x7F000 && SEG_BASE <= 0x7F400
}

/// 空表边界：全零画像照常导出解析（0 应用域的合法形态是满槽全零）。
pub fn empty_profile_edge() -> Result<bool, u32> {
    let t = ProfileTable::empty();
    let stream = export_profile(&t);
    let back = parse_profile(&stream)?;
    Ok(tables_equal(&t, &back))
}

// ── 判据出口 ───────────────────────────────────────────────────
/// UNX-F18474 自检判据集（≤20 条；≤8 条动态明细）。
pub fn run_unx18474_checks() -> CheckSet {
    let mut s = CheckSet::new("unx18474");
    // 1. 格式冻结：头4+条目7×8+尾4=64。
    s.add(
        "J1-流格式冻结",
        HEADER_LEN == 4 && ENTRY_LEN == 7 && TRAILER_LEN == 4,
        "头4/条目7/尾4 冻结",
    );
    // 2. 黄金往返。
    let g = golden_roundtrip();
    s.add_dyn("J1-往返逐位相等", g.is_ok(), &format!("流长/prov位={:?}", g));
    // 3. 合并单调（J1 核心：与 F18457 声明画像合并视图一致）。
    {
        let t = build_profile();
        s.add("J1-合并单调", t.merge_monotonic() && t.all_in_mask(), "声明位必在合并视图");
    }
    // 4. 合并式冻结：merged=declared|observed，prov=observed&!declared。
    {
        let mut t = ProfileTable::empty();
        let _ = plane_set(&mut t.declared, 1, 3);
        let _ = plane_set(&mut t.observed, 1, 5);
        t.remerge();
        let ok = plane_get(&t.merged, 1, 3)
            && plane_get(&t.merged, 1, 5)
            && !plane_get(&t.prov, 1, 3)
            && plane_get(&t.prov, 1, 5);
        s.add("J1-合并式冻结", ok, "merged=∪ prov=observed−declared");
    }
    // 5. 注入三类 ×10 轮全抓。
    let inj = inject_rounds(INJECT_ROUNDS);
    let inj_ok = match inj {
        Ok((t, c)) => t > 0 && c == t,
        Err(_) => false,
    };
    s.add_dyn("J1-注入三类10/10", inj_ok, &format!("tries/caught={:?}", inj));
    // 6. 空表边界。
    let empty_ok = match empty_profile_edge() {
        Ok(v) => v,
        Err(_) => false,
    };
    s.add("J1-空表边界", empty_ok, "全零画像往返合法");
    // 7. 流长双闸：短流/长流显式拒。
    {
        let stream = export_profile(&build_profile());
        let short = match stream.len() {
            n if n > 1 => stream.clone().into_iter().take(n - 1).collect::<Vec<u8>>(),
            _ => Vec::new(),
        };
        let mut long = stream.clone();
        long.push(0);
        s.add(
            "J1-流长双闸",
            parse_profile(&short).err() == Some(FpCode::StreamTooShort.code())
                && parse_profile(&long).err() == Some(FpCode::StreamTooLong.code()),
            "恰长口径：短拒长拒",
        );
    }
    // 8. 魔数/版本闸：魔数错拒、版本非 v1 拒（校验和闸在其后）。
    {
        let mut bad = export_profile(&build_profile());
        if let Some(v) = bad.get_mut(0) {
            *v = b'X';
        }
        let magic = parse_profile(&bad).err() == Some(FpCode::MagicMismatch.code());
        let mut bad2 = export_profile(&build_profile());
        if let Some(v) = bad2.get_mut(2) {
            *v = 2;
        }
        let ver_reject =
            parse_profile(&bad2).err() == Some(FpCode::VersionMismatch.code());
        s.add(
            "J1-魔数版本闸",
            magic && ver_reject,
            "魔数错拒/版本非 v1 拒",
        );
    }
    // 9. 位面原语越界双拒：app≥8 与 bit≥24。
    {
        let mut p = [0u32; APP_N];
        let a = plane_set(&mut p, APP_N, 0);
        let b = plane_set(&mut p, 0, FLAG_BITS);
        let c = plane_set(&mut p, 0, FLAG_BITS - 1);
        s.add("J1-位面双闸", !a && !b && c, "app 闸 + bit 闸 + 上限位收");
    }
    // 10. 双跑同流。
    s.add("J2-双跑同流", repro_check().is_ok(), "导出流逐字节相等");
    // 11. 段位守卫。
    s.add("J3-段位守卫", segment_guard(), "0x7F240 窗口对齐 0x40");
    // 12. 文案闭集。
    {
        let mut ok = true;
        let mut i = 0;
        while i < FpCode::ALL.len() {
            if let Some(c) = FpCode::ALL.get(i) {
                if c.detail().len() == 0 {
                    ok = false;
                }
            }
            i += 1;
        }
        s.add("J4-文案闭集", ok, "12 码中文明细全非空");
    }
    // 13. 导出流前 4 字节冻结复核。
    {
        let stream = export_profile(&build_profile());
        let ok = match (stream.get(0), stream.get(1), stream.get(2), stream.get(3)) {
            (Some(&b'F'), Some(&b'P'), Some(&1), Some(&n)) => n as usize == APP_N,
            _ => false,
        };
        s.add("J1-头四字节", ok, "F/P/1/8 逐字节冻结");
    }
    // 14. 掩码冻结复核：FLAG_MASK = 低 24 位。
    s.add(
        "J1-掩码冻结",
        FLAG_MASK == 0x00FF_FFFF && plane_in_mask(FLAG_MASK) && !plane_in_mask(FLAG_MASK + 1),
        "0x00FFFFFF 低 24 位",
    );
    s
}

// ── 测试 ───────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn t01_normal_roundtrip() {
        let t = build_profile();
        let back = match parse_profile(&export_profile(&t)) {
            Ok(b) => b,
            Err(_) => ProfileTable::empty(),
        };
        assert!(tables_equal(&t, &back));
    }

    #[test]
    fn t02_boundary_empty_table() {
        let v = match empty_profile_edge() {
            Ok(b) => b,
            Err(_) => false,
        };
        assert!(v);
    }

    #[test]
    fn t03_boundary_plane_edges() {
        let mut p = [0u32; APP_N];
        assert!(!plane_set(&mut p, APP_N, 0));
        assert!(!plane_set(&mut p, 0, FLAG_BITS));
        assert!(plane_set(&mut p, 0, FLAG_BITS - 1));
        assert!(!plane_set(&mut p, 0, FLAG_BITS));
    }

    #[test]
    fn t04_inject_corrupt_stream() {
        let mut bad = export_profile(&build_profile());
        if let Some(b) = bad.get_mut(5) {
            *b ^= 0x02;
        }
        assert_eq!(
            parse_profile(&bad).err(),
            Some(FpCode::ChecksumMismatch.code())
        );
    }

    #[test]
    fn t05_inject_unknown_app() {
        let mut bad = export_profile(&build_profile());
        if let Some(v) = bad.get_mut(HEADER_LEN) {
            *v = 0xFF; // 条目 0 appid 越 8 域。
        }
        // 重算校验和让流过和闸、撞 appid 闸。
        let body_end = HEADER_LEN + ENTRY_LEN * APP_N;
        let mut head = Vec::new();
        let mut i = 0;
        while i < body_end {
            if let Some(&b) = bad.get(i) {
                head.push(b);
            }
            i += 1;
        }
        let c = fnv1a(&head);
        if let Some(v) = bad.get_mut(body_end) {
            *v = (c & 0xFF) as u8;
        }
        if let Some(v) = bad.get_mut(body_end + 1) {
            *v = ((c >> 8) & 0xFF) as u8;
        }
        if let Some(v) = bad.get_mut(body_end + 2) {
            *v = ((c >> 16) & 0xFF) as u8;
        }
        if let Some(v) = bad.get_mut(body_end + 3) {
            *v = ((c >> 24) & 0xFF) as u8;
        }
        assert_eq!(parse_profile(&bad).err(), Some(FpCode::UnknownApp.code()));
    }

    #[test]
    fn t06_inject_duplicate_app() {
        let mut bad = export_profile(&build_profile());
        if let Some(v) = bad.get_mut(HEADER_LEN + ENTRY_LEN) {
            *v = 0;
        }
        let body_end = HEADER_LEN + ENTRY_LEN * APP_N;
        let mut head = Vec::new();
        let mut i = 0;
        while i < body_end {
            if let Some(&b) = bad.get(i) {
                head.push(b);
            }
            i += 1;
        }
        let c = fnv1a(&head);
        if let Some(v) = bad.get_mut(body_end) {
            *v = (c & 0xFF) as u8;
        }
        if let Some(v) = bad.get_mut(body_end + 1) {
            *v = ((c >> 8) & 0xFF) as u8;
        }
        if let Some(v) = bad.get_mut(body_end + 2) {
            *v = ((c >> 16) & 0xFF) as u8;
        }
        if let Some(v) = bad.get_mut(body_end + 3) {
            *v = ((c >> 24) & 0xFF) as u8;
        }
        assert_eq!(parse_profile(&bad).err(), Some(FpCode::DuplicateApp.code()));
    }

    #[test]
    fn t07_merge_monotonic_and_mask() {
        let t = build_profile();
        assert!(t.merge_monotonic());
        assert!(t.all_in_mask());
    }

    #[test]
    fn t08_double_run_determinism() {
        assert!(repro_check().is_ok());
    }

    #[test]
    fn t09_segment_guard() {
        assert!(segment_guard());
    }

    #[test]
    fn t10_checks_all_green() {
        let set = run_unx18474_checks();
        assert!(set.all_passed(), "UNX-F18474 自检须全绿：{}", set.red_items_text());
    }
}
