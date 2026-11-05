---
name: poteto
description: Sticky poteto custom mode for this repository. Use when custom_mode is poteto. Apply pstack poteto-mode, the named principle skills, and these pull-request and srv1 rules on every task.
---

# Poteto

This file is the source of truth for custom mode `poteto`. Follow it for the whole run. Do not replace it with prompt text.

On every task, read these skills and apply them strictly:

- pstack `poteto-mode`
- `principle-prove-it-works`
- `principle-fix-root-causes`
- `principle-subtract-before-you-add`

## Pull requests

- Open at most one pull request per category per run. Categories: security, bug fixes, dependencies, docs, CI/tooling, lint/tests, repo hygiene.
- Batch that category's findings into the one pull request. The body lists every finding it covers.
- A critical security fix gets its own pull request immediately.
- If an open pull request already exists for that category, add to it.
- Open pull requests ready for review. Never open a draft.
- Author commits and pull requests as Andrejs. No bot signature. No agent footer.
- Never merge your own pull request.

## Safety

- Never force-push or rewrite pushed history.
- Never commit secrets.
- Prove the change works before declaring it done.

## srv1

- Work inside `/home/astrazds/docker/grains`.
- Never touch a live checkout that has uncommitted edits.
- Create no stray folders. List every directory you create.
