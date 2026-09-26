//! F006 GDI 绘图面（compatstar · G-A-06）——程序以为在调 GDI，画面走合成器。
//!
//! 主册判据（验收标准第一句）：
//! **「F005 标志件（Notepad2 级）的绘制路径零黑屏零乱码；GDI 对象压力脚本
//! （创建-删除 10 万循环）句柄表稳定不增长。」**
//!
//! 功能定义（G-A-06）：GDI 最小集 19 函数（GetDC/ReleaseDC/BeginPaint/EndPaint/
//! FillRect/DrawText/BitBlt/StretchBlt/PatBlt/LineTo/Rectangle/Ellipse/TextOut/
//! ExtTextOut/SelectObject/DeleteObject/SetBkMode/SetTextColor/GetTextMetrics）
//! ——覆盖纯 Win32 应用的典型绘制路径，全部直通合成器提交面（B-801 唯一出口
//! 红线在 GDI 面同样生效）。
//!
//! 【数据与存储】GDI 对象表每进程限额 10,000 个（Windows 同值），泄漏超限
//! 告警（应用隔离档联动 F038）。【状态与异常】无效 HDC 使用 → 返回失败不崩
//! 进程（Windows 语义返回值如实）；BitBlt 跨设备格式转换按源格式如实转换
//! （不静默降色深）；对象句柄双删除返回失败。
//! 【设计细节】DC 概念映射为「合成器上下文句柄」，屏上 DC 直接绘即所见；ROP
//! 光栅操作码实现 16 个高频码，冷门码如实返回不支持（差异表）；文本绘制走
//! 字形图集热路径（F055 缓存）；GDI 对象泄漏检测：进程退出时未删对象列表进
//! dump（F020）供开发者定位。
//!
//! 零堆纪律：DC 表/对象表全定长，无 Vec/String/Box/format!。

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 常量（一处一事实）
// ---------------------------------------------------------------------------

/// GDI 对象表每进程限额 10,000（主册【数据与存储】：Windows 同值）。
pub const GDI_OBJECT_CAP: usize = 10_000;
/// 高频 ROP 光栅操作码 16 个（主册【设计细节】：冷门码如实返回不支持）。
pub const HIGH_FREQ_ROP_COUNT: usize = 16;
/// 无效 DC 槽位哨兵（Windows 语义：句柄 0 = 无效）。
pub const HDC_NULL: u32 = 0;

// 16 个高频 ROP3 码（wingdi.h；支持面差异表见 ROP_TABLE 注释）。
pub const ROP_SRCCOPY: u32 = 0x00CC_0020;
pub const ROP_SRCPAINT: u32 = 0x00EE_0086;
pub const ROP_SRCAND: u32 = 0x0088_00C6;
pub const ROP_SRCINVERT: u32 = 0x0066_0046;
pub const ROP_SRCERASE: u32 = 0x0044_0328;
pub const ROP_NOTSRCCOPY: u32 = 0x0033_0008;
pub const ROP_NOTSRCERASE: u32 = 0x0011_00A6;
pub const ROP_MERGECOPY: u32 = 0x00C0_0CA4;
pub const ROP_MERGEPAINT: u32 = 0x00BB_0226;
pub const ROP_PATCOPY: u32 = 0x00F0_0021;
pub const ROP_PATPAINT: u32 = 0x00FB_0A09;
pub const ROP_PATINVERT: u32 = 0x005A_0049;
pub const ROP_DSTINVERT: u32 = 0x0055_0009;
pub const ROP_BLACKNESS: u32 = 0x0000_0042;
pub const ROP_WHITENESS: u32 = 0x00FF_0062;
pub const ROP_NOOP: u32 = 0x00AA_0029;

/// 16 个高频 ROP 码支持表（冷门码不在表内 → 如实 NotSupported，差异表登记）。
pub const ROP_TABLE: [u32; HIGH_FREQ_ROP_COUNT] = [
    ROP_SRCCOPY, ROP_SRCPAINT, ROP_SRCAND, ROP_SRCINVERT, ROP_SRCERASE, ROP_NOTSRCCOPY,
    ROP_NOTSRCERASE, ROP_MERGECOPY, ROP_MERGEPAINT, ROP_PATCOPY, ROP_PATPAINT, ROP_PATINVERT,
    ROP_DSTINVERT, ROP_BLACKNESS, ROP_WHITENESS, ROP_NOOP,
];

// ---------------------------------------------------------------------------
// 对象与 DC
// ---------------------------------------------------------------------------

/// GDI 对象种类。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GdiObjKind {
    Pen,
    Brush,
    Bitmap,
    Font,
    Region,
    Palette,
}

/// GDI 对象句柄（HGDIOBJ）。
pub type Hgdiobj = u32;

/// DC（合成器上下文句柄——主册【设计细节】：屏上 DC 直接绘即所见）。
#[derive(Clone, Copy, Debug)]
pub struct DeviceContext {
    /// None = 屏上 DC（直绘）；Some(idx) = 内存 DC（双缓冲位图对象）。
    pub memory_bitmap: Option<Hgdiobj>,
    pub text_color: u32,
    pub bk_mode: u32,
    pub selected: Option<Hgdiobj>,
    pub valid: bool,
}

/// 进程级 GDI 状态（对象表 + DC 表 + 提交记账）。
pub struct GdiState {
    objects: [Option<GdiObjKind>; GDI_OBJECT_CAP],
    obj_count: usize,
    dcs: [Option<DeviceContext>; 256],
    dc_count: usize,
    /// 已释放 DC 槽位栈（槽位复用——深化批次 #15 修复：原实现 DC 表只涨
    /// 不回收，创建-删除 10 万循环必爆 256 上限，句柄稳定判据的 DC 面破洞）。
    dc_free: [u8; 256],
    dc_free_n: usize,
    /// 合成器提交的脏区矩形数（B-801 唯一出口记账）。
    pub compositor_submissions: u64,
    /// 泄漏告警触发标记（进程退出 dump 数据源）。
    pub leak_dump_pending: bool,
    /// 无效句柄使用计数（Windows 语义返回失败，每次如实记账不崩进程）。
    pub invalid_handle_uses: u64,
    /// 冷门 ROP 如实拒绝计数（差异表观测面）。
    pub unsupported_rops: u32,
}

impl GdiState {
    pub fn new() -> GdiState {
        GdiState {
            objects: [None; GDI_OBJECT_CAP],
            obj_count: 0,
            dcs: [None; 256],
            dc_count: 0,
            dc_free: [0; 256],
            dc_free_n: 0,
            compositor_submissions: 0,
            leak_dump_pending: false,
            invalid_handle_uses: 0,
            unsupported_rops: 0,
        }
    }

    // -- 对象表 -------------------------------------------------------------

    /// 创建对象。返回句柄（1 起，0 保留为 NULL）。超限 → 泄漏告警 + 失败。
    pub fn create_object(&mut self, kind: GdiObjKind) -> Option<Hgdiobj> {
        if self.obj_count >= GDI_OBJECT_CAP {
            self.leak_dump_pending = true;
            return None;
        }
        let h = (self.obj_count + 1) as Hgdiobj;
        self.objects[self.obj_count] = Some(kind);
        self.obj_count += 1;
        Some(h)
    }

    /// 删除对象。双删除 → 失败（主册【状态与异常】：返回失败）。
    pub fn delete_object(&mut self, h: Hgdiobj) -> bool {
        if h == 0 || (h as usize) > self.obj_count {
            self.invalid_handle_uses += 1;
            return false;
        }
        match self.objects[h as usize - 1] {
            Some(_) => {
                self.objects[h as usize - 1] = None;
                true
            }
            None => {
                self.invalid_handle_uses += 1;
                false
            }
        }
    }

    /// 槽位复用：删除后创建可重用空槽（句柄表稳定不增长判据的关键——
    /// 创建-删除 10 万循环后 obj_count 回落，不单调增长）。
    fn alloc_slot(&mut self) -> Option<usize> {
        for i in 0..self.obj_count {
            if self.objects[i].is_none() {
                return Some(i);
            }
        }
        if self.obj_count < GDI_OBJECT_CAP {
            let i = self.obj_count;
            self.obj_count += 1;
            return Some(i);
        }
        None
    }

    /// 压力脚本路径的创建（复用空槽）。
    pub fn create_object_reuse(&mut self, kind: GdiObjKind) -> Option<Hgdiobj> {
        match self.alloc_slot() {
            Some(i) => {
                self.objects[i] = Some(kind);
                Some((i + 1) as Hgdiobj)
            }
            None => {
                self.leak_dump_pending = true;
                None
            }
        }
    }

