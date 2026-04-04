# UI Flat Surface Redesign Task Checklist

## Phase 0 - Contract Alignment
- [x] Create a dedicated implementation branch
- [x] Read `README.md`
- [x] Read `AGENTS.md`
- [x] Read `CLAUDE.md`
- [x] Read the redesign delta from the attached source file
- [x] Inspect shared frontend shell and styling hotspots
- [x] Record repo drift for the missing in-repo delta path
- [x] Create the implementation log
- [x] Create this task checklist

## Phase 1 - Shared Flat Visual Primitives and Button Tuning
- [x] Add non-AI flat primitives in [globals.css](/Users/iwanteague/Desktop/Rustyfin/ui/src/app/globals.css)
- [x] Add non-AI button calming scoped away from `/ai`
- [x] Flatten notification popover shell/item styling
- [x] Add a flat `SurfaceTabsBar` variant without breaking existing callers
- [x] Preserve nav underline gradient
- [x] Preserve destructive confirmation modal styling/behavior
- [x] Run frontend validation for Phase 1
- [x] Record results in the implementation log

## Phase 2 - Home, Libraries, Downloads, Network, Backups, Smart Home
- [x] Remove the home welcome card
- [x] Flatten home continue watching rows
- [x] Flatten home open room rows
- [x] Flatten home calendar preview structure
- [x] Flatten library rows and related sections
- [x] Flatten downloads hero and artifact list
- [x] Flatten network hero, node list, and status dots
- [x] Flatten backups hero, history, and policies
- [x] Flatten smart-home hero and device list
- [x] Run frontend validation for Phase 2
- [x] Record results in the implementation log

## Phase 3 - Channels Workspace Rebase
- [x] Remove the outer channels workspace window shell
- [x] Keep the channel workspace full-height and page-level
- [x] Flatten sidebar rows and context menu chrome
- [x] Simplify the text composer/input shell
- [x] Flatten voice transcript/session rows
- [x] Flatten channel user settings modal internals
- [x] Verify composer remains pinned at the bottom
- [x] Run frontend validation for Phase 3
- [x] Record results in the implementation log

## Phase 4 - Account, Rooms Index, Room Detail
- [x] Remove the account hero card
- [x] Flatten account section wrappers and field shells
- [x] Flatten rooms index create-room flow
- [x] Flatten invites/options/invite picker component shells
- [x] Flatten room detail non-functional outer wrappers
- [x] Flatten room detail membership and invite panes
- [x] Preserve destructive end-room confirmation behavior
- [x] Run frontend validation for Phase 4
- [x] Record results in the implementation log

## Phase 5 - Calendar, Items, Player Polish
- [x] Flatten the calendar main/side shells and event rows
- [x] Flatten item detail surrounding chrome
- [x] Reduce decorative non-functional player chrome without impacting playback UX
- [x] Run frontend validation for Phase 5
- [x] Record results in the implementation log

## Phase 6 - Admin, Servers, Vault
- [x] Flatten admin tabs and dense section shells
- [x] Flatten servers sections, forms, and status indicators
- [x] Remove status glow shadows on the servers page
- [x] Flatten vault hero, stats, and feature-owned sections
- [x] Preserve destructive admin/vault flows
- [x] Run frontend validation for Phase 6
- [x] Record results in the implementation log

## Final Validation
- [x] `/ai` is visually unchanged
- [x] Nav hover underline gradient still works
- [x] Non-AI primary buttons are calmer
- [x] Non-AI surfaces do not rely on box shadows
- [x] Home welcome header is flat
- [x] Libraries are flat rows instead of windows
- [x] Channels workspace is page-level with pinned composer
- [x] Notifications are flat rows without per-item cards
- [x] Account page has no hero card or nested section cards
- [x] Rooms are flatter and destructive confirms still work
- [x] Utility/admin pages match the flatter visual language
- [x] `npm --prefix ui run build` passes
- [x] No TypeScript route regressions were introduced

## Completion Summary
- [x] All phases completed
- [x] Implementation log fully updated
- [x] Work committed on the feature branch
- [x] Ready for review

## Post-Review Refinements
- [x] Remove redundant Libraries descriptive copy and tighten header spacing
- [x] Reduce Libraries Continue Watching row gaps
- [x] Move Rooms create flow into an overlay dialog triggered from a single page button
- [x] Run frontend validation for this refinement pass
- [x] Record final validation result in the implementation log
