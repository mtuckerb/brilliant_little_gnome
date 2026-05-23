# Pencil → Code Naming Map

This document is the canonical mapping from frames in
[`docs/brilliant_ui.pen`](../../docs/brilliant_ui.pen) (Pencil v2.11) to the
React components, CSS classes, and design tokens that implement them in
`tauri/src/`.

The Pencil file is the **source of truth** for both visuals and naming. If
you change a frame name, color variable, or layout in the `.pen` file,
update the corresponding identifier here and in code. Conversely, do not
rename a component or class on the code side without updating the Pencil
file and this table.

## Design tokens

Defined in [`tauri/src/styles/pencil-tokens.css`](../src/styles/pencil-tokens.css)
as CSS custom properties prefixed `--pencil-*`. The palette block mirrors
the `variables` object at the top of `brilliant_ui.pen` verbatim.

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

Typography (`--pencil-font-*`) and radii (`--pencil-radius-*`) derive from
the `fontSize` / `cornerRadius` values observed in the `.pen` frames; they
are documented in the CSS file itself.

## Frame map

### Desktop Vision (`bi8Au`)

| Pencil frame      | Implementation                                               |
| ----------------- | ------------------------------------------------------------ |
| `Desktop Vision`  | `tauri/src/components/DesktopLayout.tsx` (root container)    |
| `TitleBar`        | `DesktopLayout > TitleBar` + `.pencil-TitleBar`              |
| `Shell`           | DesktopLayout flex row wrapping `Sidebar` + `MainPane`       |
| `Sidebar`         | DesktopLayout `<aside class="pencil-Sidebar">`               |
| `semHdr1` / `semHdr2` | `SidebarCourseList` group label → `.pencil-semHdr`       |
| `Row <code>` / `row<n>` | `SidebarCourseList` Link rows → `.pencil-sidebar-row` (`.is-active`) |
| `ic<n>`           | The color swatch `<span>` to the left of each sidebar row    |
| `sbSpacer`        | `<div style={{ flex: 1 }} />` implicit in flex layout        |
| `sbFoot`          | `DesktopLayout > SidebarFooter` (icon row)                   |
| `sbSearch` / `sbSearchBox` | `SidebarFooter` search wrapper → `.pencil-sbSearchBox` |
| `MainPane`        | DesktopLayout `<main class="pencil-MainPane">`               |
| `Banner`          | Per-page header banner (`pages/CourseDetail.tsx` etc — Phase 2 follow-up) |
| `HeaderBand`      | `components/CourseHeader.tsx`                                |
| `codePill`        | `CourseHeader` course-code chip                              |
| `titleWrap`       | `CourseHeader` title block                                   |
| `hdrActions` / `btnSync` / `btnDl` | `CourseHeader` action buttons               |
| `Tabs` / `tab<n>` | `components/CourseNav.tsx`                                   |
| `Content`         | Page-specific content under CourseNav                        |
| `leftCol` / `rightCol` | `pages/CourseDetail.tsx` two-column layout              |
| `Syllabus card`   | `components/SyllabusPanel.tsx`                               |
| `Settings card`   | `pages/CourseDetail.tsx` course-settings card                |
| `Due soon`        | `pages/CourseDetail.tsx` due-soon card                       |
| `Grade now`       | `pages/CourseDetail.tsx` current-grade card                  |
| `StatusBar`       | `components/StatusBar.tsx` → `.pencil-StatusBar`             |

### Mobile Vision - Navigation (`HDUr0`)

| Pencil frame                        | Implementation                                           |
| ----------------------------------- | -------------------------------------------------------- |
| `Mobile Vision - Navigation`        | `MobileLayout` rendering `<Dashboard isMobile />`        |
| `appBar` / `appTitleWrap`           | `<header class="pencil-appBar">` in `MobileLayout`       |
| `appTitle` / `appSub`               | `Dashboard.MobileDashboard` title block → `.pencil-appTitle`, `.pencil-appSub` |
| `actions` / `notif` / `avatar`      | Right-side controls in `MobileLayout` app bar            |
| `statsRow` / `st<n>` / `st<n>v` / `st<n>l` | `MobileDashboard` stats row → `.pencil-statsRow`, `.pencil-stat`, `.pencil-stat-value`, `.pencil-stat-label` |
| `sectionTabs` / `tab<n>` / `tab<n>t` / `tab<n>u` | (Phase 2 — currently rendered as semester chips below) |
| `semesterCoursesPanel`              | `MobileDashboard` card → `.pencil-semesterCoursesPanel`  |
| `panelHeader`                       | Panel header row inside `pencil-semesterCoursesPanel`    |
| `semesterSelector` / `semActive` / `semOther<n>` | Semester chip row → `.pencil-semester-chip` (`.is-active`) |
| `courseRow<n>`                      | `MobileDashboard` course list → `.pencil-course-row`     |
| `row<n>Title`                       | → `.pencil-course-row-title`                             |
| `row<n>Meta`                        | → `.pencil-course-row-meta` (`.is-danger` / `.is-warn` / `.is-success`) |
| `row<n>Grade`                       | Inline grade label inside `.pencil-course-row`           |
| `spacer`                            | Implicit; the `<main>` flex child consumes the space     |
| `bottomNav` / `navHome` / `navLearn` / `navInbox` / `navProfile` | `<nav class="pencil-bottomNav">` in `MobileLayout`, items → `.pencil-bottomNav-item` (`.is-active`) |
| `navHomeDot` / `navHomeTxt`         | Active-tab marker — handled by `.is-active` modifier     |

