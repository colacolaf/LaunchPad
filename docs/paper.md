# The Research Layer (B) — simulator → paper

The simulator isn't a toy — it's an instrument. This doc defines the experiments and the paper.

## Candidate experiments (pick 2–3, preregister, run, report honestly)

1. **Latency effects on spread.** Vary simulator latency; measure equilibrium spread / bid-ask bounce. Does faster matching tighten spreads? By how much? (Directly enabled by ABIDES-style pairwise latencies.)
2. **Market impact of order size.** How does impact scale with order size vs. book depth? (This is the experiment ABIDES itself validated — a good replication target.)
3. **Agent behavior.** Naive agents (random / trend-following) vs. market-making agents — who earns, who loses, and why. Ties to Barber & Odean's finding that ~97% of persistent retail day traders lose money.
4. **Maker/taker fee effects on liquidity provision.** Vary fee structure; measure spread and depth.

## Method (honest by construction)

- **Preregister** each experiment before running: hypothesis, setup, metrics, stopping rule.
- **Reproducible:** fixed seed, scripted harness, config recorded. Re-running gives the same result.
- **Include negative results.** An experiment that finds "no effect" or "unexpected result" is published too — that's the rare, honest signal.
- **No leakage, no survivorship bias, transaction costs included** where relevant.
- Every result ships with its methodology (hardware, config, data, seed).

## Data grounding (optional)

Free tiers if real-market grounding is needed:
- **SEC EDGAR XBRL APIs** — free, no key (verified).
- **FRED** — free economic data API.
- **yfinance** — free market data (check ToS).

## Paper outline (Phase 7 draft)

```
Title: [working] "Building the Market: A Matching Engine, Agent-Based Simulator,
        and the Experiments They Enable"

1. Introduction — why build the machinery instead of trusting black boxes
2. The Exchange Core — architecture, correctness invariants, honest benchmarks
3. The Simulator — design, agent model, latency model, validation
4. Experiments — the 2–3 preregistered studies, including negative results
5. Discussion — what the results say, limitations, honesty about uncertainty
6. Reproducibility — code + data pointers (this repo)
```

## Publishing route

- Draft → high-school journals/competitions (verify current cycles at Phase 7; most publish seasonally).
- Feeds the team competition report and the internship portfolio.
- Never present the paper as finished before it exists (planned ≠ completed).

## Status

- [ ] Experiments preregistered (Phase 5)
- [ ] 2+ experiments run with reproducible results (Phase 5)
- [ ] Paper draft started, negative results included (Phase 7)
