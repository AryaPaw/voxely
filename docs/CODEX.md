# Codex setup

Project instructions live in the root [AGENTS.md](../AGENTS.md). Codex discovers the repository's skills under `.agents/skills`:

- `$voxely-version`: change the confirmed app version through its canonical script.
- `$voxely-improve`: perform a read-only improvement audit.

Personal instructions stay in `~/.codex/AGENTS.md`. Install or maintain ECC through its native Codex marketplace plugin, following [the official ECC installation guide](https://github.com/affaan-m/ECC#codex-app-and-cli). ECC already provides `$ecc:configure-ecc`; the personal `$ap-ecc-install` skill bootstraps installation when that plugin isn't present. Don't copy ECC payloads into the repository or combine native installation with the deprecated global sync flow. Native hook trust is a separate Codex-owned decision.

## Migration from Cursor

The nine project rules formerly in `.cursor/rules` are consolidated in `AGENTS.md`: commit checkpoints, DSP ownership, local runtime, release workflow, runtime acceptance, testing, text insertion, semantic tokens and version ownership. The version skill is now under `.agents/skills/voxely-version`; `/improve` is now `$voxely-improve`.

The migration preserves the current authorization boundaries: commit/publication actions require authorization, and published tags aren't rewritten automatically after a failed release. These replace conflicting automatic-commit and retag directions from the old Cursor files.

On 2026-10-08 the old `.cursor` tree was archived locally under `.local/cursor-migration/20261008`: `cursor.zip`, a SHA-256 manifest and `retired-cursor/`. The 831 source files matched their archive hashes before retirement. This is a local recovery copy, not a checked-in dependency or an active instruction source.

Start a new Codex task/session to rebuild the project instruction chain. Newly installed skills are discovered on subsequent turns; restart Codex if they remain absent. A successful plugin inventory/cache check proves installation, not execution of hooks or acceptance of Voxely's native runtime.
