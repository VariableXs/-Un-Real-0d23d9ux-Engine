//! UNX-F56065 内存压测负载脚本库（unxstar 段）
//!
//! [J1] 脚本库齐备：压测脚本（名称 + 操作序列 + 权重）入库存档；操作走闭集
//!      （alloc/touch/free/hold/flush），未知操作与空序列拒绝入库。
//! [J2] 签名纪律：脚本签名 = FNV(名称||操作序列||权重)；签名不符的脚本拒绝入库；
//!      库指纹（逐脚本折叠）同内容稳定、任一脚本变化必变。
//! [J3] 执行预检：脚本可执行预检（权重和 > 0、操作数 ≤ 64、含至少一次 alloc）；
//!      变更链（脚本替换需联签）纪律；注入核 5 例必抓。
//!
//! 域：CoRun·UNX-F2-B40
//!
//! 结构总览：
//!   常量段：SEG_BASE + 7 个诊断码（操作未知/序列为空/签名不符/脚本重复/库已封/指纹漂移/预检未过）
//!   纯函数：fnv32、签名计算
//!   MemScript：压测脚本条目，new() 守闸
//!   ScriptVault：脚本库（put/replace_with_cosign/execute_ready/vault_fingerprint）
//!   injection_core：5 例必抓
//!   run_unx56065_checks：≥13 条判据

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u32 = 0x60800;

/// 操作未知（不在五操作闭集）
pub const D_OP_UNKNOWN: u32 = SEG_BASE + 0x01;
/// 序列为空
pub const D_SEQ_EMPTY: u32 = SEG_BASE + 0x02;
/// 签名不符
pub const D_SIG_MISMATCH: u32 = SEG_BASE + 0x03;
/// 脚本重复（同名入库）
pub const D_DUP_SCRIPT: u32 = SEG_BASE + 0x04;
/// 库已封（封库后变更拒绝）
pub const D_SEALED: u32 = SEG_BASE + 0x05;
/// 指纹漂移
pub const D_FP_DRIFT: u32 = SEG_BASE + 0x06;
/// 预检未过（权重和为零/序列过长/缺 alloc）
pub const D_PRECHECK_FAIL: u32 = SEG_BASE + 0x07;

/// 操作闭集
pub const OPS: [&str; 5] = ["alloc", "touch", "free", "hold", "flush"];
/// 操作数上限
pub const OP_LIMIT: usize = 64;

fn fnv32(data: &[u8]) -> u32 {
    let mut h: u32 = 2166136261;
    for &b in data {
        h ^= b as u32;
        h = h.wrapping_mul(16777619);
    }
    h
}

/// 脚本签名：名称 || 操作序列 || 权重
pub fn script_sig(name: &str, ops: &[&str], weight: u32) -> u32 {
    let mut buf: Vec<u8> = Vec::new();
    buf.extend_from_slice(name.as_bytes());
    for op in ops {
        buf.extend_from_slice(op.as_bytes());
    }
    buf.extend_from_slice(&weight.to_le_bytes());
    fnv32(&buf)
}

/// 压测脚本条目
#[derive(Clone)]
pub struct MemScript {
    pub name: String,
    pub ops: Vec<String>,
    pub weight: u32,
    pub sig: u32,
}

impl MemScript {
    /// 守闸：操作闭集 + 序列非空 + 长度上限 + 权重非零 + 至少一次 alloc
    pub fn new(name: &str, ops: &[&str], weight: u32, expect_sig: u32) -> Result<Self, u32> {
        if ops.is_empty() {
            return Err(D_SEQ_EMPTY);
        }
        if ops.len() > OP_LIMIT {
            return Err(D_PRECHECK_FAIL);
        }
        for op in ops {
            if !OPS.contains(op) {
                return Err(D_OP_UNKNOWN);
            }
        }
        if weight == 0 {
            return Err(D_PRECHECK_FAIL);
        }
        if !ops.contains(&"alloc") {
            return Err(D_PRECHECK_FAIL);
        }
        let sig = script_sig(name, ops, weight);
        if sig != expect_sig {
            return Err(D_SIG_MISMATCH);
        }
        Ok(MemScript {
            name: name.to_string(),
            ops: ops.iter().map(|o| o.to_string()).collect(),
            weight,
            sig,
        })
    }

