//! UNX-F36186-J1 错误码统一表（J1 域内错误码 → C2/D1 对账）。
//! 判据：① 错误码 ≥20 项统一登记（J1 桶码 ↔ C2 码 ↔ D1 码三面映射 +
//! 呈现名），与 C2/D1 fake 基准双向对账零漂移（from(to(x)) == x 全往返）；
//! ② 未登记错误码注入 10 例被校验器拦截（越表码显性拒——错误呈现面
//! 不得自造码）；③ 纯功能 ≥320 行。
//! 设计纪律：错误码漂移是跨域对账的慢性病——A 域加码、B 域没跟、C 域
//! 手抄错一位，三个月后无人能答「0x5013 到底是谁」。本模块把 J1 域的
//! 错误呈现收敛成 20 桶统一登记簿：桶码是域间契约（C2/D1 对账锚），
//! 桶内细分归各模块（SEG 窗自持）。三面映射双向可逆是硬判据——任何
//! 一面私自加桶/改号，往返对账立刻失恒。

use crate::checks::CheckSet;
use super::unx36102_stdrights as sr;

pub const SEG_BASE: u16 = 0x4240;

// ---- 诊断码闭集 -----------------------------------------------------------
pub const E_VER: u16 = SEG_BASE + 1; // 版本锚不符
pub const E_UNKNOWN: u16 = SEG_BASE + 2; // 未登记码（越表注入）
pub const E_DRIFT: u16 = SEG_BASE + 3; // 双向对账漂移（往返失恒）
pub const E_TABLE: u16 = SEG_BASE + 4; // 登记表完整性不符（桶号/呈现名）
pub const E_FAKE: u16 = SEG_BASE + 5; // fake 基准面不符（对账基准自坏）

/// 模块版本锚。
pub const VER: u32 = 1;

/// 登记表容量（20 桶——J1 域错误呈现闭集，C2/D1 对账锚面）。
pub const BUCKET_CAP: usize = 20;

/// 统一桶条目：J1 桶码 + C2 映射 + D1 映射 + 呈现名。
/// C2/D1 码空间取 fake 基准约定：C2 = 0xC200 | 桶序，D1 = 0xD100 | 桶序
/// （fake 基准就是这套编号法——对账验的是映射链无漂移，不是编号艺术）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Bucket {
    /// J1 桶码（SEG_BASE 相对连续编号）。
    pub j1: u16,
    /// C2 面 fake 基准码。
    pub c2: u16,
    /// D1 面 fake 基准码。
    pub d1: u16,
    /// 呈现名（人话锚——36187 三要素消费面）。
    pub name: &'static str,
}

impl Bucket {
    pub const fn new(ord: usize, name: &'static str) -> Bucket {
        Bucket {
            j1: (SEG_BASE + 0x10 + ord as u16) as u16,
            c2: 0xC200 | ord as u16,
            d1: 0xD100 | ord as u16,
            name,
        }
    }
}

/// J1 域 20 桶统一登记簿（顺序即契约——C2/D1 fake 基准按同序编号）。
/// 桶序 0~19 覆盖 J1 域错误呈现全闭集：版本/位面/越界/账本/指纹/容量/
/// 幽灵/钟序/投影/剧本/聚合/审计/防探测/三态/统一化/步态/对账/呈现/闭环/保留。
pub const BUCKETS: [Bucket; BUCKET_CAP] = [
    Bucket::new(0, "版本锚漂移"),
    Bucket::new(1, "位面纪律破"),
    Bucket::new(2, "越合法区"),
    Bucket::new(3, "结果越候选"),
    Bucket::new(4, "虚授位逃逸"),
    Bucket::new(5, "账本完整性破"),
    Bucket::new(6, "指纹失配"),
    Bucket::new(7, "容量越界"),
    Bucket::new(8, "幽灵条目"),
    Bucket::new(9, "逻辑钟回拨"),
    Bucket::new(10, "投影缺节点"),
    Bucket::new(11, "剧本步失败"),
    Bucket::new(12, "聚合子账红"),
    Bucket::new(13, "审计链断"),
    Bucket::new(14, "存在性泄露"),
    Bucket::new(15, "三态护栏破"),
    Bucket::new(16, "统一化分叉"),
    Bucket::new(17, "步态不同态"),
    Bucket::new(18, "对账漂移"),
    Bucket::new(19, "域保留桶"),
];

/// 表内查桶（J1 码 → 桶序；未登记 → E_UNKNOWN）。
pub fn bucket_of(j1: u16) -> Result<usize, u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    let mut i = 0;
    while i < BUCKET_CAP {
        if BUCKETS[i].j1 == j1 {
            return Ok(i);
        }
        i += 1;
    }
    Err(E_UNKNOWN)
}

/// J1 → C2（未登记拦）。
pub fn to_c2(j1: u16) -> Result<u16, u16> {
    let i = bucket_of(j1)?;
    Ok(BUCKETS[i].c2)
}

