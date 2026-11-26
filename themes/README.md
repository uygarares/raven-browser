# Raven themes

Raven's theme store shows the built-in themes and the ones listed in
`index.json`. To add yours, open a pull request that adds one entry:

```json
{ "id": "my-theme", "name": "My theme", "author": "Your name",
  "colors": { "ink": "#0d0c1a", "ink2": "#14132a", "page": "#111020", "menu": "#19182f",
              "violet": "#8f6cf2", "blue": "#4f7dff", "teal": "#2fd0b8" } }
```

- `ink`: the frame around the page, `ink2`: raised surfaces, `page`: behind pages
  and panels, `menu`: menus. Keep these dark; in the light appearance Raven uses
  its own light surfaces.
- `violet`, `blue`, `teal`: the three lights of the sheen (any colours you like).
