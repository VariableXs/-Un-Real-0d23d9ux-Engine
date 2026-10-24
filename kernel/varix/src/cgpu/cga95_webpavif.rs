//! cga95_webpavif —— CGPU-F0095 WebP/AVIF 软解算子预留。
//! 锚点（CGPU 施工书）：CGPU-F0095 · WebP/AVIF 软解算子预留 · 目标 320 行
//! 判据摘录：预留接口闭集（VP8 预测模式 4×4 十四模 / VP9 方向
//! 模 / AVIF 帧内 65 角模——三档预留表冻结，只登记不实现）；
//! 能力位账（每档 0/1 就绪位——未就绪调用显性拒绝不静默）；
//! 预测模式域闸（越界模拒核）；角度表完整性（65 角模角度闭
//! 集：0..=180 度按 2 度步进——表长 66 含 DC）；DC 模特例（模
//! 0 == DC——表首锚）；预留升级纪律（就绪位只能 0→1 单向——
//! 倒退拒）；版本账（三档各持版本号，升位必须升版）。
//! 组件：PredMode 登记表（三档×就绪位×版本）+ 角度闭集表 +
//!   能力查询 + 调用前闸 + 升级单向闸 + 完整性核。
//! 职责定位：WebP/AVIF 软解的「占位骨架」——把预测模式表、能
//! 力位、版本账先立起来，实现按档逐个点亮（本批只立账不实现
//! 解码路径）；与 cga93（JPEG 已实现）形成「一档已就绪 + 两
//! 档预留」的诚实能力矩阵。
//!
//! 表设计：
//!   档 0 = JPEG（已就绪，引用 cga93）
//!   档 1 = WebP/VP8（4×4 十四模 + 16×16 四模——表登记）
//!   档 2 = AVIF/VP9 角模（65 模——表登记）

use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

// ===== 诊断码闭集（0xE940 段 · 本模块独占）=====
pub const XC_MODE_OOB: u32 = 0xE940; // 预测模式越界
pub const XC_NOT_READY: u32 = 0xE941; // 档位未就绪
pub const XC_READY_BACK: u32 = 0xE942; // 就绪位倒退
pub const XC_VER_BACK: u32 = 0xE943; // 版本倒退
pub const XC_TIER_OOB: u32 = 0xE944; // 档位越界
pub const XC_ANGLE_OOB: u32 = 0xE945; // 角度越界

// ===== 档位闭集 =====
pub const T_JPEG: u8 = 0;
pub const T_WEBP: u8 = 1;
pub const T_AVIF: u8 = 2;
pub const TIERS: [u8; 3] = [T_JPEG, T_WEBP, T_AVIF];

/// VP8 4×4 预测模数（十四：DC/TM/V/H + 10 方向）。
pub const VP8_BMODE_N: u16 = 14;
/// VP8 16×16 预测模数（四：DC/V/H/TM）。
pub const VP8_MMODE_N: u16 = 4;
/// AVIF 帧内角模数（65：DC + 平滑 + 63 方向）。
pub const AVIF_ANGLE_N: u16 = 65;
/// 角度闭集（度数 0..=180 步进 2 → 91 个角度；63 方向模映射）。
pub const ANGLE_MIN: u16 = 0;
pub const ANGLE_MAX: u16 = 180;
pub const ANGLE_STEP: u16 = 2;

/// 角度表合法（(MAX−MIN)/STEP + 1 == 91）。
pub fn angle_table_valid() -> bool {
    ((ANGLE_MAX - ANGLE_MIN) / ANGLE_STEP) + 1 == 91
}

/// 角度域闸。
pub fn angle_gate(deg: u16) -> Result<usize, u32> {
    if deg < ANGLE_MIN || deg > ANGLE_MAX || (deg - ANGLE_MIN) % ANGLE_STEP != 0 {
        return Err(XC_ANGLE_OOB);
    }
    Ok(((deg - ANGLE_MIN) / ANGLE_STEP) as usize)
}

/// 档位描述。
#[derive(Clone)]
pub struct Tier {
    pub id: u8,
    pub name: &'static str,
    pub mode_n: u16,
    pub ready: bool,
    pub ver: u32,
}

/// 能力矩阵（三档线性表）。
pub struct CapMatrix {
    tiers: Vec<Tier>,
}

impl CapMatrix {
    /// 标准初装：JPEG 就绪 v1，WebP/AVIF 预留 v1 未就绪。
    pub fn standard() -> CapMatrix {
        CapMatrix {
            tiers: alloc::vec![
                Tier { id: T_JPEG, name: "jpeg-idct", mode_n: 0, ready: true, ver: 1 },
                Tier { id: T_WEBP, name: "webp-vp8", mode_n: VP8_BMODE_N, ready: false, ver: 1 },
                Tier { id: T_AVIF, name: "avif-av1", mode_n: AVIF_ANGLE_N, ready: false, ver: 1 },
            ],
        }
    }