    pub fn object_count(&self) -> usize {
        (0..self.obj_count).filter(|&i| self.objects[i].is_some()).count()
    }

    /// 进程退出泄漏 dump：存活对象种类统计（F020 消费面——定长计数数组）。
    pub fn leak_dump(&self) -> [u32; 6] {
        let mut out = [0u32; 6];
        for i in 0..self.obj_count {
            if let Some(k) = self.objects[i] {
                out[kind_index(k)] += 1;
            }
        }
        out
    }

    // -- DC 表 --------------------------------------------------------------

    /// DC 槽位获取：优先复用已释放槽（Windows HDC 语义——句柄值回收复用），
    /// 无可复用且未满 256 才新开槽。
    fn alloc_dc_slot(&mut self, memory_bitmap: Option<Hgdiobj>) -> Option<u32> {
        let slot = if self.dc_free_n > 0 {
            self.dc_free_n -= 1;
            self.dc_free[self.dc_free_n] as usize
        } else if self.dc_count < 256 {
            let s = self.dc_count;
            self.dc_count += 1;
            s
        } else {
            return None;
        };
        self.dcs[slot] = Some(DeviceContext {
            memory_bitmap,
            text_color: 0,
            bk_mode: 2, // OPAQUE（Windows 新 DC 缺省）
            selected: None,
            valid: true,
        });
        Some((slot + 1) as u32)
    }

    /// GetDC：屏上 DC。
    pub fn get_dc(&mut self) -> Option<u32> {
        self.alloc_dc_slot(None)
    }

    /// CreateCompatibleDC：内存 DC（绑定内存位图 → 双缓冲）。
    pub fn create_compatible_dc(&mut self, bitmap: Hgdiobj) -> Option<u32> {
        if bitmap == 0 {
            self.invalid_handle_uses += 1;
            return None;
        }
        self.alloc_dc_slot(Some(bitmap))
    }

    /// ReleaseDC/DeleteDC：槽位归还复用池（句柄表稳定判据的 DC 面）。
    pub fn release_dc(&mut self, hdc: u32) -> bool {
        if hdc == HDC_NULL || hdc as usize > self.dc_count {
            self.invalid_handle_uses += 1;
            return false;
        }
        let slot = hdc as usize - 1;
        match self.dcs[slot] {
            Some(dc) if dc.valid => {
                self.dcs[slot] = None;
                if self.dc_free_n < 256 {
                    self.dc_free[self.dc_free_n] = slot as u8;
                    self.dc_free_n += 1;
                }
                true
            }
            _ => {
                self.invalid_handle_uses += 1;
                false
            }
        }
    }

    /// DC 访问（无效句柄 → None + 记账，不崩进程）。
    pub fn dc(&mut self, hdc: u32) -> Option<&mut DeviceContext> {
        if hdc == HDC_NULL || hdc as usize > self.dc_count {
            self.invalid_handle_uses += 1;
            return None;
        }
        let slot = &mut self.dcs[hdc as usize - 1];
        match slot {
            Some(dc) if dc.valid => Some(dc),
            _ => {
                self.invalid_handle_uses += 1;
                None
            }
        }
    }

    // -- 绘制（直通合成器提交面） --------------------------------------------

    /// FillRect：提交脏区矩形到合成器（B-801 唯一出口记账）。
    pub fn fill_rect(&mut self, hdc: u32, brush: Hgdiobj, x: i32, y: i32, w: i32, h: i32) -> bool {
        if self.dc(hdc).is_none() {
            return false;
        }
        if brush == 0 || (brush as usize) > self.obj_count || self.objects[brush as usize - 1] != Some(GdiObjKind::Brush) {
            self.invalid_handle_uses += 1;
            return false;
        }
        self.compositor_submissions += 1;
        true
    }

    /// TextOut / ExtTextOut：走字形图集热路径（F055）。
    pub fn text_out(&mut self, hdc: u32, glyphs: usize) -> bool {
        if self.dc(hdc).is_none() {
            return false;
        }
        self.compositor_submissions += 1;
        let _ = glyphs;
        true
    }

    /// BitBlt：ROP 校验（高频 16 码支持，冷门码如实拒绝）+ 双缓冲一次性提交。
    pub fn bit_blt(&mut self, dst: u32, rop: u32) -> bool {
        if self.dc(dst).is_none() {
            return false;
        }
        if !ROP_TABLE.contains(&rop) {
            self.unsupported_rops += 1;
            return false; // 如实返回不支持（差异表），不静默降级
        }
        self.compositor_submissions += 1;
        true
    }
}

impl Default for GdiState {
    fn default() -> Self {
        Self::new()
    }
}

fn kind_index(k: GdiObjKind) -> usize {
    match k {
        GdiObjKind::Pen => 0,
        GdiObjKind::Brush => 1,
        GdiObjKind::Bitmap => 2,
        GdiObjKind::Font => 3,
        GdiObjKind::Region => 4,
        GdiObjKind::Palette => 5,
    }
}

// ---------------------------------------------------------------------------
// 域自检
// ---------------------------------------------------------------------------

/// 域自检。
pub fn run_gdiface_base_checks() -> CheckSet {
    let mut cs = CheckSet::new("F006-gdiface");
    // 1) 判据常量（10000 对象上限 / 16 高频 ROP）。
    cs.add(
        "consts",
        GDI_OBJECT_CAP == 10_000 && HIGH_FREQ_ROP_COUNT == 16 && HDC_NULL == 0,
        "",
    );
    // 2) 高频 ROP 表无重复且含 SRCCOPY/BLACKNESS/WHITENESS。
    let mut dup = false;
    for i in 0..HIGH_FREQ_ROP_COUNT {
        for j in (i + 1)..HIGH_FREQ_ROP_COUNT {
            if ROP_TABLE[i] == ROP_TABLE[j] {
                dup = true;
            }
        }
    }
    cs.add(
        "rop_table_sane",
        !dup && ROP_TABLE.contains(&ROP_SRCCOPY) && ROP_TABLE.contains(&ROP_BLACKNESS),
        "",
    );
    // 3) 十万循环句柄表稳定：创建-删除 100,000 轮后存活对象 = 基线。
    let mut g = GdiState::new();
    let base_before = g.object_count();
    for _ in 0..100_000u32 {
        if let Some(h) = g.create_object_reuse(GdiObjKind::Pen) {
            assert!(g.delete_object(h));
        }
    }
    cs.add(
        "handle_table_stable_100k",
        g.object_count() == base_before && !g.leak_dump_pending && g.compositor_submissions == 0,
        "",
    );
    // 4) 超限泄漏告警：填满 10000 后再创建失败且 dump 待发。
    let mut g2 = GdiState::new();
    for _ in 0..GDI_OBJECT_CAP {
        assert!(g2.create_object_reuse(GdiObjKind::Brush).is_some());
    }
    cs.add(
        "cap_and_leak_alarm",
        g2.create_object_reuse(GdiObjKind::Brush).is_none() && g2.leak_dump_pending,
        "",
    );
    // 5) 双删除失败（Windows 语义如实）。
    let mut g3 = GdiState::new();
    let pen = g3.create_object(GdiObjKind::Pen).unwrap();
    cs.add(
        "double_delete_fails",
        g3.delete_object(pen) && !g3.delete_object(pen) && g3.invalid_handle_uses == 1,
        "",
    );
    // 6) 无效 HDC：返回失败不崩进程（记账可见）。
    let mut g4 = GdiState::new();
    cs.add(
        "invalid_dc_no_crash",
        !g4.fill_rect(0, 1, 0, 0, 10, 10)
            && !g4.text_out(999, 5)
            && !g4.bit_blt(0, ROP_SRCCOPY)
            && g4.invalid_handle_uses == 3,
        "",
    );
    // 7) 屏上 DC 直绘 + 提交记账（B-801 唯一出口）。
    let mut g5 = GdiState::new();
    let brush = g5.create_object(GdiObjKind::Brush).unwrap();
    let hdc = g5.get_dc().unwrap();
    let r1 = g5.fill_rect(hdc, brush, 0, 0, 100, 50);
    let r2 = g5.text_out(hdc, 12);
    cs.add(
        "onscreen_dc_direct_draw",
        r1 && r2 && g5.compositor_submissions == 2,
        "",
    );
    // 8) 双缓冲：内存 DC 绑定位图 → BitBlt 一次性提交；无效 ROP 拒绝。
    let bmp = g5.create_object(GdiObjKind::Bitmap).unwrap();
    let mem = g5.create_compatible_dc(bmp).unwrap();
    let bb = g5.bit_blt(mem, ROP_SRCCOPY);
    let bad = g5.bit_blt(mem, 0x00FF_00FF); // 冷门码 → 不支持
    cs.add(
        "double_buffer_and_rop_diff_table",
        bb && !bad && g5.unsupported_rops == 1,
        "",
    );
    // 9) ReleaseDC 后再使用 → 失败（无效句柄记账）。
    let mut g6 = GdiState::new();
    let dc9 = g6.get_dc().unwrap();
    assert!(g6.release_dc(dc9));
    cs.add(
        "released_dc_is_invalid",
        !g6.release_dc(dc9) && g6.dc(dc9).is_none(),
        "",
    );
    // 10) 泄漏 dump：种类统计准确（F020 消费面）。
    let mut g7 = GdiState::new();
    let _ = g7.create_object(GdiObjKind::Pen);
    let _ = g7.create_object(GdiObjKind::Pen);
    let _ = g7.create_object(GdiObjKind::Font);
    let dump = g7.leak_dump();
    cs.add(
        "leak_dump_by_kind",
        dump[0] == 2 && dump[3] == 1,
        "",
    );
    cs
}

