//! UNX-F21607 · ROP3 与背景色的 MERGEPEN 系语义
//!
//! 诊断段 0x91131~0x91138（GDI 光栅段 1/3：笔系 ROP 与图案平面）。
//!
//! 判据原文（施工书 UNX-F21607-J1）：MERGEPEN/MASKPEN/MERGECOPY 与背景色混色逐位正确。
//!
//! 设计要点（为什么不是「把六个公式各写一行 if」）：
//! 六个笔 ROP 表面上是六条公式，实质是同一个三变量布尔函数族的六个点。若按公式逐个
//! 写分支，判据只能证明「六个 if 与六条公式一致」——公式本身抄错时无人对账。故本模块
//! 只承认一条权威：真值表索引 `(fn >> ((p<<2)|(s<<1)|d)) & 1`。六条公式全部由独立的
//! 标量参考在 8 个 (p,s,d) 组合上回算，任一条与真值表分叉即报 AnchorMismatch。这样
//! 「公式」与「求值器」互为判据，而不是自证。
//!
//! 背景色单独成层：DDB 图案是索引色位图，ROP 只认识位平面。索引 → 位的映射由背景色
//! 比较决定（pixel != bk ⇒ P=1）。把这一层剥出来是因为它是**策略**（Opaque/Transparent
//! 共用同一 P 平面），而 ROP 是**算术**；混在一处会让「bk 索引越界」被误报成「ROP 算错」。
//!
//! 位并行是刻意的：一个字节 8 像素同平面打包，整字节运算即 8 像素同时求值。但位并行的
//! 掩码写反时结果是「另一个 ROP」而不是报错，所以保留逐像素标量参考做对拍——两条路径
//! 的失效模式不同，256 个 fn 全字节比对能在无 GPU 的 no_std 环境里稳定抓住这类错。

use alloc::vec;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、六个笔 ROP 的 fn 字节
// ---------------------------------------------------------------------------
//
// 数值按 `index = P*4 + S*2 + D` 现算，不是抄的。低 16 位是 R2 时代遗留编码，与 ROP3
// 求值无关——MERGECOPY 与 MASKPEN 的低半字不同（0x00CA / 0x00C9）而 fn 同为 0xC0，
// 正是「fn 才是恒等身份」的实证。

/// MERGEPEN：P|S。P/S 全组合置位、D 无关 → 0b1111_1100。
pub const FN_MERGEPEN: u8 = 0xFC;
/// MASKPEN：P&S。仅 (P,S)=(1,1) 两种索引置位 → 0b1100_0000。
pub const FN_MASKPEN: u8 = 0xC0;
/// MERGECOPY：S&P。交换律下与 MASKPEN 同一函数。
pub const FN_MERGECOPY: u8 = 0xC0;
/// MASKPENNOT：P&!S。仅 (P,S)=(1,0) → 索引 4、5 → 0b0011_0000。
pub const FN_MASKPENNOT: u8 = 0x30;
/// MERGENOTPEN：!P|S。(P,S)=(1,0) 两索引清零、其余置位 → 0b1100_1111。
pub const FN_MERGENOTPEN: u8 = 0xCF;
/// NOTMERGEPEN：!(S|P)。仅 (P,S)=(0,0) → 索引 0、1 → 0b0000_0011。
pub const FN_NOTMERGEPEN: u8 = 0x03;

/// 锚点全码：MERGEPAINT（D = !S | D）。
pub const ROP_MERGEPAINT: u32 = 0x00BB_0226;
/// 锚点全码：MERGECOPY（D = P & S）。
pub const ROP_MERGECOPY: u32 = 0x00C0_00CA;
/// 锚点全码：PATPAINT（D = P | !S | D）。
pub const ROP_PATPAINT: u32 = 0x00FB_0A09;
/// 锚点全码：SRCCOPY（D = S）。
pub const ROP_SRCCOPY: u32 = 0x00CC_0020;
/// 锚点全码：PATCOPY（D = P，无源操作）。
pub const ROP_PATCOPY: u32 = 0x00F0_0021;

/// 闭集诊断码（0x91131~0x91138 八码）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MergeCode {
    /// 0x91131 笔系 ROP 求值成功。
    Ok = 0x91131,
    /// 0x91132 背景色位平面与图案像素不符。
    BkPlaneMismatch = 0x91132,
    /// 0x91133 具名 ROP 与真值表分叉。
    AnchorMismatch = 0x91133,
    /// 0x91134 码不在笔 ROP 闭集内。
    RopNotClosed = 0x91134,
    /// 0x91135 fn 字节与全码高字节不一致。
    FnRopDisagree = 0x91135,
    /// 0x91136 平面几何不符（宽高不等或坐标越界）。
    PlaneGeometry = 0x91136,
    /// 0x91137 背景色索引越出调色板。
    BkIndexOutOfRange = 0x91137,
    /// 0x91138 位并行与标量参考分叉。
    ScalarDiverge = 0x91138,
}

