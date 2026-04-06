# Hydra Network Intelligence: Sequential Implementation Prompts

**Created:** 2026-04-07
**Purpose:** Детальные промпты для последовательной реализации pipeline сетевого интеллекта.
**Target models:** GPT-5.4 (o-series reasoning), Claude Opus-4.6 (extended thinking)

---

## Dependency Graph

```
Prompt 1 (App Attribution / Kotlin+Rust)
    |
    v
Prompt 2 (ConnectionInfo enrichment fields / Rust)
    |
    +---> Prompt 3 (Reverse DNS + WHOIS enrichment service / Rust)
    |
    +---> Prompt 4 (Tracker database + static classifier / Rust)
    |
    v
Prompt 5 (Classification verdict cache + rule engine / Rust)
    |
    v
Prompt 6 (Mobile bridge: enriched snapshots to Flutter / Rust+Dart)
    |
    v
Prompt 7 (Intelligence UI / Flutter)
    |
    v
Prompt 8 (LLM classifier: repurpose hydra-ai / Rust)
    |
    v
Prompt 9 (Classification event log + dataset export / Rust)
```

Prompts 3 and 4 can be executed in parallel after Prompt 2.

---

## Prompt 1: App Attribution via getConnectionOwnerUid

**Model:** GPT-5.4 or Opus-4.6
**Reasoning level:** Medium (low o-series, standard thinking). Straightforward Android API integration, no architectural ambiguity.

```
CONTEXT
=======

Hydra is a Rust+Flutter VPN app for Android. The VPN service runs in
HydraVpnService.kt, captures all device traffic via VpnService, and
routes it through tun2proxy into a local SOCKS5 server (Rust, hydra-core).

Currently, connections are attributed to apps only through a heuristic
for Telegram (IP range matching in socks.rs). For all other traffic,
group_kind="domain" and the originating app is unknown.

Android 10+ provides ConnectivityManager.getConnectionOwnerUid() which,
when called by the active VPN service, returns the UID of the app owning
a TCP/UDP connection given (protocol, local_addr, remote_addr).
PackageManager.getPackagesForUid() then maps UID to package name.

TASK
====

Implement app attribution so every connection passing through the VPN
knows which Android app created it.

Architecture:
1. In HydraVpnService.kt, create a singleton AppResolver that:
   - Accepts (protocol: Int, localAddr: InetSocketAddress, remoteAddr: InetSocketAddress)
   - Calls ConnectivityManager.getConnectionOwnerUid() to get UID
   - Calls PackageManager.getPackagesForUid() + getApplicationLabel() to get (packageName, appLabel)
   - Caches UID -> (packageName, appLabel) in a LRU map (capacity 256)
   - Returns AppInfo(uid: Int, packageName: String?, appLabel: String?)

2. Expose a method callable from Rust via JNI or Flutter platform channel:
   resolveApp(protocol: Int, localIp: String, localPort: Int, remoteIp: String, remotePort: Int) -> JSON string {"uid": N, "package_name": "...", "app_label": "..."}

3. In hydra_mobile/rust/src/api/, create a module app_resolver.rs that:
   - Calls the Kotlin method via flutter_rust_bridge platform channel
   - Provides: pub async fn resolve_app_for_connection(local_addr: &str, local_port: u16, remote_addr: &str, remote_port: u16) -> Option<AppAttribution>
   - AppAttribution { uid: u32, package_name: String, app_label: String }
   - Cache results in a moka cache (already a dependency) keyed by uid, TTL 1 hour

4. In hydra-core/src/lib.rs handle_connection(), after SOCKS5 target is
   parsed, call resolve_app_for_connection using the peer address from the
   accepted TcpStream. Use the result to populate ConnectionGroup with
   group_kind=App, group_key=package_name, app_label, package_name.
   Fall back to the existing domain-group heuristic if resolution fails.

FILES TO READ FIRST
===================
- hydra_mobile/android/app/src/main/kotlin/com/hydra/network/hydra_mobile/HydraVpnService.kt
- hydra_mobile/android/app/src/main/kotlin/com/hydra/network/hydra_mobile/MainActivity.kt
- hydra-core/src/lib.rs (handle_connection function)
- hydra-core/src/connections.rs (ConnectionGroup, best_effort_group_for_target)
- hydra_mobile/rust/src/api/simple.rs (start_hydra_node, SHARED_REGISTRY)
- hydra_mobile/lib/platform/hydra_platform_gateway.dart

FILES TO MODIFY
===============
- HydraVpnService.kt: add AppResolver class
- MainActivity.kt: register platform channel for app resolution
- hydra_mobile/rust/src/api/mod.rs: add app_resolver module
- hydra_mobile/rust/src/api/app_resolver.rs: new file
- hydra-core/src/lib.rs: integrate app resolution into handle_connection
- hydra-core/src/connections.rs: update best_effort_group_for_target to accept optional AppAttribution

CONSTRAINTS
===========
- getConnectionOwnerUid requires API 29+. Guard with Build.VERSION.SDK_INT check.
- The VPN service sees connections from tun2proxy, so the local address in
  the SOCKS5 stream is 127.0.0.1. You need the ORIGINAL source address.
  tun2proxy may not expose this directly. If not feasible through SOCKS5,
  an alternative approach: parse /proc/net/tcp6 to map local port to UID
  (works on Android 10+ if VPN app has NET_ADMIN or reads its own connections).
  Evaluate which approach is viable and implement the working one.
- Do NOT add placeholder/mock implementations. If a path is not technically
  feasible, document why and propose the alternative.
- Delete any dead code you encounter in touched files.
- Add tests for the Rust cache and AppAttribution struct.
```

