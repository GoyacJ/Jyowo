# Codex 风格会话侧栏 Design QA

## 对比目标

- Source visual truth:
  - `.design-qa/reference-sidebar.png`
  - `.design-qa/reference-task-menu.png`
  - 用户提供的其余 Codex 截图用于校验项目添加、排序和菜单语义。
- Implementation:
  - `.design-qa/implementation-full-v2.png`
  - `.design-qa/implementation-sidebar-v2.png`
  - `.design-qa/implementation-task-menu-v2.png`
- Full-view comparison: `.design-qa/sidebar-comparison-v2.png`
- Focused comparison: `.design-qa/task-menu-comparison-v2.png`
- Viewport: 1440 × 987 CSS px；侧栏宽 300 px。
- State: 中文、深色主题、置顶/项目/会话展开、项目展开、默认工作区会话可见。

## Findings

- 无未解决的 P0、P1、P2。
- [P3] Jyowo 使用现有 Inter 字体和更深的产品色板，参考 Codex 使用偏暖的系统深灰。保留现有设计令牌，避免侧栏与主工作区产生字体和主题断层。
- [P3] 参考图顶部含 Codex 品牌和其他全局入口。本次范围是会话结构，保留 Jyowo 现有全局壳层，只复刻新建会话、置顶、项目、会话及其菜单层级。

## Fidelity surfaces

- Fonts and typography: 字号、字重、行高、截断和菜单密度接近参考；长会话名使用单行截断。Inter 是现有产品字体。
- Spacing and layout rhythm: 300 px 侧栏、32–36 px 行高、项目缩进、分组间距和右侧菜单位置与参考一致。无裁切、重叠或横向溢出。
- Colors and visual tokens: 使用现有 `background`、`raised-surface`、`selection`、`destructive` 令牌；错误和危险操作具备独立语义色。
- Image quality and asset fidelity: 目标没有需要复刻的照片或插画。所有 UI 图标来自项目既有 Lucide 图标库，无手写 SVG、CSS 图形或占位资产。
- Copy and content: 用户可见的 `New Task` 已替换为“新建会话”；分组为“置顶 / 项目 / 会话”；菜单、确认框和空状态均已中文化。

## Interaction verification

- 应用内 Browser 已验证：侧栏展开/收起。
- “置顶 / 项目 / 会话”三个分组可独立折叠。
- 单个项目可独立折叠；项目内“新建会话”入口存在且可操作。
- 会话可打开；会话菜单包含置顶、重命名、归档、移除。
- 项目菜单包含重命名、上移、下移、移除项目。
- 重命名对话框可打开，字段和保存/取消按钮可访问。
- 会话 mutation 失败时错误显示在侧栏，现有会话列表保留。
- 浏览器控制台最终检查：0 条 warning，0 条 error。
- 默认工作区创建、项目工作区创建、任务 mutation 成功路径和持久化由自动化边界测试覆盖；浏览器 fixture 不伪造 daemon 写入。

## Comparison history

### Iteration 1

- Evidence: `.design-qa/sidebar-comparison-v1.png`、`.design-qa/task-menu-comparison-v1.png`。
- Earlier finding: [P1] 会话置顶、重命名、归档或移除失败时产生未处理 Promise，且侧栏不显示错误。
- Reproduction: `SidebarNav.test.tsx` 新增失败用例，首次运行得到 1 个失败测试和 1 个 unhandled rejection。

### Iteration 2

- Fix: 四类会话写操作统一进入 TanStack mutation；成功后刷新任务投影；归档/移除当前会话后安全返回空路由；失败时在侧栏显示错误并保留列表。
- Post-fix evidence: `.design-qa/implementation-full-v2.png`、`.design-qa/sidebar-comparison-v2.png`、`.design-qa/task-menu-comparison-v2.png`。
- Verification: 回归测试通过；应用内 Browser 实际触发 fixture mutation 错误后显示状态文本；控制台保持 0 warning / 0 error。

## Implementation checklist

- [x] 默认工作区新建会话入口
- [x] 项目内新建会话入口
- [x] 置顶 / 项目 / 会话独立折叠
- [x] 项目独立折叠
- [x] 会话置顶、重命名、归档、移除
- [x] 项目重命名、排序、移除
- [x] 中文术语与空状态
- [x] 错误反馈与列表保留
- [x] 浏览器视觉、交互和控制台验收

## Follow-up polish

- P3：如后续统一 Jyowo 与 Codex 的全局壳层，可单独评估品牌标题、全局快捷入口和暖灰色板；不属于本次会话侧栏范围。

final result: passed
