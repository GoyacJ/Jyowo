# 任务内容视窗实施计划

## 目标

按《任务内容视窗与三层工作区设计》将当前“工作台兼任运行视窗”的实现拆分为三个独立层级：

1. 任务概览只展示标准化状态和入口。
2. 侧边工作台只展示命令、文件源码、Diff、环境、子智能体和审计等检查对象。
3. 独立的内容视窗展示 HTML/Web App、浏览器、图片、视频、音频、文档和其他展示型产物。

## 实施基线

当前代码已经包含通用运行会话、HTML loopback 预览、内置浏览器面板、产物渲染器、工作台标签和浮动几何逻辑。本次是迁移和拆分，不重建 daemon 运行协议。

保留：

- daemon 中已有的 `RuntimeCommand`、`RuntimeSession`、资源归属校验和停止清理。
- HTML 预览服务的 loopback、凭证、CSP、MIME、路径校验和 iframe sandbox。
- `ArtifactRenderer`、Blob 延迟加载、错误边界和 Object URL 生命周期。
- 现有工作台的临时预览标签、固定标签、会话定位和焦点恢复。

迁移：

- 将浮动位置、缩放、全屏和恢复状态从 `TaskWorkbenchSession` 移到新的 `TaskContentViewportSession`。
- 将 `BrowserPanel` 和 `HtmlRuntimePanel` 从工作台内容移到内容视窗。
- 将媒体和展示型产物的打开目标从工作台分流到内容视窗。

## 阶段一：统一对象目标和分流协议

### 修改范围

- 修改 `crates/jyowo-harness-contracts/src/daemon.rs`。
- 修改 `crates/jyowo-harness-contracts/tests/daemon_contract.rs`。
- 按需修改 `crates/jyowo-harness-journal/src/task_projection.rs` 及对应测试。
- 新增 `apps/desktop/src/features/tasks/surfaces/task-surface-target.ts`。
- 新增 `apps/desktop/src/features/tasks/surfaces/task-surface-target.test.ts`。
- 迁移 `apps/desktop/src/features/tasks/workbench/task-workbench-target.ts` 的公共目标映射逻辑。

### 实施内容

1. 将 `TaskWorkbenchTarget` 提升为两个详细层共用的 `TaskSurfaceTarget`。
2. 增加 `TaskSurface = 'workbench' | 'viewport'` 和集中的 `resolveTaskSurface(target, intent)` 纯函数。
3. 将用户显式操作作为最高优先级，再读取产物展示提示，最后按对象类型、MIME 和格式回退。
4. 为 `TimelineArtifactSurface` 增加 `Viewport`，使产物生产者可以显式要求内容视窗。
5. 为 `TimelineArtifactPresentation` 增加可选 `autoOpen`。默认值为 `false`，只有主要展示型产物可以显式设置为 `true`。
6. 产物目标的 `resourceId` 优先使用稳定 `artifactId`，再回退到 Blob ID 或事件 ID，使新版本更新原标签。
7. 目标 key 包含显示层，允许同一产物的源码视图和渲染视图同时存在。
8. 更新任务概览、会话行和工作台面板的目标类型引用，不在组件中保留媒体分流分支。

### 验证

- 命令、Diff、文本文件、环境、子智能体和审计目标解析到 `workbench`。
- 图片、视频、音频、GeoJSON、HTML 运行和浏览器目标解析到 `viewport`。
- 显式“查看源码”或“在内容视窗中预览”覆盖默认分流。
- 同一 `artifactId` 的新 Blob 不创建重复标签。
- 运行 `pnpm generate:daemon-protocol` 和 `pnpm check:daemon-protocol`。

## 阶段二：拆分工作台与内容视窗状态

### 修改范围

- 修改 `apps/desktop/src/shared/state/workbench-selection.ts` 及测试。
- 新增 `apps/desktop/src/shared/state/content-viewport.ts`。
- 新增 `apps/desktop/src/shared/state/content-viewport.test.ts`。
- 修改 `apps/desktop/src/shared/state/ui-store.ts` 及测试。

### 实施内容