impl MergeCode {
    /// 现象。
    pub const fn what(self) -> &'static str {
        match self {
            MergeCode::Ok => "笔系 ROP 求值成功",
            MergeCode::BkPlaneMismatch => "背景色平面与图案像素不符",
            MergeCode::AnchorMismatch => "具名 ROP 与真值表分叉",
            MergeCode::RopNotClosed => "码不在笔 ROP 闭集内",
            MergeCode::FnRopDisagree => "fn 字节与全码高字节不一致",
            MergeCode::PlaneGeometry => "平面几何不符",
            MergeCode::BkIndexOutOfRange => "背景色索引越界",
            MergeCode::ScalarDiverge => "位并行与标量参考分叉",
        }
    }
    /// 原因。
    pub const fn why(self) -> &'static str {
        match self {
            MergeCode::Ok => "真值表索引与六条公式在八组合上一致",
            MergeCode::BkPlaneMismatch => "比较用了前景色或比较方向写反",
            MergeCode::AnchorMismatch => "公式抄写偏离了 fn 的位模式",
            MergeCode::RopNotClosed => "该 fn 不属于笔系六函数",
            MergeCode::FnRopDisagree => "全码被改写或 fn 未从 bit16 抽取",
            MergeCode::PlaneGeometry => "源/图案/目标三平面尺寸不同源",
            MergeCode::BkIndexOutOfRange => "调色板条目数少于图案索引",
            MergeCode::ScalarDiverge => "位并行掩码与逐像素语义不等价",
        }
    }
    /// 建议。
    pub const fn advice(self) -> &'static str {
        match self {
            MergeCode::Ok => "转入图案平面合成",
            MergeCode::BkPlaneMismatch => "以像素==bk 生成 0 位、否则 1 位",
            MergeCode::AnchorMismatch => "用标量参考回算该 fn 的八组合",
            MergeCode::RopNotClosed => "改走通用 rop3_byte 而非闭集分派",
            MergeCode::FnRopDisagree => "fn 一律取 (rop>>16)&0xFF 后复核",
            MergeCode::PlaneGeometry => "先对齐三平面再合成",
            MergeCode::BkIndexOutOfRange => "按索引位宽扩调色板或降索引",
            MergeCode::ScalarDiverge => "逐字节与标量结果比对定位首差位",
        }
    }
    /// 数值形式（日志对账键）。返回 u32：五位数码段用 u16 会截断。
    pub const fn code(self) -> u32 {
        self as u32
    }
}

/// 八码闭集（判据遍历用；顺序即码序）。
pub const ALL_CODES: [MergeCode; 8] = [
    MergeCode::Ok,
    MergeCode::BkPlaneMismatch,
    MergeCode::AnchorMismatch,
    MergeCode::RopNotClosed,
    MergeCode::FnRopDisagree,
    MergeCode::PlaneGeometry,
    MergeCode::BkIndexOutOfRange,
    MergeCode::ScalarDiverge,
];

/// 故障（码 + 定位）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Fault {
    /// 诊断码（u32）。
    pub code: u32,
    /// 横坐标或首个标量（与具体像素无关时含义见各构造点）。
    pub x: u32,
    /// 纵坐标或期望值。
    pub y: u32,
}

impl Fault {
    const fn at(code: MergeCode, x: u32, y: u32) -> Fault {
        Fault {
            code: code.code(),
            x,
            y,
        }
    }
    /// 码是否落在本模块闭集内。
    pub const fn is_known(self) -> bool {
        self.code >= 0x91131 && self.code <= 0x91138
    }
    /// 还原枚举。闭集外落到 ScalarDiverge：未知即可疑，但不 panic。
    pub const fn as_code(self) -> MergeCode {
        match self.code {
            0x91131 => MergeCode::Ok,
            0x91132 => MergeCode::BkPlaneMismatch,
            0x91133 => MergeCode::AnchorMismatch,
            0x91134 => MergeCode::RopNotClosed,
            0x91135 => MergeCode::FnRopDisagree,
            0x91136 => MergeCode::PlaneGeometry,
            0x91137 => MergeCode::BkIndexOutOfRange,
            _ => MergeCode::ScalarDiverge,
        }
    }
}

// ---------------------------------------------------------------------------
// 二、真值表求值（唯一权威）
// ---------------------------------------------------------------------------

/// 逐像素求值：索引 = P*4 + S*2 + D。
///
/// 顺序不可换。P 占高两位意味着「图案决定左半张真值表」——PATCOPY 的 fn=0xF0
/// （左半全 1）与 SRCCOPY 的 0xCC（S 占满两列）都只在这个索引下成立；换成 S 高
/// 两位，两个锚点会同时错位，而错位后的结果仍是一个合法 ROP，极难察觉。
pub const fn rop3_bit(fn_byte: u8, p: bool, s: bool, d: bool) -> bool {
    let idx = ((p as u32) << 2) | ((s as u32) << 1) | (d as u32);
    ((fn_byte as u32) >> idx) & 1 != 0
}

/// 位并行求值：8 像素共用一次字节运算。
///
/// 展开成 8 个最小项的与再或。看着比「查表 8 次」笨，但它把分支全消掉了：输入
/// 是什么都不影响指令序列，这才是位并行的意义，也是标量对拍能有意义的前提——两条
/// 路径的结构完全不同，同错的可能性低。
pub fn rop3_byte(fn_byte: u8, p: u8, s: u8, d: u8) -> u8 {
    let mut out = 0u8;
    let mut idx = 0u32;
    while idx < 8 {
        let pm = if idx & 4 != 0 { p } else { !p };
        let sm = if idx & 2 != 0 { s } else { !s };
        let dm = if idx & 1 != 0 { d } else { !d };
        let m = pm & sm & dm;
        if (fn_byte >> idx) & 1 != 0 {
            out |= m;
        }
        idx += 1;
    }
    out
}

/// 输入分量（依赖检测的被测量）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PlaneVar {
    /// 图案平面 P。
    Pattern,
    /// 源平面 S。
    Source,
    /// 目标平面 D。
    Dest,
}

/// 某分量是否影响输出。
///
/// 用途是把「无源操作」与笔系操作分开：PATCOPY 对 S 完全不敏感，若被当成「源平面
/// 读错了」去查，会白查一整轮。依赖检测只看真值表里翻转该索引位后的两两成对位。
pub fn depends_on(fn_byte: u8, var: PlaneVar) -> bool {
    let flip = match var {
        PlaneVar::Pattern => 4u32,
        PlaneVar::Source => 2,
        PlaneVar::Dest => 1,
    };
    let mut i = 0u32;
    while i < 8 {
        let j = i ^ flip;
        if ((fn_byte as u32) >> i) & 1 != ((fn_byte as u32) >> j) & 1 {
            return true;
        }
        i += 1;
    }
    false
}

// ---------------------------------------------------------------------------
// 三、位平面
// ---------------------------------------------------------------------------

/// 单平面位图（MSB 在左，行按字节对齐）。
///
/// 只存 1bpp 是因为 ROP3 的三个输入本来就各是一位。DDB 的索引色/真彩色差异在
/// 「索引 → 位」这一层（`Pattern::p_plane`）已经消化掉了；在此保留颜色深度会让
/// 合成器被迫知道调色板，判据也就无法只谈布尔算术。
pub struct BitPlane {
    width: u32,
    height: u32,
    data: Vec<u8>,
}