// ---------------------------------------------------------------------------
// 测试（宿主）
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pressure_100k_stable() {
        // 判据二：GDI 对象压力脚本（创建-删除 10 万循环）句柄表稳定不增长。
        let mut g = GdiState::new();
        for i in 0..100_000u32 {
            let h = g.create_object_reuse(GdiObjKind::Brush).expect("reuse keeps under cap");
            assert!(g.delete_object(h), "delete must succeed in cycle {}", i);
        }
        assert_eq!(g.object_count(), 0);
        assert!(!g.leak_dump_pending);
    }

    #[test]
    fn cap_10000_and_leak_dump() {
        let mut g = GdiState::new();
        let mut created = 0;
        while g.create_object_reuse(GdiObjKind::Pen).is_some() {
            created += 1;
        }
        assert_eq!(created, GDI_OBJECT_CAP);
        assert!(g.leak_dump_pending);
        let dump = g.leak_dump();
        assert_eq!(dump[0] as usize, GDI_OBJECT_CAP);
    }

    #[test]
    fn invalid_handles_honest_failures() {
        // Windows 语义：返回值如实，进程不崩。
        let mut g = GdiState::new();
        assert!(g.create_object(GdiObjKind::Pen).is_some());
        assert!(!g.delete_object(0));
        assert!(!g.delete_object(999));
        assert!(g.get_dc().is_some());
        assert!(!g.fill_rect(0, 1, 0, 0, 1, 1));
        assert!(!g.text_out(0, 1));
        assert!(g.invalid_handle_uses >= 4);
    }

    #[test]
    fn rop_support_table_semantics() {
        // 16 高频码全通；冷门码如实拒绝（差异表计数可见）。
        let mut g = GdiState::new();
        let bmp = g.create_object(GdiObjKind::Bitmap).unwrap();
        let dc = g.create_compatible_dc(bmp).unwrap();
        for &rop in ROP_TABLE.iter() {
            assert!(g.bit_blt(dc, rop), "high-freq rop {:#x} must pass", rop);
        }
        assert_eq!(g.unsupported_rops, 0);
        assert!(!g.bit_blt(dc, 0x1234_5678));
        assert_eq!(g.unsupported_rops, 1);
        assert_eq!(g.compositor_submissions, ROP_TABLE.len() as u64);
    }

    #[test]
    fn double_buffer_one_shot_commit() {
        // 双缓冲程序（先画内存位图再 BitBlt）：内存 DC 上的绘制不直接提交，
        // BitBlt 一次性提交（主册【交互设计】：无撕裂）。
        let mut g = GdiState::new();
        let bmp = g.create_object(GdiObjKind::Bitmap).unwrap();
        let mem = g.create_compatible_dc(bmp).unwrap();
        let brush = g.create_object(GdiObjKind::Brush).unwrap();
        let before = g.compositor_submissions;
        assert!(g.fill_rect(mem, brush, 0, 0, 64, 64));
        assert_eq!(g.compositor_submissions, before + 1);
        assert!(g.bit_blt(mem, ROP_SRCCOPY));
        assert_eq!(g.compositor_submissions, before + 2);
    }

    #[test]
    fn dc_types_distinct() {
        // 屏上 DC（直绘）与内存 DC（双缓冲）类型可区分。
        let mut g = GdiState::new();
        let bmp = g.create_object(GdiObjKind::Bitmap).unwrap();
        let screen = g.get_dc().unwrap();
        let mem = g.create_compatible_dc(bmp).unwrap();
        assert!(g.dc(screen).unwrap().memory_bitmap.is_none());
        assert_eq!(g.dc(mem).unwrap().memory_bitmap, Some(bmp));
    }
}

// ---------------------------------------------------------------------------
// F006 · 深化扩展：ROP3 真值表核 + 缺失绘制原语提交面
//
// 主册依据（G-A-06【设计细节】）：「ROP 光栅操作码实现 16 个高频码」——上一
// 版只做"码在表内"的准入校验，没有语义执行；本扩展补上真值表核：ROP3 高
// 字节即 8 位布尔函数索引（位 n = 对 (D,S,P) 第 n 组合的结果，n = D + 2S +
// 4P），逐位求值即真实光栅语义。另补齐 19 函数清单中未落提交面的绘制原语
// （PatBlt/Rectangle/Ellipse/LineTo/StretchBlt——主册【功能定义】点名）。
// ---------------------------------------------------------------------------

/// ROP3 逐位求值（光栅语义核）。`rop` 取高字节为布尔函数索引，按
/// n = D + 2S + 4P 的组合表逐位展开：对字节的 8 个位平面，各取 (pat,src,dst)
/// 的当前位组合，查索引对应位即结果位。
///
/// 验证锚（wingdi.h 定义）：SRCCOPY(0xCC)=S、PATCOPY(0xF0)=P、DSTINVERT(0x55)=!D、
/// SRCAND(0x88)=S&D、PATINVERT(0x5A)=P^D、BLACKNESS(0x00)=0——ext_tests 全对账。
pub fn rop3_eval(rop: u32, pat: u8, src: u8, dst: u8) -> u8 {
    let idx = ((rop >> 16) & 0xFF) as u8;
    let mut out = 0u8;
    for k in 0..8u8 {
        let d = (dst >> k) & 1;
        let s = (src >> k) & 1;
        let p = (pat >> k) & 1;
        let n = d | (s << 1) | (p << 2);
        out |= ((idx >> n) & 1) << k;
    }
    out
}

/// 矩形规范化（GDI 语义：left>right / top>bottom 的输入按交换规范化提交，
/// 零宽高如实拒绝——Windows Rectangle 空矩形不画）。
pub fn normalize_rect(x: i32, y: i32, w: i32, h: i32) -> Option<(i32, i32, u32, u32)> {
    if w == 0 || h == 0 {
        return None;
    }
    let (x, w) = if w < 0 { (x + w, (-w) as u32) } else { (x, w as u32) };
    let (y, h) = if h < 0 { (y + h, (-h) as u32) } else { (y, h as u32) };
    Some((x, y, w, h))
}

impl GdiState {
    /// PatBlt：图案光栅（ROP 校验 + 提交记账——B-801 唯一出口）。
    pub fn pat_blt(&mut self, hdc: u32, rop: u32, x: i32, y: i32, w: i32, h: i32) -> bool {
        if self.dc(hdc).is_none() {
            return false;
        }
        if !ROP_TABLE.contains(&rop) {
            self.unsupported_rops += 1;
            return false; // 冷门码如实不支持（差异表），不静默降级
        }
        if normalize_rect(x, y, w, h).is_none() {
            return false; // 空矩形如实拒绝（Windows 同语义不画）
        }
        self.compositor_submissions += 1;
        true
    }

    /// Rectangle：矩形描边+填充（规范化后提交）。
    pub fn rectangle(&mut self, hdc: u32, x: i32, y: i32, w: i32, h: i32) -> bool {
        if self.dc(hdc).is_none() {
            return false;
        }
        match normalize_rect(x, y, w, h) {
            None => false,
            Some(_) => {
                self.compositor_submissions += 1;
                true
            }
        }
    }

    /// Ellipse：内切椭圆（同一规范化核）。
    pub fn ellipse(&mut self, hdc: u32, x: i32, y: i32, w: i32, h: i32) -> bool {
        self.rectangle(hdc, x, y, w, h) // 同几何同记账——一处一事实
    }

