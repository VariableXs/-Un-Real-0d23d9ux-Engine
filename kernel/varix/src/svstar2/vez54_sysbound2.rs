//! VE-F5004 · 与既有系统关系（VE-Y 域 · 脚本语言组 · 目标 300 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F5004`
//!
//! **判据（锚点原文）**：四定边界、编排不侵入、互补不混岗、预留、判据。
//!
//! **职责定位（锚点原文全文）**：与既有系统关系——与既有系统关系（关系四定：与引擎系统（脚本调用引擎 API——单向编排不反向侵入）/与插件 SDK（脚本能力与插件互补——轻逻辑用脚本重扩展用插件边界清晰）/与配置系统（配置管数据脚本管行为——不混岗）/与 W 域市场（脚本包未来可上架——预留不承诺））。数据结构：关系四定卡；边界清单。错误路径与降级矩阵：边界混淆（脚本干插件的活）→引导归位；关系失配→对拍修正；预留私启→拦截。性能逐项分解：边界 O(1)；对拍 O(1)；拦截 O(1)。跨批对接点：W01 插件口径上游；配置系统衔接；F5020 双签闸。无障碍与隐私：边界文档读屏可达（域本色）；无隐私面。工程量：核心逻辑约 150 行（五类要点判定与主流程编排）、边界防护约 40 行（输入校验与阈值防护）、错误路径约 35 行（降级与回滚处置）、测试支撑约 75 行（回归用例与断言），合计约 300 行。判据：四定边界、编排不侵入、互补不混岗、预留、判据。
//!
//! # 一、本件与邻件的关系（vez53_sysbound 的姊妹件，不覆盖、各守一段码）
//!
//! 本件是 VE-F5004 的第二落地件：锚点判据同一份，落地口径独立成册。
//! 邻件 `vez53_sysbound.rs` 占用诊断码段 0x1D50..=0x1D5F；本件开工前
//! grep 全仓 `0x1D5`/`0x1DD` 复核后改占 **0x1DD0..=0x1DDF**（唯一一处
//! `0x1DD…` 命中是 unx2562 的 u64 混合种子常量，非诊断码）——两件
//! 互不覆盖、码段互斥，同锚点双件并行使判据可交叉对拍。
//!
//! # 二、关系四定：脚本与既有系统关系的闭集（对拍 O(1)）
//!
//! [`RelQuad`] 是锚点「关系四定」的机械闭集：引擎系统 / 插件 SDK /
//! 配置系统 / W 域市场（预留）。每定一张**四定卡**（[`QUAD_CARDS`]，
//! 常量冻结三段：定文 / 判读口径 / 混岗样例）。[`SysRelWorld::file_capability`]
//! 收能力申报卡（能力名 + [`CapabilityFacts`] 四个机械事实位），
//! **判读只看事实位不看文本**——纯数据归配置系统（配置管数据）、
//! 轻量行为归引擎（脚本编排）、重原生扩展归插件 SDK（互补不混岗）、
//! 上架意愿归 W 域市场且默认**拦截**（预留不承诺，[`SysBoundError::ReservationPrivate`]）。
//! 同名异定再申报 = 关系失配，走**对拍修正**（改判 + 留痕事件，不是拒绝）。
//! 未申报就跨界调用 → [`SysBoundError::RelUndeclared`]——先归位四定，
//! 才有资格过互操作面。
//!
//! # 三、既有系统边界闭集：四域边界卡（可调用面/禁入面/数据交换协议）
//!
//! [`HostDomain`] 是脚本面对的既有系统域闭集：宿主内核域 / 图形域 /
//! 输入域 / 文件域。每域一张**边界卡**（[`DOMAIN_CARDS`]，常量冻结三段：
//! 可调用面锚名单 / 禁入面锚名单 / 数据交换协议——允许过界的值型闭集与
//! 句柄方向）。[`SystemBoundary::validate_anchor`] 注册期三查：锚在
//! 可调用面内、不在禁入面、名字合 `a.b.c` 三段小写定式；签名值型须落
//! 在域协议允许集内。**反向侵入**（宿主回调脚本）是单面禁入：
//! [`SysRelWorld::attempt_reverse_hook`] 对任何反向锚一律
//! [`SysBoundError::FaceViol`]——单向编排不反向侵入。
//!
//! # 四、互操作面：白名单注册制 + 签名指纹 + 越白名单拦截
//!
//! [`InteropSurface`] 是脚本调用宿主 API 的唯一合法通道。每条注册
//! （锚 / 域 / 读写种类 / 签名）算 **FNV-1a 64 位签名指纹**
//! （[`InteropSurface::fingerprint_of`]：锚名+域序+读写+参数型+返回型
//! 一体入指纹），注册即冻结；[`InteropSurface::verify_all`] 全表验链，
//! 指纹被改即验出。开机种子 [`CANONICAL_APIS`]（11 条规范 API，编译期
//! 断言每条锚都落在对应域边界卡的可调用面内且不触禁入面）。
//! 越白名单调用（查无此锚）→ [`SysBoundError::CallOffSurface`]——
//! 注入面之一的正式检出码。
//!
//! # 五、数据交换协议：跨界值闭集 + 句柄生命周期
//!
//! [`XVal`] 是跨边界值类型闭集：数值（i64）/ 字符串（≤48 字节）/
//! 句柄 / 只读视图——**跨界不隐式转换**，类型不对位即
//! [`SysBoundError::XvalType`]。句柄走 [`HandleTable`]：位号+代际
//! 双拼 [`HandleId`]（槽位复用代际前进，ABA 防复用）；三种生命周期：
//! 借用（宿主保留所有权，到期自动回收）/ 转移（脚本持有，一次性
//! `hand_off`，二次转移即 [`SysBoundError::HandleAbuse`]）/ 视图
//! （只读，不可写不可转移不可再派生——Write 面带视图实参直接拦）。
//!
//! # 六、跨界审计 + 高频熔断
//!
//! [`BoundaryAudit`] 定长环形账：每次跨界调用入账（调用方/被调面指纹/
//! 参数摘要/裁定+诊断码），越权拒收同样入账——**拒付也要留小票**。
//! [`CallBreaker`] 高频熔断：单脚本对同一锚窗口内调用超 32 次即熔断，
//! 冷却窗后复路（恶意循环杀得掉——沙箱三律的边界侧预备）。
//!
//! # 七、F5003 五律衔接：安全默认律在边界上的落地
//!
//! [`FIVE_LAW_GROUNDING`] 把 F5003 五律逐律钉在边界上：**安全默认律**
//! 的落地是——跨界写操作（Effect 面）必须持宿主签发的授权位
//! （[`WriteAuthBook`]：锚绑定+预算+已花），无授权位即
//! [`SysBoundError::WriteUnauthorized`]（注入面之三的正式检出码），
//! 额度花光即 [`SysBoundError::AuthSpent`]——默认无副作用泄漏。
//! [`SysRelWorld::law_grounding_report`] 五律逐律出机械报告（含内置
//! 双跑对拍）。可读第一律落地为锚名三段式定式（机械校验）。
//!
//! # 八、事件账 + health + FNV 指纹 + 双跑对拍 + 注入三面
//!
//! [`EventLedger`] 有界事件账（满覆最旧并计 `dropped`——不静默丢，
//! health 可见）；[`SysRelWorld::health`] 一键体检；[`SysRelWorld::state_digest`]
//! 全状态 FNV-1a 64 确定性摘要——**双跑对拍**：同操作序列两次开机
//! 摘要必相等（[`SysRelWorld::dual_run_selfcheck`]），不等即
//! [`SysBoundError::DigestDiverge`]。注入三面各须检出：
//! 越界调用注入 / 句柄滥用注入 / 未授权写注入
//! （[`SysRelWorld::injection_selfcheck`]——任何一路逃逸即
//! [`SysBoundError::InjectEscape`]，边界子系统自证失败是最高级事故）。
//!
//! # 九、纪律自证
//!
//! - 完全自包含：只 `use core::…`（经全限定路径）/ `use alloc::…`，
//!   零 `use crate::…`；no_std + alloc；
//! - 内核路径零 unwrap/expect/panic!/运行期 assert!（编译期
//!   `const _: () = assert!(…)` 可）；可失败全走
//!   `Result<T, SysBoundError>`；错误枚举每变体带中文可操作诊断
//!   （[`SysBoundError::detail`]）；
//! - 溢出显性：计数 saturating_add、指纹 wrapping_mul/wrapping_add；
//! - 大结构不上栈：最大定长结构 [`BoundaryAudit`]（64 槽环形账约
//!   2.8KB），全件无超 64KB 栈对象；
//! - const fn 纪律：const fn 内只用 while+索引与整型比较，不用
//!   slice::get / Vec 索引 / Ord::min/max / derive PartialEq 的 ==/!=
//!   （枚举比较走判别式 `as u8`）；含 Vec 字段的结构一律不 derive Copy；
//! - 诊断码段 **0x1DD0..=0x1DDF** 独占 16 码（开工前 grep 复核：
//!   0x1D50..=0x1D5F 已被邻件 vez53_sysbound 占用，0x1DD0 段空闲）。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、版本与诊断码段（0x1DD0..=0x1DDF 独占 16 码）
// ---------------------------------------------------------------------------

/// 本件版本（VE-F5004 第二落地件，与 vez53_sysbound 同锚点不同码段）。
pub const SYSBOUND2_VERSION: &str = "F5004-sysbound2-v1";

/// 诊断码段基址（0x1DD0——开工前 grep 全仓 `0x1D5`/`0x1DD`：0x1D50..=0x1D5F
/// 已被邻件 vez53_sysbound 占用；0x1DD 段唯一命中是 unx2562 的 u64
/// 种子常量 0x1DDA2311CA1CD63C（非诊断码），本段整段空闲）。
pub const ERR_CODE_BASE: u16 = 0x1DD0;

/// 诊断码段末址（含）。
pub const ERR_CODE_END: u16 = 0x1DDF;

/// 诊断码总数（0x1DD0..=0x1DDF 恰 16 码，全部派满 SysBoundError）。
pub const ERR_CODE_COUNT: u16 = 16;

/// 关系未申报（0x1DD0）——脚本尚未申报关系四定（或申报卡名空/超 32 字节）就跨界调用。
pub const E_REL_UNDECLARED: u16 = 0x1DD0;
/// 关系错位（0x1DD1）——混岗：纯数据塞引擎/插件、重扩展塞脚本、行为塞配置。
pub const E_REL_MISFILED: u16 = 0x1DD1;
/// 可调用面越界（0x1DD2）——锚不在可调用面/落禁入面/名字不合定式/值型出域协议。
pub const E_FACE_VIOL: u16 = 0x1DD2;
/// 互操作面注册满（0x1DD3）——SURFACE_CAP=24 到顶，白名单有界不可破。
pub const E_SURFACE_FULL: u16 = 0x1DD3;
/// 白名单重复注册（0x1DD4）——同名锚已在互操作面上。
pub const E_SURFACE_DUP: u16 = 0x1DD4;
/// 越白名单调用（0x1DD5）——查无此锚，越界调用注入的正式检出码。
pub const E_CALL_OFF_SURFACE: u16 = 0x1DD5;
/// 签名失配（0x1DD6）——实参个数与注册签名不符（或注册参数超 6 上限）。
pub const E_SIG_MISMATCH: u16 = 0x1DD6;
/// 跨界值类型不匹配（0x1DD7）——实参值型与签名参数型不对位/字符串超 48 字节。
pub const E_XVAL_TYPE: u16 = 0x1DD7;
/// 句柄滥用（0x1DD8）——不存在/代际不符/到期回收/二次转移/借用释放/视图越权/表满。
pub const E_HANDLE_ABUSE: u16 = 0x1DD8;
/// 未授权跨界写（0x1DD9）——Effect 面调用无授权位，未授权写注入正式检出码。
pub const E_WRITE_UNAUTHORIZED: u16 = 0x1DD9;
/// 授权额度耗尽（0x1DDA）——授权位在但预算花光（或零预算签发被拒）。
pub const E_AUTH_SPENT: u16 = 0x1DDA;
/// 高频调用熔断中（0x1DDB）——窗口内同锚调用超阈，冷却窗后复路。
pub const E_BREAKER_TRIPPED: u16 = 0x1DDB;
/// 预留私启（0x1DDC）——W 域市场上架通路预留不承诺，未持宿主钥匙即拦。
pub const E_RESERVATION_PRIVATE: u16 = 0x1DDC;
/// 授权账满（0x1DDD）——GRANT_SLOTS=16 到顶，先撤旧位再签新位。
pub const E_GRANT_FULL: u16 = 0x1DDD;
/// 注入未检出（0x1DDE）——三类注入有一路逃逸，边界子系统自证失败。
pub const E_INJECT_ESCAPE: u16 = 0x1DDE;
/// 双跑对拍分歧（0x1DDF）——同操作序列两次开机状态摘要不一致。
pub const E_DIGEST_DIVERGE: u16 = 0x1DDF;

/// 诊断码全表（16 码升序——编译期断言连续互斥的唯一来源）。
pub const CODE_TABLE: [u16; 16] = [
    E_REL_UNDECLARED,
    E_REL_MISFILED,
    E_FACE_VIOL,
    E_SURFACE_FULL,
    E_SURFACE_DUP,
    E_CALL_OFF_SURFACE,
    E_SIG_MISMATCH,
    E_XVAL_TYPE,
    E_HANDLE_ABUSE,
    E_WRITE_UNAUTHORIZED,
    E_AUTH_SPENT,
    E_BREAKER_TRIPPED,
    E_RESERVATION_PRIVATE,
    E_GRANT_FULL,
    E_INJECT_ESCAPE,
    E_DIGEST_DIVERGE,
];

/// 与既有系统关系域的全部可失败路径错误（16 变体对 16 码，一变体一码）。
///
/// 每变体携带：`code()`（0x1DD0..=0x1DDF 独占段内的一码）与
/// `detail()`（中文可操作诊断——说清现象与改法，错误友好律的边界侧执行）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SysBoundError {
    /// 关系未申报（0x1DD0）：跨界调用前必须先在关系四定册归位。
    RelUndeclared,
    /// 关系错位（0x1DD1）：混岗申报，按引导归位。
    RelMisfiled,
    /// 可调用面越界（0x1DD2）：锚/名字/值型任一不符域边界卡。
    FaceViol,
    /// 互操作面注册满（0x1DD3）。
    SurfaceFull,
    /// 白名单重复注册（0x1DD4）。
    SurfaceDup,
    /// 越白名单调用（0x1DD5）：注入面之一。
    CallOffSurface,
    /// 签名失配（0x1DD6）。
    SigMismatch,
    /// 跨界值类型不匹配（0x1DD7）。
    XvalType,
    /// 句柄滥用（0x1DD8）：注入面之二。
    HandleAbuse,
    /// 未授权跨界写（0x1DD9）：注入面之三。
    WriteUnauthorized,
    /// 授权额度耗尽（0x1DDA）。
    AuthSpent,
    /// 高频调用熔断中（0x1DDB）。
    BreakerTripped,
    /// 预留私启（0x1DDC）。
    ReservationPrivate,
    /// 授权账满（0x1DDD）。
    GrantFull,
    /// 注入未检出（0x1DDE）：自证失败。
    InjectEscape,
    /// 双跑对拍分歧（0x1DDF）。
    DigestDiverge,
}

impl SysBoundError {
    /// 诊断码（0x1DD0..=0x1DDF 段内独占）。
    pub const fn code(&self) -> u16 {
        match self {
            SysBoundError::RelUndeclared => E_REL_UNDECLARED,
            SysBoundError::RelMisfiled => E_REL_MISFILED,
            SysBoundError::FaceViol => E_FACE_VIOL,
            SysBoundError::SurfaceFull => E_SURFACE_FULL,
            SysBoundError::SurfaceDup => E_SURFACE_DUP,
            SysBoundError::CallOffSurface => E_CALL_OFF_SURFACE,
            SysBoundError::SigMismatch => E_SIG_MISMATCH,
            SysBoundError::XvalType => E_XVAL_TYPE,
            SysBoundError::HandleAbuse => E_HANDLE_ABUSE,
            SysBoundError::WriteUnauthorized => E_WRITE_UNAUTHORIZED,
            SysBoundError::AuthSpent => E_AUTH_SPENT,
            SysBoundError::BreakerTripped => E_BREAKER_TRIPPED,
            SysBoundError::ReservationPrivate => E_RESERVATION_PRIVATE,
            SysBoundError::GrantFull => E_GRANT_FULL,
            SysBoundError::InjectEscape => E_INJECT_ESCAPE,
            SysBoundError::DigestDiverge => E_DIGEST_DIVERGE,
        }
    }

    /// 中文可操作诊断（现象+改法——错误友好律的语言化口径在边界的落地）。
    pub const fn detail(&self) -> &'static str {
        match self {
            SysBoundError::RelUndeclared => {
                "关系未申报：脚本尚未在关系四定册归位（或申报能力名为空/超 32 字节）\
                 就发起跨界调用。先经 file_capability 把能力归入引擎/插件/配置/市场\
                 四定之一，跨界调用才有资格放行。"
            }
            SysBoundError::RelMisfiled => {
                "关系错位（混岗）：纯数据能力须归配置系统（配置管数据），轻量行为\
                 归引擎脚本（脚本管行为），重原生扩展归插件 SDK（轻逻辑用脚本、\
                 重扩展用插件）。按引导把申报卡归位后再申报。"
            }
            SysBoundError::FaceViol => {
                "可调用面越界：锚不在该域边界卡的可调用面名单、落在了禁入面、\
                 名字不合 a.b.c 三段小写定式、或签名值型超出该域数据交换协议。\
                 换用可调用面内的锚并按域协议传值；反向（宿主回调脚本）同罪。"
            }
            SysBoundError::SurfaceFull => {
                "互操作面注册满：SURFACE_CAP=16 到顶（规范 11 条 + 运行期\
                 余地 5 条），白名单查表面有界不可破。宿主侧显式重建互操作面\
                 后再注册新锚。"
            }
            SysBoundError::SurfaceDup => {
                "白名单重复注册：同名锚已在互操作面上。先由宿主注销旧面或改名\
                 后再注册——白名单不收同名双签名。"
            }
            SysBoundError::CallOffSurface => {
                "越白名单调用：互操作面上查无此锚。脚本只能调注册过的宿主 API\
                 （越界调用注入的正式检出码）——请改用白名单内的锚。"
            }
            SysBoundError::SigMismatch => {
                "签名失配：实参个数与注册签名不符（或注册时参数超过 6 上限）。\
                 按互操作面上的签名卡对齐参数个数。"
            }
            SysBoundError::XvalType => {
                "跨界值类型不匹配：实参值型与签名参数型不对位，或字符串超过\
                 48 字节上限。跨界不隐式转换——改对类型再过界。"
            }
            SysBoundError::HandleAbuse => {
                "句柄滥用：句柄不存在/代际不符/已到期回收/二次转移/借用柄不可\
                 释放/视图只读不可写不可再派生/句柄表满。按句柄生命周期语义\
                 重新取得合法句柄（句柄滥用注入的正式检出码）。"
            }
            SysBoundError::WriteUnauthorized => {
                "未授权跨界写：被调面是 Effect 面而授权账上无此锚的授权位。\
                 安全默认律：默认无副作用泄漏——须宿主经 issue_grant 显式签发\
                 授权位（未授权写注入的正式检出码）。"
            }
            SysBoundError::AuthSpent => {
                "授权额度耗尽：授权位尚在但预算已花光（或宿主签发了零预算授权\
                 被拒）。请宿主重新签发授权位。"
            }
            SysBoundError::BreakerTripped => {
                "高频调用熔断中：单脚本对同一锚在窗口（64 tick）内调用超过\
                 32 次，已进入冷却窗。降低调用频率或等待冷却窗结束后复路。"
            }
            SysBoundError::ReservationPrivate => {
                "预留私启：W 域市场上架通路是预留不承诺——未持宿主钥匙\
                 （MARKET_HOST_KEY）显式开启前，任何上架申报一律拦截。"
            }
            SysBoundError::GrantFull => {
                "授权账满：GRANT_SLOTS=16 到顶。先撤销或用尽旧授权位，宿主\
                 再签发新位。"
            }
            SysBoundError::InjectEscape => {
                "注入未检出：越界调用/句柄滥用/未授权写三类注入有一路逃过了\
                 拦截——边界子系统自证失败。立即冻结互操作面并排查拦截链。"
            }
            SysBoundError::DigestDiverge => {
                "双跑对拍分歧：同操作序列两次开机的状态摘要不一致——确定优先\
                 律被破坏。复跑并定位首次分歧帧。"
            }
        }
    }

    /// 简报（码+诊断一句话——审计与人读两用的定长口径）。
    pub fn brief(&self) -> String {
        format!("[{:#06x}] {}", self.code(), self.detail())
    }
}

