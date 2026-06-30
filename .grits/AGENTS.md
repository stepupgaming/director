# Purpose

- Own local issue tracking and focus records used during development.

## Ownership

- `issues.jsonl` and backup JSONL files are issue records.
- `focus` records current local focus state.

## Local Contracts

- Preserve JSONL line structure when editing issue records.
- Do not treat backups as source of truth unless restoring data is the task.
- Avoid mixing implementation notes into durable code or documentation contracts.

## Work Guidance

- Keep changes narrowly scoped to the requested issue or focus operation.

## Verification

- For JSONL edits, verify each changed line remains valid JSON.

## Child DOX Index

- No child AGENTS.md files currently required.
