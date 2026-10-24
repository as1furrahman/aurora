# Run-store fixtures

`run-v1/` is a frozen schema-v1 run record and workflow definition used to test compatibility
with records created before the Rust-only migration. Rust tests read the files directly; no
generator or other language runtime is needed.

The handoff bundle snapshots in `../handoff/` are also static Rust test inputs. Update expected
files only alongside an intentional, reviewable behavior change.