---

## Prompt 2: Enrich ConnectionInfo with Classification Fields

**Model:** GPT-5.4 or Opus-4.6
**Reasoning level:** Low (minimal o-series, minimal thinking). Purely mechanical struct expansion with no design decisions.

```
CONTEXT
=======

Hydra VPN tracks connections in ConnectionRegistry (hydra-core/src/connections.rs).
We are adding a network intelligence pipeline that classifies each connection.
This prompt adds the data fields needed by the classifier and UI.

TASK
====

Expand ConnectionInfo and ConnectionSnapshot with enrichment and classification fields.

1. Add to ConnectionInfo:

   pub app_uid: Option<u32>,
   pub reverse_dns: Option<String>,        // PTR record result
   pub whois_org: Option<String>,          // WHOIS organization name
   pub whois_asn: Option<u32>,             // Autonomous System Number
   pub whois_country: Option<String>,      // 2-letter country code
   pub classification: Option<ConnectionClassification>,

2. Create ConnectionClassification:

   #[derive(Debug, Clone, Serialize, Deserialize)]
   pub struct ConnectionClassification {
       pub category: TrafficCategory,
       pub confidence: f32,          // 0.0 - 1.0
       pub source: ClassificationSource,
       pub explanation: Option<String>,  // human-readable, for UI
   }

   #[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
   #[serde(rename_all = "snake_case")]
   pub enum TrafficCategory {
       Legitimate,
       Advertising,
       Analytics,
       Telemetry,
       SocialTracking,
       Malware,
       Unknown,
   }

   #[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
   #[serde(rename_all = "snake_case")]
   pub enum ClassificationSource {
       TrackerDb,      // matched Disconnect/DDG list
       RuleCache,      // previously classified, cached verdict
       LlmAnalysis,    // on-device LLM
       UserOverride,   // user manually set
   }

3. Add corresponding fields to ConnectionSnapshot (all as Option<String>
   or flattened primitives for JSON serialization).

4. Add these fields to the snapshot() method serialization.

5. Add to ConnectionRegistry:
   pub fn update_enrichment(&self, id: u64, reverse_dns: Option<String>,
       whois_org: Option<String>, whois_asn: Option<u32>,
       whois_country: Option<String>)
   pub fn update_classification(&self, id: u64, classification: ConnectionClassification)

6. Add to ConnectionStats:
   pub blocked_count: usize,
   pub tracker_count: usize,
   Populate in stats() by counting connections with classification
   where category is Advertising|Analytics|Telemetry|SocialTracking.

7. Update existing tests. Add tests for new methods and serialization.

FILES TO MODIFY
===============
- hydra-core/src/connections.rs

CONSTRAINTS
===========
- All new fields must be Option<T> to maintain backward compatibility.
- Do not break existing register() or snapshot() signatures.
- TrafficCategory and ClassificationSource must derive Serialize+Deserialize
  for JSON persistence and future dataset export.
- Delete the is_telegram field from ConnectionInfo/Snapshot -- it is now
  subsumed by app_uid + package_name based detection. Update
  best_effort_group_for_target accordingly. Also remove is_telegram from
  socks module calls in lib.rs.
```

---

## Prompt 3: Reverse DNS and WHOIS Enrichment Service

**Model:** GPT-5.4 or Opus-4.6
**Reasoning level:** Medium. Async service design with caching, but well-understood patterns.

