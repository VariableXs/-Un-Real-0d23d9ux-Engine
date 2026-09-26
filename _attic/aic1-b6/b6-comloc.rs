
// ---------------------------------------------------------------------------
// F019 · 深化批次六：ProgID ↔ CLSID 查询面（COM 激活的名称入口）
//
// 主册依据（G-A-19【功能定义】）：「进程内 COM（CoCreateInstance 类 ID 注册
// 表 = F009 蜂巢的 CLSID 子树）」——ProgID（人读名）→ CLSID 查询是激活的
// 第一步：注册表里的 ProgID 映射面（会话级）。
// ---------------------------------------------------------------------------

/// ProgID 映射表（会话级——容量 16）。
pub struct ProgIdMap {
    pairs: [Option<(&'static str, Clsid)>; 16],
    n: usize,
}

impl ProgIdMap {
    pub const fn new() -> ProgIdMap {
        ProgIdMap { pairs: [None; 16], n: 0 }
    }

    /// 注册 ProgID → CLSID（同名重注册更新不占双槽——幂等）。
    pub fn register(&mut self, progid: &'static str, clsid: Clsid) -> bool {
        if let Some(slot) = self.pairs[..self.n].iter_mut().find(|p| matches!(p, Some((name, _)) if *name == progid)) {
            *slot = Some((progid, clsid));
            return true;
        }
        if self.n >= self.pairs.len() {
            return false;
        }
        self.pairs[self.n] = Some((progid, clsid));
        self.n += 1;
        true
    }

    /// 查询（未注册 → None——结构化错误由激活面给三要素）。
    pub fn lookup(&self, progid: &str) -> Option<Clsid> {
        self.pairs[..self.n]
            .iter()
            .flatten()
            .find(|(name, _)| *name == progid)
            .map(|(_, c)| *c)
    }

    pub fn len(&self) -> usize {
        self.n
    }
}

/// F019 深化批次六自检。
pub fn run_comloc_deep5_checks() -> CheckSet {
    let mut cs = CheckSet::new("F019-comloc-deep5");
    // 1) 注册/查询往返：ShellLink 的标准 ProgID。
    let mut pm = ProgIdMap::new();
    let ok = pm.register("Shell.Link", Clsid::ShellLink);
    cs.add(
        "progid_register_lookup",
        ok && pm.lookup("Shell.Link") == Some(Clsid::ShellLink),
        "",
    );
    // 2) 同名重注册更新（幂等不占双槽）；未注册如实 None。
    let upd = pm.register("Shell.Link", Clsid::AppActivation);
    let none = pm.lookup("No.Such");
    cs.add(
        "progid_update_and_missing_honest",
        upd && pm.lookup("Shell.Link") == Some(Clsid::AppActivation) && pm.len() == 1 && none.is_none(),
        "",
    );
    // 3) 满容如实拒（16 槽账本纪律）。
    let mut pm2 = ProgIdMap::new();
    let mut all = true;
    for i in 0..16u32 {
        // 同名重注册不算新槽——用不同名填满。
        let name: &'static str = match i {
            0 => "p0", 1 => "p1", 2 => "p2", 3 => "p3", 4 => "p4", 5 => "p5", 6 => "p6",
            7 => "p7", 8 => "p8", 9 => "p9", 10 => "p10", 11 => "p11", 12 => "p12",
            13 => "p13", 14 => "p14", _ => "p15",
        };
        all &= pm2.register(name, Clsid::DropSource);
    }
    let overflow = pm2.register("p16", Clsid::DropSource);
    cs.add("progid_cap_honest", all && !overflow && pm2.len() == 16, "");
    cs
}