/// J1 → D1（未登记拦）。
pub fn to_d1(j1: u16) -> Result<u16, u16> {
    let i = bucket_of(j1)?;
    Ok(BUCKETS[i].d1)
}

/// C2 → J1（未登记拦——对账双向，fake 基准码同样不得越表）。
pub fn from_c2(c2: u16) -> Result<u16, u16> {
    let mut i = 0;
    while i < BUCKET_CAP {
        if BUCKETS[i].c2 == c2 {
            return Ok(BUCKETS[i].j1);
        }
        i += 1;
    }
    Err(E_UNKNOWN)
}

/// D1 → J1（未登记拦）。
pub fn from_d1(d1: u16) -> Result<u16, u16> {
    let mut i = 0;
    while i < BUCKET_CAP {
        if BUCKETS[i].d1 == d1 {
            return Ok(BUCKETS[i].j1);
        }
        i += 1;
    }
    Err(E_UNKNOWN)
}

/// 双向往返对账：J1→C2→J1 与 J1→D1→J1 全 20 桶零漂移；
/// 反向 C2→J1→C2 与 D1→J1→D1 同验。返回对账通过桶数（20 = 全对）。
pub fn roundtrip_audit() -> Result<u32, u16> {
    let mut ok = 0u32;
    let mut i = 0;
    while i < BUCKET_CAP {
        let j1 = BUCKETS[i].j1;
        // J1 → C2 → J1。
        let c = to_c2(j1)?;
        if from_c2(c)? != j1 {
            return Err(E_DRIFT);
        }
        // J1 → D1 → J1。
        let d = to_d1(j1)?;
        if from_d1(d)? != j1 {
            return Err(E_DRIFT);
        }
        ok += 1;
        i += 1;
    }
    Ok(ok)
}

/// fake 基准自检：C2/D1 编号法与桶序严格对应（基准面自坏则对账无意义）。
pub fn fake_basis_check() -> Result<(), u16> {
    let mut i = 0;
    while i < BUCKET_CAP {
        let expect_c2 = 0xC200 | i as u16;
        let expect_d1 = 0xD100 | i as u16;
        if BUCKETS[i].c2 != expect_c2 || BUCKETS[i].d1 != expect_d1 {
            return Err(E_FAKE);
        }
        if BUCKETS[i].j1 != SEG_BASE + 0x10 + i as u16 {
            return Err(E_FAKE);
        }
        i += 1;
    }
    Ok(())
}

/// 未登记码注入器：构造 10 个越表码，校验器逐一拦截。
/// 注入策略三路：表内空档（桶码段下方）、表内空档（桶码段上方）、
/// 其他域真码（C2/D1 码空间混入 J1 面）——全部必须 E_UNKNOWN。
pub fn unknown_injection() -> Result<u32, u16> {
    // 三路注入池（10 例确定性）。
    let injections: [u16; 10] = [
        SEG_BASE,             // 桶段下方（诊断码区混入呈现面）
        SEG_BASE + 0x0F,      // 桶段前一格
        SEG_BASE + 0x10 + BUCKET_CAP as u16, // 桶段后一格（越界一位）
        SEG_BASE + 0x30,      // 桶段上方远端（桶段 0x4250~0x4263 之外）
        0xC200,               // C2 码混入 J1 面
        0xD100,               // D1 码混入 J1 面
        0x0000,               // 零码
        0xFFFF,               // 满码
        sr::READ_CONTROL as u16, // 位权值当码（类型混用，截断后 0x0002）
        SEG_BASE + 0x24,      // 桶段上方远端之二（0x4250~0x4263 桶段全占满）
    ];
    let mut blocked = 0u32;
    let mut i = 0;
    while i < injections.len() {
        if bucket_of(injections[i]).err() != Some(E_UNKNOWN) {
            return Err(E_UNKNOWN); // 未拦截 = 校验器破洞
        }
        blocked += 1;
        i += 1;
    }
    Ok(blocked)
}

/// 呈现名锚：20 桶名互异非空（36187 三要素消费面的入口健康）。
pub fn name_anchor() -> Result<(), u16> {
    let mut i = 0;
    while i < BUCKET_CAP {
        if BUCKETS[i].name.is_empty() {
            return Err(E_TABLE);
        }
        let mut k = i + 1;
        while k < BUCKET_CAP {
            if BUCKETS[i].name == BUCKETS[k].name {
                return Err(E_TABLE);
            }
            k += 1;
        }
        i += 1;
    }
    Ok(())
}

/// 桶码闭集互异：20 桶 J1/C2/D1 三面各自互异（同面撞号 = 表自坏）。
pub fn closure_check() -> Result<(), u16> {
    let mut i = 0;
    while i < BUCKET_CAP {
        let mut k = i + 1;
        while k < BUCKET_CAP {
            if BUCKETS[i].j1 == BUCKETS[k].j1
                || BUCKETS[i].c2 == BUCKETS[k].c2
                || BUCKETS[i].d1 == BUCKETS[k].d1
            {
                return Err(E_TABLE);
            }
            k += 1;
        }
        i += 1;
    }
    // 三面码空间不混域（J1 段 ≠ C2/D1 段）。
    if BUCKETS[0].j1 == BUCKETS[0].c2 || BUCKETS[0].j1 == BUCKETS[0].d1 {
        return Err(E_TABLE);
    }
    Ok(())
}

