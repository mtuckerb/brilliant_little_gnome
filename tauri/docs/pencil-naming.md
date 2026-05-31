# Pencil → Code Naming Map

Canonical mapping from frames in
[`docs/brilliant_ui.pen`](../../docs/brilliant_ui.pen) (Pencil v2.11) to the
React components, CSS classes, and design tokens that implement them in
`tauri/src/`.

The Pencil file is the **source of truth** for both visuals and naming. If you
change a frame name, color variable, or layout in the `.pen` file, update the
corresponding identifier here and in code — and vice versa.

## Naming rules (from the task AC)

- **kebab-case** Pencil frame names stay kebab-case as CSS classes
  (`course-rows` → `.pencil-course-row`).
- **camelCase** Pencil frame names stay camelCase as JS state / props and CSS
  classes (`appBar` → `.pencil-appBar`, `semesterCoursesPanel` →
  `.pencil-semesterCoursesPanel`).
- **React component files** use the PascalCase form of the frame they root
  (`Desktop Vision` → `DesktopVision.tsx`).
- A component is renamed **only** when a frame maps to it *directly and
  verifiably*; components with no corresponding frame keep their existing name
  (see "Intentionally unchanged" below).

## Design tokens

Defined in [`tauri/src/styles/pencil-tokens.css`](../src/styles/pencil-tokens.css)
as CSS custom properties prefixed `--pencil-*`. The palette block mirrors the
`variables` object at the top of `brilliant_ui.pen` verbatim.

| Pencil variable           | CSS custom property                | Hex      |
| ------------------------- | ---------------------------------- | -------- |
| `$accent`                 | `--pencil-accent`                  | #739AC3  |
| `$accent-dark`            | `--pencil-accent-dark`             | #3D6691  |
| `$bg-app`                 | `--pencil-bg-app`                  | #F4F5F7  |
| `$bg-pane`                | `--pencil-bg-pane`                 | #FFFFFF  |
| `$bg-pane-alt`            | `--pencil-bg-pane-alt`             | #FAFBFC  |
| `$bg-sidebar`             | `--pencil-bg-sidebar`              | #1F2A33  |
| `$bg-sidebar-row-active`  | `--pencil-bg-sidebar-row-active`   | #2A3744  |
| `$border-soft`            | `--pencil-border-soft`             | #E5E8EC  |
| `$danger`                 | `--pencil-danger`                  | #C04A4A  |
| `$success`                | `--pencil-success`                 | #2C8F61  |
| `$text-on-dark`           | `--pencil-text-on-dark`            | #FFFFFF  |
| `$text-on-dark-dim`       | `--pencil-text-on-dark-dim`        | #9DA9B5  |
| `$text-on-dark-faint`     | `--pencil-text-on-dark-faint`      | #5E6975  |
| `$text-primary`           | `--pencil-text-primary`            | #1B2530  |
| `$text-secondary`         | `--pencil-text-secondary`          | #5A6573  |
| `$warn`                   | `--pencil-warn`                    | #D4A437  |

Typography (`--pencil-font-*`) and radii (`--pencil-radius-*`) derive from the
`fontSize` / `cornerRadius` values in the `.pen` frames; both ladders are
defined at the top of the CSS file.

## Renamed this pass (file/component renames)

| Pencil frame (line in `.pen`) | Old file                          | New file                         | Component fn       |
| ----------------------------- | --------------------------------- | -------------------------------- | ------------------ |
| `Desktop Vision` (L9)         | `components/DesktopLayout.tsx`    | `components/DesktopVision.tsx`   | `DesktopVision`    |
| `course-rows` (L3015)         | `components/SidebarCourseList.tsx`| `components/CourseRows.tsx`      | `CourseRows`       |
| `HeaderBand` (L669)           | `components/CourseHeader.tsx`     | `components/HeaderBand.tsx`      | `HeaderBand`       |
| `Due soon` (L1475)            | *(none — newly created)*          | `components/DueSoon.tsx`         | `DueSoon`          |

Import sites updated for every rename:

- `DesktopVision`: `App.tsx` import + the `Shell` ternary; comment in
  `hooks/useIsMobile.ts`.
- `CourseRows`: `components/DesktopVision.tsx` import + `<CourseRows />` usage.
- `HeaderBand`: all 10 course-scoped pages that render it
  (`AssignmentDetail`, `Assignments`, `CourseAnnouncements`, `CourseDetail`,
  `CourseSearch`, `Discussions`, `DiscussionTopic`, `Grades`, `ModuleDetail`,
  `Modules`) plus the comment in `styles.css`.
- `DueSoon`: imported and rendered in `pages/Dashboard.tsx` (desktop branch).

### `DueSoon.tsx` — note on "extraction"

The AC says the Dashboard "Due soon" panel is *extracted* to `DueSoon.tsx`.
There was **no inline Due soon panel** anywhere in the code to extract — the
prior pass's mapping that pointed `Due soon` at a `CourseDetail.tsx` card was
inaccurate (no such card exists). The Pencil `Due soon` frame
(`Desktop Vision > rightCol`, id `RLKQX`) had simply never been implemented.

`DueSoon.tsx` therefore **realizes** that frame as a dashboard widget. It does
not add a new data flow: it reads the same already-synced assignment rows the
Calendar page streams (`api.listCourses` + `api.listAssignments`), filters to
upcoming (≤14 days) incomplete items, and renders the next five — matching the
frame's `dueHdr` + `due<n>` structure. No new backend command. It is placed at
the top of the desktop Dashboard content (not as a separate `rightCol`) so the
existing drag-to-reorder course tables keep working unchanged; the full
two-column `leftCol`/`rightCol` restructure is intentionally out of scope to
avoid regressing that behavior.

## Frame map

