//! probe_w5 库壳：四模块以 pub 形式挂载（bin 直挂会触发死码误报，
//! lib 的 pub 项视为可达， warnings 归零且不改动模块本身）。
extern crate alloc;

pub mod checks;

#[path = "../../../../varix/src/unxstar/unx12754_seh_doc_align.rs"]
pub mod unx12754;
#[path = "../../../../varix/src/unxstar/unx12755_risk_recheck_rd1008.rs"]
pub mod unx12755;
#[path = "../../../../varix/src/unxstar/unx12756_ktest_b38.rs"]
pub mod unx12756;
#[path = "../../../../varix/src/unxstar/unx12757_stress_close.rs"]
pub mod unx12757;