```
CONTEXT
=======

Hydra VPN (Rust, tokio) needs to enrich network connections with:
- Reverse DNS (PTR record lookup)
- WHOIS/RDAP data (organization, ASN, country)

hickory-resolver is already in hydra-core/Cargo.toml dependencies.
moka (async cache) is available in the workspace.

TASK
====

Create a new module hydra-core/src/enrichment.rs that provides
EnrichmentService with async methods for IP enrichment.

1. EnrichmentService struct:
   - Owns a hickory_resolver::TokioAsyncResolver for DNS
   - Owns a moka::future::Cache<IpAddr, EnrichmentResult> with TTL 24h, max 10_000 entries
   - Constructed via EnrichmentService::new() -> Result<Self>

2. EnrichmentResult:
   pub struct EnrichmentResult {
       pub reverse_dns: Option<String>,
       pub whois_org: Option<String>,
       pub asn: Option<u32>,
       pub country: Option<String>,
   }

3. pub async fn enrich(&self, ip: IpAddr) -> EnrichmentResult
   - Check cache first. On hit, return immediately.
   - Reverse DNS: resolver.reverse_lookup(ip). Extract first PTR name.
   - WHOIS: Use the whois-rust crate (add to Cargo.toml, latest version
     from crates.io -- currently 1.6.x). Look up the IP address.
     Parse the response text for: OrgName/org-name/organisation,
     OriginAS/origin, Country/country fields using simple regex/string parsing.
     Do NOT use a heavy RDAP client.
   - Combine into EnrichmentResult, store in cache, return.
   - Each sub-lookup (DNS, WHOIS) has a 3-second timeout. If one fails,
     the others still proceed.

4. pub async fn enrich_host(&self, host: &str) -> EnrichmentResult
   - If host parses as IpAddr, call enrich(ip).
   - If host is a domain, resolve A record first via resolver, then
     enrich the first IP. Set reverse_dns to the original domain name.

5. Integration point (do NOT integrate yet, just expose):
   - Add EnrichmentService as an Arc<EnrichmentService> field in Socks5Server.
   - Pass it through to handle_connection. After conn_id is assigned, spawn
     a detached task: enrich the target_host, then call
     registry.update_enrichment(conn_id, ...).
   - This means enrichment is non-blocking: the connection proceeds
     immediately, enrichment data arrives async.

FILES TO CREATE
===============
- hydra-core/src/enrichment.rs

FILES TO MODIFY
===============
- hydra-core/Cargo.toml: add whois-rust dependency
- hydra-core/src/lib.rs: add pub mod enrichment; add EnrichmentService
  to Socks5Server::new and handle_connection; spawn enrichment task.

CONSTRAINTS
===========
- WHOIS lookups are slow (1-3 seconds). They MUST NOT block connection
  establishment. Use tokio::spawn for the enrichment task.
- Cache by IP /32 (not /24 for now -- keep it simple).
- Error in WHOIS parsing must never crash. Log warnings, return None fields.
- Add #[cfg(test)] tests using known IPs (8.8.8.8 -> Google).
- whois-rust requires a servers.json. Check crate docs for the embedded
  default or bundling strategy. If it needs a file, embed it as a const.
```

---

## Prompt 4: Tracker Database and Static Classifier

**Model:** Opus-4.6 or GPT-5.4
**Reasoning level:** Medium. Data structure design + efficient domain matching.

