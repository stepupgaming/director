# Documentation Guidelines

How to write and maintain documentation for Director Engine.

## Documentation Structure

| Location | Purpose |
|----------|---------|
| `docs/architecture/` | System design, internals, **core contracts** |
| `docs/plans/` | Implementation plans; the active plan holds current status, measurements, and the next action |
| `docs/contributing/` | Contributor guides |
| `docs/archive/` | Superseded / deferred docs (not current contracts) |
| `crates/*/AGENTS.md` | Per-crate behavior contracts |
| `crates/*/README.md` | Per-crate documentation |

---

## When to Update Docs

| Change | Update |
|--------|--------|
| New Rhai API | `.agent/skills/director-rhai/resources/API_REFERENCE.md` |
| Architecture / frame loop | `docs/architecture/overview.md` |
| Time, identity, pipeline, anim, scene, determinism spines | `docs/architecture/core-contracts.md` + relevant `docs/plans/0*.md` |
| New crate | `crates/<name>/README.md` |
| Finished or superseded plan | Plan status + `docs/plans/00-core-hardening-index.md`; move dead product drafts to `docs/archive/` |
| A test re-run | Replace that check's row in the active plan's measurement table; do not add counts anywhere else |

---

## Writing Style

1. **Be concise** — Keep paragraphs short
2. **Use tables** — For structured data
3. **Use code blocks** — For all code examples
4. **Use consistent formatting** — Headers, lists, emphasis

### AI Agent Considerations

- Keep file sizes manageable for context windows
- Use tables for quick scanning
- All public APIs should have examples

---

## Spec Lifecycle

Specs have a lifecycle:
- **Draft** — Initial proposal
- **In Review** — Under discussion
- **Approved** — Ready for implementation
- **Implemented** — Merged to main
- **Superseded** — Replaced by newer spec

---

*Documentation is a feature. Treat it with the same care as code.*