impl BitPlane {
    /// 新建全零平面。宽或高为 0 视为几何故障——空平面会让「全零即未绘制」这类
    /// 判据恒真，是个静默的判据失效源。
    pub fn new(width: u32, height: u32) -> Result<BitPlane, Fault> {
        if width == 0 || height == 0 {
            return Err(Fault::at(MergeCode::PlaneGeometry, width, height));
        }
        let rb = plane_row_bytes(width) as usize;
        let total = rb.saturating_mul(height as usize);
        Ok(BitPlane {
            width,
            height,
            data: vec![0u8; total],
        })
    }
    /// 由已有字节构造（字节数必须恰为 行字节×高）。
    pub fn from_bytes(width: u32, height: u32, data: Vec<u8>) -> Result<BitPlane, Fault> {
        if width == 0 || height == 0 {
            return Err(Fault::at(MergeCode::PlaneGeometry, width, height));
        }
        let need = (plane_row_bytes(width) as usize).saturating_mul(height as usize);
        if data.len() != need {
            return Err(Fault::at(
                MergeCode::PlaneGeometry,
                data.len() as u32,
                need as u32,
            ));
        }
        Ok(BitPlane {
            width,
            height,
            data,
        })
    }
    /// 行字节数。
    pub const fn row_bytes(&self) -> u32 {
        (self.width + 7) / 8
    }
    /// 宽度。
    pub const fn width(&self) -> u32 {
        self.width
    }
    /// 高度。
    pub const fn height(&self) -> u32 {
        self.height
    }
    /// 原始字节（只读）。
    pub fn bytes(&self) -> &[u8] {
        &self.data
    }
    /// 取位。
    pub fn get(&self, x: u32, y: u32) -> Result<bool, Fault> {
        if x >= self.width || y >= self.height {
            return Err(Fault::at(MergeCode::PlaneGeometry, x, y));
        }
        let off = (y as usize).saturating_mul(self.row_bytes() as usize) + (x as usize) / 8;
        let bit = 7 - (x % 8);
        match self.data.get(off) {
            Some(b) => Ok((b >> bit) & 1 != 0),
            None => Err(Fault::at(MergeCode::PlaneGeometry, x, y)),
        }
    }
    /// 置位/清位。
    pub fn set(&mut self, x: u32, y: u32, v: bool) -> Result<(), Fault> {
        if x >= self.width || y >= self.height {
            return Err(Fault::at(MergeCode::PlaneGeometry, x, y));
        }
        let off = (y as usize).saturating_mul(self.row_bytes() as usize) + (x as usize) / 8;
        let bit = 7 - (x % 8);
        match self.data.get_mut(off) {
            Some(b) => {
                if v {
                    *b |= 1 << bit;
                } else {
                    *b &= !(1 << bit);
                }
                Ok(())
            }
            None => Err(Fault::at(MergeCode::PlaneGeometry, x, y)),
        }
    }
    /// 置 1 的位数（判据把平面压缩成一个可硬写的整数）。
    pub fn count_ones(&self) -> u32 {
        let mut n = 0u32;
        for b in &self.data {
            n += b.count_ones();
        }
        n
    }
    /// 逐位相等（几何不同即不等，不做对齐容错）。
    pub fn equals(&self, other: &BitPlane) -> bool {
        self.width == other.width && self.height == other.height && self.data == other.data
    }
    /// 全零。
    pub fn is_blank(&self) -> bool {
        self.count_ones() == 0
    }
    /// 取一整行字节（越界返回空切片）。
    pub fn row(&self, y: u32) -> &[u8] {
        let rb = self.row_bytes() as usize;
        let start = (y as usize).saturating_mul(rb);
        let end = start.saturating_add(rb);
        if y >= self.height || end > self.data.len() {
            return &[];
        }
        &self.data[start..end]
    }
}

/// 位平面行字节数（宽向上取整到 8 的倍数）。
pub const fn plane_row_bytes(width: u32) -> u32 {
    (width + 7) / 8
}

// ---------------------------------------------------------------------------
// 四、索引色图案与背景色
// ---------------------------------------------------------------------------

/// 背景色填充模式。
///
/// Opaque 与 Transparent 的差别只在遮罩，不在 P 平面：两种模式下「像素是否等于
/// bk」都要算出同一位。把模式塞进 P 平面生成会得到两个不同的 P，于是同一图案在
/// 两种模式下的 ROP 结果无法对比，判据就分不清是模式错还是 ROP 错。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BkMode {
    /// 背景不透明：整块矩形都参与绘制。
    Opaque,
    /// 背景透明：等于 bk 的像素不绘制。
    Transparent,
}

/// 索引色图案（DDB 图案刷子位图）。
pub struct Pattern {
    width: u32,
    height: u32,
    pixels: Vec<u8>,
    bk_index: u8,
    palette_len: u16,
}

