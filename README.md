# headroom

`headroom` tells you whether your Mac can take one more worker process. It
samples the host twice, one second apart, then prints a verdict and the numbers
behind it. It uses public macOS APIs and does not write files, make network
requests, or launch other processes.

## Get started

Install the latest release with Homebrew:

```sh
brew install --cask blacktop/tap/headroom
headroom
```

For source builds, tests, and releases, see [Contributing](CONTRIBUTING.md).

The report looks like this:

```text
ADMIT: cpu busy 0.75, load 35.7 on 18 cores, memory free 0.69, thermal nominal, disk free 1024.0 GiB
cpu_busy_ratio: 0.75
load1_per_core: 1.98
load1: 35.7
load5: 9.3
load15: 9.0
ncpu: 18
mem_total_bytes: 137438953472
mem_free_ratio: 0.69
compressor_bytes: 5237522432
swapouts_delta: 0
swap_used_bytes: 788398080
pressure_level: normal
thermal_level: nominal
disk_free_bytes: 1099511627776
```

For scripts and agent orchestrators, request JSON:

```sh
headroom --format json
```

Both formats use the same exit codes:

| Code | Verdict | What to do |
| --- | --- | --- |
| `0` | `ADMIT` | The host is within the configured limits. |
| `1` | `REFUSE` | Wait before starting another worker. |
| `2` | `UNKNOWN` | Check the report or diagnostic message. |

An orchestrator should launch a worker only after exit code `0`.

## Set the limits

By default, `headroom` refuses when CPU use exceeds 90%, thermal pressure
reaches `heavy`, readable memory pressure reaches `warn`, the swapout counter
increases between samples, or the volume holding your home directory has less
than 150 GiB free. If CPU use cannot be measured, it falls back to refusing
when the latest one-minute load exceeds the CPU count.

| Flag | Default | Meaning |
| --- | --- | --- |
| `--format text\|json` | `text` | Choose the report format. |
| `--max-cpu-busy RATIO` | `0.90` | Refuse when the CPU busy ratio exceeds this number. |
| `--max-load-per-core NUMBER` | `1.0` | Load-per-core limit when CPU use is unavailable. |
| `--max-thermal LEVEL` | `heavy` | Refuse at or above this thermal level. |
| `--max-pressure LEVEL` | `warn` | Refuse at or above this memory-pressure level. |
| `--allow-swapping` | Off | Permit an increase in swapouts. |
| `--min-free-disk SIZE` | `150GiB` | Refuse when free disk space is below this size. `0` disables the rule. |
| `--disk-path PATH` | Home directory (`HOME`) | Measure the volume that holds this path. |

Thermal levels are `nominal`, `moderate`, `heavy`, `trapping`, and `sleeping`.
Memory-pressure levels are `normal`, `warn`, and `critical`. The CPU limit must
be between `0` and `1`. The load limit must be a finite, non-negative number.

A size is a whole number of bytes, optionally followed by a binary suffix: `K`,
`M`, `G`, or `T`, each of which may also be written `KiB`, `MiB`, `GiB`, or
`TiB`. So `150G` and `150GiB` both mean 150 × 2^30 bytes. Suffixes are
case-sensitive; decimal forms such as `150GB`, fractions, and spaces are
rejected. Use `--help` for the full command help.

## Read the report

`REFUSE` lists every triggered reason, ordered by thermal pressure, memory
pressure, active swapping, CPU use (or load when the fallback applies), then
free disk space. Missing or invalid required data produces `UNKNOWN`, even when
another refusal reason is known. Required data includes load, CPU count, total
RAM, VM counters, kernel page size, thermal state, and free disk space. A
decreasing swap counter also produces `UNKNOWN`.

Swap usage and memory pressure are optional. If macOS denies either read, its
value is `null`; that alone does not produce `UNKNOWN`. Free disk space becomes
optional in the same way when `--min-free-disk 0` disables its rule.

`disk_free_bytes` is the space available to the current user on the measured
volume, the figure `df` reports as available. It comes from `statfs`, so it
leaves out purgeable space that Finder counts as available. A path that does
not exist or cannot be read makes the value unreadable, and so does an unset or
empty `HOME` when `--disk-path` is not given. An unreadable value produces
`UNKNOWN` while the rule is enabled.

`cpu_busy_ratio` measures user, system, and nice CPU ticks over all ticks in the
one-second interval. Missing ticks or a zero total produce `null` and activate
the load fallback. The text verdict says when that fallback is in use.

JSON is one compact object with `schema: 2`, `verdict`, `reasons`, and the same
evidence fields as the text report. It keeps full numeric precision. Byte
counts are integers, and `swapouts_delta` is a count of pages. The free-memory
ratio adds free, inactive, speculative, and purgeable pages, then divides by
total RAM. It is capped at `1.0` because those categories can overlap.
`disk_free_bytes` is the last field; the fields before it keep their names and
order.

## License

MIT. See [LICENSE](LICENSE).
