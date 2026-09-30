# -*- coding: utf-8 -*-
"""C4-B09 边界注记批量插入（AI-14 · finalize ⑤ 四项齐备修复）"""
import re
import sys

FN = "C4-B09.md"

PATCHES = {
    "UNX-F10562": "边界注记：解析键=family+有效长度（首个 NUL 截断，垃圾字节不入键）；绝对路径假设声明（相对路径按 Linux 同样解释注账）；108 上限判定在拷贝前（不半拷再报错）。",
    "UNX-F10564": "边界注记：二次 listen 只更新容量不追认超容在途（在途请求保留语义注账）；backlog=0 的最小队列语义落账；SOMAXCONN 夹逼声明（超上限按上限取值不静默改语义）。",
    "UNX-F10566": "边界注记：三态迁移单向（ESTABLISHED 不回退 CONNECTING，终态走 F10568/F10569 链）；EINPROGRESS 不适用（本地 unix connect 无异步挂起态，声明注差异）；同 fd 重复 connect 幂等键=fd+目标二元组。",
    "UNX-F10567": "边界注记：双端缓冲独立锁（锁序账：固定取锁序防交叉死锁）；读空/写满等待器挂接先查后挂（F10492 同款防线）；通路容量创建期定死不支持变更（声明落账）。",
    "UNX-F10569": "边界注记：EOF 位一次性置位不可逆；排空语义=read 消费至缓冲空后才返 0（不跳过在途数据）；SIGPIPE 投递挂位规则与 pipe 链一致（F10481 同账不重立）。",
    "UNX-F10570": "边界注记：三入口判定单点（旗标解析处统一拦截不散写）；ENOSYS 与 EOPNOTSUPP 分派账（本域选 ENOSYS 注声明）；实现预留位不占判据（E 型批次评估后另行立项）。",
    "UNX-F10571": "边界注记：peek 零指针推进断言探针（水位前后快照比对）；peek 与写并发序（peek 读到的是快照，写方并发追加不保证 peek 立即可见，声明）；请求量 0 的 peek 返 0 不报错（边界账）；MSG_PEEK 与 MSG_TRUNC/DONTWAIT 组合行为注 recv(2) 条文分立落账。",
    "UNX-F10572": "边界注记：判定序单点不可跳（旗标→fd 态→终态三段）；DONTWAIT 只影响本次调用不回写 fd 态（两源独立性声明）；EAGAIN 与 EOF 判定序（先终态后容量，对端关后读不再 EAGAIN 而得 EOF）。",
    "UNX-F10573": "边界注记：等价面仅限无旗标路径（旗标路径行为以各旗标条判据为准）；地址参数面差异不涉等价（send/recv 旗标参、write/read 无参）；同源断言在编译期符号级（运行期另有 1 MB 对照双保险）。",
    "UNX-F10574": "边界注记：pair 无名字无节点（bind/connect 不适用 EINVAL 账）；双端生命周期独立（单端 close 另一端存活，与 F10569 终态链一致）；fork 继承语义（fd 复制后引用计数增，声明注账）。",
    "UNX-F10576": "边界注记：入队原子=容量判定+登记同锁（超容探针零容忍）；溢出拒绝不重试（客户端侧 ECONNREFUSED 即终态，不静默重排）；accept 摘取与入队同队列锁（并发摘取不双发）。",
    "UNX-F10577": "边界注记：幽灵节点 connect 判定序（节点存在但无监听对象→ECONNREFUSED 不挂起）；unlink 后既有连接不中断（节点与连接解耦声明）；B1 联签边界=目录项操作归 B1（本条只设判据不越界）。",
    "UNX-F10578": "边界注记：三错误判定优先序落账（域/型错误→地址错误→连接态错误）；正反用例配对（触发后修复前置必成，防误杀合法路径）；errno 单点=符号引用（裸数值 grep 断言入回归常跑）。",
    "UNX-F10579": "边界注记：串扰探针以独立校验和定界（A/B 内容交叉即红，探针入回归）；100 轮重连含异常退出路径（半开连接清理断言）；压测种子入账可复现（随机流确定性声明）；场末回收三账对平（连接/fd/内存缺一即红）。",
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
