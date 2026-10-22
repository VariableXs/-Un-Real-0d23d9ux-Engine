//! UNX-F21610 · 调色板像素展开（8bpp 及以下索引色）
//!
//! 诊断段 0x91149~0x91150（F2-B40 域收官段 21/100，八码）。
//!
//! 判据原文（施工书 UNX-F21610-J1）：1/4/8bpp 索引经调色板展开为 32bpp，
//! 索引越界显性拒绝。
//!
//! 设计要点（为什么不是「查表 + 越界钳到 0」）：
//! 钳位是这类展开最常见的事故源：越界索引被折成调色板首项，画面看起来
//! 「有颜色」，缺陷活到出图之后才暴露，且再也没法定位到像素。故本模块把
//! 越界做成 `Err`，并带上行列与原始索引——错误必须指向一个能改的位置。
//!
//! 调色板的「有效项数」与 `entries.len()` 分开存：GDI 的 LOGPALETTE 里
//! `palNumEntries` 才是真实项数，结构体尾部常带上一轮残留。若按 len 判边界，
//! 残留项会被当成合法颜色，越界就从「报错」退化成「取到一个垃圾色」。
//!
//! 三种位宽的抽取各写一处、不抽公共移位器：1bpp 是 MSB-first、4bpp 是
//! 「偶列取高半字节」、8bpp 直取。硬套一个通用公式最容易把 1bpp 的位序写反，
//! 而写反之后画面只是「镜像的图标」，肉眼几乎判定不出来。

use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// 调色板上限（DDB 索引色最深 8bpp）。
pub const PALETTE_MAX: usize = 256;

/// 每像素展开步数：一次索引抽取 + 一次调色板取色。
///
/// 写成常量而不是事后累加：步数是对外承诺的性能口径，口径不随实现挪动，
/// 性能回归才测得出来；若口径跟着实现漂，两边一起变，永远对得上。
pub const STEPS_PER_PIXEL: u32 = 2;

/// 支持的位深（闭集，顺序即由浅到深）。
pub const SUPPORTED_BPPS: [u8; 3] = [1, 4, 8];

/// 索引色展开诊断码闭集（0x91149~0x91150 八码）。
///
/// 显式写判别式而不用隐式递增：码值是跨模块日志的对账键，插入一个变体会让
/// 后续码整体漂移，历史日志再也归不了因。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PalCode {
    /// 0x91149 展开成功。
    Ok = 0x91149,
    /// 0x9114A 位深不在 1/4/8 闭集内。
    BadDepth = 0x9114A,
    /// 0x9114B 调色板项数已达 256 仍被塞入。
    PaletteFull = 0x9114B,
    /// 0x9114C 索引超出调色板有效项数。
    IndexShort = 0x9114C,
    /// 0x9114D 索引超出该位深可表达的索引跨度。
    IndexOverBpp = 0x9114D,
    /// 0x9114E 行距与位宽不匹配（被篡改或按错公式算）。
    StrideBad = 0x9114E,
    /// 0x9114F 像素缓冲区短于行距 × 高。
    BufferShort = 0x9114F,
    /// 0x91150 行距或总字节数溢出 32 位。
    SpanOverflow = 0x91150,
}

impl PalCode {
    /// 诊断三要素之一：现象（发生了什么）。
    pub const fn what(self) -> &'static str {
        match self {
            PalCode::Ok => "索引色展开成功",
            PalCode::BadDepth => "位深不在闭集内",
            PalCode::PaletteFull => "调色板已满仍被追加",
            PalCode::IndexShort => "索引越过调色板有效项数",
            PalCode::IndexOverBpp => "索引越过位深跨度",
            PalCode::StrideBad => "行距与位宽不匹配",
            PalCode::BufferShort => "像素缓冲区不足",
            PalCode::SpanOverflow => "行距或总字节数溢出",
        }
    }
    /// 诊断三要素之二：原因（为什么会这样）。
    pub const fn why(self) -> &'static str {
        match self {
            PalCode::Ok => "索引全落在有效项内且位序按 DDB 约定抽取",
            PalCode::BadDepth => "调用方传了 2/16/24bpp 等本引擎不走的路径",
            PalCode::PaletteFull => "palNumEntries 被写成大于 256",
            PalCode::IndexShort => "像素索引超过了调色板声明项数",
            PalCode::IndexOverBpp => "索引字段高位带着上一行残留",
            PalCode::StrideBad => "行距按未补齐公式算出或结构体被就地改写",
            PalCode::BufferShort => "像素数据被截断，或高宽与实际缓冲不一致",
            PalCode::SpanOverflow => "宽度乘位深超过了 u32 位图坐标的口径",
        }
    }
    /// 诊断三要素之三：建议（下一步做什么）。
    pub const fn advice(self) -> &'static str {
        match self {
            PalCode::Ok => "转入 BitBlt 主路径",
            PalCode::BadDepth => "先经位深转换再调用展开",
            PalCode::PaletteFull => "裁剪调色板或改走直彩路径",
            PalCode::IndexShort => "补齐调色板尾部项，或确认像素是否被越界写坏",
            PalCode::IndexOverBpp => "清空索引高位，按该位深掩码重取",
            PalCode::StrideBad => "按 ((w*bpp+31)/32)*4 重算行距",
            PalCode::BufferShort => "按行距 × 高重新分配像素缓冲",
            PalCode::SpanOverflow => "把位图切块或降到更浅位深",
        }
    }
    /// 数值形式（日志/判据对账用）。
    ///
    /// 返回 u32 而非 u16：五位数码段（0x91149）超出 u16 值域，`as u16` 会静默
    /// 截断成 0x1149，看着像低段另一个模块的码。
    pub const fn code(self) -> u32 {
        self as u32
    }
}

