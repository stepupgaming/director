# Purpose

- Own the React preview UI for Director.

## Ownership

- `src/api/` owns backend API client calls.
- `src/components/` owns preview, timeline, controls, and script editor components.
- `src/stores/` owns Zustand project state.
- `src/styles/` owns Tailwind and global styling.
- `public/` owns static frontend assets.

## Local Contracts

- Use Bun for package scripts because the lockfile is `bun.lock`.
- Keep UI behavior aligned with backend endpoints documented in `README.md`.
- Keep common editing workflows usable: script editing, preview rendering, timeline scrubbing, and playback controls.

## Work Guidance

- Prefer existing component and store patterns before adding new state structure.
- Use familiar controls and keep operational UI dense and scannable.
- API client changes must be matched with backend route changes in `crates/director-view/src/main.rs`.

## Verification

- `bun run build`
- `bun run lint`
- When running locally: `bun run dev`

## Child DOX Index

- No child AGENTS.md files currently required.
