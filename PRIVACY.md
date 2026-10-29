# Privacy

**Short version: InferMark keeps everything on your device. Nothing is sent anywhere unless you
explicitly ask for it.**

## What InferMark stores (locally)

Everything lives in `~/.infermark/` (or `INFERMARK_HOME`):

| Folder | Contents |
| --- | --- |
| `results/` | Benchmark results: hardware model names, scores, timings, and the sensor readings taken during the run (CPU/GPU load, clocks, temperatures, battery charge and drain). |
| `profiles/` | Profiles you created. |
| `packs/` | Test packs you installed. |
| `settings.toml` | Your settings (storage folder, Ollama address). |
| `models/` | Model files you chose to download. |
| `engines/` | The llama.cpp build InferMark installed for you. |

`models/` and `engines/` can be moved to another drive with the **storage folder** setting.

On Android the same folders are inside the app's private storage, which Android deletes when the
app is uninstalled. The app reads the chip and phone model names (`Build.SOC_MODEL`,
`Build.MODEL`) to name the hardware, the same way the CPU model is used on a computer; it does not
read the serial number, IMEI, phone number, accounts or location.

Results contain **no** usernames, hostnames, IP addresses, serial numbers or file paths. Devices are
identified only by an anonymous hash of the hardware configuration (CPU model, GPU model, RAM size,
OS family) that cannot be traced back to you. Messages from inference engines are scrubbed of your
home and storage folder paths before they are saved.

## Network access

InferMark only connects to the internet when you:

- **download a model** (from Hugging Face) or **install the llama.cpp engine** (from the official
  GitHub releases). Downloads use HTTPS only, every redirect must stay on an allow-listed host,
  and each file must match its published SHA-256 before it is used, or
- **check for updates** (can be turned off), or
- **share a result to the public leaderboard** (future feature, always opt-in; you'll see the exact
  data before anything is uploaded, and local file paths are removed).

There is **no telemetry, analytics, crash reporting or advertising**, and no account is needed.
Sensor readings shown during a run go only to the app's own screen and the local result file.

While a benchmark runs, InferMark talks to inference engines **on your own computer**: the
llama.cpp server it starts (listening on `127.0.0.1` only, protected by a random per-run key) and,
if you use it, your local Ollama.

## LLM-as-judge

Some optional quality tests can use a judge model. By default this is a local model. If you choose
a cloud API instead, the prompts and the tested model's answers are sent to that provider under
their privacy policy, and InferMark tells you before it does this. Your API key is stored in your
operating system's keychain.

## Deleting your data

Delete the `~/.infermark` folder, or individual results from the app.
