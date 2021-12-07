# Journal v2 in Rust

## Usage


To create an initial configuration that stores files in `where/to/store`

```sh
journal init [--path where/to/store]
```

To create a new entry
```sh
journal new "This is the title"
```

Configuration can be placed anywhere and referenced with `JOURNAL_CONFIG` which defaults to `$HOME/journal.config.yml`.

The content of the config should look like this:

```
dir: "/Users/$your-name/journal"

pull_requests:
  auth:
    personal_access_token: "$your-github-access-token"
  select:
    - repo: felipesere/sane-flags
    - org: vanilla-project
      authors:
        - christop
        - felipe
    - org: vanilla-project
      labels:
        - dependencies

```

`dir` is the important key, as it tells `journal` where to store files.

You can also adjust that value on each call using `JOURNAL_DIR=/different/location`.


## Working with `TODOs`

## Working with `Github`

## Working with Reminders

You can have `journal` remind you of events:

| Example usage                            | Meaning                         |
| ---------------------------------------- | ------------------------------- |
| `--on $WEEKDAY` like `--on Monday` ...   | On next `$WEEKDAY`, e.g. Monday |
| `--on 15.Jan` or  `--on 15.Jan.2022` ... | On that specific day            |
| `--every $WEEKDAY` like `--every Monday` | Every `$WEEKDAY`                |
| `--every 2.days` or `--every 3.weeks`    | Repeat every `n` interval       |

Possible modifier flags?

* `--max 10.times` useful for `--every` to give it a natural "end" time
* `--skip-on-weekends` when an event would fall on Saturday/Sunday then simply drop it
* `--before-weeknd` and `--after-weekend` in case an event falls on a Saturday/Sunday, move it to the next best Monday/Tuesday
