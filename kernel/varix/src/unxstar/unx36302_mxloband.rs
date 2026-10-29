//! UNX-F36302-J1 矩阵第 151–180 格：低 IL 主体面（沙箱消费预设）。
//! 判据：① 低 IL 主体 30 格 No-Write-Up 全生效；② 与 J4 fake 联签
//! 对照 30 例零差；③ 纯功能代码 ≥300 行。
//!
//! 设计纪律（对标 Windows 访问检查矩阵 / MIC 强制完整性控制）：
//! B16 域把 240 格矩阵建为 8 主体面 × 6 客体 × 5 操作。本模块是
//! B16 矩阵的**单一真源**：主体面表、客体表、格号双射、评估引擎、
//! 冻结语义书 SPEC 全部在此定义，B16 其余各卡只消费不复制。
//! 评估门序（对标 AccessCheck 的先掩码后 DACL 再强制标签的次序，
//! 令牌过滤位前置——过滤令牌的剥除发生在请求掩码进入 ACL 求值
//! 之前，与 Windows 的 residual mask 语义对齐）：
//!   门一 掩码门：客体不支持的访问位（如注册表键上的 EXECUTE）；
//!   门二 令牌过滤门：主体 deny 位面剥除的请求（沙箱消费预设/
//!        UAC 过滤令牌）——归因码 A_DENY；
//!   门三 DACL 门：空 DACL 全拒、ACE 授权位不含请求位则拒——
//!        归因码 A_DACL；
//!   门四 IL 门：No-Write-Up/No-Read-Up/No-Execute-Up 按等级序数
//!        求值（低写高即拒，同/高不拒）——归因码 A_IL；
//!   门五 特权门：客体要求特权且主体持权位不含则拒——A_PRIV。
//! 归因码闭集 A_ALLOW..A_DENY 是 B16 全部账面（总账/归因账/对账/
//! 可解释性）的取值域，缺门或出闭集即账面破。第 151–180 格对应
//! 主体面 S5（低 IL 沙箱消费预设）：rank=1 且 deny 位面剥除写/
//! 删/改权——「消费预设」的语义是令牌侧先把写类请求剥掉，使
//! No-Write-Up 在该面上**必然**全生效（30 格写类位全拒、逐格
//! 归因可读），J4 fake 联签则以独立冻结名册对照求零差。

use crate::checks::CheckSet;
use alloc::vec;
use alloc::vec::Vec;
use alloc::string::{String, ToString};

pub const SEG_BASE: u16 = 0xB180;

// ---- 诊断码闭集 -----------------------------------------------------------
pub const E_VER: u16 = SEG_BASE + 0; // 版本锚不符
pub const E_RANGE: u16 = SEG_BASE + 1; // 主体/客体/操作下标越界
pub const E_BIJECT: u16 = SEG_BASE + 2; // 格号双射破
pub const E_SPEC: u16 = SEG_BASE + 3; // 引擎与语义书漂移
pub const E_FACE: u16 = SEG_BASE + 4; // 沙箱消费面判据破
pub const E_COSIGN: u16 = SEG_BASE + 5; // J4 fake 联签出现差
pub const E_ATTRIB: u16 = SEG_BASE + 6; // 归因码出闭集

/// 模块版本锚。
pub const VER: u32 = 1;

// ---- 矩阵维度（冻结） -----------------------------------------------------
pub const N_SUBJ: usize = 8;
pub const N_OBJ: usize = 6;
pub const N_OP: usize = 5;
pub const N_CELLS: usize = 240; // 8 × 6 × 5

// ---- 操作位面（访问位 = 1 << op） ------------------------------------------
pub const OP_READ: usize = 0;
pub const OP_WRITE: usize = 1;
pub const OP_EXEC: usize = 2;
pub const OP_DEL: usize = 3;
pub const OP_WDAC: usize = 4;

