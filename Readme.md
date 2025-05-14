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
- /:table/:id/filepath (GET)
- /instances/:id/file (GET)
- /instances/:id/png (GET)
- /instances/:id/json-ext (GET)

## /tools
### /backup
generates SureQL snapshot of the database
### /{import,move,store} (POST)
`curl http://localhost:3000/tools/{import,move,store}/{text,json}[?<parameters>] -d"<glob>"`
- `echo=true` generate output for successfully registered or stored files (default:false)
- `echo_existing=true` generate output for already existing (and thus ignored) files (default:false)

#### Modes
- `import` Won't touch or own the file, but register it in the DB.
- `store`  Won't touch the original file but create an owned copy inside the configured storage path (which might collide with the source file).
- `move`  The DB takes ownership of the existing file. If the source is outside the configured storage path it will be moved into it.
  

## offline import
    rudicom --file /tmp/db import "<glob>"
