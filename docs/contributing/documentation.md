# Documentation Guidelines

How to write and maintain documentation for Director Engine.

## Documentation Structure

| Location | Purpose |
|----------|---------|
| `docs/architecture/` | System design, internals |
| `docs/contributing/` | Contributor guides |
| `docs/specs/` | Design specifications |
| `crates/*/README.md` | Per-crate documentation |

---

## When to Update Docs

| Change | Update |
|--------|--------|
| New Rhai API | `.agent/skills/director-rhai/resources/API_REFERENCE.md` |
| Architecture change | `docs/architecture/overview.md` |
| New crate | `crates/<name>/README.md` |
| New feature spec | `docs/specs/<name>.md` |

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