```
CONTEXT
=======

Hydra VPN needs to classify connections against known tracker/ad databases.
Two open-source databases will be embedded:
- Disconnect tracking protection list (JSON, ~2500 entries organized by category)
- DuckDuckGo Tracker Radar (JSON, ~80000 rules)

Both are MIT/Apache licensed and publicly available.

TASK
====

Create hydra-core/src/tracker_db.rs with an embedded tracker classification engine.

1. Download and process the databases:
   - Disconnect list: https://raw.githubusercontent.com/nickthecook/disconnect-tracking-protection/main/services.json
     Structure: { "categories": { "Advertising": [...], "Analytics": [...], ... } }
     Each entry maps domain -> company info.
   - DuckDuckGo Tracker Radar: https://github.com/nickthecook/AIO-tracker-list
     OR use the simplified domain list from:
     https://raw.githubusercontent.com/nickthecook/AIO-tracker-list/main/AIO_domains.txt
     One domain per line.

   Create a build script (build.rs) or embed script that:
   - Downloads both lists (or include them as files in hydra-core/data/)
   - Produces a single merged HashMap<String, TrackerEntry> keyed by domain
   - TrackerEntry { company: String, category: TrafficCategory, source_db: &str }
   - Serialize to a compact binary format (bincode or rmp-serde) and
     include_bytes!() it into the binary.

2. TrackerDatabase struct:
   - Loaded once at startup from embedded data
   - pub fn classify_domain(&self, domain: &str) -> Option<TrackerMatch>
   - TrackerMatch { domain_matched: String, company: String, category: TrafficCategory }
   - Matching logic: exact match first, then check parent domains
     (e.g., "pixel.facebook.com" -> check "pixel.facebook.com",
     then "facebook.com", then "com")

3. pub fn classify_ip(&self, ip: &str, reverse_dns: Option<&str>) -> Option<TrackerMatch>
   - If reverse_dns is available, classify the PTR domain
   - Otherwise return None (IPs alone aren't in tracker lists)

4. Statistics:
   - pub fn entry_count(&self) -> usize
   - pub fn categories_summary(&self) -> HashMap<TrafficCategory, usize>

PRACTICAL APPROACH: If downloading and embedding is too complex for the
build step, use a simpler approach:
- Include a curated list of top ~5000 tracker domains as a Rust const array
  or JSON file in hydra-core/data/trackers.json.
- Organize by category: advertising, analytics, social_tracking, telemetry.
- Sources: extract from Disconnect list + common ad/analytics domains
  (doubleclick.net, google-analytics.com, facebook.com/tr, etc.)
- This is a STARTING POINT. The full database integration is a follow-up.

FILES TO CREATE
===============
- hydra-core/src/tracker_db.rs
- hydra-core/data/trackers.json (curated tracker list)

FILES TO MODIFY
===============
- hydra-core/src/lib.rs: add pub mod tracker_db
- hydra-core/Cargo.toml: add serde_json (if not already present)

CONSTRAINTS
===========
- The database must load in < 50ms on a mobile device.
- Domain matching must be O(1) average (HashMap).
- No network requests at runtime for tracker data. Everything embedded.
- The tracker list must be easily updatable: replace the JSON file,
  rebuild. Document the update procedure in a comment at top of tracker_db.rs.
- Write tests: known trackers must match, known legitimate domains must not.
```

---

## Prompt 5: Classification Verdict Cache and Rule Engine

**Model:** Opus-4.6
**Reasoning level:** High (extended thinking). This is the core intelligence logic -- combines multiple signals, manages cache coherence, produces verdicts.

