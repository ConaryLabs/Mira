# Testing

## Unit & Integration Tests (Rust)

```bash
cargo test              # run all tests
cargo test --all-features
```

## Install Tests

Two layers verify that the wrapper and installer scripts work end-to-end against real GitHub release artifacts.

### CI: `.github/workflows/test-install.yml`

Runs automatically on every `release: published` event. Can also be triggered manually via `workflow_dispatch` (with an optional `version` input — defaults to the latest release tag).

**Jobs:**

| Job | Runners | What it tests |
|-----|---------|---------------|
| `test-wrapper` | ubuntu-latest, macos-latest | `plugin/bin/mira-wrapper` auto-download flow |
| `test-installer` | ubuntu-latest | `install.sh` standalone installer |

**Wrapper test assertions (per OS):**

1. `MIRA_VERSION` floor in wrapper matches the release tag (fast-fail gate)
2. First run downloads binary → `~/.mira/bin/mira` exists and is executable
3. Version file `~/.mira/bin/.mira-version` matches expected version
4. stderr contains install/update log
5. `mira --version` outputs the correct version string
6. Update cache file `~/.mira/.last-update-check` created after first run
7. Second run (fast path) produces no download log in stderr
8. Version pinning via `MIRA_VERSION_PIN` env var downloads and installs the pinned version

**Installer test assertions:**

1. Binary installed to `$MIRA_INSTALL_DIR` and executable
2. `mira --version` outputs correct version

**Manual trigger:**

```bash
gh workflow run test-install.yml                    # test latest release
gh workflow run test-install.yml -f version=0.3.7   # test specific version
```

### Local: `scripts/test-install.sh`

Podman-based multi-distro test script. Tests the same wrapper + installer flows inside containers against multiple Linux distributions.

**Default distros:** `ubuntu:24.04`, `debian:12`, `fedora:43`, `alpine:latest`

```bash
./scripts/test-install.sh                        # all distros
./scripts/test-install.sh --distro alpine:latest # single distro
CONTAINER_RT=docker ./scripts/test-install.sh    # use Docker instead of Podman
MIRA_TEST_VERSION=0.3.6 ./scripts/test-install.sh  # override version
```

**What each container run tests:**

1. Wrapper first run — downloads binary, creates version file, logs to stderr
2. Wrapper second run — fast path via `exec`, no re-download
3. `install.sh` — downloads binary to custom `MIRA_INSTALL_DIR`
4. Binary execution (`--version`) — soft assertion, reports WARN on incompatible libc

**Known platform limitations:**

| Distro | Install mechanism | Binary execution |
|--------|------------------|-----------------|
| ubuntu:24.04 | PASS | PASS |
| debian:12 | PASS | WARN — needs GLIBC 2.38+, Debian 12 ships 2.36 |
| fedora:43 | PASS | PASS |
| alpine:latest | PASS | WARN — musl libc, binary built for glibc |

Binary execution warnings are expected — the release binary is built on `ubuntu-latest` (glibc 2.39). These distros correctly exercise the shell script logic (POSIX sh compat, different package managers) even though the downloaded binary can't run on their libc.

**Alpine specifically tests:** POSIX `sh` compatibility of the wrapper (no bash), `apk` package manager, musl libc detection.

### Verifying a failure

To confirm the version-check gate catches mismatches, edit `MIRA_VERSION` in `plugin/bin/mira-wrapper` to a bogus value like `0.0.0` and run either test — the wrapper version check will fail immediately.
