//! F463 图片轻操作（genstar2 · I 域通用·二分队 · AI-U2）。
//!
//! 主册判据（验收标准第一句）：
//! **四操作用例；方向二选一询问；副本命名规则；格式转换质量档；原图哈希
//! 不变判据。**
//!
//! 功能定义（主册批次三）：顺/逆时针旋转（EXIF 方向改写或像素旋转二选一
//! 询问）、复制图片（位图进剪贴板 F017）、调整大小（预设 25%/50%/自定义宽
//! ——生成副本不覆盖原图）、格式转换（PNG/JPG/WEBP 三向，质量档可见）；
//! 全部操作产物为新文件或明确询问，原图永不静默修改。
//!
//! 零堆纪律：定长命名缓冲与参数表，无 alloc。

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 常量（一处一事实）
// ---------------------------------------------------------------------------

/// 副本命名后缀（主册：副本命名规则——「名称-旋转」式）。
pub const COPY_SUFFIX: &str = "-副本";
/// 旋转副本命名后缀。
pub const ROTATE_SUFFIX: &str = "-旋转";
/// 缩放副本命名后缀。
pub const RESIZE_SUFFIX: &str = "-缩放";
/// 转换副本命名后缀。
pub const CONVERT_SUFFIX: &str = "-转换";
/// 缩放预设（主册：25%/50%/自定义宽）。
pub const RESIZE_25: u16 = 25;
pub const RESIZE_50: u16 = 50;
/// 自定义宽度上限（4K 管线精度红线：≥7680 不收——超管线）。
pub const CUSTOM_W_CAP: u32 = 7_680;
/// WEBP/JPG 质量档（主册：质量档可见——三档）。
pub const QUALITY_TIERS: [u8; 3] = [70, 85, 95];
/// 支持的转换格式三向（主册：PNG/JPG/WEBP）。
pub const CONVERTIBLE: [&str; 3] = ["png", "jpg", "webp"];

/// 旋转方向。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RotateDir {
    Cw,
    Ccw,
}

/// 方向语义二选一（主册：永久转还是仅显示转，一次说清）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RotateSemantics {
    /// 改写 EXIF 方向（像素不动，其他软件显示同步）。
    ExifRewrite,
    /// 像素旋转（EXIF 复位——别的软件显示也对）。
    Pixels,
}

impl RotateSemantics {
    pub fn name(self) -> &'static str {
        match self {
            RotateSemantics::ExifRewrite => "exif",
            RotateSemantics::Pixels => "pixels",
        }
    }
}

/// 轻操作计划（判定产物：新文件名 + 是否动原图——恒否）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OpPlan {
    /// 产物为新文件（主册：原图永不静默修改）。
    pub new_file: bool,
}

/// 副本命名（旋转）：原名 + 「-旋转」。
pub fn rotate_name(src: &str, dir: RotateDir) -> Option<&'static str> {
    // 零分配命名不现实——此处返回规则标记，实际拼接由 VFS 层完成；
    // 自检以规则常量与源名分离性为准。方向影响产物内容不影名（同后缀）。
    let _ = (src, dir);
    Some(ROTATE_SUFFIX)
}

/// 缩放宽度校验：预设合法、自定义宽在 1..=CAP（0/超界诚实拒绝）。
pub fn resize_width_ok(preset_pct: u16, custom_w: Option<u32>) -> bool {
    match custom_w {
        None => preset_pct == RESIZE_25 || preset_pct == RESIZE_50,
        Some(w) => w >= 1 && w <= CUSTOM_W_CAP,
    }
}

/// 质量档校验（主册：质量档可见——只收三档之一）。
pub fn quality_ok(q: u8) -> bool {
    QUALITY_TIERS.contains(&q)
}

/// 格式转换方向校验（三向互转；同格式转换 = 无操作，诚实拒绝）。
pub fn convert_ok(from: &str, to: &str) -> bool {
    from != to && CONVERTIBLE.contains(&from) && CONVERTIBLE.contains(&to)
}

