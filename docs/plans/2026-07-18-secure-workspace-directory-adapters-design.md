# 安全工作区目录适配器设计

## 目标

让 `ListDir`、`Glob`、`Grep` 在 daemon 工作区租约内正常执行，同时继续防止符号链接逃逸和授权后路径替换。

## 根因

- 三个工具都把目录声明为 `ActionResource::FileRead`。
- daemon 对文件系统资源统一改走 `WorkspaceToolAuthorization`。
- 当前授权对象只提供安全文件读写，没有目录遍历能力。
- 当前适配器只处理 `FileRead`、`FileWrite`、`FileEdit`，其余文件系统工具按 fail-closed 拒绝。

## 核心决策

- 在 `WorkspaceToolAuthorization` 上增加基于目录文件描述符的只读遍历接口。
- 每一级目录和文件都使用 `openat`、`NOFOLLOW` 和相对路径打开，不退回绝对路径 `std::fs` 访问。
- 符号链接和其他特殊文件不进入结果，也不递归。
- 遍历接口按条目调用回调。需要搜索内容时逐文件读取，避免把整个工作区快照留在内存中。
- daemon 适配器分别恢复 `ListDir`、`Glob`、`Grep` 的既有输出结构、排序、隐藏文件和深度语义。
- 保留未知文件系统工具 fail-closed 的默认分支。

## 安全边界

- 授权仍绑定工作区租约、规范化根目录和请求路径。
- 遍历期间持有授权激活状态；授权失效后不能开始新操作。
- 目录递归只使用已经安全打开的父目录描述符。
- 单文件读取继续使用现有读取上限。
- 增加目录深度和条目数上限，避免递归和无界枚举耗尽资源。

## 非目标

- 不改变工具权限提示或用户规则。
- 不让工具直接绕过 daemon 调用原始文件系统实现。
- 不改变 Bash 沙箱命令通道。
- 不在本次修复 Windows 的既有 `SecureWorkspaceIoUnavailable` 限制。
