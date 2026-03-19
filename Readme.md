# quickstart

1. write the default configuration file
   `rudicom write-config /tmp/config.toml`
2. edit that config file, pay special attention to the last entry `storage_path`. That is where the files will be stored. Don't forget to create that directory.
3. import some data
   `rudicom --config /tmp/config.toml --file /tmp/db.index import --mode import "/data/**/*.ima"`
   - make sure the storage path from the config above exists as well as the `/tmp/db.index` from here.
   - add `--echo` if you want to see feedback from the import
   - the import mode `import` won't touch or own the original data, so you won't have to worry about them
4. run the server
   `rudicom --config /tmp/config.toml --file /tmp/db_index server`
   - this defaults to http://localhost:3000
5. access the HTML "landing page" via http://localhost/html/studies

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
- /:table/:id/col/:name (GET,POST,DELETE)
- /instances/:id/file (GET)
- /instances/:id/png (GET)
- /instances/:id/json-ext (GET)

## /tools
### /backup
generates SureQL snapshot of the database
### /{import,move,store} (POST)
`curl http://localhost:3000/tools/{import,move,store}[?<parameters>] -d"<glob>"`
- `echo=true` generate output for successfully registered or stored files (default:false)
- `echo_existing=true` generate output for already existing (and thus ignored) files (default:false)

#### Modes
- `import` Won't touch or own the file, but register it in the DB.
- `store`  Won't touch the original file but create an owned copy inside the configured storage path (which might collide with the source file).
- `move`  The DB takes ownership of the existing file. If the source is outside the configured storage path it will be moved into it.

### Json feedback
Force json formatted feedback by adding header to the request `Content-Type: application/json`  

## offline import
    rudicom --file /tmp/db import "<glob>"
