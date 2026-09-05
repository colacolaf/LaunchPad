# TODO — Phase 0 task 2: §5 domain model

- [ ] `core/src/domain.rs`: constants, ids, Price/Qty (parse + Display), Side, TimeInForce, OrderType (+validity), OrderAction, DomainError, Order
- [ ] Unit tests (~15): parsing acceptance/rejection, zero rejections, TIF validity combos, Display round-trips
- [ ] `lib.rs`: crate docs + `pub mod domain;`, smoke placeholder deleted
- [ ] Docs: TODO §5 ticks, decision-log rows, weekly-log Built line

## Checkpoint (all green before done)
- [ ] `cargo build --workspace`
- [ ] `cargo fmt --all -- --check`
- [ ] `cargo clippy --workspace --all-targets -- -D warnings`
- [ ] `cargo test --workspace` (domain tests pass)
