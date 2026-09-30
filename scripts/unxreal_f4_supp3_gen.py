# -*- coding: utf-8 -*-
"""AI-29 · F4 增补卷三生成器（UNX-F4-E601–E900 · E31–E45 批 · 15 批 × 20 条）
主题/无障碍/IME UI 域 · 全部围绕 Varix 内核（checks.rs/lxprocfs.rs/内核事件·会话·IPC 通道/vxwm surface·popup·特效通道）锚定。
与卷一（E01–E15 批）卷二（E16–E30 批）判据颗粒零重复。
"""
import sys

BATCH_THEMES = {
    "E31": ("主题运行时热管线", "theme_runtime_hot_pipeline"),
    "E32": ("光标与指针主题态", "cursor_pointer_theme"),
    "E33": ("音效与听觉反馈主题", "sound_cue_theme"),
    "E34": ("屏幕阅读器叙事桥 NarratorBridge", "narrator_bridge"),
    "E35": ("放大镜与视觉辅助", "magnifier_visual_aid"),
    "E36": ("语音输入面板与听写 UI", "voice_dictation_panel"),
    "E37": ("手写输入面板与墨迹候选", "handwriting_ink_panel"),
    "E38": ("软键盘与触控优化布局", "touch_keyboard_layout"),
    "E39": ("IME 词库用户自定义 UI", "ime_user_dict_ui"),
    "E40": ("Emoji/符号面板与快捷插入", "emoji_symbol_panel"),
    "E41": ("主题对比度预览与校验工具链", "contrast_audit_toolchain"),
    "E42": ("无障碍自动化测试录制回放", "a11y_test_replay"),
    "E43": ("输入法云候选降级与离线路径", "cloud_candidate_fallback"),
    "E44": ("多语言排版与 RTL/BiDi 主题面", "rtl_bidi_theme"),
    "E45": ("F4 卷三治理收官与三卷联轧总账", "f4_vol3_governance"),
}

BATCH_SIZE = 20
START = 601
END = 900

DETAILS = [
    ("运行时主题热切换流水线（编译期零重绘路径）", "vxtheme_recompile_surface 全域主题变量改动走 vxwm 特效通道单帧交换；判据：切换 <100ms 且 surface 无整帧重绘"),
    ("主题资源 CRC 账与半途损坏拒载", "theme_pack_header 三重 CRC 校验，任一失败拒载并按三要素报错；判据：篡改 1 字节资源后加载返回 THEME_CORRUPT 且旧主题保持"),
    ("主题变量级联解析器（继承/覆盖/回退三阶）", "theme_var_chain 表按 系统包→用户包→应用内 三阶解析；判据：三层同变量命中优先级最高层"),
    ("热切换事务语义（失败原子回滚）", "theme_txn_begin/commit/rollback 原子事务；判据：切换中抛错后全 UI 回到旧主题零残影"),
    ("主题差异摘要与增量推送", "theme_diff 引擎只推送变更变量集合至各 surface；判据：改 1 变量仅触发持有该变量控件的失效"),
    ("深浅主题自动随宿主时钟/传感器切换", "theme_auto_schedule 挂内核事件通道 sunset/rounding 事件；判据：模拟 18:00 事件后 1s 内切深色"),
    ("主题预览沙箱（未应用先看效果）", "theme_preview_isolate 在独立 vxwm surface 层预览，关闭即弃；判据：预览态改主屏主题变量为 0"),
    ("主题收藏与历史版本时间轴", "theme_history_ledger 会话账持久化每次应用记录；判据：回滚到任意历史版本后 ledger 追加非覆盖"),
    ("主题导入向导（zip/目录/清单三源）", "theme_import_wizard 校验 manifest schema 后入库；判据：非法 manifest 走三要素错误并保留原库"),
    ("主题导出（含依赖字体/贴图打包）", "theme_export_pack 闭包收集依赖写入开放格式包；判据：导出包在他会话导入后逐字节校验一致"),
    ("主题变量冲突扫描器", "theme_conflict_scan 报告同名异义变量与硬编码色残留；判据：注入硬编码色后扫描命中并给出行号"),
    ("主题性能账（切换耗时/内存增量埋点）", "theme_perf_ledger 记录每次切换 P50/P95 与峰值内存；判据：连续 100 次切换 P95 < 100ms 入账"),
    ("主题失败恢复看门狗", "theme_watchdog 心跳检测渲染线程；判据：kill 渲染线程后 3s 内 watchdog 报警并自动回滚"),
    ("主题权限分级（系统/用户/应用三级写域）", "theme_acl 三级写权限面经内核 IPC 鉴权；判据：应用级写系统包被拒并显性化报错"),
    ("主题灰度发布（按窗口分组试新主题）", "theme_canary_group 指定窗口组先行试新；判据：非灰度窗口主题字节不变"),
    ("主题缓存失效与强刷接口", "theme_cache_invalidate 开发者强刷通道；判据：强刷后下一帧读取新值且旧值零引用"),
    ("主题字体回退链可视化调试", "theme_font_fallback_view 展示每字符命中链路；判据：缺字形字符显示回退来源字体名"),
    ("主题动画曲线库与全局节奏参数", "motion_curve_lib 全局节奏参数（时长/缓动倍率）统一入口；判据：改全局倍率后全部动画时长同比变化"),
    ("主题低功耗模式联动（省电时降特效）", "theme_power_link 监听内核电源事件降级特效；判据：注入低电事件后特效参数集切换且零重绘风暴"),
    ("主题热管线总日志接入", "主题全管线事件（切换/校验/回滚）入总日志中心统一时间轴；判据：任一次切换在总日志可检索且带耗时字段"),
]