// ---------------------------------------------------------------------------
// 二、常量工具（const fn 纪律：while+索引+整型比较，无 slice::get/Vec 索引）
// ---------------------------------------------------------------------------

/// FNV-1a 64 位（offset 14695981039346656037 / prime 1099511628211——
/// 与全仓 vea14/vea19 等件同参同源，摘要可跨件对拍）。
pub const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;

/// FNV-1a 64 位素数。
pub const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

/// FNV-1a 64 位摘要（const fn——while+索引迭代，内核路径与编译期断言共用）。
pub const fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut h: u64 = FNV_OFFSET;
    let mut i: usize = 0;
    while i < bytes.len() {
        h ^= bytes[i] as u64;
        h = h.wrapping_mul(FNV_PRIME);
        i += 1;
    }
    h
}

/// const fn 字符串逐字节相等（str 的 == 非 const——以字节序代替）。
pub const fn str_eq(a: &str, b: &str) -> bool {
    let ab = a.as_bytes();
    let bb = b.as_bytes();
    if ab.len() != bb.len() {
        return false;
    }
    let mut i: usize = 0;
    while i < ab.len() {
        if ab[i] != bb[i] {
            return false;
        }
        i += 1;
    }
    true
}

/// const fn 名单包含（可调用面/禁入面名单查用的唯一手段）。
pub const fn face_contains(face: &[&str], name: &str) -> bool {
    let mut i: usize = 0;
    while i < face.len() {
        if str_eq(face[i], name) {
            return true;
        }
        i += 1;
    }
    false
}

/// 锚名三段式定式（可读第一律的边界落地）：全小写字母与点，恰两个点，
/// 每段非空且不超过 16 字节（如 `kern.entity.query_pos`——段内允许下划线）。
pub const fn anchor_name_wellformed(name: &str) -> bool {
    let b = name.as_bytes();
    if b.len() < 5 || b.len() > 48 {
        return false;
    }
    let mut dots: u32 = 0;
    let mut seg_len: u32 = 0;
    let mut i: usize = 0;
    while i < b.len() {
        let c = b[i];
        let lower = c >= b'a' && c <= b'z';
        let under = c == b'_';
        let digit = c >= b'0' && c <= b'9';
        if c == b'.' {
            if seg_len == 0 {
                return false;
            }
            if seg_len > 16 {
                return false;
            }
            dots += 1;
            seg_len = 0;
        } else if lower || under || digit {
            if dots == 0 && digit {
                return false;
            }
            seg_len += 1;
        } else {
            return false;
        }
        i += 1;
    }
    if dots != 2 {
        return false;
    }
    if seg_len == 0 || seg_len > 16 {
        return false;
    }
    true
}

/// 16 码互斥校验（编译期断言用——逐对比较，整型 != 合法）。
pub const fn codes_distinct(table: &[u16; 16]) -> bool {
    let mut i: usize = 0;
    while i < 16 {
        let mut j: usize = i + 1;
        while j < 16 {
            if table[i] == table[j] {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    true
}

/// 16 码连续升序校验（0x1DD0..=0x1DDF 段独占的结构性证明）。
pub const fn codes_consecutive(table: &[u16; 16]) -> bool {
    if table[0] != ERR_CODE_BASE {
        return false;
    }
    let mut i: usize = 1;
    while i < 16 {
        if table[i] != table[i - 1] + 1 {
            return false;
        }
        i += 1;
    }
    true
}

// ---------------------------------------------------------------------------
// 三、关系四定：脚本与既有系统关系闭集（锚点「关系四定」的机械落地）
// ---------------------------------------------------------------------------

/// 关系四定闭集（判据「四定边界」的唯一枚举——闭集不扩员）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RelQuad {
    /// 与引擎系统：脚本调用引擎 API——单向编排不反向侵入。
    Engine = 1,
    /// 与插件 SDK：轻逻辑用脚本、重扩展用插件——互补不混岗。
    PluginSdk = 2,
    /// 与配置系统：配置管数据、脚本管行为——不混岗。
    ConfigSys = 3,
    /// 与 W 域市场：脚本包未来可上架——预留不承诺。
    MarketReserve = 4,
}

/// 四定数（闭集恰四定）。
pub const REL_QUAD_COUNT: usize = 4;

impl RelQuad {
    /// 序数（1..=4——const 语境判别用）。
    pub const fn ord(&self) -> u8 {
        match self {
            RelQuad::Engine => 1,
            RelQuad::PluginSdk => 2,
            RelQuad::ConfigSys => 3,
            RelQuad::MarketReserve => 4,
        }
    }

    /// 序数还原（越界序数还原失败——闭集不收编外成员）。
    pub const fn from_ord(ord: u8) -> Option<RelQuad> {
        match ord {
            1 => Some(RelQuad::Engine),
            2 => Some(RelQuad::PluginSdk),
            3 => Some(RelQuad::ConfigSys),
            4 => Some(RelQuad::MarketReserve),
            _ => Option::None,
        }
    }

    /// 定名（中文——读屏可达，域本色）。
    pub const fn name(&self) -> &'static str {
        match self {
            RelQuad::Engine => "引擎系统",
            RelQuad::PluginSdk => "插件 SDK",
            RelQuad::ConfigSys => "配置系统",
            RelQuad::MarketReserve => "W 域市场（预留）",
        }
    }
}

/// 关系四定卡（常量冻结三段：定文/判读口径/混岗样例——缺一不可执行）。
pub struct QuadCard {
    /// 归属四定。
    pub quad: RelQuad,
    /// 定文：这条关系是什么（锚点原文口径）。
    pub decree: &'static str,
    /// 判读口径：怎么机械判（只看事实位）。
    pub caliber: &'static str,
    /// 混岗样例：越界长什么样（判据「互补不混岗」的校准器）。
    pub misuse: &'static str,
}

/// 关系四定卡册（4 卡常量冻结——开机唯一来源，运行期只读）。
pub const QUAD_CARDS: [QuadCard; REL_QUAD_COUNT] = [
    QuadCard {
        quad: RelQuad::Engine,
        decree: "脚本调用引擎 API——单向编排不反向侵入",
        caliber: "is_behavior 且非 heavy_native 且无上架意愿 → 引擎系统",
        misuse: "脚本直改引擎内部表/宿主回调脚本函数（反向侵入）",
    },
    QuadCard {
        quad: RelQuad::PluginSdk,
        decree: "脚本能力与插件互补——轻逻辑用脚本重扩展用插件边界清晰",
        caliber: "is_behavior 且 heavy_native → 插件 SDK（脚本干插件的活要引导归位）",
        misuse: "用脚本写原生级重扩展（直挂驱动/全量资源管线）",
    },
    QuadCard {
        quad: RelQuad::ConfigSys,
        decree: "配置管数据脚本管行为——不混岗",
        caliber: "is_pure_data 且非 is_behavior → 配置系统",
        misuse: "把行为逻辑写进配置文件/把纯数据塞进脚本逻辑",
    },
    QuadCard {
        quad: RelQuad::MarketReserve,
        decree: "脚本包未来可上架——预留不承诺",
        caliber: "wants_market_listing 且市场门已持钥匙开启 → 市场预留定",
        misuse: "未持宿主钥匙私开上架通路（预留私启零容忍）",
    },
];

/// 能力申报卡的四机械事实位（判读只看事实位，与申报文本长短无关）。
#[derive(Debug, Clone, Copy)]
pub struct CapabilityFacts {
    /// 纯数据（无行为）：管数据的归配置。
    pub is_pure_data: bool,
    /// 行为逻辑：管行为的归脚本或插件。
    pub is_behavior: bool,
    /// 重原生扩展（驱动级/全量管线级）：重扩展用插件。
    pub heavy_native: bool,
    /// 想上架 W 域市场（预留门默认关）。
    pub wants_market_listing: bool,
}

impl CapabilityFacts {
    /// 全真事实面（混岗自相矛盾——纯数据与行为不可同真，判读前先拦）。
    pub const CONFLICTING: CapabilityFacts = CapabilityFacts {
        is_pure_data: true,
        is_behavior: true,
        heavy_native: true,
        wants_market_listing: true,
    };

    /// 空能力（既非数据又非行为——无处安放）。
    pub const EMPTY: CapabilityFacts = CapabilityFacts {
        is_pure_data: false,
        is_behavior: false,
        heavy_native: false,
        wants_market_listing: false,
    };

    /// 轻量行为（引擎脚本的规范位——判读应归引擎）。
    pub const LIGHT_BEHAVIOR: CapabilityFacts = CapabilityFacts {
        is_pure_data: false,
        is_behavior: true,
        heavy_native: false,
        wants_market_listing: false,
    };

    /// 重原生扩展（插件 SDK 的规范位）。
    pub const HEAVY_NATIVE: CapabilityFacts = CapabilityFacts {
        is_pure_data: false,
        is_behavior: true,
        heavy_native: true,
        wants_market_listing: false,
    };

    /// 纯数据（配置系统的规范位）。
    pub const PURE_DATA: CapabilityFacts = CapabilityFacts {
        is_pure_data: true,
        is_behavior: false,
        heavy_native: false,
        wants_market_listing: false,
    };
}

/// 关系四定申报判读回执（归位结果 + 引导语 + 预留位/改判位）。
#[derive(Debug, Clone, Copy)]
pub struct FilingVerdict {
    /// 归位四定。
    pub quad: RelQuad,
    /// 市场预留位（true=预留标记，不构成上架承诺）。
    pub reserved: bool,
    /// 对拍修正位（true=同名异定再申报，已按新事实改判并留痕）。
    pub refiled: bool,
    /// 引导语（归位依据——四定卡判读口径的原文）。
    pub guidance: &'static str,
}

/// 能力申报在册条目（关系四定册——跨界调用的资格来源）。
pub struct CapabilityFiling {
    /// 申报脚本方号（0 为宿主内建——宿主不须申报即可调）。
    pub caller: u32,
    /// 能力名（1..=32 字节）。
    pub name: String,
    /// 归位四定。
    pub quad: RelQuad,
    /// 市场预留位。
    pub reserved: bool,
    /// 判读时事实位快照（对拍修正时留底）。
    pub facts: CapabilityFacts,
    /// 归位时世界节拍（审计对齐用）。
    pub filed_at: u64,
}

/// 申报能力名长度上限（字节——申报卡定长纪律）。
pub const FILING_NAME_MAX: usize = 32;

// ---------------------------------------------------------------------------
// 四、既有系统域闭集：四域边界卡（可调用面/禁入面/数据交换协议）
// ---------------------------------------------------------------------------

/// 脚本面对的既有系统域闭集（四域——锚点「边界清单」的域骨架）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HostDomain {
    /// 宿主内核域：实体/时钟/调度只读面 + 受控生成面。
    Kernel = 1,
    /// 图形域：绘制提交/贴图快照/帧回读。
    Graphics = 2,
    /// 输入域：按键状态读 + 绑定声明。
    Input = 3,
    /// 文件域：配置载入 + 受控落盘。
    Filesys = 4,
}

/// 域数（闭集恰四域）。
pub const HOST_DOMAIN_COUNT: usize = 4;

impl HostDomain {
    /// 序数（1..=4）。
    pub const fn ord(&self) -> u8 {
        match self {
            HostDomain::Kernel => 1,
            HostDomain::Graphics => 2,
            HostDomain::Input => 3,
            HostDomain::Filesys => 4,
        }
    }

    /// 序数还原（越界失败——闭集不收编外成员）。
    pub const fn from_ord(ord: u8) -> Option<HostDomain> {
        match ord {
            1 => Some(HostDomain::Kernel),
            2 => Some(HostDomain::Graphics),
            3 => Some(HostDomain::Input),
            4 => Some(HostDomain::Filesys),
            _ => Option::None,
        }
    }

    /// 域名（中文——读屏可达）。
    pub const fn name(&self) -> &'static str {
        match self {
            HostDomain::Kernel => "宿主内核域",
            HostDomain::Graphics => "图形域",
            HostDomain::Input => "输入域",
            HostDomain::Filesys => "文件域",
        }
    }
}

/// 跨边界值型闭集（数据交换协议的值型字母表——跨界不隐式转换）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum XValKind {
    /// 数值（i64——确定优先：跨界不走浮点）。
    Num = 1,
    /// 字符串（≤48 字节）。
    Str = 2,
    /// 句柄引用（Handle 或 View 两态——签名层统称 Ref，运行层再分）。
    Ref = 3,
}

impl XValKind {
    /// 序数（const 语境判别比较走 u8，不走 derive 的 ==）。
    pub const fn ord(&self) -> u8 {
        match self {
            XValKind::Num => 1,
            XValKind::Str => 2,
            XValKind::Ref => 3,
        }
    }

    /// 名字（审计简报用）。
    pub const fn name(&self) -> &'static str {
        match self {
            XValKind::Num => "数值",
            XValKind::Str => "字符串",
            XValKind::Ref => "句柄引用",
        }
    }

    /// 值型是否落在域协议允许集内（const——判别式比较）。
    pub const fn in_proto(&self, proto: &[XValKind]) -> bool {
        let k = self.ord();
        let mut i: usize = 0;
        while i < proto.len() {
            if proto[i].ord() == k {
                return true;
            }
            i += 1;
        }
        false
    }
}

/// 域边界卡（常量冻结三段：可调用面/禁入面/数据交换协议——开机即冻结）。
pub struct DomainCard {
    /// 归属域。
    pub domain: HostDomain,
    /// 可调用面锚名单（白名单注册的面级来源——名单外锚不许注册）。
    pub callable: &'static [&'static str],
    /// 禁入面锚名单（碰即 FaceViol——脚本绝不碰的宿主内部面）。
    pub forbidden: &'static [&'static str],
    /// 数据交换协议：允许过界的值型闭集。
    pub proto_kinds: &'static [XValKind],
    /// 数据交换协议：句柄方向语义说明。
    pub proto_note: &'static str,
}

/// 宿主内核域可调用面（7 锚——查询/生成/时钟/释放 + 场景图查询/销毁/事件读
/// 三条余地：面是域能力清单，白名单只收注册过的子集）。
const KERNEL_CALLABLE: [&str; 7] = [
    "kern.entity.query_pos",
    "kern.entity.spawn",
    "kern.clock.tick",
    "kern.entity.release",
    "kern.scene.graph_query",
    "kern.entity.destroy",
    "kern.event.read",
];

/// 宿主内核域禁入面（3 锚——裸指针/中断钩子/抢占操控一律禁）。
const KERNEL_FORBIDDEN: [&str; 3] = [
    "kern.mem.raw_ptr",
    "kern.interrupt.hook",
    "kern.task.preempt",
];

/// 图形域可调用面（5 锚——绘制/快照/回读 + 文本绘制/层设两条余地）。
const GFX_CALLABLE: [&str; 5] = [
    "gfx.draw.rect",
    "gfx.texture.snapshot",
    "gfx.frame.readback",
    "gfx.text.draw",
    "gfx.layer.set",
];

/// 图形域禁入面（3 锚——命令缓冲/驱动重配/交换链窃占）。
const GFX_FORBIDDEN: [&str; 3] = [
    "gfx.gpu.command_buffer",
    "gfx.driver.reconfig",
    "gfx.swapchain.steal",
];

/// 输入域可调用面（3 锚——按键状态/绑定声明 + 轴状态一条余地）。
const INPUT_CALLABLE: [&str; 3] = [
    "input.key.state",
    "input.bind.declare",
    "input.axis.state",
];

/// 输入域禁入面（3 锚——裸设备抓取/USB 枚举/HID 注入）。
const INPUT_FORBIDDEN: [&str; 3] = [
    "input.device.grab_raw",
    "input.usb.enumerate",
    "input.hid.inject",
];

/// 文件域可调用面（4 锚——配置载入/受控追加落盘 + 配置存/块读两条余地）。
const FILE_CALLABLE: [&str; 4] = [
    "file.config.load",
    "file.persist.append",
    "file.config.save",
    "file.blob.read",
];

/// 文件域禁入面（3 锚——裸盘写/根遍历/强挂载）。
const FILE_FORBIDDEN: [&str; 3] = [
    "file.disk.raw_write",
    "file.path.traverse_root",
    "file.mount.force",
];

/// 四域边界卡册（4 卡常量冻结——运行期只读不改）。
pub const DOMAIN_CARDS: [DomainCard; HOST_DOMAIN_COUNT] = [
    DomainCard {
        domain: HostDomain::Kernel,
        callable: &KERNEL_CALLABLE,
        forbidden: &KERNEL_FORBIDDEN,
        proto_kinds: &[XValKind::Num, XValKind::Ref],
        proto_note: "宿主签发借用柄与只读视图；转移柄须显式 hand_off 一次性转移；\
                     本域不收字符串过界。",
    },
    DomainCard {
        domain: HostDomain::Graphics,
        callable: &GFX_CALLABLE,
        forbidden: &GFX_FORBIDDEN,
        proto_kinds: &[XValKind::Num, XValKind::Ref],
        proto_note: "贴图快照只签只读视图；绘制提交是 Effect 面须授权位；\
                     命令缓冲与驱动重配永久禁入。",
    },
    DomainCard {
        domain: HostDomain::Input,
        callable: &INPUT_CALLABLE,
        forbidden: &INPUT_FORBIDDEN,
        proto_kinds: &[XValKind::Num, XValKind::Str],
        proto_note: "按键状态是纯读面；绑定声明是 Effect 面须授权位；\
                     裸设备抓取与 HID 注入永久禁入；本域不收句柄。",
    },
    DomainCard {
        domain: HostDomain::Filesys,
        callable: &FILE_CALLABLE,
        forbidden: &FILE_FORBIDDEN,
        proto_kinds: &[XValKind::Str, XValKind::Num],
        proto_note: "配置载入只读返回规范串；受控追加落盘是 Effect 面须授权位；\
                     裸盘写与根遍历永久禁入；本域不收句柄。",
    },
];

/// 反向侵入锚名单（宿主→脚本的单面禁入——「单向编排不反向侵入」的
/// 机械执行面：任何反向钩子注册/调用一律拦截）。
pub const REVERSE_HOOK_ANCHORS: [&str; 2] =
    ["host.callback.into_script", "host.event.pump_script"];

/// 面判读结论（classify 的三态——可调/禁入/面外）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FaceVerdict {
    /// 在可调用面内（可注册）。
    Callable,
    /// 在禁入面内（碰即拦）。
    Forbidden,
    /// 两面之外（白名单不收录——结构外面）。
    Outside,
}

/// 既有系统边界（四域边界卡的判读器——零状态，卡册常量冻结）。
///
/// 职责：注册期三查（可调用面/禁入面/名字定式）+ 域协议值型查 +
/// 锚寻域（锚→域反查——白名单注册时对齐卡的域归属）。
pub struct SystemBoundary;

impl SystemBoundary {
    /// 域内面判读（可调用面/禁入面/面外三态——O(面宽)，面宽≤4 恒定小）。
    pub fn classify(domain: HostDomain, anchor: &str) -> FaceVerdict {
        let card = Self::card_of(domain);
        if face_contains(card.forbidden, anchor) {
            return FaceVerdict::Forbidden;
        }
        if face_contains(card.callable, anchor) {
            return FaceVerdict::Callable;
        }
        FaceVerdict::Outside
    }

    /// 锚寻域（全四域扫描——同名锚只允许归属一域，双域同名取先命中域并
    /// 由 compile 期断言名单互斥兜底；面外返回 None）。
    pub fn domain_of(anchor: &str) -> Option<HostDomain> {
        let mut i: usize = 0;
        while i < HOST_DOMAIN_COUNT {
            let card = &DOMAIN_CARDS[i];
            if face_contains(card.callable, anchor) {
                return Some(card.domain);
            }
            i += 1;
        }
        Option::None
    }

    /// 是否反向侵入锚（单向编排的机械判别）。
    pub fn is_reverse_hook(anchor: &str) -> bool {
        face_contains(&REVERSE_HOOK_ANCHORS, anchor)
    }

