# Fixtures

`pods.json` was **written from the documented `v1.Pod` schema, not captured from
a server.** The machine this crate was built on has `kubectl` v1.34.1 and no
cluster at all — `kubectl config view -o json` reports `"contexts": null` — so
there was nothing to capture from.

The schema is versioned and stable, which is why it is safe to write against.
It has still not been checked against a live API server, and
`phase-04-kubernetes-backend-freezes-the-trait.md` records that as an open debt
rather than hiding it. Anyone with a cluster should replace this file with real
`kubectl get pods -o json` output and note the server version here.

`kubeconfig-empty.json` **is** captured: it is the verbatim output of
`kubectl config view -o json` on a machine with no contexts, from kubectl
v1.34.1. It matters more than it looks — it is the state a developer sees by
default.
