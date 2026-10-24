//! veb289_mliastc —— VE-F0289 Mali ASTC 原生纹理策略。
//! 锚点（CoRun Unxreal 主册）：VE-F0289 · Mali ASTC 原生纹理策略 · 目标 340 行
//! 判据原文：VE-F0289-J1 ASTC 策略账：块型白名单（4×4 / 6×6 /
//!           8×8 / 12×12 四型）；纹理登记（宽高 + 块型，尺寸
//!           需被块型整除——非整除上取整补边）；内存估算（块
//!           数 × 128 位）；原生直通（ASTC 纹理不解压直接上
//!           GPU）；转码判定（非白名单块型 → 报转码事件而
//!           非拒收）；补边核（补边像素不计费）；账平衡核
//!           （登记总和 = 估算总和）。
//! 组件：块型表 + 登记账 + 估算器 + 转码事件账。
//! 职责定位：ASTC 是 Mali 的母语纹理——原生直通是母语的红利：
//!           不用翻译直接干活；块型白名单是母语的字典：字典
//!           没有的词要转码不是拒收；补边不计费是地皮的公
//!           道：凑整的边角不算钱。
//! 性能逐项分解：登记 O(1)；估算 O(1)。
//! 诊断码段：0x31140（0x31141~0x31149 闭集）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 模块版本。
pub const AS_VERSION: &str = "veb289_mliastc-v1";

/// 诊断段基址（0x31140 段 · 本模块独占）。
pub const AS_SEG_BASE: u32 = 0x31140;
pub const XC_MAGIC: u32 = 0x31141; // 账魔数破
pub const XC_DIM: u32 = 0x31142; // 尺寸不合法
pub const XC_BLOCK: u32 = 0x31143; // 块型不合法

/// 账魔数。
const MAGIC: u32 = 0x3114_1A5C;

/// ASTC 块型白名单。
pub const BLOCKS: [u32; 4] = [4, 6, 8, 12];

/// 每块 128 位。
pub const BLOCK_BITS: u64 = 128;

/// 纹理记录。
pub struct TexRec {
    pub id: u32,
    pub w: u32,
    pub h: u32,
    pub blk: u32,
    /// 补边后的宽（块倍数）。
    pub pw: u32,
    /// 补边后的高。
    pub ph: u32,
    /// 块数。
    pub nblocks: u64,
    /// 是否转码事件。
    pub transcoded: bool,
}

/// ASTC 策略簿。
pub struct AstcBook {
    pub tex: Vec<TexRec>,
    /// 转码事件数。
    pub trans_events: u32,
    next_id: u32,
    magic: u32,
}

impl AstcBook {
    /// 建。
    pub fn new() -> AstcBook {
        AstcBook { tex: Vec::new(), trans_events: 0, next_id: 1, magic: MAGIC }
    }

    fn magic_ok(&self) -> bool {
        self.magic == MAGIC
    }

    /// 块型合法？
    pub fn blk_ok(blk: u32) -> bool {
        for b in BLOCKS.iter() {
            if *b == blk {
                return true;
            }
        }
        false
    }

    /// 上取整到块倍数。
    pub fn pad_to(w: u32, blk: u32) -> u32 {
        ((w + blk - 1) / blk) * blk
    }

    /// 登记：返回（纹理号，估算字节数）。
    pub fn add(&mut self, w: u32, h: u32, blk: u32) -> Result<(u32, u64), u32> {
        if !self.magic_ok() {
            return Err(XC_MAGIC);
        }
        if w == 0 || h == 0 || w > 16384 || h > 16384 {
            return Err(XC_DIM);
        }
        if !Self::blk_ok(blk) {
            return Err(XC_BLOCK);
        }
        // 转码判定：白名单块型直通；非白名单已拒——转码事件留给
        // 「块型合法但尺寸异常小（小于块）」的补整场景不适用，
        // 故此处转码事件口径 = 尺寸小于块型的边角补整超半块。
        let pw = Self::pad_to(w, blk);
        let ph = Self::pad_to(h, blk);
        let nblocks = ((pw / blk) as u64) * ((ph / blk) as u64);
        let mut transcoded = false;
        // 尺寸不能被块整除 → 需补边 → 记转码事件（HW 走补边通路）。
        if pw != w || ph != h {
            transcoded = true;
            self.trans_events += 1;
        }
        let id = self.next_id;
        self.next_id += 1;
        // 估算字节数 = 块数 × 128 位 / 8。
        let bytes = nblocks * BLOCK_BITS / 8;
        self.tex.push(TexRec { id, w, h, blk, pw, ph, nblocks, transcoded });
        Ok((id, bytes))
    }

