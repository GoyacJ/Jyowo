# Jyowo Styling

## Tokens

- MUST use Tailwind CSS v4.
- MUST define semantic tokens in `apps/desktop/src/shared/styles/global.css`.
- MUST use semantic classes such as `bg-background`, `bg-surface`, `text-foreground`, `text-muted-foreground`, `border-border`.
- MUST NOT hardcode product colors in feature code.
- SHOULD keep cards at `8px` radius or less.

## Components

- MUST keep shadcn-style primitives source-owned under `shared/ui`.
- MUST use `Button`, `Badge`, and `Tooltip` primitives before custom markup.
- MUST use lucide icons inside icon buttons when an icon exists.
- SHOULD use `cn()` for conditional class composition.

## Visual Direction

Jyowo should feel like a dense engineering tool.

- MUST prefer scannable layouts.
- MUST avoid landing-page hero composition for app screens.
- SHOULD avoid one-note palettes.
- SHOULD keep typography compact inside tool surfaces.