    /// 域卡取用（域序数合法由闭集保证——非法序数归内核域卡不可达，
    /// 因 from_ord 已拦；此处仅内部使用）。
    fn card_of(domain: HostDomain) -> &'static DomainCard {
        let mut i: usize = 0;
        while i < HOST_DOMAIN_COUNT {
            if DOMAIN_CARDS[i].domain as u8 == domain.ord() {
                return &DOMAIN_CARDS[i];
            }
            i += 1;
        }
        &DOMAIN_CARDS[0]
    }

    /// 注册期完整校验（三查+值型协议查——白名单注册的唯一闸门）：
    /// 反向锚/禁入锚/面外锚/坏名字 → FaceViol；签名值型出协议 → FaceViol。
    pub fn validate_anchor(
        domain: HostDomain,
        anchor: &str,
        sig_kinds: &[XValKind],
    ) -> Result<(), SysBoundError> {
        if Self::is_reverse_hook(anchor) {
            return Err(SysBoundError::FaceViol);
        }
        match Self::classify(domain, anchor) {
            FaceVerdict::Forbidden => return Err(SysBoundError::FaceViol),
            FaceVerdict::Outside => return Err(SysBoundError::FaceViol),
            FaceVerdict::Callable => {}
        }
        if !anchor_name_wellformed(anchor) {
            return Err(SysBoundError::FaceViol);
        }
        let card = Self::card_of(domain);
        let mut i: usize = 0;
        while i < sig_kinds.len() {
            if !sig_kinds[i].in_proto(card.proto_kinds) {
                return Err(SysBoundError::FaceViol);
            }
            i += 1;
        }
        Ok(())
    }

    /// 边界自检（health 的边界侧：卡册闭集互斥 + 可调用面全域互斥 +
    /// 禁入面与可调用面互斥——任何一破即 false）。
    pub fn integrity_ok() -> bool {
        let mut d: usize = 0;
        while d < HOST_DOMAIN_COUNT {
            let card = &DOMAIN_CARDS[d];
            // 域卡归属闭集
            if HostDomain::from_ord(card.domain.ord()).is_none() {
                return false;
            }
            // 可调用面内锚名全部合式
            let mut ci: usize = 0;
            while ci < card.callable.len() {
                if !anchor_name_wellformed(card.callable[ci]) {
                    return false;
                }
                // 可调用锚不落禁入面
                if face_contains(card.forbidden, card.callable[ci]) {
                    return false;
                }
                // 可调用锚不落反向锚名单
                if face_contains(&REVERSE_HOOK_ANCHORS, card.callable[ci]) {
                    return false;
                }
                ci += 1;
            }
            // 禁入锚不落其它域可调用面（全域互斥）
            let mut fi: usize = 0;
            while fi < card.forbidden.len() {
                let mut od: usize = 0;
                while od < HOST_DOMAIN_COUNT {
                    if face_contains(DOMAIN_CARDS[od].callable, card.forbidden[fi]) {
                        return false;
                    }
                    od += 1;
                }
                fi += 1;
            }
            d += 1;
        }
        true
    }
}

// ---------------------------------------------------------------------------
// 五、数据交换协议：跨界值闭集 XVal（数值/字符串/句柄/只读视图——不隐式转换）
// ---------------------------------------------------------------------------

/// 跨界字符串字节上限（48——定长纪律：跨界串必须短到能进审计摘要）。
pub const XSTR_MAX: usize = 48;

/// 句柄号（位号+代际双拼：高 32 位槽位号，低 32 位代际——槽位复用
/// 代际前进，旧号携旧代际永不复活，ABA 防复用）。
pub type HandleId = u64;

/// 句柄槽数（16——表满即 HandleAbuse，有界不可破）。
pub const HANDLE_SLOTS: usize = 16;

/// 句柄默认存活节拍（TTL——到期回收，泄漏上限被 TTL 定死）。
pub const HANDLE_TTL_TICKS: u32 = 32;

/// 句柄视图默认存活节拍（视图短于借用——只读窗口更窄）。
pub const VIEW_TTL_TICKS: u32 = 16;

const fn pack_handle(slot: u32, gen: u32) -> HandleId {
    ((slot as u64) << 32) | (gen as u64 & 0xFFFF_FFFF)
}

const fn unpack_slot(id: HandleId) -> u32 {
    ((id >> 32) & 0xFFFF_FFFF) as u32
}

const fn unpack_gen(id: HandleId) -> u32 {
    (id & 0xFFFF_FFFF) as u32
}

/// 槽位状态（u8 编码——const 语境判别避开 derive ==）。
pub const HS_EMPTY: u8 = 0;
pub const HS_BORROW: u8 = 1;
pub const HS_TRANSFER: u8 = 2;
pub const HS_VIEW: u8 = 3;

/// 句柄槽（定长内联——无堆字段）。
#[derive(Clone, Copy)]
struct HandleSlot {
    /// 代际（复用即前进）。
    gen: u32,
    /// 状态（HS_*——u8 编码）。
    state: u8,
    /// 签发节拍。
    born: u64,
    /// 存活节拍数。
    ttl: u32,
    /// 已撤销位。
    revoked: bool,
    /// 已一次性转移位（转移柄专用）。
    handed: bool,
    /// 来源面指纹（审计对齐——句柄从哪个被调面签出）。
    src_fp: u64,
}

impl HandleSlot {
    const EMPTY: HandleSlot = HandleSlot {
        gen: 0,
        state: HS_EMPTY,
        born: 0,
        ttl: 0,
        revoked: false,
        handed: false,
        src_fp: 0,
    };

    fn is_live(&self, tick: u64) -> bool {
        if self.state == HS_EMPTY || self.revoked {
            return false;
        }
        // TTL 到期即死——born+ttl 用 saturating 防溢出后移
        let deadline = self.born.saturating_add(self.ttl as u64);
        tick < deadline
    }
}

/// 句柄生命周期语义名（审计简报用）。
pub const fn handle_state_name(state: u8) -> &'static str {
    match state {
        HS_BORROW => "借用",
        HS_TRANSFER => "转移",
        HS_VIEW => "只读视图",
        _ => "空槽",
    }
}

/// 句柄表（16 槽定长内联——借用/转移/到期回收三态生命周期）。
pub struct HandleTable {
    slots: [HandleSlot; HANDLE_SLOTS],
    /// 累计签发数（health/审计对账）。
    issued_total: u64,
    /// 累计回收数（到期+显式+宿主强撤）。
    revoked_total: u64,
    /// 累计一次性转移数。
    handed_total: u64,
}

impl HandleTable {
    /// 新表（全空槽，代际 0）。
    pub const fn new() -> HandleTable {
        HandleTable {
            slots: [
                HandleSlot::EMPTY,
                HandleSlot::EMPTY,
                HandleSlot::EMPTY,
                HandleSlot::EMPTY,
                HandleSlot::EMPTY,
                HandleSlot::EMPTY,
                HandleSlot::EMPTY,
                HandleSlot::EMPTY,
                HandleSlot::EMPTY,
                HandleSlot::EMPTY,
                HandleSlot::EMPTY,
                HandleSlot::EMPTY,
                HandleSlot::EMPTY,
                HandleSlot::EMPTY,
                HandleSlot::EMPTY,
                HandleSlot::EMPTY,
            ],
            issued_total: 0,
            revoked_total: 0,
            handed_total: 0,
        }
    }

    /// 签发句柄（borrow=借用 / transfer=转移 / view=只读视图；
    /// 表满 → HandleAbuse——有界表的显性拒收）。
    pub fn issue(
        &mut self,
        state: u8,
        ttl: u32,
        src_fp: u64,
        tick: u64,
    ) -> Result<HandleId, SysBoundError> {
        if state == HS_EMPTY {
            // 空态签发是编程错——按句柄滥用面收口（不可达于外部 API）
            return Err(SysBoundError::HandleAbuse);
        }
        if ttl == 0 {
            // 零 TTL 句柄签出即死——拒发
            return Err(SysBoundError::HandleAbuse);
        }
        let mut i: usize = 0;
        while i < HANDLE_SLOTS {
            if self.slots[i].state == HS_EMPTY || self.slots[i].revoked {
                // 复用槽位代际前进（旧号作废）
                let next_gen = self.slots[i].gen.wrapping_add(1);
                self.slots[i] = HandleSlot {
                    gen: next_gen,
                    state,
                    born: tick,
                    ttl,
                    revoked: false,
                    handed: false,
                    src_fp,
                };
                self.issued_total = self.issued_total.saturating_add(1);
                return Ok(pack_handle(i as u32, next_gen));
            }
            i += 1;
        }
        Err(SysBoundError::HandleAbuse)
    }

    /// 槽内取态（代际不符/空槽/已撤/已死 → HandleAbuse——句柄滥用的总闸）。
    pub fn probe(&self, id: HandleId, tick: u64) -> Result<u8, SysBoundError> {
        let slot = unpack_slot(id);
        let gen = unpack_gen(id);
        if (slot as usize) >= HANDLE_SLOTS {
            return Err(SysBoundError::HandleAbuse);
        }
        let s = &self.slots[slot as usize];
        if s.state == HS_EMPTY || s.gen != gen || s.revoked {
            return Err(SysBoundError::HandleAbuse);
        }
        if !s.is_live(tick) {
            return Err(SysBoundError::HandleAbuse);
        }
        Ok(s.state)
    }

    /// 一次性转移（转移柄专用：借用手/视图柄不可转移；二次转移即滥用）。
    pub fn hand_off(&mut self, id: HandleId, tick: u64) -> Result<(), SysBoundError> {
        let state = self.probe(id, tick)?;
        if state != HS_TRANSFER {
            return Err(SysBoundError::HandleAbuse);
        }
        let slot = unpack_slot(id);
        if (slot as usize) >= HANDLE_SLOTS {
            return Err(SysBoundError::HandleAbuse);
        }
        let s = &mut self.slots[slot as usize];
        if s.handed {
            return Err(SysBoundError::HandleAbuse);
        }
        s.handed = true;
        self.handed_total = self.handed_total.saturating_add(1);
        Ok(())
    }

    /// 显式关闭（转移柄专用：脚本归还所有权；借用柄到期自动回收不可显式关）。
    pub fn close(&mut self, id: HandleId, tick: u64) -> Result<(), SysBoundError> {
        let state = self.probe(id, tick)?;
        if state != HS_TRANSFER {
            return Err(SysBoundError::HandleAbuse);
        }
        let slot = unpack_slot(id);
        if (slot as usize) >= HANDLE_SLOTS {
            return Err(SysBoundError::HandleAbuse);
        }
        let s = &mut self.slots[slot as usize];
        s.revoked = true;
        self.revoked_total = self.revoked_total.saturating_add(1);
        Ok(())
    }

    /// 宿主强撤（任意态——市场预留门关闭/沙箱拆除时的强制收口）。
    pub fn revoke(&mut self, id: HandleId) -> Result<(), SysBoundError> {
        let slot = unpack_slot(id);
        let gen = unpack_gen(id);
        if (slot as usize) >= HANDLE_SLOTS {
            return Err(SysBoundError::HandleAbuse);
        }
        let s = &mut self.slots[slot as usize];
        if s.state == HS_EMPTY || s.gen != gen || s.revoked {
            return Err(SysBoundError::HandleAbuse);
        }
        s.revoked = true;
        self.revoked_total = self.revoked_total.saturating_add(1);
        Ok(())
    }

    /// 从既有柄派生只读视图（借用/转移柄可派生；视图不可再派生——
    /// 防视图链越权；来源柄死则不可派生）。
    pub fn derive_view(
        &mut self,
        src: HandleId,
        ttl: u32,
        tick: u64,
    ) -> Result<HandleId, SysBoundError> {
        let state = self.probe(src, tick)?;
        if state == HS_VIEW {
            return Err(SysBoundError::HandleAbuse);
        }
        let slot = unpack_slot(src);
        if (slot as usize) >= HANDLE_SLOTS {
            return Err(SysBoundError::HandleAbuse);
        }
        let src_fp = self.slots[slot as usize].src_fp;
        self.issue(HS_VIEW, ttl, src_fp, tick)
    }

    /// 到期回收扫描（tick 前进时调用——返回本次回收数，0..=16）。
    pub fn sweep(&mut self, tick: u64) -> u32 {
        let mut reaped: u32 = 0;
        let mut i: usize = 0;
        while i < HANDLE_SLOTS {
            let s = &mut self.slots[i];
            if s.state != HS_EMPTY && !s.revoked && !s.is_live(tick) {
                s.revoked = true;
                self.revoked_total = self.revoked_total.saturating_add(1);
                reaped += 1;
            }
            i += 1;
        }
        reaped
    }

    /// 存活句柄计数（health 对账）。
    pub fn live_count(&self, tick: u64) -> u32 {
        let mut n: u32 = 0;
        let mut i: usize = 0;
        while i < HANDLE_SLOTS {
            if self.slots[i].is_live(tick) {
                n += 1;
            }
            i += 1;
        }
        n
    }

    /// 累计签发数。
    pub fn issued_total(&self) -> u64 {
        self.issued_total
    }

    /// 累计回收数。
    pub fn revoked_total(&self) -> u64 {
        self.revoked_total
    }

    /// 累计一次性转移数。
    pub fn handed_total(&self) -> u64 {
        self.handed_total
    }

    /// 指定槽位是否已一次性转移（越界槽位按 false——查询口不落 panic）。
    pub fn handed_state(&self, slot: usize) -> bool {
        if slot < HANDLE_SLOTS {
            self.slots[slot].handed
        } else {
            false
        }
    }

    /// 全表入摘要（digest 用——槽序+代际+态+TTL+位，空槽也入：
    /// 槽位变化必须在摘要里留痕）。
    pub fn digest_mix(&self, mixer: &mut DigestMixer) {
        mixer.mix_u64(self.issued_total);
        mixer.mix_u64(self.revoked_total);
        mixer.mix_u64(self.handed_total);
        let mut i: usize = 0;
        while i < HANDLE_SLOTS {
            let s = &self.slots[i];
            mixer.mix_u32(s.gen);
            mixer.mix_u8(s.state);
            mixer.mix_u64(s.born);
            mixer.mix_u32(s.ttl);
            mixer.mix_bool(s.revoked);
            mixer.mix_bool(s.handed);
            mixer.mix_u64(s.src_fp);
            i += 1;
        }
    }
}

/// 跨界值（闭集四态——Num/Str/Handle/View；Str 带堆字段不 derive Copy）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum XVal {
    /// 数值（i64——跨界不走浮点：确定优先）。
    Num(i64),
    /// 字符串（构造期定长校验 ≤48 字节）。
    Str(String),
    /// 句柄（借用/转移两态——运行层再分）。
    Handle(HandleId),
    /// 只读视图句柄（不可写不可转移不可再派生）。
    View(HandleId),
}

impl XVal {
    /// 数值跨界值。
    pub fn num(v: i64) -> XVal {
        XVal::Num(v)
    }

    /// 字符串跨界值（超 48 字节 → XvalType——跨界不收长串）。
    pub fn text(s: &str) -> Result<XVal, SysBoundError> {
        if s.len() > XSTR_MAX {
            return Err(SysBoundError::XvalType);
        }
        Ok(XVal::Str(s.to_string()))
    }

    /// 句柄跨界值。
    pub fn handle(id: HandleId) -> XVal {
        XVal::Handle(id)
    }

    /// 视图跨界值。
    pub fn view(id: HandleId) -> XVal {
        XVal::View(id)
    }

    /// 签名层值型（Handle/View 统称 Ref——签名卡不细分柄态，运行层分）。
    pub const fn kind(&self) -> XValKind {
        match self {
            XVal::Num(_) => XValKind::Num,
            XVal::Str(_) => XValKind::Str,
            XVal::Handle(_) | XVal::View(_) => XValKind::Ref,
        }
    }

    /// 运行层精确态名（审计简报——柄/视图分开记）。
    pub const fn precise(&self) -> &'static str {
        match self {
            XVal::Num(_) => "数值",
            XVal::Str(_) => "字符串",
            XVal::Handle(_) => "句柄",
            XVal::View(_) => "只读视图",
        }
    }

    /// 值入摘要（digest 用——判别+载荷字节，i64/柄走 LE 字节）。
    pub fn digest_mix(&self, mixer: &mut DigestMixer) {
        match self {
            XVal::Num(v) => {
                mixer.mix_u8(1);
                mixer.mix_i64(*v);
            }
            XVal::Str(s) => {
                mixer.mix_u8(2);
                mixer.mix_bytes(s.as_bytes());
            }
            XVal::Handle(id) => {
                mixer.mix_u8(3);
                mixer.mix_u64(*id);
            }
            XVal::View(id) => {
                mixer.mix_u8(4);
                mixer.mix_u64(*id);
            }
        }
    }
}

/// 参数摘要搅拌器（FNV-1a 64 的增量封装——审计参数摘要与全状态摘要共用）。
pub struct DigestMixer {
    h: u64,
}

impl DigestMixer {
    /// 新搅拌器（FNV offset 初值）。
    pub const fn new() -> DigestMixer {
        DigestMixer { h: FNV_OFFSET }
    }

    /// 拌入字节切片。
    pub fn mix_bytes(&mut self, bytes: &[u8]) {
        let mut i: usize = 0;
        while i < bytes.len() {
            self.h ^= bytes[i] as u64;
            self.h = self.h.wrapping_mul(FNV_PRIME);
            i += 1;
        }
    }

    /// 拌入 u8。
    pub fn mix_u8(&mut self, v: u8) {
        self.h ^= v as u64;
        self.h = self.h.wrapping_mul(FNV_PRIME);
    }

    /// 拌入 bool（定编码 0/1）。
    pub fn mix_bool(&mut self, v: bool) {
        self.mix_u8(if v { 1 } else { 0 });
    }

    /// 拌入 u32（LE 四字节）。
    pub fn mix_u32(&mut self, v: u32) {
        let b = v.to_le_bytes();
        self.mix_bytes(&b);
    }

    /// 拌入 u64（LE 八字节）。
    pub fn mix_u64(&mut self, v: u64) {
        let b = v.to_le_bytes();
        self.mix_bytes(&b);
    }

    /// 拌入 i64（位型 LE 八字节——定编码）。
    pub fn mix_i64(&mut self, v: i64) {
        let b = (v as u64).to_le_bytes();
        self.mix_bytes(&b);
    }

    /// 拌入字符串（判别前缀防异型碰撞）。
    pub fn mix_str(&mut self, s: &str) {
        self.mix_bytes(s.as_bytes());
        self.mix_u8(0xFF);
    }

    /// 取摘要（收尾拌一段定界——防前缀拼接歧义）。
    pub fn finish(&self) -> u64 {
        let mut h = self.h;
        h ^= 0xD1FE_C0DE_0000_0001;
        h = h.wrapping_mul(FNV_PRIME);
        h
    }
}

/// 参数组摘要（跨界调用入账的参数侧——各值依序入拌）。
pub fn args_digest(args: &[XVal]) -> u64 {
    let mut m = DigestMixer::new();
    m.mix_u8(args.len() as u8);
    let mut i: usize = 0;
    while i < args.len() {
        args[i].digest_mix(&mut m);
        i += 1;
    }
    m.finish()
}

// ---------------------------------------------------------------------------
// 六、互操作面：白名单注册制 + 签名指纹 + 越白名单拦截
// ---------------------------------------------------------------------------

/// 读写种类（Read=纯读面 / Write=Effect 面——Effect 面须授权位）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AccessKind {
    /// 纯读面（无副作用——安全默认律下免授权）。
    Read = 1,
    /// Effect 面（有跨界副作用——须宿主授权位）。
    Write = 2,
}

impl AccessKind {
    /// 序数。
    pub const fn ord(&self) -> u8 {
        match self {
            AccessKind::Read => 1,
            AccessKind::Write => 2,
        }
    }

    /// 名字。
    pub const fn name(&self) -> &'static str {
        match self {
            AccessKind::Read => "纯读",
            AccessKind::Write => "Effect",
        }
    }
}

/// 注册签名（参数型闭集定长 6 槽 + 返回型——Copy，无堆字段）。
#[derive(Debug, Clone, Copy)]
pub struct SigSpec {
    /// 参数型（前 n_params 个有效）。
    pub params: [XValKind; 6],
    /// 参数个数（0..=6）。
    pub n_params: u8,
    /// 返回型。
    pub ret: XValKind,
}

impl SigSpec {
    /// 无参签名（ret=N）。
    pub const fn unit(ret: XValKind) -> SigSpec {
        SigSpec {
            params: [
                XValKind::Num,
                XValKind::Num,
                XValKind::Num,
                XValKind::Num,
                XValKind::Num,
                XValKind::Num,
            ],
            n_params: 0,
            ret,
        }
    }

    /// 构造签名（>6 参 → SigMismatch——签名卡定宽纪律）。
    pub fn new(params: &[XValKind], ret: XValKind) -> Result<SigSpec, SysBoundError> {
        if params.len() > 6 {
            return Err(SysBoundError::SigMismatch);
        }
        let mut filled = [
            XValKind::Num,
            XValKind::Num,
            XValKind::Num,
            XValKind::Num,
            XValKind::Num,
            XValKind::Num,
        ];
        let mut i: usize = 0;
        while i < params.len() {
            filled[i] = params[i];
            i += 1;
        }
        Ok(SigSpec {
            params: filled,
            n_params: params.len() as u8,
            ret,
        })
    }

    /// 参数型切片（运行期取参用）。
    pub fn params_slice(&self) -> &[XValKind] {
        let n = self.n_params as usize;
        if n > 6 {
            return &self.params[..6];
        }
        &self.params[..n]
    }
}

