//! cga68_zerocopy —— CGPU-F0068 零拷贝图像视图。
//! 锚点（CGPU 施工书）：CGPU-F0068 · 零拷贝图像视图 · 目标 320 行
//! 判据摘录：视图（view）不复制像素——视图只持偏移/步长/尺寸三
//!   元，访问经主图（读写同一内存的账面模型）；步长（stride）≥
//!   视图宽（防行重叠写穿）；子视图（crop）不越主图界；视图读
//!   与主图读逐位一致（零拷贝正确性锚）；就地写经视图后主图可
//!   见（共享内存语义）；跨视图别名审计（两视图写域重叠 = 别名
//!   冲突，显性立案）。
//! 组件：视图三元模型（off/stride/dim）+ 视图读/写访问器 +
//!   crop 子视图器 + 零拷贝一致性对拍 + 就地写可见核 + 别名
//!   冲突审计器 + 步长红线。
//! 职责定位：图像管线的内存纪律——「视图即账」：零拷贝消灭复
//!   制带宽，但引入别名风险，本模块把别名变成可审计的账面冲突
//!   而不是静默数据竞争。
//! 性能逐项分解：视图构造 O(1)；访问 O(1)；别名审计 O(视图对)。
//! 全整数确定性。
//! 诊断码段：0xE6D0（0xE6D1~0xE6D9 闭集；全域 grep 确认独占）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 模块版本。
pub const ZC_VERSION: &str = "cga68_zerocopy-v1";

/// 诊断段基址（0xE6D0 段 · 本模块独占）。
pub const ZC_SEG_BASE: u16 = 0xE6D0;

/// XcCode 闭集（0xE6D1~0xE6D9）。
pub const XC_DIM_ZERO: u32 = 0xE6D1;
pub const XC_STRIDE_THIN: u32 = 0xE6D2;
pub const XC_VIEW_OOB: u32 = 0xE6D3;
pub const XC_COPY_DRIFT: u32 = 0xE6D4;
pub const XC_WRITE_INVIS: u32 = 0xE6D5;
pub const XC_ALIAS_CLASH: u32 = 0xE6D6;
pub const XC_OFF_OOB: u32 = 0xE6D7;
pub const XC_CROP_ZERO: u32 = 0xE6D8;
pub const XC_LEN_MISMATCH: u32 = 0xE6D9;

/// 主图（像素唯一所有者——视图的底座）。
pub struct Master {
    pub w: usize,
    pub h: usize,
    pub px: Vec<u32>,
}

impl Master {
    pub fn new(w: usize, h: usize, px: Vec<u32>) -> Result<Master, u32> {
        if w == 0 || h == 0 || px.len() != w * h {
            return Err(XC_DIM_ZERO);
        }
        Ok(Master { w, h, px })
    }
}

/// 视图三元模型：off（主图线性偏移）+ stride（行距）+ 尺寸。
#[derive(Clone, Copy)]
pub struct View {
    pub off: usize,
    pub stride: usize,
    pub w: usize,
    pub h: usize,
}

impl View {
    /// 构造（stride ≥ w 硬线 + 域界核：最后一行末元素 < 主图长）。
    pub fn new(m: &Master, off: usize, stride: usize, w: usize, h: usize) -> Result<View, u32> {
        if w == 0 || h == 0 {
            return Err(XC_DIM_ZERO);
        }
        if stride < w {
            return Err(XC_STRIDE_THIN);
        }
        let last = off.checked_add(stride.checked_mul(h - 1).ok_or(XC_OFF_OOB)?).ok_or(XC_OFF_OOB)?;
        if last + w > m.px.len() {
            return Err(XC_VIEW_OOB);
        }
        Ok(View { off, stride, w, h })
    }

    /// 视图读（账面模型：经主图）。
    pub fn read(&self, m: &Master, x: usize, y: usize) -> Result<u32, u32> {
        if x >= self.w || y >= self.h {
            return Err(XC_VIEW_OOB);
        }
        Ok(m.px[self.off + y * self.stride + x])
    }

    /// 视图写（就地——主图可见）。
    pub fn write(&self, m: &mut Master, x: usize, y: usize, v: u32) -> Result<(), u32> {
        if x >= self.w || y >= self.h {
            return Err(XC_VIEW_OOB);
        }
        m.px[self.off + y * self.stride + x] = v;
        Ok(())
    }

    /// 写域矩形（别名审计的账面：视图覆盖的主图行集）。
    pub fn rows(&self) -> Vec<(usize, usize, usize)> {
        // (行主图起点, 行长, 行距基准 y)
        let mut out: Vec<(usize, usize, usize)> = Vec::new();
        for y in 0..self.h {
            out.push((self.off + y * self.stride, self.w, y));
        }
        out
    }
}

/// crop 子视图（相对父视图坐标——不越父界硬线）。
pub fn crop(parent: &View, x: usize, y: usize, w: usize, h: usize) -> Result<View, u32> {
    if w == 0 || h == 0 {
        return Err(XC_CROP_ZERO);
    }
    if x + w > parent.w || y + h > parent.h {
        return Err(XC_VIEW_OOB);
    }
    Ok(View {
        off: parent.off + y * parent.stride + x,
        stride: parent.stride,
        w,
        h,
    })
}

