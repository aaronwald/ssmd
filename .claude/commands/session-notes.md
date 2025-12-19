# Session Notes Generator

Review the entire conversation history and generate comprehensive implementation notes for today's session.

## Instructions

1. **Determine today's date** in YYYY_MM_DD format
2. **Target file**: `implementation/YYYY_MM_DD.md`
3. **Check if file exists**: If it does, append to it. If not, create it.
4. Update completed TODOs in TODO.md. Make sure ephemeral TodoWrite tasks are persisted.

## Content to Document

Generate a detailed summary including:

### Session Overview
- Date and time range
- Primary objectives/tasks
- Overall outcome

### What Was Accomplished
- List all completed tasks with checkmarks
- Include specific version numbers, IPs, hostnames
- Note any infrastructure changes

### Key Decisions Made
- Technical choices and rationale
- Deferred decisions and why
- Trade-offs considered

### Files Created or Modified
- Organized by category (terraform, ansible, kubernetes, docs, etc.)
- Include file paths
- Brief description of changes

### Commands Executed
- Important commands that were run
- Configuration changes applied
- Deployments performed

### Issues Encountered and Resolved
- Problems that came up
- Root causes identified
- Solutions implemented

### Lessons Learned
- What worked well
- What could be improved
- Tips for future sessions

### Next Steps
- Immediate priorities
- Outstanding tasks
- Recommendations

## Format

Use clean markdown with:
- Clear section headers (##)
- Bullet points for lists
- Code blocks for commands/configs
- Checkboxes for completed items (- [x])
- Timestamps where relevant

## Important

- Be thorough and specific
- Include technical details (IPs, versions, hostnames)
- Reference file paths accurately
- Note any security considerations
- Update the phased_plan.md status if relevant tasks completed

After generating the notes, save them to the appropriate file in the implementation/ directory.