```
CONTEXT
=======

Hydra VPN has three information sources for classifying connections:
1. TrackerDatabase (Prompt 4) -- static lists of known trackers
2. EnrichmentService (Prompt 3) -- reverse DNS, WHOIS org, ASN, country
3. LLM classifier (future, Prompt 8) -- on-device model for unknowns

The classification pipeline must be fast: most connections should get a
verdict from cache or static rules within 1ms.

Architecture (three tiers):
- Tier 1: Cache lookup by (host, app_uid) -> verdict. Sub-millisecond.
- Tier 2: TrackerDB + WHOIS-based rules. 0-100ms (enrichment is async).
- Tier 3: LLM analysis queue (future, stub the interface).

TASK
====

Create hydra-core/src/classifier.rs with the ConnectionClassifier.

1. VerdictCache:
   - moka::sync::Cache<VerdictKey, ConnectionClassification>
   - VerdictKey: (host_or_domain: String, app_uid: Option<u32>)
   - TTL: 12 hours, max 50_000 entries
   - pub fn get(&self, key: &VerdictKey) -> Option<ConnectionClassification>
   - pub fn put(&self, key: VerdictKey, verdict: ConnectionClassification)

2. ConnectionClassifier struct:
   - tracker_db: Arc<TrackerDatabase>
   - verdict_cache: VerdictCache
   - llm_queue: tokio::sync::mpsc::Sender<ClassificationRequest>  // for Tier 3

3. pub fn classify_fast(&self, host: &str, port: u16, app_uid: Option<u32>) -> ClassificationResult
   Synchronous, called in the hot path before connection proceeds:
   - Tier 1: check verdict_cache. If hit, return Cached(classification).
   - Tier 2a: check tracker_db.classify_domain(host). If hit, cache and return.
   - Return Pending -- connection should proceed, async enrichment will complete later.

   enum ClassificationResult {
       Verdict(ConnectionClassification),  // immediate decision
       Pending,                             // proceed, classify async
   }

4. pub async fn classify_enriched(&self, conn_id: u64, host: &str, port: u16,
       app_uid: Option<u32>, enrichment: &EnrichmentResult,
       registry: &ConnectionRegistry)
   Called after EnrichmentService completes:
   - Re-check tracker_db with reverse_dns (may match where raw IP didn't)
   - Apply WHOIS-based heuristic rules:
     - Known ad company ASNs (Google Ads: AS15169 with specific orgs,
       Facebook: AS32934, Amazon Ads: specific subnets)
     - Classification by WHOIS org name keywords: "advertising", "analytics",
       "marketing", "tracker"
     - Country-based risk flags (unusual countries for the app)
   - If still Unknown, send to LLM queue (Tier 3):
     ClassificationRequest { conn_id, host, port, app_uid, enrichment, ... }
   - Update registry.update_classification(conn_id, classification)
   - Update verdict_cache for future connections to same host

5. pub fn stats(&self) -> ClassifierStats
   - cache_size, cache_hit_rate, tracker_hits, llm_pending, rules_applied

6. Integration into handle_connection (hydra-core/src/lib.rs):
   - After target is parsed, call classifier.classify_fast().
   - If verdict is Block (Advertising/Analytics/Telemetry with confidence > 0.8),
     treat as TransportPlan::blocked() (same as policy Block).
   - If Pending or non-blocking category, proceed normally.
   - The enrichment task (from Prompt 3) calls classify_enriched() after
     WHOIS/DNS complete. Late-arriving Block verdicts for ACTIVE
     connections: log them, cache for next time, but do NOT kill the
     active connection (user experience).

7. User override: ConnectionClassifier must respect RoutePolicyAction.
   If user has set a policy (Direct, specific VLESS, etc.) for a group,
   the classifier does NOT override it. Classifier only acts on Auto policy.

FILES TO CREATE
===============
- hydra-core/src/classifier.rs

FILES TO MODIFY
===============
- hydra-core/src/lib.rs: add pub mod classifier; integrate into
  Socks5Server and handle_connection.

CONSTRAINTS
===========
- classify_fast() is synchronous and must complete in < 1ms.
- The LLM queue sender is created here but the receiver (Prompt 8) is
  separate. Use an mpsc channel. Define ClassificationRequest struct.
- ALL classification decisions must be logged (tracing::info with structured
  fields) for future dataset collection.
- The blocking behavior (refusing tracker connections) must be CONFIGURABLE
  via hydra.toml: [intelligence] auto_block_trackers = true/false.
  Default: false (opt-in, so users see the classifications before enabling blocking).
- Write comprehensive tests: known tracker domain -> classified as Advertising,
  unknown domain -> Pending, user policy override -> no classifier action.
```

---

## Prompt 6: Mobile Bridge -- Enriched Snapshots to Flutter

**Model:** GPT-5.4
**Reasoning level:** Low. Mechanical serialization plumbing, no architectural decisions.

```
CONTEXT
=======

Hydra mobile (Flutter + Rust via flutter_rust_bridge) reads connection data
through get_active_connections() in hydra_mobile/rust/src/api/simple.rs,
which serializes ConnectionSnapshot to JSON. Flutter parses this in
MobileStateRepository (hydra_mobile/lib/mvp/mobile_state_repository.dart)
into ConnectionSnapshotModel.

After Prompts 2-5, ConnectionSnapshot now has new fields: app_uid,
reverse_dns, whois_org, whois_asn, whois_country, classification
(category, confidence, source, explanation).

TASK
====

1. Update get_active_connections() in simple.rs to include new fields
   in the JSON serialization.

2. Update ConnectionSnapshotModel in mobile_state_repository.dart to
   parse the new fields:
   - int? appUid
   - String? reverseDns
   - String? whoisOrg
   - int? whoisAsn
   - String? whoisCountry
   - String? classificationCategory  // "advertising", "analytics", etc.
   - double? classificationConfidence
   - String? classificationSource     // "tracker_db", "rule_cache", etc.
   - String? classificationExplanation

3. Update get_connection_stats() to include blocked_count and tracker_count.
   Update ConnectionStatsModel accordingly.

4. Add a new FRB function:
   pub async fn get_classifier_stats() -> anyhow::Result<String>
   Returns JSON with cache_size, cache_hit_rate, tracker_hits, etc.
   from ConnectionClassifier::stats().

5. Add a new FRB function:
   pub async fn set_intelligence_auto_block(enabled: bool) -> anyhow::Result<()>
   Toggles the auto_block_trackers flag at runtime.

FILES TO MODIFY
===============
- hydra_mobile/rust/src/api/simple.rs
- hydra_mobile/lib/mvp/mobile_state_repository.dart

CONSTRAINTS
===========
- Maintain backward compatibility: if a field is null in JSON, Dart model
  handles it gracefully (all new fields are nullable).
- Do not break existing UI. ConnectionsScreen still works with old fields.
- Run `flutter analyze` equivalent checks mentally -- no type errors.
```

