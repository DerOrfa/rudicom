# run the server

    rudicom --database "ws://localhost:8000"  server
or (only with feature `embedded`)

    rudicom --file /tmp/db server 

# interfaces

## /html

### /studies
`http://localhost:3000/html/studies[?<parameters>]`
- `sort_by=<field>` where `<field>` can be any column in the table
- `sort_reverse=true` reverse sorting
- `filter` simple substring filter for study name (first column)

`http://localhost:3000/html/studies/<uid>`

## /api
- /info (GET)
- /statistics (GET)
- /instances (POST)
- /:table (GET)
- /:table/:id (GET,DELETE)
- /:table/:id/instances (GET)
- /studies/:id/series (GET)
- /:table/:id/parents (GET)
- /:table/:id/verify (GET)
- /instances/:id/file (GET)
- /instances/:id/png (GET)
- /instances/:id/json-ext (GET)

## /tools
### /import (POST)
`curl http://localhost:3000/tools/import/{text,json}[?<parameters>] -d"<glob>"`
- `echo=true` generate output for successfully registered or stored files (default:false)
- `echo_existing=true` generate output for already existing (and thus ignored) files (default:false)
- `store=true` store (aka copy files into storage) instead of just importing the existing files

## offline import
    rudicom --file /tmp/db import "<glob>"
