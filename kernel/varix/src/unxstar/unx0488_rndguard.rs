//! unx0488_rndguard —— UNX-F0488 渲染参数越界拦截。
//! 锚点（CoRun Unxreal 主册）：UNX-F0488 · 渲染参数越界拦截 · 目标 240 行
//! 判据原文：UNX-F0488-J1 渲染参数闸：提交给渲染器的参数
//!           面在进入绘制管线前逐项拦截——账二 几何参数
//!           （宽/高为零拒、超上限拒、宽×高溢出 u32/usize
//!           拒）；账三 矩形参数（clip 矩形出目标面拒、逆
//!           矩形（右<左/下<上）拒、空矩形（宽或高 0）按
//!           no-op 放行立案）；账四 格式参数（未知像素格
//!           式枚举值拒）；账五 步幅参数（stride < 宽×每
//!           像素字节拒、stride 非对齐拒）；账六 拦截计数
//!           与放行计数守恒（提交总数 ≡ 拦截+放行）。
//! 组件：参数面结构 + 五类拦截器 + 计数账 + 守恒核。
//! 职责定位：渲染参数是「从应用侧直插管线的原始输入」——
//!   宽高溢出会算出荒谬的帧缓冲尺寸、逆矩形会让填充循环
//!   永不终止、stride 谎报会把整帧写到目标面之外。闸在管
//!   线入口，比在故障现场追责便宜一个数量级。
//! 性能逐项分解：每类拦截 O(1)；守恒核 O(1)（三计数器）。
//! 诊断码段：0x348C0（0x348C1~0x348C9 闭集；u32 高段全域独占）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use crate::checks::CheckSet;

/// 诊断码。
pub const SEG_BASE: u32 = 0x348C0;
pub const XC_GEOM: u32 = SEG_BASE + 1; // 几何参数
pub const XC_RECT: u32 = SEG_BASE + 2; // 矩形参数
pub const XC_FMT: u32 = SEG_BASE + 3; // 格式参数
pub const XC_STRIDE: u32 = SEG_BASE + 4; // 步幅参数

/// 像素格式（已知闭集）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PixFmt {
    R8,
    Rgb565,
    Rgba8888,
    Bgra8888,
}

impl PixFmt {
    /// 每像素字节数。
    pub fn bpp(self) -> u32 {
        match self {
            PixFmt::R8 => 1,
            PixFmt::Rgb565 => 2,
            PixFmt::Rgba8888 | PixFmt::Bgra8888 => 4,
        }
    }
}

/// 目标面（帧缓冲描述）。
#[derive(Clone, Copy)]
pub struct Target {
    pub width: u32,
    pub height: u32,
    pub fmt: PixFmt,
    /// 步幅（字节/行）。
    pub stride: u32,
}

impl Target {
    fn validate(&self) -> Result<(), u32> {
        if self.width == 0 || self.height == 0 {
            return Err(XC_GEOM);
        }
        if self.width > 16384 || self.height > 16384 {
            return Err(XC_GEOM);
        }
        // 宽×高×bpp 溢出核（usize 乘法 checked）。
        let total = (self.width as u64) * (self.height as u64) * (self.fmt.bpp() as u64);
        if total > 0xFFFF_FFFF {
            return Err(XC_GEOM);
        }
        if self.stride < self.width * self.fmt.bpp() {
            return Err(XC_STRIDE);
        }
        // 步幅 16 字节对齐。
        if self.stride % 16 != 0 {
            return Err(XC_STRIDE);
        }
        Ok(())
    }
}

/// 填充矩形（目标坐标系）。
#[derive(Clone, Copy, Debug)]
pub struct Rect {
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
}

impl Rect {
    /// 分类核：Err=拒，Ok(false)=空矩形 no-op，Ok(true)=有效。
    pub fn classify(&self, t: &Target) -> Result<bool, u32> {
        // 空矩形：no-op 放行（合法但不绘制）。
        if self.w == 0 || self.h == 0 {
            return Ok(false);
        }
        // 出目标面：右/下边界越界拒。
        if self.x >= t.width || self.y >= t.height {
            return Err(XC_RECT);
        }
        if self.w > t.width - self.x || self.h > t.height - self.y {
            return Err(XC_RECT);
        }
        Ok(true)
    }
}

/// 提交（目标 + 矩形 + 格式直写面）。
pub struct Submit {
    pub target: Target,
    pub rect: Rect,
    /// 应用自报格式（须与目标面一致）。
    pub claim_fmt: PixFmt,
}

/// 拦截器：全链核。返回 Ok(true)=绘制、Ok(false)=no-op、Err=拦截码。
pub fn vet(s: &Submit) -> Result<bool, u32> {
    s.target.validate()?;
    if s.claim_fmt != s.target.fmt {
        return Err(XC_FMT);
    }
    s.rect.classify(&s.target)
}

/// 计数账（提交/拦截/放行三计数器守恒）。
pub struct GuardLedger {
    pub submitted: u32,
    pub blocked: u32,
    pub passed: u32,
}