/// 边标号：故障落在哪条边上（越界/裁剪定位用）。
pub const EDGE_NONE: u8 = 0;
/// 右边（x 越界）。
pub const EDGE_RIGHT: u8 = 3;
/// 下边（y 越界）。
pub const EDGE_BOTTOM: u8 = 4;

/// 展开故障：码 + 像素定位 + 原始索引 + 边标。
///
/// 三个定位字段一起带出，是因为「越界」有两种完全不同的修法：索引超过调色板
/// 项数要补表，索引超过位深跨度要清高位。只看码分不出，必须看索引与位深的关系。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct PalFault {
    /// 诊断码（u32：五位数码段）。
    pub code: u32,
    /// 行号（0 起；与像素无关的故障为 0）。
    pub row: u16,
    /// 列号（0 起）。
    pub col: u16,
    /// 触发故障的原始索引。
    pub index: u16,
    /// 边标号（仅越界类故障有意义）。
    pub edge: u8,
}

impl PalFault {
    /// 与像素无关的故障（位深、容量、溢出）。
    pub const fn plain(code: PalCode) -> PalFault {
        PalFault {
            code: code.code(),
            row: 0,
            col: 0,
            index: 0,
            edge: EDGE_NONE,
        }
    }

    /// 与像素相关的故障。
    pub const fn at(code: PalCode, row: u16, col: u16, index: u16) -> PalFault {
        PalFault {
            code: code.code(),
            row,
            col,
            index,
            edge: EDGE_NONE,
        }
    }

    /// 打上边标（行列已填好后只补边）。
    pub const fn on_edge(mut self, edge: u8) -> PalFault {
        self.edge = edge;
        self
    }

    /// 把码还原成枚举，便于取三要素文本。
    ///
    /// 闭集外的值不 panic：落到 `SpanOverflow` 作为「未知即可疑」的保守落点，
    /// 另有 `is_known()` 让调用方识别未知码。
    pub const fn as_code(self) -> PalCode {
        match self.code {
            0x91149 => PalCode::Ok,
            0x9114A => PalCode::BadDepth,
            0x9114B => PalCode::PaletteFull,
            0x9114C => PalCode::IndexShort,
            0x9114D => PalCode::IndexOverBpp,
            0x9114E => PalCode::StrideBad,
            0x9114F => PalCode::BufferShort,
            _ => PalCode::SpanOverflow,
        }
    }

    /// 码是否落在闭集内。
    pub const fn is_known(self) -> bool {
        self.code >= 0x91149 && self.code <= 0x91150
    }

    /// 单行诊断文本（字段顺序固定，便于日志 diff）。
    pub fn describe(self) -> String {
        let mut s = String::with_capacity(64);
        s.push_str(self.as_code().what());
        s.push_str(" row=");
        s.push_str(&self.row.to_string());
        s.push_str(" col=");
        s.push_str(&self.col.to_string());
        s.push_str(" idx=");
        s.push_str(&self.index.to_string());
        s.push_str(" code=0x");
        s.push_str(&hex_u32(self.code));
        s
    }
}

/// 单个颜色分量（BGRA 字节序，与 Windows RGBQUAD 的内存布局一致）。
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Bgra {
    /// 蓝。
    pub b: u8,
    /// 绿。
    pub g: u8,
    /// 红。
    pub r: u8,
    /// 保留/alpha（DDB 调色板里恒 0，展开后补 0xFF 表示不透明）。
    pub a: u8,
}

impl Bgra {
    /// 全字段构造。
    pub const fn new(b: u8, g: u8, r: u8, a: u8) -> Bgra {
        Bgra { b, g, r, a }
    }

    /// 按 RGB 构造不透明色（参数顺序与 `packed()` 的通道次序对齐）。
    pub const fn opaque(r: u8, g: u8, b: u8) -> Bgra {
        Bgra { b, g, r, a: 0xFF }
    }

    /// 打包成 32bpp 像素值（小端：低字节蓝）。
    pub const fn packed(self) -> u32 {
        (self.b as u32) | ((self.g as u32) << 8) | ((self.r as u32) << 16) | ((self.a as u32) << 24)
    }

    /// 从 32bpp 像素值解开。
    pub const fn from_packed(v: u32) -> Bgra {
        Bgra {
            b: (v & 0xFF) as u8,
            g: ((v >> 8) & 0xFF) as u8,
            r: ((v >> 16) & 0xFF) as u8,
            a: ((v >> 24) & 0xFF) as u8,
        }
    }
}

/// 调色板：有效项数与底层容量分开记账。
///
/// `entries` 允许比 `used` 长（尾部是残留），但**取色只认 `used`**。这是本模块
/// 唯一一条不能被「顺手优化掉」的约束：按 len 取色会在残留项上静默返回垃圾色。
pub struct Palette {
    entries: Vec<Bgra>,
    used: u16,
}

