# neutrino
Virtualized development environments for agents

## Goal
Increase developer confidence in letting claude work unsupervised for long periods of time.

## Strategy
Rather than aim for a perfectly hermetically sealed secure enclave for development, the virtualized environment simply reduces access to just what claude needs for development.

1. VMs that are completely recreatable from a well-defined config.
2. Explicitly provided tools, zero access to bash.
3. Access granted via tightly-scoped, revocable access tokens.
4. Human IDE access over ssh

## Buy vs Build
I have yet to find an off-the-shelf offering that achieves the same goal. There are lots of tools out there to help secure a local agent, but nothing that orchestrates them all into a usable development flow.
