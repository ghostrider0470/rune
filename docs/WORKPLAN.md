# Overnight Workplan

Goal for morning: have a planning package that is materially closer to implementation, while preserving these hard constraints:

- functionally identical to OpenClaw
- fully Azure compatible
- Docker-friendly with mountable persistent filesystem
- Rust-first runtime and extension path

## Deliverables to prepare overnight

1. `docs/CRATE-LAYOUT.md`
   - Rust workspace/crate breakdown
   - ownership boundaries
   - dependency direction rules

2. `docs/PROTOCOLS.md`
   - internal runtime contracts
   - gateway HTTP/WS surface outline
   - tool execution contract
   - session/event model

3. `docs/PARITY-CONTRACTS.md`
   - subsystem-level parity contracts
   - invariants, required persisted state, failure expectations
   - minimum evidence needed before claiming parity

4. `docs/AZURE-COMPATIBILITY.md`
   - Azure OpenAI / Foundry requirements
   - Document Intelligence integration
   - deployment/auth/config considerations
   - Azure-specific parity constraints
   - Azure data platform options (Cosmos DB, Azure Database family, storage services)

5. `docs/DOCKER-DEPLOYMENT.md`
   - container topology
   - mount strategy
   - persistent vs ephemeral state
   - backup/restore expectations

6. `docs/COMPETITIVE-RESEARCH.md`
   - relevant Rust/open-source projects
   - especially zeroclaw
   - what to borrow / what to avoid

7. `docs/IMPLEMENTATION-PHASES.md`
   - strict delivery phases
   - parity-first milestone sequence

8. tighten existing docs
   - `PLAN.md`
   - `STACK.md`
   - `DATABASES.md`
   - `PARITY-SPEC.md`

## Scope boundaries

- no coding yet
- no repo scaffolding yet
- no speculative feature expansion beyond OpenClaw parity unless clearly marked optional

## Priority order

1. full OpenClaw parity inventory
2. parity and protocol definitions
3. crate architecture
4. Azure compatibility
5. Docker/mounted storage model
6. ecosystem/competitive research
7. implementation sequencing