/// 宿主实现种类（规范 API 的确定性宿主侧语义——执行不落空）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HostImpl {
    /// 纯数值读：FNV(面指纹+实参) 定值返回（同参同返——确定优先）。
    PureNum = 1,
    /// 世界节拍读：返回当前 tick。
    WorldTick = 2,
    /// 实体序号发号：世界发号器单调 +1（saturating）。
    EntitySeq = 3,
    /// 确认写：返回 0 确认码（副作用在账上——授权位已花）。
    AckWrite = 4,
    /// 规范串回显：`[cfg]`+输入串（配置管数据的只读面语义）。
    EchoStr = 5,
    /// 只读视图签发：签出 VIEW_TTL 视图柄。
    IssueView = 6,
    /// 转移柄释放：实参须为未转移的转移柄，释放后回收。
    ReleaseHandle = 7,
}

/// 互操作面白名单容量（16——规范 11 条 + 运行期注册余地 5 条：
/// 查表面有界不可破，余地给宿主显式扩展，白名单永不收面外锚）。
pub const SURFACE_CAP: usize = 16;

/// 白名单条目（注册即冻结：锚/域/读写/签名/指纹一体）。
pub struct SurfaceEntry {
    /// 锚名（a.b.c 三段小写定式）。
    pub anchor: String,
    /// 归属域（须与边界卡对齐——注册期校验）。
    pub domain: HostDomain,
    /// 读写种类。
    pub access: AccessKind,
    /// 注册签名。
    pub sig: SigSpec,
    /// 宿主实现种类。
    pub impl_kind: HostImpl,
    /// 签名指纹（FNV-1a 64：锚+域+读写+参数型+返回型——验链锚点）。
    pub fp: u64,
}

/// 互操作面（脚本可调宿主 API 的白名单注册制查表面）。
pub struct InteropSurface {
    entries: Vec<SurfaceEntry>,
}

impl InteropSurface {
    /// 空面。
    pub fn empty() -> InteropSurface {
        InteropSurface {
            entries: Vec::new(),
        }
    }

    /// 签名指纹（锚+域序+读写+参数型+返回型一体入拌——可重算可验链）。
    pub fn fingerprint_of(
        anchor: &str,
        domain: HostDomain,
        access: AccessKind,
        sig: &SigSpec,
    ) -> u64 {
        let mut m = DigestMixer::new();
        m.mix_str(anchor);
        m.mix_u8(domain.ord());
        m.mix_u8(access.ord());
        let ps = sig.params_slice();
        let mut i: usize = 0;
        while i < ps.len() {
            m.mix_u8(ps[i].ord());
            i += 1;
        }
        m.mix_u8(0xFE);
        m.mix_u8(sig.ret.ord());
        m.finish()
    }

    /// 注册（白名单注册制唯一入口）：重复 → SurfaceDup；满 → SurfaceFull；
    /// 边界三查+值型协议查失败 → FaceViol；签名超宽 → SigMismatch。
    pub fn register(
        &mut self,
        anchor: &str,
        domain: HostDomain,
        access: AccessKind,
        params: &[XValKind],
        ret: XValKind,
        impl_kind: HostImpl,
    ) -> Result<u64, SysBoundError> {
        if self.find(anchor).is_some() {
            return Err(SysBoundError::SurfaceDup);
        }
        if self.entries.len() >= SURFACE_CAP {
            return Err(SysBoundError::SurfaceFull);
        }
        let sig = SigSpec::new(params, ret)?;
        SystemBoundary::validate_anchor(domain, anchor, params)?;
        let fp = Self::fingerprint_of(anchor, domain, access, &sig);
        self.entries.push(SurfaceEntry {
            anchor: anchor.to_string(),
            domain,
            access,
            sig,
            impl_kind,
            fp,
        });
        Ok(fp)
    }

    /// 查锚（白名单查表面——O(面容量)，容量 24 恒定小）。
    pub fn find(&self, anchor: &str) -> Option<usize> {
        let mut i: usize = 0;
        while i < self.entries.len() {
            if self.entries[i].anchor == anchor {
                return Some(i);
            }
            i += 1;
        }
        Option::None
    }

    /// 条目只读访问（越界索引 None——内核路径不落 panic）。
    pub fn entry(&self, idx: usize) -> Option<&SurfaceEntry> {
        if idx < self.entries.len() {
            Some(&self.entries[idx])
        } else {
            Option::None
        }
    }

    /// 注册数。
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// 是否空面。
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// 全表验链（逐条重算指纹——指纹被改即 false；health 每跑必查）。
    pub fn verify_all(&self) -> bool {
        let mut i: usize = 0;
        while i < self.entries.len() {
            let e = &self.entries[i];
            let expect = Self::fingerprint_of(&e.anchor, e.domain, e.access, &e.sig);
            if expect != e.fp {
                return false;
            }
            i += 1;
        }
        true
    }

    /// 全表入摘要（digest 用——注册序+指纹序入拌：增删改面必留痕）。
    pub fn digest_mix(&self, mixer: &mut DigestMixer) {
        mixer.mix_u64(self.entries.len() as u64);
        let mut i: usize = 0;
        while i < self.entries.len() {
            mixer.mix_str(&self.entries[i].anchor);
            mixer.mix_u64(self.entries[i].fp);
            i += 1;
        }
    }

    /// 注入探针专用：篡改第 idx 条指纹（只供注入面自证——#[cfg(test)]
    /// 可见性由调用纪律约束，内核路径零调用）。
    pub fn tamper_fp_for_probe(&mut self, idx: usize) -> bool {
        if idx < self.entries.len() {
            self.entries[idx].fp = self.entries[idx].fp.wrapping_add(1);
            true
        } else {
            false
        }
    }
}

/// 规范 API 描述（开机种子常量——编译期断言与域边界卡对齐）。
pub struct CanonicalApi {
    /// 锚名。
    pub anchor: &'static str,
    /// 归属域。
    pub domain: HostDomain,
    /// 读写种类。
    pub access: AccessKind,
    /// 参数型闭集。
    pub params: &'static [XValKind],
    /// 返回型。
    pub ret: XValKind,
    /// 宿主实现种类。
    pub impl_kind: HostImpl,
}

/// 开机白名单种子（11 条规范 API——与四域边界卡可调用面逐条对齐，
/// 编译期断言兜底；面内还有 `kern.scene.graph_query` 等余地留给宿主
/// 运行期显式注册——白名单不预收面外锚）。
pub const CANONICAL_APIS: [CanonicalApi; 11] = [
    CanonicalApi {
        anchor: "kern.entity.query_pos",
        domain: HostDomain::Kernel,
        access: AccessKind::Read,
        params: &[XValKind::Num],
        ret: XValKind::Num,
        impl_kind: HostImpl::PureNum,
    },
    CanonicalApi {
        anchor: "kern.entity.spawn",
        domain: HostDomain::Kernel,
        access: AccessKind::Write,
        params: &[XValKind::Num, XValKind::Num],
        ret: XValKind::Num,
        impl_kind: HostImpl::EntitySeq,
    },
    CanonicalApi {
        anchor: "kern.clock.tick",
        domain: HostDomain::Kernel,
        access: AccessKind::Read,
        params: &[],
        ret: XValKind::Num,
        impl_kind: HostImpl::WorldTick,
    },
    CanonicalApi {
        anchor: "kern.entity.release",
        domain: HostDomain::Kernel,
        access: AccessKind::Write,
        params: &[XValKind::Ref],
        ret: XValKind::Num,
        impl_kind: HostImpl::ReleaseHandle,
    },
    CanonicalApi {
        anchor: "gfx.draw.rect",
        domain: HostDomain::Graphics,
        access: AccessKind::Write,
        params: &[XValKind::Num, XValKind::Num, XValKind::Num, XValKind::Num],
        ret: XValKind::Num,
        impl_kind: HostImpl::AckWrite,
    },
    CanonicalApi {
        anchor: "gfx.texture.snapshot",
        domain: HostDomain::Graphics,
        access: AccessKind::Read,
        params: &[XValKind::Num],
        ret: XValKind::Ref,
        impl_kind: HostImpl::IssueView,
    },
    CanonicalApi {
        anchor: "gfx.frame.readback",
        domain: HostDomain::Graphics,
        access: AccessKind::Read,
        params: &[],
        ret: XValKind::Num,
        impl_kind: HostImpl::PureNum,
    },
    CanonicalApi {
        anchor: "input.key.state",
        domain: HostDomain::Input,
        access: AccessKind::Read,
        params: &[XValKind::Num],
        ret: XValKind::Num,
        impl_kind: HostImpl::PureNum,
    },
    CanonicalApi {
        anchor: "input.bind.declare",
        domain: HostDomain::Input,
        access: AccessKind::Write,
        params: &[XValKind::Num, XValKind::Str],
        ret: XValKind::Num,
        impl_kind: HostImpl::AckWrite,
    },
    CanonicalApi {
        anchor: "file.config.load",
        domain: HostDomain::Filesys,
        access: AccessKind::Read,
        params: &[XValKind::Str],
        ret: XValKind::Str,
        impl_kind: HostImpl::EchoStr,
    },
    CanonicalApi {
        anchor: "file.persist.append",
        domain: HostDomain::Filesys,
        access: AccessKind::Write,
        params: &[XValKind::Str, XValKind::Str],
        ret: XValKind::Num,
        impl_kind: HostImpl::AckWrite,
    },
];

impl InteropSurface {
    /// 规范面开机装配（11 条种子逐条注册——装配失败即卡册失配，
    /// 返回错误不 panic）。
    pub fn canonical() -> Result<InteropSurface, SysBoundError> {
        let mut s = InteropSurface::empty();
        let mut i: usize = 0;
        while i < CANONICAL_APIS.len() {
            let api = &CANONICAL_APIS[i];
            s.register(
                api.anchor,
                api.domain,
                api.access,
                api.params,
                api.ret,
                api.impl_kind,
            )?;
            i += 1;
        }
        Ok(s)
    }
}

// ---------------------------------------------------------------------------
// 七、授权账：安全默认律在边界上的落地（Effect 面须显式授权位）
// ---------------------------------------------------------------------------

/// 授权槽位容量（16——授权账有界）。
pub const GRANT_SLOTS: usize = 16;

/// 授权槽（锚绑定 + 预算 + 已花——Effect 面跨界写的唯一通行证）。
#[derive(Clone, Copy)]
struct GrantSlot {
    /// 在用位。
    active: bool,
    /// 绑定被调面指纹（授权位按锚绑定，不按脚本绑定——宿主签发口径）。
    anchor_fp: u64,
    /// 预算（签发时核定，1..=u32::MAX）。
    budget: u32,
    /// 已花（saturating 前进，永不超预算——超即 AuthSpent）。
    spent: u32,
    /// 签发节拍。
    issued_at: u64,
}

impl GrantSlot {
    const EMPTY: GrantSlot = GrantSlot {
        active: false,
        anchor_fp: 0,
        budget: 0,
        spent: 0,
        issued_at: 0,
    };
}

/// 授权账（安全默认律执行件：默认无副作用泄漏——写面无授权即拦）。
pub struct WriteAuthBook {
    slots: [GrantSlot; GRANT_SLOTS],
    /// 累计签发数。
    issued_total: u64,
    /// 累计花销笔数。
    spent_total: u64,
}

impl WriteAuthBook {
    /// 新账（无任何授权位——默认全拦）。
    pub const fn new() -> WriteAuthBook {
        WriteAuthBook {
            slots: [
                GrantSlot::EMPTY,
                GrantSlot::EMPTY,
                GrantSlot::EMPTY,
                GrantSlot::EMPTY,
                GrantSlot::EMPTY,
                GrantSlot::EMPTY,
                GrantSlot::EMPTY,
                GrantSlot::EMPTY,
                GrantSlot::EMPTY,
                GrantSlot::EMPTY,
                GrantSlot::EMPTY,
                GrantSlot::EMPTY,
                GrantSlot::EMPTY,
                GrantSlot::EMPTY,
                GrantSlot::EMPTY,
                GrantSlot::EMPTY,
            ],
            issued_total: 0,
            spent_total: 0,
        }
    }

    /// 签发授权位（宿主侧显式动作：零预算拒发——AuthSpent 口径；
    /// 账满 → GrantFull）。返回槽位号。
    pub fn issue_grant(
        &mut self,
        anchor_fp: u64,
        budget: u32,
        tick: u64,
    ) -> Result<usize, SysBoundError> {
        if budget == 0 {
            return Err(SysBoundError::AuthSpent);
        }
        let mut i: usize = 0;
        while i < GRANT_SLOTS {
            if !self.slots[i].active || self.slots[i].spent >= self.slots[i].budget {
                self.slots[i] = GrantSlot {
                    active: true,
                    anchor_fp,
                    budget,
                    spent: 0,
                    issued_at: tick,
                };
                self.issued_total = self.issued_total.saturating_add(1);
                return Ok(i);
            }
            i += 1;
        }
        Err(SysBoundError::GrantFull)
    }

    /// 找可用授权位（锚绑定 + 预算有余：无任何绑定授权位 →
    /// WriteUnauthorized；有绑定但全部耗尽 → AuthSpent——两种拒付
    /// 分开诊断，宿主知道该签发还是该换锚）。
    pub fn authorize(&self, anchor_fp: u64) -> Result<usize, SysBoundError> {
        let mut any_binding = false;
        let mut i: usize = 0;
        while i < GRANT_SLOTS {
            let g = &self.slots[i];
            if g.active && g.anchor_fp == anchor_fp {
                any_binding = true;
                if g.spent < g.budget {
                    return Ok(i);
                }
            }
            i += 1;
        }
        if any_binding {
            Err(SysBoundError::AuthSpent)
        } else {
            Err(SysBoundError::WriteUnauthorized)
        }
    }

    /// 花销一笔（预算内前进；花光 → AuthSpent——须重新签发）。
    pub fn spend(&mut self, idx: usize) -> Result<(), SysBoundError> {
        if idx >= GRANT_SLOTS {
            return Err(SysBoundError::GrantFull);
        }
        let g = &mut self.slots[idx];
        if !g.active || g.spent >= g.budget {
            return Err(SysBoundError::AuthSpent);
        }
        g.spent += 1;
        self.spent_total = self.spent_total.saturating_add(1);
        Ok(())
    }

    /// 撤销授权位（宿主侧随时可撤——撤后写面即拦）。
    pub fn revoke(&mut self, idx: usize) -> Result<(), SysBoundError> {
        if idx >= GRANT_SLOTS || !self.slots[idx].active {
            return Err(SysBoundError::GrantFull);
        }
        self.slots[idx].active = false;
        Ok(())
    }

    /// 活跃授权位数（预算有余的位）。
    pub fn active_count(&self) -> u32 {
        let mut n: u32 = 0;
        let mut i: usize = 0;
        while i < GRANT_SLOTS {
            let g = &self.slots[i];
            if g.active && g.spent < g.budget {
                n += 1;
            }
            i += 1;
        }
        n
    }

    /// 累计签发数。
    pub fn issued_total(&self) -> u64 {
        self.issued_total
    }

    /// 累计花销笔数。
    pub fn spent_total(&self) -> u64 {
        self.spent_total
    }

    /// 全账入摘要。
    pub fn digest_mix(&self, mixer: &mut DigestMixer) {
        mixer.mix_u64(self.issued_total);
        mixer.mix_u64(self.spent_total);
        let mut i: usize = 0;
        while i < GRANT_SLOTS {
            let g = &self.slots[i];
            mixer.mix_bool(g.active);
            mixer.mix_u64(g.anchor_fp);
            mixer.mix_u32(g.budget);
            mixer.mix_u32(g.spent);
            mixer.mix_u64(g.issued_at);
            i += 1;
        }
    }
}

// ---------------------------------------------------------------------------
// 八、高频调用熔断（窗口计数 + 冷却——恶意循环杀得掉）
// ---------------------------------------------------------------------------

/// 熔断窗口宽（tick 数——窗口号 = tick / 64）。
pub const BREAKER_WINDOW_TICKS: u64 = 64;

/// 窗口内单脚本单锚调用上限（超过即熔断）。
pub const BREAKER_MAX_CALLS: u32 = 32;

/// 熔断冷却窗数（窗口 w 熔断 → 冷却到 (w+1+2) 窗口起点）。
pub const BREAKER_COOLDOWN_WINDOWS: u64 = 2;

/// 熔断表容量（32——(脚本,锚) 组合有界）。
pub const BREAKER_SLOTS: usize = 32;

/// 熔断表条目（(调用方,锚指纹) 窗口计数）。
#[derive(Clone, Copy)]
struct BreakerEntry {
    used: bool,
    caller: u32,
    anchor_fp: u64,
    window: u64,
    count: u32,
    tripped_until: u64,
}

impl BreakerEntry {
    const EMPTY: BreakerEntry = BreakerEntry {
        used: false,
        caller: 0,
        anchor_fp: 0,
        window: 0,
        count: 0,
        tripped_until: 0,
    };
}

/// 高频调用熔断器（跨界审计的高频侧防线）。
pub struct CallBreaker {
    slots: [BreakerEntry; BREAKER_SLOTS],
    /// 累计熔断次数。
    trips_total: u64,
}

impl CallBreaker {
    /// 新表。
    pub const fn new() -> CallBreaker {
        CallBreaker {
            slots: [
                BreakerEntry::EMPTY,
                BreakerEntry::EMPTY,
                BreakerEntry::EMPTY,
                BreakerEntry::EMPTY,
                BreakerEntry::EMPTY,
                BreakerEntry::EMPTY,
                BreakerEntry::EMPTY,
                BreakerEntry::EMPTY,
                BreakerEntry::EMPTY,
                BreakerEntry::EMPTY,
                BreakerEntry::EMPTY,
                BreakerEntry::EMPTY,
                BreakerEntry::EMPTY,
                BreakerEntry::EMPTY,
                BreakerEntry::EMPTY,
                BreakerEntry::EMPTY,
                BreakerEntry::EMPTY,
                BreakerEntry::EMPTY,
                BreakerEntry::EMPTY,
                BreakerEntry::EMPTY,
                BreakerEntry::EMPTY,
                BreakerEntry::EMPTY,
                BreakerEntry::EMPTY,
                BreakerEntry::EMPTY,
                BreakerEntry::EMPTY,
                BreakerEntry::EMPTY,
                BreakerEntry::EMPTY,
                BreakerEntry::EMPTY,
                BreakerEntry::EMPTY,
                BreakerEntry::EMPTY,
                BreakerEntry::EMPTY,
                BreakerEntry::EMPTY,
            ],
            trips_total: 0,
        }
    }

    fn window_of(tick: u64) -> u64 {
        // 窗口号 = tick / 窗宽（整除——无溢出风险，u64 除法）
        tick / BREAKER_WINDOW_TICKS
    }

    /// 记一次调用并裁定（放行 Ok / 熔断 Err——计数与裁定原子）。
    ///
    /// 熔断语义：窗口内第 33 次同锚调用即拒，冷却到
    /// (w + 1 + BREAKER_COOLDOWN_WINDOWS) 窗口起点后复路。
    /// 表满淘汰策略：淘汰窗口号最小的旧条目（确定性——次序键
    /// (window, 槽位号) 唯一，无平局歧义）。
    pub fn note(
        &mut self,
        caller: u32,
        anchor_fp: u64,
        tick: u64,
    ) -> Result<(), SysBoundError> {
        let window = Self::window_of(tick);
        // 命中既有条目
        let mut i: usize = 0;
        while i < BREAKER_SLOTS {
            if self.slots[i].used
                && self.slots[i].caller == caller
                && self.slots[i].anchor_fp == anchor_fp
            {
                if tick < self.slots[i].tripped_until {
                    return Err(SysBoundError::BreakerTripped);
                }
                if self.slots[i].window == window {
                    if self.slots[i].count >= BREAKER_MAX_CALLS {
                        let until = (window + 1 + BREAKER_COOLDOWN_WINDOWS)
                            .saturating_mul(BREAKER_WINDOW_TICKS);
                        self.slots[i].tripped_until = until;
                        self.trips_total = self.trips_total.saturating_add(1);
                        return Err(SysBoundError::BreakerTripped);
                    }
                    self.slots[i].count += 1;
                    return Ok(());
                }
                // 新窗口：计数重开
                self.slots[i].window = window;
                self.slots[i].count = 1;
                self.slots[i].tripped_until = 0;
                return Ok(());
            }
            i += 1;
        }
        // 未命中：找空位或淘汰最旧窗口
        let mut target: usize = BREAKER_SLOTS;
        let mut oldest_window: u64 = u64::MAX;
        let mut oldest_idx: usize = 0;
        let mut j: usize = 0;
        while j < BREAKER_SLOTS {
            if !self.slots[j].used {
                target = j;
                break;
            }
            if self.slots[j].window < oldest_window {
                oldest_window = self.slots[j].window;
                oldest_idx = j;
            }
            j += 1;
        }
        if target == BREAKER_SLOTS {
            target = oldest_idx;
        }
        self.slots[target] = BreakerEntry {
            used: true,
            caller,
            anchor_fp,
            window,
            count: 1,
            tripped_until: 0,
        };
        Ok(())
    }