impl Palette {
    /// 空调色板（零有效项：任何索引都必须被拒）。
    pub fn new() -> Palette {
        Palette {
            entries: Vec::with_capacity(PALETTE_MAX),
            used: 0,
        }
    }

    /// 追加一项，返回其索引。
    pub fn push(&mut self, c: Bgra) -> Result<u16, PalFault> {
        if self.entries.len() >= PALETTE_MAX || self.used as usize >= PALETTE_MAX {
            return Err(PalFault::plain(PalCode::PaletteFull));
        }
        self.entries.push(c);
        let idx = self.used;
        self.used = self.used.saturating_add(1);
        Ok(idx)
    }

    /// 有效项数（`palNumEntries` 的语义）。
    pub const fn used(&self) -> u16 {
        self.used
    }

    /// 底层已分配项数。与 `used()` 的差值就是「尾部残留」。
    pub fn reserved(&self) -> usize {
        self.entries.len()
    }

    /// 批量填灰度阶：第 i 项为 (i,i,i,0xFF)。
    ///
    /// 灰度阶直接取整数等值通道，不引入浮点；8bpp 全表就是 0..=255 的等值三元组。
    pub fn fill_gray(&mut self, n: u16) -> Result<(), PalFault> {
        let mut i: u16 = 0;
        while i < n {
            let v = (i & 0xFF) as u8;
            self.push(Bgra::opaque(v, v, v))?;
            i += 1;
        }
        Ok(())
    }

    /// 取色：先按位深卡索引跨度，再按有效项数卡表长。
    ///
    /// 顺序不能反：索引 4 落在 1bpp 上属于「索引字段坏了」（结构错），
    /// 落在 8bpp 且表长 4 上属于「表没配齐」（数据错）。两种修法不同，
    /// 所以结构错必须先报。
    pub fn lookup(&self, index: u16, bpp: u8) -> Result<Bgra, PalFault> {
        let span = bpp_span(bpp)?;
        if index as u32 >= span {
            return Err(PalFault::at(PalCode::IndexOverBpp, 0, 0, index));
        }
        if index >= self.used {
            return Err(PalFault::at(PalCode::IndexShort, 0, 0, index));
        }
        Ok(self.entries[index as usize])
    }
}

impl Default for Palette {
    fn default() -> Self {
        Self::new()
    }
}

/// 位深对应的索引跨度（可表达索引个数）。
pub fn bpp_span(bpp: u8) -> Result<u32, PalFault> {
    match bpp {
        1 => Ok(2),
        4 => Ok(16),
        8 => Ok(256),
        _ => Err(PalFault::plain(PalCode::BadDepth)),
    }
}

/// 位深是否在闭集内。
pub fn is_supported(bpp: u8) -> bool {
    let mut i = 0usize;
    while i < SUPPORTED_BPPS.len() {
        if SUPPORTED_BPPS[i] == bpp {
            return true;
        }
        i += 1;
    }
    false
}

/// 索引色位图（DDB 布局：行按 4 字节对齐，位在字节内 MSB-first）。
pub struct IndexedBitmap {
    /// 宽（像素）。
    pub width: u32,
    /// 高（像素）。
    pub height: u32,
    /// 位深。
    pub bpp: u8,
    /// 行距（字节）。
    pub stride: u32,
    /// 像素数据（含行尾补齐字节）。
    pub data: Vec<u8>,
}

impl IndexedBitmap {
    /// 按 DDB 公式算行距：`((w*bpp + 31) / 32) * 4`。
    ///
    /// 中间量用 u64：`w*bpp` 在 u32 上会绕回成一个「看起来合法的小行距」，
    /// 随后按小行距算出的缓冲长度也小，越界读被掩盖成「读到下一行」，不报错。
    pub fn stride_for(width: u32, bpp: u8) -> Result<u32, PalFault> {
        if !is_supported(bpp) {
            return Err(PalFault::plain(PalCode::BadDepth));
        }
        let bits = (width as u64) * (bpp as u64);
        if bits > 0xFFFF_FFFFu64 {
            return Err(PalFault::plain(PalCode::SpanOverflow));
        }
        let padded = match (bits as u32).checked_add(31) {
            Some(v) => v,
            None => return Err(PalFault::plain(PalCode::SpanOverflow)),
        };
        Ok((padded / 32) * 4)
    }

    /// 按宽高与位深构造，行距自动推导，缓冲长度必须够。
    pub fn new(width: u32, height: u32, bpp: u8, data: Vec<u8>) -> Result<IndexedBitmap, PalFault> {
        let stride = IndexedBitmap::stride_for(width, bpp)?;
        let need = match (stride as usize).checked_mul(height as usize) {
            Some(v) => v,
            None => return Err(PalFault::plain(PalCode::BufferShort)),
        };
        if data.len() < need {
            return Err(PalFault::plain(PalCode::BufferShort));
        }
        Ok(IndexedBitmap {
            width,
            height,
            bpp,
            stride,
            data,
        })
    }