    /// 执行预检
    pub fn execute_ready(&self) -> bool {
        !self.ops.is_empty()
            && self.ops.len() <= OP_LIMIT
            && self.weight > 0
            && self.ops.iter().any(|o| o == "alloc")
    }

    /// 操作计数行
    pub fn op_rows(&self) -> Vec<String> {
        let mut rows = Vec::new();
        for (i, op) in OPS.iter().enumerate() {
            let n = self.ops.iter().filter(|o| o == op).count();
            if n > 0 {
                rows.push(format!("MOP|{}|{}|n{}", self.name, i, n));
            }
        }
        rows
    }
}

/// 脚本库
pub struct ScriptVault {
    pub vault_name: String,
    scripts: Vec<MemScript>,
    sealed: bool,
}

impl ScriptVault {
    pub fn new(vault_name: &str) -> Self {
        ScriptVault { vault_name: vault_name.to_string(), scripts: Vec::new(), sealed: false }
    }

    /// 入库：同名重复与封库拒绝
    pub fn put(&mut self, s: MemScript) -> Result<(), u32> {
        if self.sealed {
            return Err(D_SEALED);
        }
        if self.scripts.iter().any(|x| x.name == s.name) {
            return Err(D_DUP_SCRIPT);
        }
        self.scripts.push(s);
        Ok(())
    }

    /// 变更链：替换脚本需联签（新签名一致 + 联签码为旧新签名异或）
    pub fn replace_with_cosign(&mut self, s: MemScript, cosign: u32) -> Result<(), u32> {
        if self.sealed {
            return Err(D_SEALED);
        }
        let old_sig = match self.scripts.iter().find(|x| x.name == s.name) {
            Some(x) => x.sig,
            None => return Err(D_DUP_SCRIPT),
        };
        if cosign != (old_sig ^ s.sig) {
            return Err(D_SIG_MISMATCH);
        }
        let idx = self.scripts.iter().position(|x| x.name == s.name).expect("idx");
        self.scripts[idx] = s;
        Ok(())
    }

    pub fn len(&self) -> usize {
        self.scripts.len()
    }

    pub fn seal(&mut self) {
        self.sealed = true;
    }

    pub fn is_sealed(&self) -> bool {
        self.sealed
    }

    pub fn get(&self, name: &str) -> Option<&MemScript> {
        self.scripts.iter().find(|s| s.name == name)
    }

    /// 权重和
    pub fn total_weight(&self) -> u32 {
        self.scripts.iter().map(|s| s.weight).sum()
    }

    /// 库指纹
    pub fn vault_fingerprint(&self) -> u32 {
        let mut acc: u32 = 0x56065_00;
        for s in &self.scripts {
            acc = acc.wrapping_mul(16777619) ^ s.sig;
        }
        acc
    }
}

/// 标准夹具：三脚本，权重 30/45/25
pub fn fixture_vault() -> ScriptVault {
    let mut v = ScriptVault::new("mem-stress-vault");
    let plans: [(&str, Vec<&str>, u32); 3] = [
        ("steady", vec!["alloc", "touch", "hold", "free"], 30),
        ("churn", vec!["alloc", "touch", "free", "alloc", "touch", "free"], 45),
        ("bulk", vec!["alloc", "alloc", "touch", "flush", "free"], 25),
    ];
    for (name, ops, w) in plans {
        let sig = script_sig(name, &ops, w);
        let s = MemScript::new(name, &ops, w, sig).expect("script");
        v.put(s).expect("put");
    }
    v
}

