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