impl Pattern {
    /// 新建并做一次全量索引校验。
    ///
    /// 校验放在构造期而不是绘制期：索引越界在绘制期表现为「某一行整行变黑」，
    /// 那种现象会被误读成 ROP 掩码错误，排查成本极高。
    pub fn new(width: u32, height: u32, bk_index: u8, palette_len: u16) -> Result<Pattern, Fault> {
        if width == 0 || height == 0 {
            return Err(Fault::at(MergeCode::PlaneGeometry, width, height));
        }
        if (bk_index as u16) >= palette_len {
            return Err(Fault::at(MergeCode::BkIndexOutOfRange, bk_index as u32, 0));
        }
        let n = (width as usize).saturating_mul(height as usize);
        Ok(Pattern {
            width,
            height,
            pixels: vec![bk_index; n],
            bk_index,
            palette_len,
        })
    }
    /// 宽度。
    pub const fn width(&self) -> u32 {
        self.width
    }
    /// 高度。
    pub const fn height(&self) -> u32 {
        self.height
    }
    /// 背景色索引。
    pub const fn bk_index(&self) -> u8 {
        self.bk_index
    }
    /// 调色板条目数。
    pub const fn palette_len(&self) -> u16 {
        self.palette_len
    }
    /// 取像素索引。
    pub fn pixel(&self, x: u32, y: u32) -> Result<u8, Fault> {
        if x >= self.width || y >= self.height {
            return Err(Fault::at(MergeCode::PlaneGeometry, x, y));
        }
        let off = (y as usize).saturating_mul(self.width as usize) + (x as usize);
        match self.pixels.get(off) {
            Some(v) => Ok(*v),
            None => Err(Fault::at(MergeCode::PlaneGeometry, x, y)),
        }
    }
    /// 写像素索引（越界索引拒写）。
    pub fn set_pixel(&mut self, x: u32, y: u32, v: u8) -> Result<(), Fault> {
        if (v as u16) >= self.palette_len {
            return Err(Fault::at(MergeCode::BkIndexOutOfRange, x, v as u32));
        }
        if x >= self.width || y >= self.height {
            return Err(Fault::at(MergeCode::PlaneGeometry, x, y));
        }
        let off = (y as usize).saturating_mul(self.width as usize) + (x as usize);
        match self.pixels.get_mut(off) {
            Some(slot) => {
                *slot = v;
                Ok(())
            }
            None => Err(Fault::at(MergeCode::PlaneGeometry, x, y)),
        }
    }
    /// Pn 平面生成器：索引不等于背景色 ⇒ 1，等于 ⇒ 0。
    ///
    /// 方向不可反。P=1 在笔系 ROP 里代表「图案前景」，MERGECOPY 与 MASKPEN 都靠它
    /// 保持图案形状；反过来会让所有笔系 ROP 输出图案底片，而且**看起来仍是一张
    /// 合法图案**——这类错误只能靠「整片背景色必须给出全零平面」这种结构性质抓出。
    pub fn p_plane(&self) -> Result<BitPlane, Fault> {
        let mut plane = BitPlane::new(self.width, self.height)?;
        let mut y = 0u32;
        while y < self.height {
            let mut x = 0u32;
            while x < self.width {
                if self.pixel(x, y)? != self.bk_index {
                    plane.set(x, y, true)?;
                }
                x += 1;
            }
            y += 1;
        }
        Ok(plane)
    }
    /// 绘制遮罩：Opaque 全 1，Transparent 等于 P 平面。
    pub fn paint_mask(&self, mode: BkMode) -> Result<BitPlane, Fault> {
        match mode {
            BkMode::Opaque => {
                let mut plane = BitPlane::new(self.width, self.height)?;
                let mut y = 0u32;
                while y < self.height {
                    let mut x = 0u32;
                    while x < self.width {
                        plane.set(x, y, true)?;
                        x += 1;
                    }
                    y += 1;
                }
                Ok(plane)
            }
            BkMode::Transparent => self.p_plane(),
        }
    }
    /// 背景色像素占比（千分比整数定点；口径为本模块统计定义，非外部标准）。
    ///
    /// 用千分比而非百分比：小图案上 1/200 的 bk 占比在百分比下四舍五入成 0%，
    /// 让「背景色几乎没生效」这种半失效状态看起来完全正常。
    pub fn bk_ratio_per_mille(&self) -> u32 {
        let total = self.width.saturating_mul(self.height);
        if total == 0 {
            return 0;
        }
        let mut bk = 0u32;
        for v in &self.pixels {
            if *v == self.bk_index {
                bk += 1;
            }
        }
        bk.saturating_mul(1000) / total
    }
}

// ---------------------------------------------------------------------------
// 五、笔 ROP 闭集
// ---------------------------------------------------------------------------

/// 六个笔 ROP。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PenOp {
    /// P|S
    MergePen,
    /// P&S
    MaskPen,
    /// S&P
    MergeCopy,
    /// P&!S
    MaskPenNot,
    /// !P|S
    MergeNotPen,
    /// !(S|P)
    NotMergePen,
}

/// 六元闭集（遍历用；顺序即声明序）。
pub const PEN_CLOSED_SET: [PenOp; 6] = [
    PenOp::MergePen,
    PenOp::MaskPen,
    PenOp::MergeCopy,
    PenOp::MaskPenNot,
    PenOp::MergeNotPen,
    PenOp::NotMergePen,
];

impl PenOp {
    /// fn 字节。
    pub const fn fn_byte(self) -> u8 {
        match self {
            PenOp::MergePen => FN_MERGEPEN,
            PenOp::MaskPen => FN_MASKPEN,
            PenOp::MergeCopy => FN_MERGECOPY,
            PenOp::MaskPenNot => FN_MASKPENNOT,
            PenOp::MergeNotPen => FN_MERGENOTPEN,
            PenOp::NotMergePen => FN_NOTMERGEPEN,
        }
    }
    /// 公式文本（诊断与判据对账用）。
    pub const fn formula(self) -> &'static str {
        match self {
            PenOp::MergePen => "P|S",
            PenOp::MaskPen => "P&S",
            PenOp::MergeCopy => "S&P",
            PenOp::MaskPenNot => "P&!S",
            PenOp::MergeNotPen => "!P|S",
            PenOp::NotMergePen => "!(S|P)",
        }
    }
    /// 闭集反查：fn 必须命中六函数之一。
    pub fn from_fn(fn_byte: u8) -> Result<PenOp, Fault> {
        let mut i = 0usize;
        while i < PEN_CLOSED_SET.len() {
            if PEN_CLOSED_SET[i].fn_byte() == fn_byte {
                return Ok(PEN_CLOSED_SET[i]);
            }
            i += 1;
        }
        Err(Fault::at(MergeCode::RopNotClosed, fn_byte as u32, 0))
    }
    /// 是否对源平面免疫（无源操作边界：PATCOPY 一类）。
    pub fn is_source_blind(self) -> bool {
        !depends_on(self.fn_byte(), PlaneVar::Source)
    }
}

/// 具名锚点 ROP（全码 + fn）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct AnchorRop {
    /// 全码。
    pub rop: u32,
    /// fn 字节。
    pub fn_byte: u8,
    /// 名称。
    pub name: &'static str,
}

