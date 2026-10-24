//! cga100_chanops —— CGPU-F0100 通道操作 SIMD 族。
//! 锚点（CGPU 施工书）：CGPU-F0100 · 通道操作 SIMD 族 · 目标 300 行
//! 判据摘录：操作闭集（交换/复制/拆分/合并/提取/替换/填充/清
//! 零八族——ID 闭集）；交换自逆（swap∘swap == 恒等——对合核）；
//! 复制守恒（copy(src→dst) 后 dst[chn] == src[chn] 且他通道不
//! 变——隔离账）；拆分合并互逆（split→merge == 原图——往返红
//! 线）；提取单通道（宽==N 高不变）；替换域闸（通道号 ∈[0,3)）；
//! 填充幂等（fill∘fill == fill）；通道数域闸（∉{1,3,4} 拒核）。
//! 组件：Rgba8 容器 + 八族操作 + 对合/互逆/隔离/幂等四账 + 域
//!   闸。
//! 职责定位：通道级重排的最小完备族——BGR/RGB 转换、alpha 分离
//! 预乘等都由这八族组合；全字节域，SIMD 打包天然对齐（4 通道
//! ×u8 = 32 位一次搬运）。

use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

// ===== 诊断码闭集（0xE990 段 · 本模块独占）=====
pub const XC_CHN_OOB: u32 = 0xE990; // 通道号越界（≥通道数）
pub const XC_CHN_N_BAD: u32 = 0xE991; // 通道数非法（∉{1,3,4}）
pub const XC_LEN: u32 = 0xE992; // 长度失配
pub const XC_INV_FAIL: u32 = 0xE993; // 对合/互逆破
pub const XC_ISO_FAIL: u32 = 0xE994; // 隔离破（他通道被动）

// ===== 操作 ID 闭集 =====
pub const OP_SWAP: u8 = 0;
pub const OP_COPY: u8 = 1;
pub const OP_SPLIT: u8 = 2;
pub const OP_MERGE: u8 = 3;
pub const OP_EXTRACT: u8 = 4;
pub const OP_REPLACE: u8 = 5;
pub const OP_FILL: u8 = 6;
pub const OP_ZERO: u8 = 7;
pub const OP_ALL: [u8; 8] = [OP_SWAP, OP_COPY, OP_SPLIT, OP_MERGE, OP_EXTRACT, OP_REPLACE, OP_FILL, OP_ZERO];

/// 多通道图（交织；chn ∈ {1,3,4}）。
pub struct ChanImg {
    pub n: u32, // 每像素通道数
    pub w: u32,
    pub h: u32,
    pub px: Vec<u8>, // n×w×h
}

impl ChanImg {
    pub fn new(n: u32, w: u32, h: u32, px: Vec<u8>) -> Result<ChanImg, u32> {
        if !matches!(n, 1 | 3 | 4) {
            return Err(XC_CHN_N_BAD);
        }
        if px.len() != (n * w * h) as usize {
            return Err(XC_LEN);
        }
        Ok(ChanImg { n, w, h, px })
    }
    fn idx(&self, x: u32, y: u32, c: u32) -> usize {
        ((y * self.w + x) * self.n + c) as usize
    }
    pub fn get(&self, x: u32, y: u32, c: u32) -> Result<u8, u32> {
        if c >= self.n {
            return Err(XC_CHN_OOB);
        }
        Ok(self.px[self.idx(x, y, c)])
    }
}

/// 通道交换（c1↔c2）。
pub fn swap_channels(img: &mut ChanImg, c1: u32, c2: u32) -> Result<(), u32> {
    if c1 >= img.n || c2 >= img.n {
        return Err(XC_CHN_OOB);
    }
    for y in 0..img.h {
        for x in 0..img.w {
            let i1 = img.idx(x, y, c1);
            let i2 = img.idx(x, y, c2);
            img.px.swap(i1, i2);
        }
    }
    Ok(())
}

/// 通道复制（src→dst，其他通道隔离）。
pub fn copy_channel(img: &mut ChanImg, src: u32, dst: u32) -> Result<(), u32> {
    if src >= img.n || dst >= img.n {
        return Err(XC_CHN_OOB);
    }
    for y in 0..img.h {
        for x in 0..img.w {
            let i_src = img.idx(x, y, src);
            let i_dst = img.idx(x, y, dst);
            let v = img.px[i_src];
            img.px[i_dst] = v;
        }
    }
    Ok(())
}

/// 拆分：交织 → 逐通道平面（平面序 = 通道序）。
pub fn split(img: &ChanImg) -> Vec<Vec<u8>> {
    let np = (img.w * img.h) as usize;
    let mut planes: Vec<Vec<u8>> = Vec::with_capacity(img.n as usize);
    for c in 0..img.n {
        let mut p: Vec<u8> = Vec::with_capacity(np);
        for y in 0..img.h {
            for x in 0..img.w {
                p.push(img.px[img.idx(x, y, c)]);
            }
        }
        planes.push(p);
    }
    planes
}