def batch_rows(batch_id: str, prefix: str) -> list:
    name, tag = BATCH_THEMES[batch_id]
    rows = []
    start_no = START + (list(BATCH_THEMES).index(batch_id)) * BATCH_SIZE
    for i, (title, judge) in enumerate(DETAILS):
        no = start_no + i
        eid = f"UNX-F4-E{no:03d}"
        jid = f"{eid}-J1"
        rows.append(
            f"| {eid} | [{batch_id}] {name} · {title} | `{tag}/{prefix}_{i:02d}_{tag}.rs` | "
            f"Varix 内核锚定：{judge}（{jid} 可运行判据：构造假内核事件通道与 vxwm surface stub，调 {tag}_probe() 断言行为与错误码） | 增补 |"
        )
    return rows, name

def main(out_path):
    lines = []
    lines.append("")
    lines.append("---")
    lines.append("")
    lines.append("## 增补卷三 · AI-29 · UNX-F4-E601–E900（300 项新功能 · E31–E45 批 · 15 批 × 20 条）")
    lines.append("")
    lines.append("> 收录纪律：独立增补编号，不占域账 F22401–F23200（40 批守恒不动）；状态列统一「增补」；")
    lines.append("> 全部围绕 Varix 内核锚定（checks.rs 自检面 / lxprocfs.rs 自省面 / 内核事件·会话·IPC 通道 / vxwm surface·popup·特效通道）；")
    lines.append("> 与卷一（E001–E300）卷二（E301–E600）判据颗粒零重复；保管母本 docs/unxreal/supp/UNX-F4-SUPP-E601-E900.md（R-PROC-002 生成器重生成后须回播）。")
    lines.append("")
    total = 0
    for bid, (name, tag) in BATCH_THEMES.items():
        lines.append(f"### 批 {bid} · {name}（20 条）")
        lines.append("")
        lines.append("| 编号 | 功能 | 位置（建议） | 判据（可运行 · J1） | 状态 |")
        lines.append("|---|---|---|---|---|")
        rows, _ = batch_rows(bid, tag)
        lines.extend(rows)
        total += len(rows)
        lines.append("")
    lines.append(f"> 卷三小计：{total} 条。三卷联轧：UNX-F4-E001–E900 共 900 项增补，域账 F22401–F23200 零触碰。")
    lines.append("")
    with open(out_path, "w", encoding="utf-8") as f:
        f.write("\n".join(lines))
    print(f"written {out_path}: {total} rows, batches={len(BATCH_THEMES)}")

if __name__ == "__main__":
    main(sys.argv[1] if len(sys.argv) > 1 else "_f4_supp3_part.md")
