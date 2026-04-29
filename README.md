# mini-ci

Single-binary CI server with a built-in web UI. You register Git repositories, pick shell scripts under `.mini-ci/`, and run them against a branch; logs stream in the browser. Successful runs can zip your build output (`dist`) as a downloadable artifact.

## Requirements

- **Git** on your `PATH` (clone and fetch).
- **macOS or Linux** on the machine running `mini-ci`. Task scripts are executed with `/bin/sh` from the repository root (`[runner.rs](src/runner.rs)`).

## Install

**Prebuilt binaries** for tagged releases are attached to **GitHub Releases** (`linux-x86_64`, `linux-aarch64`, `macos-aarch64`). Extract the archive and run `./mini-ci`.

**From source** (Rust + Node for the embedded UI):

```bash
./.mini-ci/build.sh    # or: cd frontend && npm ci && npm run build && cd .. && cargo build --release
```

The binary is `target/release/mini-ci` (and copied to `dist/mini-ci` by the script).

## Run

By default the server listens on `**127.0.0.1:8787**` (localhost only).

```bash
mini-ci
mini-ci --port 9000
mini-ci --dir /path/to/data
```

**Data directory** (SQLite DB, cloned repos, artifact files):

- `**--dir PATH`**, or
- `**MINICI_DATA**`, or
- `**~/.mini-ci**` if neither is set.

Open **[http://127.0.0.1:8787](http://127.0.0.1:8787)** (or your chosen port) in a browser.

Optional logging: `RUST_LOG=debug mini-ci` for more verbose traces.

## Projects

In the UI, create a **project** with:


| Field              | Meaning                                                                                                                                                                                                                                        |
| ------------------ | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| **Repository URL** | Remote Git URL to clone (shallow clone, single branch).                                                                                                                                                                                        |
| **Branch**         | Branch to build (`git clone … --branch`).                                                                                                                                                                                                      |
| **Dist path**      | Path **relative to the repo root** where your build writes output (e.g. `dist` or `frontend/dist`). After a **successful** task (exit code `0`), if this path exists **and is a directory**, mini-ci zips it and stores it as an **artifact**. |
| **Name**           | Display name only.                                                                                                                                                                                                                             |


The server keeps one clone per project under `<data>/repos/<project-id>/`.

## Tasks (`.mini-ci` scripts)

Tasks are **executable shell scripts** named `*.sh` in a `**.mini-ci`** directory at the **root of the repository**. The file name (e.g. `build.sh`) is the task name shown in the UI.

Example layout:

```text
your-repo/
  .mini-ci/
    build.sh
    test.sh
  src/
  dist/          # if dist_path is set to "dist"
```

Each run:

1. Updates the repo (clone or fetch for the configured branch).
2. Runs `**.mini-ci/<task>.sh**` with working directory = repo root and `/bin/sh`.
3. Streams **stdout** / **stderr** into the run log.
4. If the script exits with **0**, tries to zip the project’s **dist path** (if that path is a directory) and records an artifact.

Write scripts so they create the **dist** directory your project expects when they succeed.

## Artifacts

Artifacts are ZIP files stored under `<data>/artifacts/<project-id>/`. You can download them from the UI when browsing a project’s artifacts list.

## Development

- **Rust**: Axum, SQLite (`bundled`), Tokio (`[src/](src/)`).
- **UI**: `[frontend/](frontend/)` (Vite + React). `cargo build` runs `[build.rs](build.rs)`, which invokes `npm ci` and `npm run build` in `frontend/` so the production bundle is embedded in the binary.

After changing code, rebuild with `.mini-ci/build.sh` or `cargo build --release`.