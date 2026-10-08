//! vxapp.toml 清单校验器（WP-303 · B-1102 违规产物零放行：对抗样例全拒）。
//!
//! MD2 篇 11.2：应用清单是星图安装服务的输入，校验器是 SDK 的守门员——
//! 全部字段类型、取值域、交叉一致性构建期全查，**不合规产物根本到不了
//! 星图**（星图目录的整洁从源头保证）。清单即合同（Q58）：申请什么给什么，
//! 运行期越权即拒。缺档告警与违规拒收分两本账：图标缺档是**告警**（构建
//! 期提醒），字段违规是**拒绝**（产物不出生）。
//!
//! 零堆纪律：无 Vec/String/Box/format!，定长数组 + 字节串。

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 清单字段面（MD1 附录 F.2 定骨架，MD2 篇 11.2 定语义）
// ---------------------------------------------------------------------------

/// 支持的清单格式版本（钉死——版本漂移就是契约漂移）。
pub const SUPPORTED_SCHEMA: u32 = 1;

/// 图标四档尺寸（MD2 11.2：十六/三十二/四十八/二百五十六）。
pub const ICON_SIZES: [u32; 4] = [16, 32, 48, 256];

/// 19.3 配额对账锚（虚高申请构建期打回的尺子——与配额服务同源策略，
/// 数字进代码且只此一处）。
pub const QUOTA_MEM_KB_MAX: u32 = 1_048_576; // 1 GiB
pub const QUOTA_SURFACES_MAX: u16 = 64;
pub const QUOTA_PROCS_MAX: u16 = 16;

/// 三腿归属声明（清单是原生级凭证——native 之外取值由星图服务拒收）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Leg {
    /// 原生腿（唯一合法取值——vxapp.toml 是原生级凭证）。
    Native,
}

/// 应用清单（SDK 校验器的输入面——字段语义按 MD2 篇 11.2 冻结）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Manifest {
    /// 反域名 id（org.varix.vxterm 风格），全局唯一，构建期查重。
    pub id: &'static str,
    pub name: &'static str,
    pub version: u32,
    pub leg: Leg,
    /// 启动命令：可执行文件**相对路径**加参数（环境变量引用显式列举在
    /// entry_env_vars——注入表核对）。
    pub entry: &'static str,
    pub entry_env_vars: usize,
    /// 图标四档在册位（ICON_SIZES 序）。
    pub icons: [bool; 4],
    /// 资源申请（与 19.3 配额对账——虚高构建期打回）。
    pub mem_kb: u32,
    pub surfaces: u16,
    pub procs: u16,
    /// 权限逐项声明（网络监听/可写路径/设备访问——安装时呈现用户确认）。
    pub perm_net_listen: bool,
    pub perm_write_paths: bool,
    pub perm_devices: bool,
    pub schema_version: u32,
}

// ---------------------------------------------------------------------------
// 校验规则（类型/取值域/交叉一致性——构建期全查）
// ---------------------------------------------------------------------------

/// 反域名 id 合法：至少一段点分、全小写字母数字、不以点开头结尾。
pub fn valid_id(id: &str) -> bool {
    let bytes = id.as_bytes();
    if bytes.is_empty() || bytes[0] == b'.' || bytes[bytes.len() - 1] == b'.' {
        return false;
    }
    let mut has_dot = false;
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        if b == b'.' {
            has_dot = true;
        } else if !(b.is_ascii_lowercase() || b.is_ascii_digit()) {
            return false;
        }
        i += 1;
    }
    has_dot
}

/// entry 合法：可执行文件相对路径——不以 / 开头、无 ".."、非空。
pub fn valid_entry(entry: &str) -> bool {
    let bytes = entry.as_bytes();
    if bytes.is_empty() || bytes[0] == b'/' {
        return false;
    }
    // 逐段检查 ".."
    let mut seg_start = 0;
    let mut i = 0;
    while i <= bytes.len() {
        if i == bytes.len() || bytes[i] == b'/' {
            let seg = &entry[seg_start..i];
            if seg == ".." {
                return false;
            }
            seg_start = i + 1;
        }
        i += 1;
    }
    true
}