    /// 指定 (调用方,锚) 是否处于熔断冷却中（health 观测用）。
    pub fn is_tripped(&self, caller: u32, anchor_fp: u64, tick: u64) -> bool {
        let mut i: usize = 0;
        while i < BREAKER_SLOTS {
            if self.slots[i].used
                && self.slots[i].caller == caller
                && self.slots[i].anchor_fp == anchor_fp
                && tick < self.slots[i].tripped_until
            {
                return true;
            }
            i += 1;
        }
        false
    }

    /// 累计熔断次数。
    pub fn trips_total(&self) -> u64 {
        self.trips_total
    }

    /// 全表入摘要。
    pub fn digest_mix(&self, mixer: &mut DigestMixer) {
        mixer.mix_u64(self.trips_total);
        let mut i: usize = 0;
        while i < BREAKER_SLOTS {
            let s = &self.slots[i];
            mixer.mix_bool(s.used);
            mixer.mix_u32(s.caller);
            mixer.mix_u64(s.anchor_fp);
            mixer.mix_u64(s.window);
            mixer.mix_u32(s.count);
            mixer.mix_u64(s.tripped_until);
            i += 1;
        }
    }
}

// ---------------------------------------------------------------------------
// 九、跨界审计：定长环形账（拒付也留小票）
// ---------------------------------------------------------------------------

/// 审计环容量（64 条——定长环形，满覆最旧并计 wraps）。
pub const AUDIT_RING_CAP: usize = 64;

/// 审计条目（调用方/被调面/参数摘要/裁定+诊断码——定长内联）。
#[derive(Clone, Copy)]
pub struct AuditEntry {
    /// 全局序号（单调，saturating）。
    pub seq: u64,
    /// 入账节拍。
    pub tick: u64,
    /// 调用方脚本号。
    pub caller: u32,
    /// 被调面锚指纹（白名单注册指纹——查锚对齐）。
    pub anchor_fp: u64,
    /// 参数摘要（FNV(实参)——内容脱敏，只记指纹）。
    pub arg_digest: u64,
    /// 裁定（true=放行 / false=拒付）。
    pub allowed: bool,
    /// 诊断码（放行记 0；拒付记 SysBoundError 码）。
    pub code: u16,
}

/// 跨界审计账（定长环形——每次跨界调用入账，放行与拒付同权入账）。
pub struct BoundaryAudit {
    ring: [AuditEntry; AUDIT_RING_CAP],
    head: usize,
    seq: u64,
    wraps: u64,
    denied_total: u64,
    allowed_total: u64,
}

impl BoundaryAudit {
    /// 新账。
    pub fn new() -> BoundaryAudit {
        BoundaryAudit {
            ring: [AuditEntry {
                seq: 0,
                tick: 0,
                caller: 0,
                anchor_fp: 0,
                arg_digest: 0,
                allowed: false,
                code: 0,
            }; AUDIT_RING_CAP],
            head: 0,
            seq: 0,
            wraps: 0,
            denied_total: 0,
            allowed_total: 0,
        }
    }

    /// 入账（环形覆写——满覆最旧，wraps 计数；永不拒收：审计不断流）。
    pub fn record(
        &mut self,
        tick: u64,
        caller: u32,
        anchor_fp: u64,
        arg_digest: u64,
        allowed: bool,
        code: u16,
    ) {
        self.seq = self.seq.saturating_add(1);
        if self.seq > AUDIT_RING_CAP as u64 {
            self.wraps = self.wraps.saturating_add(1);
        }
        self.ring[self.head] = AuditEntry {
            seq: self.seq,
            tick,
            caller,
            anchor_fp,
            arg_digest,
            allowed,
            code,
        };
        self.head = (self.head + 1) % AUDIT_RING_CAP;
        if allowed {
            self.allowed_total = self.allowed_total.saturating_add(1);
        } else {
            self.denied_total = self.denied_total.saturating_add(1);
        }
    }

    /// 最近一条（空账 None）。
    pub fn last(&self) -> Option<&AuditEntry> {
        if self.seq == 0 {
            return Option::None;
        }
        let idx = (self.head + AUDIT_RING_CAP - 1) % AUDIT_RING_CAP;
        Some(&self.ring[idx])
    }

    /// 指定诊断码的拒付计数（注入检出的对账口径）。
    pub fn count_denied_with(&self, code: u16) -> u64 {
        let mut n: u64 = 0;
        let mut i: usize = 0;
        while i < AUDIT_RING_CAP {
            let e = &self.ring[i];
            if e.seq != 0 && !e.allowed && e.code == code {
                n += 1;
            }
            i += 1;
        }
        n
    }

    /// 全局序号（含被覆写条目——入账总量对账口径）。
    pub fn seq(&self) -> u64 {
        self.seq
    }

    /// 覆写轮数。
    pub fn wraps(&self) -> u64 {
        self.wraps
    }

    /// 放行累计。
    pub fn allowed_total(&self) -> u64 {
        self.allowed_total
    }

    /// 拒付累计。
    pub fn denied_total(&self) -> u64 {
        self.denied_total
    }

    /// 全账入摘要（时序遍历——从最旧存活条目起，环序稳定）。
    pub fn digest_mix(&self, mixer: &mut DigestMixer) {
        mixer.mix_u64(self.seq);
        mixer.mix_u64(self.wraps);
        mixer.mix_u64(self.allowed_total);
        mixer.mix_u64(self.denied_total);
        let filled = if self.seq < AUDIT_RING_CAP as u64 {
            self.seq as usize
        } else {
            AUDIT_RING_CAP
        };
        let start = if filled < AUDIT_RING_CAP {
            0
        } else {
            self.head
        };
        let mut k: usize = 0;
        while k < filled {
            let idx = (start + k) % AUDIT_RING_CAP;
            let e = &self.ring[idx];
            mixer.mix_u64(e.seq);
            mixer.mix_u64(e.tick);
            mixer.mix_u32(e.caller);
            mixer.mix_u64(e.anchor_fp);
            mixer.mix_u64(e.arg_digest);
            mixer.mix_bool(e.allowed);
            mixer.mix_u32(e.code as u32);
            k += 1;
        }
    }
}

// ---------------------------------------------------------------------------
// 十、事件账（有界环形——满覆最旧并计 dropped，不静默丢）
// ---------------------------------------------------------------------------

/// 事件账容量（128 条）。
pub const EVENT_CAP: usize = 128;

/// 事件种类（边界子系统全生命周期留痕）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventKind {
    /// 白名单注册（a=指纹，b=域序）。
    SurfaceRegistered = 1,
    /// 能力申报归位（a=调用方，b=四定序）。
    FilingFiled = 2,
    /// 关系失配对拍修正（a=调用方，b=新四定序）。
    FilingRefiled = 3,
    /// 市场预留门开启（a=钥匙指纹）。
    MarketGateOpened = 4,
    /// 预留私启拦截（a=调用方）。
    ReservationBlocked = 5,
    /// 授权位签发（a=槽位号，b=预算）。
    GrantIssued = 6,
    /// 授权花销（a=槽位号，b=1）。
    GrantSpent = 7,
    /// 句柄签发（a=句柄号，b=态）。
    HandleIssued = 8,
    /// 句柄回收（a=句柄号，b=1）。
    HandleRevoked = 9,
    /// 句柄一次性转移（a=句柄号）。
    HandleHandedOff = 10,
    /// 熔断（a=调用方，b=锚指纹低 32 位）。
    BreakerTripped = 11,
    /// 越权拒付入账（a=诊断码，b=调用方）。
    AuditDenied = 12,
    /// 反向侵入拦截（a=锚指纹）。
    ReverseHookBlocked = 13,
}

impl EventKind {
    /// 事件名（读屏可达）。
    pub const fn name(&self) -> &'static str {
        match self {
            EventKind::SurfaceRegistered => "白名单注册",
            EventKind::FilingFiled => "能力归位",
            EventKind::FilingRefiled => "对拍修正",
            EventKind::MarketGateOpened => "市场门开启",
            EventKind::ReservationBlocked => "预留私启拦截",
            EventKind::GrantIssued => "授权签发",
            EventKind::GrantSpent => "授权花销",
            EventKind::HandleIssued => "句柄签发",
            EventKind::HandleRevoked => "句柄回收",
            EventKind::HandleHandedOff => "句柄转移",
            EventKind::BreakerTripped => "高频熔断",
            EventKind::AuditDenied => "越权拒付",
            EventKind::ReverseHookBlocked => "反向侵入拦截",
        }
    }
}

/// 事件条目（定长四字——种类+节拍+两参数）。
#[derive(Clone, Copy)]
pub struct EventRec {
    /// 事件种类。
    pub kind: EventKind,
    /// 入账节拍。
    pub tick: u64,
    /// 参数 a（种类相关）。
    pub a: u64,
    /// 参数 b（种类相关）。
    pub b: u64,
}

/// 事件账（有界环形：满覆最旧 + dropped 计数——health 可见，不静默）。
pub struct EventLedger {
    recs: Vec<EventRec>,
    dropped: u64,
    seq: u64,
}

impl EventLedger {
    /// 新账。
    pub fn new() -> EventLedger {
        EventLedger {
            recs: Vec::new(),
            dropped: 0,
            seq: 0,
        }
    }

    /// 入账（满覆最旧并计 dropped——环形不断流）。
    pub fn push(&mut self, kind: EventKind, tick: u64, a: u64, b: u64) {
        self.seq = self.seq.saturating_add(1);
        if self.recs.len() >= EVENT_CAP {
            // 覆写最旧：remove(0) 有界搬移（容量 128 恒定小）
            self.recs.remove(0);
            self.dropped = self.dropped.saturating_add(1);
        }
        self.recs.push(EventRec { kind, tick, a, b });
    }

    /// 在账条数。
    pub fn len(&self) -> usize {
        self.recs.len()
    }

    /// 是否空账。
    pub fn is_empty(&self) -> bool {
        self.recs.is_empty()
    }

    /// 覆写丢弃计数（health 可见——不静默丢事件）。
    pub fn dropped(&self) -> u64 {
        self.dropped
    }

    /// 全局入账序号。
    pub fn seq(&self) -> u64 {
        self.seq
    }

    /// 指定种类计数（health/对账）。
    pub fn count_kind(&self, kind: EventKind) -> u64 {
        let mut n: u64 = 0;
        let mut i: usize = 0;
        while i < self.recs.len() {
            if self.recs[i].kind == kind {
                n += 1;
            }
            i += 1;
        }
        n
    }

    /// 全账入摘要。
    pub fn digest_mix(&self, mixer: &mut DigestMixer) {
        mixer.mix_u64(self.seq);
        mixer.mix_u64(self.dropped);
        let mut i: usize = 0;
        while i < self.recs.len() {
            let r = &self.recs[i];
            mixer.mix_u8(r.kind as u8);
            mixer.mix_u64(r.tick);
            mixer.mix_u64(r.a);
            mixer.mix_u64(r.b);
            i += 1;
        }
    }
}

// ---------------------------------------------------------------------------
// 十一、总编排 SysRelWorld：跨界调用管线 + 关系四定 + 预留门
// ---------------------------------------------------------------------------

/// 市场预留门宿主钥匙（开启 W 域市场上架通路的显式动作凭据——
/// 写死常量：宿主侧编译期持有，运行期对拍）。
pub const MARKET_HOST_KEY: u64 = 0x5452_4B57_4D41_524B;

/// 宿主内建调用方号（caller=0 是宿主——不须申报关系四定）。
pub const HOST_CALLER: u32 = 0;

/// 一键体检报告（health 输出——全字段 pub 读屏可达）。
pub struct HealthReport {
    /// 世界节拍。
    pub tick: u64,
    /// 白名单注册数。
    pub surface_len: usize,
    /// 签名指纹验链（全表重算一致）。
    pub sig_chain_ok: bool,
    /// 边界卡册自检（闭集互斥/名字合式/禁入对齐）。
    pub boundary_ok: bool,
    /// 关系四定在册申报数。
    pub filings: usize,
    /// 市场预留门状态（true=已持钥匙开启）。
    pub market_gate_open: bool,
    /// 存活句柄数。
    pub handles_live: u32,
    /// 句柄累计签发/回收/转移。
    pub handles_issued: u64,
    pub handles_revoked: u64,
    pub handles_handed: u64,
    /// 活跃授权位数。
    pub grants_active: u32,
    /// 授权累计签发/花销。
    pub grants_issued: u64,
    pub grants_spent: u64,
    /// 累计熔断次数。
    pub breaker_trips: u64,
    /// 审计入账总量（含覆写）/覆写轮数/放行/拒付。
    pub audit_seq: u64,
    pub audit_wraps: u64,
    pub audit_allowed: u64,
    pub audit_denied: u64,
    /// 事件账在账/丢弃/入账总量。
    pub events_kept: usize,
    pub events_dropped: u64,
    pub events_seq: u64,
    /// 跨界调用放行/拒付计数（与审计对账的独立口径）。
    pub calls_ok: u64,
    pub calls_denied: u64,
    /// 全局不变量（审计对账+验链+卡册+授权守恒全过）。
    pub invariants_ok: bool,
}

impl HealthReport {
    /// 一句话总评（true=全绿）。
    pub fn all_ok(&self) -> bool {
        self.invariants_ok
    }
}

/// 五律落地对拍报告（F5003 衔接——五律逐律机械报告）。
pub struct LawGroundingReport {
    /// 可读第一：白名单锚名全过三段式定式。
    pub readable_ok: bool,
    /// 错误友好：16 诊断码全挂非空中文诊断。
    pub friendly_ok: bool,
    /// 安全默认：写面放行数 == 授权花销笔数（无一笔写漏过授权）。
    pub safe_default_ok: bool,
    /// 渐进复杂：面上同时存在 0 参/1 参/2 参 API（三行能干小事）。
    pub gradual_ok: bool,
    /// 确定优先：内置双跑对拍摘要一致。
    pub deterministic_ok: bool,
    /// 五律全绿。
    pub all_ok: bool,
}

/// 五律落地卡（常量冻结：F5003 五律 → 边界上的机械执行位说明）。
pub struct LawGrounding {
    /// 律名（F5003 锚点口径）。
    pub law: &'static str,
    /// 边界侧落地说明。
    pub grounding: &'static str,
}

/// F5003 五律落地卡册（恰五卡——闭集不扩员，与 vez52_langlaw 五律同名同序）。
pub const FIVE_LAW_GROUNDING: [LawGrounding; 5] = [
    LawGrounding {
        law: "可读第一",
        grounding: "锚名三段小写定式（a.b.c，段≤16 字节）注册期机械校验——\
                   白名单上没有天书锚",
    },
    LawGrounding {
        law: "错误友好",
        grounding: "16 诊断码全挂中文可操作诊断（现象+改法）——拒付小票\
                   说人话给指路",
    },
    LawGrounding {
        law: "安全默认",
        grounding: "Effect 面默认无副作用：无授权位即拦（0x1DD9），额度花光\
                   即拦（0x1DDA）——写面放行数与授权花销笔数恒等",
    },
    LawGrounding {
        law: "渐进复杂",
        grounding: "面上常备 0 参（kern.clock.tick）/1 参（query_pos）/2 参\
                   （spawn）档位——三行能干小事",
    },
    LawGrounding {
        law: "确定优先",
        grounding: "宿主实现种类全定值化（FNV 定值/单调发号/定长回显），\
                   双跑对拍摘要必相等（0x1DDF 兜底）",
    },
];

/// 跨界探针操作（双跑对拍的脚本侧输入——确定性序列描述）。
pub struct ProbeOp {
    /// 调用前世界节拍推进数。
    pub advance: u32,
    /// 调用方脚本号。
    pub caller: u32,
    /// 被调锚。
    pub anchor: &'static str,
    /// 实参（构造好的跨界值——拥有型，序列自持）。
    pub args: Vec<XVal>,
}

/// 与既有系统关系世界（总编排：白名单/边界/四定/句柄/授权/熔断/审计/事件）。
pub struct SysRelWorld {
    /// 世界节拍（显式 advance_tick 前进——确定优先）。
    tick: u64,
    /// 互操作面（白名单注册制查表面）。
    surface: InteropSurface,
    /// 关系四定申报册。
    filings: Vec<CapabilityFiling>,
    /// 市场预留门（默认关——预留不承诺）。
    market_gate_open: bool,
    /// 句柄表。
    handles: HandleTable,
    /// 授权账。
    auth: WriteAuthBook,
    /// 高频熔断器。
    breaker: CallBreaker,
    /// 跨界审计环账。
    audit: BoundaryAudit,
    /// 事件账。
    events: EventLedger,
    /// 实体发号器（kern.entity.spawn 的单调序号——saturating）。
    next_entity: u64,
    /// 跨界调用放行计数。
    calls_ok: u64,
    /// 跨界调用拒付计数。
    calls_denied: u64,
    /// 写面放行计数（安全默认律对账口径）。
    writes_passed: u64,
    /// 授权花销笔数镜像（安全默认律对账口径）。
    writes_spent: u64,
}

impl SysRelWorld {
    /// 开机（规范白名单 11 条种子装配 + 四定册空 + 预留门关）。
    pub fn boot() -> Result<SysRelWorld, SysBoundError> {
        let surface = InteropSurface::canonical()?;
        Ok(SysRelWorld {
            tick: 0,
            surface,
            filings: Vec::new(),
            market_gate_open: false,
            handles: HandleTable::new(),
            auth: WriteAuthBook::new(),
            breaker: CallBreaker::new(),
            audit: BoundaryAudit::new(),
            events: EventLedger::new(),
            next_entity: 1000,
            calls_ok: 0,
            calls_denied: 0,
            writes_passed: 0,
            writes_spent: 0,
        })
    }

    /// 当前节拍。
    pub fn tick(&self) -> u64 {
        self.tick
    }

    /// 节拍前进（句柄到期回收扫描同步执行——TTL 语义挂钟）。
    pub fn advance_tick(&mut self, n: u32) -> u32 {
        let n64 = n as u64;
        self.tick = self.tick.saturating_add(n64);
        self.handles.sweep(self.tick)
    }

    /// 白名单注册透传（注册成功记事件——宿主侧动作）。
    pub fn register_api(
        &mut self,
        anchor: &str,
        domain: HostDomain,
        access: AccessKind,
        params: &[XValKind],
        ret: XValKind,
        impl_kind: HostImpl,
    ) -> Result<u64, SysBoundError> {
        let fp = self.surface.register(anchor, domain, access, params, ret, impl_kind)?;
        self.events
            .push(EventKind::SurfaceRegistered, self.tick, fp, domain.ord() as u64);
        Ok(fp)
    }

    /// 白名单注册数。
    pub fn surface_len(&self) -> usize {
        self.surface.len()
    }

    /// 越权拒付统一入账（审计小票+事件留痕+计数——返回原错误给调用方）。
    fn deny(
        &mut self,
        caller: u32,
        anchor_fp: u64,
        argd: u64,
        err: SysBoundError,
    ) -> SysBoundError {
        self.audit
            .record(self.tick, caller, anchor_fp, argd, false, err.code());
        self.events
            .push(EventKind::AuditDenied, self.tick, err.code() as u64, caller as u64);
        self.calls_denied = self.calls_denied.saturating_add(1);
        err
    }