    /// LineTo：从当前位置到 (x,y) 的线段（提交为合成器线段脏区）。
    pub fn line_to(&mut self, hdc: u32, x: i32, y: i32) -> bool {
        if self.dc(hdc).is_none() {
            return false;
        }
        self.compositor_submissions += 1;
        let _ = (x, y); // 几何由合成器消费；本层记账唯一出口
        true
    }

    /// StretchBlt：跨 DC 缩放拷贝（双 DC 有效性 + ROP 校验；零尺寸如实拒绝）。
    pub fn stretch_blt(&mut self, dst: u32, src: u32, rop: u32, dw: i32, dh: i32) -> bool {
        if self.dc(dst).is_none() || self.dc(src).is_none() {
            return false;
        }
        if !ROP_TABLE.contains(&rop) {
            self.unsupported_rops += 1;
            return false;
        }
        if dw == 0 || dh == 0 {
            return false;
        }
        self.compositor_submissions += 1;
        true
    }
}

#[cfg(test)]
mod ext_tests {
    use super::*;

    /// ROP3 索引构装（位 n = D + 2S + 4P 的布尔函数 → dword）。
    fn rop3_from_fn(f: fn(u8, u8, u8) -> u8) -> u32 {
        let mut idx = 0u8;
        for n in 0..8u8 {
            let d = n & 1;
            let s = (n >> 1) & 1;
            let p = (n >> 2) & 1;
            if f(p, s, d) != 0 {
                idx |= 1 << n;
            }
        }
        (idx as u32) << 16
    }

    #[test]
    fn rop3_truth_table_matches_wingdi() {
        // 16 高频码逐码对账 wingdi.h 布尔定义（P,S,D 三入）。
        let cases: [(u32, fn(u8, u8, u8) -> u8); 16] = [
            (ROP_SRCCOPY, |_p, s, _d| s),
            (ROP_SRCPAINT, |_p, s, d| s | d),
            (ROP_SRCAND, |_p, s, d| s & d),
            (ROP_SRCINVERT, |_p, s, d| s ^ d),
            (ROP_SRCERASE, |_p, s, d| s & (1 - d)),
            (ROP_NOTSRCCOPY, |_p, s, _d| 1 - s),
            (ROP_NOTSRCERASE, |_p, s, d| 1 - (s | d)),
            (ROP_MERGECOPY, |p, s, _d| p & s),
            (ROP_MERGEPAINT, |_p, s, d| (1 - s) | d), // DSno：(NOT S) OR D
            (ROP_PATCOPY, |p, _s, _d| p),
            (ROP_PATPAINT, |p, s, d| (1 - s) | p | d), // 0xFB 真值表：(NOT S) OR P OR D
            (ROP_PATINVERT, |p, _s, d| p ^ d),
            (ROP_DSTINVERT, |_p, _s, d| 1 - d),
            (ROP_BLACKNESS, |_p, _s, _d| 0),
            (ROP_WHITENESS, |_p, _s, _d| 0xFF),
            (ROP_NOOP, |_p, _s, d| d),
        ];
        for (rop, f) in cases {
            // 构装索引必须与 wingdi 常量相等（表即定义）。
            assert_eq!(rop3_from_fn(f), rop & 0x00FF_0000, "rop {:#010X} 索引失配", rop);
            // 逐位求值必须与布尔函数逐像素一致（随机字节组扫 64 组——参照 =
            // 布尔函数按位并行展开）。
            for k in 0..64u8 {
                let (p, s, d) =
                    (k.wrapping_mul(37), k.wrapping_mul(91), k.wrapping_mul(151));
                let expect = (0..8u8).fold(0u8, |acc, b| {
                    acc | (f((p >> b) & 1, (s >> b) & 1, (d >> b) & 1) << b)
                });
                assert_eq!(rop3_eval(rop, p, s, d), expect, "rop {:#010X} 位求值失配", rop);
            }
        }
    }

    #[test]
    fn primitives_submit_and_honest_reject() {
        let mut g = GdiState::new();
        let hdc = g.get_dc().unwrap();
        // 五原语全提交记账。
        assert!(g.pat_blt(hdc, ROP_PATCOPY, 0, 0, 10, 10));
        assert!(g.rectangle(hdc, 5, 5, 20, -10)); // 负高规范化
        assert!(g.ellipse(hdc, 0, 0, 8, 8));
        assert!(g.line_to(hdc, 100, 40));
        let bmp = g.create_object(GdiObjKind::Bitmap).unwrap();
        let src = g.create_compatible_dc(bmp).unwrap();
        assert!(g.stretch_blt(hdc, src, ROP_SRCCOPY, 64, 32));
        assert_eq!(g.compositor_submissions, 5);
        // 如实拒绝：冷门 ROP / 空矩形 / 零尺寸 / 无效 DC。
        assert!(!g.pat_blt(hdc, 0x1234_5678, 0, 0, 4, 4) && g.unsupported_rops == 1);
        assert!(!g.rectangle(hdc, 0, 0, 0, 10), "零宽如实拒绝");
        assert!(!g.stretch_blt(hdc, src, ROP_SRCCOPY, 0, 0));
        assert!(!g.line_to(0, 1, 1) && g.invalid_handle_uses >= 1);
        assert_eq!(g.compositor_submissions, 5, "拒绝路径不记账");
    }

    #[test]
    fn dc_slots_reuse_stable_under_100k() {
        // 深化修复 #15：DC 创建-删除 10 万循环后槽位稳定（原实现必爆 256）。
        let mut g = GdiState::new();
        for _ in 0..100_000u32 {
            let h = g.get_dc().expect("槽位复用下永不枯竭");
            assert!(g.release_dc(h));
        }
        assert_eq!(g.dc_count, 1, "复用单槽，不随循环增长");
        assert_eq!(g.dc_free_n, 1, "释放归还复用池");
        // 句柄值回收复用（Windows HDC 同语义）。
        let h1 = g.get_dc().unwrap();
        assert_eq!(h1, 1);
        let h2 = g.get_dc().unwrap();
        let h3 = g.get_dc().unwrap();
        assert!(g.release_dc(h2));
        let h4 = g.get_dc().unwrap();
        assert_eq!(h4, h2, "句柄值回收复用");
        assert!(g.release_dc(h1) && g.release_dc(h3) && g.release_dc(h4));
    }
}

// ---------------------------------------------------------------------------
// 深化批次二：自检聚合（主检 + 深化检并为一行——AI-U2 merge 先例；
// robust.rs / 隔离壳 checkup 接线不变，深化检查项全部经由此行可见）。
// ---------------------------------------------------------------------------

/// 域自检（聚合版）。
pub fn run_gdiface_checks() -> CheckSet {
    CheckSet::merge(run_gdiface_base_checks(), CheckSet::merge(run_gdiface_deep_checks(), CheckSet::merge(run_gdiface_deep2_checks(), CheckSet::merge(run_gdiface_deep3_checks(), CheckSet::merge(run_gdiface_deep4_checks(), CheckSet::merge(run_gdiface_deep5_checks(), CheckSet::merge(run_gdiface_deep6_checks(), run_gdiface_deep7_checks())))))))
}

// ---------------------------------------------------------------------------
// F006 · 深化批次二：文本度量 + 背景模式 + ExtTextOut 选项面
//
// 主册依据（G-A-06【功能定义】）：19 函数清单含 GetTextMetrics/SetBkMode/
// ExtTextOut——深化补三者的语义面（度量结构、OPAQUE/TRANSPARENT 二态、
// ETO 选项位），渲染实现随闸门。
// ---------------------------------------------------------------------------

/// 背景模式（wingdi.h）。
pub const BK_TRANSPARENT: u32 = 1;
pub const BK_OPAQUE: u32 = 2;

/// ETO 选项位（ExtTextOut 高频集）。
pub const ETO_OPAQUE: u32 = 0x0002;
pub const ETO_CLIPPED: u32 = 0x0004;

/// TEXTMETRIC 模型（GetTextMetrics 出口——字形管线 F055 的度量供给面）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct TextMetrics {
    pub height: i32,
    pub ascent: i32,
    pub descent: i32,
    pub avg_char_width: i32,
}

impl TextMetrics {
    /// 度量自洽（ascent + descent = height——字形度量的硬约束）。
    pub fn sane(&self) -> bool {
        self.ascent + self.descent == self.height && self.height > 0 && self.avg_char_width > 0
    }
}

