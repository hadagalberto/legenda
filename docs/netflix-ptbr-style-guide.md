# Netflix Brazilian Portuguese style-guide values used by `legenda`

The `netflix-ptbr` reflow preset and the `lint` command use the **exact published
values** from the official Netflix Partner Help Center pages below. No value is
guessed or inferred. Both pages were fetched and verified against the tables
below on 2026-11-20 (the language guide's changelog also confirms the 2022-12-22
wording "reading speed limits" and the general guide's 2020-07-24 removal of its
timing/frame-gap section).

## Primary source (pt-BR language specific)

**Official page:** <https://partnerhelp.netflixstudios.com/hc/en-us/articles/215600497-Portuguese-Brazil-Timed-Text-Style-Guide>

Values taken verbatim from that page:

| Guide section | Rule | Exact value |
| --- | --- | --- |
| I.2 Character Limitation | "42 characters per line" | `42` chars per line |
| I.11 Line Treatment and Line Breaks | "Maximum two lines." | `2` lines max |
| I.11 Line Treatment and Line Breaks | "Text should usually be kept to one line, unless it exceeds the character limitation." | prefer `1` line |
| I.11 Line Treatment and Line Breaks | "Prefer bottom-heavy pyramid shaped subtitles where possible." | bottom-heavy balance |
| I.11 Line Treatment and Line Breaks | Break **after punctuation marks**, **before conjunctions**, **before prepositions** | break-point preferences |
| I.11 Line Treatment and Line Breaks | Do **not** separate a noun from an article; a noun from an adjective; a first name from a last name; a verb from a subject pronoun; a prepositional verb from its preposition; a verb from an auxiliary, reflexive pronoun or negation | forbidden break-points |
| I.14 Reading Speed Limits | "Adult programs: Up to 17 characters per second" | `17` CPS (adult) |
| I.14 Reading Speed Limits | "Children's programs: Up to 13 characters per second" | `13` CPS (children) |
| II.2 Reading Speed Limits (SDH) | "Adult programs: Up to 20 characters per second"; "Children's programs: Up to 17 characters per second" | `20` / `17` CPS (SDH, quoted for completeness; not used by the default preset) |

## Secondary source (general requirements)

**Official page:** <https://partnerhelp.netflixstudios.com/hc/en-us/articles/215758617-Timed-Text-Style-Guide-General-Requirements>

| Guide section | Rule | Exact value |
| --- | --- | --- |
| 1 Duration | "Minimum duration: 5/6 (five-sixths) of a second per subtitle event (e.g. 20 frames for 24fps)" | `5/6` s min |
| 1 Duration | "Maximum duration: 7 seconds per subtitle event" | `7` s max |
| 4 Line Treatment | "2 lines maximum" and the same break rules as I.11 above | `2` lines max |

## Value added by this project (NOT from Netflix)

The **minimum gap of 2 frames** between subtitle events is a project requirement
from `SPEC.md`, not a published Netflix value (the general guide's former timing/
frame-gap section was removed in its 2020-07-24 changelog entry). It is applied
using the frame rate supplied to the operation (`24` fps by default, matching
the guide's own "20 frames for 24fps" example).

## Notes on CPS counting

CPS is computed as `visible characters / duration in seconds` using exact
rational arithmetic. "Visible characters" means Unicode scalar values after
inline markup (`<i>`, `<font>`, ASS override blocks) and line breaks are
removed; spaces are counted, matching the guide's character-per-second model.
