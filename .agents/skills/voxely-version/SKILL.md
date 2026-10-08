---
name: voxely-version
description: Change Voxely's app version through its canonical script after the user confirms the target version.
---

# Voxely version

Read the Version and build date section of the repository's root `AGENTS.md`. Keep version selection, branch push, tag and GitHub Release authorization separate.

If no exact number or increment is confirmed, propose patch/minor/major with a reason and wait. Don't infer version authorization from ordinary implementation work.

Run from the repository root:

```text
pwsh -File scripts/set-version.ps1 -To X.Y.Z
pwsh -File scripts/check-version.ps1 -Expected X.Y.Z
```

The setter also accepts a confirmed `patch`, `minor` or `major`; verify the resulting exact version. It updates `package.json` and syncs Cargo metadata. Tauri's version remains the `../package.json` reference. Don't manually repeat the bump across manifests or write an About date into source.

If the running app must show the new number, follow the Local runtime section of `AGENTS.md` and run `bun run rebuild:local-app`. This is not a GitHub Release.

For separately authorized publication, follow the Release workflow in `AGENTS.md` and `ap-release-prep`. Never rewrite an existing tag without explicit authorization.
