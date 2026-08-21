# Research: ABIDES

> **Repo:** github.com/abides-sim/abides · **Language:** Python · **Stars:** ~560 · **Paper:** arXiv:1904.12066 · **Verified:** Aug 21, 2026 (GitHub API + arXiv)

## What it is

**ABIDES** = Agent-Based Interactive Discrete Event Simulation environment, built at NYU to support AI agent research in market applications. It's the academic reference for our SIM layer.

## Why it's the bar

- Simulates **tens of thousands of trading agents** interacting with an exchange agent.
- **Configurable pairwise network latencies** between each individual agent and the exchange — the latency model is built in, not bolted on.
- Message-based design **modeled after NASDAQ's published ITCH and OUCH** equity trading protocols.
- Validated in the paper with experiments to develop a **market impact model** — the exact kind of experiment we want to run (see `docs/paper.md`).

## Paper abstract (arXiv:1904.12066, verbatim key points)

> ABIDES is designed from the ground up to support AI agent research in market applications. While simulations are certainly available within trading firms for their own internal use, there are no broadly available high-fidelity market simulation environments... ABIDES currently enables the simulation of tens of thousands of trading agents interacting with an exchange agent to facilitate transactions. It supports configurable pairwise network latencies between each individual agent as well as the exchange. Our simulator's message-based design is modeled after NASDAQ's published equity trading protocols ITCH and OUCH. We introduce the design of the simulator and illustrate its use and configuration with sample code, validating the environment with example trading scenarios. The utility of ABIDES is illustrated through experiments to develop a market impact model.

## What to steal vs. what to skip

**Steal:**
- **The architecture:** exchange agent + many trading agents, message-passing between them.
- **The pairwise latency model:** every agent↔exchange pair gets a configurable latency — this is what makes latency experiments possible.
- **ITCH/OUCH-style messaging:** the sim talks to our CORE over a machine protocol modeled on real exchange feeds.
- **Experiment methodology:** how they designed and validated the market-impact experiment (reproducible, configurable, seeded).

**Skip:**
- The whole codebase — we build our own sim in Python, informed by their design.
- Anything that assumes a particular market structure we don't need for our experiments.

## Relevance to our phases

- **Phase 5 (Simulator):** ABIDES is the direct model. We don't need to match their scale on day one (their "tens of thousands" is our 50k+ stretch), but the architecture should be the same shape.
- **Phase 8 (Paper):** the market-impact experiment they ran is one of our candidate experiments — we can replicate the methodology on our own simulator.

## Source

- README (fetched from `raw.githubusercontent.com/abides-sim/abides/master/README.md`, Aug 21, 2026)
- arXiv:1904.12066 abstract (fetched from arxiv.org, Aug 21, 2026)
- GitHub API repo metadata (Aug 21, 2026)