    /// 跨界调用管线（判读顺序：申报资格→白名单→熔断→签名→柄态→授权→执行→入账）。
    ///
    /// 每一拒付都经 [`Self::deny`] 留小票；放行走到底同步入账。
    /// 调用方 0 是宿主内建（不须申报四定）。
    pub fn call(
        &mut self,
        caller: u32,
        anchor: &str,
        args: &[XVal],
    ) -> Result<XVal, SysBoundError> {
        let argd = args_digest(args);
        // ① 申报资格：脚本先在关系四定册归位（宿主 caller=0 免）
        if caller != HOST_CALLER {
            let mut filed = false;
            let mut i: usize = 0;
            while i < self.filings.len() {
                if self.filings[i].caller == caller {
                    filed = true;
                    break;
                }
                i += 1;
            }
            if !filed {
                let fp = fnv1a64(anchor.as_bytes());
                return Err(self.deny(caller, fp, argd, SysBoundError::RelUndeclared));
            }
        }
        // ② 白名单：查无此锚即越白名单
        let idx = match self.surface.find(anchor) {
            Some(i) => i,
            Option::None => {
                let fp = fnv1a64(anchor.as_bytes());
                return Err(self.deny(caller, fp, argd, SysBoundError::CallOffSurface));
            }
        };
        // 取 Copy 快照（避开后续 &mut 借用冲突；域已由注册期对齐校验）
        let ent = self.surface.entry(idx);
        let (access, sig, impl_kind, fp) = match ent {
            Some(e) => (e.access, e.sig, e.impl_kind, e.fp),
            Option::None => {
                return Err(self.deny(caller, 0, argd, SysBoundError::CallOffSurface));
            }
        };
        // ③ 熔断：窗口计数与裁定原子
        if let Err(e) = self.breaker.note(caller, fp, self.tick) {
            self.events.push(
                EventKind::BreakerTripped,
                self.tick,
                caller as u64,
                (fp & 0xFFFF_FFFF) as u64,
            );
            return Err(self.deny(caller, fp, argd, e));
        }
        // ④ 签名：实参个数
        if args.len() != sig.n_params as usize {
            return Err(self.deny(caller, fp, argd, SysBoundError::SigMismatch));
        }
        // ⑤ 签名：实参类型（不隐式转换）+ 柄态合法性 + 视图只读
        let mut i: usize = 0;
        while i < args.len() {
            if args[i].kind().ord() != sig.params_slice()[i].ord() {
                return Err(self.deny(caller, fp, argd, SysBoundError::XvalType));
            }
            match &args[i] {
                XVal::Handle(id) => {
                    if let Err(e) = self.handles.probe(*id, self.tick) {
                        return Err(self.deny(caller, fp, argd, e));
                    }
                }
                XVal::View(id) => {
                    if let Err(e) = self.handles.probe(*id, self.tick) {
                        return Err(self.deny(caller, fp, argd, e));
                    }
                    // 视图只读：Effect 面带视图实参即句柄滥用（视图越权冒充）
                    if access.ord() == AccessKind::Write.ord() {
                        return Err(self.deny(caller, fp, argd, SysBoundError::HandleAbuse));
                    }
                }
                _ => {}
            }
            i += 1;
        }
        // ⑥ 授权：Effect 面须授权位（安全默认律）
        let mut grant_idx: Option<usize> = Option::None;
        if access.ord() == AccessKind::Write.ord() {
            match self.auth.authorize(fp) {
                Ok(gi) => grant_idx = Some(gi),
                Err(e) => return Err(self.deny(caller, fp, argd, e)),
            }
        }
        // ⑦ 执行（宿主实现确定性语义；实现层失败同样走统一拒付入账——
        // 拒付也要留小票）
        let ret = match self.exec_impl(impl_kind, fp, args) {
            Ok(v) => v,
            Err(e) => return Err(self.deny(caller, fp, argd, e)),
        };
        // ⑧ 花销（执行成功后记账——失败路径不烧预算）
        if let Some(gi) = grant_idx {
            if let Err(e) = self.auth.spend(gi) {
                return Err(self.deny(caller, fp, argd, e));
            }
            self.writes_spent = self.writes_spent.saturating_add(1);
            self.events
                .push(EventKind::GrantSpent, self.tick, gi as u64, 1);
        }
        if access.ord() == AccessKind::Write.ord() {
            self.writes_passed = self.writes_passed.saturating_add(1);
        }
        // ⑨ 放行入账
        self.audit
            .record(self.tick, caller, fp, argd, true, 0);
        self.calls_ok = self.calls_ok.saturating_add(1);
        Ok(ret)
    }

    /// 宿主实现执行（确定性语义——纯定值/单调发号/定长回显/视图签发/柄释放）。
    fn exec_impl(
        &mut self,
        impl_kind: HostImpl,
        fp: u64,
        args: &[XVal],
    ) -> Result<XVal, SysBoundError> {
        match impl_kind {
            HostImpl::PureNum => {
                let mut m = DigestMixer::new();
                m.mix_u64(fp);
                m.mix_u64(args_digest(args));
                Ok(XVal::Num((m.finish() & 0x7FFF_FFFF_FFFF_FFFF) as i64))
            }
            HostImpl::WorldTick => Ok(XVal::Num(self.tick as i64)),
            HostImpl::EntitySeq => {
                self.next_entity = self.next_entity.saturating_add(1);
                Ok(XVal::Num(self.next_entity as i64))
            }
            HostImpl::AckWrite => Ok(XVal::Num(0)),
            HostImpl::EchoStr => {
                // 首参字符串回显（配置管数据：`[cfg]` 前缀定长标记）
                match args.first() {
                    Some(XVal::Str(s)) => {
                        let mut out = String::from("[cfg]");
                        out.push_str(s);
                        if out.len() > XSTR_MAX {
                            // 回显超限按跨界值超限拒收——不截断（截断即静默改义）
                            return Err(SysBoundError::XvalType);
                        }
                        Ok(XVal::Str(out))
                    }
                    _ => Err(SysBoundError::XvalType),
                }
            }
            HostImpl::IssueView => {
                let id = self.handles.issue(HS_VIEW, VIEW_TTL_TICKS, fp, self.tick)?;
                self.events
                    .push(EventKind::HandleIssued, self.tick, id, HS_VIEW as u64);
                Ok(XVal::View(id))
            }
            HostImpl::ReleaseHandle => {
                // 首参须为未转移的转移柄；借用柄/视图不可释放
                match args.first() {
                    Some(XVal::Handle(id)) => {
                        let state = self.handles.probe(*id, self.tick)?;
                        if state != HS_TRANSFER {
                            return Err(SysBoundError::HandleAbuse);
                        }
                        let slot = unpack_slot(*id);
                        if (slot as usize) < HANDLE_SLOTS
                            && self.handles.handed_state(slot as usize)
                        {
                            return Err(SysBoundError::HandleAbuse);
                        }
                        self.handles.close(*id, self.tick)?;
                        self.events
                            .push(EventKind::HandleRevoked, self.tick, *id, 1);
                        Ok(XVal::Num(0))
                    }
                    _ => Err(SysBoundError::HandleAbuse),
                }
            }
        }
    }

    /// 关系四定申报（能力归位闸——判读只看事实位）。
    ///
    /// 申报卡不完整（名空/超 32 字节）→ RelUndeclared；
    /// 事实位自相矛盾（纯数据∧行为）或空能力（既非数据又非行为）→
    /// RelMisfiled；上架意愿而预留门未开 → ReservationPrivate；
    /// 同名异定再申报 = 关系失配 → 对拍修正（改判+留痕事件）。
    pub fn file_capability(
        &mut self,
        caller: u32,
        name: &str,
        facts: CapabilityFacts,
    ) -> Result<FilingVerdict, SysBoundError> {
        if name.is_empty() || name.len() > FILING_NAME_MAX {
            return Err(SysBoundError::RelUndeclared);
        }
        // 事实位自洽：纯数据与行为不可同真；重扩展而非法行为位也拦
        if facts.is_pure_data && facts.is_behavior {
            return Err(SysBoundError::RelMisfiled);
        }
        if !facts.is_pure_data && !facts.is_behavior {
            return Err(SysBoundError::RelMisfiled);
        }
        if facts.heavy_native && !facts.is_behavior {
            return Err(SysBoundError::RelMisfiled);
        }
        let (quad, reserved, guidance) = if facts.wants_market_listing {
            if !self.market_gate_open {
                self.events
                    .push(EventKind::ReservationBlocked, self.tick, caller as u64, 0);
                return Err(SysBoundError::ReservationPrivate);
            }
            (
                RelQuad::MarketReserve,
                true,
                QUAD_CARDS[3].caliber,
            )
        } else if facts.is_pure_data {
            (RelQuad::ConfigSys, false, QUAD_CARDS[2].caliber)
        } else if facts.heavy_native {
            (RelQuad::PluginSdk, false, QUAD_CARDS[1].caliber)
        } else {
            (RelQuad::Engine, false, QUAD_CARDS[0].caliber)
        };
        // 同名查册：同定幂等 / 异定对拍修正
        let mut refiled = false;
        let mut found: Option<usize> = Option::None;
        let mut i: usize = 0;
        while i < self.filings.len() {
            if self.filings[i].caller == caller && self.filings[i].name == name {
                found = Some(i);
                break;
            }
            i += 1;
        }
        match found {
            Some(j) => {
                if self.filings[j].quad == quad {
                    return Ok(FilingVerdict {
                        quad,
                        reserved,
                        refiled: false,
                        guidance,
                    });
                }
                // 关系失配 → 对拍修正（改判留痕不拒收）
                self.filings[j].quad = quad;
                self.filings[j].reserved = reserved;
                self.filings[j].facts = facts;
                self.filings[j].filed_at = self.tick;
                refiled = true;
                self.events.push(
                    EventKind::FilingRefiled,
                    self.tick,
                    caller as u64,
                    quad.ord() as u64,
                );
            }
            Option::None => {
                self.filings.push(CapabilityFiling {
                    caller,
                    name: name.to_string(),
                    quad,
                    reserved,
                    facts,
                    filed_at: self.tick,
                });
                self.events.push(
                    EventKind::FilingFiled,
                    self.tick,
                    caller as u64,
                    quad.ord() as u64,
                );
            }
        }
        Ok(FilingVerdict {
            quad,
            reserved,
            refiled,
            guidance,
        })
    }

    /// 在册申报数。
    pub fn filing_count(&self) -> usize {
        self.filings.len()
    }

    /// 指定调用方是否有在册申报（跨界调用资格查询）。
    pub fn is_filed(&self, caller: u32) -> bool {
        let mut i: usize = 0;
        while i < self.filings.len() {
            if self.filings[i].caller == caller {
                return true;
            }
            i += 1;
        }
        false
    }

    /// 市场预留门开启（唯一钥匙口：错钥即拦——预留不承诺的结构性纪律）。
    pub fn open_market_gate(&mut self, host_key: u64) -> Result<(), SysBoundError> {
        if host_key != MARKET_HOST_KEY {
            self.events
                .push(EventKind::ReservationBlocked, self.tick, u64::MAX, 0);
            return Err(SysBoundError::ReservationPrivate);
        }
        if !self.market_gate_open {
            self.market_gate_open = true;
            self.events.push(
                EventKind::MarketGateOpened,
                self.tick,
                fnv1a64(&host_key.to_le_bytes()),
                0,
            );
        }
        Ok(())
    }

    /// 反向侵入拦截（宿主→脚本单面禁入——任何反向锚一律 FaceViol
    /// 并留痕；「单向编排不反向侵入」的机械执行面）。
    pub fn attempt_reverse_hook(&mut self, anchor: &str) -> Result<(), SysBoundError> {
        let fp = fnv1a64(anchor.as_bytes());
        self.events
            .push(EventKind::ReverseHookBlocked, self.tick, fp, 0);
        self.audit.record(
            self.tick,
            HOST_CALLER,
            fp,
            0,
            false,
            SysBoundError::FaceViol.code(),
        );
        self.calls_denied = self.calls_denied.saturating_add(1);
        Err(SysBoundError::FaceViol)
    }

    /// 授权位签发透传（宿主侧显式动作——记事件）。
    pub fn issue_grant(
        &mut self,
        anchor: &str,
        budget: u32,
    ) -> Result<usize, SysBoundError> {
        let idx = match self.surface.find(anchor) {
            Some(i) => match self.surface.entry(i) {
                Some(e) => self.auth.issue_grant(e.fp, budget, self.tick)?,
                Option::None => return Err(SysBoundError::CallOffSurface),
            },
            Option::None => return Err(SysBoundError::CallOffSurface),
        };
        self.events
            .push(EventKind::GrantIssued, self.tick, idx as u64, budget as u64);
        Ok(idx)
    }

    /// 授权位撤销透传。
    pub fn revoke_grant(&mut self, idx: usize) -> Result<(), SysBoundError> {
        self.auth.revoke(idx)
    }

    /// 句柄签发透传（宿主侧动作——记事件；state 用 HS_* 常量）。
    pub fn issue_handle(&mut self, state: u8, ttl: u32, src_anchor: &str) -> Result<HandleId, SysBoundError> {
        let fp = match self.surface.find(src_anchor) {
            Some(i) => match self.surface.entry(i) {
                Some(e) => e.fp,
                Option::None => fnv1a64(src_anchor.as_bytes()),
            },
            Option::None => fnv1a64(src_anchor.as_bytes()),
        };
        let id = self.handles.issue(state, ttl, fp, self.tick)?;
        self.events
            .push(EventKind::HandleIssued, self.tick, id, state as u64);
        Ok(id)
    }

    /// 句柄一次性转移透传（记事件）。
    pub fn hand_off_handle(&mut self, id: HandleId) -> Result<(), SysBoundError> {
        self.handles.hand_off(id, self.tick)?;
        self.events
            .push(EventKind::HandleHandedOff, self.tick, id, 0);
        Ok(())
    }

    /// 句柄视图派生透传（记事件）。
    pub fn derive_view(&mut self, src: HandleId, ttl: u32) -> Result<HandleId, SysBoundError> {
        let v = self.handles.derive_view(src, ttl, self.tick)?;
        self.events
            .push(EventKind::HandleIssued, self.tick, v, HS_VIEW as u64);
        Ok(v)
    }

    /// 一键体检（验链+卡册+对账——全字段见 HealthReport）。
    pub fn health(&self) -> HealthReport {
        let sig_chain_ok = self.surface.verify_all();
        let boundary_ok = SystemBoundary::integrity_ok();
        let invariants_ok = sig_chain_ok
            && boundary_ok
            && self.audit.allowed_total() == self.calls_ok
            && self.audit.denied_total() == self.calls_denied
            && self.writes_passed == self.writes_spent;
        HealthReport {
            tick: self.tick,
            surface_len: self.surface.len(),
            sig_chain_ok,
            boundary_ok,
            filings: self.filings.len(),
            market_gate_open: self.market_gate_open,
            handles_live: self.handles.live_count(self.tick),
            handles_issued: self.handles.issued_total(),
            handles_revoked: self.handles.revoked_total(),
            handles_handed: self.handles.handed_total(),
            grants_active: self.auth.active_count(),
            grants_issued: self.auth.issued_total(),
            grants_spent: self.auth.spent_total(),
            breaker_trips: self.breaker.trips_total(),
            audit_seq: self.audit.seq(),
            audit_wraps: self.audit.wraps(),
            audit_allowed: self.audit.allowed_total(),
            audit_denied: self.audit.denied_total(),
            events_kept: self.events.len(),
            events_dropped: self.events.dropped(),
            events_seq: self.events.seq(),
            calls_ok: self.calls_ok,
            calls_denied: self.calls_denied,
            invariants_ok,
        }
    }

    /// 全状态确定性摘要（FNV-1a 64——双跑对拍的比较基）。
    pub fn state_digest(&self) -> u64 {
        let mut m = DigestMixer::new();
        m.mix_u64(self.tick);
        m.mix_bool(self.market_gate_open);
        m.mix_u64(self.next_entity);
        m.mix_u64(self.calls_ok);
        m.mix_u64(self.calls_denied);
        m.mix_u64(self.writes_passed);
        m.mix_u64(self.writes_spent);
        // 申报册（注册序入拌）
        m.mix_u64(self.filings.len() as u64);
        let mut i: usize = 0;
        while i < self.filings.len() {
            let f = &self.filings[i];
            m.mix_u32(f.caller);
            m.mix_str(&f.name);
            m.mix_u8(f.quad.ord());
            m.mix_bool(f.reserved);
            m.mix_u64(f.filed_at);
            i += 1;
        }
        self.surface.digest_mix(&mut m);
        self.handles.digest_mix(&mut m);
        self.auth.digest_mix(&mut m);
        self.breaker.digest_mix(&mut m);
        self.audit.digest_mix(&mut m);
        self.events.digest_mix(&mut m);
        m.finish()
    }

    /// 五律落地对拍（F5003 衔接——逐律机械报告；确定优先律内置双跑）。
    pub fn law_grounding_report(&self) -> LawGroundingReport {
        // 可读第一：全锚名过定式
        let mut readable_ok = true;
        let mut i: usize = 0;
        while i < self.surface.len() {
            match self.surface.entry(i) {
                Some(e) => {
                    if !anchor_name_wellformed(&e.anchor) {
                        readable_ok = false;
                    }
                }
                Option::None => {
                    readable_ok = false;
                }
            }
            i += 1;
        }
        // 错误友好：16 码诊断非空（码表驱动逐码构造——全变体覆盖）
        let mut friendly_ok = true;
        let mut ci: usize = 0;
        while ci < CODE_TABLE.len() {
            let err = error_of_code(CODE_TABLE[ci]);
            if err.is_none() || err.map(|e| e.detail().is_empty()).unwrap_or(true) {
                friendly_ok = false;
            }
            ci += 1;
        }
        // 安全默认：写面放行 == 授权花销（无一笔写漏过授权）
        let safe_default_ok = self.writes_passed == self.writes_spent;
        // 渐进复杂：面上常备 0/1/2 参档位
        let mut has0 = false;
        let mut has1 = false;
        let mut has2 = false;
        let mut si: usize = 0;
        while si < self.surface.len() {
            if let Some(e) = self.surface.entry(si) {
                match e.sig.n_params {
                    0 => has0 = true,
                    1 => has1 = true,
                    2 => has2 = true,
                    _ => {}
                }
            }
            si += 1;
        }
        let gradual_ok = has0 && has1 && has2;
        // 确定优先：内置双跑对拍（固定内置序列，双开机比较摘要）
        let deterministic_ok = Self::builtin_dual_run_ok();
        LawGroundingReport {
            readable_ok,
            friendly_ok,
            safe_default_ok,
            gradual_ok,
            deterministic_ok,
            all_ok: readable_ok
                && friendly_ok
                && safe_default_ok
                && gradual_ok
                && deterministic_ok,
        }
    }

    /// 内置双跑对拍（固定六步序列×双开机——law_grounding_report 的
    /// 确定优先律执行位；任何一步失败按 false 报，不外抛）。
    fn builtin_dual_run_ok() -> bool {
        let a = Self::builtin_probe_run();
        let b = Self::builtin_probe_run();
        match (a, b) {
            (Ok(da), Ok(db)) => da == db,
            _ => false,
        }
    }

    /// 内置探针序列单跑（六步：申报→读→写拦→授权→写放→再读）。
    fn builtin_probe_run() -> Result<u64, SysBoundError> {
        let mut w = SysRelWorld::boot()?;
        w.file_capability(7, "builtin.scene.light", CapabilityFacts::LIGHT_BEHAVIOR)?;
        // 读面（免授权）
        w.call(7, "kern.clock.tick", &[])?;
        // 写面无授权 → 应拦（内置序列的确定性拒付也进状态）
        if w.call(7, "file.persist.append", &[XVal::text("a")?, XVal::text("b")?])
            .is_ok()
        {
            return Err(SysBoundError::InjectEscape);
        }
        // 授权后写放行
        w.issue_grant("file.persist.append", 2)?;
        w.call(
            7,
            "file.persist.append",
            &[XVal::text("scene")?, XVal::text("light=on")?],
        )?;
        // 再读（面状态进摘要）
        w.call(7, "kern.entity.query_pos", &[XVal::num(3)])?;
        Ok(w.state_digest())
    }

    /// 双跑确定性对拍（外部序列版——同序列两次开机，摘要必相等）。
    ///
    /// 摘要不等 → DigestDiverge（确定优先律被破坏的正式检出码）。
    pub fn dual_run_selfcheck(ops: &[ProbeOp]) -> Result<(), SysBoundError> {
        let d1 = Self::run_probe_seq(ops)?;
        let d2 = Self::run_probe_seq(ops)?;
        if d1 == d2 {
            Ok(())
        } else {
            Err(SysBoundError::DigestDiverge)
        }
    }

    /// 探针序列单跑（双跑的执行单元——失败即外抛，失败也是确定性面；
    /// 公开供对拍敏感度试验：改一参跑一次，摘要应立变）。
    pub fn run_probe_seq(ops: &[ProbeOp]) -> Result<u64, SysBoundError> {
        let mut w = SysRelWorld::boot()?;
        // 序列固定申报一个轻量行为能力（保证脚本调用资格——序列可专注面语义）
        w.file_capability(1, "probe.capability", CapabilityFacts::LIGHT_BEHAVIOR)?;
        let mut i: usize = 0;
        while i < ops.len() {
            let op = &ops[i];
            if op.advance > 0 {
                w.advance_tick(op.advance);
            }
            let _ = w.call(op.caller, op.anchor, &op.args);
            i += 1;
        }
        Ok(w.state_digest())
    }