---

## Prompt 7: Intelligence Dashboard UI

**Model:** Opus-4.6
**Reasoning level:** Medium-High (extended thinking). UI/UX design decisions, layout architecture, making the intelligence data visually compelling.

```
CONTEXT
=======

Hydra is a Flutter Android app. Current main screen (connect_screen.dart)
shows a VPN toggle, stats grid, route inventory, relay snapshot.
Connections screen (connections_screen.dart) shows grouped connections.

The product is pivoting from "VPN app" to "Personal Network Intelligence."
The UI must make the intelligence data the HERO of the experience.

Current nav tabs: Connect | Connections | Routes | Relay Usage | Settings

TASK
====

Redesign the main screen and connections screen to showcase network
intelligence. The VPN toggle remains but becomes secondary to the
intelligence dashboard.

1. CONNECT SCREEN REDESIGN (connect_screen.dart):
   Replace the current hero card + stats grid with:

   a) Intelligence Summary Card (top):
      - Large number: "X connections analyzed"
      - Row of category pills with counts:
        [ADS 23] [ANALYTICS 15] [TRACKERS 8] [BLOCKED 12] [CLEAN 156]
      - "Bandwidth saved: 45 MB blocked today"
      - Small text: "Powered by on-device AI"

   b) VPN Status (compact, below summary):
      - Horizontal card with toggle button, status text, uptime
      - Not the 152px circle hero -- just a status bar

   c) Top Threats card:
      - Top 3 apps by tracker connections count:
        "Instagram -> 23 tracker connections (Facebook Analytics, DoubleClick)"
        "Chrome -> 15 analytics connections (Google Analytics)"
      - Each row shows app icon placeholder, app name, count, company names
      - Tap to navigate to filtered connections view

   d) Keep Route Inventory and Relay Snapshot cards as-is

2. CONNECTIONS SCREEN ENHANCEMENT (connections_screen.dart):
   Add to each group card:

   a) Classification badge next to the group title:
      Color-coded pill showing dominant category
      (red=Ads, orange=Analytics, yellow=Telemetry, gray=Unknown, green=Clean)

   b) In the expanded connection row, add:
      - Reverse DNS line: "resolves to cdn.facebook.com"
      - WHOIS line: "Facebook Inc. (AS32934, US)"
      - Classification line: "[ANALYTICS] Facebook pixel tracking (confidence: 0.92)"
      - If classification has explanation, show it

   c) Add a filter bar below stats bar:
      Toggleable pills: [All] [Ads] [Analytics] [Trackers] [Clean] [Unknown]
      Filter connections by classification category

3. NAVIGATION UPDATE:
   Rename tabs: Intelligence | Connections | Routes | Relay | Settings
   (The main tab is now "Intelligence", not "Connect")

DESIGN LANGUAGE
===============
- Dark theme (already in use), Material 3
- Classification colors:
  Advertising: #EF4444 (red)
  Analytics: #F97316 (orange)
  Telemetry: #EAB308 (yellow)
  SocialTracking: #8B5CF6 (purple)
  Legitimate: #22C55E (green)
  Unknown: #94A3B8 (gray)
  Malware: #DC2626 (dark red)
- Use text tags in brackets [ADS], [ANALYTICS], etc. per user preference (no emoji icons)

FILES TO MODIFY
===============
- hydra_mobile/lib/screens/connect_screen.dart (major redesign)
- hydra_mobile/lib/screens/connections_screen.dart (enhancements)
- hydra_mobile/lib/main.dart (tab rename)

FILES TO READ FOR CONTEXT
=========================
- hydra_mobile/lib/mvp/mobile_state_repository.dart (data models)
- hydra_mobile/lib/screens/settings_screen.dart (design patterns)

CONSTRAINTS
===========
- VPN toggle must remain accessible and prominent enough. Users still
  need to start/stop VPN easily. Just not the ONLY thing on the screen.
- All new UI elements degrade gracefully when classification data is null
  (show "Analyzing..." or "Classification pending" instead of empty).
- Performance: connections list polls every 2 seconds. The category filter
  must not cause jank. Filter locally, don't re-fetch.
- gIsVpnActive global must still work for the compact VPN status bar.
```

