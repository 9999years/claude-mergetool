# claude-mergetool

<a href="https://crates.io/crates/claude-mergetool">
<img src="https://img.shields.io/crates/v/claude-mergetool" alt="Crates.io">
</a>
<br>
<a href="https://repology.org/project/claude-mergetool/versions">
<img src="https://repology.org/badge/vertical-allrepos/claude-mergetool.svg?header=" alt="Packaging status">
</a>

Have you ever wished that resolving Git merge conflicts could cost you
real-world dollars? Now it can!

`claude-mergetool` implements AI-powered merge conflict resolution using [Claude Code](https://docs.anthropic.com/en/docs/claude-code).
When `git` or `jj` hits a merge conflict, `claude-mergetool` launches Claude to read the three versions of the file, resolve the conflict, and write the result — fully automatically.

It works surprisingly well, although I feel much more comfortable using it in `jj` where the conflicted files are written to a temporary directory (so the AI doesn't get free write access to my checkout) and I can easily run `jj diff` to inspect how the conflicts were resolved once it finishes.

> [!WARNING]
>
> Note that `claude` is launched with `--permission-mode=acceptEdits` to the various conflicted files.
> With `jj`, these are in a temporary directory, but with `git` I'm not sure.
> There is, as always with AI tools that can touch files on your disk, some risk of unintended changes!

## Install

```sh
cargo install claude-mergetool
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
jj resolve -r REVSET --tool claude
```

### git

Add to `~/.config/git/config` (or `~/.gitconfig`):

```ini
[mergetool "claude"]
    cmd = claude-mergetool merge "$BASE" "$LOCAL" "$REMOTE" -o "$MERGED"
    trustExitCode = true
```

Then resolve conflicts with:

```sh
git mergetool -t claude
```

## Usage

claude-mergetool is normally invoked by git or jj, but you can also run it directly:

```sh
claude-mergetool merge base.txt left.txt right.txt -o resolved.txt
```

### CLI reference

```
Usage: claude-mergetool merge [OPTIONS] <BASE> <LEFT> <RIGHT>

Arguments:
  <BASE>   Base version (common ancestor)
  <LEFT>   Left version (ours / current branch)
  <RIGHT>  Right version (theirs / incoming)

Options:
      --git-merge-driver  Git merge driver mode (writes result to `<left>` path)
  -o, --output <OUTPUT>  Output file path (jj mode)
  -s <ANCESTOR_LABEL>    Ancestor conflict label
  -x <LEFT_LABEL>        Left/ours conflict label [default: ours]
  -y <RIGHT_LABEL>       Right/theirs conflict label [default: theirs]
  -p <FILEPATH>          Original file path [default: "unknown file"]
  -l <MARKER_SIZE>       Conflict marker size
  -h, --help             Print help
```

## How it works

`claude-mergetool` runs `claude` in non-interactive mode (`--print`) with `--permission-mode=acceptEdits`, so tool calls (Read, Edit, Write) are auto-approved with no user interaction required.
Claude's reasoning and tool calls are streamed to stderr as dimmed text so you can follow along.
When Claude finishes, the merge continues automatically.