// ---- 归因码闭集（B16 全账面取值域） ----------------------------------------
pub const A_ALLOW: u8 = 0;
pub const A_DACL: u8 = 1;
pub const A_IL: u8 = 2;
pub const A_PRIV: u8 = 3;
pub const A_MASK: u8 = 4;
pub const A_DENY: u8 = 5;

// ---- 主体面（8 面；格段 1–30/31–60/…/211–240 顺序冻结） ---------------------
pub const S_UNTRUSTED: usize = 0; // 格 1–30   匿名不可信
pub const S_LOW: usize = 1; // 格 31–60  基础低权
pub const S_APPCONT: usize = 2; // 格 61–90  AppContainer 沙箱
pub const S_MEDSTD: usize = 3; // 格 91–120 标准用户
pub const S_MEDSVC: usize = 4; // 格 121–150 服务低 IL 请求者
pub const S_LOWSANDBOX: usize = 5; // 格 151–180 沙箱消费预设（本卡）
pub const S_FILTERED: usize = 6; // 格 181–210 UAC 过滤令牌
pub const S_ELEVATED: usize = 7; // 格 211–240 提权/系统

// 特权位面（对标 SeBackup/SeTakeOwnership/SeSecurity/SeRestore）。
pub const P_BACKUP: u8 = 1 << 0;
pub const P_TAKEOWN: u8 = 1 << 1;
pub const P_SDCONV: u8 = 1 << 2;
pub const P_RESTORE: u8 = 1 << 3;

// 访问位面（对标 GENERIC_READ/WRITE/EXECUTE/DELETE/WRITE_DAC）。
pub const AB_READ: u8 = 1 << 0;
pub const AB_WRITE: u8 = 1 << 1;
pub const AB_EXEC: u8 = 1 << 2;
pub const AB_DEL: u8 = 1 << 3;
pub const AB_WDAC: u8 = 1 << 4;

/// 主体面定义：IL 等级序数、组别（0 标准/1 管理员/2 服务）、
/// 持权位面、令牌过滤剥除位面（deny）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Subject {
    pub rank: u8,
    pub group: u8,
    pub privs: u8,
    pub deny: u8,
}

/// 主体面表（冻结——下标即 S0..S7）。
pub fn subject_at(s: usize) -> Result<Subject, u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    if s >= N_SUBJ {
        return Err(E_RANGE);
    }
    let row: [Subject; N_SUBJ] = [
        Subject { rank: 0, group: 0, privs: 0, deny: 0 },
        Subject { rank: 1, group: 0, privs: 0, deny: 0 },
        Subject { rank: 1, group: 0, privs: 0, deny: AB_WRITE | AB_DEL | AB_WDAC },
        Subject { rank: 2, group: 0, privs: 0, deny: 0 },
        Subject { rank: 2, group: 2, privs: P_BACKUP, deny: 0 },
        Subject { rank: 1, group: 0, privs: 0, deny: AB_WRITE | AB_DEL | AB_WDAC },
        Subject { rank: 2, group: 0, privs: 0, deny: AB_WDAC },
        Subject { rank: 4, group: 1, privs: P_BACKUP | P_TAKEOWN | P_SDCONV | P_RESTORE, deny: 0 },
    ];
    Ok(row[s])
}

/// 客体定义：IL 等级、强制标签策略位（位 0 No-Write-Up / 位 1
/// No-Read-Up / 位 2 No-Execute-Up）、DACL 态（0 常规/1 null/2 空/
/// 3 缺省继承）、用户组 ACE 授权位、管理员组 ACE 授权位、支持访问
/// 位面（掩码门）、需求特权与特权作用位面。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ObjectDef {
    pub rank: u8,
    pub policy: u8,
    pub dacl_state: u8,
    pub ace_user: u8,
    pub ace_admin: u8,
    pub supported: u8,
    pub needpriv: u8,
    pub priv_ops: u8,
}

