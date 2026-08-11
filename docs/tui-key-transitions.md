# TUI Key-Transition Matrix

This matrix defines input ownership for the current TUI. `Esc` backs out one
interaction level. `q` and `Ctrl+C` quit only when content is not owned by text
entry or a confirmation. `Tab` and `Shift+Tab` always return content focus to
the tab bar without changing the current screen mode.

At tab-bar focus, `Esc`, `q`, and `Ctrl+C` quit. At ordinary top-level content,
`Esc` returns to the tab bar while `q` and `Ctrl+C` quit.

| Screen and mode | `Esc` | `q` | `Ctrl+C` |
|---|---|---|---|
| Dashboard | Tab bar | Quit | Quit |
| Repository browser | Tab bar; preserve browser | Quit | Quit |
| Repository path input | Browser | Insert `q` | Consumed |
| Repository namespace input | Browser; cancel operation | Insert `q` | Consumed |
| Repository ownership confirmation | Cancel to browser | Consumed | Consumed |
| Repository namespace confirmation | Cancel to namespace input | Consumed | Consumed |
| Sources list | Tab bar | Quit | Quit |
| Sources browser, unchanged | List; end session | Quit | Quit |
| Sources browser, changed | Pending-change choice | Quit | Quit |
| Sources path input | Browser | Insert `q` | Consumed |
| Sources delete confirmation | Cancel to list | Consumed | Consumed |
| Sources pending-change choice | Continue editing | Consumed | Consumed |
| Sources removal confirmation | Back to pending-change choice | Consumed | Consumed |
| Ignore list | Tab bar | Quit | Quit |
| Ignore pattern input | Cancel to list | Insert `q` | Consumed |
| Ignore preview | List | Quit | Quit |
| Backup Preview | Tab bar | Quit | Quit |
| Automation | Tab bar | Quit | Quit |
| Automation confirmation | Cancel confirmation | Consumed | Consumed |
| History list | Tab bar | Quit | Quit |
| History log view | History list | Quit | Quit |

The Sources pending-change choice uses `a` to apply, `d` to discard the whole
editing session, and `c` or `Esc` to continue editing. Choosing apply when the
diff removes sources opens the existing `y`/`n` removal confirmation; `n` or
`Esc` cancels only that confirmation and returns to the three-way choice.
