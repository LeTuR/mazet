# mazet

Run `az` under several Azure identities at once, chosen by the directory you
are standing in.

The Azure CLI keeps all of its authentication state in one place
(`AZURE_CONFIG_DIR`), so a second `az login` competes with the first: one
active identity at a time, and a service-principal login can displace a user
one. `mazet` gives each named profile its own isolated store and resolves which
profile applies from the current directory — so `az` in `~/work/client-a`
speaks as one identity and `az` in `~/work/client-b` as another, at the same
time, with no re-login between them.

Status: initial scaffold.