---

## Prompt 8: LLM Classifier -- Repurpose hydra-ai

**Model:** Opus-4.6
**Reasoning level:** High (extended thinking). Prompt engineering for classification, structured output parsing, integration with the pipeline.

```
CONTEXT
=======

Hydra has hydra-ai crate with AiNegotiator that wraps llama-cpp-2 for
GGUF model inference. Currently used for free-text "analyze_connections"
and "analyze_host" functions in simple.rs.

The classifier pipeline (Prompt 5) sends ClassificationRequest to an
mpsc channel when Tier 1 (cache) and Tier 2 (tracker DB + WHOIS rules)
cannot classify a connection.

The LLM must produce STRUCTURED classification output, not free text.

Target model: Qwen 3 0.6B Q4_K_M (~400MB) -- smallest viable model
for structured classification.

TASK
====

1. Create hydra-ai/src/connection_classifier.rs:

   pub struct LlmClassifier {
       infer: Arc<Mutex<Option<Qwen2Infer>>>,
       pending_rx: mpsc::Receiver<ClassificationRequest>,
   }

   pub async fn run(&mut self, registry: Arc<ConnectionRegistry>,
       verdict_cache: Arc<VerdictCache>)
   - Consumes from pending_rx in a loop
   - Batches requests: collect up to 5 requests or wait 2 seconds
   - For each batch, construct a classification prompt (see below)
   - Parse structured JSON output
   - Update registry and verdict_cache with results
   - Log every classification for dataset collection

2. Classification prompt design:

   System prompt:
   "You are a network traffic classifier. For each connection, output
   a JSON object with: category (one of: legitimate, advertising,
   analytics, telemetry, social_tracking, malware, unknown), confidence
   (0.0-1.0), explanation (one sentence in Russian).

   Use these signals: domain name, reverse DNS, WHOIS organization,
   ASN, port number, app name. Known patterns: doubleclick/adsense =
   advertising, google-analytics/firebase = analytics, facebook
   pixel/graph = social_tracking, crash reporters/telemetry endpoints
   = telemetry."

   User prompt (batched):
   "Classify these connections:
   1. host=pixel.facebook.com port=443 app=Instagram reverse_dns=edge-star-shv-01-nrt1.facebook.com whois_org=Facebook whois_asn=32934
   2. host=api.openai.com port=443 app=ChatGPT reverse_dns=api.openai.com whois_org=Microsoft whois_asn=8075
   ...
   Output JSON array:"

   Expected output:
   [{"id":1,"category":"social_tracking","confidence":0.95,"explanation":"Facebook пиксель для отслеживания действий пользователя Instagram"},
    {"id":2,"category":"legitimate","confidence":0.88,"explanation":"API ChatGPT, основной сервис приложения"}]

3. Structured output parsing:
   - Try to parse response as JSON array
   - If parsing fails, try to extract JSON from markdown code blocks
   - If still fails, try line-by-line JSON objects
   - Final fallback: mark all as Unknown with explanation "LLM output parsing failed"
   - Log raw LLM output for debugging/dataset regardless of parse success

4. Rate limiting:
   - Max 1 LLM call per 5 seconds (model is slow, 10-30 tok/s)
   - If queue grows > 100 items, drop oldest items with category=Unknown
   - LLM classification is BEST EFFORT, never blocking

5. Replace existing analyze_connections and analyze_host in simple.rs:
   - analyze_connections: rewrite to use classifier stats + recent
     verdicts, no longer raw LLM call
   - analyze_host: rewrite to return cached classification if available,
     trigger async LLM classification if not

FILES TO CREATE
===============
- hydra-ai/src/connection_classifier.rs

FILES TO MODIFY
===============
- hydra-ai/src/lib.rs: add pub mod connection_classifier, re-export
- hydra_mobile/rust/src/api/simple.rs: rewrite analyze_connections/analyze_host
- hydra-core/src/lib.rs: spawn LlmClassifier::run() task in Socks5Server

CONSTRAINTS
===========
- The LLM is optional. If no model is downloaded, the pipeline works
  with Tier 1+2 only. LlmClassifier::run() exits immediately if
  no model is loaded.
- Prompt must be < 512 tokens total (including batch). 0.6B models
  have limited context. Keep system prompt concise.
- Max generation: 300 tokens per batch response.
- ALL LLM inputs and outputs must be logged with tracing::info for
  future dataset collection. Use structured logging:
  tracing::info!(input = %prompt, output = %response, parse_success = success, "llm_classification");
```

