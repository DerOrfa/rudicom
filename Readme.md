# run the server

    rudicom --database "ws://localhost:8000"  server
or (only with feature `embedded`)

    rudicom --database "file:///file.db"  server 


# rest interface
## import

    http post http://localhost:3000/tools/import/{text,json}?registered=true&existing=true <glob>
- `registered` generate output for successfully registered files (default:false)
- `existing` generate output for already existing (and thus ignored) files (default:false)
