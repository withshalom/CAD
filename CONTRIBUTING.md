# Working on BlackCAD

How changes get from an idea to `main`. The rules about what may go into
the code at all (no third-party code, published specifications only) are
in [`docs/PROVENANCE.md`](docs/PROVENANCE.md) and come first.

## Where the project lives

- **The GitHub repository is the source of truth.** `main` always builds
  and passes CI; every release is a tag on `main`.
- **Forks are for experiments.** Work in a fork or a branch, and bring it
  back as a pull request.
- **The claude.ai Project holds a copy of each release** for reading and
  discussion. It is refreshed after a release, not edited on its own: a
  change made there first goes into a branch here before it counts.

## Branches

- Nothing is committed to `main` directly. Every change gets a branch,
  named for its kind: `fix/…`, `feat/…`, `docs/…`, `build/…`, `test/…`.
- Keep branches short-lived and about one thing. Rebase on `main` before
  asking for a merge.

## Commits

- **One logical change per commit**, and the workspace builds and its
  tests pass at every commit, so `git bisect` and reverting work.
- **Subject:** `area: what changed`, in the imperative, under 72
  characters (`fix: refuse hatch boundaries with no area`). Areas in use:
  `build`, `ci`, `fix`, `feat`, `test`, `docs`, `chore`, `license`,
  `release`.
- **Body:** why the change was needed, what it does that the diff does
  not show, and how it was checked.
- **Mechanical changes stand alone:** reformatting, renames and moved
  files go in commits of their own, never mixed with changes of meaning.
- Commits written with an AI assistant say so with a `Co-Authored-By:`
  trailer.

## Pull requests

- Say what changed, why, and how it was tested; link the issue if there
  is one.
- CI must be green: clippy with warnings denied, the tests on Linux,
  Windows and macOS, the declared minimum Rust version, the Android APK
  and the web build.
- Merge with **Rebase and merge**, so `main` keeps each commit as it was
  reviewed. Squash only a branch whose commits are not worth keeping.

Before pushing:

```
cargo clippy --all-targets -- -D warnings
cargo test --release
```

`crates/cad-ffi/src/web.rs` compiles only for WebAssembly; CI's web job
lints and builds it.

The open crates (`blackcad-model`, `cad-dwg`, `cad-dxf`) take nothing from the
rest of the workspace, and outside their tests they deny the calls that
can panic (`unwrap`, `expect`, `panic!`, `unreachable!`…): other programs
read untrusted files with them. Counts read from a file are claims, never
the size of an allocation by themselves.

## Releases

1. Set the version in `Cargo.toml` (`[workspace.package]`). Until 1.0, a
   change that breaks the API or a file format takes the next minor
   version.
2. Add its entry at the top of `docs/CHANGELOG.md`, with the date and the
   test counts.
3. Merge to `main`, tag it (`git tag -a vX.Y.Z`), push the tag, and make a
   GitHub release from it with CI's build artifacts.
4. Refresh the Project copy from the tag.

## The corpus

`corpus/` holds real drawings that `cargo test` checks. Only drawings the
owner has cleared go in: never customer drawings, and never AutoCAD's
lock files (`.dwl`, `.dwl2`). AutoCAD records the Windows user name and
the path a drawing was saved from in its files; check that before adding
a drawing to a public repository.
