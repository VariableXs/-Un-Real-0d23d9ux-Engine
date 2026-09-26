
// ---------------------------------------------------------------------------
// F009 · 深化批次六：注册表值类型标记面（REG_SZ/DWORD/BINARY 语义层）
//
// 主册依据（G-A-09【功能定义】）：「应用视角的 HKCU/HKLM 读写」——Windows
// 注册表值有类型（REG_SZ/REG_DWORD/REG_BINARY…）；自有蜂巢字节值之上的
// **类型语义层**：类型标记 + DWORD 字节序语义（小端 4 字节）。
// ---------------------------------------------------------------------------

/// 值类型（Windows 注册表常用三型——50 件采样覆盖面）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ValType {
    Sz,
    Dword,
    Binary,
}

/// 类型标记表（键 → 类型；容量 32——键树视图显示类型徽标的数据源）。
pub struct ValTypeTable {
    keys: [(u64, ValType); 32],
    n: usize,
}

impl ValTypeTable {
    pub const fn new() -> ValTypeTable {
        ValTypeTable { keys: [(0, ValType::Binary); 32], n: 0 }
    }

    /// 标记（重复标记更新不占双槽——幂等）。
    pub fn tag(&mut self, key_hash: u64, t: ValType) -> bool {
        if let Some((_, existing)) = self.keys[..self.n].iter_mut().find(|(k, _)| *k == key_hash) {
            *existing = t;
            return true;
        }
        if self.n >= self.keys.len() {
            return false;
        }
        self.keys[self.n] = (key_hash, t);
        self.n += 1;
        true
    }

    pub fn type_of(&self, key_hash: u64) -> Option<ValType> {
        self.keys[..self.n].iter().find(|(k, _)| *k == key_hash).map(|(_, t)| *t)
    }
}

/// DWORD 值读（REG_DWORD：小端 4 字节；长度 ≠4 → None——类型语义不猜）。
pub fn dword_value(val: &[u8]) -> Option<u32> {
    if val.len() != 4 {
        return None;
    }
    Some(u32::from_le_bytes([val[0], val[1], val[2], val[3]]))
}

/// F009 深化批次六自检。
pub fn run_reghive_deep5_checks() -> CheckSet {
    let mut cs = CheckSet::new("F009-reghive-deep5");
    // 1) 类型标记：标记/查询/更新幂等（同键改型不占双槽）。
    let mut tt = ValTypeTable::new();
    tt.tag(0xA1, ValType::Sz);
    tt.tag(0xA2, ValType::Dword);
    tt.tag(0xA1, ValType::Binary);
    cs.add(
        "valtype_tag_update_idempotent",
        tt.type_of(0xA1) == Some(ValType::Binary) && tt.type_of(0xA2) == Some(ValType::Dword),
        "",
    );
    // 2) DWORD 语义：小端 4 字节读出；长度不符如实 None。
    cs.add(
        "dword_value_little_endian",
        dword_value(&[0x39, 0x05, 0x00, 0x00]) == Some(1337)
            && dword_value(&[0x01, 0x02]).is_none()
            && dword_value(&[1, 2, 3, 4, 5]).is_none(),
        "",
    );
    // 3) 未标记键如实 None（不猜缺省类型——诚实边界）。
    cs.add("valtype_untagged_none", ValTypeTable::new().type_of(0xA1).is_none(), "");
    cs
}
