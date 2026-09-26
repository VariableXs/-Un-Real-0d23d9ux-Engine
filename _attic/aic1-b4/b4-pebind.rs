
// ---------------------------------------------------------------------------
// F003 · 深化批次四：会话缓存全局只读共享（零拷贝面）
//
// 主册依据（G-A-03【设计细节】）：「会话缓存只读共享以全局映射实现（零拷贝）」
// ——系统 DLL 符号表全进程共用一份，只读是安全语义：写请求恒拒绝（进程族
// 专属的写走 BindTable，不碰共享面）。
// ---------------------------------------------------------------------------

/// 会话共享缓存（全局映射的进程面模型：引用计数 + 只读强制）。
#[derive(Clone, Copy, Debug)]
pub struct SharedSessionCache {
    /// 当前映射引用数（全进程共用一份——多进程 attach 计数递增）。
    pub refs: u32,
    /// 共享映射是否在位。
    pub mapped: bool,
    /// 写请求拒绝计数（只读红线的审计面——每次如实计数）。
    pub write_refusals: u32,
}

impl SharedSessionCache {
    pub const fn new() -> SharedSessionCache {
        SharedSessionCache { refs: 0, mapped: false, write_refusals: 0 }
    }

    /// 进程 attach：首个引用建立全局只读映射；后续引用零拷贝（映射不重建）。
    pub fn attach(&mut self) -> u32 {
        self.refs += 1;
        self.mapped = true;
        self.refs
    }

    /// 进程 detach：引用归零 → 映射解除（资源零残留）。
    pub fn detach(&mut self) -> u32 {
        self.refs = self.refs.saturating_sub(1);
        if self.refs == 0 {
            self.mapped = false;
        }
        self.refs
    }

    /// 写请求：恒拒绝（只读共享——返回 false 并计数，不给「写成功」假象）。
    pub fn write_request(&mut self) -> bool {
        self.write_refusals += 1;
        false
    }
}

/// F003 深化批次四自检。
pub fn run_pebind_deep3_checks() -> CheckSet {
    let mut cs = CheckSet::new("F003-pebind-deep3");
    // 1) 多进程 attach：引用递增且映射不重建（零拷贝——一次映射多进程共用）。
    let mut sh = SharedSessionCache::new();
    let r1 = sh.attach();
    let r2 = sh.attach();
    cs.add(
        "shared_cache_zero_copy_attach",
        r1 == 1 && r2 == 2 && sh.mapped && sh.write_refusals == 0,
        "",
    );
    // 2) 只读强制：写请求恒拒绝并计数（不给假象）。
    let w1 = sh.write_request();
    let w2 = sh.write_request();
    cs.add(
        "shared_cache_readonly_enforced",
        !w1 && !w2 && sh.write_refusals == 2 && sh.mapped,
        "",
    );
    // 3) detach 全部引用 → 映射解除（资源零残留）；半途 detach 映射仍在。
    let r3 = sh.detach();
    let half = sh.mapped;
    let r4 = sh.detach();
    cs.add(
        "shared_cache_detach_lifecycle",
        r3 == 1 && half && r4 == 0 && !sh.mapped,
        "",
    );
    cs
}
