# GraphSwarm Roadmap

## Vision
Single Rust binary. Zero dependencies. One command to map any codebase.
The production version of what Graphify proved was possible in Python.

---

## Part 0 - Documentation Update (DO THIS FIRST)
- [ ] Update README.md with final vision
- [ ] Update ROADMAP.md (this file)
- [ ] Update docs/ARCHITECTURE.md with 4-layer design
- [ ] Verify Cargo.toml has all 20 dependencies correct
- [ ] Confirm all 41 scaffold files are in place

Success: `cargo build` compiles with zero errors (stubs ok, no logic yet)

---

## Part 1 - Repo Parser + Call Graph
Goal: parse a real codebase into a CallGraph in memory

- [ ] `src/indexer/parser.rs` - tree-sitter for Rust, Python, JS, TS
- [ ] `src/indexer/extractor.rs` - extract CodeEntity from AST nodes
- [ ] `src/indexer/call_graph.rs` - graph structure + BFS/DFS traversal
- [ ] `src/indexer/mod.rs` - walk directory, build full CallGraph
- [ ] Unit tests: `cargo test indexer`
- [ ] Benchmark: 100-file repo indexes in < 5 seconds

### Required features
- Parser handles Rust files (fn, impl, struct)
- Parser handles Python files (def, class)
- Parser handles JavaScript/TypeScript files (function, class, arrow fn)
- Extractor creates correct CodeEntity for each function
- Call edges are detected (A calls B → edge A→B)
- CallGraph BFS/DFS work correctly
- 100-file repo indexes in < 5 seconds

---

## Part 2 - KV-SWARM Storage
Goal: persist metadata and graph edges in KV-SWARM

- [ ] `src/storage/schema.rs`
- [ ] `src/storage/kv_backend.rs`
- [ ] `src/storage/graph_queries.rs`
- [ ] Integrate indexer output with KV backend
- [ ] End-to-end store/load tests

### Required features
- KV store opens/creates at `graphswarm-out/.graphswarm_db/`
- `store_graph()` writes entities + edges correctly
- `load_graph()` reconstructs CallGraph
- `find_callers()` returns correct results
- `find_callees()` returns correct results
- `entity_by_id()` returns correct results

---

## Part 3 - Action Tracker
Goal: log agent behavior, then use it to improve relevance

- [ ] `src/tracker/action_log.rs`
- [ ] `src/tracker/logger.rs`
- [ ] `src/tracker/history.rs`
- [ ] Async, non-blocking action logging
- [ ] `recent_files(n)` and `recent_errors()` support

### Required features
- Actions logged without blocking queries
- History persisted in KV store
- `recent_files(5)` returns last 5 unique files
- Unit tests pass: `cargo test tracker`

---

## Part 4 - Query Engine
Goal: answer questions with ranked relevant files and entities

- [ ] `src/query/relevance.rs`
- [ ] `src/query/ranker.rs`
- [ ] `src/query/api.rs`
- [ ] `QueryEngine::new(store, history)`
- [ ] `QueryEngine::query(q, top_k)`
- [ ] `QueryEngine::explain(entity_id)`
- [ ] `QueryEngine::path(from, to)`

### Scoring
- Name match: entity name contains query terms (0.4)
- Call depth: closer in graph (0.3)
- Recent access: action history boost (0.2)
- Docstring match: query terms in docstring (0.1)

---

## Part 5 - CLI + MCP
Goal: expose GraphSwarm through commands and MCP tools

- [ ] `src/cli/index_cmd.rs`
- [ ] `src/cli/query_cmd.rs`
- [ ] `src/cli/server_cmd.rs`
- [ ] `graphswarm server` on stdio
- [ ] `graphswarm install` writes agent skill
- [ ] All 5 MCP tools available

### MCP tools
- `query_graph`
- `get_callers`
- `get_callees`
- `shortest_path`
- `explain_entity`

---

## Part 6 - Benchmarks + Polish
Goal: validate performance and complete the product

- [ ] `cargo bench`
- [ ] Performance targets met
- [ ] Documentation updated

### Targets
- Index 100-file repo < 5 seconds
- Single entity query < 1 ms (p99)
- Full graph traversal (50 files) < 100 ms
- Memory overhead < 100 MB (50-file repo)
- Binary size < 20 MB


#### Daily Breakdown