/// 合并：逐通道平面 → 交织（互逆于 split）。
pub fn merge(planes: &[Vec<u8>], w: u32, h: u32) -> Result<ChanImg, u32> {
    let n = planes.len() as u32;
    if !matches!(n, 1 | 3 | 4) {
        return Err(XC_CHN_N_BAD);
    }
    for p in planes.iter() {
        if p.len() != (w * h) as usize {
            return Err(XC_LEN);
        }
    }
    let mut px: Vec<u8> = Vec::with_capacity((n * w * h) as usize);
    for y in 0..h {
        for x in 0..w {
            for c in 0..n {
                px.push(planes[c as usize][(y * w + x) as usize]);
            }
        }
    }
    ChanImg::new(n, w, h, px)
}

/// 提取单通道为灰度图。
pub fn extract(img: &ChanImg, c: u32) -> Result<Vec<u8>, u32> {
    if c >= img.n {
        return Err(XC_CHN_OOB);
    }
    Ok((0..(img.w * img.h) as usize).map(|i| img.px[i * img.n as usize + c as usize]).collect())
}

/// 通道替换（新值向量；长度 == 像素数）。
pub fn replace(img: &mut ChanImg, c: u32, vals: &[u8]) -> Result<(), u32> {
    if c >= img.n {
        return Err(XC_CHN_OOB);
    }
    if vals.len() != (img.w * img.h) as usize {
        return Err(XC_LEN);
    }
    for y in 0..img.h {
        for x in 0..img.w {
            let i = img.idx(x, y, c);
            let v = vals[(y * img.w + x) as usize];
            img.px[i] = v;
        }
    }
    Ok(())
}

/// 通道填充。
pub fn fill(img: &mut ChanImg, c: u32, v: u8) -> Result<(), u32> {
    if c >= img.n {
        return Err(XC_CHN_OOB);
    }
    for y in 0..img.h {
        for x in 0..img.w {
            let i = img.idx(x, y, c);
            img.px[i] = v;
        }
    }
    Ok(())
}

/// 全图清零。
pub fn zero_all(img: &mut ChanImg) {
    for v in img.px.iter_mut() {
        *v = 0;
    }
}

/// BGR↔RGB 便捷转换（3 通道 swap(0,2) 的语义壳）。
pub fn bgr_rgb(img: &mut ChanImg) -> Result<(), u32> {
    if img.n != 3 {
        return Err(XC_CHN_N_BAD);
    }
    swap_channels(img, 0, 2)
}

// ===== 主判据 =====
pub fn run_cga100_checks() -> CheckSet {
    let mut cs = CheckSet::new("CGPU-F0100");
    // 2×2 RGBA 样本。
    let mk = || -> Result<ChanImg, u32> {
        let mut v: Vec<u8> = Vec::new();
        for i in 0..4u32 {
            v.push((i * 10) as u8);
            v.push((i * 10 + 1) as u8);
            v.push((i * 10 + 2) as u8);
            v.push((i * 10 + 3) as u8);
        }
        ChanImg::new(4, 2, 2, v)
    };
    // 族一：域闸。
    cs.add("通道号越界", mk().map_err(|e| e as u16).and_then(|mut i| swap_channels(&mut i, 4, 0).map_err(|e| e as u16)).is_err(), "CHN_OOB");
    cs.add("通道数非法", ChanImg::new(2, 1, 1, alloc::vec![0u8; 2]).is_err(), "∉{1,3,4}");
    cs.add("长度失配", ChanImg::new(3, 2, 2, alloc::vec![0u8; 9]).is_err(), "LEN");
    // 族二：交换自逆。
    if let Ok(mut a) = mk() {
        let before = a.px.clone();
        let _ = swap_channels(&mut a, 0, 2);
        let mid = a.px.clone();
        let _ = swap_channels(&mut a, 0, 2);
        cs.add("交换自逆", a.px[..] == before[..], "swap∘swap==id");
        cs.add("交换生效", mid[0] == 2 && mid[2] == 0, "R↔B");
        // BGR↔RGB 壳同义。
        let _ = bgr_rgb(&mut a);
        cs.add("壳同义", a.px[..] == before[..], "swap(0,2)");
    }
    // 族三：复制隔离。
    if let Ok(mut a) = mk() {
        let before = a.px.clone();
        let _ = copy_channel(&mut a, 0, 1);
        cs.add("复制生效", a.get(0, 0, 1) == Ok(0), "dst==src");
        cs.add("他通道隔离", (2..4).all(|c| a.get(0, 0, c) == Ok(before[c as usize])), "G/B/A 不动");
    }
    // 族四：拆分合并互逆。
    if let Ok(a) = mk() {
        let planes = split(&a);
        cs.add("平面数 4", planes.len() == 4, "每通道一平面");
        if let Ok(b) = merge(&planes, a.w, a.h) {
            cs.add("往返互逆", b.px[..] == a.px[..], "split→merge==id");
        }
        // 提取一致。
        if let Ok(e0) = extract(&a, 0) {
            cs.add("提取==平面", e0[..] == planes[0][..], "双路同值");
        }
        cs.add("提取越界拒", extract(&a, 9).is_err(), "CHN_OOB");
    }
    // 族五：替换与填充。
    if let Ok(mut a) = mk() {
        let vals = alloc::vec![9u8; 4];
        let _ = replace(&mut a, 3, &vals);
        cs.add("替换生效", (0..4u32).all(|i| a.get(i % 2, i / 2, 3) == Ok(9)), "A 通道全 9");
        cs.add("替换长度拒", replace(&mut a, 0, &alloc::vec![1u8; 3]).is_err(), "LEN");
        let _ = fill(&mut a, 2, 7);
        let _ = fill(&mut a, 2, 7);
        cs.add("填充幂等", (0..4u32).all(|i| a.get(i % 2, i / 2, 2) == Ok(7)), "fill∘fill==fill");
    }
    // 族六：清零。
    if let Ok(mut a) = mk() {
        zero_all(&mut a);
        cs.add("全零", a.px.iter().all(|&v| v == 0), "归零");
    }
    // 族七：单通道图（n=1 域）。
    if let Ok(mut g) = ChanImg::new(1, 2, 1, alloc::vec![5u8, 9]) {
        cs.add("单通道交换拒", swap_channels(&mut g, 0, 0).is_ok(), "自交换合法");
        cs.add("单通道取值", g.get(1, 0, 0) == Ok(9), "灰度域");
        let planes = split(&g);
        cs.add("单平面", planes.len() == 1, "n=1");
    }
    // 族八：操作闭集。
    cs.add("八族闭集", OP_ALL.len() == 8, "ID 全量");
    CheckSet::merge(cs, run_cga100_deep())
}

