# Scout free source registry

Reviewed 2026-11-02. Scout uses no paid feed or API key. A source record is a lead, not proof of identity, legal status, or misconduct. The operator must review source policies and contact information before enabling automated collection.

## GLEIF Global LEI Index

GLEIF states that its Global LEI Index and API are free and the data are CC0: [terms of use](https://www.gleif.org/en/meta/lei-data-terms-of-use/), [API](https://www.gleif.org/en/lei-data/gleif-api/). The Index covers entities with LEIs, not every Vietnamese company. `registeredAs` is the register identifier supplied to the LEI issuer; Scout does not treat it as a verified MST. Candidates remain in the `gleif:` namespace.

The built-in query selects legal-address country VN, sorts by LEI, and requests pages of 100. A transaction checkpoints the next page with the outbox; a short page or the cap of 1,000 pages resets the sweep. Mutable datasets may shift between pages, so a sweep is not a snapshot and can miss or repeat records. Validators are cleared between pages; returned pagination links are never followed. This preserves the old source ID for existing operators. The configured maximum is four requests per UTC day, at least six hours apart. A sample request on 2026-11-02 returned HTTP 200 and a reported total of 377, which will change and is not a verified count of Vietnamese companies.

## Wikidata Query Service

[Wikidata structured data](https://www.wikidata.org/wiki/Wikidata:Licensing) is CC0. The [public query service](https://www.mediawiki.org/wiki/Wikidata_query_service/User_Manual) supports SPARQL JSON, requests that clients respect HTTP 429 and Retry-After, and has shared infrastructure limits. Its [User-Agent policy](https://foundation.wikimedia.org/wiki/Policy:Wikimedia_Foundation_User-Agent_Policy/en) requires an informative identifier with operator contact. Set `LANTERN_WIKIDATA_CONTACT` to a real operator contact, such as an appropriate project URL, then explicitly enable `wikidata-vn-business-v1`. The resulting header is `LANTERN-ScoutBot/0.1 (<contact>)`. The project URL `https://github.com/dakiemdarktharr/NoSleepClub_team1avalanche` may be used when it is maintained as a contact point.

The fixed, bounded query selects items directly marked as a business (P31=Q4830453) and with country Vietnam (P17=Q881) and a Vietnamese label; it optionally reads official website (P856) and ticker symbol (P249). It orders results and pages 100 bindings at a time; it resets on a short page or after 1,000 pages. Records may change during a sweep, and multiple values can straddle pages. A page deduplicates by QID and retains one sorted value; subsequent observations remain candidates rather than confirmed facts. Results do not include an inferred MST, and links are recorded but never crawled. Candidates live in the `wikidata:` namespace and are never matched by name to GLEIF or other companies. Community-contributed data can be wrong, incomplete or stale; P17 and P31 claims are not independent verification. A fixed-query HTTPS test returned HTTP 200 with 100 bindings on 2026-11-02. A broader subclass query returned HTTP 502 and is not used. Network conditions may differ in the running environment.

The source is disabled on first registration; its own origin has a four-request daily cap, six-hour minimum interval, bounded bytes and timeout, and independent backoff. No remote pagination URLs or user-provided SPARQL are executed. For high-volume needs, consider provider data dumps and separately review operational terms rather than increasing this cap without review. The public service is migrating backends, so endpoint and rate-limit guidance should be rechecked.

## Other sources

Vietnamese official registries and exchanges may be useful, but no broad crawling or automated reuse is enabled without confirming each source's access terms, licensing, privacy requirements, and rate limits. A public webpage alone is not evidence of permission for bulk collection. No paid feeds are configured.