1. 从 `TaskWorkbenchSession` 删除 `viewportMode`、`viewportRestoreMode` 和 `viewportGeometry`。
2. 保留工作台的 `open`、`tabs`、`activeTabId` 和 `previewTabId`。
3. 建立按任务隔离的 `TaskContentViewportSession`，包含：
   - 打开状态。
   - 临时预览标签和固定标签。
   - `floating`、`minimized` 和 `fullscreen` 模式。
   - 全屏前模式和几何快照。
   - 位置、尺寸和边界夹紧。
   - `autoOpenSuppressedForRunId`。
4. 将工作台中已有的默认尺寸、夹紧和八方向缩放纯函数移入内容视窗状态模块。
5. 在 UI store 中增加 `taskContentViewportByTaskId` 和打开、隐藏、最小化、恢复、全屏、标签、固定、几何与自动打开抑制操作。
6. 关闭、最小化和停止运行保持为三个独立操作；UI store 不直接发送运行停止命令。
7. 任务切换恢复各自 Session，不将标签和几何状态写入 daemon Projection。

### 验证

- 两个 Session 的标签、打开状态和活动对象不互相覆盖。
- 内容视窗按任务隔离几何和标签。
- 最小化后保留标签、活动对象和运行会话标识。
- 全屏退出后恢复之前的浮动几何和模式。
- 当前运行被用户抑制自动打开后，后续产物只增加提示。

## 阶段三：恢复侧边工作台的单一职责

### 修改范围

- 修改 `apps/desktop/src/features/tasks/workbench/TaskWorkbench.tsx` 及测试。
- 修改 `apps/desktop/src/features/tasks/workbench/task-workbench-target.ts` 或删除已迁移的映射逻辑。
- 修改 `apps/desktop/src/shared/styles/global.css`。

### 实施内容

1. 移除工作台的标题栏拖动、八方向缩放、浮动/停靠切换和用户全屏模式。
2. 宽屏只使用右侧停靠；中屏使用右侧覆盖；窄屏使用任务区全屏。
3. 保留面板宽度调整、标签替换/固定、会话定位、`Escape` 和关闭后焦点恢复。
4. 从 `WorkbenchContent` 移除浏览器、HTML 运行和展示型产物的完整渲染。
5. 保留命令、Diff、文本/代码文件、环境、子智能体和审计内容。
6. 为可渲染文件或产物增加“在内容视窗中预览”，使用共用 `TaskSurfaceTarget` 打开关联对象。
7. 工作台不能保留内容视窗的模式、几何、最小化或自动打开状态。

### 验证

- 命令、Diff 和文本文件只打开侧边工作台。
- 工作台在宽屏不再显示浮动、拖动和全屏控件。
- 工作台关闭不改变内容视窗 Session。
- “在内容视窗中预览”打开正确产物，工作台标签保持不变。

## 阶段四：建立独立内容视窗

### 新增范围

- 新增 `apps/desktop/src/features/tasks/content-viewport/TaskContentViewport.tsx`。
- 新增 `apps/desktop/src/features/tasks/content-viewport/TaskContentViewport.test.tsx`。
- 新增 `apps/desktop/src/features/tasks/content-viewport/TaskContentViewport.stories.tsx`。
- 按需新增纯函数布局或内容选择模块，不创建第二套几何状态。

### 实施内容

1. 建立标题栏、标签区、内容工具栏和内容区四层结构。
2. 将现有工作台的 Pointer Event 拖动、八方向缩放和卸载清理迁移到内容视窗，不复制两份手势逻辑。
3. 实现 `560 × 400` 默认尺寸、`360 × 240` 最小尺寸、工作区边界夹紧和完整标题栏保留。
4. 实现浮动、最小化状态条、任务区全屏和全屏恢复。
5. 浮动模式不设为模态；全屏模式将其他任务内容设为 `inert` 和 `aria-hidden`，并限制焦点。
6. 实现关闭视窗、关闭标签、最小化和停止运行的独立控件与状态转移。
7. 标签保留一个临时预览标签和若干固定标签，支持相邻标签焦点、方向键、`Home`、`End` 和标签溢出滚动。
8. 使用项目设计变量、Lucide 图标和现有 Button 原语，不建立另一套视觉体系。

### 验证

