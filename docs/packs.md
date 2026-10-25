# Writing a test pack

A **pack** is a set of questions that checks how good a model's answers are. Packs are plain data,
so you don't need to know Rust (or any programming language) to write one, and they are never executed
as code.

## Layout

```text
my-pack/
  pack.toml     what the pack is and how answers are scored
  items.jsonl   one question per line (JSON Lines)
```

Put the folder in `packs/` (to contribute it) or `~/.infermark/packs/` (just for you).

## `pack.toml`

```toml
schema = 1
id = "capital-cities"              # lowercase-kebab-case, unique
name = "Capital cities"
version = "1.0.0"                  # bump when you change items
description = "Do models know world capitals?"
category = "knowledge"             # coding, reasoning, knowledge, summarization, creative, …
scorer = "exact_match"             # how answers are graded (see below)
license = "CC-BY-4.0"              # license of the questions
authors = ["Your Name"]
subjective = false                 # true for opinion-based scoring (creative writing)

[scorer_options]
contains = true                    # exact_match: answer only has to contain the expected text

[prompt]
system = "Answer with the city name only."
template = "What is the capital of {{input}}?"

[generation]
max_tokens = 16
temperature = 0.0
```

## `items.jsonl`

```json
{"id": "fr", "input": "France", "expected": "Paris"}
{"id": "au", "input": "Australia", "expected": ["Canberra"]}
```

| Field | Required | Meaning |
| --- | --- | --- |
| `id` | yes | Unique within the pack |
| `input` | yes | Inserted into `{{input}}` in the template |
| `expected` | depends on scorer | A string or a list of accepted answers |
| `choices` | `multiple_choice` | Options, shown as `A) …`, `B) …` via `{{choices}}` |
| `rubric` | `llm_judge` | What a good answer looks like |
| `tests` | `code_exec` | Test code that the answer must pass |
| `tags` | no | Free-form labels |
| `metadata` | no | Anything else (source, difficulty…) |

## Scorers

| Scorer | Status | Passes when |
| --- | --- | --- |
| `exact_match` | available | The normalised answer equals (or with `contains = true`, contains) an expected answer |
| `multiple_choice` | available | The option letter the model picked matches `expected` |
| `regex` | available | Any regular expression in `expected` matches the answer |
| `json_schema` | planned (M3) | The answer is valid JSON matching a schema |
| `code_exec` | planned (M3) | Generated code passes `tests` inside a sandbox |
| `rouge` | planned (M3) | Summary overlaps enough with the reference |
| `llm_judge` | planned (M3) | A judge model scores the answer against the `rubric` |
| `needle` | planned (M3) | A fact hidden in a long context is recalled |

## Check your pack

```bash
cargo run -p infermark-cli -- pack validate path/to/my-pack
```

The validator lists every problem at once (bad ids, duplicate items, missing answers, unknown scorer…).

## Licensing

Only contribute questions you wrote yourself or that come from a source whose license allows
redistribution, and put that license in `pack.toml`.