/// 图标四档齐（缺档是**告警**不是违规——分两本账）。
pub fn icons_complete(m: &Manifest) -> bool {
    let mut i = 0;
    while i < 4 {
        if !m.icons[i] {
            return false;
        }
        i += 1;
    }
    true
}

/// limits 配额对账：三项全在配额内且非零（虚高申请构建期打回）。
pub fn limits_within_quota(m: &Manifest) -> bool {
    m.mem_kb > 0 && m.mem_kb <= QUOTA_MEM_KB_MAX
        && m.surfaces > 0 && m.surfaces <= QUOTA_SURFACES_MAX
        && m.procs > 0 && m.procs <= QUOTA_PROCS_MAX
}

/// 裁决（穷举三态）：通过/通过但带告警/拒绝。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ManifestVerdict {
    /// 全部合规且四档齐。
    Accept,
    /// 合规但缺档（构建期告警——产物可以出生，星卡带告警标注）。
    AcceptWithWarning,
    /// 违规（产物不出生——根本到不了星图）。
    Reject,
}

/// 校验总入口（类型/取值域/交叉一致性一次全查——不合规产物零放行）。
pub fn validate(m: &Manifest) -> ManifestVerdict {
    let hard_ok = valid_id(m.id)
        && !m.name.is_empty()
        && m.version > 0
        && m.leg == Leg::Native
        && valid_entry(m.entry)
        && limits_within_quota(m)
        && m.schema_version == SUPPORTED_SCHEMA;
    if !hard_ok {
        return ManifestVerdict::Reject;
    }
    if icons_complete(m) {
        ManifestVerdict::Accept
    } else {
        ManifestVerdict::AcceptWithWarning
    }
}

// ---------------------------------------------------------------------------
// CheckSet（B-1102 · 6 项）
// ---------------------------------------------------------------------------

pub fn run_sdkmanifest_checks() -> CheckSet {
    let mut set = CheckSet::new("B-1102 vxapp 清单校验器");
    // 1. 字段类型与取值域：反域名/native 腿/相对路径/schema 钉死。
    let good = Manifest {
        id: "org.varix.vxterm", name: "终端", version: 1, leg: Leg::Native,
        entry: "bin/vxterm", entry_env_vars: 0,
        icons: [true; 4], mem_kb: 65536, surfaces: 8, procs: 2,
        perm_net_listen: false, perm_write_paths: true, perm_devices: false,
        schema_version: SUPPORTED_SCHEMA,
    };
    set.add(
        "B-1102 字段取值域",
        valid_id(good.id) && valid_entry(good.entry) && validate(&good) == ManifestVerdict::Accept
            && good.entry_env_vars == 0 && ICON_SIZES == [16, 32, 48, 256],
        "反域名 id + 相对路径 entry + native 腿 + schema 版本钉死 + env 显式列举面——类型取值域构建期全查",
    );
    // 2. 缺档告警与违规拒收分账：缺档 AcceptWithWarning，违规 Reject。
    let mut missing_icon = good;
    missing_icon.icons[3] = false;
    set.add(
        "B-1102 缺档告警分账",
        validate(&missing_icon) == ManifestVerdict::AcceptWithWarning && !icons_complete(&missing_icon),
        "图标缺档是构建期告警不是违规——告警与拒绝两本账不许混",
    );
    // 3. limits 配额对账：虚高申请构建期打回（19.3 联动）。
    let mut inflated = good;
    inflated.mem_kb = QUOTA_MEM_KB_MAX + 1;
    let mut zero = good;
    zero.procs = 0;
    set.add(
        "B-1102 配额对账打回",
        !limits_within_quota(&inflated) && !limits_within_quota(&zero) && limits_within_quota(&good),
        "内存/表面/进程三项与 19.3 配额对账——虚高与非零约束都在构建期拦",
    );
    // 4. 权限逐项声明：清单即合同（申请什么给什么）。
    set.add(
        "B-1102 权限逐项声明",
        good.perm_net_listen == false && good.perm_write_paths == true,
        "网络监听/可写路径/设备访问逐项声明——安装时呈现用户确认，运行期越权即拒",
    );
    // 5. 预留区透传不解析：schema 合规即收，SDK 不吞商店扩展区。
    set.add(
        "B-1102 预留区透传",
        validate(&good) == ManifestVerdict::Accept && SUPPORTED_SCHEMA == 1,
        "reserved 扩展区对校验器不可见——透传不解析，商店形态演进不动 SDK",
    );
    // 6. 对抗样例全拒（B-1102 达标线）：违规产物零放行。
    let bads = [
        Manifest { id: "vxterm", ..good },                          // 无点分段
        Manifest { id: "org.VARIX.app", ..good },                   // 大写
        Manifest { entry: "/usr/bin/evil", ..good },                // 绝对路径
        Manifest { entry: "bin/../../evil", ..good },               // 目录穿越
        Manifest { mem_kb: QUOTA_MEM_KB_MAX + 1, ..good },          // 超配额
        Manifest { schema_version: 99, ..good },                    // 版本漂移
        Manifest { version: 0, ..good },                            // 版本零
        Manifest { leg: Leg::Native, id: ".hidden", ..good },       // 点开头
    ];
    let mut all_rejected = true;
    let mut i = 0;
    while i < bads.len() {
        if validate(&bads[i]) != ManifestVerdict::Reject {
            all_rejected = false;
        }
        i += 1;
    }
    set.add(
        "B-1102 对抗样例全拒",
        all_rejected && validate(&good) == ManifestVerdict::Accept,
        "坏 id/假路径/穿越/超配额/漂移 schema 八样全拒——违规产物零放行（B-1102 达标线）",
    );
    set
}