### Desktop Vision (`bi8Au`, L9)

| Pencil frame          | Implementation                                                        |
| --------------------- | --------------------------------------------------------------------- |
| `Desktop Vision`      | `components/DesktopVision.tsx` (root shell)                           |
| `TitleBar` (L20)      | `DesktopVision > TitleBar` + `.pencil-TitleBar`                       |
| `Shell` (L132)        | `DesktopVision` flex row wrapping `Sidebar` + `MainPane`              |
| `Sidebar` (L139)      | `DesktopVision <aside class="pencil-Sidebar">`                        |
| `semHdr1` / `semHdr2` | `CourseRows` group label → `.pencil-semHdr`                           |
| sidebar rows          | `CourseRows` `<Link>` rows → `.pencil-sidebar-row` (`.is-active`)     |
| `MainPane` (L636)     | `DesktopVision <main class="pencil-MainPane">`                        |
| `HeaderBand` (L669)   | `components/HeaderBand.tsx` (course header band, all course pages)    |
| `Due soon` (L1475)    | `components/DueSoon.tsx` → `.pencil-DueSoon` / `.pencil-dueHdr` / `.pencil-due-row` |
| `Tabs` (Desktop)      | `components/CourseNav.tsx` (see "Intentionally unchanged")            |
| `StatusBar` (L1816)   | `components/StatusBar.tsx` → `.pencil-StatusBar`                      |

### Mobile Vision - Navigation (`HDUr0`, L1945)

| Pencil frame                        | Implementation                                                |
| ----------------------------------- | ------------------------------------------------------------- |
| `Mobile Vision - Navigation`        | `MobileLayout` rendering the mobile `Dashboard` branch        |
| `appBar` (L1962) / `appTitleWrap`   | `<header class="pencil-appBar">` in `MobileLayout`            |
| `appTitle` (L1981) / `appSub`       | `Dashboard.MobileDashboard` title block → `.pencil-appTitle`, `.pencil-appSub` |
| `statsRow`                          | `MobileDashboard` → `.pencil-statsRow` / `.pencil-stat` / `.pencil-stat-value` / `.pencil-stat-label` |
| `semesterCoursesPanel`              | `MobileDashboard` card → `.pencil-semesterCoursesPanel`       |
| semester chips                      | `MobileDashboard` chip row → `.pencil-semester-chip` (`.is-active`) |
| `courseRow<n>`                      | `MobileDashboard` course list → `.pencil-course-row` / `-title` / `-meta` |
| `bottomNav` (L2582)                 | `<nav class="pencil-bottomNav">` in `MobileLayout`, items → `.pencil-bottomNav-item` (`.is-active`) |

### Mobile Vision - Assignment (`c3DdR`, L2661)

| Pencil frame                        | Implementation                                                |
| ----------------------------------- | ------------------------------------------------------------- |
| `Mobile Vision - Assignment`        | `pages/AssignmentDetail.tsx` inside `MobileLayout`            |
| `appbar` (L2678)                    | Page-level back button + title                                |
| `assn` / `meta` / `due` / `pts`     | → `.pencil-assn`, `.pencil-assn-meta-due`, `.pencil-assn-meta-pts` |
| `course-rows` (L3015) / `course-row-1..N` | `.pencil-course-row` markup (shared with mobile dashboard) |
| `bottomNav`                         | `.pencil-bottomNav` (shared)                                  |

### Mobile Vision - Calendar (`NouE8`, L3256)

| Pencil frame                        | Implementation                                                |
| ----------------------------------- | ------------------------------------------------------------- |
| `Mobile Vision - Calendar`          | `pages/Calendar.tsx` inside `MobileLayout`                    |
| `appBar` (L3273) / `appTitleWrap` / `appTitle` (L3292) | `Calendar` page header              |
| `bottomNav` (L3364)                 | `.pencil-bottomNav` (shared)                                  |

## Intentionally unchanged (with rationale)

These files keep their current names. Each was checked against the `.pen`
frame list before deciding *not* to rename.

| File                          | Why it is not renamed                                                                 |
| ----------------------------- | ------------------------------------------------------------------------------------- |
| `components/StatusBar.tsx`    | Already matches the verbatim `StatusBar` frame (L1816). Verified it is **not** `appBar`/`bottomNav` — those are *mobile* frames (L1962/L2582/L3273/L3364); `StatusBar` is the desktop bottom strip. No rename needed. |
| `components/MobileLayout.tsx` | There is **no single mobile root frame** to rename to. The `.pen` has three per-screen frames (`Mobile Vision - Navigation` / `- Assignment` / `- Calendar`); `MobileLayout` is the reusable chrome shell (appBar + content + bottomNav) that has no standalone frame. `appBar`/`bottomNav` are applied as `.pencil-*` classes inside it. |
| `components/CourseNav.tsx`    | Maps to the `Tabs` frame, but `Tabs.tsx` is too generic a filename (collision-prone) and `Tabs` is a sub-region, not a screen root. The `Tabs` identity is carried by the `.pencil-tab*` classes instead. |
| Page components (`Dashboard`, `Calendar`, `Settings`, `Grades`, `Assignments`, etc.) | Route-level components named after the *route*, not a design frame. They render the relevant `Mobile Vision - *` screens via CSS classes; no 1:1 frame justifies a file rename. |

## Build / test status

- `npm run build` (`tsc && vite build`): **passes**.
- `npm test` (`vitest run`): **passes** (6 tests; none reference the renamed
  components).
- `npm run dev` / `dev:ios` / `ios:build`: see the PR description for
  environment status — the iOS toolchain (Xcode) is not available in the
  headless agent worktree, so `dev:ios` / `ios:build` are documented as unrun
  there; `vite build` exercises the same web bundle the iOS WebView loads.