    /// 全零位图（判据夹具与占位用）。
    pub fn new_zeroed(width: u32, height: u32, bpp: u8) -> Result<IndexedBitmap, PalFault> {
        let stride = IndexedBitmap::stride_for(width, bpp)?;
        let need = match (stride as usize).checked_mul(height as usize) {
            Some(v) => v,
            None => return Err(PalFault::plain(PalCode::BufferShort)),
        };
        let mut data = Vec::with_capacity(need);
        let mut i = 0usize;
        while i < need {
            data.push(0u8);
            i += 1;
        }
        Ok(IndexedBitmap {
            width,
            height,
            bpp,
            stride,
            data,
        })
    }

    /// 强行指定行距（**不做一致性校验**）。
    ///
    /// 存在的唯一理由是造「行距被篡改」的样本：若这里顺手把行距纠正了，
    /// 展开器里的行距闸门就永远走不到，闸门是否有效也无从证实。
    pub fn with_stride(
        width: u32,
        height: u32,
        bpp: u8,
        stride: u32,
        data: Vec<u8>,
    ) -> IndexedBitmap {
        IndexedBitmap {
            width,
            height,
            bpp,
            stride,
            data,
        }
    }

    /// 写一个索引（夹具用；越界与位深跨度都校验）。
    pub fn set_index(&mut self, x: u32, y: u32, idx: u8) -> Result<(), PalFault> {
        if x >= self.width {
            return Err(PalFault::at(PalCode::BufferShort, y as u16, x as u16, idx as u16)
                .on_edge(EDGE_RIGHT));
        }
        if y >= self.height {
            return Err(PalFault::at(PalCode::BufferShort, y as u16, x as u16, idx as u16)
                .on_edge(EDGE_BOTTOM));
        }
        let span = bpp_span(self.bpp)?;
        if (idx as u32) >= span {
            return Err(PalFault::at(
                PalCode::IndexOverBpp,
                y as u16,
                x as u16,
                idx as u16,
            ));
        }
        let base = (self.stride as usize) * (y as usize);
        match self.bpp {
            1 => {
                let byte = base + (x as usize) / 8;
                let bit = 7 - (x % 8) as u32;
                if idx & 1 == 1 {
                    self.data[byte] |= 1u8 << bit;
                } else {
                    self.data[byte] &= !(1u8 << bit);
                }
            }
            4 => {
                let byte = base + (x as usize) / 2;
                if x & 1 == 0 {
                    self.data[byte] = (self.data[byte] & 0x0F) | ((idx & 0x0F) << 4);
                } else {
                    self.data[byte] = (self.data[byte] & 0xF0) | (idx & 0x0F);
                }
            }
            _ => {
                let byte = base + (x as usize);
                self.data[byte] = idx;
            }
        }
        Ok(())
    }

    /// 读一个索引（夹具自检用）。
    pub fn get_index(&self, x: u32, y: u32) -> Result<u8, PalFault> {
        if x >= self.width {
            return Err(PalFault::at(PalCode::BufferShort, y as u16, x as u16, 0).on_edge(EDGE_RIGHT));
        }
        if y >= self.height {
            return Err(PalFault::at(PalCode::BufferShort, y as u16, x as u16, 0).on_edge(EDGE_BOTTOM));
        }
        Ok(self.raw_index(x, y))
    }

    /// 无校验的索引抽取（展开主循环内用：宽高已在入口验过）。
    fn raw_index(&self, x: u32, y: u32) -> u8 {
        let base = (self.stride as usize) * (y as usize);
        match self.bpp {
            1 => {
                let byte = self.data[base + (x as usize) / 8];
                let bit = 7 - (x % 8) as u32;
                (byte >> bit) & 1
            }
            4 => {
                let byte = self.data[base + (x as usize) / 2];
                if x & 1 == 0 {
                    (byte >> 4) & 0x0F
                } else {
                    byte & 0x0F
                }
            }
            _ => self.data[base + (x as usize)],
        }
    }
}

/// 展开结果画布：32bpp 像素按行紧密排列（无行尾补齐——显示侧按 u32 取）。
pub struct Rgba32 {
    /// 宽。
    pub width: u32,
    /// 高。
    pub height: u32,
    /// 像素（BGRA 打包）。
    pub px: Vec<u32>,
}

impl Rgba32 {
    /// 全零画布。
    pub fn new(width: u32, height: u32) -> Rgba32 {
        let n = (width as usize).saturating_mul(height as usize);
        let mut px = Vec::with_capacity(n);
        let mut i = 0usize;
        while i < n {
            px.push(0u32);
            i += 1;
        }
        Rgba32 { width, height, px }
    }

    /// 取像素（越界返回 0 而不 panic：判据要用它做「不该被写」的对照）。
    pub fn at(&self, x: u32, y: u32) -> u32 {
        if x >= self.width || y >= self.height {
            return 0;
        }
        let i = (y as usize) * (self.width as usize) + (x as usize);
        self.px[i]
    }

    /// 画布指纹（FNV-1a 64 逐像素咬合）。
    ///
    /// 逐像素喂而不是喂 `px` 的原始字节：`Vec` 容量可能大于长度，容量尾部的
    /// 未初始化字节会让指纹在两次分配之间漂移，幂等判据就变成假阴性。
    pub fn fingerprint(&self) -> u64 {
        let mut h = 0xcbf2_9ce4_8422_2325u64;
        let mut i = 0usize;
        while i < self.px.len() {
            h = fnv_u64(h, self.px[i] as u64);
            i += 1;
        }
        h
    }

