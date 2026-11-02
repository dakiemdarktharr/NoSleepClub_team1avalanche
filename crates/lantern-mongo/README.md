# lantern-mongo

Optional storage adapter for **independent, unsharded** MongoDB clusters. It is
not a drop-in replacement for `lantern-core`, an active ingestion service, a
SQLite migration, or a production deployment. The server has not switched its
source of truth merely by compiling this crate.

`RoutingTable` maps SHA-256(company ID), first eight bytes big-endian modulo
the fixed number of virtual buckets, to a named cluster. The caller persists an
immutable, trusted copy of the entire table outside all clusters and passes it
to `connect` or `initialize_empty`. `validate_against_pin` rejects a difference.
Each cluster also stores its fingerprint. Restarts with missing or conflicting
cluster metadata fail closed. The application is responsible for protecting the
external pin and ensuring no other application bypasses it. Do not alter bucket
count, owners, algorithm, or version without a separately designed, durable,
fenced migration. **Bucket migration and SQLite migration are not implemented.**
There is no automatic failover to another cluster, fallback data source, or
cross-cluster transaction.

The explicit first-use `initialize_empty` requires a previously secured pin,
fresh dedicated databases and exclusive provisioning: no concurrent writers.
It checks for existing adapter records and incompatible metadata before writing;
retries with the same pin can finish a partial initialization. These checks are
not a distributed lock. `connect` only accepts already initialized metadata.
Ensure operator-provisioned collections are **unsharded**: this adapter relies
on `_id` uniqueness on each cluster and does not verify sharding topology.
Use narrowly scoped database credentials in named environment variables; never
pass a URI as an environment variable *name*. Driver error details are suppressed
because they can contain connection secrets. Database/index provisioning is
performed only on explicit method calls; no Atlas cluster is created here.

Company, snapshot, claim and job records have explicit company/entity/version
identity. The same immutable version and JSON payload can be retried, while a
different payload for that key returns `Conflict`. `put` success includes a
read-back; timeout or lost response means the write outcome is unknown and the
caller must retry the **same** record, not mint a new version. Every connection
overrides the URI's write concern to request `majority`; a failed or timed-out
write acknowledgement cannot be converted to success by a subsequent read.
Concurrent duplicate-key races also return an error for retry. Majority does not
replace backup, correct replica configuration, or tested failover and recovery.
Source timestamps, provenance, user review and proper payload schema remain
responsibilities of the integrating application.

`list_company` is bounded and reports `truncated`. `list_clusters` requires an
explicit scope and reports each failed or truncated cluster separately;
`is_complete` is false for either condition. It is not a paginated export and
does not promise a point-in-time cross-cluster snapshot. When a required cluster
fails, callers must not treat the partial response as a complete business result.
Health is a ping, not proof of rights, correctness, consistency or capacity.
Time limits apply to each driver operation; a multi-cluster operation can take
multiple such intervals.

Before any real migration, design an independently reviewed protocol covering
consistent SQLite snapshots, event/version mapping, schema validation,
reconciliation, idempotent resumption, write freeze/fencing, backup, cutover and
rollback. Switching a mutable bucket map or performing ad-hoc dual writes can
lose data and is deliberately unsupported.
