# Native dependency sources

- fishhook: Facebook fishhook, BSD license retained in each file; an extra EOF blank line in the header is removed. Upstream file commits: fishhook.c `31e51827675cbdee0a8ed2533d1dd700ea4c8306`, fishhook.h `27dd82687eee18f01801e60205a22a6338851caf`. https://github.com/facebook/fishhook
- Node-API headers: Node.js v26.8.2, copied without modification from the installed official headers. Node-API uses a stable ABI and needs no Node library at build time. Node.js license is included in `node/LICENSE` with one indentation-only whitespace correction. https://github.com/nodejs/node/tree/v26.8.2/src

Only the verified official remote-control module's Security import is rebound. No global interposition is installed.