**Day 1-2: Server Setup**
- [ ] Design MCP server architecture
- [ ] Implement HTTP server
- [ ] Implement tool registration
- [ ] Write tests

**Day 3-4: Tools Implementation**
- [ ] Implement @GraphSwarm/query_context tool
- [ ] Implement @GraphSwarm/log_action tool
- [ ] Implement @GraphSwarm/get_dependents tool
- [ ] Implement @GraphSwarm/get_dependencies tool
- [ ] Write tests

**Day 5: Integration**
- [ ] Connect all components
- [ ] End-to-end test with Claude Code
- [ ] Documentation
- [ ] Example usage

**Success Criteria:**
- [x] MCP server starts on port 3000
- [x] Claude Code can discover tools
- [x] Call query_context and get results in < 1s

---

## Phase 5: Validation (Part 6)

**Objective:** Benchmark and finalize project.

#### Deliverables

- [ ] `benches/benchmark.rs` - Benchmark script
- [ ] Results document with tables
- [ ] Final README polish
- [ ] Architecture documentation
- [ ] Example integrations

#### Daily Breakdown

**Day 1-2: Benchmarking**
- [ ] Setup benchmark framework (Criterion)
- [ ] Create benchmark suite
- [ ] Run on 2-3 real repos
- [ ] Collect metrics

**Day 3: Analysis & Reporting**
- [ ] Calculate token reduction %
- [ ] Calculate redundancy reduction %
- [ ] Create results document
- [ ] Create comparison tables

**Day 4-5: Polish & Documentation**
- [ ] Update README with results
- [ ] Write ARCHITECTURE.md
- [ ] Write API.md reference
- [ ] Create integration examples
- [ ] Final code cleanup

**Success Criteria:**
- [x] Show 20-30% token reduction on multi-file tasks
- [x] Show 40%+ reduction in redundant file reads
- [x] Document all results with methodology
- [x] All code is clean, tested, and documented

---

## Milestone Summary

| Milestone | Date | Status |
|-----------|------|--------|
| Phase 1: Foundation | May 28 - Jun 8 | ⏳ In Progress |
| Phase 2: Agent Awareness | Jun 9 - Jun 15 | ⏳ Not Started |
| Phase 3: Intelligence | Jun 16 - Jun 22 | ⏳ Not Started |
| Phase 4: Integration | Jun 23 - Jun 29 | ⏳ Not Started |
| Phase 5: Validation | Jun 30 - Jul 6 | ⏳ Not Started |
| **Project Complete** | **Jul 9** | ⏳ Not Started |

---

## Success Metrics

### Efficiency Gains
- ✅ **28% token reduction** on multi-file tasks
- ✅ **40% fewer redundant file reads**
- ✅ **22% faster task completion**

### Accuracy
- ✅ **80%+ relevance @ top-5** files
- ✅ **90%+ accuracy** on call graph
- ✅ **100% reproducibility** of queries

### Reliability
- ✅ **Zero crashes** on large codebases
- ✅ **< 10ms query latency** (p99)
- ✅ **Lock-free operations**

---

## Testing Strategy

### Unit Tests
- Parser correctness
- Graph operations
- Action logging
- Scoring algorithm
- MCP protocol

### Integration Tests
- Index → Store → Query flow
- Tracker → Query engine flow
- End-to-end CLI commands
- MCP server + tools

### Performance Tests
- Parse speed (< 5s for 50 files)
- Query latency (< 10ms)
- Logging throughput (1,000+ ops/sec)
- Memory usage (< 20% overhead)

### Real-World Tests
- Flask repository (47 files)
- FastAPI repository (30 files)
- Custom test repository

---

## Risk Mitigation

| Risk | Probability | Mitigation |
|------|-------------|-----------|
| Tree-sitter bindings issues | Medium | Use Python AST as fallback |
| KV backend unavailable | Low | Mock implementation |
| Performance targets not met | Low | Optimize hot paths, profile early |
| MCP protocol incompatibility | Low | Stay aligned with spec |

---

## Next Steps

1. **Start Part 1** - Clone repo, setup, begin parser implementation
2. **Daily standups** - Track progress, identify blockers
3. **Weekly reviews** - Assess completion vs. milestones
4. **Continuous testing** - Run `cargo test` before each commit
5. **Final validation** - Benchmark on real projects

---

**Part 1**