    /// 非零像素计数（空白画布检测）。
    pub fn ink(&self) -> u32 {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < self.px.len() {
            if self.px[i] != 0 {
                n = n.saturating_add(1);
            }
            i += 1;
        }
        n
    }
}

/// 展开账目。
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct ExpandStats {
    /// 写出的像素数。
    pub pixels: u32,
    /// 调色板取色次数（与像素数相等是可对拍的不变量）。
    pub lookups: u32,
    /// 总步数。
    pub steps: u32,
}

impl ExpandStats {
    /// 步数口径自洽性：步数必须等于像素数 × 每像素步数。
    pub const fn balanced(&self) -> bool {
        self.pixels == self.lookups && self.steps == self.pixels.saturating_mul(STEPS_PER_PIXEL)
    }
}

/// 展开结果。
pub struct Expansion {
    /// 32bpp 画布。
    pub bitmap: Rgba32,
    /// 账目。
    pub stats: ExpandStats,
}

/// 索引色 → 32bpp 展开主路径。
///
/// 校验顺序：位深 → 行距 → 缓冲长度 → 逐像素取色。前三步是纯标量开销，
/// 放在循环外，保证失败样本不会先跑一半再报错——半途失败会留下写了一半的
/// 画布，调用方若把 Err 当告警吞掉，屏幕上就是一张残图。
pub fn expand(src: &IndexedBitmap, pal: &Palette) -> Result<Expansion, PalFault> {
    if !is_supported(src.bpp) {
        return Err(PalFault::plain(PalCode::BadDepth));
    }
    let expect = IndexedBitmap::stride_for(src.width, src.bpp)?;
    if src.stride != expect {
        return Err(PalFault::plain(PalCode::StrideBad));
    }
    let need = (src.stride as usize).saturating_mul(src.height as usize);
    if src.data.len() < need {
        return Err(PalFault::plain(PalCode::BufferShort));
    }
    let mut out = Rgba32::new(src.width, src.height);
    let mut stats = ExpandStats::default();
    let mut y = 0u32;
    while y < src.height {
        let mut x = 0u32;
        while x < src.width {
            let idx = src.raw_index(x, y) as u16;
            let color = match pal.lookup(idx, src.bpp) {
                Ok(c) => c,
                Err(mut f) => {
                    // 把行/列补进故障：调色板不知道自己被哪个像素问到。
                    f.row = y as u16;
                    f.col = x as u16;
                    f.index = idx;
                    return Err(f);
                }
            };
            let i = (y as usize) * (src.width as usize) + (x as usize);
            out.px[i] = color.packed();
            stats.pixels = stats.pixels.saturating_add(1);
            stats.lookups = stats.lookups.saturating_add(1);
            stats.steps = stats.steps.saturating_add(STEPS_PER_PIXEL);
            x += 1;
        }
        y += 1;
    }
    Ok(Expansion {
        bitmap: out,
        stats,
    })
}

/// FNV-1a 64 单步（质数与偏移基写死，来源：FNV 规范 1a 变体）。
pub const fn fnv_u64(mut h: u64, v: u64) -> u64 {
    let mut i = 0u32;
    while i < 8 {
        let byte = ((v >> (i * 8)) & 0xFF) as u8;
        h ^= byte as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
        i += 1;
    }
    h
}

/// 32 位十六进制（no_std 手写：诊断文本不引 format!，避免「分配里套分配」）。
pub fn hex_u32(v: u32) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut s = String::with_capacity(8);
    let mut shift = 28i32;
    while shift >= 0 {
        let nib = ((v >> shift) & 0xF) as usize;
        s.push(HEX[nib] as char);
        shift -= 4;
    }
    s
}

/// 线性同余发生器（种子写死，参数取 Numerical Recipes 常用三元组）。
///
/// 判据需要「看起来无规律」的索引流，但绝不能引入时钟或系统随机：同一种子
/// 必须每次跑出同一张图，否则逐像素对拍不可重放。
pub struct Lcg {
    state: u32,
}

impl Lcg {
    /// 固定种子构造。
    pub const fn new(seed: u32) -> Lcg {
        Lcg { state: seed }
    }
    /// 下一个 32 位数。
    pub fn next_u32(&mut self) -> u32 {
        self.state = self
            .state
            .wrapping_mul(1_664_525)
            .wrapping_add(1_013_904_223);
        self.state
    }
    /// 取 `[0, span)` 内的值。
    pub fn next_below(&mut self, span: u32) -> u32 {
        if span == 0 {
            return 0;
        }
        self.next_u32() % span
    }
    /// 取一个字节（高位：低 8 位的周期性明显更强）。
    pub fn next_byte(&mut self) -> u8 {
        (self.next_u32() >> 24) as u8
    }
}

/// 夹具种子（由单号派生，写死以保证可重放）。
pub const LCG_SEED: u32 = 0x2161_0001;

/// 双色表：0 黑 / 1 白（1bpp 的标准配置）。
pub fn two_tone() -> Result<Palette, PalFault> {
    let mut p = Palette::new();
    p.push(Bgra::opaque(0x00, 0x00, 0x00))?;
    p.push(Bgra::opaque(0xFF, 0xFF, 0xFF))?;
    Ok(p)
}

/// 16 阶灰度表（4bpp 的标准配置）。
pub fn gray16() -> Result<Palette, PalFault> {
    let mut p = Palette::new();
    p.fill_gray(16)?;
    Ok(p)
}

