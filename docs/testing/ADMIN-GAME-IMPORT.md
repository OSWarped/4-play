# Admin game-file imports

## Supported workflow

The Admin **Add a game** panel stages files you supply, asks the installed MAME
build for metadata/dependencies, audits them, and requires an explicit install
confirmation. No ROM, BIOS, device, CHD or sample downloads are implemented.

Supported input:

- `.zip` / `.7z` game, parent, BIOS and device archives, kept compressed.
- `.chd` disk images placed in the machine subfolder reported by MAME XML.
- Optional ZIP packs of WAV samples, in the separate configured sample path.

QSound is a **device ROM**, not a WAV sample pack. Current MAME may expect an
archive such as `qsound_hle.zip`, while another version may use a different
device name. Neo Geo BIOS dependencies are identified through MAME's `romof`
chain. Dependencies can already be embedded in non-merged archives: **the audit,
not the mere presence of a separate ZIP, determines readiness**.

See MAME's [ROM/set explanation](https://docs.mamedev.org/usingmame/aboutromsets.html)
and [audit/path commands](https://docs.mamedev.org/commandline/commandline-all.html).

## One-time setup on the reference Linux host

This first version supports a control plane and MAME runtime on the **same
machine and service user**, one configured ROM root, and the first configured
sample path. It is not a remote-host file distribution service.

After pulling/building `control-plane-server` and `runtime-host-agent`, run:

```bash
cd ~/src/4-play
sudo python3 deploy/systemd/enable-game-imports.py --dry-run
sudo python3 deploy/systemd/enable-game-imports.py
```

The helper reads the existing private EnvironmentFile and MAME paths without
printing bearer secrets. It backs up that environment, copies the current catalog
to `~/.local/share/4play/catalog.json` if needed, and points both services there.
The original catalog is preserved. This prevents future imports from modifying
a Git-tracked example catalog. It creates private staging and the first sample
directory if missing, but will not take ownership of existing directories or
replace a differing private catalog. ROMs and MAME configuration are unchanged.

`chdman` is required for uploaded CHDs. The reference host already has it; other
Debian installations can install the MAME tools package if needed:

```bash
sudo apt-get install mame-tools
```

**End active games before restarting the host agent**, which supervises them:

```bash
sudo systemctl restart 4play-control-plane.service
sudo systemctl restart 4play-runtime-host-agent.service
```

Reload `/admin`. New imports thereafter do not require service restarts: the
updated agent notices atomic manifest changes on heartbeat, verifies the full
manifest, and publishes its catalog. Verification failure retains the previous
catalog. Existing metadata overrides are preserved.

## Try the Fighters folder

Source: `C:\Users\blake\Downloads\Fighters`.

1. Choose that folder in **Add a game** (or choose individual archives).
2. Choose `aof` in the MAME short-name field/list and click **Inspect dependencies**.
   The reference MAME identifies the Neo Geo BIOS automatically.
3. Click **Upload matching files & audit**. Only matching game/dependency files
   are uploaded; unrelated games, media/images and DAT files are ignored.
4. Read the audit. If a dependency is missing, select its file alongside the
   existing selection and retry. Complete chunks are retained for resumption.
5. Once ready, click **Install verified game** and confirm. The game appears
   after the host's next successful catalog refresh. Admin refreshes again after
   a short delay; V2 browsing also refreshes automatically.

`aof.zip` plus this folder's `neogeo.zip` passed an isolated real-MAME audit
and the complete Linux HTTP upload, audit, sandbox installation and automatic
host-agent catalog publication test on 2026-10-15. All 73 control-plane tests,
14 host-agent tests and 7 importer unit tests also passed on Linux. These tests
did **not** install anything into the live library. The folder also
contains `kinst/kinst.chd` and `kinst2/kinst2.chd`; select their game ZIPs and
CHDs together (folder selection preserves folder hints for ambiguous disk names).

Choose the US set where available, then World, to maintain the preferred catalog.
Keep parent/BIOS archives on disk even when only one variant is visible. This
version reviews one game per job, not an unattended whole-folder bulk import.

## Safety, integrity and recovery

- Existing files are reused and never overwritten, including alternate archive
  formats of the same ROM. Repair/replacement of an incompatible installed BIOS
  or ROM remains a separate maintenance operation with backups.
- Archives are never extracted, preventing archive paths from becoming writes.
  Only MAME-reported archive/disk targets are accepted; filenames, UUIDs, upload
  offsets, size bounds and symlink crossings are checked.
- Files upload in 4 MiB chunks, up to 16 GiB/file and a 32 GiB declared staging
  budget; at most 16 unfinished jobs. Reselect the same original files to resume
  after refresh. A different same-sized file can still produce a failed audit;
  discard/re-upload if the original selection changed.
- `-verifyroms` must pass. Newly uploaded CHDs also run `chdman verify`. Delta
  CHDs needing a parent can fail with a parent error; no automatic CHD conversion
  or reconstruction is attempted. Files cannot be added to the catalog on a
  failed audit. MAME games marked preliminary/not working are blocked; imperfect
  emulation is reported as a warning, not silently treated as perfect.
- Missing optional WAV samples are warnings. A supplied sample archive that
  fails its audit blocks install rather than publishing a known-invalid upload.
- Audits/installs run in the background, one heavy job at a time. MAME audits are
  limited to 120 s; CHD checks to 30 minutes. On Linux child audit processes use
  lower CPU priority and memory/CPU/output limits. Hashing/copying can still use
  disk/CPU: prefer quiet maintenance windows, not tournament matches.
- Installation re-audits, publishes new files without overwrite, backs up and
  atomically updates the manifest. A partial install failure may leave newly
  added, unlisted files; retry can reuse byte-identical files. No rollback deletes
  shared ROMs or BIOS files. Keep the source archives until validation finishes.
- Discard removes **staged copies only**, not installed files or source files.
  Job metadata remains for diagnosis. Local browser storage remembers the most
  recent job, not ROM bytes or bearer secrets.
- Admin APIs use the existing privileged bearer boundary. V2 browser-seat cookies
  cannot access imports. Deploy only on the trusted Admin LAN; an untrusted public
  upload endpoint would require separate roles, stronger quotas and isolation.

## Automated checks

```powershell
python tools/test-game-import.py
node --test tools/test-admin-game-import.cjs
cargo test -p control-plane-server -p runtime-host-agent
cargo clippy -p control-plane-server -p runtime-host-agent --all-targets -- -D warnings -A clippy::too_many_arguments
```

The Python unit tests use synthetic metadata/files. For a real Linux HTTP and
host-agent integration using your own known-good Art of Fighting/Neo Geo files:

```bash
python3 tools/test-game-import-live.py \
  --mame /home/blake/src/mame-4play/mame \
  --ini /opt/4play/config/mame \
  --rom /path/to/aof.zip --bios /path/to/neogeo.zip
```

This starts isolated, loopback-only service processes with a temporary database,
ROM tree and MAME INI. It uploads, audits and installs **inside that sandbox**,
checks automatic catalog publication, then stops its processes and removes the
sandbox. No live MAME game is launched and no installed ROM is replaced.
