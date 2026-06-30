# Purpose

- Own the Director preview application: Rust backend server, static serving, and React frontend.

## Ownership

- `src/main.rs` owns the Axum preview backend.
- `static/` owns backend-served static assets.
- `frontend/` owns the React, Vite, TypeScript, Tailwind, and Zustand application.

## Local Contracts

- Backend API endpoints must stay aligned with the frontend API client.
- Static build output belongs under `static/` only when intentionally produced for serving.
- Frontend-specific contracts live in `frontend/AGENTS.md`.

## Work Guidance

- Test backend changes with `cargo test -p director-view` or a local server run when endpoints change.
- Coordinate API shape changes across `src/main.rs` and `frontend/src/api/director.ts`.

## Verification

- `cargo test -p director-view`
- Frontend verification from `frontend/AGENTS.md` when frontend files change.

## Child DOX Index

- `frontend/AGENTS.md` - React preview UI, TypeScript, Vite, Tailwind, and frontend package workflow.