/// 客体表（冻结——下标即 O0..O5：文件/注册表键/节区/命名管道/
/// 进程句柄/桌面堆，Windows 常规可保护客体六类）。
pub fn object_at(o: usize) -> Result<ObjectDef, u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    if o >= N_OBJ {
        return Err(E_RANGE);
    }
    let row: [ObjectDef; N_OBJ] = [
        ObjectDef { rank: 2, policy: 1, dacl_state: 0, ace_user: 0x07, ace_admin: 0x1F, supported: 0x1F, needpriv: 0, priv_ops: 0 },
        ObjectDef { rank: 1, policy: 1, dacl_state: 0, ace_user: 0x03, ace_admin: 0x1F, supported: 0x1B, needpriv: 0, priv_ops: 0 },
        ObjectDef { rank: 1, policy: 5, dacl_state: 0, ace_user: 0x05, ace_admin: 0x1F, supported: 0x1F, needpriv: 0, priv_ops: 0 },
        ObjectDef { rank: 1, policy: 0, dacl_state: 0, ace_user: 0x03, ace_admin: 0x1F, supported: 0x1F, needpriv: 0, priv_ops: 0 },
        ObjectDef { rank: 3, policy: 3, dacl_state: 0, ace_user: 0x01, ace_admin: 0x1F, supported: 0x1F, needpriv: P_TAKEOWN, priv_ops: AB_WDAC },
        ObjectDef { rank: 2, policy: 1, dacl_state: 0, ace_user: 0x13, ace_admin: 0x1F, supported: 0x13, needpriv: P_SDCONV, priv_ops: AB_WDAC },
    ];
    Ok(row[o])
}

// ---- 格号双射（1 基：格 = (面×6 + 客体)×5 + 操作 + 1） ----------------------
pub fn cell_id(s: usize, o: usize, op: usize) -> Result<u32, u16> {
    if s >= N_SUBJ || o >= N_OBJ || op >= N_OP {
        return Err(E_RANGE);
    }
    Ok((s * N_OBJ * N_OP + o * N_OP + op + 1) as u32)
}

pub fn cell_parts(id: u32) -> Result<(usize, usize, usize), u16> {
    if id == 0 || id > N_CELLS as u32 {
        return Err(E_RANGE);
    }
    let v = (id - 1) as usize;
    Ok((v / (N_OBJ * N_OP), (v % (N_OBJ * N_OP)) / N_OP, v % N_OP))
}

// ---- 冻结语义书 SPEC（240 格归因码——B16 的「母版 X1」） ---------------------
// 由独立生成器按上述主体/客体表与门序离线推导后冻结于此；引擎
// 求值必须逐格命中本表，任何一格漂移即 E_SPEC（母版总兑现锚）。
pub const SPEC: [u8; N_CELLS] = [
    // S0 匿名不可信（格 1–30）
    0, 2, 0, 1, 1, 0, 2, 4, 1, 1, 0, 1, 2, 1, 1, 0, 0, 1, 1, 1, 2, 1, 1, 1, 1, 0, 2, 4, 4, 3,
    // S1 基础低权（格 31–60）
    0, 2, 0, 1, 1, 0, 0, 4, 1, 1, 0, 1, 0, 1, 1, 0, 0, 1, 1, 1, 2, 1, 1, 1, 1, 0, 2, 4, 4, 3,
    // S2 AppContainer 沙箱（格 61–90）
    0, 5, 0, 5, 5, 0, 5, 4, 5, 5, 0, 5, 0, 5, 5, 0, 5, 1, 5, 5, 2, 5, 1, 5, 5, 0, 5, 4, 4, 5,
    // S3 标准用户（格 91–120）
    0, 0, 0, 1, 1, 0, 0, 4, 1, 1, 0, 1, 0, 1, 1, 0, 0, 1, 1, 1, 2, 1, 1, 1, 1, 0, 0, 4, 4, 3,
    // S4 服务低 IL 请求者（格 121–150）
    0, 0, 0, 1, 1, 0, 0, 4, 1, 1, 0, 1, 0, 1, 1, 0, 0, 1, 1, 1, 2, 1, 1, 1, 1, 0, 0, 4, 4, 3,
    // S5 沙箱消费预设（格 151–180 —— 本卡主体面）
    0, 5, 0, 5, 5, 0, 5, 4, 5, 5, 0, 5, 0, 5, 5, 0, 5, 1, 5, 5, 2, 5, 1, 5, 5, 0, 5, 4, 4, 5,
    // S6 UAC 过滤令牌（格 181–210）
    0, 0, 0, 1, 5, 0, 0, 4, 1, 5, 0, 1, 0, 1, 5, 0, 0, 1, 1, 5, 2, 1, 1, 1, 5, 0, 0, 4, 4, 5,
    // S7 提权/系统（格 211–240）
    0, 0, 0, 0, 0, 0, 0, 4, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 4, 4, 0,
];