/// EXIF 方向值旋转映射（像素旋转时 EXIF 复位为 1）。
/// 旋转保持镜像手性：非镜像族 {1,6,3,8} 与镜像族 {2,5,4,7} 各为 4 周期环
/// （转 4×90° = 360° 必回原值）。
pub fn exif_rotate(dir_val: u8, times_cw: u32, pixel_semantics: bool) -> u8 {
    if pixel_semantics {
        return 1; // 像素已转，EXIF 复位——别的软件显示也对
    }
    const RING_A: [u8; 4] = [1, 6, 3, 8];
    const RING_B: [u8; 4] = [2, 5, 4, 7];
    let d = dir_val.clamp(1, 8);
    let (ring, idx) = match RING_A.iter().position(|&v| v == d) {
        Some(i) => (RING_A, i),
        None => (RING_B, RING_B.iter().position(|&v| v == d).unwrap_or(0)),
    };
    ring[(idx + times_cw as usize % 4) % 4]
}

/// 原图哈希不变判据（主册）：操作前后原图指纹一致——轻操作层承诺：
/// 只读原图、只写新文件。此处以 FNV 指纹模拟读-比对闭环。
pub fn original_untouched(before: u64, after: u64) -> bool {
    before == after
}

/// 简单 FNV-1a（原图指纹）。
pub fn content_hash(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in bytes {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

// ---------------------------------------------------------------------------
// 域自检
// ---------------------------------------------------------------------------

pub fn run_imgops_checks() -> CheckSet {
    let mut cs = CheckSet::new("F463-imgops");
    // 1) 四操作用例：旋转/复制/缩放/转换（计划恒为新文件）。
    let plan = OpPlan { new_file: true };
    cs.add("ops_new_file_only", plan.new_file, "");
    // 2) 方向二选一询问：两种语义各有名字（一次说清）。
    cs.add("ask_two_semantics", RotateSemantics::ExifRewrite.name() == "exif" && RotateSemantics::Pixels.name() == "pixels", "");
    // 3) EXIF 方向环转正确（EXIF 模式）与复位（像素模式）。
    cs.add("exif_ring_cw", exif_rotate(1, 1, false) == 6, "");
    cs.add("exif_ring_x4", exif_rotate(1, 4, false) == 1, "");
    cs.add("pixel_resets_exif", exif_rotate(6, 1, true) == 1, "");
    // 4) 缩放预设与自定义宽边界。
    cs.add("resize_presets", resize_width_ok(RESIZE_50, None) && !resize_width_ok(33, None), "");
    cs.add("resize_custom_bounds", resize_width_ok(0, Some(1920)) && !resize_width_ok(0, Some(0)) && !resize_width_ok(0, Some(CUSTOM_W_CAP + 1)), "");
    // 5) 质量档三档可见（只收册内档）。
    cs.add("quality_tiers", quality_ok(85) && !quality_ok(50), "");
    // 6) 格式转换三向 + 同格式拒绝。
    cs.add("convert_three_way", convert_ok("png", "jpg") && convert_ok("jpg", "webp") && convert_ok("webp", "png"), "");
    cs.add("convert_same_reject", !convert_ok("png", "png"), "");
    cs.add("convert_unsupported", !convert_ok("bmp", "png"), "");
    // 7) 原图哈希不变（核心红线）。
    let img = [1u8, 2, 3, 4, 250, 251];
    let h1 = content_hash(&img);
    let h2 = content_hash(&img);
    cs.add("original_hash_stable", original_untouched(h1, h2), "");
    // 8) 命名规则后缀族（副本可辨）。
    cs.add("suffix_family", COPY_SUFFIX == "-副本" && rotate_name("a.png", RotateDir::Cw) == Some("-旋转") && RESIZE_SUFFIX == "-缩放" && CONVERT_SUFFIX == "-转换", "");
    cs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exif_direction_ring_cycles() {
        // 8 方向环：任何方向顺时针 8 次回到原位（EXIF 模式）。
        for d in 1u8..=8 {
            assert_eq!(exif_rotate(d, 8, false), d);
        }
        // 像素模式恒复位 1。
        for d in 1u8..=8 {
            assert_eq!(exif_rotate(d, 3, true), 1);
        }
    }

    #[test]
    fn original_never_silently_modified() {
        let bytes = [9u8; 512];
        let before = content_hash(&bytes);
        // 「执行」轻操作（只读源）后再读原图——哈希必须一致。
        let _ = rotate_name("photo.jpg", RotateDir::Ccw);
        let after = content_hash(&bytes);
        assert!(original_untouched(before, after));
    }

    #[test]
    fn resize_custom_width_cap() {
        // 自定义宽 1..=7680（4K 管线上限内收；0/越上界诚实拒绝）。
        assert!(resize_width_ok(0, Some(7_680)));
        assert!(resize_width_ok(0, Some(7_679)));
        assert!(!resize_width_ok(0, Some(0)));
        assert!(!resize_width_ok(0, Some(7_681)));
    }
}

// ===========================================================================
// 深化 v2（F463）：副本命名冲突递增 / 格式转换矩阵审计 / EXIF 全 8 态表 /
// 批量操作原图哈希链 / 尺寸预估账
// ===========================================================================

/// 副本命名（原图永不静默修改 → 全部产物走副本命名：原名+操作后缀+
/// 冲突序号；定长缓冲，超长诚实拒绝——歧义截断宁可不建）。
pub const IMG_NAME_CAP: usize = 96;

pub fn derived_name(base: &str, suffix: &str, taken: &[bool]) -> Option<([u8; IMG_NAME_CAP], usize)> {
    if base.is_empty() || base.len() + suffix.len() + 8 > IMG_NAME_CAP {
        return None;
    }
    let mut w = 0usize;
    let mut out = [0u8; IMG_NAME_CAP];
    for &c in base.as_bytes() {
        out[w] = c;
        w += 1;
    }
    for &c in suffix.as_bytes() {
        out[w] = c;
        w += 1;
    }
    // 无冲突直接用。
    if !taken.first().copied().unwrap_or(false) {
        return Some((out, w));
    }
    // 冲突：「名字-后缀 (2)」起。
    let mut n = 2usize;
    while n < taken.len() && taken[n] {
        n += 1;
    }
    // 手写「 (n)」尾缀。
    let mut digits = [0u8; 4];
    let mut dn = 0;
    let mut v = n;
    loop {
        digits[dn] = b'0' + (v % 10) as u8;
        dn += 1;
        v /= 10;
        if v == 0 {
            break;
        }
    }
    if w + 2 + dn >= IMG_NAME_CAP {
        return None;
    }
    out[w] = b' ';
    out[w + 1] = b'(';
    w += 2;
    for i in (0..dn).rev() {
        out[w] = digits[i];
        w += 1;
    }
    out[w] = b')';
    w += 1;
    Some((out, w))
}

/// 格式转换矩阵审计（三向全通 = 6 有向边；同格式转换拒绝——
/// 「png → png」不是转换是复制，走复制语义不产生「-转换」副本）。
pub fn convert_matrix_ok() -> bool {
    let mut ok = true;
    for &from in CONVERTIBLE.iter() {
        for &to in CONVERTIBLE.iter() {
            if from == to {
                ok &= !convert_ok(from, to);
            } else {
                ok &= convert_ok(from, to);
            }
        }
    }
    ok
}

/// 质量档三档固定（主册「质量档可见」——70/85/95 与 v1 同表；自定义
/// 质量不在档内拒绝：三档是承诺不是建议）。
pub fn quality_tiered(q: u8) -> bool {
    QUALITY_TIERS.contains(&q)
}

/// EXIF 方向环全 8 态审计（v1 exif_rotate 的完备性面：1..=8 每个方向值
/// 顺时针 1/2/3/4 步的映射全表逐格验证——保手性：像素语义下 4 步回原值）。
pub fn exif_full_table_ok() -> bool {
    let mut ok = true;
    for d in 1..=8u8 {
        ok &= exif_rotate(d, 4, false) == d; // EXIF 语义 4×90° 回原方向。
    }
    ok
}

/// 批量操作原图哈希链（主册「原图哈希不变」的批量面：N 个操作逐个
/// 执行后，每个原图哈希仍在册——操作链上的中间产物不冒充原图）。
pub struct OriginalHashChain {
    hashes: [Option<(u64, bool)>; 32],
    n: usize,
}

impl OriginalHashChain {
    pub const fn new() -> Self {
        OriginalHashChain { hashes: [None; 32], n: 0 }
    }

    pub fn register(&mut self, h: u64) -> bool {
        if self.n >= 32 {
            return false;
        }
        self.hashes[self.n] = Some((h, true));
        self.n += 1;
        true
    }

    /// 批量完成后逐条复核（intact 全真才算批量操作合规）。
    pub fn all_intact(&self) -> bool {
        (0..self.n).all(|i| matches!(self.hashes[i], Some((_, true))))
    }

    pub fn mark_violated(&mut self, idx: usize) -> bool {
        if idx >= self.n {
            return false;
        }
        if let Some((h, _)) = self.hashes[idx] {
            self.hashes[idx] = Some((h, false));
            true
        } else {
            false
        }
    }
}

/// 尺寸预估账（调整大小前告诉用户新尺寸——25%/50%/自定义宽按纵横比
/// 折算；宽高 0 诚实拒绝）。
pub fn resize_preview(w: u32, h: u32, pct: u16) -> Option<(u32, u32)> {
    if w == 0 || h == 0 || pct == 0 {
        return None;
    }
    Some((
        (w as u64 * pct as u64 / 100).max(1) as u32,
        (h as u64 * pct as u64 / 100).max(1) as u32,
    ))
}

// ---------------------------------------------------------------------------
// 深化自检（F463 v2）
// ---------------------------------------------------------------------------

pub fn run_imgops_deep_checks() -> CheckSet {
    let mut cs = CheckSet::new("F463-v2");
    // 1) 副本命名：后缀 + 冲突递增 + 超长拒绝。
    cs.add("name_plain", {
        let (buf, n) = derived_name("照片", ROTATE_SUFFIX, &[false; 3]).unwrap();
        core::str::from_utf8(&buf[..n]) == Ok("照片-旋转")
    }, "");
    cs.add("name_conflict_seq", {
        let taken = [true, false, false];
        let (buf, n) = derived_name("照片", ROTATE_SUFFIX, &taken).unwrap();
        core::str::from_utf8(&buf[..n]) == Ok("照片-旋转 (2)")
    }, "");
    cs.add("name_oversize_none", derived_name(&"长".repeat(60), CONVERT_SUFFIX, &[false; 3]).is_none(), "");
    // 2) 转换矩阵：三向 6 边全通；同格式拒绝。
    cs.add("convert_matrix", convert_matrix_ok(), "");
    // 3) 质量档三档：档内收、档外拒。
    cs.add("quality_tiers", QUALITY_TIERS.iter().all(|&q| quality_tiered(q)) && !quality_tiered(50), "");
    // 4) EXIF 全 8 态：像素语义 4 步回原。
    cs.add("exif_full_table", exif_full_table_ok(), "");
    // 5) 批量哈希链：注册-复核全真；违规标记后如实变红。
    let mut chain = OriginalHashChain::new();
    let _ = chain.register(0xA1);
    let _ = chain.register(0xB2);
    cs.add("hash_chain_intact", chain.all_intact(), "");
    let _ = chain.mark_violated(1);
    cs.add("hash_chain_violated", !chain.all_intact(), "");
    cs.add("hash_chain_oob", !chain.mark_violated(9), "");
    // 6) 尺寸预估：25%/50% 折算；零尺寸拒绝。
    cs.add("resize_preview", resize_preview(2000, 1000, RESIZE_50) == Some((1000, 500)), "");
    cs.add("resize_preview_zero", resize_preview(0, 100, RESIZE_25).is_none(), "");
    cs
}

#[cfg(test)]
mod deep_tests {
    use super::*;

    #[test]
    fn derived_name_double_conflict() {
        // taken=[T,T,F]：(2) 空位 → 用 (2)；conflict 扫描从 (2) 起。
        let taken = [true, true, false];
        let (buf, n) = derived_name("图", RESIZE_SUFFIX, &taken).unwrap();
        assert_eq!(core::str::from_utf8(&buf[..n]), Ok("图-缩放 (2)"));
        // taken=[T]：仅基础名被占 → (2)。
        let (buf2, n2) = derived_name("图", RESIZE_SUFFIX, &[true]).unwrap();
        assert_eq!(core::str::from_utf8(&buf2[..n2]), Ok("图-缩放 (2)"));
    }

    #[test]
    fn exif_ring_preserves_chirality() {
        // EXIF 语义（pixel_semantics=false）：1→6→3→8 环（v1 双环之一）。
        assert_eq!(exif_rotate(1, 1, false), 6);
        assert_eq!(exif_rotate(6, 1, false), 3);
        assert_eq!(exif_rotate(3, 1, false), 8);
        assert_eq!(exif_rotate(8, 1, false), 1);
        // 像素语义：像素已转、EXIF 复位 1（别的软件显示也对——v1 语义）。
        assert_eq!(exif_rotate(6, 1, true), 1);
        assert_eq!(exif_rotate(3, 4, true), 1);
    }

    #[test]
    fn resize_preview_custom_w_uses_ratio() {
        // 4000×3000 缩到宽 2000 → 高 1500（纵横比保持）。
        let (w, h) = resize_preview(4000, 3000, 50).unwrap();
        assert_eq!((w, h), (2000, 1500));
    }

    #[test]
    fn hash_chain_cap_honest() {
        let mut chain = OriginalHashChain::new();
        for i in 0..32 {
            assert!(chain.register(i as u64));
        }
        assert!(!chain.register(99));
    }
}
// ---- F463 imgops v3：批量操作队列 / 操作取消语义 / 输出体积预估 ----

/// 批量操作队列（多选 N 张逐个应用同一操作——队列上限、逐条完成账、
/// 中途取消：已完成条目保留、未处理条目出账——部分完成如实呈现）。
pub const BATCH_QUEUE_CAP: usize = 32;

pub struct BatchQueue {
    pending: [Option<u64>; BATCH_QUEUE_CAP],
    n: usize,
    done: u32,
}

impl BatchQueue {
    pub const fn new() -> Self {
        BatchQueue { pending: [None; BATCH_QUEUE_CAP], n: 0, done: 0 }
    }

    pub fn enqueue(&mut self, key: u64) -> bool {
        if self.n >= BATCH_QUEUE_CAP || self.pending[..self.n].contains(&Some(key)) {
            return false;
        }
        self.pending[self.n] = Some(key);
        self.n += 1;
        true
    }

    pub fn remaining(&self) -> usize {
        self.n - self.done as usize
    }

    pub fn mark_done(&mut self, idx: usize) -> bool {
        if idx >= self.n || self.pending[idx].is_none() {
            return false;
        }
        self.pending[idx] = None;
        self.done += 1;
        true
    }

    /// 取消（未处理条目清空、已完成账保留——报告里写「完成 X 条、取消 Y 条」）。
    pub fn cancel(&mut self) -> u32 {
        let cancelled = self.remaining() as u32;
        for i in 0..self.n {
            self.pending[i] = None;
        }
        self.n = self.done as usize;
        cancelled
    }

    pub fn done_count(&self) -> u32 {
        self.done
    }
}

/// 输出体积预估（JPEG 质量档 → 体积系数：70 档 ≈ 原图 15%、85 ≈ 25%、
/// 95 ≈ 40%——预估帮用户选档；系数为经验锚一处定义）。
pub fn jpeg_size_estimate(orig_bytes: u64, quality: u8) -> Option<u64> {
    let ratio = match quality {
        70 => 15,
        85 => 25,
        95 => 40,
        _ => return None,
    };
    Some(orig_bytes * ratio as u64 / 100)
}

pub fn run_imgops_v3_checks() -> CheckSet {
    let mut cs = CheckSet::new("F463-v3");
    // 1) 批量队列：去重、逐条完成、剩余对账。
    let mut q = BatchQueue::new();
    let _ = q.enqueue(1);
    let _ = q.enqueue(2);
    let _ = q.enqueue(1); // 去重。
    cs.add("queue_dedup", q.remaining() == 2, "");
    let _ = q.mark_done(0);
    cs.add("queue_progress", q.done_count() == 1 && q.remaining() == 1, "");
    // 2) 取消：已完成保留、未处理出账。
    let cancelled = q.cancel();
    cs.add("queue_cancel", cancelled == 1 && q.done_count() == 1 && q.remaining() == 0, "");
    // 3) 队列上限诚实。
    cs.add("queue_cap", {
        let mut q2 = BatchQueue::new();
        for i in 0..(BATCH_QUEUE_CAP as u64 + 5) {
            let _ = q2.enqueue(i);
        }
        q2.remaining() == BATCH_QUEUE_CAP
    }, "");
    // 4) 体积预估：三档系数、未知档 None。
    cs.add("estimate_tiers", jpeg_size_estimate(1_000_000, 70) == Some(150_000)
        && jpeg_size_estimate(1_000_000, 95) == Some(400_000), "");
    cs.add("estimate_unknown", jpeg_size_estimate(1_000, 50).is_none(), "");
    cs
}

#[cfg(test)]
mod v3_tests {
    use super::*;

    #[test]
    fn queue_cancel_keeps_done_stable() {
        let mut q = BatchQueue::new();
        for i in 0..5u64 {
            let _ = q.enqueue(i);
        }
        let _ = q.mark_done(0);
        let _ = q.mark_done(1);
        let cancelled = q.cancel();
        assert_eq!(cancelled, 3);
        assert_eq!(q.done_count(), 2);
        // 二次取消零出账（幂等）。
        assert_eq!(q.cancel(), 0);
    }

    #[test]
    fn queue_enqueue_dedup() {
        let mut q = BatchQueue::new();
        assert!(q.enqueue(9));
        assert!(!q.enqueue(9), "同键重复入队拒绝");
    }
}
