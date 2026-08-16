# Contributing

Thanks for taking the time to contribute. This is an educational,
correctness-first trading-engine project; changes must preserve deterministic
behavior and must not add real-money or live-exchange functionality.

## Before opening a pull request

1. Keep changes focused and explain the behavior they change.
2. Add or update tests for every behavior change.
3. Preserve the project invariants described in `AGENTS.md` and the
   architecture documentation.
4. Run the full local verification gate:

   ```bash
   ./scripts/verify_final.sh
   ```

5. Do not include credentials, personal data, generated build output, or
   fabricated benchmark results. Benchmarks must state their environment and
   remain machine-specific.

## Pull request expectations

- Use a concise title that describes the user-visible or correctness impact.
- Include the commands you ran and their results.
- Call out any API, determinism, risk-control, or performance-reporting
  implications.
- Keep experimental optimizations isolated behind feature flags.

By contributing, you agree that your contributions are licensed under this
repository's [MIT License](LICENSE).