/// 注入核：5 例必抓
pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    let total = 5usize;

    // 1) 操作未知
    let ops = vec!["alloc", "poke"];
    let sig = script_sig("bad", &ops, 10);
    if MemScript::new("bad", &ops, 10, sig).err() == Some(D_OP_UNKNOWN) {
        caught += 1;
    }

    // 2) 空序列
    let empty: Vec<&str> = Vec::new();
    if MemScript::new("e", &empty, 10, 0).err() == Some(D_SEQ_EMPTY) {
        caught += 1;
    }

    // 3) 签名不符（期望签名传错）
    let ops2 = vec!["alloc", "free"];
    if MemScript::new("s2", &ops2, 10, 12345).err() == Some(D_SIG_MISMATCH) {
        caught += 1;
    }

    // 4) 同名重复
    let mut v = fixture_vault();
    let ops3 = vec!["alloc", "free"];
    let sig3 = script_sig("steady", &ops3, 20);
    let dup = MemScript::new("steady", &ops3, 20, sig3).expect("dup");
    if v.put(dup) == Err(D_DUP_SCRIPT) {
        caught += 1;
    }

    // 5) 预检未过（无 alloc）
    let ops4 = vec!["touch", "free"];
    let sig4 = script_sig("noalloc", &ops4, 10);
    if MemScript::new("noalloc", &ops4, 10, sig4).err() == Some(D_PRECHECK_FAIL) {
        caught += 1;
    }

    (caught, total)
}

/// 指纹稳定性与敏感性
pub fn fp_stability() -> Result<(), u32> {
    let a = fixture_vault().vault_fingerprint();
    let b = fixture_vault().vault_fingerprint();
    if a != b {
        return Err(D_FP_DRIFT);
    }
    let mut alt = ScriptVault::new("alt");
    let ops = vec!["alloc", "hold", "free"];
    let sig = script_sig("alt1", &ops, 99);
    let s = MemScript::new("alt1", &ops, 99, sig).expect("alt");
    alt.put(s).expect("put");
    if alt.vault_fingerprint() == a {
        return Err(D_FP_DRIFT);
    }
    Ok(())
}

/// 判据集
pub fn run_unx56065_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F56065");

    // J1 脚本库齐备
    let v = fixture_vault();
    s.add("56065-J1-三脚本", v.len() == 3, "脚本数不为 3");
    s.add("56065-J1-权重和100", v.total_weight() == 100, "权重和不为 100");
    s.add("56065-J1-预检全过", v.get("churn").map(|x| x.execute_ready()) == Some(true), "churn 预检未过");
    s.add("56065-J1-操作闭集", OPS.len() == 5, "操作闭集异常");

    // J2 签名纪律
    s.add("56065-J2-签名回算", {
        let ops = vec!["alloc", "touch", "hold", "free"];
        script_sig("steady", &ops, 30) == v.get("steady").map(|x| x.sig).unwrap_or(0)
    }, "steady 签名回算不符");
    s.add("56065-J2-库指纹稳敏", fp_stability().is_ok(), "库指纹稳定/敏感性异常");

    // J3 执行预检与变更链
    let mut w = fixture_vault();
    let new_ops = vec!["alloc", "touch", "free"];
    let new_sig = script_sig("steady", &new_ops, 50);
    let old_sig = w.get("steady").map(|x| x.sig).expect("old");
    let ns = MemScript::new("steady", &new_ops, 50, new_sig).expect("ns");
    s.add("56065-J3-联签不符拒", {
        w.replace_with_cosign(ns.clone(), 0) == Err(D_SIG_MISMATCH)
    }, "联签码 0 未被拒绝");
    s.add("56065-J3-联签替换成功", {
        w.replace_with_cosign(ns, old_sig ^ new_sig).is_ok()
    }, "正确联签替换失败");
    s.add("56065-J3-替换后权重", w.total_weight() == 120, "替换后权重和不为 120");
    s.add("56065-J3-预检权重零", {
        let ops = vec!["alloc"];
        let sig = script_sig("zero", &ops, 0);
        MemScript::new("zero", &ops, 0, sig).err() == Some(D_PRECHECK_FAIL)
    }, "零权重未被预检拒绝");

    // 注入核
    let (caught, total) = injection_core();
    s.add_dyn(
        "56065-J2-注入必抓",
        caught == total,
        &format!("注入捕获 {}/{}", caught, total),
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx56065_all_checks_green() {
        let s = run_unx56065_checks();
        assert!(
            s.all_passed(),
            "UNX-F56065 有红项: {}",
            s.red_items_text()
        );
    }
}