    pub fn find(&self, id: u8) -> Result<&Tier, u32> {
        self.tiers.iter().find(|t| t.id == id).ok_or(XC_TIER_OOB)
    }

    /// 预测模式域闸（就绪档校验模式 < mode_n；预留档一律拒）。
    pub fn mode_gate(&self, tier_id: u8, mode: u16) -> Result<(), u32> {
        let t = self.find(tier_id)?;
        if !t.ready {
            return Err(XC_NOT_READY);
        }
        if t.mode_n > 0 && mode >= t.mode_n {
            return Err(XC_MODE_OOB);
        }
        Ok(())
    }

    /// 升级就绪位（单向闸 + 版本闸）。
    pub fn promote(&mut self, tier_id: u8, new_ver: u32) -> Result<(), u32> {
        let idx = self.tiers.iter().position(|t| t.id == tier_id).ok_or(XC_TIER_OOB)?;
        let t = &mut self.tiers[idx];
        if t.ready {
            return Err(XC_READY_BACK); // 已就绪重复升级拒
        }
        if new_ver <= t.ver {
            return Err(XC_VER_BACK);
        }
        t.ready = true;
        t.ver = new_ver;
        Ok(())
    }

    /// 版本推进（不改动就绪位——预留期表修正通道）。
    pub fn bump_ver(&mut self, tier_id: u8, new_ver: u32) -> Result<(), u32> {
        let t = self.tiers.iter_mut().find(|t| t.id == tier_id).ok_or(XC_TIER_OOB)?;
        if new_ver <= t.ver {
            return Err(XC_VER_BACK);
        }
        t.ver = new_ver;
        Ok(())
    }

    /// 完整性：三档全在 ∧ 就绪档 ≤1（本批纪律：只 JPEG 点亮）。
    pub fn integrity(&self) -> Result<(), u32> {
        if self.tiers.len() != 3 {
            return Err(XC_TIER_OOB);
        }
        let ready_n = self.tiers.iter().filter(|t| t.ready).count();
        if ready_n > 1 {
            return Err(XC_READY_BACK);
        }
        Ok(())
    }

    /// 能力指纹。
    pub fn fingerprint(&self) -> u64 {
        let mut h: u64 = 0xcbf29ce484222325;
        for t in self.tiers.iter() {
            for v in [t.id as u64, t.mode_n as u64, t.ready as u64, t.ver as u64].iter() {
                h ^= *v;
                h = h.wrapping_mul(0x100000001b3);
            }
            for b in t.name.bytes() {
                h ^= b as u64;
                h = h.wrapping_mul(0x100000001b3);
            }
        }
        h
    }
}

/// 预留模式登记（WebP 十四模名单——只报名字不做实现）。
pub fn vp8_bmode_names() -> [&'static str; 14] {
    [
        "DC", "TM", "V", "H", "LD", "RD", "VR", "VL", "HD", "HU", // 10 方向
        "D45", "D135", "D117", "D153", // 4 补方向（VP9 语义预留）
    ]
}

