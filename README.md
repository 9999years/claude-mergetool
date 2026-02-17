# claude-mergetool

AI-powered merge conflict resolution using [Claude Code](https://docs.anthropic.com/en/docs/claude-code). When git or jj hits a merge conflict, claude-mergetool launches Claude to read the three versions of the file, resolve the conflict, and write the result — fully automatically.

## Install

```sh
cargo install --path .
```

### Prerequisites

[Claude Code](https://docs.anthropic.com/en/docs/claude-code) (`claude` CLI) must be installed and available in PATH.

## Setup

### jj

Add to `~/.config/jj/config.toml`:

```toml
[merge-tools.claude]
program = "claude-mergetool"
merge-args = ["merge", "$base", "$left", "$right", "-o", "$output", "-p", "$path"]
```

Then resolve conflicts with:

```sh
jj resolve -r <revision> --tool claude
```

### git

Add to `~/.config/git/config` (or `~/.gitconfig`):

```ini
[merge "claude-mergetool"]
    name = claude-mergetool
    driver = claude-mergetool merge --git %O %A %B -s %S -x %X -y %Y -p %P -l %L
```

Then add a `.gitattributes` to your repo (or `~/.config/git/attributes` for global use):

```
* merge=claude-mergetool
```

Conflicts during `git merge`, `git rebase`, `git cherry-pick`, etc. will automatically launch Claude.

## Usage

claude-mergetool is normally invoked by git or jj, but you can also run it directly:

```sh
claude-mergetool merge base.txt left.txt right.txt -o resolved.txt
```

### CLI reference

```
claude-mergetool merge [OPTIONS] <BASE> <LEFT> <RIGHT>

Arguments:
  <BASE>   Base version (common ancestor)
  <LEFT>   Left version (ours / current branch)
  <RIGHT>  Right version (theirs / incoming)

Options:
      --git              Git merge driver mode (writes result to <LEFT>)
  -o, --output <PATH>    Output file path (jj mode)
  -s <LABEL>             Ancestor conflict label
  -x <LABEL>             Left/ours conflict label [default: ours]
  -y <LABEL>             Right/theirs conflict label [default: theirs]
  -p <NAME>              Original file path (for display in prompts) [default: "unknown file"]
  -l <SIZE>              Conflict marker size
```

## How it works

claude-mergetool runs `claude` in non-interactive mode (`--print`) with `--permission-mode=acceptEdits`, so tool calls (Read, Edit, Write) are auto-approved with no user interaction required. Claude's reasoning and tool calls are streamed to stderr as dimmed text so you can follow along. When Claude finishes, the merge continues automatically.
