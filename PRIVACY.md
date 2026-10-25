# Privacy

**Short version: InferMark keeps everything on your device. Nothing is sent anywhere unless you
explicitly ask for it.**

## What InferMark stores (locally)

Everything lives in `~/.infermark/` (or `INFERMARK_HOME`):

| Folder | Contents |
| --- | --- |
| `results/` | Benchmark results: hardware model names, scores, timings. |
| `profiles/` | Profiles you created. |
| `packs/` | Test packs you installed. |
| `models/` | Model files you chose to download. |

Results contain **no** usernames, hostnames, IP addresses, serial numbers or file paths. Devices are
identified only by an anonymous hash of the hardware configuration (CPU model, GPU model, RAM size,
OS family) that cannot be traced back to you.

## Network access

InferMark only connects to the internet when you:

- **download a model** (from Hugging Face, or a source you add), or
- **check for updates** (can be turned off), or
- **share a result to the public leaderboard** (future feature, always opt-in; you'll see the exact
  data before anything is uploaded, and local file paths are removed).

There is **no telemetry, analytics, crash reporting or advertising**, and no account is needed.

## LLM-as-judge

Some optional quality tests can use a judge model. By default this is a local model. If you choose
a cloud API instead, the prompts and the tested model's answers are sent to that provider under
their privacy policy, and InferMark tells you before it does this. Your API key is stored in your
operating system's keychain.

## Deleting your data

Delete the `~/.infermark` folder, or individual results from the app.
