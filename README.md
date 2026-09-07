# anonterm

Run a shell inside a pseudo-terminal and have personally identifying
information rewritten before it reaches your screen. Useful when taking
screenshots or recording a terminal session to share.

```
anonterm                      # wraps $SHELL
anonterm -- nvim ~/.zshrc     # wraps a single command
anonterm --list               # show what would be masked
```

Anything that runs in a terminal runs inside anonterm: nvim, htop, tmux,
ssh. Keystrokes are forwarded untouched. Only the program output is filtered.

## What gets masked

By default:

- your login name, hostname, and home directory name
- `user.name` and `user.email` from your global git config
- `Host`, `HostName` and `User` values from `~/.ssh/config`
- email addresses, IPv4 and IPv6 addresses, MAC addresses
- window titles, working-directory reports and hyperlink targets set by
  the shell (OSC 0, 1, 2, 7, 8)
- common secret formats: AWS access keys, GitHub and Slack tokens,
  `sk-` API keys, JWTs, `Bearer` tokens

Literals shorter than three characters are ignored. Loopback addresses are
left readable.

Replacements keep the exact character count and shape of the original, so
column layouts and cursor positioning inside full-screen programs stay
correct. Letters become pronounceable filler, digits stay digits, and
punctuation is kept. The same input always maps to the same output within a
session, so a screenshot stays internally consistent. Set `ANONTERM_SEED` to
get the same replacements across sessions.

## Configuration

`~/.config/anonterm/config.toml` (or `-c FILE`):

```toml
literals = ["Acme Corp", "project-x"]
patterns = ['\bPROJ-\d+\b']
builtin = true
```

Flags `-l WORD` and `-p REGEX` add to the config, `--no-builtin` disables
the environment-derived and builtin rules.

## Limitations

The filter works on the output byte stream, not on the rendered screen.
Matching runs over runs of text between cursor movements, so a name split by
a colour change is still caught, but a name drawn in fragments by separate
cursor jumps is not. Full-screen programs redraw whole lines when opening or
scrolling a file, which is the common case, but a partially edited line can
leak. Treat anonterm as a guard against accidental leaks, not a security
boundary. Check the result before sharing.

Replacement preserves character count, not display width. A double-width
character replaced by a single-width letter shifts the rest of that line.

## Build

```
cargo build --release
```
