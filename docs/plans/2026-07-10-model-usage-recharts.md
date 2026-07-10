# Model Usage Charts Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Replace the broken handwritten weekly and cumulative charts with responsive Recharts visualizations and finish the daily heatmap interaction and layout.

**Architecture:** Keep metric and daily heatmap rendering local to the settings feature. Use Recharts only for weekly and cumulative data, with shared localized formatting and an explicit empty state. Keep theme colors sourced from existing CSS variables.

**Tech Stack:** React 19, TypeScript, Recharts, Tailwind CSS, Radix Tooltip, Vitest, Testing Library, Storybook, Playwright.

---

### Task 1: Capture Broken Behaviors

**Files:**
- Modify: `apps/desktop/src/features/settings/models/ModelUsageInsightsPanel.test.tsx`

1. Add tests for responsive chart containers, zero-value weeks, valid theme colors, empty data, single-row month labels, and bounded keyboard focus.
2. Run the focused test and confirm the new assertions fail for the current implementation.

### Task 2: Add Recharts

**Files:**
- Modify: `apps/desktop/package.json`
- Modify: `pnpm-lock.yaml`

1. Add Recharts to the desktop package with pnpm.
2. Confirm the lockfile changes contain only the new dependency graph.

### Task 3: Rebuild Weekly And Cumulative Views

**Files:**
- Modify: `apps/desktop/src/features/settings/models/ModelUsageInsightsPanel.tsx`
- Modify: `apps/desktop/src/shared/i18n/locales/en-US.ts`
- Modify: `apps/desktop/src/shared/i18n/locales/zh-CN.ts`

1. Add localized chart labels and an explicit empty state.
2. Render weekly values with a responsive bar chart and preserve true zero values.
3. Render cumulative values with a responsive area chart using `var(--info)` and `var(--border)`.
4. Use sampled axis ticks and a shared localized tooltip.

### Task 4: Finish Daily Heatmap

**Files:**
- Modify: `apps/desktop/src/features/settings/models/ModelUsageInsightsPanel.tsx`
- Modify: `apps/desktop/src/features/settings/models/ModelUsageInsightsPanel.test.tsx`

1. Fit a full 53-week range without clipping.
2. Place localized month labels on one row; keep year context in the selected-date detail to avoid label collisions.
3. Add an intensity legend and Radix tooltip.
4. Replace hundreds of tab stops with roving keyboard focus and visible focus styles.

### Task 5: Verify

1. Run the focused component test and confirm it passes.
2. Run desktop typecheck, Biome, all unit tests, and Storybook build.
3. Capture daily, weekly, and cumulative Storybook screenshots at desktop and narrow widths.
4. Run code review and address all important findings.