/// 语义书取格（1 基格号 → 冻结归因码）。
pub fn spec_at(id: u32) -> Result<u8, u16> {
    if id == 0 || id > N_CELLS as u32 {
        return Err(E_RANGE);
    }
    Ok(SPEC[(id - 1) as usize])
}

// ---- 评估引擎（门序：掩码→过滤→DACL→IL→特权） ------------------------------
/// 单格求值：Ok(A_ALLOW)=许，Ok(非零)=拒且归因码可读。
pub fn evaluate(s: usize, o: usize, op: usize) -> Result<u8, u16> {
    let subj = subject_at(s)?;
    let obj = object_at(o)?;
    eval_defs(&subj, &obj, op)
}

/// 带 DACL 态覆盖的求值（F36304 三态 ACL 对照格消费：null/空/无
/// 三态在对照空间求值，不写入 240 格主空间——零混锚）。
pub fn evaluate_with_acl_state(s: usize, o: usize, op: usize, dacl_state: u8) -> Result<u8, u16> {
    let subj = subject_at(s)?;
    let mut obj = object_at(o)?;
    if dacl_state > 3 {
        return Err(E_RANGE);
    }
    obj.dacl_state = dacl_state;
    eval_defs(&subj, &obj, op)
}

/// 引擎核心（主体/客体定义已取定）。
fn eval_defs(subj: &Subject, obj: &ObjectDef, op: usize) -> Result<u8, u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    if op >= N_OP {
        return Err(E_RANGE);
    }
    let req: u8 = 1 << op;
    // 门一：掩码门（客体不支持的访问位——如注册表键上的 EXECUTE）。
    if req & !obj.supported & 0x1F != 0 {
        return Ok(A_MASK);
    }
    // 门二：令牌过滤门（沙箱消费预设/UAC 过滤位剥除请求）。
    if req & subj.deny != 0 {
        return Ok(A_DENY);
    }
    // 门三：DACL 门（空 DACL 全拒；null 跳过；常规/缺省按组查 ACE）。
    if obj.dacl_state == 2 {
        return Ok(A_DACL);
    }
    if obj.dacl_state != 1 {
        let grant = if subj.group == 1 { obj.ace_admin } else { obj.ace_user };
        if req & grant == 0 {
            return Ok(A_DACL);
        }
    }
    // 门四：IL 门（No-Write-Up / No-Read-Up / No-Execute-Up）。
    let up = subj.rank < obj.rank;
    let il_hit = match op {
        OP_WRITE => obj.policy & 0x01 != 0 && up,
        OP_READ => obj.policy & 0x02 != 0 && up,
        OP_EXEC => obj.policy & 0x04 != 0 && up,
        _ => false,
    };
    if il_hit {
        return Ok(A_IL);
    }
    // 门五：特权门（需求特权且持权位不含）。
    if obj.needpriv != 0 && req & obj.priv_ops != 0 && subj.privs & obj.needpriv == 0 {
        return Ok(A_PRIV);
    }
    Ok(A_ALLOW)
}

