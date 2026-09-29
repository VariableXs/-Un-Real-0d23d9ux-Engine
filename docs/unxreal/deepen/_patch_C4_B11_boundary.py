# -*- coding: utf-8 -*-
"""C4-B11 边界注记批量插入（AI-14 · finalize ⑤ 四项齐备修复）"""
import re
import sys

FN = "C4-B11.md"

PATCHES = {
    "UNX-F10602": "边界注记：注入序=校验（上限+有效性）先于取引用（校验失败零副作用）；SCM_MAX_FD=253 超限整批拒（不部分注入）；失败回滚幂等（重复回滚不重复退）。",
    "UNX-F10603": "边界注记：安装序=预检容量→预分配→批量安装三段（失败全回滚不部分成功）；新号与发送者原号无关（映射账，同名不同表）；CLOEXEC 缺省位按进程缺省声明（F10605 旗标另算）。",
    "UNX-F10604": "边界注记：三态账取锁序=fd 表锁→暂持账锁（固定序防死锁）；对平探针含异常路径（失败回滚轮次计入守恒）；计数本体消费既有接口（本条不另立计数器，防双账漂移）。",
    "UNX-F10606": "边界注记：三口判定单点=控制消息 type 分派处（不散写入口）；SO_PASSCRED 拒绝优先于 cmsg 传递判定（选项态先判）；红账含 fd、type、调用点三元组可查。",
    "UNX-F10607": "边界注记：flush 判定序=SHUT_WR 不动接收队列（半关只关发送向）；销毁结算与 F10569 回收同点（结算先于对象销毁，序声明）；未收 fd 报文退还顺序不承诺（账面守恒即可）。",
    "UNX-F10609": "边界注记：DEL 与 wait 竞态=树删+就绪清理同锁原子（wait 不见半删态）；MOD 即时生效=掩码单点读取（wait 出队过滤时读，不快照缓存）；ADD 后既有就绪态立即可 wait 到（LT 首报语义）。",
    "UNX-F10610": "边界注记：挂接幂等=同 fd 同事件位合并不双挂（重复探针）；wait 消费摘链与回挂同锁（LT 回挂不丢并发事件）；对平账含异常路径（DEL 摘除计入守恒）；就绪链与 interest 树分锁（锁序账落册）。",
    "UNX-F10611": "边界注记：ET 摘链后数据残留不追报（未读尽语义声明，调用方循环读约定）；翻转判定源=统一 poll 就绪位翻转（不依赖数据量估算）；ET 与 ONESHOT 组合按 ONESHOT 优先注 epoll(7)；翻转账探针含写-读-再写交错序（1:1 守恒跨轮对平）。",
    "UNX-F10612": "边界注记：三态判定序=就绪链非空先于超时挂起（有事件不等超时）；maxevents=0 EINVAL、负值按声明账；EINTR 后不自动重启（SA_RESTART 表 F10472 联签判定）。",
    "UNX-F10613": "边界注记：豁免过滤序=HUP/ERR 直通先于 interest∧events（判定序单点）；ERR 源=各就绪面错误位（不新造错误源）；HUP 上报后摘链时点=wait 消费时（与 LT 回挂协调）。",
    "UNX-F10614": "边界注记：禁用实现=interest 清零非摘树（条目保留账面可查，MOD 复活走原条目）；复活按当前就绪态重算（陈旧事件不补报，探针）；与 ET 组合=oneshot 上报后摘且不复挂（双重静默语义注账）；多线程并发 wait 的单唤醒由摘链原子性保证。",
    "UNX-F10615": "边界注记：事件归属探针以 fd 对象身份定界（事件必带源 fd 号+类型标记，错配即红）；四源事件语义差异按各自就绪面定义（本条不分派语义只验分流）；签名断言编译期+联签账双保险（第四方 B12 接入前锁定）；混合树 1 万事件流种子入账可复现。",
    "UNX-F10616": "边界注记：失效判定挂引用归零点（事件回调序，非 close 调用点——dup 场不早摘）；失效条目内存回收归显式 DEL 或树重建（失效≠释放，两态账）；幽灵探针含 wait 后再 DEL 的幂等用例（ENOENT 预期）。",
    "UNX-F10617": "边界注记：10k 混合源比例入账（管道/socket 各半声明）；事件风暴种子固定可复现（随机序确定性）；分层扫描账=就绪链增量（不全树重扫，量级账实测记录）；挂号三对应核验含错误码面（EPERM/ENOENT/EEXIST 抽查）。",
    "UNX-F10618": "边界注记：旗标解析在 ADD 掩码合并前（含旗标整请求拒，不剥旗标放行）；红账含 epoll fd、目标 fd、事件集三元组；替代方案 ONESHOT 注账（多 waiter 场 per-waiter epoll+ONESHOT 等价声明）；实现预留位不占判据。",
    "UNX-F10619": "边界注记：拒入判定=ADD 入口 fd 类型单点（epoll 型即拒，含跨树/自树两向）；互监控用例=A 树含 B 树 fd 后 B 再 ADD A（环检测由拒入单点覆盖，不另立图算法）；read/write epoll fd 的 ENOTTY 家族行为注 epoll(7) 声明账。",
    "UNX-F10620": "边界注记：归属探针=安装进程 fd 表逐轮核对（fd 对象易主即绿、跨进程误装即红）；ET+oneshot 混合配比入账（各 50% 声明）；三账归零含异常路径轮（中途 close 的客户端计入结算）；三桥账核销字段=fd 型桥消费登记（F10591 联签收口）。",
}

def main():
    with open(FN, encoding="utf-8") as f:
        text = f.read()

    applied, skipped = [], []
    for fid, note in PATCHES.items():
        m = re.search(r"(### " + fid + r"\b.*?\n)(- 正文：[^\n]*)", text, re.S)
        if not m:
            skipped.append((fid, "正文行未定位"))
            continue
        line = m.group(2)
        if "边界注记：" in line:
            skipped.append((fid, "已含边界注记，跳过"))
            continue
        anchor = "与现存内核衔接点："
        idx = line.find(anchor)
        if idx < 0:
            skipped.append((fid, "衔接点锚未命中"))
            continue
        end = line.find("。", idx)
        if end < 0:
            skipped.append((fid, "衔接点句号未命中"))
            continue
        if text.count(line) != 1:
            skipped.append((fid, "正文行不唯一，拒绝盲替换"))
            continue
        newline = line[:end + 1] + note + line[end + 1:]
        text = text.replace(line, newline, 1)
        applied.append(fid)

    with open(FN, "w", encoding="utf-8", newline="\n") as f:
        f.write(text)

    print(f"applied={len(applied)}: {', '.join(applied)}")
    if skipped:
        print(f"skipped={len(skipped)}:")
        for fid, why in skipped:
            print(f"  {fid}: {why}")
    if len(applied) != len(PATCHES):
        sys.exit(1)

if __name__ == "__main__":
    main()