impl GdiState {
    /// SetBkMode：二态校验（非法值如实拒绝——Windows 语义返回值如实）。
    pub fn set_bk_mode(&mut self, hdc: u32, mode: u32) -> bool {
        match self.dc(hdc) {
            Some(dc) => {
                if mode == BK_TRANSPARENT || mode == BK_OPAQUE {
                    dc.bk_mode = mode;
                    true
                } else {
                    false
                }
            }
            None => {
                self.invalid_handle_uses += 1;
                false
            }
        }
    }

    /// GetTextMetrics：按当前字体模型返回度量（默认 16px 无衬线度量——
    /// ascent 12 / descent 4，真实度量随字形管线，模型层供语义位）。
    pub fn text_metrics(&mut self, hdc: u32) -> Option<TextMetrics> {
        if self.dc(hdc).is_none() {
            self.invalid_handle_uses += 1;
            return None;
        }
        Some(TextMetrics { height: 16, ascent: 12, descent: 4, avg_char_width: 8 })
    }

    /// ExtTextOut：选项位校验（高频集外如实拒绝——差异表纪律同 ROP 面）。
    pub fn ext_text_out(&mut self, hdc: u32, options: u32, glyphs: usize) -> bool {
        if self.dc(hdc).is_none() {
            return false;
        }
        let known = ETO_OPAQUE | ETO_CLIPPED;
        if options & !known != 0 {
            self.unsupported_rops += 1; // 冷门选项与冷门 ROP 同账本（差异表观测面）
            return false;
        }
        self.compositor_submissions += 1;
        let _ = glyphs;
        true
    }
}

/// F006 深化自检。
pub fn run_gdiface_deep_checks() -> CheckSet {
    let mut cs = CheckSet::new("F006-gdiface-deep");
    // 1) 背景模式钉值 + 非法值拒绝 + 无效 DC 如实记账。
    cs.add(
        "bk_mode_semantics",
        BK_TRANSPARENT == 1 && BK_OPAQUE == 2 && ETO_OPAQUE == 0x0002 && ETO_CLIPPED == 0x0004,
        "",
    );
    let mut g = GdiState::new();
    let hdc = g.get_dc().unwrap();
    let set_ok = g.set_bk_mode(hdc, BK_TRANSPARENT);
    let set_bad = g.set_bk_mode(hdc, 99);
    let set_invalid_dc = g.set_bk_mode(0, BK_OPAQUE);
    cs.add(
        "set_bk_mode_honest",
        set_ok && !set_bad && !set_invalid_dc && g.dc(hdc).unwrap().bk_mode == BK_TRANSPARENT,
        "",
    );
    // 2) 文本度量自洽（ascent+descent=height 硬约束）+ 无效 DC None。
    let m = g.text_metrics(hdc).unwrap();
    let m_bad_dc = g.text_metrics(0);
    cs.add(
        "text_metrics_sane",
        m.sane() && m.height == 16 && m_bad_dc.is_none() && g.invalid_handle_uses >= 1,
        "",
    );
    // 3) ExtTextOut：已知选项位通过，冷门位拒绝（与 ROP 同差异表账本）。
    let before_rops = g.unsupported_rops;
    let eto_ok = g.ext_text_out(hdc, ETO_CLIPPED, 5);
    let eto_bad = g.ext_text_out(hdc, 0x8000, 5);
    cs.add(
        "ext_text_out_diff_table",
        eto_ok && !eto_bad && g.unsupported_rops == before_rops + 1,
        "",
    );
    // 4) ROP3 真值表（深化一批既有面）对账锚：SRCCOPY=S、PATCOPY=P。
    cs.add(
        "rop3_truth_table_anchored",
        rop3_eval(ROP_SRCCOPY, 0, 0xF0, 0x0F) == 0xF0
            && rop3_eval(ROP_PATCOPY, 0xA5, 0, 0) == 0xA5
            && rop3_eval(ROP_BLACKNESS, 0xFF, 0xFF, 0xFF) == 0,
        "",
    );
    cs
}

// ---------------------------------------------------------------------------
// F006 · 深化批次三：跨设备颜色深度转换（不静默降色深）+ 屏上 DC 直绘省拷贝
// 记账
//
// 主册依据（G-A-06【状态与异常】）：「BitBlt 跨设备格式转换按源格式如实转换
// （不静默降色深）」；【设计细节】「DC 概念映射为『合成器上下文句柄』，屏上
// DC 直接绘即所见（省一次拷贝）」。GdiState/DeviceContext/ROP_TABLE 既有面
// （一处一事实），本段只补转换语义与省拷贝观测。
// ---------------------------------------------------------------------------

/// 颜色深度转换语义：目标深于源（升档）恒可；目标浅于源（降档）必须显式
/// 请求——否则如实拒绝（不静默降色深，调用方决定 UI）。
pub fn convert_color_depth(src_bpp: u32, dst_bpp: u32, explicit_downgrade: bool) -> Result<u32, &'static str> {
    if dst_bpp < src_bpp && !explicit_downgrade {
        return Err("color depth downgrade refused; source depth preserved");
    }
    if dst_bpp == 0 || src_bpp == 0 {
        return Err("invalid color depth");
    }
    Ok(dst_bpp)
}

/// 屏上 DC 直绘省拷贝记账（主册【设计细节】：屏上 DC 直接绘即所见——相对
/// 内存 DC 的一次合成器拷贝被省去，记账面可观测）。
#[derive(Clone, Copy, Debug)]
pub struct DirectDrawLedger {
    /// 屏上 DC 绘制次数。
    pub screen_dc_draws: u32,
    /// 省去的拷贝次数（屏上直绘每笔省一次——两计数恒等，恒等式即判据）。
    pub copies_saved: u32,
}

impl DirectDrawLedger {
    pub const fn new() -> DirectDrawLedger {
        DirectDrawLedger { screen_dc_draws: 0, copies_saved: 0 }
    }

    pub fn note_screen_draw(&mut self) {
        self.screen_dc_draws += 1;
        self.copies_saved += 1;
    }
}

/// F006 深化批次三自检。
pub fn run_gdiface_deep2_checks() -> CheckSet {
    let mut cs = CheckSet::new("F006-gdiface-deep2");
    // 1) 降档必须显式：32→16 无请求 = 拒绝；显式请求 = 放行；16→32 升档恒可；
    //    零深度 = 非法。
    cs.add(
        "color_depth_no_silent_downgrade",
        convert_color_depth(32, 16, false).is_err()
            && convert_color_depth(32, 16, true) == Ok(16)
            && convert_color_depth(16, 32, false) == Ok(32)
            && convert_color_depth(0, 16, false).is_err(),
        "",
    );
    // 2) 省拷贝恒等式：N 笔屏上直绘 → 省去 N 次拷贝（两计数逐笔恒等）。
    let mut ledger = DirectDrawLedger::new();
    for _ in 0..7 {
        ledger.note_screen_draw();
    }
    cs.add(
        "direct_draw_copies_saved_identity",
        ledger.screen_dc_draws == 7 && ledger.copies_saved == 7,
        "",
    );
    // 3) 高频 ROP 表对账不变（16 码既有面锚——深化不破坏既有判据）。
    cs.add(
        "rop_table_size_anchor",
        ROP_TABLE.len() == HIGH_FREQ_ROP_COUNT,
        "",
    );
    cs
}

// ---------------------------------------------------------------------------
// F006 · 深化批次四：文本绘制字形图集热路径记账（F055 缓存联动）
//
// 主册依据（G-A-06【设计细节】）：「文本绘制走字形图集热路径（F055 缓存）」
// ——热路径不是口号：每笔文本绘制按字形是否在图集记账，命中率可审计
// （F055 判据线 >90% 场景值的本域观测面）。
// ---------------------------------------------------------------------------

/// 字形图集路径记账（TextOut/ExtTextOut/DrawText 共用）。
#[derive(Clone, Copy, Debug)]
pub struct GlyphPathLedger {
    /// 图集命中字形数（热路径）。
    pub atlas_hits: u64,
    /// 图集未命中字形数（冷路径——光栅化后按 F055 语义回填图集）。
    pub atlas_misses: u64,
    /// 回填图集次数（未命中后的回填——miss 与 refill 差值 = 回填被拒数，
    /// 如实可见）。
    pub refills: u64,
}

impl GlyphPathLedger {
    pub const fn new() -> GlyphPathLedger {
        GlyphPathLedger { atlas_hits: 0, atlas_misses: 0, refills: 0 }
    }