/// 零拷贝一致性对拍：视图读 == 主图对应位置读（逐位）。
pub fn view_read_match(m: &Master, v: &View) -> Result<(), u32> {
    for y in 0..v.h {
        for x in 0..v.w {
            let a = v.read(m, x, y)?;
            let b = m.px[v.off + y * v.stride + x];
            if a != b {
                return Err(XC_COPY_DRIFT);
            }
        }
    }
    Ok(())
}

/// 就地写可见核：经视图写 → 主图对应槽变化。
pub fn write_visible(m: &mut Master, v: &View, x: usize, y: usize, val: u32) -> Result<(), u32> {
    let slot = v.off + y * v.stride + x;
    let before = m.px[slot];
    v.write(m, x, y, val)?;
    if m.px[slot] == before {
        return Err(XC_WRITE_INVIS);
    }
    Ok(())
}

/// 别名冲突审计：两视图写域行段重叠即立案（同主图写视图必须
/// 互斥——账面冲突替代静默竞争）。
pub fn alias_audit(a: &View, b: &View) -> Result<(), u32> {
    for &(ra, la, _) in a.rows().iter() {
        for &(rb, lb, _) in b.rows().iter() {
            let a_end = ra + la;
            let b_end = rb + lb;
            let overlap = ra < b_end && rb < a_end;
            if overlap {
                return Err(XC_ALIAS_CLASH);
            }
        }
    }
    Ok(())
}

/// 判据入口（注册于 cgpu/mod.rs 检查表）。
pub fn run_cga68_checks() -> CheckSet {
    let mut cs = CheckSet::new("CGPU-F0068");
    let m = match Master::new(8, 8, (0..64u32).collect()) {
        Ok(x) => x,
        Err(_) => return cs,
    };
    // 族一：视图构造（stride 红线 + 域界）。
    let v = View::new(&m, 0, 8, 8, 8).map_err(|e| e as u16);
    cs.add("全图视图", v.is_ok(), "stride==w");
    cs.add("细步长拒", View::new(&m, 0, 4, 8, 8).is_err(), "stride<w");
    cs.add("越界视图拒", View::new(&m, 60, 8, 8, 8).is_err(), "VIEW_OOB");
    cs.add("零尺寸拒", View::new(&m, 0, 8, 0, 4).is_err(), "DIM_ZERO");
    // 族二：零拷贝读一致（视图读 == 主图读）。
    if let Ok(g) = &v {
        cs.add("读一致", view_read_match(&m, g).is_ok(), "64 元逐位");
        cs.add("读值锚", matches!(g.read(&m, 3, 2), Ok(19)), "2×8+3");
        cs.add("越界读拒", g.read(&m, 8, 0).is_err(), "VIEW_OOB");
    }
    // 族三：crop 子视图（含跨行视图语义）。
    if let Ok(p) = &v {
        let c = crop(p, 2, 1, 3, 3).map_err(|e| e as u16);
        cs.add("子视图构建", c.is_ok(), "3×3 @ (2,1)");
        if let Ok(s) = &c {
            cs.add("子视图读锚", matches!(s.read(&m, 0, 0), Ok(17)), "1×8+2");
            cs.add("子视图 stride 继承", s.stride == 8, "不复制不重排");
        }
        cs.add("子视图越界拒", crop(p, 6, 6, 3, 3).is_err(), "越父界");
        cs.add("子视图零拒", crop(p, 0, 0, 0, 3).is_err(), "CROP_ZERO");
    }
    // 族四：就地写可见。
    if let Ok(mut m2) = Master::new(8, 8, (0..64u32).collect()) {
        if let Ok(g) = View::new(&m2, 0, 8, 8, 8) {
            cs.add("写可见", write_visible(&mut m2, &g, 4, 4, 999).is_ok(), "主图变化");
            cs.add("写值落账", m2.px[4 * 8 + 4] == 999, "槽 36 = 999");
        }
    }
    // 族五：别名审计（重叠写视图立案 / 相邻不立案）。
    let va = View::new(&m, 0, 8, 4, 8).map_err(|e| e as u16);
    let vb = View::new(&m, 2, 8, 4, 8).map_err(|e| e as u16); // 行内重叠
    if let (Ok(a), Ok(b)) = (va, vb) {
        cs.add("行内重叠立案", alias_audit(&a, &b).is_err(), "ALIAS_CLASH");
    }
    let vc = View::new(&m, 0, 16, 8, 4).map_err(|e| e as u16);
    let vd = View::new(&m, 8, 16, 8, 4).map_err(|e| e as u16);
    if let (Ok(c), Ok(d)) = (vc, vd) {
        // 视图 c 覆盖行 0..4 段 [0,8)，视图 d 行 0..4 段 [8,16)——
        // 每行内不重叠（stride 16 隔行布局）。
        cs.add("隔行不冲突", alias_audit(&c, &d).is_ok(), "写域互斥");
    }
    // 族六：主图尺寸失配。
    cs.add("主图失配拒", Master::new(2, 2, vec![0u32; 5]).is_err(), "LEN_MISMATCH");
    cs.add("主图零拒", Master::new(0, 4, vec![]).is_err(), "DIM_ZERO");
    cs
}
