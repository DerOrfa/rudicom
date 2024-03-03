# run the server

    rudicom --database "ws://localhost:8000"  server
or (only with feature `embedded`)

    rudicom --file /tmp/db server 


# import

## offline
    rudicom --file /tmp/db import "<glob>"

## REST
    curl http://localhost:3000/tools/import/{text,json}[?<parameters>] -d"<glob>"
- `echo` generate output for successfully registered or stored files (default:false)
- `echo_existing` generate output for already existing (and thus ignored) files (default:false)
- `store` store (aka copy files into storage) instead of just importing the existing files