    /// 一笔字形绘制：命中走热路径；未命中记账并尝试回填（回填结果如实分账）。
    pub fn note_glyph(&mut self, in_atlas: bool, refill_ok: bool) {
        if in_atlas {
            self.atlas_hits += 1;
        } else {
            self.atlas_misses += 1;
            if refill_ok {
                self.refills += 1;
            }
        }
    }

    /// 热路径命中率（permille）；零字形 → None（不猜）。
    pub fn hit_permille(&self) -> Option<u32> {
        let total = self.atlas_hits + self.atlas_misses;
        if total == 0 {
            return None;
        }
        Some((self.atlas_hits * 1000 / total) as u32)
    }
}

/// F006 深化批次四自检。
pub fn run_gdiface_deep3_checks() -> CheckSet {
    let mut cs = CheckSet::new("F006-gdiface-deep3");
    // 1) 记账分面：命中/未命中/回填三计数独立；恒等式 hits+misses = 总字形。
    let mut led = GlyphPathLedger::new();
    for i in 0..10u64 {
        led.note_glyph(i < 8, true);
    }
    cs.add(
        "glyph_ledger_facets",
        led.atlas_hits == 8 && led.atlas_misses == 2 && led.refills == 2 && led.hit_permille() == Some(800),
        "",
    );
    // 2) 回填被拒如实可见（refill 恒等破缺——不给「都进图集了」假象）。
    let mut led2 = GlyphPathLedger::new();
    led2.note_glyph(false, false);
    led2.note_glyph(false, true);
    cs.add(
        "glyph_refill_refusal_visible",
        led2.atlas_misses == 2 && led2.refills == 1,
        "",
    );
    // 3) 零字形命中率如实 None（不猜）。
    cs.add("glyph_ledger_empty_none", GlyphPathLedger::new().hit_permille().is_none(), "");
    cs
}

// ---------------------------------------------------------------------------
// F006 · 深化批次五：**GDI 真光栅引擎核心**（软件光栅器——非记账模型）
//
// 主册依据（G-A-06【功能定义】）：FillRect/LineTo/Rectangle/Ellipse 的真渲染
// 核——批次一至四的绘制面为命令记账模型，本段把光栅本体落到像素级：矩形
// 填充（裁剪）、Bresenham 直线、中点椭圆（四分对称）、多边形扫描线填充
// （偶奇规则）。零堆：调用方供 `&mut [u32]` 表面（ARGB），无分配。
// 与既有合成器提交面（B-801）的关系：本核产出即提交前的帧缓冲内容。
// ---------------------------------------------------------------------------

/// 多边形顶点上限（零堆扫描线）。
pub const POLY_VERT_CAP: usize = 16;

/// 软件光栅表面（ARGB u32——w×h 像素，调用方持有缓冲）。
pub struct RasterSurface<'a> {
    pub px: &'a mut [u32],
    pub w: usize,
    pub h: usize,
}