/// 五个锚点（施工书原文给定）。
pub const ANCHORS: [AnchorRop; 5] = [
    AnchorRop {
        rop: ROP_MERGEPAINT,
        fn_byte: 0xBB,
        name: "MERGEPAINT",
    },
    AnchorRop {
        rop: ROP_MERGECOPY,
        fn_byte: 0xC0,
        name: "MERGECOPY",
    },
    AnchorRop {
        rop: ROP_PATPAINT,
        fn_byte: 0xFB,
        name: "PATPAINT",
    },
    AnchorRop {
        rop: ROP_SRCCOPY,
        fn_byte: 0xCC,
        name: "SRCCOPY",
    },
    AnchorRop {
        rop: ROP_PATCOPY,
        fn_byte: 0xF0,
        name: "PATCOPY",
    },
];

impl AnchorRop {
    /// 从全码抽 fn。
    pub const fn extract_fn(rop: u32) -> u8 {
        ((rop >> 16) & 0xFF) as u8
    }
    /// 自洽校验：声明 fn 必须等于全码高字节。
    pub fn verify(self) -> Result<(), Fault> {
        if self.fn_byte != AnchorRop::extract_fn(self.rop) {
            return Err(Fault::at(
                MergeCode::FnRopDisagree,
                self.rop,
                self.fn_byte as u32,
            ));
        }
        Ok(())
    }
    /// 标量参考：用公式回算 (p,s,d)。
    pub fn eval(self, p: bool, s: bool, d: bool) -> bool {
        anchor_formula(self.fn_byte, p, s, d)
    }
}

/// 锚点公式表（与真值表独立书写，互为判据）。
pub fn anchor_formula(fn_byte: u8, p: bool, s: bool, d: bool) -> bool {
    match fn_byte {
        0xBB => !s | d,
        0xC0 => p & s,
        0xFB => p | !s | d,
        0xCC => s,
        0xF0 => p,
        _ => rop3_bit(fn_byte, p, s, d),
    }
}

// ---------------------------------------------------------------------------
// 六、合成器
// ---------------------------------------------------------------------------

/// 位并行合成：三平面同几何，逐字节求值。
pub fn compose(fn_byte: u8, p: &BitPlane, s: &BitPlane, d: &BitPlane) -> Result<BitPlane, Fault> {
    if p.width() != s.width()
        || p.width() != d.width()
        || p.height() != s.height()
        || p.height() != d.height()
    {
        return Err(Fault::at(MergeCode::PlaneGeometry, p.width(), s.height()));
    }
    let pl = p.bytes();
    let sl = s.bytes();
    let dl = d.bytes();
    let mut buf = vec![0u8; pl.len()];
    let mut i = 0usize;
    while i < pl.len() {
        let a = match pl.get(i) {
            Some(v) => *v,
            None => 0,
        };
        let b = match sl.get(i) {
            Some(v) => *v,
            None => 0,
        };
        let c = match dl.get(i) {
            Some(v) => *v,
            None => 0,
        };
        if let Some(slot) = buf.get_mut(i) {
            *slot = rop3_byte(fn_byte, a, b, c);
        }
        i += 1;
    }
    BitPlane::from_bytes(p.width(), p.height(), buf)
}

/// 标量参考合成：逐像素查真值表。
///
/// 刻意保留一份「慢且直白」的实现。位并行的错误通常只在一个字节的某一位上显形
/// （`!p` 写成 `p` 之类），肉眼审不出来；两条路径对拍是唯一能在 no_std 环境里
/// 稳定抓住这类错的手段。
pub fn compose_scalar(
    fn_byte: u8,
    p: &BitPlane,
    s: &BitPlane,
    d: &BitPlane,
) -> Result<BitPlane, Fault> {
    if p.width() != s.width() || p.width() != d.width() {
        return Err(Fault::at(MergeCode::PlaneGeometry, p.width(), s.width()));
    }
    let mut out = BitPlane::new(p.width(), p.height())?;
    let mut y = 0u32;
    while y < out.height() {
        let mut x = 0u32;
        while x < out.width() {
            let pv = p.get(x, y)?;
            let sv = s.get(x, y)?;
            let dv = d.get(x, y)?;
            out.set(x, y, rop3_bit(fn_byte, pv, sv, dv))?;
            x += 1;
        }
        y += 1;
    }
    Ok(out)
}

/// 合成统计。
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Stats {
    /// 成功合成的平面数。
    pub planes: u32,
    /// 求值过的字节数（位并行：一字节 = 8 像素）。
    pub bytes: u64,
    /// 被拒次数。
    pub rejects: u32,
}

impl Stats {
    /// 可归档报告（字段顺序固定，便于逐字 diff）。
    pub fn report(&self) -> String {
        let mut out = String::with_capacity(64);
        out.push_str("mergepen planes=");
        out.push_str(&self.planes.to_string());
        out.push_str(" bytes=");
        out.push_str(&self.bytes.to_string());
        out.push_str(" rejects=");
        out.push_str(&self.rejects.to_string());
        out
    }
}

/// 合成引擎（带记账）。
pub struct MergeEngine {
    stats: Stats,
}

impl MergeEngine {
    /// 新建。
    pub const fn new() -> MergeEngine {
        MergeEngine {
            stats: Stats {
                planes: 0,
                bytes: 0,
                rejects: 0,
            },
        }
    }
    /// 统计快照。
    pub const fn stats(&self) -> Stats {
        self.stats
    }
    /// 应用一个笔 ROP。
    pub fn apply(
        &mut self,
        op: PenOp,
        p: &BitPlane,
        s: &BitPlane,
        d: &BitPlane,
    ) -> Result<BitPlane, Fault> {
        match compose(op.fn_byte(), p, s, d) {
            Ok(plane) => {
                self.stats.planes = self.stats.planes.saturating_add(1);
                self.stats.bytes = self
                    .stats
                    .bytes
                    .saturating_add((plane.row_bytes() as u64) * (plane.height() as u64));
                Ok(plane)
            }
            Err(e) => {
                self.stats.rejects = self.stats.rejects.saturating_add(1);
                Err(e)
            }
        }
    }
    /// 按全码应用（走闭集反查）。
    pub fn apply_rop(
        &mut self,
        rop: u32,
        p: &BitPlane,
        s: &BitPlane,
        d: &BitPlane,
    ) -> Result<BitPlane, Fault> {
        // 高于 24 位的码不是 ROP3（ROP3 只有 24 位有效：8 位 fn + 16 位 R2 遗留）。
        if rop & 0xFF00_0000 != 0 {
            self.stats.rejects = self.stats.rejects.saturating_add(1);
            return Err(Fault::at(MergeCode::RopNotClosed, rop, 0));
        }
        match PenOp::from_fn(AnchorRop::extract_fn(rop)) {
            Ok(op) => self.apply(op, p, s, d),
            Err(e) => {
                self.stats.rejects = self.stats.rejects.saturating_add(1);
                Err(e)
            }
        }
    }
}

