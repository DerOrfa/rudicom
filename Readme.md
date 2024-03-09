# run the server

    rudicom --database "ws://localhost:8000"  server
or (only with feature `embedded`)

    rudicom --file /tmp/db server 

# html

## studies
    http://localhost:3000/studies/html[?<parameters>]
- `sort_by=<field>` where `<field>` can be any column in the table
- `sort_reverse=true` reverse sorting
- `filter` simple substring filter for study name (first column)

# import

## offline
    rudicom --file /tmp/db import "<glob>"

## REST
    curl http://localhost:3000/tools/import/{text,json}[?<parameters>] -d"<glob>"
- `echo` generate output for successfully registered or stored files (default:false)
- `echo_existing` generate output for already existing (and thus ignored) files (default:false)
- `store` store (aka copy files into storage) instead of just importing the existing files