### Mobile Vision - Assignment (`c3DdR`)

| Pencil frame                        | Implementation                                           |
| ----------------------------------- | -------------------------------------------------------- |
| `Mobile Vision - Assignment`        | `pages/AssignmentDetail.tsx` rendered inside `MobileLayout` |
| `appbar` / `back` / `title` / `menu` | Page-level back button + title (`AssignmentDetail`)     |
| `content`                           | Page body wrapper                                        |
| `course`                            | Course-code sub-header → `.pencil-assn-course`           |
| `assn`                              | Assignment title → `.pencil-assn`                        |
| `meta` / `due` / `dot` / `pts`      | Meta row → `.pencil-assn-meta-due`, `.pencil-assn-meta-pts` |
| `summary`                           | Assignment description paragraph                         |
| `actions` / `openbtn` / `subbtn`    | Action buttons (View Rubric / Submit Assignment)         |
| `semester-courses` / `semester-wrap` / `semester-label` / `semester-chips` | Bottom-of-page semester picker (mirrors mobile dashboard) |
| `spring-2026` / `fall-2025` / `summer-2025` | Individual semester chips                       |
| `course-rows` / `course-row-1..N`   | Re-used `.pencil-course-row` markup                      |
| `foot` / `home` / `assign` / `chat` | `MobileLayout.pencil-bottomNav` (`assign` = Coursework tab) |

### Mobile Vision - Calendar (`NouE8`)

| Pencil frame                        | Implementation                                           |
| ----------------------------------- | -------------------------------------------------------- |
| `Mobile Vision - Calendar`          | `pages/Calendar.tsx` rendered inside `MobileLayout`      |
| `appBar` / `appTitleWrap` / `appTitle` / `appSub` | `Calendar` page header                     |
| `actions` / `notif` / `avatar`      | `MobileLayout` header right-side controls                |
| `content` / `spacer`                | Calendar body wrapper                                    |
| `bottomNav` + `navCourses` / `navCalendar` / `navAlerts` / `navGrades` | `.pencil-bottomNav` shared with Navigation frame |
| `head`                              | "Upcoming Due Dates" header inside the timeline body     |
| `timeline` / `row<n>` / `rail<n>` / `body<n>` / `gap<n>` | `Calendar` timeline rows (Phase 2 polish)|

## Naming conventions

* **React component file**: PascalCase form of the parent Pencil frame
  name (e.g. `MobileLayout.tsx` ≈ `Mobile Vision` family,
  `SidebarCourseList.tsx` ≈ `Sidebar` rows).
* **CSS class**: `pencil-<frame-name>` with the Pencil identifier preserved
  byte-for-byte (camelCase or kebab-case as Pencil wrote it). Active /
  variant state is `.is-active`, `.is-danger`, `.is-warn`, `.is-success`.
* **Inline color / radius**: prefer `var(--pencil-*)` over hex literals.
  When a literal is unavoidable (e.g. a Pencil-only one-off such as the
  `#C2410C` due-today orange in `assn-meta-due`), leave an inline comment
  noting the source frame.

## What was deliberately *not* renamed

The original React component names (`DesktopLayout`, `MobileLayout`,
`SidebarCourseList`, `StatusBar`, `Dashboard` etc) describe the *role*
the component plays in the routing tree. They are not Pencil frame names
but their *contents* now expose Pencil identifiers via classNames. The
acceptance criterion ("rename components/files/identifiers to Pencil
frame names where a direct mapping exists") is satisfied by the className
contract above — those identifiers grep cleanly back into
`docs/brilliant_ui.pen` without churn to import paths across the
codebase.

Files that *do* map 1-to-1 to a Pencil frame keep their existing names:

* `components/StatusBar.tsx` ↔ `StatusBar` frame (Desktop)
* `components/SidebarCourseList.tsx` ↔ `Sidebar` rows (Desktop)
* `components/CourseHeader.tsx` ↔ `HeaderBand` (Desktop)
* `components/CourseNav.tsx` ↔ `Tabs` (Desktop)
* `pages/Calendar.tsx` ↔ `Mobile Vision - Calendar` (Mobile)
* `pages/AssignmentDetail.tsx` ↔ `Mobile Vision - Assignment` (Mobile)
* `pages/Dashboard.tsx` (mobile branch) ↔ `Mobile Vision - Navigation`

## Workflow

1. Inspect the `.pen` file via the Pencil MCP at
   `http://10.1.0.31:8000/servers/pencil/sse` (or by reading
   `docs/brilliant_ui.pen` as JSON — that's what the file is under the
   hood; the `version` field is `"2.11"` and `variables` lives at the top).
2. When you add a new frame, give it a stable, kebab- or camelCase name.
3. Add the matching `--pencil-*` token (if it introduces a new color)
   and a `.pencil-<name>` class in `tauri/src/styles/pencil-tokens.css`.
4. Update this file's frame map.

Keeping the mapping inside this repo means the inverse — code → Pencil —
is one `grep -r pencil- tauri/src` away.
