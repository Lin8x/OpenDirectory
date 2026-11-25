# Contributing

## Setup

```bash
git clone https://github.com/Lin8x/OpenDirectory && cd OpenDirectory
cargo x setup     # installs UI deps, rustfmt + clippy, git hooks, runs `cargo x doctor`
```

`cargo x doctor` tells you exactly what is missing for your OS (Rust, Node 20+, WebKitGTK on Linux, Xcode tools on macOS, WebView2 on Windows) and how to fix it.

## Workflow

1. Branch from `main`.
2. Make your change. `cargo x dev` runs the Admin Center with hot reload; `cargo x dev --browser` runs it in a browser against a dev server.
3. `cargo x check` runs exactly what CI runs (format, clippy, tests, UI typecheck).
4. Commit with `cargo x commit`, or by hand using Conventional Commits.
5. Open a pull request. CI builds the agent and the Admin Center on Windows, macOS and Linux.

## Commit messages

We use [Conventional Commits](https://www.conventionalcommits.org). The changelog is generated from them.

```
<type>(<optional scope>): <summary in the imperative, ≤ 72 chars>

<optional body>

BREAKING CHANGE: <optional description>
```

Types: `feat`, `fix`, `docs`, `style`, `refactor`, `perf`, `test`, `build`, `ci`, `chore`, `revert`.
Scopes we use: `agent`, `server`, `cli`, `app`, `core`, `client`, `xtask`, `docs`.

## Git hooks

`cargo x hooks` (run by `setup`) points git at `.githooks/`:

| Hook | What it does |
| --- | --- |
| `commit-msg` | Rejects messages that aren't Conventional Commits |
| `pre-commit` | Checks Rust formatting when `.rs` files are staged |
| `pre-push` | Runs `cargo x lint` |

Skip a hook once with `--no-verify`.

## Code style

- Rust: `cargo x fmt` (config in `rustfmt.toml`); clippy must be clean.
- UI: TypeScript strict mode; components live in `apps/admin-center/src/components/ui.tsx`. Use industry terms (OU, GPO, Security Group, Endpoint) and add a glossary entry in `src/lib/glossary.ts` for any new concept.
- Agent changes that touch the OS must handle all three platforms, or report `Unsupported` in compliance.

## Releasing (maintainers)

`cargo x release <patch|minor|major|X.Y.Z>`. See the README.
