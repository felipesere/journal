# {title} on {today}

## Notes

> This is where your notes will go!

## TODOs
{{ for todo in todos }}
{ todo | trim}
{{ endfor }}

{{ if prs }}
## Pull Requests:
{{ for pr in prs }}
* [ ] {pr.title-} on [{pr.repo-}]({pr.url-}) by { pr.author -}
{{ endfor }}
{{ endif }}
{{ if tasks }}
## Open tasks
{{ for task in tasks }}
* [ ] {task.summary-} [here]({task.href-})
{{ endfor }}

{{ endif }}
{{ if  reminders }}
## Your reminders for today:

{{ for reminder in reminders -}}
* [ ] { reminder }
{{ endfor }}
{{ endif }}
