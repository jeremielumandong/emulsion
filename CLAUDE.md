# CLAUDE.md
<!-- agentops:workflow-guidelines:begin -->


## Workflow Orchestration

- **Plan proportionally.** A quick change starts with the code, not a spec; plan only for architectural decisions or work that spans several subsystems. If something goes sideways, stop and re-plan instead of pushing on.
- **Parallel by default.** Split independent parts (separate files or subsystems, research vs. implementation, tests vs. docs) across subagents launched in the same turn, within the SUBAGENT POLICY cap; keep one sequential pass only for small, tightly coupled changes. Never end your turn while a subagent is still running.
- **Prove it works.** Run the relevant tests and linters, check logs, and show the evidence before calling a task done.
- **Keep the lesson.** When the user corrects you or confirms a non-obvious approach, record it with the `knowledge_lesson_record` tool — the rule plus a short reason — never for what the repo already records.
- **Simplest correct change.** Fix root causes, no temporary patches; touch only what the task needs; write the report the step asks for and nothing more.
<!-- agentops:workflow-guidelines:end -->