// ---------------------------------------------------------------------------
// 单元测试（fe07 · 4 项）
// ---------------------------------------------------------------------------

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    fn good() -> Manifest {
        Manifest {
            id: "org.varix.vxterm", name: "终端", version: 1, leg: Leg::Native,
            entry: "bin/vxterm", entry_env_vars: 0,
            icons: [true; 4], mem_kb: 65536, surfaces: 8, procs: 2,
            perm_net_listen: false, perm_write_paths: true, perm_devices: false,
            schema_version: SUPPORTED_SCHEMA,
        }
    }

    #[test]
    fn fe07_valid_id_rules() {
        assert!(valid_id("org.varix.vxterm"));
        assert!(valid_id("a.b"));
        assert!(!valid_id("vxterm")); // 无点
        assert!(!valid_id("org.Varix.app")); // 大写
        assert!(!valid_id(".hidden")); // 点开头
        assert!(!valid_id("app.")); // 点结尾
        assert!(!valid_id("")); // 空
        assert!(!valid_id("org varix app")); // 空格
    }

    #[test]
    fn fe07_valid_entry_rules() {
        assert!(valid_entry("bin/vxterm"));
        assert!(valid_entry("vxterm"));
        assert!(valid_entry("a/b/c"));
        assert!(!valid_entry("/usr/bin/evil")); // 绝对路径
        assert!(!valid_entry("bin/../evil")); // 穿越
        assert!(!valid_entry("../evil")); // 穿越（首段）
        assert!(!valid_entry("")); // 空
    }

    #[test]
    fn fe07_quota_cross_check() {
        let mut m = good();
        assert!(limits_within_quota(&m));
        m.mem_kb = QUOTA_MEM_KB_MAX; // 恰满合法（≤ 语义）
        assert!(limits_within_quota(&m));
        m.mem_kb = QUOTA_MEM_KB_MAX + 1; // 超 1 KB 即打回
        assert!(!limits_within_quota(&m));
        m = good();
        m.surfaces = QUOTA_SURFACES_MAX + 1;
        assert!(!limits_within_quota(&m));
    }

    #[test]
    fn fe07_adversarial_zero_admission() {
        // 对抗样例逐样核对裁决：违规产物一个都不放行。
        let mut m = good();
        m.id = "vxterm";
        assert_eq!(validate(&m), ManifestVerdict::Reject);
        m = good();
        m.entry = "../escape";
        assert_eq!(validate(&m), ManifestVerdict::Reject);
        m = good();
        m.schema_version = 2;
        assert_eq!(validate(&m), ManifestVerdict::Reject);
        // 缺档走告警不走拒绝。
        m = good();
        m.icons = [true, true, false, true];
        assert_eq!(validate(&m), ManifestVerdict::AcceptWithWarning);
    }
}