/// 256 阶灰度表（8bpp 的标准配置）。
pub fn gray256() -> Result<Palette, PalFault> {
    let mut p = Palette::new();
    p.fill_gray(256)?;
    Ok(p)
}

/// 标准 1bpp 位图：8×2，第一行 0xAA（1010…），第二行 0xCC（1100…）。
pub fn standard_1bpp() -> Result<IndexedBitmap, PalFault> {
    let mut bm = IndexedBitmap::new_zeroed(8, 2, 1)?;
    let mut x = 0u32;
    while x < 8 {
        let top = if x % 2 == 0 { 1 } else { 0 };
        bm.set_index(x, 0, top)?;
        let bottom = if (x / 2) % 2 == 0 { 1 } else { 0 };
        bm.set_index(x, 1, bottom)?;
        x += 1;
    }
    Ok(bm)
}

/// 标准 4bpp 位图：4×2，第一行 0,1,2,3，第二行 15,14,13,12。
pub fn standard_4bpp() -> Result<IndexedBitmap, PalFault> {
    let mut bm = IndexedBitmap::new_zeroed(4, 2, 4)?;
    let mut x = 0u32;
    while x < 4 {
        bm.set_index(x, 0, (x as u8) & 0x0F)?;
        bm.set_index(x, 1, 15 - ((x as u8) & 0x0F))?;
        x += 1;
    }
    Ok(bm)
}

/// 标准 8bpp 位图：16×4，索引由写死种子的 LCG 生成。
pub fn standard_8bpp() -> Result<IndexedBitmap, PalFault> {
    let mut bm = IndexedBitmap::new_zeroed(16, 4, 8)?;
    let mut rng = Lcg::new(LCG_SEED);
    let mut y = 0u32;
    while y < 4 {
        let mut x = 0u32;
        while x < 16 {
            let idx = rng.next_below(256) as u8;
            bm.set_index(x, y, idx)?;
            x += 1;
        }
        y += 1;
    }
    Ok(bm)
}

/// 空位图（夹具构造失败时的占位：保持判据能继续跑完并留下红项）。
fn empty_bitmap(bpp: u8) -> IndexedBitmap {
    IndexedBitmap::with_stride(0, 0, bpp, 0, Vec::new())
}

/// 全零缓冲（造「行距/长度被篡改」的样本）。
fn zeros(n: usize) -> Vec<u8> {
    let mut v = Vec::with_capacity(n);
    let mut i = 0usize;
    while i < n {
        v.push(0u8);
        i += 1;
    }
    v
}

/// 标准调色板第 i 项的打包值（越界返回 0，不 panic）。
fn gray_of(i: u16) -> u32 {
    Bgra::opaque(i as u8, i as u8, i as u8).packed()
}

