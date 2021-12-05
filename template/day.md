# {{ title }} on {{ date }}

## Notes

> This is where your notes will go!

## TODOs

{% for todo in open_todos -%}
{{ todo | trim }}

{% endfor %}

## Pull Requests:

{% for pr in prs -%}
* [ ] {{pr.title}} on [{{pr.repo}}]({{pr.url}}) by {{pr.author}}
{% endfor %}