- 拖动只从标题栏开始，右键或非主指针不启动手势。
- 八个缩放边界均正确夹紧，容器改变后视窗不移出任务区。
- 最小化后焦点进入状态条，恢复后返回原标签或控件。
- 全屏下其他任务内容不可聚焦，`Escape` 恢复浮动尺寸和位置。
- 视窗框架卸载后没有 Pointer Capture 或 `user-select` 残留。

## 阶段五：迁移内容渲染与运行视图

### 修改范围

- 修改 `apps/desktop/src/features/artifacts/model.ts`、`registry.ts` 和渲染器测试。
- 修改 `apps/desktop/src/features/artifacts/ArtifactRenderer.tsx` 及测试。
- 将 `apps/desktop/src/features/tasks/workbench/HtmlRuntimePanel.tsx` 及测试迁移到 `content-viewport`。
- 将 `apps/desktop/src/features/tasks/workbench/BrowserPanel.tsx` 及测试迁移到 `content-viewport`。

### 实施内容

1. 为 `ArtifactRenderer` 增加 `viewport` surface，内置图片、视频、音频、GeoJSON 和 fallback 渲染器提供对应视图。
2. 保留 `inline` 和 `card` 在会话中的摘要展示；完整媒体内容打开 `viewport`。
3. 文本、代码和配置文件继续在 `workbench` 显示，不因渲染器共用而改变默认分流。
4. 将 HTML 源码视图和运行视图分开：源码打开工作台，显式“运行”后在内容视窗打开受限 iframe。
5. 浏览器会话迁移到内容视窗，保留现有 daemon request/response 和会话状态。
6. 为渲染器定义有限能力元数据，内容工具栏只显示当前渲染器声明的操作。首版不为未实现的 PDF/Office 功能显示占位按钮。
7. 关闭或最小化视窗不发送 runtime stop；只有内容工具栏的停止操作释放运行资源。

### 验证

- 图片、视频、音频和 GeoJSON 在内容视窗中渲染，会话内卡片仍保留摘要。
- HTML 源码不执行；显式运行后只加载经校验的 loopback URL。
- 浏览器和 HTML 视图最小化后会话保持，停止后资源释放。
- Blob 加载失败、缺失和渲染失败保持独立状态与重试。

## 阶段六：集成任务工作区和自动打开

### 修改范围

- 修改 `apps/desktop/src/features/tasks/TaskWorkspace.tsx` 及测试。
- 修改 `apps/desktop/src/features/tasks/timeline/TaskTimeline.tsx`、`TimelineEvent.tsx` 及测试。
- 将 `TaskWorkbenchSummary` 及其模型重命名为 `TaskOverview`，并更新测试和样式类名。
- 修改 `apps/desktop/src/shared/styles/global.css`。
- 修改任务区相关 Storybook 场景。

### 实施内容

1. 在 `TaskWorkspace` 中增加单一 `openSurfaceTarget(target, intent, trigger)` 协调入口。
2. 根据 `resolveTaskSurface` 打开工作台或内容视窗，分别保存焦点来源。
3. 会话中的展示型产物卡片打开内容视窗；命令、Diff 和文件源码继续打开工作台。
4. 任务概览行使用同一分流入口，不将目标强制为工作台。
5. 实现内容视窗的“查看源码/详情”和“在会话中定位”，不关闭当前内容标签。
6. 宽屏允许侧边工作台与内容视窗同时交互，任务概览收为紧凑状态条。
7. 中屏只展开一个详细层；窄屏将工作台和内容视窗都转为任务区全屏。
8. 自动打开只处理 `autoOpen=true` 的展示型主要产物，不自动全屏或移走焦点。
9. 用户在当前运行中最小化或关闭内容视窗后，后续自动产物只更新状态条和任务概览提示。
10. 打开、隐藏、拖动和全屏切换不改变会话的 Following/Paused 状态或输入草稿。

### 验证

- 三层使用各自状态，且会话、任务概览和两个详细层可以双向定位。
- 内容视窗自动打开不抢占输入框焦点。
- 用户主动最小化或关闭后，新产物不重新展开。
- `719/720/1039/1040px` 边界具有确定性布局。
- 中窄屏不会同时暴露两个全屏或覆盖详细层。

## 阶段七：本地化、可访问性和失败状态

