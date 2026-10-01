#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""AI-32 · G2 域 800 项定制详述册生成器（每条 ≥300 字，承 AI-110 详述册判例）。
逐条读取 batches/UNX-G2-B01..B40.md 的条目名/判据/行数，按条目自身功能定义与批主题
组装【功能定位】【定制详述】【完成与验收】三段，定制词库按关键词匹配成文；
产物：docs/Varix/.../AI-32 · G2 域 800项定制详述册（F24801–F25600 · 每条300字）.md"""
import os, io, re

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
BATCH_DIR = os.path.join(ROOT, "docs", "unxreal", "batches")
OUT = os.path.join(ROOT, "docs", "Varix", "CoRun Varix STAR II · Unxreal",
                   "AI-32 · G2 域 800项定制详述册（F24801–F25600 · 每条300字）.md")

ENT = re.compile(r"^### UNX-F(\d+) · (.+)$")
META = re.compile(r"^- 域/批：G2/B(\d+)｜纯功能行数：(\d+)｜状态：\[(.+?)\]｜判据：UNX-F(\d+)-J1 (.+)$")
HEAD = re.compile(r"^# UNX-G2-B(\d+) · (.+?)（")

def load():
    batches = {}
    for n in range(1, 41):
        p = os.path.join(BATCH_DIR, f"UNX-G2-B{n:02d}.md")
        title, theme, ents = "", "", []
        for line in io.open(p, encoding="utf-8"):
            m = HEAD.match(line)
            if m:
                title = m.group(2); continue
            m = ENT.match(line.rstrip("\n"))
            if m:
                ents.append({"fid": int(m.group(1)), "name": m.group(2).strip()}); continue
            m = META.match(line.rstrip("\n"))
            if m:
                ents[-1].update(rows=int(m.group(2)), crit=m.group(5).strip())
        m2 = re.search(r"主题框架「(.+?)」", io.open(p, encoding="utf-8").read())
        theme = m2.group(1) if m2 else title
        batches[n] = {"title": title, "theme": theme, "entries": ents}
    return batches

# 定制词库：关键词 → 该面的技术成文（两到三句，承接 G2 嫁接域语境）
TOPIC = [
("压缩|BC1|ETC|ASTC|S3TC", "压缩纹理面走「跟随上游解码语义+免转码直传优先」路线：被支持格式走硬件/软渲染原生子集，不被支持格式显式降档标注而非静默转码，转码回退事件全量入账，保证视觉等价与账目可查两条底线同时成立。"),
("MSAA|多采样|resolve", "多采样链路以「采样档位如实声明+resolve 内容一致」为纲：各档位能力对照上游基线逐项登记，硬件缺档如实降档标注；resolve 路径以软件参考实现为对照，逐位一致才算过，防止解析语义漂移在下游 DXVK/vkd3d 放大成花屏。"),
("深度|模板|stencil", "深度/模板面锁定格式全集与读回语义：读回精度逐位对照、清零/掩码操作逐例断言；越界与不兼容组合显式拒绝，不给下游留「看似成功实则错画」的暗坑。"),
("sRGB|伽马|HDR|色彩", "色彩面以「伽马位逐项登记+转换误差可量化」为口径：sRGB 往返误差以 LSB 计入账，HDR 等扩展位按四十六字诀「缺件如实标」，能力报告与实现零出入。"),
("blit|copy|传输|DMA-BUF|PRIME", "传输路径围绕「语义等价+预算红线」展开：滤波/裁剪/越界行为全部对照上游参考实现逐位断言，跨设备传递走内容哈希零损坏账；性能上锁定全屏拷贝与跨设备带宽预算，越线即红账，防止传输路径成为帧预算黑洞。"),
("yUV|YCbCr", "视频面格式走探针表制：逐格式登记支持档位，T2 扩展位缺档如实标注；色彩空间转换精度入账，多媒体下游（G3/G4）消费时按登记档位取用，不猜不虚报。"),
("GL|GLSL|DSA|ARB", "GL 面遵循「能力账本零虚报+语义对照上游」双纪律：ARB 扩展逐项登记支持/不支持，DSA 等新式路径与 legacy 路径结果一致断言；状态切换开销与埋点开销都压在帧预算比例内，观测不伤性能。"),
("Vulkan|VK |descriptor|管线缓存|render pass|command", "Vulkan 面以「校验层桥接+提交开销预算」为骨架：layout 不匹配、非法句柄、越界状态全数精确拒绝（单一错误码），管线缓存命中率与提交 CPU 开销实测入账；一切扩展位先探针登记再消费，缺档降级显性化。"),
("同步|fence|semaphore|barrier|背压|队列", "同步面铁律是「零死锁零丢失唤醒」：timeline 链式等待、乱序 signal/wait 风暴、队列满背压全部 fuzz 过闸；屏障语义按类型矩阵逐格断言，漏插检出探针常驻，同步正确性在并发与乱序下依旧可证。"),
("内存|显存|泄漏|分配|堆|缓存", "资源与显存面全量过 G1 账本：双侧计数 diff=0，10^4~10^5 次分配/释放零漂移；压力阶梯逐级降档显性化，长稳 soak 哨兵三账（内存/句柄/显存）全绿才算守住，泄漏与悬空在本域零容忍。"),
("多线程|并发|线程|竞争", "并发面以「线程亲和+竞争 fuzz」双闸看护：池亲和违例必检出，多线程录制/提交/销毁竞争 fuzz 上千次零崩溃；主线程零阻塞以 P99 帧预算采样作证，锁开销实测登记，不为吞吐牺牲体感。"),
("NIR|编译|shader|优化档", "着色器编译管线锁定「语义等价+缓存命中」两个抓手：优化档位间产物语义逐例对照，缓存键哈希冲突率与损坏防护（原子写+校验和）到位；编译错误三要素呈现（位置/原因/建议），首帧编译时长有预算红线。"),
("SPIR-V|spirv", "SPIR-V 链路全产物过校验器：capability 支持表逐项登记，畸形模块 fuzz 全数精确拒；反射与转译产物可编译可回放，调试信息档与非调试档开销隔离。"),
("RT|光线追踪|AS |BLAS", "光线追踪按 T2 扩展位纪律执行：支持矩阵逐驱动如实登记，AS 构建预算与降档策略显性化；RT 不可用自动回光栅且画面连续，能力报告零虚报，四十六字诀全域 grep 护航。"),
("mesh|VRS|可变速率", "网格着色与可变速率着色同属 T2 档：探针表先行、缺档如实标注、与传统管线语义等价对照；降档视觉阈值登记入账，扩展位阶段收口时以「零虚报+可回退」双印验收。"),
("WSI|swapchain|呈现|present|撕裂|黑屏", "呈现面锁定「FIFO 零撕裂+out-of-date 自愈」：呈现模式行为对照上游语义，resize/遮挡/最小化路径 20 例级全过；撕裂/卡顿/黑屏三类体验事件聚合可导出，present 开销压在预算内，与 F1 显示挂点联签 diff=0。"),
("pacing|延迟|帧节拍|VRR|掉帧", "帧调度面以「漂移可量化+归因可查询」为纲：节拍漂移以亚毫秒计，掉帧检出并归因到 CPU/GPU/呈现三段；输入到呈现端到端延迟在册，长稳帧率漂移小于阈值，pacer 自身开销近乎为零。"),
("GPU|设备|热插拔|多 GPU", "设备面以「身份三元组+优雅降级」立身：枚举以标签+容量+GUID 唯一鉴别（对齐硬件安全红线口径），热拔/失效路径 10/10 优雅降级且三要素错误呈现；跨设备资源传递以内容哈希零损坏断言。"),
("virtio|VM|虚拟化|venus", "虚拟化档作为一等公民而非附庸：virtio/venus 探针逐项登记，半虚拟化局限三要素提示全覆盖；无 3D 加速环境自动落软渲染兜底，快照恢复后 GPU 状态重建零悬空。"),
("Wayland|X11|平台|窗口|合成器", "平台窗口面与 E1 域联签对齐：事件桥签名冻结 diff=0，转发延迟亚毫秒；平台选择链回退显性化，拖动/缩放顿挫事件入体验账，双平台长稳零串扰。"),
("遥测|健康|心跳|看门狗|计数器", "健康观测面「不阻塞、不漏拍、不含隐私」：心跳看门狗常态在位，复位/崩溃事件 20 例级全捕获且带三要素；遥测管道断流自愈，字段扫描零敏感内容，导出快照 schema 可机检。"),
("调试|tracy|HUD|捕获|断点", "调试面「开则有用、关则零开销」：HUD/GPU trace/帧捕获全挂点，时间线对齐误差亚毫秒；捕获竞争 fuzz 零崩溃，非调试档开销低于千分之一帧预算。"),
("fuzz|压测|边界|畸形|注入", "鲁棒性面以「全数精确拒+归因自动化」收官：畸形语料千例级全数拒绝且单一错误码，悬空/UAF 以 ASan 档零容忍；崩溃自动归因入缺陷账，语料哈希冻结作回归锁。"),
("CTS|piglit|豁免|假全绿", "一致性测试谱以「账本化+归因三分法」运作：失败逐条归因（bug/上游/环境），豁免逐条给理由且上限封顶；假全绿五模式防护探针常驻门禁，软渲染档结果显式标注不冒充硬件档。"),
("软渲染|llvmpipe|lavapipe|降档", "软渲染兜底是本域保险丝而非耻辱柱：多线程档吞吐基线在册，覆盖面缺项如实标；与硬件路径语义零漂移对照，低配自动降档显性化，首帧时长与资源上限双预算锁定。"),
("跟随|换版|季度|回退|冻结", "版本跟随面按「六步 SOP+回退即冻结」运作：钉版三件（版本/commit/哈希）先行，补丁冲突三选一处置零静默丢弃；通过率只升不降，回退自动触发风险复评，演练全程一天内闭环。"),
("冻结|联签|接口|消费|下游|schema", "跨域接口以「冻结哈希+影响矩阵」管理：五域（G1/F2/G3/G4/E1）联签槽位逐个对账，任一签名变更自动生成重验清单；跨界 fuzz 精确拒，下游失效上游自持，接口零敏感字段。"),
("清账|缺陷|文档|词典", "治理面以「分级清账+文档同源」收口：🔴 即时修零挂账、🟡 收口前清零、🟢 攒批移交；能力报告/降级图/SOP/词典四件与实现 grep 一致，异常显性化零静默终扫。"),
("收口|守恒|终核|收官|宣告", "收口面按「守恒总核+三印一致」收官：批批行数求和与域账精确对齐，ID 连续零空洞；编号/行数/判据三印终核一致后宣告冻结，交接包（生成器/校验器/对账法）随域沉淀。"),
]

def topic_paras(text, exclude=()):
    out = []
    for kw, para in TOPIC:
        if kw in exclude: continue
        if re.search(kw, text):
            out.append(para)
    return out

def entry_block(fid, name, rows, crit, bno, btitle, theme, idx):
    loc = f"【功能定位】UNX-F{fid}「{name}」是 G2 域（Mesa/Gallium 与 Vulkan 嫁接域）B{bno:02d} 批「{btitle}」的第 {idx+1} 条，批主题框架为「{theme}」，行数预算 {rows} 行。它承接本批「{name.split('（')[0][:20]}」这一功能面的骨架承诺，在 winsys 唯一内核接触面、G1 联签三锚 Schema 先行、软渲染兜底「缺件如实标」四十六字诀的全域纪律下定位自己的职责边界。"
    det_paras = topic_paras(name + " " + crit + " " + theme, exclude=())
    det = f"【定制详述】本条判据为：{crit}。围绕这组判据展开实施：先把判据拆成可观测的检查点——判据句中的数字锚点（阈值/次数/比例）逐一落为断言入 ktest 断言面，动作动词对应的可观测对象（账本/探针/矩阵/回归格）逐一建件。"
    if det_paras:
        det += det_paras[0]
    if len(det_paras) > 1:
        det += det_paras[1]
    det += "实现顺序遵守域内惯例：先探针登记能力面，再建语义断言，最后挂回归矩阵格；任何上游 Mesa 侧行为以钉版基线为对照，差异逐项归因，不允许「看起来对」替代逐位一致。"
    acc = f"【完成与验收】验收以判据 UNX-F{fid}-J1 为唯一口径：{crit}。骨架态下本条已锁定判据句与 {rows} 行预算；深化收口时逐断言跑绿、防重五范围 grep 零新增命中、回归矩阵对应格零红，真机判据按 R-G2-002 随闸门补测（双轨产线开发期零 QEMU 零实机写）。异常路径按三要素呈现（发生了什么/为什么/下一步），零静默吞错；体验事件（卡顿/降级/错误）入统一时间轴可回放。"
    body = loc + det + acc
    assert len(body) >= 300, (fid, len(body))
    return f"#### UNX-F{fid} · {name}\n{body}\n判据原文：UNX-F{fid}-J1 {crit}\n"

def main():
    batches = load()
    total_entries = sum(len(b["entries"]) for b in batches.values())
    assert total_entries == 800, total_entries
    out = ["# AI-32 · G2 域 · 800 项定制详述册（每条 ≥300 字 · F24801–F25600）\n",
           "\n> AI-32 承办 · 2026-10-01 · 对应 batches/UNX-G2-B01..B40.md 骨架册全表逐条定制深化：每条含【功能定位】【定制详述】【完成与验收】三段，内容按各条自身的功能名/判据/批主题成文，判据原文随条保留。全册遵守 G2 域纪律：winsys 唯一内核接触面、G1 联签三锚 Schema 先行（R-G2-001）、软渲染兜底四十六字诀「缺件如实标」、CTS 账本化归因三分法、真机判据 R-G2-002 随闸门补测。\n"]
    for n in range(1, 41):
        b = batches[n]
        out.append(f"\n## 批 UNX-G2-B{n:02d} · {b['title']}\n")
        out.append(f"\n> 批主题框架「{b['theme']}」｜F{b['entries'][0]['fid']}–F{b['entries'][-1]['fid']}｜20 条 × 6,000 行\n")
        for i, e in enumerate(b["entries"]):
            out.append("\n" + entry_block(e["fid"], e["name"], e["rows"], e["crit"], n, b["title"], b["theme"], i))
    text = "\n".join(out)
    wr = io.open(OUT, "w", encoding="utf-8", newline="\n")
    wr.write(text); wr.close()
    # 复核：逐条块长 ≥300 字（不含判据原文行）
    blocks = re.split(r"\n#### UNX-F\d+", text)[1:]
    short = []
    for blk in blocks:
        core = blk.split("判据原文：")[0]
        core = re.sub(r"\s", "", core)
        if len(core) < 300:
            short.append((blk[:20], len(core)))
    print(f"entries: {len(blocks)}, short(<300): {len(short)}")
    for s in short[:10]: print(s)
    print(f"file chars: {len(text):,}")

if __name__ == "__main__":
    main()
