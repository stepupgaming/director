# Purpose

- Own RON feature specs consumed by `director-developer`.

## Ownership

- Each `.ron` file defines a feature request, related system truth, proposed schema/script/pipeline changes, and verification expectations.

## Local Contracts

- Specs must remain valid RON for the `director-developer` parser.
- Specs should reference real types, functions, and affected areas that can be validated by current reflectors.
- Update specs when related feature work changes scope, contracts, or verification requirements.

## Work Guidance

- Create new specs with `cargo run -p director-developer -- new "<Feature Name>" --template <type> --output specs`.
- Validate changed specs with the developer CLI before implementation when feasible.

## Verification

- `cargo run -p director-developer -- validate --spec specs/<file>.ron`
- `cargo run -p director-developer -- generate --spec specs/<file>.ron --output <context>.md` when implementation context is needed.

## Child DOX Index

- No child AGENTS.md files currently required.