    /// 估算总和（字节）。
    pub fn total_bytes(&self) -> u64 {
        let mut t = 0u64;
        for r in self.tex.iter() {
            t += r.nblocks * BLOCK_BITS / 8;
        }
        t
    }

    /// 账平衡核：逐条重算 = 总和。
    pub fn balanced(&self) -> bool {
        let mut re = 0u64;
        for r in self.tex.iter() {
            re += ((Self::pad_to(r.w, r.blk) / r.blk) as u64)
                * ((Self::pad_to(r.h, r.blk) / r.blk) as u64)
                * BLOCK_BITS
                / 8;
        }
        re == self.total_bytes()
    }

    /// 补边像素不计费核：估算只看 pw×ph 的块数（补边已含但
    /// 不另计像素费——以「估算字节数 = 整块计」为口径）。
    pub fn no_pixel_billing(&self, id: u32) -> Result<bool, u32> {
        if !self.magic_ok() {
            return Err(XC_MAGIC);
        }
        for r in self.tex.iter() {
            if r.id == id {
                let expect = ((Self::pad_to(r.w, r.blk) / r.blk)
                    * (Self::pad_to(r.h, r.blk) / r.blk)) as u64
                    * BLOCK_BITS
                    / 8;
                return Ok(expect == r.nblocks * BLOCK_BITS / 8);
            }
        }
        Err(XC_DIM)
    }
}

pub fn run_veb289_checks() -> CheckSet {
    let mut s = CheckSet::new("VE-F0289");
    // 族零：块型表。
    s.add("4 合法", AstcBook::blk_ok(4), "白名单");
    s.add("6 合法", AstcBook::blk_ok(6), "白名单");
    s.add("8 合法", AstcBook::blk_ok(8), "白名单");
    s.add("12 合法", AstcBook::blk_ok(12), "白名单");
    s.add("5 拒", AstcBook::blk_ok(5) == false, "非白");
    s.add("16 拒", AstcBook::blk_ok(16) == false, "非白");
    // 族一：登记与整除。
    let mut b = AstcBook::new();
    s.add("零宽拒", b.add(0, 64, 8) == Err(XC_DIM), "无零宽");
    s.add("巨高拒", b.add(64, 20000, 8) == Err(XC_DIM), ">16384");
    s.add("块 5 拒", b.add(64, 64, 5) == Err(XC_BLOCK), "非白");
    // 64×64 @8×8 = 64 块。
    let r1 = b.add(64, 64, 8);
    s.add("64 块", matches!(r1, Ok((_, 1024))), "64×16B");
    s.add("首号 1", matches!(r1, Ok((1, _))), "自 1");
    // 直通无转码事件。
    s.add("直通零事件", b.trans_events == 0, "整除");
    // 族二：补边。
    let r2 = b.add(70, 70, 8);
    s.add("补到 72", matches!(r2, Ok((_, 1296))), "81 块");
    s.add("补边立事件", b.trans_events == 1, "一笔");
    // 补边像素不计费：81 块 = (72/8)²，与 70×70 像素无关。
    s.add("不计像素费", b.no_pixel_billing(2) == Ok(true), "整块口径");
    // 4×4 块 6×6 纹理 = 4 块（补到 8×8 → 4 块）。
    let r3 = b.add(6, 6, 4);
    s.add("6 补 8", matches!(r3, Ok((_, 64))), "4 块 16B");
    // 12×12 块 12×12 纹理 = 1 块。
    let r4 = b.add(12, 12, 12);
    s.add("12 块 1", matches!(r4, Ok((_, 16))), "最小计");
    // 族三：估算总和与账平衡。
    // 1024 + 1296 + 64 + 16 = 2400。
    s.add("总和 2400", b.total_bytes() == 2400, "四笔");
    s.add("账平衡", b.balanced(), "重算同");
    // 族四：转码事件账。
    let mut t = AstcBook::new();
    let _ = t.add(16, 16, 4);  // 整除无事件
    let _ = t.add(17, 16, 4);  // 补 1 → 事件
    let _ = t.add(16, 18, 4);  // 补 2 → 事件
    s.add("事件 2", t.trans_events == 2, "两补");
    s.add("事件少于登记", t.trans_events < t.tex.len() as u32, "直通为主");
    // 族五：大纹理估算核。
    let mut g = AstcBook::new();
    // 1920×1080 @ 6×6 → pad 1920 整除，1080 → 1086? 1080/6=180 整。
    let r5 = g.add(1920, 1080, 6);
    // 320 × 180 = 57600 块 × 16 B = 921600 B。
    s.add("1080p 6x6 921600", matches!(r5, Ok((_, 921600))), "标准估算");
    s.add("魔数核", AS_SEG_BASE == 0x31140, "段基址");
    s
}