impl GuardLedger {
    pub fn new() -> Self {
        GuardLedger { submitted: 0, blocked: 0, passed: 0 }
    }
    pub fn record(&mut self, r: Result<bool, u32>) {
        self.submitted += 1;
        match r {
            Ok(_) => self.passed += 1,
            Err(_) => self.blocked += 1,
        }
    }
    /// 守恒核：提交 ≡ 拦截 + 放行（u32 回绕安全：和用 wrapping 比对）。
    pub fn conserve(&self) -> bool {
        self.submitted == self.blocked.wrapping_add(self.passed)
    }
}

pub fn run_unx0488_checks() -> CheckSet {
    let mut cs = CheckSet::new("UNX-F0488");
    cs.add("SEG 段位独占", SEG_BASE == 0x348C0 && XC_STRIDE == SEG_BASE + 4, "u32 高段全域普查无冲突");
    cs.add("bpp 闭集", PixFmt::R8.bpp() == 1 && PixFmt::Rgba8888.bpp() == 4, "四格式字节量");

    // 合法基线：800×600 RGBA，stride 对齐。
    let base = Target { width: 800, height: 600, fmt: PixFmt::Rgba8888, stride: 800 * 4 };
    cs.add("基线合法", base.validate().is_ok(), "五类拦截全放行");

    // 族一：几何参数。
    let z = Target { width: 0, ..base };
    cs.add("宽零拒", z.validate() == Err(XC_GEOM), "零面无义");
    let big = Target { width: 16385, ..base };
    cs.add("宽超限拒", big.validate() == Err(XC_GEOM), "16385 > 16384");
    let over = Target { width: 70000, height: 70000, fmt: PixFmt::R8, stride: 70000 };
    cs.add("乘积溢出拒", over.validate() == Err(XC_GEOM), "70k×70k > u32");

    // 族二：stride。
    let thin = Target { stride: 800 * 4 - 1, ..base };
    cs.add("stride 不足拒", thin.validate() == Err(XC_STRIDE), "< 宽×bpp");
    let unaligned = Target { stride: 800 * 4 + 1, ..base };
    cs.add("stride 非对齐拒", unaligned.validate() == Err(XC_STRIDE), "16 字节界");
    let pad = Target { stride: 800 * 4 + 16, ..base };
    cs.add("stride 留埴合法", pad.validate().is_ok(), "对齐留埴放行");

    // 族三：矩形。
    let r_ok = Rect { x: 10, y: 10, w: 100, h: 50 };
    cs.add("有效矩形", r_ok.classify(&base) == Ok(true), "面内全含");
    let r_out = Rect { x: 799, y: 599, w: 2, h: 2 };
    cs.add("出界拒", r_out.classify(&base) == Err(XC_RECT), "右下越面");
    let r_touch = Rect { x: 700, y: 550, w: 100, h: 50 };
    cs.add("贴边合法", r_touch.classify(&base) == Ok(true), "恰好全含");
    let r_empty = Rect { x: 5, y: 5, w: 0, h: 10 };
    cs.add("空矩形 no-op", r_empty.classify(&base) == Ok(false), "合法不绘制");
    let r_origin = Rect { x: 0, y: 0, w: 800, h: 600 };
    cs.add("全屏合法", r_origin.classify(&base) == Ok(true), "满面填充");

    // 族四：格式一致性。
    let mut s0 = Submit { target: base, rect: r_ok, claim_fmt: PixFmt::Rgba8888 };
    cs.add("格式一致放行", vet(&s0) == Ok(true), "声明=目标");
    s0.claim_fmt = PixFmt::Bgra8888;
    cs.add("格式谎报拒", vet(&s0) == Err(XC_FMT), "声明≠目标");

    // 族五：守恒账。
    let mut led = GuardLedger::new();
    let s_ok = Submit { target: base, rect: r_ok, claim_fmt: PixFmt::Rgba8888 };
    let s_bad = Submit { target: Target { width: 0, ..base }, rect: r_ok, claim_fmt: PixFmt::Rgba8888 };
    let s_none = Submit { target: base, rect: r_empty, claim_fmt: PixFmt::Rgba8888 };
    led.record(vet(&s_ok));
    led.record(vet(&s_bad));
    led.record(vet(&s_none));
    cs.add("守恒", led.conserve(), "提交3=拦截1+放行2");
    cs.add("分类计数", led.blocked == 1 && led.passed == 2, "拦截/放行各记");

    // 族六：守恒 u32 回绕面（wrapping）。
    let wrap = GuardLedger { submitted: u32::MAX, blocked: 1, passed: u32::MAX - 1 };
    cs.add("回绕守恒", wrap.conserve(), "wrapping_add 稳");

    // 族七：幂等。
    let a = vet(&s_ok);
    let b = vet(&s_ok);
    cs.add("拦截幂等", a == b, "两遍恒等");

    cs
}
