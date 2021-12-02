# {{ title }} on {{ date }}

## Notes

> This is where your notes will go!

## TODOs

{% for todo in open_todos -%}
{{ todo | trim }}

{% endfor %}

