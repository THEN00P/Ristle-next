# Plugin host: the connection lifecycle

Design review from 2026-10-30, at `main` 2bbe435 (after #37). No code has
changed yet. This document asks whether the way `ristle-plugin-host` ties a
call to its Provider Connection's generation needs a different design before
the rest of M5 (`live`, `queue-projection`, `accounts`, `connection.setting`)
builds on it.

## 1. Why now

Codex reviewed #33, #34 and #35 and left 26 inline findings (the last two
on #34 were fixed in #37). 17 of them
are about one mechanism: what happens to a call, and to what it touches, when
its connection's generation changes or ends, or when the Plugin is retired. In
the text below that mechanism is the *lifecycle*. Each of those findings was
fixed on its own: a lock, a counter, a set, a spawned task or a re-check.
Each fix was right, and the tests prove each one. But when the same mechanism
keeps producing findings, the problem is more than a run of separate bugs.

The other 9 findings are unrelated to the lifecycle and are left out:
- result and error bounds (#33);
- the table-element and growth-rollback limits (#33);
- trial instantiation, closed by "no import works outside a call" (#33);
- optional export types (#33);
- idle-instance eviction (#33);
- the credential-key collision (#34);
- response sequence timing (#34).

## 2. The findings as a set

Three invariants cover all 17 lifecycle findings.

**I1. Containment.** A call is admitted under one *epoch*: its connection's
generation together with the Plugin's code version. Every effect of the call
happens while that epoch is current, or not at all. Effects include every read
of host state (connection info, grants, credential, data, `storage`), every
write (data, credential, cache, `storage`, the instance pool), and every
outside action (requests, socket I/O, file reads). The check that authorizes
an effect and the effect itself must be atomic with respect to the epoch
ending. Any I/O still running when the epoch ends is stopped.

**I2. Clean admission.** No call is admitted to an epoch while anything an
earlier epoch of the same connection id left behind is still reachable: data,
cache entries, the Credential Record, instances. A cleanup that fails keeps
the connection closed until a retry succeeds. This holds whether the earlier
epoch ended by a new generation or by removal.

**I3. Durable, ordered transitions.** Once a transition (new generation,
removal, retirement) has begun, it runs to completion or to a recorded
failure, whatever its caller does. A connection's transitions happen one at a
time, each through the end of its cleanup.

| PR | Finding (Codex title, shortened) | Invariant | How it was broken |
|---|---|---|---|
| #34 | Prevent retired requests from repopulating the HTTP cache | I1 | Running effect not stopped: a slow response wrote the cache after cleanup |
| #34 | Include Plugin retirement in the cache commit gate | I1 | Missing dimension: the gate checked the generation but not the code version |
| #34 | Bind the Server Grant lookup to the call's generation (and `account`, `server`, `folders`) | I1 | Check, then act: the grant was read without the epoch |
| #34 | Make instance recycling atomic with generation retirement | I1 | Check, then act: the pool took back an instance after cleanup emptied it |
| #34 | Pin the generation across all batches of a Tracks request | I1 | Spanning: one operation of several calls straddled two epochs |
| #34 | Check the connection generation inside storage commits (fixed in #37) | I1 | Missing dimension: `storage` checked only the code version under the lock |
| #35 | Recheck the Folder Grant inside the commit gate | I1 | Check, then act: grant check outside the gate |
| #35 | Cancel active socket I/O when its connection is retired | I1 | Running effect not stopped: socket I/O kept going after the epoch ended |
| #35 | Release the socket quota slot when closing the stream | I1 (resources) | A resource the host closed kept its quota until the guest dropped the handle |
| #34 | Complete generation cleanup before admitting new calls | I2 | The new epoch was published before cleanup |
| #34 | Keep failed generation cleanup pending until a successful retry | I2 | A failed cleanup still opened admission |
| #34 | Attempt credential deletion even when other sign-out cleanup fails | I2 | A failed step skipped revoking the credential |
| #34 | Retain failed removal cleanup before allowing re-registration | I2 | Removal forgot its own unfinished cleanup |
| #34 | Serialize cleanup operations for the same connection | I3 (→ I2) | Two transitions overlapped, and one released the other's barrier |
| #34 | Make generation cleanup survive cancellation of its caller | I3 | Dropping `set_connection` stopped the cleanup half-way |
| #34 | Make sign-out cleanup survive caller cancellation | I3 | The same for `remove_connection` |
| #34 | Include Plugin identity in connection-data namespaces | (I2, partly) | Two Plugins' anonymous connections shared one namespace, so one Plugin's removal cleaned the other's data |

The I1 findings come in four shapes, plus the quota finding, which concerns
resource ownership. Each shape will come back with every new import that has
effects:

- **Check, then act** (3): a liveness or grant check, then the effect, with no
  lock covering both.
- **Missing dimension** (2): a check covered the generation or the code
  version, not both.
- **Running effect not stopped** (2): something already in flight when the
  epoch ended kept going.
- **Spanning** (1): an operation of several calls had no single epoch.

The I2 and I3 findings all come from one fact: a transition is a multi-step
procedure (publish, retire, clean up, revoke, admit) spread over a registry
method, a spawned task, a mutex and a set. Every way to interleave or
interrupt those steps was a separate finding.

## 3. The current model

### State that must agree

For one Plugin there are 13 pieces of state, guarded by 4 kinds of locks:

| # | State | Where | Guards |
|---|---|---|---|
| 1 | `Entry.info` (generation, account, grants) | `Connections.entries` (`Mutex<HashMap>`) | the epoch, the grants |
| 2 | `Entry.cleanup` (`Done`/`Running`/`Failed`) | same | I2 admission |
| 3 | `Entry.credential` (cached record, version) | same | credential compare-and-set |
| 4 | `unfinished` (`HashSet`, tombstones) | `Connections` | I2 after a failed removal |
| 5 | `retirements` (`AtomicU64`) | `Connections` | the code-version half of the epoch |
| 6 | `commits` (`RwLock<()>`, one per Plugin) | `Connections` | I1 atomicity |
| 7 | `ready` (`Notify`) | `Connections` | waking admission |
| 8 | `changed` (`watch`) | `Connections` | stopping socket I/O |
| 9 | `transitions` (`tokio::Mutex`, one per Plugin) | `runtime.rs` | I3 ordering |
| 10 | the spawned transition task | `set_connection`, `remove_connection` | I3 durability |
| 11 | pool tags `(generation, retirements)` | `Instance.served` | I1 for instances |
| 12 | `CallContext` (connection, generation, retirements, `credential_base`) | each store | the call's epoch |
| 13 | the copied tags in `TcpHandle`, `GenerationGate`, `pin()` | sockets, cache, `tracks` | the epoch outliving the call frame |

The epoch is not a value anywhere. It is the pair `(generation, retirements)`,
copied into four places and compared field by field. The generation comes
from the caller (`ConnectionInfo.generation`).

### What an import has to remember

An import author has to follow these rules today. Nothing enforces them
except review:

1. Start with `live_call()`, and treat `None` as retired with the right
   per-import default (`none`, empty, `unauthorized`, or do nothing).
2. Read connection info only through `info_at` or `commit_with`, never the
   plain registry.
3. Run every effect inside `commit`, `commit_with` or `commit_current` with
   the call's three values, and run the authorizing check *inside* the
   closure.
4. Race long-running I/O against `ended(…)` and close on loss.
5. After any blocking wait, check `retired()` again before returning data.
6. For a resource that outlives the call, store the tag, re-validate it on
   every use, close it on mismatch, and tie its quota to the open state
   rather than to the handle.
7. For an operation of several calls, `pin()` before and after.
8. Never hold the commit lock across an `.await`. Mind the lock order
   (`transitions` → `commits` → `entries`).

The services an import needs (`store`, `vault`, `net`, `folders`) are
reachable from `HostState` directly. Nothing stops an import from skipping
rules 1–3.

Today: 13 `live_call()` sites, 5 bare `retired()` re-checks, 15 gated reads
and writes (`commit`, `commit_with`, `commit_current`, `info_at`,
`credential`, `write_credential`), 2 `ended()` races, 1 `pin()`.

### Where the remaining M5 imports would have to remember it all again

- **`live` and `queue-projection`.** Host-owned WebSocket channels outlive
  instances and calls. They must close at every transition (rules 4 and 6),
  frames must not reach a projection of a new epoch, and after a generation
  change the host must restart the projection under the new epoch. That makes
  a transition with a follow-up action, which the current model has no place
  for: it would be one more task subscribing to `changed`.
- **`accounts`.** `begin-authorization` and `continue-authorization` run
  under a *candidate* credential that belongs to no generation, with no cache
  and no connection data. Accepting the candidate starts a transition from a
  call's result. The current model has no state for "a call that is not in any
  generation". It would get a fifth flag next to `cleanup`, `unfinished`,
  `retirements` and `served`.
- **`connection.setting`.** Another info read, under rule 2.
- **Byte Sources and Stream Sources.** Long reads from `ristle-media` cross
  transitions, the same shape as sockets (rule 4).

### Gaps found while writing this

These came up while reading the code for this review. None has a test yet.

- **Requests in flight are not stopped.** `fetch` and `fetch-all` block on
  `http::send` without racing `ended()`. Their cache writes and results are
  discarded, but a request still connecting, or following a redirect, when
  the epoch ends is still sent, with the old `attach` rules. `host.wit` says
  that from the moment of a change a retired call "can reach nothing outside
  its own instance". This is the socket finding again, for HTTP.
- **`http.rate-limit` checks, then acts.** It checks `retired()` and then
  writes the host-wide limit outside the gate. This is minor: the limit is not
  account data, but it is an effect after the epoch.
- **The epoch is ABA-prone.** If a connection id is removed and registered
  again with the *same* generation number, a call admitted before the removal
  passes every check against the new registration. It could then write
  connection data or the credential over the new account. `ristle-storage`
  prevents this by keeping a removed connection's generation counter
  (`repo/plugins.rs`), so the app never reuses a number. But that is a
  contract the host does not state and does not enforce, and the host's own
  tests do reuse numbers.
- **Transitions block a runtime worker.** `Connections::set`, `remove` and
  `retire` take the `std::sync::RwLock` for writing from async tasks. They
  wait for in-flight commits, which do SQLite and vault I/O on blocking
  threads. Because the lock is per Plugin, one connection's transition also
  stalls every other connection of that Plugin.

## 4. Options

### A. Keep the current model, add a written checklist and a transition harness

Keep the code. Write rules 1–8 into the crate docs and add a test harness that
ends the epoch at every commit and await of every import (a test hook in
`Connections`). Fix the four gaps above in the current style.

- **Cost:** about 1 day (harness ½ day, gaps ½ day).
- **Risk:** the rules stay conventions. `live` and `accounts` each add new
  kinds of state (a follow-up after transitions, candidate calls), so the list
  grows to 10–11 rules, and the past rate suggests 3–6 more findings per
  import PR. The harness catches only the paths someone thought to put under
  it.
- **For M5:** nothing changes up front. Each remaining import pays the
  lifecycle cost again.

### B. Epoch handles and leases on the current registry

Turn the epoch into a value and make it the only way in:

- **Host-minted epochs.** An `Epoch` (an `Arc`) is created by the host for
  each admission period of a connection and never reused, whatever the
  caller's generation numbers. Retirement ends every current epoch of the
  Plugin and opens new ones for the same generations. There is no second
  counter.
- **Leases.** A call holds a `Lease` on one epoch. The only ways to have an
  effect go through it:
  - `lease.commit(|view| …)` runs the closure under that epoch's read lock and
    hands it the connection's info and grants. Info is reachable only inside a
    commit, so a check and its effect are atomic by construction.
  - `lease.run(future)` races async I/O (fetch, socket, `live`, file reads)
    against the epoch's end and returns `Retired` when the epoch wins.
  - `lease.own(resource)` puts a socket or channel in the epoch's registry.
    The epoch closes it at its end, and its quota slot goes with it.
- **Capabilities, not services.** `Services` becomes private to the lifecycle
  module. An import gets the data store, vault, client and folders only from
  its lease, through methods that already run under `commit` or `run`. An
  import that skips the lease does not compile.
- **Epoch end.** It takes that epoch's write lock (one lock per epoch, not one
  per Plugin), marks it ended and fires its cancellation. When it returns,
  no commit is running or can start, and every leased I/O has been stopped.
- **Instances and multi-call operations.** The pool is keyed by epoch.
  `give_back` is a `lease.commit`, so it fails once the epoch has ended.
  `tracks` takes one lease for all its batches, which replaces `pin()`.

Transitions stay where they are: the spawned task, the `transitions` mutex and
the `unfinished` set.

- **Cost:** about 2½–3 days.
- **Risk:** low to medium. Every I1 shape becomes impossible or a compile
  error, and the ABA gap closes. I2 and I3 still rest on the three cooperating
  mechanisms that produced 7 findings, and `live`'s restart and `accounts`'
  candidate calls still have no home in it.

### C. Per-connection lifecycle actor, with leases (B + an owner for transitions)

Everything in B. In addition, one task per (Plugin, connection id) owns that
connection's state machine and does every transition, cleanup included:

```
Vacant ──register──► Cleaning(target) ──ok──► Open(epoch)
  ▲                      │  ▲                    │
  │                      fail  retry(register)   │ new generation / retire
  │                      ▼  │                    ▼
  └──── cleaned ──── Failed(target)      end epoch ─► Cleaning(target)
                                         remove ────► Removing ─ok─► Vacant
                                                          └ fail ─► Failed(vacant)
```

- **Commands.** `set_connection`, `remove_connection`, `retire_calls` and
  same-generation grant updates are messages on the actor's channel, each
  with a reply channel. The actor handles them one at a time and finishes each
  one whether or not anyone is still waiting for the reply. Ordering (I3) and
  durability (I3) follow from the shape, so the mutex and the spawned tasks
  go away.
- **Failure states.** A failed cleanup or removal is the state
  `Failed(target)`. That is the tombstone, and nothing has to remember to
  check a set. Admission is a lease request: it waits while the state is
  `Cleaning`, fails at once while it is `Failed`, and gets the epoch while it
  is `Open` (I2 by construction).
- **Room for the next imports.** The actor is where a follow-up action
  belongs: after a new epoch opens, it tells the projection supervisor to
  restart `queue-projection` under it. An `accounts` authorization becomes a
  candidate lease, a lease with no data, cache or stored credential. It is
  ended by the acceptance (a `register` command from the call's result) or by
  cancellation.
- **Plugin-wide work.** Retirement is a broadcast to the Plugin's actors.
  Unloading drops the actors after their running transitions finish.
- **Stopping the actor.** A connection with no calls and no transition costs
  one parked task. The actor exits when it reaches `Vacant` with no
  outstanding leases.

- **Cost:** about 4–5 days. That is B's 2½–3, the actor and its state machine
  1–1½, and moving the 32 lifecycle tests plus a transition harness that ends
  the epoch at every lease point ½–1. One PR, or two stacked (leases, then the
  actor).
- **Risk:** medium while it is being built: it replaces code whose behavior
  is pinned by 23 connection tests and 9 files and sockets tests, and all of
  them have to pass unchanged. Afterwards the risk is the lowest of the three:
  each of the 17 findings maps to something the design makes impossible (see
  the next table), and the remaining imports plug into existing points
  instead of adding state.
- **For M5:** about a week of delay before `accounts` and `live`. They are
  the imports that would add the most new lifecycle state, and in C they land
  without any.

### D. Considered as an addition: epoch-keyed persistent state

Key connection data (and the credential) by generation, as `cache.sqlite`
already keys HTTP cache rows. Old state is then unreachable as soon as the
epoch ends, and cleanup becomes garbage collection: admission no longer waits
for it, and a failed cleanup no longer closes the connection. This would
remove I2 at the root. It changes `PluginStore` (fixed in plan §3) and the
`connection_data` schema of `ristle-storage` (a migration in workstream B's
crate), and sign-out still has to revoke the credential promptly for security
reasons. It is worth doing later, but it needs B and F, and C already meets
I2 without it.

### How each option covers the findings

| Finding shape | A (as is + harness) | B (leases) | C (actor + leases) |
|---|---|---|---|
| Check, then act (3) | rule 3, by review | structural (info only inside `commit`) | structural |
| Missing dimension (2) | rule 3, by review | structural (one epoch value) | structural |
| Running effect not stopped (2), HTTP gap | rule 4, by review | structural (`lease.run`) | structural |
| Spanning (1) | `pin()` | one lease | one lease |
| Resource quota on close (1) | rule 6 | `lease.own` | `lease.own` |
| ABA epoch (gap) | open | closed (minted epochs) | closed |
| I2 (4) | 3 mechanisms | 3 mechanisms | states of one machine |
| I3 (3) | mutex + spawn | mutex + spawn | one task, one queue |
| `live` restart, `accounts` candidates | new state each | new state each | existing extension points |

## 5. Recommendation

**C: a lifecycle actor per connection, with leases as the only way to reach
host state.** About **4–5 days**, ideally as two stacked PRs:

1. **Epochs and leases (B):** host-minted epochs, `Lease` with `commit`,
   `run` and `own`, private `Services`, an epoch-keyed instance pool and one
   lease per `tracks`. This PR also fixes the four gaps in §3. All existing
   tests pass unchanged.
2. **The actor:** it replaces the `transitions` mutex, the spawned tasks,
   `Cleanup`, `ready`, `changed` and `unfinished`. It adds the transition
   harness: a test hook that ends the epoch at the Nth lease point and runs
   every import under it.

B alone is the fallback if a week is too much right now. It removes the I1
class, which is most of the findings and every finding that a new import
would add, for about half the cost. The transition machinery would then need
redoing once `live` needs its restart hook.

I recommend against A. 17 findings in four PRs means the cost of the current
design has to be paid with every import. The remaining imports add new kinds
of lifecycle state, so that cost would go up, not down. D is a sensible later
step with B and F once C is in.

**Order for the rest of M5 if C is accepted:** the refactor, then `accounts`
(candidate leases), then `live` and `queue-projection` (projection restart
from the actor), then `connection.setting` and the remaining export wrappers.
The export wrappers go through `call()` and barely notice the change. The
exit tests (Pulley conformance, tamper and takeover, the spike deletions) are
independent of all this.