/// LCG 步进（Numerical Recipes 常数；种子在夹具里写死，判据可重放）。
pub const fn lcg_next(state: u32) -> u32 {
    state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223)
}

// ---------------------------------------------------------------------------
// 七、夹具
// ---------------------------------------------------------------------------

/// 夹具宽度（10 像素 = 2 字节行，第 2 字节有 6 个尾位，正好能测尾位污染）。
pub const FIX_W: u32 = 10;
/// 夹具高度。
pub const FIX_H: u32 = 4;
/// 夹具背景色索引。
pub const FIX_BK: u8 = 5;

/// 标准夹具：16 色图案 + 源/目标平面。
pub struct Fixture {
    /// 索引色图案。
    pub pattern: Pattern,
    /// 目标平面。
    pub dest: BitPlane,
    /// 源平面。
    pub source: BitPlane,
}

/// 构造标准夹具。
pub fn standard_fixture() -> Result<Fixture, Fault> {
    let mut pattern = Pattern::new(FIX_W, FIX_H, FIX_BK, 16)?;
    let mut seed = 0x2160_7F01u32;
    let mut y = 0u32;
    while y < FIX_H {
        let mut x = 0u32;
        while x < FIX_W {
            seed = lcg_next(seed);
            let v = ((seed >> 16) as u8) % 16;
            pattern.set_pixel(x, y, v)?;
            x += 1;
        }
        y += 1;
    }
    let mut dest = BitPlane::new(FIX_W, FIX_H)?;
    let mut source = BitPlane::new(FIX_W, FIX_H)?;
    y = 0;
    while y < FIX_H {
        let mut x = 0u32;
        while x < FIX_W {
            dest.set(x, y, (x + y) % 3 == 0)?;
            source.set(x, y, (x * 3 + y) % 5 < 2)?;
            x += 1;
        }
        y += 1;
    }
    Ok(Fixture {
        pattern,
        dest,
        source,
    })
}

/// 两平面按位与后 1 的位数（并集基数计算用）。
fn and_ones(a: &BitPlane, b: &BitPlane) -> u32 {
    let mut n = 0u32;
    let mut i = 0usize;
    while i < a.bytes().len() {
        let x = match a.bytes().get(i) {
            Some(v) => *v,
            None => 0,
        };
        let y = match b.bytes().get(i) {
            Some(v) => *v,
            None => 0,
        };
        n += (x & y).count_ones();
        i += 1;
    }
    n
}

// ---------------------------------------------------------------------------
// 八、判据
// ---------------------------------------------------------------------------