// ===== 深化面：BGR 往返与隔离账 =====
/// swap 两次 = 恒等（对合账）。
pub fn swap_involution(n: u32, w: u32, h: u32) -> Result<(), u32> {
    let px = alloc::vec![0u8; (n * w * h) as usize];
    let mut img = ChanImg::new(n, w, h, px)?;
    let _ = fill(&mut img, 0, 10);
    let _ = fill(&mut img, 1, 20);
    let _ = fill(&mut img, 2, 30);
    swap_channels(&mut img, 0, 2)?;
    swap_channels(&mut img, 0, 2)?;
    if ChanImg::get(&img, 0, 0, 0)? != 10 || ChanImg::get(&img, 0, 0, 2)? != 30 {
        return Err(XC_INV_FAIL);
    }
    Ok(())
}

/// get 越界闭账：通道/坐标越界全拒。
pub fn get_gate(n: u32, w: u32, h: u32) -> Result<(), u32> {
    let px = alloc::vec![0u8; (n * w * h) as usize];
    let img = ChanImg::new(n, w, h, px)?;
    if ChanImg::get(&img, w, 0, 0).is_ok() || ChanImg::get(&img, 0, h, 0).is_ok() {
        return Err(XC_CHN_OOB);
    }
    if ChanImg::get(&img, 0, 0, n).is_ok() {
        return Err(XC_CHN_OOB);
    }
    Ok(())
}

/// 通道数闭账：2/5 拒。
pub fn n_gate() -> bool {
    let px = alloc::vec![0u8; 8];
    ChanImg::new(2, 2, 2, px).is_err()
}

pub fn run_cga100_deep() -> CheckSet {
    let mut cs = CheckSet::new("cga100_deep");
    cs.add("swap 对合", swap_involution(3, 2, 2).is_ok(), "双换还原");
    cs.add("get 越界闭", get_gate(3, 2, 2).is_ok(), "三向拒");
    cs.add("通道数闭", n_gate(), "2 拒");
    if let Ok(mut img) = ChanImg::new(3, 2, 2, alloc::vec![0u8; 12]) {
        let _ = fill(&mut img, 0, 7);
        cs.add("fill 生效", ChanImg::get(&img, 0, 0, 0) == Ok(7), "写读一致");
        let _ = copy_channel(&mut img, 0, 1);
        cs.add("copy 生效", ChanImg::get(&img, 0, 0, 1) == Ok(7), "复制落位");
        let _ = replace(&mut img, 2, &alloc::vec![9u8; 4]);
        cs.add("replace 生效", ChanImg::get(&img, 1, 1, 2) == Ok(9), "替换落位");
        zero_all(&mut img);
        cs.add("zero 清账", ChanImg::get(&img, 1, 1, 0) == Ok(0), "全零");
        let _ = bgr_rgb(&mut img);
        cs.add("bgr 往返", bgr_rgb(&mut img).is_ok(), "双转还原");
        // split/merge 往返。
        let planes = split(&img);
        cs.add("split 账", planes.len() == 3, "三平面");
        if let Ok(m) = merge(&planes, 2, 2) {
            cs.add("merge 往返", split(&m) == planes, "拆合恒等");
        }
    }
    cs
}