/// 判据入口（UNX-F21610-J1 及其配套十四条）。
///
/// 判据常量独立写死：期望像素值、行距、步数全用字面量，不复用被测函数的返回值，
/// 否则实现错了判据会跟着错，等于没有判据。
pub fn run_unx21610_palettexpand_checks() -> crate::checks::CheckSet {
    let mut cs = crate::checks::CheckSet::new("unx-f21610");
    let mono = two_tone();
    let g16 = gray16();
    let g256 = gray256();
    let one = standard_1bpp();
    let four = standard_4bpp();
    let eight = standard_8bpp();

    // 1) 夹具可构造：先证夹具，再证被测逻辑。
    {
        let roundtrip = Bgra::from_packed(Bgra::new(1, 2, 3, 4).packed());
        cs.add(
            "f21610.fixture_buildable",
            mono.is_ok()
                && g16.is_ok()
                && g256.is_ok()
                && one.is_ok()
                && four.is_ok()
                && eight.is_ok()
                && SUPPORTED_BPPS.len() == 3
                && roundtrip == Bgra::new(1, 2, 3, 4),
            "标准调色板/位图构造失败，或 BGRA 打包往返不一致",
        );
    }

    // 2) 调色板容量：256 项满表可建，第 257 项必拒。
    {
        let mut p = Palette::new();
        let full = p.fill_gray(256);
        let over = p.push(Bgra::opaque(1, 2, 3)).err().map(|f| f.code);
        cs.add(
            "f21610.palette_capacity",
            full.is_ok()
                && p.used() == 256
                && p.reserved() == 256
                && over == Some(0x9114B)
                && p.fill_gray(1).err().map(|f| f.code) == Some(0x9114B),
            "256 项未被接纳或第 257 项未被拒（应 0x9114B）",
        );
        // 空表边界：零有效项时任何索引都必须被拒，不能取到 entries[0]。
        let empty = Palette::new();
        let c = empty.lookup(0, 8).err().map(|f| f.code);
        cs.add(
            "f21610.empty_palette_rejected",
            empty.used() == 0 && c == Some(0x9114C),
            "空调色板的索引 0 未被拒（应 0x9114C）",
        );
    }

    // 3) 位深闸门：2/3/16bpp 全部拒绝，拒绝码固定。
    {
        let a = IndexedBitmap::stride_for(8, 2).err().map(|f| f.code);
        let b = IndexedBitmap::stride_for(8, 3).err().map(|f| f.code);
        let c = IndexedBitmap::stride_for(8, 16).err().map(|f| f.code);
        let d = bpp_span(2).err().map(|f| f.code);
        cs.add(
            "f21610.depth_gate",
            a == Some(0x9114A) && b == Some(0x9114A) && c == Some(0x9114A) && d == Some(0x9114A),
            "非法位深未被拒或码不是 0x9114A",
        );
    }

    // 4) 行距公式：四个独立算例，覆盖 1/4/8bpp 与补齐边界。
    {
        let a = IndexedBitmap::stride_for(8, 1).ok();
        let b = IndexedBitmap::stride_for(4, 4).ok();
        let c = IndexedBitmap::stride_for(3, 8).ok();
        let d = IndexedBitmap::stride_for(33, 1).ok();
        cs.add(
            "f21610.stride_formula",
            a == Some(4) && b == Some(4) && c == Some(4) && d == Some(8),
            "行距不是 ((w*bpp+31)/32)*4（期望 4/4/4/8）",
        );
    }

    // 5) 1bpp 展开：位序必须 MSB-first（0xAA 行 → 白,黑,白,黑）。
    {
        let ok = match (&mono, &one) {
            (Ok(p), Ok(bm)) => match expand(bm, p) {
                Ok(e) => {
                    e.bitmap.at(0, 0) == 0xFFFF_FFFF
                        && e.bitmap.at(1, 0) == 0xFF00_0000
                        && e.bitmap.at(6, 0) == 0xFFFF_FFFF
                        && e.bitmap.at(7, 0) == 0xFF00_0000
                        && e.bitmap.at(0, 1) == 0xFFFF_FFFF
                        && e.bitmap.at(2, 1) == 0xFF00_0000
                        && e.bitmap.at(4, 1) == 0xFFFF_FFFF
                        && e.bitmap.at(6, 1) == 0xFF00_0000
                }
                Err(_) => false,
            },
            _ => false,
        };
        cs.add(
            "f21610.expand_1bpp_bits",
            ok,
            "1bpp 展开位序不符（应为 MSB-first：0xAA → 白,黑,白,黑…）",
        );
    }

    // 6) 4bpp 展开：偶列取高半字节（字节 0x01 → 列 0 索引 0、列 1 索引 1）。
    {
        let ok = match (&g16, &four) {
            (Ok(p), Ok(bm)) => match expand(bm, p) {
                Ok(e) => {
                    e.bitmap.at(0, 0) == 0xFF00_0000
                        && e.bitmap.at(1, 0) == 0xFF01_0101
                        && e.bitmap.at(2, 0) == 0xFF02_0202
                        && e.bitmap.at(3, 0) == 0xFF03_0303
                        && e.bitmap.at(0, 1) == 0xFF0F_0F0F
                        && e.bitmap.at(1, 1) == 0xFF0E_0E0E
                        && e.bitmap.at(3, 1) == 0xFF0C_0C0C
                }
                Err(_) => false,
            },
            _ => false,
        };
        cs.add(
            "f21610.expand_4bpp_nibbles",
            ok,
            "4bpp 展开未按「偶列高半字节」抽取（期望 00,01,02,03 / 0F,0E,0D,0C）",
        );
    }

    // 7) 8bpp 直取：每个像素的打包值必须等于索引对应的灰度阶。
    {
        let ok = match (&g256, &eight) {
            (Ok(p), Ok(bm)) => match expand(bm, p) {
                Ok(e) => {
                    let mut all = true;
                    let mut y = 0u32;
                    while y < bm.height && all {
                        let mut x = 0u32;
                        while x < bm.width && all {
                            let idx = match bm.get_index(x, y) {
                                Ok(v) => v,
                                Err(_) => 255u8,
                            };
                            if e.bitmap.at(x, y) != gray_of(idx as u16) {
                                all = false;
                            }
                            x += 1;
                        }
                        y += 1;
                    }
                    all && e.bitmap.ink() == 64
                }
                Err(_) => false,
            },
            _ => false,
        };
        cs.add(
            "f21610.expand_8bpp_direct",
            ok,
            "8bpp 直取展开与索引灰度不符（16×4 应为 64 个非零像素）",
        );
    }

    // 8) 索引越表必拒：8bpp、表长 4、某像素索引 5 → 0x9114C，行列出对。
    {
        let mut p = Palette::new();
        let mut built = true;
        let mut i = 0u16;
        while i < 4 {
            if p.push(Bgra::opaque(i as u8, 0, 0)).is_err() {
                built = false;
            }
            i += 1;
        }
        let mut bm = match IndexedBitmap::new_zeroed(2, 2, 8) {
            Ok(v) => v,
            Err(_) => empty_bitmap(8),
        };
        let _ = bm.set_index(0, 0, 0);
        let _ = bm.set_index(1, 0, 1);
        let _ = bm.set_index(0, 1, 2);
        let _ = bm.set_index(1, 1, 5);
        let f = expand(&bm, &p).err();
        let hit = match f {
            Some(v) => v.code == 0x9114C && v.row == 1 && v.col == 1 && v.index == 5,
            None => false,
        };
        cs.add(
            "f21610.index_short_rejected",
            built && hit,
            "越表索引未被拒或定位不符（应 0x9114C @ row=1 col=1 idx=5）",
        );
        let text = match expand(&bm, &p).err() {
            Some(v) => v.describe(),
            None => String::new(),
        };
        let known = match expand(&bm, &p).err() {
            Some(v) => v.is_known() && v.as_code() == PalCode::IndexShort,
            None => false,
        };
        cs.add(
            "f21610.fault_reportable",
            text.contains("row=1")
                && text.contains("idx=5")
                && text.contains("0x9114c")
                && known,
            "故障文本缺定位字段，或码未落回闭集枚举",
        );
    }

    // 9) 索引越位深跨度：4bpp 上问索引 16 → 0x9114D（哪怕表有 256 项）。
    {
        let mut p = Palette::new();
        let _ = p.fill_gray(256);
        let a = p.lookup(16, 4).err().map(|f| f.code);
        let b = p.lookup(2, 1).err().map(|f| f.code);
        let c = p.lookup(255, 8).ok().map(|v| v.r);
        cs.add(
            "f21610.index_over_bpp_rejected",
            a == Some(0x9114D) && b == Some(0x9114D) && c == Some(255),
            "越位深跨度索引未被拒（应 0x9114D），或 8bpp 索引 255 取色失败",
        );
    }

    // 10) 表尾残留不得被当成颜色：表长 200、问索引 250 → 0x9114C。
    {
        let mut p = Palette::new();
        let _ = p.fill_gray(200);
        let code = p.lookup(250, 8).err().map(|f| f.code);
        let ok = p.lookup(199, 8).is_ok();
        cs.add(
            "f21610.gray_palette_truncated",
            code == Some(0x9114C) && ok && p.used() == 200 && p.reserved() == 200,
            "表长 200 时索引 250 未被拒，或索引 199 未取到",
        );
    }

    // 11) 行距被篡改必拒：16 宽的 8bpp 行距应为 16，改成 12。
    {
        let tampered = IndexedBitmap::with_stride(16, 1, 8, 12, zeros(16));
        let code = match &g256 {
            Ok(p) => expand(&tampered, p).err().map(|f| f.code),
            Err(_) => None,
        };
        cs.add(
            "f21610.stride_tamper_rejected",
            code == Some(0x9114E),
            "行距被改为 12 后未被拒（应 0x9114E）",
        );
    }

    // 12) 缓冲被截断必拒：16×4 的 8bpp 只给 32 字节（应 64）。
    {
        let short = IndexedBitmap::with_stride(16, 4, 8, 16, zeros(32));
        let code = match &g256 {
            Ok(p) => expand(&short, p).err().map(|f| f.code),
            Err(_) => None,
        };
        let mut probe = IndexedBitmap::new_zeroed(4, 4, 8).unwrap_or(empty_bitmap(8));
        let edge = probe.set_index(4, 0, 0).err().map(|f| f.edge);
        cs.add(
            "f21610.buffer_short_rejected",
            code == Some(0x9114F) && edge == Some(EDGE_RIGHT),
            "像素缓冲只有一半时未被拒（应 0x9114F），或越界写未标右边",
        );
    }

    // 13) 溢出防护：宽 0x2000_0000 的 8bpp 行距必溢出 u32。
    {
        let code = IndexedBitmap::stride_for(0x2000_0000, 8).err().map(|f| f.code);
        let still = IndexedBitmap::stride_for(0x0100_0000, 8).ok();
        cs.add(
            "f21610.span_overflow_rejected",
            code == Some(0x91150) && still == Some(0x1000_0000),
            "宽 0x20000000 未报溢出（应 0x91150），或合法宽的行距算错（应 0x10000000）",
        );
    }

    // 14) 账目：8×8 = 64 像素，步数 = 64 × 2 = 128，取色次数同像素数。
    {
        let bm = match IndexedBitmap::new_zeroed(8, 8, 8) {
            Ok(v) => v,
            Err(_) => empty_bitmap(8),
        };
        let ok = match &g256 {
            Ok(p) => match expand(&bm, p) {
                Ok(e) => {
                    e.stats.pixels == 64
                        && e.stats.lookups == 64
                        && e.stats.steps == 128
                        && e.stats.balanced()
                        && e.bitmap.ink() == 64
                }
                Err(_) => false,
            },
            Err(_) => false,
        };
        cs.add(
            "f21610.step_account",
            ok,
            "账目不符（期望 pixels=64 lookups=64 steps=128 ink=64）",
        );
    }

    // 15) 幂等：同输入两次展开逐像素相同（指纹相等），账目也相同。
    {
        let bm = match IndexedBitmap::new_zeroed(12, 6, 8) {
            Ok(v) => v,
            Err(_) => empty_bitmap(8),
        };
        let (fa, sa, fb, sb) = match &g256 {
            Ok(p) => match (expand(&bm, p), expand(&bm, p)) {
                (Ok(x), Ok(y)) => (
                    Some(x.bitmap.fingerprint()),
                    Some(x.stats),
                    Some(y.bitmap.fingerprint()),
                    Some(y.stats),
                ),
                _ => (None, None, None, None),
            },
            Err(_) => (None, None, None, None),
        };
        cs.add(
            "f21610.idempotent",
            fa.is_some() && fa == fb && sa == sb && fa != Some(0xcbf2_9ce4_8422_2325),
            "两次展开结果或账目不一致（幂等性/确定性破坏）",
        );
    }

    cs
}