---

## Prompt 9: Classification Event Log and Dataset Export

**Model:** GPT-5.4
**Reasoning level:** Low-Medium. File I/O, JSON lines format, straightforward.

```
CONTEXT
=======

Every classification decision in Hydra (from tracker DB, WHOIS rules,
LLM, or user override) must be persisted as a dataset for:
1. Local model improvement (future fine-tuning)
2. P2P sharing between users (future DHT-based exchange)
3. Aggregate analysis and proprietary model training

TASK
====

Create hydra-core/src/classification_log.rs with ClassificationEventLog.

1. Event structure:

   #[derive(Serialize, Deserialize)]
   pub struct ClassificationEvent {
       pub timestamp: u64,            // unix epoch seconds
       pub host: String,
       pub port: u16,
       pub app_uid: Option<u32>,
       pub package_name: Option<String>,
       pub reverse_dns: Option<String>,
       pub whois_org: Option<String>,
       pub whois_asn: Option<u32>,
       pub whois_country: Option<String>,
       pub category: TrafficCategory,
       pub confidence: f32,
       pub source: ClassificationSource,
       pub explanation: Option<String>,
       pub bytes_total: u64,
       pub duration_ms: u64,
       pub user_approved: Option<bool>,  // filled when user overrides
   }

2. ClassificationEventLog:
   - Appends events to a JSONL file (one JSON object per line):
     {base_dir}/classification_events.jsonl
   - Buffered writer, flush every 30 seconds or on 100 events
   - pub fn log_event(&self, event: ClassificationEvent)
   - pub fn export_dataset(&self) -> Result<Vec<ClassificationEvent>>
     (reads and parses the entire JSONL file)
   - pub fn stats(&self) -> LogStats { total_events, file_size_bytes, oldest_timestamp }

3. Rotation:
   - When file exceeds 10MB, rotate to classification_events.1.jsonl
   - Keep max 3 rotated files (30MB total max)

4. Integration:
   - ClassificationEventLog is created in Socks5Server, passed to classifier
   - Every verdict in ConnectionClassifier (Prompt 5) calls log_event()
   - Every LLM classification (Prompt 8) calls log_event()
   - When user overrides a policy, log_event with user_approved=true/false

5. Mobile API:
   - In simple.rs, add:
     pub async fn get_classification_log_stats() -> Result<String>
     pub async fn export_classification_events(limit: u32) -> Result<String>
     (Returns last N events as JSON array)

FILES TO CREATE
===============
- hydra-core/src/classification_log.rs

FILES TO MODIFY
===============
- hydra-core/src/lib.rs: add pub mod classification_log, integrate
- hydra-core/src/classifier.rs: call log_event on every verdict
- hydra_mobile/rust/src/api/simple.rs: add export functions

CONSTRAINTS
===========
- File writes must NOT block the connection pipeline. Use a channel +
  background writer task.
- JSONL format is deliberate (not a JSON array) -- supports append without
  reading the whole file and is standard for ML datasets.
- Events must be machine-parseable for future ingestion into training
  pipelines. No free-form fields except explanation.
- user_approved field enables supervised learning: when user says "this
  is not a tracker" or "this should be blocked," that's a training signal.
- DO NOT implement P2P sharing yet. Just the local log and export.
  The JSONL format is designed to be shareable later.
```

---

## Execution Notes

### Parallel execution opportunities
- Prompts 3 and 4 are independent and can run simultaneously after Prompt 2
- All other prompts are sequential

### Testing strategy
Each prompt includes its own unit tests. After all prompts are complete,
run the full integration test:
```
cd hydra-core && cargo test
cd hydra_mobile/rust && cargo build --target aarch64-linux-android
```

### Model cost optimization
- Prompts 1, 2, 6: Low reasoning -- use GPT-5.4 mini/fast tier or Claude Sonnet
- Prompts 3, 4: Medium reasoning -- standard GPT-5.4 or Claude Opus with normal thinking
- Prompts 5, 7, 8: High reasoning -- GPT-5.4 with high o-series or Claude Opus with extended thinking
- Prompt 9: Low reasoning -- GPT-5.4 mini/fast tier

### Data flywheel
After Prompt 9, every user session generates classification_events.jsonl.
Future prompts (not in this batch):
- P2P DHT sharing of anonymized classification events
- Fine-tuning pipeline: JSONL -> QLoRA adapter for the 0.6B model
- Community ruleset generation from aggregate events