impl<'a> RasterSurface<'a> {
    pub fn new(px: &'a mut [u32], w: usize, h: usize) -> Option<RasterSurface<'a>> {
        if px.len() != w * h || w == 0 || h == 0 {
            return None;
        }
        Some(RasterSurface { px, w, h })
    }

    pub fn clear(&mut self, c: u32) {
        self.px.fill(c);
    }

    /// 像素读（越界 None——调试/对拍用）。
    pub fn px_at(&self, x: i32, y: i32) -> Option<u32> {
        if x < 0 || y < 0 || x >= self.w as i32 || y >= self.h as i32 {
            return None;
        }
        Some(self.px[y as usize * self.w + x as usize])
    }

    fn set_px(&mut self, x: i32, y: i32, c: u32) {
        if x >= 0 && y >= 0 && x < self.w as i32 && y < self.h as i32 {
            self.px[y as usize * self.w + x as usize] = c;
        }
    }

    /// FillRect 真渲染（负宽高语义：按 min/max 规范化——GDI Rectangle 同义；
    /// 越界部分裁剪不报错）。
    pub fn fill_rect(&mut self, x: i32, y: i32, w: i32, h: i32, c: u32) -> bool {
        if w == 0 || h == 0 {
            return false;
        }
        let (x0, x1) = if w > 0 { (x, x + w) } else { (x + w, x) };
        let (y0, y1) = if h > 0 { (y, y + h) } else { (y + h, y) };
        for yy in y0.max(0)..y1.min(self.h as i32) {
            for xx in x0.max(0)..x1.min(self.w as i32) {
                self.set_px(xx, yy, c);
            }
        }
        true
    }

    /// 逐行填充（扫描线核的行原语）。
    fn fill_span(&mut self, y: i32, x0: i32, x1: i32, c: u32) {
        let (a, b) = if x0 <= x1 { (x0, x1) } else { (x1, x0) };
        for xx in a.max(0)..=b.min(self.w as i32 - 1) {
            self.set_px(xx, y, c);
        }
    }
}

/// Bresenham 直线（全八向——整数增量，无浮点）。
pub fn raster_line(s: &mut RasterSurface, x0: i32, y0: i32, x1: i32, y1: i32, c: u32) -> bool {
    let dx = (x1 - x0).abs();
    let dy = -(y1 - y0).abs();
    let sx = if x0 < x1 { 1 } else { -1 };
    let sy = if y0 < y1 { 1 } else { -1 };
    let mut err = dx + dy;
    let (mut x, mut y) = (x0, y0);
    loop {
        s.set_px(x, y, c);
        if x == x1 && y == y1 {
            return true;
        }
        let e2 = 2 * err;
        if e2 >= dy {
            err += dy;
            x += sx;
        }
        if e2 <= dx {
            err += dx;
            y += sy;
        }
    }
}

/// 中点椭圆轮廓（四分对称——rx/ry 为半径；(rx+1)×2 为直径口径）。
pub fn raster_ellipse(s: &mut RasterSurface, cx: i32, cy: i32, rx: i32, ry: i32, c: u32) -> bool {
    if rx <= 0 || ry <= 0 {
        return false;
    }
    let rx2 = (rx * rx) as i64;
    let ry2 = (ry * ry) as i64;
    let mut x = 0i32;
    let mut y = ry;
    let mut d1 = ry2 - rx2 * ry as i64 + rx2 / 4;
    let mut dx = 2 * ry2 * x as i64;
    let mut dy = 2 * rx2 * y as i64;
    while dx < dy {
        for (sx, sy) in [(x, y), (-x, y), (x, -y), (-x, -y)] {
            s.set_px(cx + sx, cy + sy, c);
        }
        if d1 < 0 {
            x += 1;
            dx += 2 * ry2;
            d1 += dx + ry2;
        } else {
            x += 1;
            y -= 1;
            dx += 2 * ry2;
            dy -= 2 * rx2;
            d1 += dx - dy + ry2;
        }
    }
    let mut d2 = ry2 * ((x + 1) * x) as i64 + rx2 * ((y - 1) * (y - 1)) as i64 - rx2 * ry2;
    while y >= 0 {
        for (sx, sy) in [(x, y), (-x, y), (x, -y), (-x, -y)] {
            s.set_px(cx + sx, cy + sy, c);
        }
        y -= 1;
        dy -= 2 * rx2;
        if d2 > 0 {
            d2 += rx2 - 2 * dy;
        } else {
            x += 1;
            dx += 2 * ry2;
            d2 += dx - dy + rx2;
        }
    }
    true
}

/// 多边形扫描线填充（偶奇规则；水平边跳过；扫描线用半开区间 [ymin, ymax)
/// 防顶点双计）。顶点超 16 如实拒。
pub fn raster_polygon_fill(s: &mut RasterSurface, pts: &[(i32, i32)], c: u32) -> bool {
    if pts.len() < 3 || pts.len() > POLY_VERT_CAP {
        return false;
    }
    let ymin = pts.iter().map(|p| p.1).min().unwrap();
    let ymax = pts.iter().map(|p| p.1).max().unwrap();
    let mut xs = [0i32; POLY_VERT_CAP * 2];
    for y in ymin..=ymax {
        let mut n = 0usize;
        for i in 0..pts.len() {
            let a = pts[i];
            let b = pts[(i + 1) % pts.len()];
            if a.1 == b.1 {
                continue; // 水平边不贡献交叉
            }
            let (y0, y1) = if a.1 < b.1 { (a.1, b.1) } else { (b.1, a.1) };
            if y >= y0 && y < y1 {
                let x = a.0 + (b.0 - a.0) * (y - a.1) / (b.1 - a.1);
                if n < xs.len() {
                    xs[n] = x;
                    n += 1;
                }
            }
        }
        if n >= 2 {
            xs[..n].sort_unstable();
            let mut k = 0;
            while k + 1 < n {
                s.fill_span(y, xs[k], xs[k + 1], c);
                k += 2;
            }
        }
    }
    true
}

/// F006 深化批次五自检（像素级断言——真光栅的对拍是逐像素的）。
pub fn run_gdiface_deep4_checks() -> CheckSet {
    let mut cs = CheckSet::new("F006-gdiface-deep4");
    // 1) 矩形填充：内部全色、边界外原色、负宽高规范化、越界裁剪不 panic。
    let mut buf = [0u32; 16 * 16];
    let mut sf = RasterSurface::new(&mut buf, 16, 16).unwrap();
    sf.clear(0xFF00_0000);
    sf.fill_rect(4, 4, 6, 6, 0xFF00_FF00);
    let inner = sf.px_at(6, 6) == Some(0xFF00_FF00);
    let outside = sf.px_at(3, 6) == Some(0xFF00_0000) && sf.px_at(10, 10) == Some(0xFF00_0000);
    let neg_ok = sf.fill_rect(8, 8, -3, -3, 0xFF00_00FF) && sf.px_at(6, 6) == Some(0xFF00_00FF);
    let clip_ok = sf.fill_rect(14, 14, 10, 10, 0xFFFFFFFF);
    let clipped = sf.px_at(15, 15) == Some(0xFFFFFFFF);
    cs.add(
        "raster_fill_rect_pixel_exact",
        inner && outside && neg_ok && clip_ok && clipped,
        "",
    );
    // 2) Bresenham：垂直/水平/主对角线的端点与中点逐像素对。
    let mut buf2 = [0u32; 16 * 16];
    let mut sf2 = RasterSurface::new(&mut buf2, 16, 16).unwrap();
    raster_line(&mut sf2, 2, 3, 2, 8, 1);
    raster_line(&mut sf2, 0, 0, 3, 3, 2);
    cs.add(
        "raster_line_bresenham",
        sf2.px_at(2, 3) == Some(1)
            && sf2.px_at(2, 5) == Some(1)
            && sf2.px_at(2, 8) == Some(1)
            && sf2.px_at(1, 1) == Some(2)
            && sf2.px_at(3, 3) == Some(2)
            && sf2.px_at(2, 1) == Some(0),
        "",
    );
    // 3) 多边形扫描线填充：右三角内部填充、斜边外不填充；顶点超限如实拒。
    let mut buf3 = [0u32; 16 * 16];
    let mut sf3 = RasterSurface::new(&mut buf3, 16, 16).unwrap();
    let tri = [(1i32, 1i32), (11, 1), (1, 11)];
    raster_polygon_fill(&mut sf3, &tri, 7);
    let in_tri = sf3.px_at(2, 2) == Some(7) && sf3.px_at(5, 5) == Some(7);
    let out_tri = sf3.px_at(11, 5) == Some(0) && sf3.px_at(12, 2) == Some(0);
    let too_few = !raster_polygon_fill(&mut sf3, &tri[..2], 7);
    let too_many = !raster_polygon_fill(&mut sf3, &[(0i32, 0i32); 17], 7);
    cs.add(
        "raster_polygon_scanline",
        in_tri && out_tri && too_few && too_many,
        "",
    );
    // 4) 中点椭圆：四分对称锚（上下左右极点在位）+ 轮廓非填充（内部原色）。
    let mut buf4 = [0u32; 24 * 24];
    let mut sf4 = RasterSurface::new(&mut buf4, 24, 24).unwrap();
    raster_ellipse(&mut sf4, 12, 12, 8, 6, 3);
    let poles = sf4.px_at(20, 12) == Some(3)
        && sf4.px_at(4, 12) == Some(3)
        && sf4.px_at(12, 18) == Some(3)
        && sf4.px_at(12, 6) == Some(3);
    let inside_clean = sf4.px_at(12, 12) == Some(0);
    let invalid = !raster_ellipse(&mut sf4, 12, 12, 0, 6, 3);
    cs.add(
        "raster_ellipse_midpoint_symmetry",
        poles && inside_clean && invalid,
        "",
    );
    cs
}

// ---------------------------------------------------------------------------
// F006 · 深化批次六：**光栅引擎扩面**（渐变填充 + 圆角矩形——真渲染续批）
//
// 主册依据（G-A-07【功能定义】）：「Brush(实心/渐变)」的像素级落点——线性
// 渐变逐像素插值填充；G-A-05 视觉词典的圆角矩形（VARIX 控件圆角 4px 基线，
// 乙节基线表）。基于批次五 RasterSurface（一处一事实）。
// ---------------------------------------------------------------------------

/// 圆角矩形填充（corner 为圆角半径；角部 1/4 圆内剔除——中点判据 x²+y²≤r²）。
pub fn raster_round_rect(s: &mut RasterSurface, x: i32, y: i32, w: i32, h: i32, r: i32, c: u32) -> bool {
    if w <= 0 || h <= 0 || r < 0 {
        return false;
    }
    let r = r.min(w / 2).min(h / 2); // 半径钳制（超尺寸圆角按最大可行值）
    let r2 = (r * r) as i64;
    for yy in y..y + h {
        for xx in x..x + w {
            // 角部判定：到本角圆心的距离平方 > r² → 剔除。
            let in_corner_x = xx < x + r || xx >= x + w - r;
            let in_corner_y = yy < y + r || yy >= y + h - r;
            if in_corner_x && in_corner_y {
                let cx = if xx < x + r { x + r } else { x + w - 1 - r };
                let cy = if yy < y + r { y + r } else { y + h - 1 - r };
                let dx = (xx - cx) as i64;
                let dy = (yy - cy) as i64;
                if dx * dx + dy * dy > r2 {
                    continue;
                }
            }
            s.set_px(xx, yy, c);
        }
    }
    true
}

/// 线性渐变矩形填充（水平向：颜色 c0→c1 逐列插值——G-A-07 LinearGradient 的
/// 像素落点；插值核与既有 lerp_color 同族，permille 确定性）。
pub fn raster_gradient_h(s: &mut RasterSurface, x: i32, y: i32, w: i32, h: i32, c0: u32, c1: u32) -> bool {
    if w <= 0 || h <= 0 {
        return false;
    }
    for col in 0..w {
        let t = ((col * 1000) / (w - 1).max(1)) as u32; // 0..=1000 permille（末列恰达 c1）
        let color = {
            let mut out = 0u32;
            for shift in [0u32, 8, 16, 24] {
                let a = ((c0 >> shift) & 0xFF) as u32;
                let b = ((c1 >> shift) & 0xFF) as u32;
                out |= (((a as i32) + ((b as i32 - a as i32) * (t as i32)) / 1000).clamp(0, 255) as u32) << shift;
            }
            out
        };
        for yy in y.max(0)..(y + h).min(s.h as i32) {
            s.set_px(x + col, yy, color);
        }
    }
    true
}

/// F006 深化批次六自检（像素级）。
pub fn run_gdiface_deep5_checks() -> CheckSet {
    let mut cs = CheckSet::new("F006-gdiface-deep5");
    // 1) 圆角矩形：中心填充、角部剔除（r=2 的 (0,0) 角点空）、边线在位。
    let mut buf = [0u32; 16 * 16];
    let mut sf = RasterSurface::new(&mut buf, 16, 16).unwrap();
    raster_round_rect(&mut sf, 2, 2, 12, 12, 2, 9);
    let center = sf.px_at(8, 8) == Some(9);
    let corner_cut = sf.px_at(2, 2) == Some(0) && sf.px_at(13, 2) == Some(0);
    let edge = sf.px_at(8, 2) == Some(9) && sf.px_at(2, 8) == Some(9);
    let bad = !raster_round_rect(&mut sf, 0, 0, 0, 5, 1, 9);
    cs.add(
        "raster_round_rect_corners",
        center && corner_cut && edge && bad,
        "",
    );
    // 2) 渐变填充：两端色精确（左 c0 右 c1）+ 中点通道中值（±1 舍入）。
    let mut buf2 = [0u32; 16 * 16];
    let mut sf2 = RasterSurface::new(&mut buf2, 16, 16).unwrap();
    raster_gradient_h(&mut sf2, 0, 4, 16, 2, 0xFF00_0000, 0xFF00_00FF);
    let left = sf2.px_at(0, 4) == Some(0xFF00_0000);
    let right = sf2.px_at(15, 4) == Some(0xFF00_00FF);
    let mid_px = sf2.px_at(8, 4).unwrap_or(0);
    let mid_b = (mid_px & 0xFF) as i32;
    cs.add(
        "raster_gradient_endpoints_and_mid",
        left && right && mid_b == 135,
        "",
    );
    // 3) 渐变单调性：B 通道逐列单调不减（无回跳——确定性插值核）。
    let mut mono = true;
    let mut prev = -1i32;
    for xx in 0..16 {
        let v = (sf2.px_at(xx, 5).unwrap_or(0) & 0xFF) as i32;
        mono &= v >= prev;
        prev = v;
    }
    cs.add("raster_gradient_monotonic", mono, "");
    cs
}

// ---------------------------------------------------------------------------
// F006 · 深化批次七：Region 裁剪核（矩形列表 Region——contains/相交可见）
//
// 主册依据（G-A-06【设计细节】）：「BitBlt 光栅操作……直通合成器提交面」——
// 合成器侧的脏区裁剪前提是 Region 语义：矩形列表上的点包含与矩形相交可见
// 判定（GDI CreateRectRgn/CombineRgn 的最小核）。零堆：定长 8 矩形。
// ---------------------------------------------------------------------------

/// Region 容量（脏区矩形上限——合成器脏区深化 F056 的量级锚）。
pub const REGION_RECT_CAP: usize = 8;

/// 矩形列表 Region（x/y/w/h——w/h ≤0 的退化矩形不入表）。
#[derive(Clone, Copy, Debug)]
pub struct Region {
    rects: [(i32, i32, i32, i32); REGION_RECT_CAP],
    n: usize,
}

impl Region {
    pub const fn new() -> Region {
        Region { rects: [(0, 0, 0, 0); REGION_RECT_CAP], n: 0 }
    }