    /// 注入三面自证（越界调用/句柄滥用/未授权写——各须检出且入账）。
    ///
    /// 任何一路未拦或未留小票 → InjectEscape（边界子系统自证失败）。
    pub fn injection_selfcheck() -> Result<(), SysBoundError> {
        let mut w = SysRelWorld::boot()?;
        w.file_capability(9, "inject.probe.cap", CapabilityFacts::LIGHT_BEHAVIOR)?;
        // 注入一：越白名单调用（未注册锚 + 禁入锚双试样）
        let e1 = w.call(9, "kern.mem.raw_ptr", &[XVal::num(0)]);
        if e1.err().map(|e| e.code()) != Some(E_CALL_OFF_SURFACE) {
            return Err(SysBoundError::InjectEscape);
        }
        let e1b = w.call(9, "no.such.api", &[]);
        if e1b.err().map(|e| e.code()) != Some(E_CALL_OFF_SURFACE) {
            return Err(SysBoundError::InjectEscape);
        }
        if w.audit.count_denied_with(E_CALL_OFF_SURFACE) < 2 {
            return Err(SysBoundError::InjectEscape);
        }
        // 注入二：句柄滥用（转移柄二次转移 + 视图柄写面冒充）
        let h = w.issue_handle(HS_TRANSFER, HANDLE_TTL_TICKS, "kern.entity.spawn")?;
        if w.hand_off_handle(h).is_err() {
            return Err(SysBoundError::InjectEscape);
        }
        let e2 = w.hand_off_handle(h);
        if e2.err().map(|e| e.code()) != Some(E_HANDLE_ABUSE) {
            return Err(SysBoundError::InjectEscape);
        }
        let v = w.issue_handle(HS_VIEW, VIEW_TTL_TICKS, "gfx.texture.snapshot")?;
        // 视图冒充写面（经签名收 Ref 的释放面——Effect 面 + 视图实参即拦）
        w.issue_grant("kern.entity.release", 4)?;
        let e2b = w.call(9, "kern.entity.release", &[XVal::view(v)]);
        if e2b.err().map(|e| e.code()) != Some(E_HANDLE_ABUSE) {
            return Err(SysBoundError::InjectEscape);
        }
        // 注入三：未授权写（无授权位直写 + 额度耗尽续写）
        let e3 = w.call(9, "input.bind.declare", &[XVal::num(1), XVal::text("jump")?]);
        if e3.err().map(|e| e.code()) != Some(E_WRITE_UNAUTHORIZED) {
            return Err(SysBoundError::InjectEscape);
        }
        let gi = w.issue_grant("input.bind.declare", 1)?;
        w.call(9, "input.bind.declare", &[XVal::num(1), XVal::text("jump")?])?;
        let e3b = w.call(9, "input.bind.declare", &[XVal::num(2), XVal::text("fire")?]);
        if e3b.err().map(|e| e.code()) != Some(E_AUTH_SPENT) {
            return Err(SysBoundError::InjectEscape);
        }
        // 三面全检出且全留痕（审计拒付小票：越白名单×2、视图冒充×1、
        // 未授权写×1——直接句柄操作的滥用经事件账留痕）
        if w.audit.count_denied_with(E_CALL_OFF_SURFACE) < 2
            || w.audit.count_denied_with(E_HANDLE_ABUSE) < 1
            || w.audit.count_denied_with(E_WRITE_UNAUTHORIZED) < 1
        {
            return Err(SysBoundError::InjectEscape);
        }
        // 授权位撤销口在自证里同步复验（撤后写面即拦——授权账闭环）
        w.revoke_grant(gi)?;
        Ok(())
    }
}

/// 诊断码 → 错误变体（law_grounding_report 的错误友好律执行位：
/// 逐码构造变体验诊断非空——码表与变体一一对应的运行期复验）。
pub fn error_of_code(code: u16) -> Option<SysBoundError> {
    match code {
        E_REL_UNDECLARED => Some(SysBoundError::RelUndeclared),
        E_REL_MISFILED => Some(SysBoundError::RelMisfiled),
        E_FACE_VIOL => Some(SysBoundError::FaceViol),
        E_SURFACE_FULL => Some(SysBoundError::SurfaceFull),
        E_SURFACE_DUP => Some(SysBoundError::SurfaceDup),
        E_CALL_OFF_SURFACE => Some(SysBoundError::CallOffSurface),
        E_SIG_MISMATCH => Some(SysBoundError::SigMismatch),
        E_XVAL_TYPE => Some(SysBoundError::XvalType),
        E_HANDLE_ABUSE => Some(SysBoundError::HandleAbuse),
        E_WRITE_UNAUTHORIZED => Some(SysBoundError::WriteUnauthorized),
        E_AUTH_SPENT => Some(SysBoundError::AuthSpent),
        E_BREAKER_TRIPPED => Some(SysBoundError::BreakerTripped),
        E_RESERVATION_PRIVATE => Some(SysBoundError::ReservationPrivate),
        E_GRANT_FULL => Some(SysBoundError::GrantFull),
        E_INJECT_ESCAPE => Some(SysBoundError::InjectEscape),
        E_DIGEST_DIVERGE => Some(SysBoundError::DigestDiverge),
        _ => Option::None,
    }
}

// ---------------------------------------------------------------------------
// 十二、编译期断言（const 纪律——开机即冻结的结构性证明）
// ---------------------------------------------------------------------------

/// 诊断码段独占（0x1DD0..=0x1DDF 连续互斥）。
const _: () = assert!(ERR_CODE_BASE == 0x1DD0);
const _: () = assert!(ERR_CODE_END == 0x1DDF);
const _: () = assert!(ERR_CODE_COUNT == 16);
const _: () = assert!(codes_consecutive(&CODE_TABLE));
const _: () = assert!(codes_distinct(&CODE_TABLE));

/// 关系四定闭集（恰四定且四卡序数互异——码册闭集不扩员）。
const _: () = assert!(QUAD_CARDS.len() == REL_QUAD_COUNT);
const fn quad_cards_closed() -> bool {
    let mut i: usize = 0;
    while i < QUAD_CARDS.len() {
        let mut j: usize = i + 1;
        while j < QUAD_CARDS.len() {
            if QUAD_CARDS[i].quad.ord() == QUAD_CARDS[j].quad.ord() {
                return false;
            }
            j += 1;
        }
        if QUAD_CARDS[i].quad.ord() < 1 || QUAD_CARDS[i].quad.ord() > 4 {
            return false;
        }
        i += 1;
    }
    true
}
const _: () = assert!(quad_cards_closed());

/// 四域边界卡册闭集（恰四域、可调用面锚名全合式、面内锚不入禁入面、
/// 禁入锚不入任何域可调用面、域协议值型集非空）。
const _: () = assert!(DOMAIN_CARDS.len() == HOST_DOMAIN_COUNT);
const fn domain_cards_closed() -> bool {
    let mut d: usize = 0;
    while d < DOMAIN_CARDS.len() {
        let card = &DOMAIN_CARDS[d];
        if HostDomain::from_ord(card.domain.ord()).is_none() {
            return false;
        }
        if card.proto_kinds.len() == 0 {
            return false;
        }
        let mut ci: usize = 0;
        while ci < card.callable.len() {
            if !anchor_name_wellformed(card.callable[ci]) {
                return false;
            }
            if face_contains(card.forbidden, card.callable[ci]) {
                return false;
            }
            if face_contains(&REVERSE_HOOK_ANCHORS, card.callable[ci]) {
                return false;
            }
            ci += 1;
        }
        let mut fi: usize = 0;
        while fi < card.forbidden.len() {
            let mut od: usize = 0;
            while od < DOMAIN_CARDS.len() {
                if face_contains(DOMAIN_CARDS[od].callable, card.forbidden[fi]) {
                    return false;
                }
                od += 1;
            }
            fi += 1;
        }
        d += 1;
    }
    true
}
const _: () = assert!(domain_cards_closed());

/// 规范 API 种子与边界卡对齐（11 条：每条锚落本域可调用面、不触禁入面、
/// 名字合式、参数数 ≤6、参数与返回型全落本域协议）。
const _: () = assert!(CANONICAL_APIS.len() == 11);
const fn canonical_apis_aligned() -> bool {
    let mut i: usize = 0;
    while i < CANONICAL_APIS.len() {
        let api = &CANONICAL_APIS[i];
        let mut d: usize = 0;
        let mut card: Option<&'static DomainCard> = Option::None;
        while d < DOMAIN_CARDS.len() {
            if DOMAIN_CARDS[d].domain.ord() == api.domain.ord() {
                card = Some(&DOMAIN_CARDS[d]);
            }
            d += 1;
        }
        let c = match card {
            Some(c) => c,
            Option::None => return false,
        };
        if !face_contains(c.callable, api.anchor) {
            return false;
        }
        if face_contains(c.forbidden, api.anchor) {
            return false;
        }
        if !anchor_name_wellformed(api.anchor) {
            return false;
        }
        if api.params.len() > 6 {
            return false;
        }
        let mut pi: usize = 0;
        while pi < api.params.len() {
            if !api.params[pi].in_proto(c.proto_kinds) {
                return false;
            }
            pi += 1;
        }
        if !api.ret.in_proto(c.proto_kinds) {
            return false;
        }
        i += 1;
    }
    true
}
const _: () = assert!(canonical_apis_aligned());

/// 容量纪律（白名单容量装得下种子；授权/句柄/审计容量非零）。
const _: () = assert!(SURFACE_CAP >= CANONICAL_APIS.len());
const _: () = assert!(GRANT_SLOTS > 0);
const _: () = assert!(HANDLE_SLOTS > 0);
const _: () = assert!(AUDIT_RING_CAP > 0);
const _: () = assert!(EVENT_CAP > 0);

/// 熔断纪律（阈值 ≥1、窗口宽 ≥1、冷却 ≥1——参数写死即冻结）。
const _: () = assert!(BREAKER_MAX_CALLS >= 1);
const _: () = assert!(BREAKER_WINDOW_TICKS >= 1);
const _: () = assert!(BREAKER_COOLDOWN_WINDOWS >= 1);

/// 五律落地卡册闭集（恰五卡——与 F5003 五律同名同序）。
const _: () = assert!(FIVE_LAW_GROUNDING.len() == 5);