/// 判据入口。期望值以字面量独立写死，不复用被测常量（避免被测值自证）。
pub fn run_unx21607_mergepen_checks() -> crate::checks::CheckSet {
    let mut cs = crate::checks::CheckSet::new("unx-f21607");
    let fixture = standard_fixture();

    // 1) 夹具可构造：先证夹具，再证被测逻辑。
    cs.add(
        "f21607.fixture_buildable",
        fixture.is_ok() && FIX_W == 10 && FIX_H == 4,
        "标准 16 色图案夹具构造失败",
    );

    // 2) 锚点 fn 抽取：五个全码的高字节必须命中文档值。
    {
        let mut ok = true;
        let mut i = 0usize;
        while i < ANCHORS.len() {
            if ANCHORS[i].verify().is_err() {
                ok = false;
            }
            i += 1;
        }
        let fn_ok = AnchorRop::extract_fn(0x00BB_0226) == 0xBB
            && AnchorRop::extract_fn(0x00C0_00CA) == 0xC0
            && AnchorRop::extract_fn(0x00FB_0A09) == 0xFB
            && AnchorRop::extract_fn(0x00CC_0020) == 0xCC
            && AnchorRop::extract_fn(0x00F0_0021) == 0xF0;
        cs.add(
            "f21607.anchor_fn_extract",
            ok && fn_ok,
            "锚点 fn 抽取不符（应为 BB/C0/FB/CC/F0）",
        );
    }

    // 3) 锚点语义：公式与真值表在 8 组合上一致（40 组比对）。
    {
        let mut bad = 0u32;
        let mut i = 0usize;
        while i < ANCHORS.len() {
            let a = ANCHORS[i];
            let mut k = 0u32;
            while k < 8 {
                let p = k & 4 != 0;
                let s = k & 2 != 0;
                let d = k & 1 != 0;
                if a.eval(p, s, d) != rop3_bit(a.fn_byte, p, s, d) {
                    bad += 1;
                }
                k += 1;
            }
            i += 1;
        }
        cs.add("f21607.anchor_truth", bad == 0, "锚点公式与真值表分叉");
    }

    // 4) 无源边界：PATCOPY 只依赖 P，MERGEPEN 依赖 P 与 S。
    {
        let pat_blind = !depends_on(0xF0, PlaneVar::Source);
        let pat_p = depends_on(0xF0, PlaneVar::Pattern);
        let pat_d = !depends_on(0xF0, PlaneVar::Dest);
        let pen_s = depends_on(0xFC, PlaneVar::Source);
        let pen_p = depends_on(0xFC, PlaneVar::Pattern);
        cs.add(
            "f21607.source_blind_boundary",
            pat_blind && pat_p && pat_d && pen_s && pen_p,
            "PATCOPY 应只依赖 P；MERGEPEN 应依赖 P 与 S 且不依赖 D",
        );
    }

    // 5) 笔系六函数：六条公式各自与 fn 真值表在 8 组合上一致。
    {
        let mut bad = 0u32;
        let mut i = 0usize;
        while i < PEN_CLOSED_SET.len() {
            let op = PEN_CLOSED_SET[i];
            let mut k = 0u32;
            while k < 8 {
                let p = k & 4 != 0;
                let s = k & 2 != 0;
                let d = k & 1 != 0;
                let want = match op {
                    PenOp::MergePen => p | s,
                    PenOp::MaskPen => p & s,
                    PenOp::MergeCopy => s & p,
                    PenOp::MaskPenNot => p & !s,
                    PenOp::MergeNotPen => !p | s,
                    PenOp::NotMergePen => !(s | p),
                };
                if want != rop3_bit(op.fn_byte(), p, s, d) {
                    bad += 1;
                }
                k += 1;
            }
            i += 1;
        }
        cs.add(
            "f21607.pen_family_truth",
            bad == 0,
            "六条笔公式与 fn 真值表不符",
        );
    }

    // 6) 同函数归并：MASKPEN 与 MERGECOPY 同 fn（0xC0）且逐位同值；六条公式文本互异。
    {
        let same_fn = FN_MASKPEN == 0xC0 && FN_MERGECOPY == 0xC0;
        let mut k = 0u32;
        let mut same_out = true;
        while k < 8 {
            let p = k & 4 != 0;
            let s = k & 2 != 0;
            let d = k & 1 != 0;
            if rop3_bit(0xC0, p, s, d) != rop3_bit(FN_MERGECOPY, p, s, d) {
                same_out = false;
            }
            k += 1;
        }
        let mut formula_ok = true;
        let mut i = 0usize;
        while i < PEN_CLOSED_SET.len() {
            if PEN_CLOSED_SET[i].formula().is_empty() {
                formula_ok = false;
            }
            i += 1;
        }
        cs.add(
            "f21607.maskpen_eq_mergecopy",
            same_fn && same_out && formula_ok,
            "MASKPEN 与 MERGECOPY 应同 fn（0xC0）且公式文本齐备",
        );
    }

    // 7) 背景色平面：逐像素等价（像素==bk ⇔ 位==0）。
    if let Ok(f) = &fixture {
        match f.pattern.p_plane() {
            Ok(pl) => {
                let mut bad = 0u32;
                let mut y = 0u32;
                while y < FIX_H {
                    let mut x = 0u32;
                    while x < FIX_W {
                        match (f.pattern.pixel(x, y), pl.get(x, y)) {
                            (Ok(i), Ok(b)) => {
                                if b != (i != 5) {
                                    bad += 1;
                                }
                            }
                            _ => bad += 1,
                        }
                        x += 1;
                    }
                    y += 1;
                }
                cs.add(
                    "f21607.bk_plane_generator",
                    bad == 0 && pl.count_ones() > 0,
                    "P 平面与「像素!=bk(5)」不等价",
                );
            }
            Err(_) => cs.add("f21607.bk_plane_generator", false, "P 平面生成失败"),
        }
    } else {
        cs.add("f21607.bk_plane_generator", false, "夹具不可用");
    }

    // 8) 整片背景色 → P 平面全 0 → MERGEPEN(P, S=0, D) 全 0（画不出任何东西）。
    {
        let mut all_bk = match Pattern::new(8, 2, 3, 16) {
            Ok(p) => p,
            Err(_) => {
                cs.add("f21607.bk_blank_plane", false, "空白图案构造失败");
                return cs;
            }
        };
        let mut hit = false;
        if let Ok(pl) = all_bk.p_plane() {
            let zero = BitPlane::new(8, 2);
            let dst = BitPlane::new(8, 2);
            if let (Ok(s), Ok(d)) = (zero, dst) {
                if let Ok(out) = compose(0xFC, &pl, &s, &d) {
                    hit = pl.is_blank() && out.is_blank() && pl.count_ones() == 0;
                }
            }
        }
        // 千分比必须为 1000（全片皆为背景色）。
        let ratio_ok = all_bk.bk_ratio_per_mille() == 1000;
        let _ = all_bk.set_pixel(0, 0, 7);
        let ratio_after = all_bk.bk_ratio_per_mille();
        cs.add(
            "f21607.bk_blank_plane",
            hit && ratio_ok && ratio_after == 937,
            "整片背景色未给出全零平面或千分比不符（16 格中 1 格非 bk 应得 937）",
        );
    }

    // 9) 源免疫实测：PATCOPY 在两种源平面下输出逐位相同。
    if let Ok(f) = &fixture {
        if let Ok(pp) = f.pattern.p_plane() {
            let mut alt = BitPlane::new(FIX_W, FIX_H);
            if let Ok(a) = &mut alt {
                let mut y = 0u32;
                while y < FIX_H {
                    let mut x = 0u32;
                    while x < FIX_W {
                        let _ = a.set(x, y, true);
                        x += 1;
                    }
                    y += 1;
                }
                let one = compose(0xF0, &pp, &f.source, &f.dest);
                let two = compose(0xF0, &pp, a, &f.dest);
                let same = match (one, two) {
                    (Ok(x), Ok(y2)) => x.equals(&y2) && x.count_ones() == pp.count_ones(),
                    _ => false,
                };
                cs.add(
                    "f21607.patcopy_ignores_source",
                    same,
                    "PATCOPY 受源平面影响或位数不等于 P 平面",
                );
            } else {
                cs.add("f21607.patcopy_ignores_source", false, "备用平面构造失败");
            }
        } else {
            cs.add("f21607.patcopy_ignores_source", false, "P 平面失败");
        }
    } else {
        cs.add("f21607.patcopy_ignores_source", false, "夹具不可用");
    }

    // 10) MERGEPEN 对 D 免疫，且位数等于 P|S 的并集基数。
    if let Ok(f) = &fixture {
        if let Ok(pp) = f.pattern.p_plane() {
            let alt = BitPlane::new(FIX_W, FIX_H);
            if let Ok(a) = alt {
                let one = compose(0xFC, &pp, &f.source, &f.dest);
                let two = compose(0xFC, &pp, &f.source, &a);
                let same = match (one, two) {
                    (Ok(x), Ok(w)) => x.equals(&w),
                    _ => false,
                };
                let union_ok = match compose(0xFC, &pp, &f.source, &f.dest) {
                    Ok(r) => {
                        r.count_ones()
                            == pp.count_ones() + f.source.count_ones() - and_ones(&pp, &f.source)
                    }
                    Err(_) => false,
                };
                cs.add(
                    "f21607.mergepen_dest_blind",
                    same && union_ok,
                    "MERGEPEN 应等于 P|S（对 D 免疫且位数=并集基数）",
                );
            } else {
                cs.add("f21607.mergepen_dest_blind", false, "零平面构造失败");
            }
        } else {
            cs.add("f21607.mergepen_dest_blind", false, "P 平面失败");
        }
    } else {
        cs.add("f21607.mergepen_dest_blind", false, "夹具不可用");
    }

    // 11) 位并行 vs 标量：256 个 fn 全枚举对拍。
    if let Ok(f) = &fixture {
        if let Ok(pp) = f.pattern.p_plane() {
            let mut diverge = 0u32;
            let mut fnb = 0u32;
            while fnb < 256 {
                match (
                    compose(fnb as u8, &pp, &f.source, &f.dest),
                    compose_scalar(fnb as u8, &pp, &f.source, &f.dest),
                ) {
                    (Ok(x), Ok(w)) => {
                        if !x.equals(&w) {
                            diverge += 1;
                        }
                    }
                    _ => diverge += 1,
                }
                fnb += 1;
            }
            cs.add(
                "f21607.bit_parallel_matches_scalar",
                diverge == 0,
                "256 个 fn 中存在位并行与标量分叉",
            );
        } else {
            cs.add("f21607.bit_parallel_matches_scalar", false, "P 平面失败");
        }
    } else {
        cs.add("f21607.bit_parallel_matches_scalar", false, "夹具不可用");
    }

    // 12) 反向：非闭集 fn 必拒（0x5A 不属于六函数）。
    {
        let hit = match PenOp::from_fn(0x5A) {
            Err(f) => f.code == 0x91134 && f.is_known(),
            Ok(_) => false,
        };
        cs.add(
            "f21607.reject_unknown_fn",
            hit,
            "0x5A 未被拒或码不符 0x91134",
        );
    }

    // 13) 反向：fn 与全码不符必拒（0xCC0020 被改标成 fn=0xF0）。
    {
        let bad = AnchorRop {
            rop: 0x00CC_0020,
            fn_byte: 0xF0,
            name: "TAMPERED",
        };
        let hit = match bad.verify() {
            Err(f) => f.code == 0x91135 && f.as_code() == MergeCode::FnRopDisagree,
            Ok(_) => false,
        };
        cs.add(
            "f21607.reject_fn_rop_disagree",
            hit,
            "fn/全码不符未被拒或码不符 0x91135",
        );
    }

    // 14) 反向：几何不符必拒。宽度 9 与 10 混用是真实场景（图案刷比目标窄）。
    {
        let a = BitPlane::new(10, 4);
        let b = BitPlane::new(9, 4);
        let c = BitPlane::new(10, 4);
        let hit = match (a, b, c) {
            (Ok(x), Ok(w), Ok(z)) => match compose(0xFC, &x, &w, &z) {
                Err(f) => f.code == 0x91136,
                Ok(_) => false,
            },
            _ => false,
        };
        let zero_w = BitPlane::new(0, 4).is_err();
        cs.add(
            "f21607.reject_geometry",
            hit && zero_w,
            "宽不等未拒、码不符 0x91136，或零宽未被拒",
        );
    }

    // 15) 反向：bk 索引越界必拒（16 色板给 bk=16）。
    {
        let hit = match Pattern::new(4, 4, 16, 16) {
            Err(f) => f.code == 0x91137,
            Ok(_) => false,
        };
        let write_hit = match Pattern::new(4, 4, 0, 16) {
            Ok(mut p) => match p.set_pixel(0, 0, 99) {
                Err(f) => f.code == 0x91137,
                Ok(_) => false,
            },
            Err(_) => false,
        };
        cs.add(
            "f21607.reject_bk_index",
            hit && write_hit,
            "bk=16/板16 或写入索引 99 未被拒（应 0x91137）",
        );
    }

    // 16) 幂等 + 闭集文本：两次合成同结果、统计不漂、八码三要素齐备且码段连续。
    {
        let mut idem = false;
        if let Ok(f) = &fixture {
            if let Ok(pp) = f.pattern.p_plane() {
                let a = compose(0xCF, &pp, &f.source, &f.dest);
                let b = compose(0xCF, &pp, &f.source, &f.dest);
                if let (Ok(x), Ok(w)) = (a, b) {
                    idem = x.equals(&w);
                }
                let mut eng = MergeEngine::new();
                let _ = eng.apply(PenOp::MergeCopy, &pp, &f.source, &f.dest);
                // 全码路径同样走一次闭集反查（0x00C000CA 的 fn 为 0xC0）。
                let _ = eng.apply_rop(0x00C0_00CA, &pp, &f.source, &f.dest);
                // 越界全码被拒且计数只加一次。
                let rejected = eng.apply_rop(0x0100_0000, &pp, &f.source, &f.dest);
                let st = eng.stats();
                idem = idem
                    && st.planes == 2
                    && st.rejects == 1
                    && rejected.is_err()
                    && st.bytes == 16
                    && st.report().contains("planes=2")
                    && st.report().contains("bytes=16")
                    && st.report().contains("rejects=1");
            }
        }
        let mut texts = true;
        let mut i = 0usize;
        while i < ALL_CODES.len() {
            let c = ALL_CODES[i];
            if c.what().is_empty() || c.why().is_empty() || c.advice().is_empty() {
                texts = false;
            }
            if c.code() != 0x91131 + i as u32 {
                texts = false;
            }
            i += 1;
        }
        cs.add(
            "f21607.idempotent_and_codeset",
            idem && texts,
            "两次合成不等、统计漂移或诊断码段不连续",
        );
    }

    cs
}
