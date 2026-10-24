# Planner golden fixtures

`cases.json` lists workflow inputs, optional registries, routing options, detected-provider sets,
and expected planner JSON. `aurora-core/tests/plan_parity.rs` reads these files as static test
assets; tests do not invoke a fixture generator.

When planner behavior intentionally changes, update the affected expected JSON and manifest in a
reviewable diff. Do not generate expected output from the planner implementation during the test,
which would make the assertion self-referential.