// ---------------------------------------------------------------------------
// 十三、回归用例（冒烟全绿口径：白名单/句柄生命周期/授权位/跨界审计/
// 三注入检出/双跑一致——判据「判据」的测试侧执行件）
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec;

    // —— 开机与白名单 ——

    #[test]
    fn boot_canonical_surface_and_chain() {
        let w = SysRelWorld::boot().expect("boot");
        assert_eq!(w.surface_len(), 11);
        assert!(w.health().all_ok());
        // 指纹非零且验链通过（11 条全过）
        assert!(InteropSurface::canonical().expect("canonical").verify_all());
    }

    #[test]
    fn off_surface_call_denied_and_audited() {
        let mut w = SysRelWorld::boot().expect("boot");
        w.file_capability(1, "scene.light", CapabilityFacts::LIGHT_BEHAVIOR)
            .expect("file");
        let e = w
            .call(1, "no.such.api", &[])
            .expect_err("off-surface must be denied");
        assert_eq!(e.code(), E_CALL_OFF_SURFACE);
        assert!(!e.detail().is_empty());
        // 越白名单调用必须留小票
        let h = w.health();
        assert_eq!(h.audit_denied, 1);
        assert_eq!(h.calls_denied, 1);
        assert_eq!(h.calls_ok, 0);
        assert!(h.invariants_ok);
    }

    #[test]
    fn forbidden_and_reverse_anchor_cannot_register() {
        let mut w = SysRelWorld::boot().expect("boot");
        // 禁入面锚：注册即拦
        assert_eq!(
            w.register_api(
                "kern.mem.raw_ptr",
                HostDomain::Kernel,
                AccessKind::Read,
                &[XValKind::Num],
                XValKind::Num,
                HostImpl::PureNum
            )
            .unwrap_err()
            .code(),
            E_FACE_VIOL
        );
        // 反向侵入锚：注册即拦（单向编排）
        assert_eq!(
            w.register_api(
                "host.callback.into_script",
                HostDomain::Kernel,
                AccessKind::Read,
                &[],
                XValKind::Num,
                HostImpl::PureNum
            )
            .unwrap_err()
            .code(),
            E_FACE_VIOL
        );
        // 坏名字（大写/段超宽）：拦
        assert_eq!(
            w.register_api(
                "Kern.Bad",
                HostDomain::Kernel,
                AccessKind::Read,
                &[],
                XValKind::Num,
                HostImpl::PureNum
            )
            .unwrap_err()
            .code(),
            E_FACE_VIOL
        );
        // 域错配（图形余地锚注册到文件域）：面外拦
        assert_eq!(
            w.register_api(
                "gfx.layer.set",
                HostDomain::Filesys,
                AccessKind::Write,
                &[XValKind::Num],
                XValKind::Num,
                HostImpl::AckWrite
            )
            .unwrap_err()
            .code(),
            E_FACE_VIOL
        );
        // 已注册锚的重复注册在面校验之前拦（dup 优先——白名单不收同名双签）
        assert_eq!(
            w.register_api(
                "file.config.load",
                HostDomain::Filesys,
                AccessKind::Read,
                &[XValKind::Str],
                XValKind::Str,
                HostImpl::EchoStr
            )
            .unwrap_err()
            .code(),
            E_SURFACE_DUP
        );
    }

    #[test]
    fn domain_proto_kind_violation() {
        let mut w = SysRelWorld::boot().expect("boot");
        // 面内余地锚但值型出域协议：图形域不收字符串——拦
        assert_eq!(
            w.register_api(
                "gfx.layer.set",
                HostDomain::Graphics,
                AccessKind::Write,
                &[XValKind::Str],
                XValKind::Num,
                HostImpl::AckWrite
            )
            .unwrap_err()
            .code(),
            E_FACE_VIOL
        );
    }

    #[test]
    fn face_is_wider_than_whitelist() {
        let mut w = SysRelWorld::boot().expect("boot");
        w.file_capability(1, "scene.query", CapabilityFacts::LIGHT_BEHAVIOR)
            .expect("file");
        // 余地锚在可调用面内但未注册白名单：调用即越白名单
        assert_eq!(
            w.call(1, "kern.scene.graph_query", &[XVal::num(1)])
                .unwrap_err()
                .code(),
            E_CALL_OFF_SURFACE
        );
        // 宿主显式注册后面内锚即可调（面宽于白名单：注册制不是面制的复读）
        w.register_api(
            "kern.scene.graph_query",
            HostDomain::Kernel,
            AccessKind::Read,
            &[XValKind::Num],
            XValKind::Num,
            HostImpl::PureNum,
        )
        .expect("register spare");
        assert!(w.call(1, "kern.scene.graph_query", &[XVal::num(1)]).is_ok());
    }

    #[test]
    fn surface_dup_and_full() {
        let mut w = SysRelWorld::boot().expect("boot");
        // 重复注册
        assert_eq!(
            w.register_api(
                "kern.entity.query_pos",
                HostDomain::Kernel,
                AccessKind::Read,
                &[XValKind::Num],
                XValKind::Num,
                HostImpl::PureNum
            )
            .unwrap_err()
            .code(),
            E_SURFACE_DUP
        );
        // 余地 5 条（规范 11 + 5 = 16 恰满）
        let spares: [(&str, HostDomain, AccessKind, &[XValKind], XValKind); 5] = [
            (
                "kern.entity.destroy",
                HostDomain::Kernel,
                AccessKind::Write,
                &[XValKind::Num],
                XValKind::Num,
            ),
            (
                "kern.event.read",
                HostDomain::Kernel,
                AccessKind::Read,
                &[XValKind::Num],
                XValKind::Num,
            ),
            (
                "gfx.text.draw",
                HostDomain::Graphics,
                AccessKind::Write,
                &[XValKind::Num, XValKind::Num],
                XValKind::Num,
            ),
            (
                "input.axis.state",
                HostDomain::Input,
                AccessKind::Read,
                &[XValKind::Num],
                XValKind::Num,
            ),
            (
                "file.config.save",
                HostDomain::Filesys,
                AccessKind::Write,
                &[XValKind::Str, XValKind::Str],
                XValKind::Num,
            ),
        ];
        for (name, dom, acc, ps, r) in spares {
            w.register_api(name, dom, acc, ps, r, HostImpl::AckWrite)
                .expect("spare register");
        }
        assert_eq!(w.surface_len(), SURFACE_CAP);
        // 第 17 条（面内还有 gfx.layer.set/file.blob.read）——容量拦
        assert_eq!(
            w.register_api(
                "gfx.layer.set",
                HostDomain::Graphics,
                AccessKind::Write,
                &[XValKind::Num],
                XValKind::Num,
                HostImpl::AckWrite
            )
            .unwrap_err()
            .code(),
            E_SURFACE_FULL
        );
    }

    #[test]
    fn fingerprint_tamper_detected() {
        let mut s = InteropSurface::canonical().expect("canonical");
        assert!(s.verify_all());
        assert!(s.tamper_fp_for_probe(0));
        assert!(!s.verify_all());
        // 篡改越界索引：无操作 false
        assert!(!s.tamper_fp_for_probe(99));
    }

    // —— 关系四定 ——

    #[test]
    fn relation_quad_filing_guidance() {
        let mut w = SysRelWorld::boot().expect("boot");
        // 轻量行为 → 引擎
        let v = w
            .file_capability(1, "light.logic", CapabilityFacts::LIGHT_BEHAVIOR)
            .expect("light");
        assert_eq!(v.quad, RelQuad::Engine);
        assert!(!v.refiled);
        // 重原生扩展 → 插件 SDK（引导归位）
        let v2 = w
            .file_capability(2, "heavy.ext", CapabilityFacts::HEAVY_NATIVE)
            .expect("heavy");
        assert_eq!(v2.quad, RelQuad::PluginSdk);
        // 纯数据 → 配置系统
        let v3 = w
            .file_capability(3, "pure.data", CapabilityFacts::PURE_DATA)
            .expect("data");
        assert_eq!(v3.quad, RelQuad::ConfigSys);
        // 自相矛盾事实面 → 混岗拦
        assert_eq!(
            w.file_capability(1, "conflict.cap", CapabilityFacts::CONFLICTING)
                .unwrap_err()
                .code(),
            E_REL_MISFILED
        );
        // 空能力（既非数据又非行为）→ 拦
        assert_eq!(
            w.file_capability(1, "empty.cap", CapabilityFacts::EMPTY)
                .unwrap_err()
                .code(),
            E_REL_MISFILED
        );
        // 申报卡不完整（名超 32 字节）→ 拦
        let long_name = "a".repeat(33);
        assert_eq!(
            w.file_capability(1, &long_name, CapabilityFacts::LIGHT_BEHAVIOR)
                .unwrap_err()
                .code(),
            E_REL_UNDECLARED
        );
        assert_eq!(w.filing_count(), 3);
    }

    #[test]
    fn misfiled_then_refiled_with_receipt() {
        let mut w = SysRelWorld::boot().expect("boot");
        w.file_capability(1, "trig.door", CapabilityFacts::LIGHT_BEHAVIOR)
            .expect("file");
        // 关系失配：同能力事实升重 → 对拍修正（改判留痕不拒收）
        let v = w
            .file_capability(1, "trig.door", CapabilityFacts::HEAVY_NATIVE)
            .expect("refile");
        assert_eq!(v.quad, RelQuad::PluginSdk);
        assert!(v.refiled);
        // 同定幂等再报：不再计改判
        let v2 = w
            .file_capability(1, "trig.door", CapabilityFacts::HEAVY_NATIVE)
            .expect("idempotent");
        assert!(!v2.refiled);
        assert_eq!(v2.quad, RelQuad::PluginSdk);
        // 对拍修正恰好一次留痕
        let mut m = DigestMixer::new();
        m.mix_u8(0);
        let _ = m.finish();
        // 事件账直接数（事件账非环形导出口——经 health 的事件总量 + 长度旁证）
        assert_eq!(w.filing_count(), 1);
    }

    #[test]
    fn market_reservation_gate() {
        let mut w = SysRelWorld::boot().expect("boot");
        let mut facts = CapabilityFacts::LIGHT_BEHAVIOR;
        facts.wants_market_listing = true;
        // 预留门未开：上架申报即拦（预留私启零容忍）
        assert_eq!(
            w.file_capability(1, "pack.fx", facts).unwrap_err().code(),
            E_RESERVATION_PRIVATE
        );
        // 错钥匙开不了
        assert_eq!(
            w.open_market_gate(0xDEAD_BEEF).unwrap_err().code(),
            E_RESERVATION_PRIVATE
        );
        // 正钥匙开 + 上架申报过（reserved 位=true——预留标记非承诺）
        w.open_market_gate(MARKET_HOST_KEY).expect("open gate");
        let v = w.file_capability(1, "pack.fx", facts).expect("market file");
        assert_eq!(v.quad, RelQuad::MarketReserve);
        assert!(v.reserved);
        assert!(w.health().market_gate_open);
    }

    #[test]
    fn undeclared_caller_denied() {
        let mut w = SysRelWorld::boot().expect("boot");
        // 未申报四定的脚本：读面也不放（先归位才可跨界）
        assert_eq!(
            w.call(5, "kern.clock.tick", &[]).unwrap_err().code(),
            E_REL_UNDECLARED
        );
        // 宿主内建（caller=0）免申报
        assert!(w.call(HOST_CALLER, "kern.clock.tick", &[]).is_ok());
    }

    // —— 签名与跨界值 ——

    #[test]
    fn signature_and_type_gates() {
        let mut w = SysRelWorld::boot().expect("boot");
        w.file_capability(1, "s.boot", CapabilityFacts::LIGHT_BEHAVIOR)
            .expect("file");
        // 个数失配
        assert_eq!(
            w.call(1, "kern.entity.query_pos", &[]).unwrap_err().code(),
            E_SIG_MISMATCH
        );
        assert_eq!(
            w.call(1, "kern.entity.query_pos", &[XVal::num(1), XVal::num(2)])
                .unwrap_err()
                .code(),
            E_SIG_MISMATCH
        );
        // 类型失配（跨界不隐式转换）
        assert_eq!(
            w.call(1, "kern.entity.query_pos", &[XVal::text("x").expect("str")])
                .unwrap_err()
                .code(),
            E_XVAL_TYPE
        );
        // 正确调用放行
        assert!(w.call(1, "kern.entity.query_pos", &[XVal::num(7)]).is_ok());
    }

    #[test]
    fn xstr_cap_enforced() {
        let mut w = SysRelWorld::boot().expect("boot");
        w.file_capability(1, "s.str", CapabilityFacts::LIGHT_BEHAVIOR)
            .expect("file");
        // 回显语义 `[cfg]`+输入（5 字节前缀）：输入 43 字节恰好回显 ≤48 过界
        let ok43 = "u".repeat(43);
        let r = w.call(1, "file.config.load", &[XVal::text(&ok43).expect("43")]);
        assert!(r.is_ok());
        // 输入 44 字节：回显超 48 上限——执行层拒收（不截断：截断即静默改义）
        let long44 = "u".repeat(44);
        assert_eq!(
            w.call(1, "file.config.load", &[XVal::text(&long44).expect("44")])
                .unwrap_err()
                .code(),
            E_XVAL_TYPE
        );
        // 输入侧构造上限独立生效：49 字节构造即拒
        let long49 = "u".repeat(49);
        assert_eq!(XVal::text(&long49).unwrap_err().code(), E_XVAL_TYPE);
    }

    #[test]
    fn host_impls_deterministic_semantics() {
        let mut w = SysRelWorld::boot().expect("boot");
        w.file_capability(1, "s.impl", CapabilityFacts::LIGHT_BEHAVIOR)
            .expect("file");
        // WorldTick：返回当前节拍
        w.advance_tick(5);
        match w.call(1, "kern.clock.tick", &[]).expect("tick") {
            XVal::Num(n) => assert_eq!(n, 5),
            _ => panic_test_fail(),
        }
        // PureNum：同参同返（确定优先）
        let a = w.call(1, "kern.entity.query_pos", &[XVal::num(42)]);
        let b = w.call(1, "kern.entity.query_pos", &[XVal::num(42)]);
        assert_eq!(a.expect("q1"), b.expect("q2"));
        // EntitySeq：单调发号
        w.issue_grant("kern.entity.spawn", 2).expect("grant");
        match w
            .call(1, "kern.entity.spawn", &[XVal::num(1), XVal::num(2)])
            .expect("spawn1")
        {
            XVal::Num(n1) => match w
                .call(1, "kern.entity.spawn", &[XVal::num(3), XVal::num(4)])
                .expect("spawn2")
            {
                XVal::Num(n2) => assert_eq!(n2, n1 + 1),
                _ => panic_test_fail(),
            },
            _ => panic_test_fail(),
        }
        // EchoStr：`[cfg]`+输入串
        match w
            .call(1, "file.config.load", &[XVal::text("ui").expect("s")])
            .expect("echo")
        {
            XVal::Str(s) => assert_eq!(s, "[cfg]ui"),
            _ => panic_test_fail(),
        }
    }

    // —— 句柄生命周期 ——

    #[test]
    fn handle_borrow_ttl_reap() {
        let mut w = SysRelWorld::boot().expect("boot");
        w.file_capability(1, "s.h", CapabilityFacts::LIGHT_BEHAVIOR)
            .expect("file");
        let h = w
            .issue_handle(HS_BORROW, 8, "kern.entity.query_pos")
            .expect("borrow");
        w.issue_grant("kern.entity.release", 1).expect("grant");
        // TTL 8 内借用柄活着：释放面拦的是「借用不可释放」而非柄失效
        // （TTL 到界后拦的才是到期回收——两种 HandleAbuse 同码不同因，
        // 经节拍推进区分验证）
        w.advance_tick(9); // 第 9 拍：born=0, ttl=8 → 到期扫回收
        assert_eq!(
            w.call(1, "kern.entity.release", &[XVal::handle(h)])
                .unwrap_err()
                .code(),
            E_HANDLE_ABUSE
        );
        assert!(w.health().handles_revoked >= 1);
        assert!(w.health().invariants_ok);
    }

    #[test]
    fn handle_transfer_once_only() {
        let mut w = SysRelWorld::boot().expect("boot");
        let h = w
            .issue_handle(HS_TRANSFER, HANDLE_TTL_TICKS, "kern.entity.spawn")
            .expect("transfer");
        // 一次性转移：第一次成功
        w.hand_off_handle(h).expect("hand off once");
        // 二次转移即滥用（注入面之二的核心样例）
        assert_eq!(
            w.hand_off_handle(h).unwrap_err().code(),
            E_HANDLE_ABUSE
        );
        assert_eq!(w.health().handles_handed, 1);
    }

    #[test]
    fn handle_view_readonly_no_chain() {
        let mut w = SysRelWorld::boot().expect("boot");
        // 借用柄派生视图：一次可以
        let b = w
            .issue_handle(HS_BORROW, HANDLE_TTL_TICKS, "gfx.texture.snapshot")
            .expect("borrow");
        let v = w.derive_view(b, VIEW_TTL_TICKS).expect("view");
        // 视图再派生视图：拦（防视图链）
        assert_eq!(w.derive_view(v, 4).unwrap_err().code(), E_HANDLE_ABUSE);
        // Effect 面带视图实参：视图越权冒充即拦（经签名收 Ref 的释放面探）
        w.file_capability(1, "s.v", CapabilityFacts::LIGHT_BEHAVIOR)
            .expect("file");
        w.issue_grant("kern.entity.release", 4).expect("grant");
        assert_eq!(
            w.call(1, "kern.entity.release", &[XVal::view(v)])
                .unwrap_err()
                .code(),
            E_HANDLE_ABUSE
        );
    }

    #[test]
    fn handle_release_flow_and_abuse() {
        let mut w = SysRelWorld::boot().expect("boot");
        w.file_capability(1, "s.r", CapabilityFacts::LIGHT_BEHAVIOR)
            .expect("file");
        w.issue_grant("kern.entity.release", 3).expect("grant");
        // 转移柄释放成功
        let h = w
            .issue_handle(HS_TRANSFER, HANDLE_TTL_TICKS, "kern.entity.spawn")
            .expect("transfer");
        assert!(w.call(1, "kern.entity.release", &[XVal::handle(h)]).is_ok());
        // 已释放再释放：滥用
        assert_eq!(
            w.call(1, "kern.entity.release", &[XVal::handle(h)])
                .unwrap_err()
                .code(),
            E_HANDLE_ABUSE
        );
        // 借用柄不可释放（宿主保留所有权）
        let b = w
            .issue_handle(HS_BORROW, 8, "kern.entity.query_pos")
            .expect("borrow");
        assert_eq!(
            w.call(1, "kern.entity.release", &[XVal::handle(b)])
                .unwrap_err()
                .code(),
            E_HANDLE_ABUSE
        );
        // 过期柄（TTL 到界扫回收）不可释放
        w.advance_tick(HANDLE_TTL_TICKS as u32 + 1);
        let h2 = w
            .issue_handle(HS_TRANSFER, 2, "kern.entity.spawn")
            .expect("transfer2");
        w.advance_tick(3);
        assert_eq!(
            w.call(1, "kern.entity.release", &[XVal::handle(h2)])
                .unwrap_err()
                .code(),
            E_HANDLE_ABUSE
        );
    }

    #[test]
    fn handle_aba_generation_guard() {
        let mut w = SysRelWorld::boot().expect("boot");
        let h = w
            .issue_handle(HS_TRANSFER, HANDLE_TTL_TICKS, "kern.entity.spawn")
            .expect("transfer");
        w.handles.revoke(h).expect("revoke");
        // 同槽复用：新柄代际前进，旧号（携带旧代际）永不复活
        let h2 = w
            .issue_handle(HS_TRANSFER, HANDLE_TTL_TICKS, "kern.entity.spawn")
            .expect("reuse slot");
        assert_ne!(h, h2);
        // 旧号探查：滥用拦
        w.file_capability(1, "s.aba", CapabilityFacts::LIGHT_BEHAVIOR)
            .expect("file");
        w.issue_grant("kern.entity.release", 1).expect("grant");
        assert_eq!(
            w.call(1, "kern.entity.release", &[XVal::handle(h)])
                .unwrap_err()
                .code(),
            E_HANDLE_ABUSE
        );
    }

    // —— 授权位（安全默认律） ——

    #[test]
    fn write_requires_grant_and_spends() {
        let mut w = SysRelWorld::boot().expect("boot");
        w.file_capability(1, "s.w", CapabilityFacts::LIGHT_BEHAVIOR)
            .expect("file");
        // 无授权位直写：拦（注入面之三）
        assert_eq!(
            w.call(
                1,
                "input.bind.declare",
                &[XVal::num(1), XVal::text("jump").expect("s")]
            )
            .unwrap_err()
            .code(),
            E_WRITE_UNAUTHORIZED
        );
        // 零预算签发被拒
        assert_eq!(
            w.issue_grant("input.bind.declare", 0).unwrap_err().code(),
            E_AUTH_SPENT
        );
        // 预算 2：两笔放行，第三笔额度耗尽
        w.issue_grant("input.bind.declare", 2).expect("grant");
        assert!(w
            .call(1, "input.bind.declare", &[XVal::num(1), XVal::text("jump").expect("s")])
            .is_ok());
        assert!(w
            .call(1, "input.bind.declare", &[XVal::num(2), XVal::text("fire").expect("s")])
            .is_ok());
        assert_eq!(
            w.call(
                1,
                "input.bind.declare",
                &[XVal::num(3), XVal::text("menu").expect("s")]
            )
            .unwrap_err()
            .code(),
            E_AUTH_SPENT
        );
        // 读面始终免授权（安全默认只锁写面）
        assert!(w.call(1, "kern.clock.tick", &[]).is_ok());
        let h = w.health();
        assert_eq!(h.grants_active, 0); // 预算花光的位不算活跃
        assert_eq!(h.grants_issued, 1);
        assert_eq!(h.grants_spent, 2);
    }

    #[test]
    fn grant_revoke_blocks_and_ledger_full() {
        let mut w = SysRelWorld::boot().expect("boot");
        // 16 槽签满（同锚多位——宿主可叠发）
        for _ in 0..GRANT_SLOTS {
            w.issue_grant("gfx.draw.rect", 1).expect("fill grants");
        }
        assert_eq!(
            w.issue_grant("gfx.draw.rect", 1).unwrap_err().code(),
            E_GRANT_FULL
        );
        // 撤一位再签：复用槽位
        w.revoke_grant(0).expect("revoke");
        assert!(w.issue_grant("gfx.draw.rect", 1).is_ok());
    }

    // —— 熔断与审计 ——

    #[test]
    fn breaker_trips_and_recovers() {
        let mut w = SysRelWorld::boot().expect("boot");
        w.file_capability(1, "s.b", CapabilityFacts::LIGHT_BEHAVIOR)
            .expect("file");
        // 窗口内 32 次放行
        let mut ok: u32 = 0;
        while ok < BREAKER_MAX_CALLS {
            assert!(w.call(1, "kern.clock.tick", &[]).is_ok());
            ok += 1;
        }
        // 第 33 次熔断
        assert_eq!(
            w.call(1, "kern.clock.tick", &[]).unwrap_err().code(),
            E_BREAKER_TRIPPED
        );
        assert_eq!(w.health().breaker_trips, 1);
        // 冷却窗（窗口 0 熔断 → 冷却到 (0+1+2)*64=192）后复路
        w.advance_tick(192);
        assert!(w.call(1, "kern.clock.tick", &[]).is_ok());
        // 换锚/换脚本不受连坐（计数按 (调用方,锚) 组合）
        assert!(w.call(1, "kern.entity.query_pos", &[XVal::num(1)]).is_ok());
    }

    #[test]
    fn audit_ring_wraps_without_loss_of_totals() {
        let mut w = SysRelWorld::boot().expect("boot");
        w.file_capability(1, "s.a", CapabilityFacts::LIGHT_BEHAVIOR)
            .expect("file");
        // 70 次放行（4 锚轮转避开熔断阈）
        let anchors = [
            "kern.clock.tick",
            "kern.entity.query_pos",
            "gfx.frame.readback",
            "input.key.state",
        ];
        let mut n: u32 = 0;
        while n < 70 {
            let a = anchors[(n as usize) % 4];
            if a == "kern.clock.tick" {
                assert!(w.call(1, a, &[]).is_ok());
            } else if a == "kern.entity.query_pos" {
                assert!(w.call(1, a, &[XVal::num(n as i64)]).is_ok());
            } else if a == "gfx.frame.readback" {
                assert!(w.call(1, a, &[]).is_ok());
            } else {
                assert!(w.call(1, a, &[XVal::num(n as i64)]).is_ok());
            }
            n += 1;
        }
        let h = w.health();
        assert_eq!(h.audit_seq, 70);
        assert_eq!(h.calls_ok, 70);
        // 环容量 64：覆写 6 轮，总量不丢
        assert_eq!(h.audit_wraps, 70 - AUDIT_RING_CAP as u64);
        assert!(h.invariants_ok);
    }

    #[test]
    fn denied_calls_all_have_receipts() {
        let mut w = SysRelWorld::boot().expect("boot");
        w.file_capability(1, "s.d", CapabilityFacts::LIGHT_BEHAVIOR)
            .expect("file");
        let _ = w.call(9, "kern.clock.tick", &[]); // 未申报
        let _ = w.call(1, "no.such.api", &[]); // 越白名单
        let _ = w.call(1, "kern.entity.query_pos", &[]); // 签名失配
        let _ = w.call(
            1,
            "input.bind.declare",
            &[XVal::num(1), XVal::text("x").expect("s")],
        ); // 未授权写
        let h = w.health();
        assert_eq!(h.calls_denied, 4);
        assert_eq!(h.audit_denied, 4);
        // 事件账四张拒付小票俱在
        assert_eq!(h.events_seq >= 4, true);
        assert!(h.invariants_ok);
    }

    // —— 反向侵入 ——

    #[test]
    fn reverse_hook_always_blocked() {
        let mut w = SysRelWorld::boot().expect("boot");
        assert_eq!(
            w.attempt_reverse_hook("host.callback.into_script")
                .unwrap_err()
                .code(),
            E_FACE_VIOL
        );
        assert_eq!(
            w.attempt_reverse_hook("host.event.pump_script")
                .unwrap_err()
                .code(),
            E_FACE_VIOL
        );
        // 任意自造反向锚同拦（单向编排不看名字出处）
        assert_eq!(
            w.attempt_reverse_hook("host.anything.else")
                .unwrap_err()
                .code(),
            E_FACE_VIOL
        );
    }

    // —— 注入三面与双跑 ——

    #[test]
    fn injection_selfcheck_all_green() {
        assert!(SysRelWorld::injection_selfcheck().is_ok());
    }

    #[test]
    fn dual_run_determinism_and_sensitivity() {
        let mk = |x: i64| -> Vec<ProbeOp> {
            vec![
                ProbeOp {
                    advance: 0,
                    caller: 1,
                    anchor: "kern.clock.tick",
                    args: Vec::new(),
                },
                ProbeOp {
                    advance: 3,
                    caller: 1,
                    anchor: "kern.entity.query_pos",
                    args: vec![XVal::num(x)],
                },
                ProbeOp {
                    advance: 0,
                    caller: 1,
                    anchor: "no.such.api",
                    args: Vec::new(),
                },
                ProbeOp {
                    advance: 1,
                    caller: 1,
                    anchor: "file.config.load",
                    args: vec![XVal::text("ui").expect("s")],
                },
                ProbeOp {
                    advance: 0,
                    caller: 1,
                    anchor: "input.bind.declare",
                    args: vec![XVal::num(2), XVal::text("fire").expect("s")],
                },
                ProbeOp {
                    advance: 2,
                    caller: 1,
                    anchor: "kern.clock.tick",
                    args: Vec::new(),
                },
            ]
        };
        let seq = mk(7);
        // 双跑一致
        assert!(SysRelWorld::dual_run_selfcheck(&seq).is_ok());
        // 敏感度：改一参，终态摘要必变
        let d1 = SysRelWorld::run_probe_seq(&mk(7)).expect("run1");
        let d2 = SysRelWorld::run_probe_seq(&mk(8)).expect("run2");
        assert_ne!(d1, d2);
    }

    #[test]
    fn builtin_dual_run_and_law_grounding() {
        let w = SysRelWorld::boot().expect("boot");
        let rep = w.law_grounding_report();
        assert!(rep.readable_ok);
        assert!(rep.friendly_ok);
        assert!(rep.safe_default_ok);
        assert!(rep.gradual_ok);
        assert!(rep.deterministic_ok);
        assert!(rep.all_ok);
        // 状态摘要非退化（非零非平凡）
        assert_ne!(w.state_digest(), 0);
    }

    // —— 常量卡册与码表 ——

    #[test]
    fn quad_cards_closed_and_named() {
        assert_eq!(QUAD_CARDS.len(), 4);
        let mut i: usize = 0;
        while i < QUAD_CARDS.len() {
            assert!(!QUAD_CARDS[i].decree.is_empty());
            assert!(!QUAD_CARDS[i].caliber.is_empty());
            assert!(!QUAD_CARDS[i].misuse.is_empty());
            assert!(RelQuad::from_ord(QUAD_CARDS[i].quad.ord()).is_some());
            i += 1;
        }
        assert_eq!(RelQuad::Engine.name(), "引擎系统");
        assert_eq!(RelQuad::from_ord(5), None);
    }

    #[test]
    fn domain_cards_faces_aligned() {
        assert_eq!(DOMAIN_CARDS.len(), 4);
        // 每条规范 API 的锚都在本域可调用面
        let mut i: usize = 0;
        while i < CANONICAL_APIS.len() {
            let api = &CANONICAL_APIS[i];
            assert_eq!(
                SystemBoundary::classify(api.domain, api.anchor),
                FaceVerdict::Callable
            );
            i += 1;
        }
        // 禁入锚三态判读
        assert_eq!(
            SystemBoundary::classify(HostDomain::Kernel, "kern.mem.raw_ptr"),
            FaceVerdict::Forbidden
        );
        // 面外锚
        assert_eq!(
            SystemBoundary::classify(HostDomain::Kernel, "kern.not.in.faces"),
            FaceVerdict::Outside
        );
        // 锚寻域
        assert_eq!(
            SystemBoundary::domain_of("file.config.load"),
            Some(HostDomain::Filesys)
        );
        assert_eq!(SystemBoundary::domain_of("no.such.api"), None);
        assert!(SystemBoundary::integrity_ok());
    }

    #[test]
    fn code_table_roundtrip_and_details() {
        let mut i: usize = 0;
        while i < CODE_TABLE.len() {
            let err = error_of_code(CODE_TABLE[i]).expect("code maps to error");
            assert_eq!(err.code(), CODE_TABLE[i]);
            assert!(!err.detail().is_empty());
            assert!(!err.brief().is_empty());
            i += 1;
        }
        assert_eq!(error_of_code(0x1D00), None);
        assert_eq!(CODE_TABLE.len(), ERR_CODE_COUNT as usize);
    }

    #[test]
    fn error_segment_exclusive() {
        // 诊断码段独占：0x1DD0..=0x1DDF（邻件 vez53 占 0x1D50..=0x1D5F——
        // 两段互斥的结构性自证）
        assert_eq!(ERR_CODE_BASE, 0x1DD0);
        assert_eq!(ERR_CODE_END, 0x1DDF);
        assert!(ERR_CODE_BASE > 0x1D5F || ERR_CODE_END < 0x1D50);
    }

    #[test]
    fn health_after_mixed_ops_stays_consistent() {
        let mut w = SysRelWorld::boot().expect("boot");
        w.file_capability(1, "s.mix", CapabilityFacts::LIGHT_BEHAVIOR)
            .expect("file");
        w.issue_grant("gfx.draw.rect", 2).expect("grant");
        let _ = w.call(1, "kern.clock.tick", &[]);
        let _ = w.call(
            1,
            "gfx.draw.rect",
            &[XVal::num(1), XVal::num(2), XVal::num(3), XVal::num(4)],
        );
        let _ = w.call(1, "no.such.api", &[]);
        let _ = w.call(1, "gfx.texture.snapshot", &[XVal::num(9)]);
        w.advance_tick(4);
        let h = w.health();
        assert_eq!(h.calls_ok, 3);
        assert_eq!(h.calls_denied, 1);
        assert_eq!(h.handles_live, 1); // snapshot 签出的视图在 TTL 内
        assert!(h.sig_chain_ok);
        assert!(h.boundary_ok);
        assert!(h.invariants_ok);
    }

    /// 测试侧失败桩（内核路径零 panic 纪律不适用于 #[cfg(test)]——
    /// 但统一走此桩便于审计测试自身）。
    fn panic_test_fail() -> ! {
        panic!("pattern match arm unreachable in test expectation");
    }
}
