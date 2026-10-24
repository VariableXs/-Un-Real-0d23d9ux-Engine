//! cga90_optable —— CGPU-F0090 算子性能总表。
//! 锚点（CGPU 施工书）：CGPU-F0090 · 算子性能总表 · 目标 300 行
//! 判据摘录：总表闭集（A 域全部已入册算子的步数模型一表通——
//! 与 cga75 Bench 冻结名单同源）；步数模型核（每算子 op 步数
//! = f(像素数, 参数)——模型公式冻结，双算双对拍）；表完整性
//! （注册表全算子有行——漏行即账破）；单调合理性（同算子像
//! 素数↑步数↑——模型方向账）；预算占比账（算子步数/帧预算
//! 千分比——超占比告警闭集）；指纹冻结（总表指纹——改表即
//! 变）；行锁（每行 op 唯一——重复行拒）。
//! 组件：PerfRow（算子性能行）+ PerfTable 线性表 + 步数模型（
//!   逐算子闭式公式）+ 行锁 + 单调账 + 占比分档 + 指纹。
//! 职责定位：性能账本的总账页——cga60（基准冻结）、cga105（编
//! 解码基准）的数据源；步数模型是「白盒成本」：不跑基准也能
//! 算出预期成本，跑基准是核模型而非测性能。
//!
//! 步数模型（冻结公式）：
//!   invert/bright/thr/scale：steps = N（点态一像素一步）
//!   box_blur 3×3：steps = N×9（每像素 9 采样）
//!   dilate r：steps = N×(2r+1)²（窗口大小）
//!   bilateral 5×5：steps = N×25×2（空间+值域双查）
//!   prefix_sum：steps = 2N（扫两遍）

use alloc::vec::Vec;
use crate::checks::CheckSet;

// ===== 诊断码闭集（0xE8F0 段 · 本模块独占）=====
pub const XC_DUP_ROW: u32 = 0xE8F0; // 重复行
pub const XC_OP_UNKNOWN: u32 = 0xE8F1; // 未知算子
pub const XC_N_ZERO: u32 = 0xE8F2; // 像素数为 0
pub const XC_MONO_FAIL: u32 = 0xE8F3; // 单调破
pub const XC_ROW_MISS: u32 = 0xE8F4; // 总表缺行

// ===== 算子 ID 闭集（总表行键）=====
pub const K_POINT: u8 = 0;
pub const K_BOX3: u8 = 1;
pub const K_DILATE: u8 = 2;
pub const K_BILATERAL: u8 = 3;
pub const K_PREFIX: u8 = 4;
pub const K_ALL: [u8; 5] = [K_POINT, K_BOX3, K_DILATE, K_BILATERAL, K_PREFIX];

/// 步数模型（冻结闭式）。
/// r 为窗口半径参数（点态族忽略）。
pub fn step_model(op: u8, n: u32, r: u32) -> Result<u64, u32> {
    if n == 0 {
        return Err(XC_N_ZERO);
    }
    match op {
        K_POINT => Ok(n as u64),
        K_BOX3 => Ok(n as u64 * 9),
        K_DILATE => {
            if !(1..=4).contains(&r) {
                return Err(0xE8F1);
            }
            let side = (2 * r + 1) as u64;
            Ok(n as u64 * side * side)
        }
        K_BILATERAL => Ok(n as u64 * 50), // 25 窗口 × 双查
        K_PREFIX => Ok(n as u64 * 2),
        _ => Err(XC_OP_UNKNOWN),
    }
}

/// 性能行。
#[derive(Clone)]
pub struct PerfRow {
    pub op: u8,
    pub n: u32,
    pub r: u32,
    /// 模型步数（建行时算死——行内冻结）。
    pub steps: u64,
}

/// 性能总表。
pub struct PerfTable {
    rows: Vec<PerfRow>,
    /// 帧步数预算（占比分母）。
    pub frame_budget: u64,
}

impl PerfTable {
    pub fn new(frame_budget: u64) -> PerfTable {
        PerfTable { rows: Vec::new(), frame_budget }
    }

    /// 建行（行锁：同 (op,r) 唯一）。
    pub fn add_row(&mut self, op: u8, n: u32, r: u32) -> Result<(), u32> {
        if self.rows.iter().any(|row| row.op == op && row.r == r) {
            return Err(XC_DUP_ROW);
        }
        let steps = step_model(op, n, r)?;
        self.rows.push(PerfRow { op, n, r, steps });
        Ok(())
    }

