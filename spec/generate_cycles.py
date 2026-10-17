#!/usr/bin/env python3
"""Writes spec/cycles/*.json: one release + hotfix cycle per combination of
the .gflow/config keys. replay.html draws them. Edit this file, not the JSON:
python3 spec/generate_cycles.py"""
import itertools
import json
from pathlib import Path

OUT = Path(__file__).parent / "cycles"
MAIN = "main"


def br(name, frm): return {"op": "branch", "name": name, "from": frm}
def start(name, frm): return {"op": "start", "name": name, "from": frm}
def dl(name): return {"op": "delete", "name": name}
def tag(name, on): return {"op": "tag", "name": name, "on": on}
def done(n): return {"op": "mergedPr", "pr": n}
def pr(n, frm, into, method): return {"op": "openPr", "pr": n, "from": frm, "into": into, "method": method}


def frame(cmd, text, ops, flag=None):
    return {"cmd": cmd, "text": text, "ops": ops, **({"flag": flag} if flag else {})}


def cm(branch, message, version=None):
    return {"op": "commit", "branch": branch, "message": message, **({"version": version} if version else {})}


def sq(frm, into, message, version=None):
    return {"op": "squash", "from": frm, "into": into, "message": message, **({"version": version} if version else {})}


def mg(frm, into, message, version=None):
    return {"op": "merge", "from": frm, "into": into, "message": message, **({"version": version} if version else {})}