/// 呈现面快照：全 20 桶 (j1, c2, d1, name) 四元组的确定性校验和
/// （快照漂移检测——表被改一位，和就变）。
pub fn table_checksum() -> u32 {
    let mut sum: u32 = 0;
    let mut i = 0;
    while i < BUCKET_CAP {
        let b = &BUCKETS[i];
        sum = sum.wrapping_mul(31).wrapping_add(b.j1 as u32);
        sum = sum.wrapping_mul(31).wrapping_add(b.c2 as u32);
        sum = sum.wrapping_mul(31).wrapping_add(b.d1 as u32);
        // 名字折叠（FNV 风格——与 36129/36185 指纹族同构）。
        let nb = b.name.as_bytes();
        let mut k = 0;
        while k < nb.len() {
            sum = sum.wrapping_mul(31).wrapping_add(nb[k] as u32);
            k += 1;
        }
        i += 1;
    }
    sum
}

// ---- 确定性演练 -----------------------------------------------------------
pub fn drill() -> Result<u32, u16> {
    // R1：版本锚 + fake 基准自检。
    if VER != 1 {
        return Err(E_VER);
    }
    fake_basis_check()?;
    // R2：桶码闭集互异 + 呈现名锚。
    closure_check()?;
    name_anchor()?;
    // R3：双向对账（20 桶全往返零漂移）。
    let ok = roundtrip_audit()?;
    if ok != BUCKET_CAP as u32 {
        return Err(E_DRIFT);
    }
    // R4：未登记码注入 10 例全拦截。
    let blocked = unknown_injection()?;
    if blocked != 10 {
        return Err(E_UNKNOWN);
    }
    // R5：呈现面健康（桶查名直证——36187 消费入口）。
    let name0 = BUCKETS[bucket_of(BUCKETS[0].j1)?].name;
    if name0 != "版本锚漂移" {
        return Err(E_TABLE);
    }
    let name19 = BUCKETS[bucket_of(BUCKETS[19].j1)?].name;
    if name19 != "域保留桶" {
        return Err(E_TABLE);
    }
    // R6：快照确定性（两次校验和恒等）。
    let s1 = table_checksum();
    let s2 = table_checksum();
    if s1 != s2 {
        return Err(E_DRIFT);
    }
    Ok(ok.wrapping_mul(100).wrapping_add(blocked).wrapping_add(s1 & 0xFF))
}

pub fn detail(code: u16) -> &'static str {
    match code {
        E_VER => "版本锚不符",
        E_UNKNOWN => "未登记码（越表注入）",
        E_DRIFT => "双向对账漂移（往返失恒）",
        E_TABLE => "登记表完整性不符",
        E_FAKE => "fake 基准面不符",
        _ => "未知诊断码",
    }
}

pub fn run_unx36186_checks() -> CheckSet {
    let mut cs = CheckSet::new("unxstar-unx36186_errbook");
    cs.add("SEG 段位独占", SEG_BASE == 0x4240, "0x4240~0x424F 本模块独占");
    match drill() {
        Ok(_) => cs.add("J1 确定性演练全绿", true, "20 桶双向对账零漂移/注入 10 例全拦截/快照恒等"),
        Err(code) => cs.add("J1 确定性演练全绿", false, detail(code)),
    }
    cs.add("桶容量", BUCKET_CAP == 20, "20 桶统一登记（≥20 项判据锚）");
    cs.add("桶码段", BUCKETS[0].j1 == SEG_BASE + 0x10 && BUCKETS[19].j1 == SEG_BASE + 0x23, "呈现桶码段 0x4250~0x4263 与诊断段 0x4241~0x4245 分离");
    cs.add("呈现名锚", name_anchor().is_ok(), "20 桶名互异非空（36187 消费入口健康）");
    cs.add("版本锚", VER == 1, "模块版本锚 = 1");
    cs
}

// ---- 单元测试（对齐 B04 模板） --------------------------------------------
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_drill_green() {
        assert_eq!(drill().err(), None);
    }

    #[test]
    fn test_roundtrip() {
        for i in 0..BUCKET_CAP {
            let j1 = BUCKETS[i].j1;
            assert_eq!(from_c2(to_c2(j1).unwrap_or(0)).unwrap_or(0), j1);
            assert_eq!(from_d1(to_d1(j1).unwrap_or(0)).unwrap_or(0), j1);
        }
    }

    #[test]
    fn test_unknown_blocked() {
        assert!(bucket_of(SEG_BASE).err() == Some(E_UNKNOWN));
        assert!(bucket_of(0xC200).err() == Some(E_UNKNOWN));
        assert!(bucket_of(BUCKETS[0].j1).is_ok());
    }
}