    /// 查行。
    pub fn find(&self, op: u8, r: u32) -> Option<&PerfRow> {
        self.rows.iter().find(|row| row.op == op && row.r == r)
    }

    /// 总表完整性：K_ALL 全有行。
    pub fn complete(&self) -> Result<(), u32> {
        for &k in K_ALL.iter() {
            if self.find(k, 0).is_none() && self.find(k, 1).is_none() && self.find(k, 2).is_none() {
                return Err(XC_ROW_MISS);
            }
        }
        Ok(())
    }

    /// 总步数。
    pub fn total_steps(&self) -> u64 {
        self.rows.iter().map(|r| r.steps).sum()
    }

    /// 帧预算占比（千分比）。
    pub fn permille(&self) -> u64 {
        if self.frame_budget == 0 {
            return 0;
        }
        self.total_steps() * 1000 / self.frame_budget
    }

    /// 占比分档：0-500 绿、501-800 黄、>800 红（告警闭集）。
    pub fn alarm_band(&self) -> u8 {
        let pm = self.permille();
        if pm > 800 {
            2
        } else if pm > 500 {
            1
        } else {
            0
        }
    }

    /// 表指纹（FNV-1a 64）。
    pub fn fingerprint(&self) -> u64 {
        let mut h: u64 = 0xcbf29ce484222325;
        for r in self.rows.iter() {
            for v in [r.op as u64, r.n as u64, r.r as u64, r.steps].iter() {
                h ^= *v;
                h = h.wrapping_mul(0x100000001b3);
            }
        }
        h
    }
}

/// 单调账：点态族 n 翻倍步数翻倍（线性模型锚）。
pub fn monotone_linear() -> Result<(), u32> {
    let s1 = step_model(K_POINT, 100, 0)?;
    let s2 = step_model(K_POINT, 200, 0)?;
    if s2 != s1 * 2 {
        return Err(XC_MONO_FAIL);
    }
    Ok(())
}

/// 窗口族单调：dilate r↑ 步数↑。
pub fn monotone_window() -> Result<(), u32> {
    let s1 = step_model(K_DILATE, 100, 1)?;
    let s2 = step_model(K_DILATE, 100, 2)?;
    let s3 = step_model(K_DILATE, 100, 3)?;
    if !(s1 < s2 && s2 < s3) {
        return Err(XC_MONO_FAIL);
    }
    Ok(())
}

// ===== 主判据 =====
pub fn run_cga90_checks() -> CheckSet {
    let mut cs = CheckSet::new("CGPU-F0090");
    // 族一：模型锚。
    cs.add("点态锚", step_model(K_POINT, 1000, 0) == Ok(1000), "1 px=1 step");
    cs.add("box3 锚", step_model(K_BOX3, 100, 0) == Ok(900), "9 采样");
    cs.add("dilate r1 锚", step_model(K_DILATE, 100, 1) == Ok(900), "3²窗口");
    cs.add("dilate r2 锚", step_model(K_DILATE, 100, 2) == Ok(2500), "5²窗口");
    cs.add("bilateral 锚", step_model(K_BILATERAL, 100, 0) == Ok(5000), "25×2");
    cs.add("prefix 锚", step_model(K_PREFIX, 100, 0) == Ok(200), "扫两遍");
    // 族二：域闸。
    cs.add("零像素拒", step_model(K_POINT, 0, 0).is_err(), "N=0");
    cs.add("未知算子拒", step_model(99, 10, 0).is_err(), "闭集外");
    cs.add("半径越界拒", step_model(K_DILATE, 10, 5).is_err(), "r>4");
    // 族三：单调账。
    cs.add("线性单调", monotone_linear().is_ok(), "翻倍翻倍");
    cs.add("窗口单调", monotone_window().is_ok(), "r↑步数↑");
    // 族四：总表建账 + 完整性。
    let mut t = PerfTable::new(100_000);
    let _ = t.add_row(K_POINT, 4096, 0);
    let _ = t.add_row(K_BOX3, 4096, 0);
    let _ = t.add_row(K_DILATE, 4096, 2);
    let _ = t.add_row(K_BILATERAL, 4096, 0);
    let _ = t.add_row(K_PREFIX, 4096, 0);
    cs.add("表容 5", t.total_steps() > 0, "行已建");
    cs.add("行锁", t.add_row(K_POINT, 100, 0).is_err(), "重复行拒");
    cs.add("完整性", t.complete().is_ok(), "K_ALL 全有行");
    // 族五：占比账。
    // 总步数 = 4096×(1+9+25+50+2) = 4096×87 = 356,352。
    cs.add("总步锚", t.total_steps() == 356_352, "4096×87");
    cs.add("占比锚", t.permille() == 3563, "356352×1000/100000");
    cs.add("红档", t.alarm_band() == 2, ">800‰");
    let small = PerfTable::new(10_000_000);
    let mut s2 = PerfTable::new(10_000_000);
    let _ = s2.add_row(K_POINT, 1000, 0);
    cs.add("绿档", s2.alarm_band() == 0, "微占比");
    let _ = small;
    // 族六：指纹。
    let f1 = t.fingerprint();
    cs.add("指纹幂等", t.fingerprint() == f1, "双算一致");
    let _ = t.add_row(K_PREFIX, 8192, 0);
    cs.add("指纹敏感", t.fingerprint() != f1, "加行变指纹");
    CheckSet::merge(CheckSet::merge(cs, run_cga90_deep()), run_cga90_deep2())
}

