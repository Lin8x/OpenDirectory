# Contributing

## Setup

Every task below has a script in `scripts/` (`.sh` on Linux and macOS, `.ps1` on Windows). `cargo x <task>` does the same thing if you prefer it. New to Rust? Read [docs/GUIDE.md](docs/GUIDE.md) first.

```bash
git clone https://github.com/Lin8x/OpenDirectory && cd OpenDirectory
./scripts/setup.sh     # Windows: .\scripts\setup.ps1. Installs UI deps, rustfmt + clippy, git hooks, runs doctor
```

`./scripts/doctor.sh` tells you exactly what is missing for your OS (Rust, Node 20+, WebKitGTK on Linux, Xcode tools on macOS, WebView2 on Windows) and how to fix it.

## Workflow

1. Branch from `main`.
2. Make your change. `./scripts/dev.sh` runs the Admin Center with hot reload; `./scripts/dev.sh --browser` runs it in a browser against a dev server.
3. `./scripts/check.sh` runs exactly what CI runs (format, clippy, tests, UI typecheck).
4. Commit with `./scripts/commit.sh`, or by hand using Conventional Commits.
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

`./scripts/hooks.sh` (run by `setup`) points git at `.githooks/`:

| Hook | What it does |
| --- | --- |
| `commit-msg` | Rejects messages that aren't Conventional Commits |
| `pre-commit` | Checks Rust formatting when `.rs` files are staged |
| `pre-push` | Runs the `lint` task |

Skip a hook once with `--no-verify`.

## Code style

- Rust: `./scripts/fmt.sh` (config in `rustfmt.toml`); clippy must be clean.
- UI: TypeScript strict mode; components live in `apps/admin-center/src/components/ui.tsx`. Use industry terms (OU, GPO, Security Group, Endpoint) and add a glossary entry in `src/lib/glossary.ts` for any new concept.
- Agent changes that touch the OS must handle all three platforms, or report `Unsupported` in compliance.

## Releasing (maintainers)

`./scripts/release.sh <patch|minor|major|X.Y.Z>`. See the README.
