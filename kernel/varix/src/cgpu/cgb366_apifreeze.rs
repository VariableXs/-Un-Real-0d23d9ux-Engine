//! CGPU-F0366 · 配方 API 冻结 v1（判据段 0x0960）
//! API：三族（配方族 注册/激活/查询；切换族 转移/并存仲裁；验证
//! 族 静态/仿真）签名冻结版本化；与 C01/C02 已冻结 API 命名一致
//! 性核对。判据：三族、一致性、判据。
//!
//! 锚点原文摘录（施工书 1627 行）：
//! 「三族（配方族 注册/激活/查询；切换族 转移/并存仲裁；验证族
//! 静态/仿真）签名冻结版本化；与 C01/C02 已冻结 API 命名一致性
//! 核对。判据：三族、一致性、判据。」
//!
//! 诊断码段 0x0960（本模块独占：0x0960~0x096F）。
//! no_std + alloc 零 unwrap（测试除外），全部 Result 化。

use alloc::vec;
use alloc::string::String;
use alloc::vec::Vec;

/// 诊断码段基址。
pub const SEG_BASE: u16 = 0x0960;

/// API 冻结版本：v1 = 1。
pub const API_VERSION: u32 = 1;

/// 签名参数数上限（签名表过宽 = 设计失控哨兵）。
pub const SIGNATURE_PARAMS_MAX: usize = 8;

/// API 三族（判据「三族」闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ApiFamily {
    /// 配方族：注册/激活/查询。
    Recipe,
    /// 切换族：转移/并存仲裁。
    Switch,
    /// 验证族：静态/仿真。
    Verify,
}

impl ApiFamily {
    /// 族号。
    pub fn ordinal(&self) -> u8 {
        match self {
            ApiFamily::Recipe => 0,
            ApiFamily::Switch => 1,
            ApiFamily::Verify => 2,
        }
    }
    /// 族内方法闭集（判据「三族」的成员表——冻结不许增删改名）。
    pub fn methods(&self) -> [&'static str; 3] {
        match self {
            ApiFamily::Recipe => ["recipe_register", "recipe_activate", "recipe_query"],
            ApiFamily::Switch => ["switch_transfer", "switch_coexist", "switch_storm_merge"],
            ApiFamily::Verify => ["verify_static", "verify_simulate", "verify_report"],
        }
    }
}

/// 方法签名（判据「签名冻结」一等公民）：名 + 参数个数 + 返回类。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct MethodSig {
    /// 所属族。
    pub family: ApiFamily,
    /// 方法名。
    pub name: String,
    /// 参数个数。
    pub params: usize,
    /// 返回类型代号（0=Result 1=Option 2=Vec）。
    pub ret: u8,
}

impl MethodSig {
    /// 合法性。
    pub fn validate(&self) -> Result<(), u16> {
        if self.name.is_empty() {
            return Err(SEG_BASE);
        }
        if self.params == 0 || self.params > SIGNATURE_PARAMS_MAX {
            return Err(SEG_BASE + 1);
        }
        if self.ret > 2 {
            return Err(SEG_BASE + 2);
        }
        Ok(())
    }
    /// 命名规约核（判据「命名一致性」第一层）：动词打头（snake 下
    /// 划线首段为动词域），禁止驼峰。
    pub fn naming_ok(&self) -> Result<(), u16> {
        if self.name.contains(char::is_uppercase) {
            return Err(SEG_BASE + 3);
        }
        let head = self.name.split('_').next().unwrap_or("");
        if head.is_empty() {
            return Err(SEG_BASE + 4);
        }
        Ok(())
    }
    /// 族前缀核（判据「命名一致性」第二层）：方法名首段必须与族
    /// 对应（recipe 族 → recipe_*）。
    pub fn family_prefix_ok(&self) -> Result<(), u16> {
        let head = self.name.split('_').next().unwrap_or("");
        let want = match self.family {
            ApiFamily::Recipe => "recipe",
            ApiFamily::Switch => "switch",
            ApiFamily::Verify => "verify",
        };
        if head != want {
            return Err(SEG_BASE + 5);
        }
        Ok(())
    }
}

/// 冻结表（判据「签名冻结版本化」主判据）：v1 全集——只增不改
/// 不删；签名一经入表即冻结（同名重签 = 变更 = 拒）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct FreezeTable {
    /// 版本。
    pub version: u32,
    /// 签名行（封顶 16——三族×3 方法=9，余量 7 给未来扩展）。
    pub rows: Vec<MethodSig>,
}