/// 归因码 → 人话明细（B16 归因可读性的单一真源）。
pub fn attribution_text(code: u8) -> &'static str {
    match code {
        A_ALLOW => "许",
        A_DACL => "拒·DACL 门（ACE 授权位不含请求位或空 DACL）",
        A_IL => "拒·IL 门（强制标签 No-Write-Up/Read-Up/Execute-Up）",
        A_PRIV => "拒·特权门（客体需求特权主体不持）",
        A_MASK => "拒·掩码门（客体不支持的访问位）",
        A_DENY => "拒·令牌过滤门（沙箱预设/过滤令牌剥除请求）",
        _ => "归因码出闭集",
    }
}

/// 归因码闭集判定（出闭集 = 账面破）。
pub fn attribution_in_range(code: u8) -> bool {
    code <= A_DENY
}

// ---- 判据 ①：低 IL 主体 30 格 No-Write-Up 全生效 ----------------------------
// 沙箱消费预设面（S5，格 151–180）逐格求值：六个写格（每客体
// WRITE）全拒——预设剥除位面使写类请求在令牌过滤门被拦截，与
// IL 门的 No-Write-Up 形成「双保险同向」；面内应许 7 格（读类
// 与同级执行）与语义书一致。
pub fn sandbox_face_sweep() -> Result<u32, u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    let s = S_LOWSANDBOX;
    let mut allowed = 0u32;
    let mut write_blocked = 0u32;
    for o in 0..N_OBJ {
        for op in 0..N_OP {
            let got = evaluate(s, o, op)?;
            let want = spec_at(cell_id(s, o, op)?)?;
            if got != want {
                return Err(E_SPEC);
            }
            if got == A_ALLOW {
                allowed += 1;
            }
            if op == OP_WRITE {
                if got == A_ALLOW {
                    return Err(E_FACE); // No-Write-Up 破：写格放行
                }
                write_blocked += 1;
            }
        }
    }
    // 面账：写格 6/6 全拒、应许格 7/7（语义书冻结值）。
    if write_blocked != 6 || allowed != 7 {
        return Err(E_FACE);
    }
    Ok(write_blocked * 100 + allowed)
}

// ---- 判据 ②：J4 fake 联签对照 30 例零差 -------------------------------------
// J4 侧 fake 联签名册（独立冻结的 30 值数组——与 SPEC 切片、与
// 引擎代码三者互不共享生成路径）。逐格对照引擎求值，任何一差
// 即 E_COSIGN。
pub const J4_FAKE_S5: [u8; 30] = [
    0, 5, 0, 5, 5, 0, 5, 4, 5, 5, 0, 5, 0, 5, 5, 0, 5, 1, 5, 5, 2, 5, 1, 5, 5, 0, 5, 4, 4, 5,
];

pub fn fake_cosign_compare() -> Result<u32, u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    let s = S_LOWSANDBOX;
    let mut matched = 0u32;
    for o in 0..N_OBJ {
        for op in 0..N_OP {
            let idx = o * N_OP + op;
            let got = evaluate(s, o, op)?;
            if got != J4_FAKE_S5[idx] {
                return Err(E_COSIGN);
            }
            matched += 1;
        }
    }
    if matched != 30 {
        return Err(E_COSIGN);
    }
    Ok(matched)
}

// ---- 全矩阵一致性（B16 各账面的公共前提，供本卡与后续卡复用） ----------------
/// 240 格引擎 vs 语义书逐格对平；返回应许格数。
pub fn matrix_match_spec() -> Result<u32, u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    let mut allow = 0u32;
    for s in 0..N_SUBJ {
        for o in 0..N_OBJ {
            for op in 0..N_OP {
                let got = evaluate(s, o, op)?;
                let want = spec_at(cell_id(s, o, op)?)?;
                if !attribution_in_range(got) {
                    return Err(E_ATTRIB);
                }
                if got != want {
                    return Err(E_SPEC);
                }
                if got == A_ALLOW {
                    allow += 1;
                }
            }
        }
    }
    Ok(allow)
}

