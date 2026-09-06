# Course R1–R6 checklist — HearthCoach V0.5.0

| Requirement | Implementation | Status after Windows verification |
|---|---|---|
| R1 Rust core | Harness, Agent, provider orchestration, Task/Session/Usage managers and native UIs are Rust | implemented |
| R2 UI | HDT-style native Control Center + in-game Overlay | implemented |
| R3 model config | Endpoint, Key, model, OpenAI/DeepSeek compatibility, context, thinking, timeout, pricing, budgets in desktop UI | implemented |
| R4 progress/cancel | TaskManager stages + % + elapsed + native Task UI; cancellable async reqwest future | implemented |
| R5 history/context | `AgentSessionArchive` JSON + auto/manual save + list/view/load/restore + Agent trace | implemented |
| R6 usage/cost/budget | exact provider usage per call + configured prices + UI + token/cost hard stop | implemented |

`VERIFY_V0_5_0.ps1` must pass on Windows before this table is treated as final acceptance evidence.