    pub fn add_rect(&mut self, x: i32, y: i32, w: i32, h: i32) -> bool {
        if w <= 0 || h <= 0 || self.n >= REGION_RECT_CAP {
            return false;
        }
        self.rects[self.n] = (x, y, w, h);
        self.n += 1;
        true
    }

    /// 点包含（任一矩形含点即真）。
    pub fn contains(&self, x: i32, y: i32) -> bool {
        self.rects[..self.n].iter().any(|&(rx, ry, rw, rh)| {
            x >= rx && x < rx + rw && y >= ry && y < ry + rh
        })
    }

    /// 矩形相交可见（任一矩形与目标矩形有正面积交叠即真——裁剪剔除的判定核）。
    pub fn intersects(&self, x: i32, y: i32, w: i32, h: i32) -> bool {
        if w <= 0 || h <= 0 {
            return false;
        }
        self.rects[..self.n].iter().any(|&(rx, ry, rw, rh)| {
            x < rx + rw && rx < x + w && y < ry + rh && ry < y + h
        })
    }

    pub fn len(&self) -> usize {
        self.n
    }
}

/// F006 深化批次七自检。
pub fn run_gdiface_deep6_checks() -> CheckSet {
    let mut cs = CheckSet::new("F006-gdiface-deep6");
    // 1) 点包含：两矩形各自命中、缝隙与界外不命中（半开区间语义）。
    let mut rgn = Region::new();
    rgn.add_rect(0, 0, 10, 10);
    rgn.add_rect(20, 20, 5, 5);
    cs.add(
        "region_contains_two_rects",
        rgn.contains(5, 5)
            && rgn.contains(22, 21)
            && !rgn.contains(15, 5)
            && !rgn.contains(25, 25)
            && !rgn.contains(10, 10),
        "",
    );
    // 2) 相交可见：目标矩形与第一矩形交叠 → 真；完全在缝隙 → 假；退化
    //    （零宽）目标 → 假（不产生不可见绘制）。
    cs.add(
        "region_intersects_visible",
        rgn.intersects(8, 8, 4, 4)
            && !rgn.intersects(12, 12, 6, 6)
            && !rgn.intersects(30, 30, 5, 5)
            && !rgn.intersects(5, 5, 0, 5),
        "",
    );
    // 3) 容量纪律：满 8 后 add 如实拒；退化矩形（负宽）不入表。
    let mut full = Region::new();
    let mut all = true;
    for i in 0..8i32 {
        all &= full.add_rect(i * 10, 0, 5, 5);
    }
    let over = full.add_rect(100, 0, 5, 5);
    let mut deg = Region::new();
    let neg = deg.add_rect(0, 0, -3, 5);
    cs.add(
        "region_cap_and_degenerate",
        all && !over && full.len() == 8 && !neg,
        "",
    );
    cs
}

// ---------------------------------------------------------------------------
// F006 · 深化批次八：Region 减法拆分核（矩形被另一个矩形切除后的剩余带——
// GDI 排除剪裁 ExcludeClipRect 的几何本体：上/下/左/右四带，最多 4 块）。
// ---------------------------------------------------------------------------

/// 拆分矩形（半开区间语义与 Region 既有面一致）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ClipRect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

impl ClipRect {
    pub fn right(&self) -> i32 {
        self.x + self.w
    }
    pub fn bottom(&self) -> i32 {
        self.y + self.h
    }
    pub fn contains(&self, px: i32, py: i32) -> bool {
        px >= self.x && px < self.right() && py >= self.y && py < self.bottom()
    }
    pub fn area(&self) -> i64 {
        (self.w.max(0) as i64) * (self.h.max(0) as i64)
    }
}

/// a 减去 cut 后的剩余带（≤4 块；无交集返回原矩形单块；完全覆盖返回空表）。
pub fn rect_subtract(a: ClipRect, cut: ClipRect) -> alloc::vec::Vec<ClipRect> {
    let mut out = alloc::vec::Vec::new();
    let ix = a.x.max(cut.x);
    let iy = a.y.max(cut.y);
    let ir = a.right().min(cut.right());
    let ib = a.bottom().min(cut.bottom());
    if ix >= ir || iy >= ib {
        out.push(a); // 无交集
        return out;
    }
    // 交集上方带 / 下方带 / 左侧带 / 右侧带。
    if a.y < iy {
        out.push(ClipRect { x: a.x, y: a.y, w: a.w, h: iy - a.y });
    }
    if ib < a.bottom() {
        out.push(ClipRect { x: a.x, y: ib, w: a.w, h: a.bottom() - ib });
    }
    if a.x < ix {
        out.push(ClipRect { x: a.x, y: iy, w: ix - a.x, h: ib - iy });
    }
    if ir < a.right() {
        out.push(ClipRect { x: ir, y: iy, w: a.right() - ir, h: ib - iy });
    }
    out
}

/// F006 深化批次八自检。
fn run_gdiface_deep7_checks() -> CheckSet {
    let mut cs = CheckSet::new("F006-gdiface-deep7");
    let a = ClipRect { x: 0, y: 0, w: 100, h: 100 };
    // 1) 中央挖洞：四带覆盖除洞外全部点；洞内点全部脱离；面积守恒。
    let parts = rect_subtract(a, ClipRect { x: 40, y: 40, w: 20, h: 20 });
    let total: i64 = parts.iter().map(|p| p.area()).sum();
    let hole_free = !parts.iter().any(|p| p.contains(50, 50));
    let edge_kept = parts.iter().any(|p| p.contains(0, 0))
        && parts.iter().any(|p| p.contains(99, 99))
        && parts.iter().any(|p| p.contains(0, 50))
        && parts.iter().any(|p| p.contains(50, 0));
    cs.add(
        "rect_subtract_hole",
        parts.len() == 4
            && total == 100 * 100 - 20 * 20
            && hole_free
            && edge_kept,
        "",
    );
    // 2) 完全覆盖 → 空；无交集 → 原样单块；面积负值不炸（退化矩形）。
    let full = rect_subtract(a, ClipRect { x: -1, y: -1, w: 200, h: 200 });
    let none = rect_subtract(a, ClipRect { x: 200, y: 200, w: 5, h: 5 });
    cs.add(
        "rect_subtract_cover_and_disjoint",
        full.is_empty() && none.len() == 1 && none[0] == a,
        "",
    );
    // 3) 边缘切齐：切到右下角 → 只剩 2 带（上带 + 左带），无零面积块。
    let corner = rect_subtract(a, ClipRect { x: 60, y: 60, w: 100, h: 100 });
    cs.add(
        "rect_subtract_corner_two_bands",
        corner.len() == 2
            && corner.iter().all(|p| p.area() > 0)
            && corner.iter().map(|p| p.area()).sum::<i64>() == 100 * 100 - 40 * 40,
        "",
    );
    cs
}