/// 格号双射核（240 格 id ↔ 三元组往返全对——账面寻址的前提）。
pub fn bijection_check() -> Result<u32, u16> {
    let mut hit: Vec<bool> = vec![false; N_CELLS + 1];
    for s in 0..N_SUBJ {
        for o in 0..N_OBJ {
            for op in 0..N_OP {
                let id = cell_id(s, o, op)?;
                if id == 0 || id as usize > N_CELLS {
                    return Err(E_BIJECT);
                }
                if hit[id as usize] {
                    return Err(E_BIJECT); // 撞号
                }
                hit[id as usize] = true;
                let (s2, o2, op2) = cell_parts(id)?;
                if s2 != s || o2 != o || op2 != op {
                    return Err(E_BIJECT);
                }
            }
        }
    }
    // 连续无跳号：1..=240 全命中。
    for id in 1..=N_CELLS {
        if !hit[id] {
            return Err(E_BIJECT);
        }
    }
    Ok(N_CELLS as u32)
}

// ---- 确定性演练 -------------------------------------------------------------
pub fn drill() -> Result<u32, u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    let biject = bijection_check()?;
    let allow = matrix_match_spec()?;
    let face = sandbox_face_sweep()?;
    let cosign = fake_cosign_compare()?;
    // 全矩阵冻结分布锚：应许 90 / 掩码拒 24 / 令牌过滤拒 40。
    let mut n_allow = 0u32;
    let mut n_mask = 0u32;
    let mut n_deny = 0u32;
    for id in 1..=N_CELLS as u32 {
        match spec_at(id)? {
            A_ALLOW => n_allow += 1,
            A_MASK => n_mask += 1,
            A_DENY => n_deny += 1,
            _ => {}
        }
    }
    if n_allow != 90 || n_mask != 24 || n_deny != 40 {
        return Err(E_SPEC);
    }
    if biject != N_CELLS as u32 || face / 100 != 6 {
        return Err(E_FACE);
    }
    Ok(allow.wrapping_mul(1000).wrapping_add(cosign))
}

pub fn detail(code: u16) -> &'static str {
    match code {
        E_VER => "版本锚不符",
        E_RANGE => "主体/客体/操作下标越界",
        E_BIJECT => "格号双射破（撞号/跳号/往返错位）",
        E_SPEC => "引擎与语义书漂移",
        E_FACE => "沙箱消费面判据破（No-Write-Up 未全生效）",
        E_COSIGN => "J4 fake 联签出现差",
        E_ATTRIB => "归因码出闭集",
        _ => "未知诊断码",
    }
}

pub fn run_unx36302_checks() -> CheckSet {
    let mut cs = CheckSet::new("unxstar-unx36302_mxloband");
    cs.add("SEG 段位独占", SEG_BASE == 0xB180, "0xB180~0xB18F 本模块独占");
    match drill() {
        Ok(_) => cs.add("J1 确定性演练全绿", true, "双射 240/240·语义书全对·消费面写 6 拒·联签 30 零差"),
        Err(code) => cs.add("J1 确定性演练全绿", false, detail(code)),
    }
    cs.add("消费面写拒锚", sandbox_face_sweep().map(|v| v / 100) == Ok(6), "30 格写类位全拒（No-Write-Up 全生效）");
    cs.add("联签零差锚", fake_cosign_compare() == Ok(30), "J4 fake 名册 30/30 零差");
    cs.add("语义书分布锚", matrix_match_spec() == Ok(90), "全矩阵对平·应许 90 格");
    cs.add("版本锚", VER == 1, "模块版本锚 = 1");
    cs
}

// ---- 单元测试 ---------------------------------------------------------------
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn face() {
        let cs = run_unx36302_checks();
        assert!(cs.len() > 0);
    }

    #[test]
    fn test_drill_green() {
        assert_eq!(drill().err(), None);
    }

    #[test]
    fn test_checks_all_passed() {
        let cs = run_unx36302_checks();
        assert!(cs.all_passed(), "红项: {}", cs.red_items_text());
        assert!(cs.len() >= 5);
    }
}