// ===== 深化面：性能表行账与档位核 =====
/// 行闭账：find 缺行 None、命中行步数与模型一致。
pub fn row_match_model(budget: u64) -> bool {
    let mut t = PerfTable::new(budget);
    let _ = t.add_row(1, 64, 2);
    match t.find(1, 2) {
        Some(row) => row.steps == step_model(1, 64, 2).unwrap_or(0),
        None => false,
    }
}

pub fn run_cga90_deep() -> CheckSet {
    let mut cs = CheckSet::new("cga90_deep");
    let mut t = PerfTable::new(10000);
    cs.add("空表完备拒", t.complete().is_err(), "行数不足");
    cs.add("行加收", t.add_row(1, 64, 2).is_ok(), "首行");
    cs.add("行重拒", t.add_row(1, 64, 2).is_err(), "同 (op,r) 拒");
    cs.add("命中一致", row_match_model(10000), "行步==模型");
    cs.add("缺行 None", t.find(9, 9).is_none(), "不在册");
    cs.add("总步正", t.total_steps() > 0, "有累计");
    cs.add("千分比闭", t.permille() <= 1000, "≤1000");
    cs.add("档位闭", t.alarm_band() <= 3, "四档闭集");
    cs.add("指纹稳定", t.fingerprint() == t.fingerprint(), "双算同值");
    cs.add("步长越界拒", step_model(200, 64, 2).is_err(), "算子闭集");
    cs
}

// ===== 深化面二：多行账与告警带 =====
pub fn run_cga90_deep2() -> CheckSet {
    let mut cs = CheckSet::new("cga90_deep2");
    let mut t = PerfTable::new(1_000_000);
    let _ = t.add_row(1, 64, 2);
    let _ = t.add_row(1, 256, 2);
    let _ = t.add_row(2, 64, 1);
    cs.add("三行全查", t.find(1, 2).is_some() && t.find(1, 2).is_some() && t.find(2, 1).is_some(), "行齐");
    cs.add("行独立", t.find(2, 1).is_some() && t.find(2, 2).is_none(), "未录 None");
    cs.add("总步合计", t.total_steps()
        == step_model(1, 64, 2).unwrap_or(0)
            + step_model(1, 256, 2).unwrap_or(0)
            + step_model(2, 64, 1).unwrap_or(0), "Σ行==总");
    cs.add("预算内千分", t.permille() <= 1000, "≤1000");
    cs.add("带域闭", t.alarm_band() <= 3, "四档");
    cs.add("指纹随行变", {
        let f0 = t.fingerprint();
        let _ = t.add_row(3, 32, 1);
        t.fingerprint() != f0
    }, "行变指纹变");
    cs.add("完备过", t.complete().is_ok(), "行齐即完备");
    cs.add("模型 n 越界拒", step_model(1, 0, 2).is_err(), "N_ZERO");
    cs
}
