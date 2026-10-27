# 实验报告 Lab 3：进程创建与 stride 调度

## 实验目标

在进程管理章节实现 spawn 系统调用和按优先级分配 CPU 时间的 stride 调度，同时延续 ch4 的匿名映射和时间读取功能。

## 实现设计

- spawn 从当前用户地址空间读取程序名，在内核嵌入的应用列表中查找目标 ELF；找到后创建新的进程控制块，设置父子关系并加入就绪队列。无效名称返回 -1。
- 每个进程维护 stride 和 priority。调度器从就绪队列中选择 stride 最小的进程，并按 BIG_STRIDE / priority 增加其 stride；优先级小于 2 的请求返回 -1。
- mmap/munmap 保留 ch4 的用户页权限、页对齐、范围冲突和映射存在性检查；get_time 通过用户页表复制结果。

## 验证

由本期官方 QEMU/RISC-V 测试覆盖 spawn、waitpid、set_priority 及前序章节兼容用例。最终点数和上传状态以本分支 GitHub Actions 为准。

## 与 AI 协作

使用 AI 协助理解进程生命周期、父子关系、就绪队列和 stride 的关系，并对照课程用户测例检查返回值和优先级边界。最终实现以本仓库代码和自动评测为准。
