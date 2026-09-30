# Repository rulesets

The rulesets enforced on this repository, kept here so a change to them is reviewed
alongside the workflows they depend on. GitHub does not read these files. A change
here takes effect only once it is applied:

```sh
# First time: create.
gh api -X POST repos/luminartech/uds_stack/rulesets --input .github/rulesets/main.json

# After that: update in place, by id.
gh api repos/luminartech/uds_stack/rulesets --jq '.[] | "\(.id) \(.name)"'
gh api -X PUT repos/luminartech/uds_stack/rulesets/<id> --input .github/rulesets/main.json
```

## `main.json`

Every change reaches `main` through a reviewed pull request and the merge queue, as one
squashed commit.

- **Squash only.** The squash commit takes the PR title, which PR Title Lint checks, so
  every commit on `main` is conventional. That is what release-plz reads for the
  version bump and the changelogs.
- **One approval, from someone other than the last pusher.** Stale approvals are dismissed
  on push. This applies to the release PR as well: its merge is what publishes.
- **Merge queue.** `ci.yml` runs the long fuzz and Miri on `merge_group`, so those run on
  the exact commit that will land.
- **Required checks.** Every check here must report on `merge_group` as well as on
  `pull_request`, or the queue waits on it until it times out. A job skipped by its `if`
  counts as passing, which is how PR Title Lint, PR Description Lint and Publish Dry Run
  satisfy the queue. `integration_id` 15368 is GitHub Actions, so only a workflow can
  report these checks. Renaming a job renames its check: update this list in the same PR.
  Semver Checks is not required while it is off (see `ci.yml`). Re-enabling it is the
  time to add it.

## `release-tags.json`

Nobody can create, move or delete a `v*` tag. release-plz tags each release on
`uds_on_ip` (see `release-plz.toml`), and a tag over a partial or rewritten release
would misstate what is on crates.io.

It has no bypass while releases are held. Lifting the hold adds the release-plz GitHub
App as the one bypass actor (`{"actor_type": "Integration", "actor_id": <app id>, "bypass_mode": "always"}`),
in the same change that sets `use-release-plz: true`.