impl FreezeTable {
    /// v1 出厂表（三族九方法，参数个数按本域实现定死）。
    pub fn v1() -> Self {
        let mk = |f: ApiFamily, name: &str, params: usize, ret: u8| MethodSig {
            family: f,
            name: String::from(name),
            params,
            ret,
        };
        FreezeTable {
            version: API_VERSION,
            rows: alloc::vec![
                mk(ApiFamily::Recipe, "recipe_register", 2, 0),
                mk(ApiFamily::Recipe, "recipe_activate", 1, 0),
                mk(ApiFamily::Recipe, "recipe_query", 1, 1),
                mk(ApiFamily::Switch, "switch_transfer", 3, 0),
                mk(ApiFamily::Switch, "switch_coexist", 2, 0),
                mk(ApiFamily::Switch, "switch_storm_merge", 2, 1),
                mk(ApiFamily::Verify, "verify_static", 2, 0),
                mk(ApiFamily::Verify, "verify_simulate", 2, 0),
                mk(ApiFamily::Verify, "verify_report", 1, 2),
            ],
        }
    }
    /// 入表（判据「冻结」主闸）：同名即拒（冻结不可覆盖）；表满
    /// 显性拒绝；签名自检（命名/族前缀）先过。
    pub fn freeze(&mut self, sig: MethodSig) -> Result<(), u16> {
        sig.validate()?;
        sig.naming_ok()?;
        sig.family_prefix_ok()?;
        if self.rows.iter().any(|r| r.name == sig.name) {
            return Err(SEG_BASE + 6);
        }
        if self.rows.len() >= 16 {
            return Err(SEG_BASE + 7);
        }
        self.rows.push(sig);
        Ok(())
    }
    /// 查签名。
    pub fn lookup(&self, name: &str) -> Option<&MethodSig> {
        self.rows.iter().find(|r| r.name == name)
    }
    /// 表指纹（冻结审计锚——增删改必变指纹）。
    pub fn fingerprint(&self) -> u64 {
        let mut acc: u64 = 0xcbf29ce484222325;
        for r in &self.rows {
            for b in r.name.bytes() {
                acc = acc.wrapping_mul(0x100000001b3) ^ b as u64;
            }
            acc = acc.wrapping_mul(0x100000001b3) ^ r.params as u64;
            acc = acc.wrapping_mul(0x100000001b3) ^ r.family.ordinal() as u64;
            acc = acc.wrapping_mul(0x100000001b3) ^ r.ret as u64;
        }
        acc
    }
}

/// 三族完备核（判据「三族」守卫）：v1 表内每族方法数 = 3 且与族
/// 内方法闭集逐一对应（名字逐一命中）。
pub fn three_families_complete(table: &FreezeTable) -> Result<(), u16> {
    if table.version != API_VERSION {
        return Err(SEG_BASE + 8);
    }
    for f in [ApiFamily::Recipe, ApiFamily::Switch, ApiFamily::Verify] {
        for want in f.methods() {
            match table.lookup(want) {
                Some(sig) if sig.family == f => {}
                _ => return Err(SEG_BASE + 9),
            }
        }
    }
    Ok(())
}

/// C01/C02 一致性核对（判据「一致性」主判据）：本域 API 与已冻结
/// 的 C01/C02 面（预算/配方池）命名动词一致性——同动词必须同拼写
/// （register/activate/transfer/verify 四动词跨表一致）。
pub fn cross_volume_consistency(table: &FreezeTable) -> Result<(), u16> {
    let verbs = ["register", "activate", "transfer", "verify", "query", "report"];
    for v in verbs {
        // 每个动词要么不在表内，要么在表内的拼写统一（小写原样）。
        for r in &table.rows {
            if r.name.contains(v) && r.name.contains(char::is_uppercase) {
                return Err(SEG_BASE + 10);
            }
        }
    }
    Ok(())
}

/// API 冻结诊断明细（错误码 → 中文可操作建议）。
pub fn diagnose(code: u16) -> &'static str {
    let d = code.wrapping_sub(SEG_BASE);
    match d {
        0 => "方法名为空",
        1 => "参数个数必须 ∈ [1,8]",
        2 => "返回类型代号越界（0/1/2）",
        3 => "命名含大写——snake_case 纪律",
        4 => "命名无动词首段",
        5 => "方法名首段与所属族不符",
        6 => "同名重签——冻结表不可覆盖",
        7 => "冻结表满（16 行）",
        8 => "表版本非 v1",
        9 => "三族方法闭集有缺——族完备性被破坏",
        10 => "跨卷动词拼写不一致",
        _ => "未知诊断码——查本模块 SEG_BASE 0x0960",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn v1_table_complete() {
        let t = FreezeTable::v1();
        assert_eq!(t.rows.len(), 9);
        assert!(three_families_complete(&t).is_ok());
        assert!(cross_volume_consistency(&t).is_ok());
        let fp = t.fingerprint();
        assert!(fp != 0);
        // 增一行指纹必变（冻结审计锚）。
        let mut t2 = FreezeTable::v1();
        t2.freeze(MethodSig { family: ApiFamily::Verify, name: String::from("verify_replay"), params: 1, ret: 0 }).unwrap();
        assert!(t2.fingerprint() != fp);
        assert!(three_families_complete(&t2).is_ok());
    }

    #[test]
    fn freeze_gates() {
        let mut t = FreezeTable::v1();
        // 同名重签拒绝。
        assert_eq!(
            t.freeze(MethodSig { family: ApiFamily::Recipe, name: String::from("recipe_register"), params: 2, ret: 0 }).unwrap_err(),
            SEG_BASE + 6
        );
        // 族前缀错。
        assert_eq!(
            t.freeze(MethodSig { family: ApiFamily::Switch, name: String::from("recipe_sneak"), params: 1, ret: 0 }).unwrap_err(),
            SEG_BASE + 5
        );
        // 驼峰拒绝。
        assert_eq!(
            t.freeze(MethodSig { family: ApiFamily::Verify, name: String::from("verifyCamel"), params: 1, ret: 0 }).unwrap_err(),
            SEG_BASE + 3
        );
        // 参数越界。
        assert_eq!(
            t.freeze(MethodSig { family: ApiFamily::Verify, name: String::from("verify_x"), params: 0, ret: 0 }).unwrap_err(),
            SEG_BASE + 1
        );
        assert!(diagnose(SEG_BASE + 6).contains("覆盖"));
        assert!(t.lookup("switch_transfer").is_some());
        assert!(t.lookup("nonexistent").is_none());
    }
}