class Cycle:
    def __init__(self, mode, strategy, keep):
        self.mode, self.strategy, self.keep = mode, strategy, keep
        self.protected = mode == "protected"
        self.actions, self.pr_no = [], 0

    def act(self, phase, actor, *frames):
        self.actions.append({"phase": phase, "actor": actor, "frames": [frame(*f) for f in frames]})

    def next_pr(self):
        self.pr_no += 1
        return self.pr_no

    def pr_cycle(self, phase, branch, target, title, open_text):
        n = self.next_pr()
        self.act(phase, "gflow", ("gflow finish" + (" --base develop --breaking=false" if target == "develop" else ""),
                                  f"gflow pushes {branch} and opens PR #{n} into {target}. {open_text}", [pr(n, branch, target, "squash")]))
        self.act(phase, "reviewer", (f"Squash and merge PR #{n}", f"The reviewer squash-merges PR #{n} into {target}.",
                                     [sq(branch, target, f"{title} (#{n})"), done(n)]))
        self.act(phase, "gflow", ("gflow finish   # re-run", f"gflow sees the merged PR and deletes {branch} locally and on origin.", [dl(branch)]))

    def work(self, phase, name):
        b = f"feature/{name}"
        self.act(phase, "gflow", (f"gflow start feature --name {name} --no-worktree", f"gflow creates {b} from develop and pushes it.", [br(b, "develop")]))
        self.act(phase, "dev", (f'git commit -m "feat: {name}"', f"The developer commits one file on {b}.", [cm(b, f"feat: {name}")]))
        self.pr_cycle(phase, b, "develop", f"feat: {name}", "It asks for a SQUASH merge.")

    def fix(self, phase, kind, parent, name, create=True):
        b = f"{kind}/{parent.split('/')[1]}/{name}"
        if create:
            self.act(phase, "gflow", (f"gflow start {kind} --name {name} --no-worktree", f"gflow creates {b} from {parent}.", [br(b, parent)]))
        self.act(phase, "dev", (f'git commit -m "fix: {name}"', f"The developer commits the fix on {b}.", [cm(b, f"fix: {name}")]))
        self.pr_cycle(phase, b, parent, f"fix: {name}", "It asks for a SQUASH merge.")

    def version_change(self, phase, cmd, version, target, after_merge=None):
        """A version commit on a branch that already exists on origin."""
        if not self.protected:
            self.act(phase, "gflow", (f"{cmd}   # same call", f"Free mode: gflow runs the version script on {target}, commits VERSION {version} and pushes {target}.",
                                      [cm(target, f"chore: set version {version}", version)]))
            return False
        vb = f"version/{version}-into-{target.replace('/', '-')}"
        n = self.next_pr()
        self.act(phase, "gflow", (f"{cmd}   # same call", f"{target} is protected, so the version change needs a PR. gflow makes {vb} from {target}, commits VERSION {version}, opens PR #{n} into {target}, turns on auto-complete with SQUASH and exits.{after_merge or ''}",
                                  [br(vb, target), cm(vb, f"chore: set version {version}", version), pr(n, vb, target, "squash")]))
        self.act(phase, "platform", (f"auto-complete PR #{n}", f"The required checks pass. The platform squash-merges PR #{n} and deletes {vb}. {target} now has VERSION {version}.",
                                     [sq(vb, target, f"chore: set version {version} (#{n})", version), done(n), dl(vb)]))
        return True

    def land(self, phase, cmd, source, target, title, conflict_keep=None, tag_after=None):
        """One leg of a release/hotfix finish: source reaches target. cmd is
        the gflow call that starts the leg."""
        if not self.protected:
            ops = [mg(source, target, f"{title}", conflict_keep)]
            if conflict_keep:
                self.act(phase, "gflow", (cmd, f"gflow merges {source} into {target}. VERSION conflicts. gflow stops mid-merge on {target}.", [], "conflict"))
                self.act(phase, "dev", (f"git checkout --theirs VERSION && git commit --no-edit && git switch {source}",
                                        f"The developer keeps {target}'s VERSION {conflict_keep} and switches back to {source}.", ops))
                return
            text = f"gflow merges {source} into {target} with a merge commit"
            if tag_after:
                ops.append(tag(tag_after, target))
                text += f", tags it {tag_after}"
            self.act(phase, "gflow", (cmd, text + " and pushes.", ops))
            return
        landing = f"landing/{source.replace('/', '-')}-into-{target.replace('/', '-')}"
        ops = [br(landing, source)]
        if conflict_keep:
            self.act(phase, "gflow", (cmd, f"gflow creates {landing} and merges {target} into it. VERSION conflicts. gflow stops mid-merge.", ops, "conflict"))
            self.act(phase, "dev", ("git checkout --theirs VERSION && git commit --no-edit", f"The developer keeps {target}'s VERSION {conflict_keep}.",
                                    [mg(target, landing, f"Merge {target} into {landing}", conflict_keep)]))
            ops = []
        n = self.next_pr()
        self.act(phase, "gflow", ("gflow finish   # re-run" if conflict_keep else cmd, f"gflow pushes {landing} and opens PR #{n} into {target} (MERGE COMMIT).",
                                  ops + [pr(n, landing, target, "merge")]))
        self.act(phase, "reviewer", (f"Merge PR #{n} (merge commit)", f"The reviewer merges PR #{n} into {target}.",
                                     [mg(landing, target, f"Merge PR #{n}: {title}", conflict_keep), done(n)]))
        if tag_after:
            self.act(phase, "gflow", ("gflow finish   # re-run", f"{target} has landed. gflow tags the merge commit {tag_after}.", [tag(tag_after, target)]))

    def next_leg_cmd(self, main_tag):
        """Free mode runs both legs in one call. Protected mode starts the
        develop leg on the re-run after the main PR merged, which is also
        the call that cut the main tag, if there is one."""
        if not self.protected:
            return "gflow finish   # same call"
        return "gflow finish   # same call" if main_tag else "gflow finish   # re-run"

    def cleanup(self, phase, source, done_text):
        landings = [f"landing/{source.replace('/', '-')}-into-{t}" for t in (MAIN, "develop")] if self.protected else []
        kept = f" It keeps {source} (keep-release-branches=true)." if self.keep else ""
        ops = [dl(b) for b in landings] + ([] if self.keep else [dl(source)])
        gone = ", ".join(landings + ([] if self.keep else [source]))
        deletes = f" gflow deletes {gone}." if gone else ""
        self.act(phase, "gflow", ("gflow finish   # re-run", f"Both legs have landed.{deletes}{kept}", ops, f"done:✔ {done_text}"))

    def build(self):
        rc = self.strategy == "rc"
        release, shipped = "0.1.0", "0.1.0" if rc else "0.1.1"
        hotfix = "0.1.1" if rc else "0.1.2"
        rb, hb = f"release/{release}", f"hotfix/{hotfix}"

        self.act("Reset", "tool", ("resetPlayground()", f"main and develop point at the first commit. It has .gflow/set-version.sh, VERSION 0.0.0 and .gflow/config with mode={self.mode}, bump-strategy={self.strategy} and keep-release-branches={str(self.keep).lower()}.",
                                   [{"op": "init", "version": "0.0.0"}]))
        self.work("Develop work", "login")

        P = "Release 0.1"
        first_tag = "v0.1.0-rc.1" if rc else "v0.1.0"
        self.act(P, "gflow",
                 ("gflow start release --minor --no-worktree", f"gflow cuts {rb} from develop.", [start(rb, "develop")]),
                 ("gflow start release --minor   # same call", f"{rb} is not on origin yet, so no PR is needed. gflow commits VERSION {release} on it, pushes it and tags that commit {first_tag}.",
                  [cm(rb, f"chore: set version {release}", release), tag(first_tag, rb)]))
        self.version_change(P, "gflow start release --minor", "0.2.0", "develop")
        self.fix(P, "release-fix", rb, "crash")
        if rc:
            self.act(P, "gflow", (f"gflow bump   # on {rb}", f"Under rc the version script gets {release} again and changes nothing. gflow tags the tip v0.1.0-rc.2.", [tag("v0.1.0-rc.2", rb)]))
        else:
            self.act(P, "gflow", (f"gflow bump   # on {rb}", f"{rb} has a fix after tag v0.1.0. Under patch the next version is 0.1.1.", []))
            if self.version_change(P, "gflow bump", "0.1.1", rb, " The tag waits for the merge."):
                self.act(P, "gflow", ("gflow bump   # re-run", "gflow sees the version PR merged. It tags the squash commit v0.1.1.", [tag("v0.1.1", rb)]))
            else:
                self.act(P, "gflow", ("gflow bump   # same call", "gflow tags the new commit v0.1.1.", [tag("v0.1.1", rb)]))

        release_tag = f"v{release}" if rc else None
        self.land(P, f"gflow finish   # on {rb}", rb, MAIN, f"release {release}", tag_after=release_tag)
        self.land(P, self.next_leg_cmd(release_tag), rb, "develop", f"release {release}", conflict_keep="0.2.0")
        self.cleanup(P, rb, f"Release {shipped} complete.")

        P = f"Hotfix {hotfix}"
        self.act(P, "gflow",
                 (f"gflow start hotfix-fix --name security --no-worktree   # on {MAIN}", f"The last shipped version is {shipped}. gflow cuts {hb} from {MAIN}.", [start(hb, MAIN)]),
                 ("gflow start hotfix-fix --name security   # same call", f"{hb} is not on origin yet, so no PR is needed. gflow commits VERSION {hotfix} on it and pushes it. No tag yet.",
                  [cm(hb, f"chore: set version {hotfix}", hotfix)]),
                 ("gflow start hotfix-fix --name security   # same call", f"gflow creates hotfix-fix/{hotfix}/security from {hb}.", [br(f"hotfix-fix/{hotfix}/security", hb)]))
        self.fix(P, "hotfix-fix", hb, "security", create=False)
        self.land(P, f"gflow finish   # on {hb}", hb, MAIN, f"hotfix {hotfix}", tag_after=f"v{hotfix}")
        self.land(P, self.next_leg_cmd(f"v{hotfix}"), hb, "develop", f"hotfix {hotfix}", conflict_keep="0.2.0")
        self.cleanup(P, hb, f"Hotfix {hotfix} complete.")

        left = ", ".join([MAIN, "develop"] + ([rb, hb] if self.keep else []))
        tags = (["v0.1.0-rc.1", "v0.1.0-rc.2", "v0.1.0"] if rc else ["v0.1.0", "v0.1.1"]) + [f"v{hotfix}"]
        self.act("Verify", "tool", ("verifyEndState()", f"{MAIN} has VERSION {hotfix} and develop has 0.2.0. Tags: {', '.join(tags)}. No PRs are open. Branches left on origin: {left}.", [], "done:✔ Cycle complete."))
        return self.actions

    def rules(self):
        r = []
        if self.protected:
            r += ["main and develop accept changes only through PRs. gflow never merges a landing PR (landing/*); a reviewer merges it with a MERGE COMMIT.",
                  "A version change on a branch that already exists on origin travels on version/X.Y.Z-into-<target>. gflow opens the PR and turns on auto-complete with SQUASH. Fallbacks: merge at once, then a human merges."]
        else:
            r += ["gflow merges release and hotfix branches into main and develop itself, with --no-ff merge commits, and pushes.",
                  "Every version change is a direct commit on the branch. Free mode makes no version/* branches."]
        r.append("A new release/* or hotfix/* branch gets its version commit before its first push, in both modes.")
        r.append("Tags: rc cuts vX.Y.0-rc.N on the release branch and the clean vX.Y.0 on main at finish."
                 if self.strategy == "rc" else
                 "Tags: patch cuts a clean vX.Y.Z on the release branch at start and at every bump. The last bump tag is the release; finish adds no tag.")
        r.append("A hotfix gets one tag, on its merge into main.")
        r.append("keep-release-branches=true keeps release/* and hotfix/* after finish." if self.keep
                 else "Finish deletes the release or hotfix branch. Its tags keep the commits.")
        return r


def main():
    OUT.mkdir(exist_ok=True)
    files = []
    for mode, strategy, keep in itertools.product(["protected", "free"], ["rc", "patch"], [False, True]):
        c = Cycle(mode, strategy, keep)
        name = f"{mode}-{strategy}-{'keep' if keep else 'delete'}.json"
        spec = {
            "title": f"gflow cycle: {mode}, {strategy}, {'keep' if keep else 'delete'} release branches",
            "settings": {"mode": mode, "bump-strategy": strategy, "keep-release-branches": str(keep).lower()},
            "rules": c.rules(),
            "actions": c.build(),
        }
        (OUT / name).write_text(json.dumps(spec, indent=1, ensure_ascii=False) + "\n")
        files.append(name)
    (OUT / "index.json").write_text(json.dumps({"cycles": files}, indent=1) + "\n")
    print(f"wrote {len(files)} cycles to {OUT}")


if __name__ == "__main__":
    main()