// ===== 主判据 =====
pub fn run_cga95_checks() -> CheckSet {
    let mut cs = CheckSet::new("CGPU-F0095");
    // 族一：角度表。
    cs.add("角度表 91", angle_table_valid(), "0..180/2");
    cs.add("角度首锚", angle_gate(0) == Ok(0), "0 度槽 0");
    cs.add("角度末锚", angle_gate(180) == Ok(90), "180 度槽 90");
    cs.add("角度步中拒", angle_gate(1).is_err(), "非步进");
    cs.add("角度越界拒", angle_gate(182).is_err() && angle_gate(200).is_err(), "ANGLE_OOB");
    // 族二：能力矩阵标准态。
    let m = CapMatrix::standard();
    cs.add("三档", m.integrity().is_ok(), "矩阵完整");
    cs.add("JPEG 就绪", matches!(m.find(T_JPEG), Ok(t) if t.ready), "档 0 点亮");
    cs.add("WebP 预留", matches!(m.find(T_WEBP), Ok(t) if !t.ready), "档 1 未就绪");
    cs.add("AVIF 预留", matches!(m.find(T_AVIF), Ok(t) if !t.ready), "档 2 未就绪");
    // 族三：模式域闸。
    cs.add("预留档调用拒", m.mode_gate(T_WEBP, 0).is_err(), "NOT_READY");
    cs.add("预留档调用拒 2", m.mode_gate(T_AVIF, 64).is_err(), "NOT_READY");
    cs.add("JPEG 零模放行", m.mode_gate(T_JPEG, 0).is_ok(), "无模表直通");
    cs.add("档越界拒", m.mode_gate(9, 0).is_err(), "TIER_OOB");
    // 族四：升级单向闸。
    let mut m2 = CapMatrix::standard();
    cs.add("升版过拒", m2.promote(T_WEBP, 1).is_err(), "VER_BACK");
    cs.add("升就绪", m2.promote(T_WEBP, 2).is_ok(), "0→1 点亮");
    cs.add("重复升级拒", m2.promote(T_WEBP, 3).is_err(), "READY_BACK");
    cs.add("点亮生效", matches!(m2.find(T_WEBP), Ok(t) if t.ready && t.ver == 2), "v2 就绪");
    // 点亮后纪律：ready_n=2 破完整性（本批纪律单就绪）。
    cs.add("双就绪破纪", m2.integrity().is_err(), "单就绪纪律");
    // 族五：版本推进。
    let mut m3 = CapMatrix::standard();
    cs.add("表修正升版", m3.bump_ver(T_AVIF, 5).is_ok(), "预留期修表");
    cs.add("降版拒", m3.bump_ver(T_AVIF, 4).is_err(), "VER_BACK");
    // 族六：VP8 模名单。
    let names = vp8_bmode_names();
    cs.add("模数 14", names.len() == VP8_BMODE_N as usize, "名单全量");
    cs.add("DC 首锚", names[0] == "DC", "模 0==DC");
    cs.add("TM 次锚", names[1] == "TM", "模 1==TM");
    // 族七：指纹。
    let f1 = CapMatrix::standard().fingerprint();
    cs.add("指纹可复算", CapMatrix::standard().fingerprint() == f1, "同构同指");
    cs.add("指纹敏感", m2.fingerprint() != f1, "升级变指");
    // 族八：常量锚。
    cs.add("VP8 16 模 4", VP8_MMODE_N == 4, "DC/V/H/TM");
    cs.add("AVIF 65 模", AVIF_ANGLE_N == 65, "DC+平滑+63 角");
    CheckSet::merge(CheckSet::merge(cs, run_cga95_deep()), run_cga95_deep2())
}

// ===== 深化面：档位矩阵与版本推进二次账 =====
/// 标准矩阵完整性：逐档 find 命中 + integrity 全过。
pub fn matrix_full() -> bool {
    let m = CapMatrix::standard();
    let mut all_found = true;
    for id in 0..6u8 {
        if m.find(id).is_err() {
            all_found = false;
        }
    }
    all_found && m.integrity().is_ok()
}

pub fn run_cga95_deep() -> CheckSet {
    let mut cs = CheckSet::new("cga95_deep");
    cs.add("角度表合法", angle_table_valid(), "表内自洽");
    cs.add("角度命中", angle_gate(90).is_ok(), "90° 在册");
    cs.add("角度越界拒", angle_gate(3600).is_err(), "越表");
    cs.add("矩阵完整", matrix_full(), "六档齐");
    let mut m = CapMatrix::standard();
    cs.add("档缺拒", m.find(99).is_err(), "不在册");
    cs.add("模式越界拒", m.mode_gate(0, 9999).is_err(), "模式闭集");
    cs.add("版本推进", m.promote(0, 2).is_ok(), "ver→2");
    cs.add("版本倒退拒", m.bump_ver(0, 1).is_err(), "不许回退");
    cs.add("推进后完整", m.integrity().is_ok(), "账仍闭");
    cs.add("指纹稳定", m.fingerprint() == m.fingerprint(), "双算同值");
    cs
}

// ===== 深化面二：逐档推进与域门 =====
pub fn run_cga95_deep2() -> CheckSet {
    let mut cs = CheckSet::new("cga95_deep2");
    let mut m = CapMatrix::standard();
    // 六档逐档 find。
    cs.add("逐档可查", {
        let mut all = true;
        for id in 0..6u8 {
            if m.find(id).is_err() {
                all = false;
            }
        }
        all
    }, "0..6 全命中");
    // 角度门扫：0/45/90/180/270/359 全过。
    cs.add("角度域扫", {
        [0u16, 45, 90, 180, 270, 359].iter().all(|&d| angle_gate(d).is_ok())
    }, "六角全收");
    cs.add("角度负域拒", angle_gate(400).is_err() || angle_gate(65535).is_err(), "越表至少一向");
    // 版本推进链：连续推进两次。
    cs.add("推进链", m.promote(1, 2).is_ok() && m.promote(1, 3).is_ok(), "2→3");
    cs.add("推进后完整", m.integrity().is_ok(), "账仍闭");
    // bump 不可回退。
    cs.add("回退拒", m.bump_ver(1, 2).is_err(), "版本单调");
    // mode_gate 未知档拒。
    cs.add("档缺拒", m.mode_gate(99, 0).is_err(), "TIER_OOB");
    cs.add("指纹三跑", m.fingerprint() == m.fingerprint() && m.fingerprint() == m.fingerprint(), "稳定");
    cs.add("标准矩阵独立", CapMatrix::standard().integrity().is_ok(), "新构造不受污染");
    cs
}
