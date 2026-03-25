---
trigger: always_on
description: Keep repository documentation synchronized so project state can be recovered after context compression.
globs:
---

**RULE: DOCUMENTATION-FIRST CONTEXT RECOVERY**

For any non-trivial task that changes code, architecture, experiments, configuration, data contracts, or operating workflow, treat repository documentation as persistent memory.

**BEFORE STARTING WORK**
1. Read the project documents that are relevant to the current task before making changes.
2. Use documentation to recover the current state of the project, recent decisions, known constraints, and open problems.
3. If documentation and code disagree, do not ignore the mismatch. Call it out and resolve it.

**DURING WORK**
1. Do not let important decisions live only in chat history.
2. When you create or change behavior, also identify which document, roadmap, manifest, runbook, or note must be updated.
3. Prefer explicit artifacts over memory:
   - markdown docs for human-readable state
   - manifests/configs/json for machine-readable state
   - logs/reports for experiment outcomes

**AFTER FINISHING WORK**
1. Update the relevant documentation in the repository before ending the task.
2. Record:
   - what changed
   - why it changed
   - new assumptions or constraints
   - artifact paths, report names, or checkpoint names when relevant
   - next logical steps
3. If the work introduced a new workflow, layer, or operational rule, write it down in a durable place so the next session can resume from files alone.

**DEFAULT BEHAVIOR**
- Repository docs are the continuity layer.
- Chat context is transient.
- The project must remain recoverable after context compression by re-reading the repo.

**EXCEPTIONS**
- For trivial one-shot edits or purely local formatting changes, a doc update is optional.
- For everything else, documentation sync is part of done.