### 修改范围

- 修改 `apps/desktop/src/shared/i18n/locales/zh-CN.ts`。
- 修改 `apps/desktop/src/shared/i18n/locales/en-US.ts`。
- 补充内容视窗、工作台、任务概览和运行视图测试。

### 实施内容

1. 所有可见字符串通过 `tasks` 命名空间提供中英文资源。
2. 为视窗、标题栏、标签列表、状态条、缩放边界、全屏和关闭提供稳定的可访问名称。
3. 浮动视窗不陷阱焦点；全屏视窗陷阱焦点并隔离其他任务内容。
4. 最小化、恢复、关闭标签和关闭视窗均有明确焦点去向。
5. 加载、Blob 缺失、格式不支持、渲染失败、运行失败和 daemon 断开分别展示。
6. 状态不只依赖颜色，并遵守 `prefers-reduced-motion`。

### 验证

- 键盘可以打开、切换、固定、最小化、恢复、全屏和关闭内容。
- 中英文界面没有硬编码用户文案。
- 全屏模式下会话、任务概览和侧边工作台不可聚焦。
- 每种失败状态提供正确的重试、重新运行或返回操作。

## 阶段八：安全回归和最终验收

### 定向测试

运行与实际修改范围一致的定向测试：

```bash
pnpm -C apps/desktop test \
  src/shared/state/content-viewport.test.ts \
  src/shared/state/workbench-selection.test.ts \
  src/shared/state/ui-store.test.ts \
  src/features/tasks/surfaces/task-surface-target.test.ts \
  src/features/tasks/content-viewport \
  src/features/tasks/workbench \
  src/features/tasks/TaskWorkspace.test.tsx \
  src/features/tasks/timeline/TaskTimeline.test.tsx \
  src/features/artifacts
```

协议和 Rust 定向测试：

```bash
pnpm generate:daemon-protocol
pnpm check:daemon-protocol
cargo test -p jyowo-harness-contracts
cargo test -p jyowo-harness-journal
cargo test -p jyowo-harness-daemon --test runtime_service
```

### 项目门禁

```bash
pnpm check:frontend:fast
pnpm check:rust:fast
pnpm check:quick
```

改动涉及桌面完整构建或 Tauri 封装时，再运行 `pnpm check`。

### 视觉与交互验收

在实际应用或 Storybook 中覆盖：

- `1440 × 900`：任务概览紧凑态、侧边工作台和浮动内容视窗共存。
- `900 × 900`：工作台和内容视窗互斥覆盖。
- `690 × 844`：内容视窗全屏、返回会话和焦点恢复。
- 浅色、深色和系统主题。
- 图片、视频、音频、GeoJSON、HTML 运行和浏览器内容。
- 拖动、八方向缩放、最小化、恢复、全屏、关闭和停止。
- 会话定位、源码/预览切换、任务切换和自动打开抑制。
- 200% 缩放、键盘导航和减少动态效果。

捕获宽屏、中屏、窄屏、浅色和深色截图，把对照结果追加到项目根目录 `design-qa.md`。

## 后续渲染器

三层架构和内容视窗完成后，再分别为以下内容建立独立设计和实施计划：

- PDF 目录、缩略图、搜索和页码引用。
- Word 和其他文档阅读器。
- 表格 Sheet、筛选、公式和单元格引用。
- 幻灯片缩略图和页面导航。
- 图片区域、视频时间段和地图要素引用。

这些后续能力复用 `TaskContentViewport`、分流协议和渲染器能力元数据，不再修改三层页面架构。

## 完成定义

- 代码中不再存在“工作台浮动模式”或工作台几何状态。
- 任务概览、侧边工作台和内容视窗使用独立状态和确定性分流。
- 工作台和内容视窗可以从同一产物双向打开，但不共用标签或开关。
- 内容视窗支持浮动、拖动、缩放、最小化和任务区全屏。
- 命令、文件源码和 Diff 不在内容视窗中重复实现。
- HTML/Web App 只在显式运行后进入受控内容视窗。
- 关闭、最小化和停止运行具有独立并通过测试的转移。
- 协议生成检查、快速门禁、相关 Rust 测试和视觉 QA 通过。
