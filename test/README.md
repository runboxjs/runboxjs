# Test And Benchmark Runner

This folder contains a reproducible runner that executes checks, tests, run smoke test, and benchmarks.

## Usage

```powershell
pwsh ./test/run_suite.ps1
```

## Output

All outputs are written to `.log/` with a timestamped prefix:

- `*-cargo_check.log`
- `*-cargo_check_wasm.log`
- `*-cargo_test.log`
- `*-cargo_run_help.log`
- `*-cargo_bench.log`
- `*-summary.log`

Use the summary file first to identify failing stages, then open the corresponding detailed log.
